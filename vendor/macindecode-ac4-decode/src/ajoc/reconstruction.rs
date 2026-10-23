//! A-JOC 对象 QMF 矩阵重建。
//!
//! 本模块把 `TS103190-2:v1.3.1:5.7.3.1`–`5.7.3.6` 已实现的四段原语接成
//! 单一帧级事务：差分解码、反量化、dry/wet/pre rolling 插值、pre 矩阵乘法、
//! 去相关和最终对象矩阵乘法。输入与输出仍停在 QMF 域；LFE 插回和终端 QMF
//! 合成属于调用方的下一层。
//!
//! # 对象各自的频带网格
//!
//! `Pseudocode 18` 把 `mtx_pre_param` 写成单一 `[pb]` 维度，但 dry/wet 参数的
//! 频带数明确定义成 `ajoc_num_bands[o]`。共享一个对象的 `pb` 去累加另一个对象
//! 因而没有定义。这里按 `5.7.3.6.2` 的 QMF 子带公式直接计算
//! `D(ts,sb) = |Csub2(ts,sb)^T| * Csub1(ts,sb)`：每个对象先用自己的
//! [`AjocBandMap`] 把当前 `sb` 映回参数带，再加入同一个 pre target。
//!
//! # 事务边界
//!
//! [`AjocWorkspace`] 持有完整候选状态。任一差分、形状、插值、去相关或数值错误
//! 都只会弄脏工作区和调用方的帧内输出，不会提交 [`AjocReconstructionState`]；
//! 成功处理全部对象和时隙后才复制候选状态。CLI 因此可以丢弃失败帧、reset 后从
//! 随机访问点继续，而不会保留半推进的历史。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "全部热路径下标由 128 对象、16 输入、7 decorrelator、3 数据点和 64 子带的固定上界派生；\
              显式循环同时固定 Pseudocode 18 的 dry 后 wet 累加顺序"
)]

use super::bands::{AjocBandMap, NUM_QMF_SUBBANDS};
use super::decorrelator::{DecorrelatorError, DecorrelatorState, kind_for_ajoc_index};
use super::dequant::{DequantError, dequantise};
use super::diff::{DiffError, DiffState, QuantizedObjectMatrix, decode};
use super::interp::{
    CoefficientGroup, CoefficientLayout, InterpolationError, InterpolationSchedule,
    InterpolationState, RollingCoefficient, RollingCoefficients, RollingLayout,
};
use super::syntax::{Ajoc, AjocObjectControl, AjocObjectMatrix};
use super::{MAX_AJOC_BANDS, MAX_AJOC_DMX_SIGNALS, MAX_DATA_POINTS, MAX_DECORRELATORS, MatrixKind};
use crate::aspx::qmf::QmfSlot;
use crate::element_drive::{QmfChannelFrame, empty_channel_frame};
use alloc::{boxed::Box, vec};
use core::fmt;

/// M6 标量重建支持的对象数上界。
///
/// SDA supports up to 128 reconstructed source components. All rolling,
/// differential and matrix buffers follow this bound and stay heap allocated.
pub const MAX_RECONSTRUCTED_OBJECTS: usize = 128;

const OUTPUT_OBJECT_LANES: usize = 2;
const DRY_OBJECT_STRIDE: usize = MAX_AJOC_DMX_SIGNALS * NUM_QMF_SUBBANDS;
const WET_OBJECT_STRIDE: usize = MAX_DECORRELATORS * NUM_QMF_SUBBANDS;
const DRY_ROLLING_LEN: usize = MAX_RECONSTRUCTED_OBJECTS * DRY_OBJECT_STRIDE;
const WET_ROLLING_LEN: usize = MAX_RECONSTRUCTED_OBJECTS * WET_OBJECT_STRIDE;
const PRE_ROLLING_LEN: usize = MAX_DECORRELATORS * MAX_AJOC_DMX_SIGNALS * NUM_QMF_SUBBANDS;

const DRY_TARGET_LEN: usize =
    MAX_DATA_POINTS * MAX_RECONSTRUCTED_OBJECTS * MAX_AJOC_DMX_SIGNALS * MAX_AJOC_BANDS;
const WET_TARGET_LEN: usize =
    MAX_DATA_POINTS * MAX_RECONSTRUCTED_OBJECTS * MAX_DECORRELATORS * MAX_AJOC_BANDS;
const PRE_TARGET_LEN: usize = MAX_DATA_POINTS * PRE_ROLLING_LEN;

/// 会改变跨帧 rolling 与 decorrelator 状态布局的 A-JOC 拓扑。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconstructionShape {
    /// 上混对象数。
    pub objects: u8,
    /// 下混输入数。
    pub num_dmx: u8,
    /// 并行 decorrelator 数。
    pub num_decorr: u8,
}

/// 一条物理 A-JOC substream 的跨帧重建状态。
///
/// rolling、差分与 decorrelator 历史在构造时安全地分配到堆上，应由会话长期持有，
/// 不要在逐帧热路径反复构造。对象带宽网格可以逐帧变化，因为 rolling 系数按 QMF
/// 子带保存；对象/input/decorrelator 拓扑变化则必须先 [`Self::reset`]。
#[derive(Debug, PartialEq)]
pub struct AjocReconstructionState {
    diff: Box<[DiffState]>,
    interpolation: InterpolationState,
    dry: Box<[RollingCoefficient]>,
    wet: Box<[RollingCoefficient]>,
    pre: Box<[RollingCoefficient]>,
    decorrelators: Box<[DecorrelatorState]>,
    shape: Option<ReconstructionShape>,
}

impl Clone for AjocReconstructionState {
    fn clone(&self) -> Self {
        Self {
            diff: self.diff.clone(),
            interpolation: self.interpolation,
            dry: self.dry.clone(),
            wet: self.wet.clone(),
            pre: self.pre.clone(),
            decorrelators: self.decorrelators.clone(),
            shape: self.shape,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        // 五份固定形状堆缓冲逐项复用；帧级候选/提交不能因迁移到堆所有权而变成
        // 每帧重新分配。长度由私有构造器锁定，若内部不变量被破坏则 slice API
        // 明确 panic，而不是静默截断历史。
        self.diff.clone_from_slice(&source.diff);
        self.interpolation = source.interpolation;
        self.dry.clone_from_slice(&source.dry);
        self.wet.clone_from_slice(&source.wet);
        self.pre.clone_from_slice(&source.pre);
        self.decorrelators.clone_from_slice(&source.decorrelators);
        self.shape = source.shape;
    }
}

impl AjocReconstructionState {
    /// 全零差分、插值和去相关历史。
    #[must_use]
    pub fn new() -> Self {
        Self {
            diff: vec![DiffState::new(); MAX_RECONSTRUCTED_OBJECTS].into_boxed_slice(),
            interpolation: InterpolationState::new(),
            dry: vec![RollingCoefficient::ZERO; DRY_ROLLING_LEN].into_boxed_slice(),
            wet: vec![RollingCoefficient::ZERO; WET_ROLLING_LEN].into_boxed_slice(),
            pre: vec![RollingCoefficient::ZERO; PRE_ROLLING_LEN].into_boxed_slice(),
            decorrelators: (0..MAX_DECORRELATORS)
                .map(|_| DecorrelatorState::new())
                .collect::<alloc::vec::Vec<_>>()
                .into_boxed_slice(),
            shape: None,
        }
    }

    /// 丢弃该 substream 的全部差分、ramp 与 decorrelator 历史。
    pub fn reset(&mut self) {
        self.diff.fill(DiffState::new());
        self.interpolation.reset();
        self.dry.fill(RollingCoefficient::ZERO);
        self.wet.fill(RollingCoefficient::ZERO);
        self.pre.fill(RollingCoefficient::ZERO);
        for decorrelator in &mut self.decorrelators {
            decorrelator.reset();
        }
        self.shape = None;
    }

    /// 最近一次成功提交的矩阵拓扑；全新或 reset 后为 `None`。
    #[must_use]
    pub const fn shape(&self) -> Option<ReconstructionShape> {
        self.shape
    }
}

impl Default for AjocReconstructionState {
    fn default() -> Self {
        Self::new()
    }
}

/// 最终对象矩阵的一对 fixed-stride 系数行，按 `[coefficient][object]` 排列。
///
/// rolling 的持久身份仍保持 object-major；这里只在一个时隙、一个对象对的局部
/// 工作区里转成 AoSoA，使热乘加能够连续装载两个对象系数，再精确提升为两个
/// `f64` lane。
#[derive(Debug)]
#[repr(C, align(16))]
struct OutputPairWorkspace {
    dry: [[f32; OUTPUT_OBJECT_LANES]; DRY_OBJECT_STRIDE],
    wet: [[f32; OUTPUT_OBJECT_LANES]; WET_OBJECT_STRIDE],
}

impl OutputPairWorkspace {
    const fn new() -> Self {
        Self {
            dry: [[0.0; OUTPUT_OBJECT_LANES]; DRY_OBJECT_STRIDE],
            wet: [[0.0; OUTPUT_OBJECT_LANES]; WET_OBJECT_STRIDE],
        }
    }

    fn load(
        &mut self,
        dry: [&[RollingCoefficient]; OUTPUT_OBJECT_LANES],
        wet: [&[RollingCoefficient]; OUTPUT_OBJECT_LANES],
        dimensions: FrameDimensions,
    ) {
        let [first_dry, second_dry] = dry;
        let dry_len = dimensions.num_dmx * NUM_QMF_SUBBANDS;
        let dry_rows = self.dry[..dry_len]
            .iter_mut()
            .zip(&first_dry[..dry_len])
            .zip(&second_dry[..dry_len]);
        for ((target, first), second) in dry_rows {
            *target = [first.current(), second.current()];
        }

        let [first_wet, second_wet] = wet;
        let wet_len = dimensions.num_decorr * NUM_QMF_SUBBANDS;
        let wet_rows = self.wet[..wet_len]
            .iter_mut()
            .zip(&first_wet[..wet_len])
            .zip(&second_wet[..wet_len]);
        for ((target, first), second) in wet_rows {
            *target = [first.current(), second.current()];
        }
    }
}

/// A-JOC 帧级候选状态与矩阵工作区。
///
/// 大型候选与 target 缓冲在构造时分配到堆上，同样应由调用方长期复用；逐帧入口
/// 本身不分配。它不包含输入和最终输出帧，后者由 CLI 的 QMF 控制 FIFO/对象缓冲
/// 持有。失败后无需清理，下一次调用会完整覆写候选状态并清空三组 target。
#[derive(Debug)]
pub struct AjocWorkspace {
    candidate: Box<AjocReconstructionState>,
    quantized: Box<[QuantizedObjectMatrix]>,
    dry_targets: Box<[f32]>,
    wet_targets: Box<[f32]>,
    pre_targets: Box<[f32]>,
    u: [QmfSlot; MAX_DECORRELATORS],
    y: [QmfSlot; MAX_DECORRELATORS],
    output_pair: Box<OutputPairWorkspace>,
}

impl AjocWorkspace {
    /// 全零工作区。
    #[must_use]
    pub fn new() -> Self {
        Self {
            candidate: Box::new(AjocReconstructionState::new()),
            quantized: vec![QuantizedObjectMatrix::new(); MAX_RECONSTRUCTED_OBJECTS]
                .into_boxed_slice(),
            dry_targets: vec![0.0; DRY_TARGET_LEN].into_boxed_slice(),
            wet_targets: vec![0.0; WET_TARGET_LEN].into_boxed_slice(),
            pre_targets: vec![0.0; PRE_TARGET_LEN].into_boxed_slice(),
            u: [QmfSlot::zero(); MAX_DECORRELATORS],
            y: [QmfSlot::zero(); MAX_DECORRELATORS],
            output_pair: Box::new(OutputPairWorkspace::new()),
        }
    }

    #[cfg_attr(feature = "ajoc-reconstruction-split-profile", inline(never))]
    fn prepare_frame(&mut self) {
        self.dry_targets.fill(0.0);
        self.wet_targets.fill(0.0);
        self.pre_targets.fill(0.0);
        self.u.fill(QmfSlot::zero());
        self.y.fill(QmfSlot::zero());
    }
}

impl Default for AjocWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

/// 对象矩阵重建失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconstructionError {
    /// A-JOC 固定维度或本阶段的 20 对象边界不成立。
    DimensionOutOfRange {
        objects: u32,
        num_dpoints: u8,
        num_dmx: u8,
        num_decorr: u8,
    },
    /// control/raw/input/output 至少有一个对象或信号切片不足。
    WorkspaceTooSmall {
        objects: usize,
        num_dmx: usize,
        controls: usize,
        raw: usize,
        input: usize,
        output: usize,
    },
    /// 已提交状态遇到不同的对象/input/decorrelator 拓扑。
    ShapeChangeRequiresReset {
        previous: ReconstructionShape,
        current: ReconstructionShape,
    },
    /// active 对象声明了表 78 之外的参数频带数。
    MissingBandMap { object: usize, num_bands: u8 },
    /// 差分输出缺少声明形状内的量化值。
    MissingQuantizedValue {
        kind: MatrixKind,
        object: usize,
        data_point: usize,
        row: usize,
        band: usize,
    },
    /// 差分解码失败。
    Diff(DiffError),
    /// 带对象/数据点上下文的反量化失败。
    Dequant {
        object: usize,
        data_point: usize,
        row: usize,
        band: usize,
        source: DequantError,
    },
    /// 插值日程或时隙无效。
    Interpolation(InterpolationError),
    /// 输入 QMF 含非有限分量。
    NonFiniteInput {
        channel: usize,
        timeslot: usize,
        subband: usize,
    },
    /// 对象求和后的 pre target 非有限。
    NonFinitePreTarget {
        data_point: usize,
        decorrelator: usize,
        channel: usize,
        subband: usize,
    },
    /// rolling 插值产生非有限系数。
    NonFiniteCoefficient {
        group: CoefficientGroup,
        index: usize,
    },
    /// pre 矩阵乘法的 QMF 输出不能表示为 f32。
    NonFiniteDecorrelatorInput {
        decorrelator: usize,
        timeslot: usize,
        subband: usize,
    },
    /// 某一路去相关器失败。
    Decorrelator {
        decorrelator: usize,
        source: DecorrelatorError,
    },
    /// 最终对象矩阵乘法的输出不能表示为 f32。
    NonFiniteOutput {
        object: usize,
        timeslot: usize,
        subband: usize,
    },
}

impl fmt::Display for ReconstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionOutOfRange {
                objects,
                num_dpoints,
                num_dmx,
                num_decorr,
            } => write!(
                f,
                "A-JOC reconstruction dimensions out of range: objects={objects}, dpoints={num_dpoints}, dmx={num_dmx}, decorr={num_decorr}"
            ),
            Self::WorkspaceTooSmall {
                objects,
                num_dmx,
                controls,
                raw,
                input,
                output,
            } => write!(
                f,
                "A-JOC reconstruction requires {objects} objects/{num_dmx} inputs; control/raw/input/output provide {controls}/{raw}/{input}/{output}"
            ),
            Self::ShapeChangeRequiresReset { previous, current } => write!(
                f,
                "A-JOC reconstruction topology changed from {previous:?} to {current:?}; reset is required"
            ),
            Self::MissingBandMap { object, num_bands } => {
                write!(
                    f,
                    "No Table 28 mapping for {num_bands} bands of A-JOC object {object}"
                )
            }
            Self::MissingQuantizedValue {
                kind,
                object,
                data_point,
                row,
                band,
            } => write!(
                f,
                "A-JOC object {object}, data point {data_point}, {kind:?} row {row} lacks quantized band {band}"
            ),
            Self::Diff(error) => write!(f, "{error}"),
            Self::Dequant {
                object,
                data_point,
                row,
                band,
                source,
            } => write!(
                f,
                "Dequantization failed for A-JOC object {object}, data point {data_point}, row {row}, band {band}: {source}"
            ),
            Self::Interpolation(error) => write!(f, "{error}"),
            Self::NonFiniteInput {
                channel,
                timeslot,
                subband,
            } => write!(
                f,
                "A-JOC input {channel}, timeslot {timeslot}, subband {subband} is non-finite"
            ),
            Self::NonFinitePreTarget {
                data_point,
                decorrelator,
                channel,
                subband,
            } => write!(
                f,
                "A-JOC pre target is non-finite at data point {data_point}/decorrelator {decorrelator}/input {channel}/subband {subband}"
            ),
            Self::NonFiniteCoefficient { group, index } => {
                write!(
                    f,
                    "A-JOC {group:?} rolling coefficient {index} is non-finite"
                )
            }
            Self::NonFiniteDecorrelatorInput {
                decorrelator,
                timeslot,
                subband,
            } => write!(
                f,
                "A-JOC decorrelator {decorrelator} input is non-finite at timeslot {timeslot}/subband {subband}"
            ),
            Self::Decorrelator {
                decorrelator,
                source,
            } => write!(f, "A-JOC decorrelator {decorrelator} failed: {source}"),
            Self::NonFiniteOutput {
                object,
                timeslot,
                subband,
            } => write!(
                f,
                "A-JOC object {object} output is non-finite at timeslot {timeslot}/subband {subband}"
            ),
        }
    }
}

impl core::error::Error for ReconstructionError {}

impl From<DiffError> for ReconstructionError {
    fn from(error: DiffError) -> Self {
        Self::Diff(error)
    }
}

impl From<InterpolationError> for ReconstructionError {
    fn from(error: InterpolationError) -> Self {
        Self::Interpolation(error)
    }
}

/// 把一帧 A-JOC side information 和对齐后的下混 QMF 重建为对象 QMF。
///
/// `input[ch]` 是 `Qin_AJOC,ch`，`output[o]` 是 `Qout_AJOC,o`；每个固定容量帧
/// 只有前 `num_qmf_timeslots` 项有效。control/raw 可以是复用后更长的工作区，
/// 但 input/output 至少要覆盖当前拓扑。成功时会把输出帧未使用的尾部也清零。
/// 本入口不执行 dialogue enhancement；产品 full 驱动必须先消费拒绝活动 DE 的
/// 支持凭证，不能把未实现分支伪装成普通矩阵重建。
///
/// # Errors
///
/// 形状、差分、反量化、插值、矩阵乘法或去相关失败时返回
/// [`ReconstructionError`]。任何错误都不提交 `state`。
#[expect(
    clippy::too_many_arguments,
    reason = "帧级事务必须显式绑定 side information、实际 QMF 时隙、输入、两类持久对象和输出；\
              把它们藏进可伪造 DTO 会模糊状态所有权"
)]
pub fn reconstruct_frame(
    ajoc: &Ajoc,
    controls: &[AjocObjectControl],
    raw: &[AjocObjectMatrix],
    num_qmf_timeslots: u8,
    input: &[QmfChannelFrame],
    state: &mut AjocReconstructionState,
    workspace: &mut AjocWorkspace,
    output: &mut [QmfChannelFrame],
) -> Result<(), ReconstructionError> {
    let schedule = InterpolationSchedule::from_data_points(&ajoc.data_points, num_qmf_timeslots)?;
    let dimensions = validate_frame(
        ajoc,
        controls,
        raw,
        usize::from(schedule.num_qmf_timeslots()),
        input,
        state,
        output,
    )?;

    #[cfg(feature = "ajoc-reconstruction-split-profile")]
    prepare_reconstruction_candidate(state, workspace, output);
    #[cfg(not(feature = "ajoc-reconstruction-split-profile"))]
    {
        workspace.prepare_frame();
        for frame in output.iter_mut() {
            *frame = empty_channel_frame();
        }
        workspace.candidate.as_mut().clone_from(state);
    }

    decode(
        ajoc,
        controls,
        raw,
        &mut workspace.candidate.diff[..dimensions.objects],
        &mut workspace.quantized[..dimensions.objects],
    )?;

    let maps = prepare_targets(controls, dimensions, workspace)?;
    process_timeslots(
        controls, dimensions, &schedule, &maps, input, workspace, output,
    )?;

    #[cfg(feature = "ajoc-reconstruction-split-profile")]
    commit_reconstruction_candidate(dimensions, state, workspace);
    #[cfg(not(feature = "ajoc-reconstruction-split-profile"))]
    {
        workspace.candidate.shape = Some(dimensions.shape);
        state.clone_from(&workspace.candidate);
    }
    Ok(())
}

#[cfg(feature = "ajoc-reconstruction-split-profile")]
#[inline(never)]
fn prepare_reconstruction_candidate(
    state: &AjocReconstructionState,
    workspace: &mut AjocWorkspace,
    output: &mut [QmfChannelFrame],
) {
    workspace.prepare_frame();
    for frame in output {
        *frame = empty_channel_frame();
    }
    workspace.candidate.as_mut().clone_from(state);
}

#[cfg(feature = "ajoc-reconstruction-split-profile")]
#[inline(never)]
fn commit_reconstruction_candidate(
    dimensions: FrameDimensions,
    state: &mut AjocReconstructionState,
    workspace: &mut AjocWorkspace,
) {
    workspace.candidate.shape = Some(dimensions.shape);
    state.clone_from(&workspace.candidate);
}

#[derive(Clone, Copy)]
struct FrameDimensions {
    objects: usize,
    num_dpoints: usize,
    num_dmx: usize,
    num_decorr: usize,
    decorr_enabled: [bool; MAX_DECORRELATORS],
    timeslots: usize,
    shape: ReconstructionShape,
}

#[cfg_attr(feature = "ajoc-reconstruction-split-profile", inline(never))]
fn validate_frame(
    ajoc: &Ajoc,
    controls: &[AjocObjectControl],
    raw: &[AjocObjectMatrix],
    timeslots: usize,
    input: &[QmfChannelFrame],
    state: &AjocReconstructionState,
    output: &[QmfChannelFrame],
) -> Result<FrameDimensions, ReconstructionError> {
    let objects = usize::try_from(ajoc.num_umx_signals).unwrap_or(usize::MAX);
    let num_dpoints = usize::from(ajoc.data_points.count);
    let num_dmx = usize::from(ajoc.num_dmx_signals);
    let num_decorr = usize::from(ajoc.num_decorr);
    if objects == 0
        || objects > MAX_RECONSTRUCTED_OBJECTS
        || num_dpoints > MAX_DATA_POINTS
        || num_dmx == 0
        || num_dmx > MAX_AJOC_DMX_SIGNALS
        || num_decorr > MAX_DECORRELATORS
    {
        return Err(ReconstructionError::DimensionOutOfRange {
            objects: ajoc.num_umx_signals,
            num_dpoints: ajoc.data_points.count,
            num_dmx: ajoc.num_dmx_signals,
            num_decorr: ajoc.num_decorr,
        });
    }
    if controls.len() < objects
        || raw.len() < objects
        || input.len() < num_dmx
        || output.len() < objects
    {
        return Err(ReconstructionError::WorkspaceTooSmall {
            objects,
            num_dmx,
            controls: controls.len(),
            raw: raw.len(),
            input: input.len(),
            output: output.len(),
        });
    }

    let shape = ReconstructionShape {
        objects: u8::try_from(objects).unwrap_or(u8::MAX),
        num_dmx: ajoc.num_dmx_signals,
        num_decorr: ajoc.num_decorr,
    };
    if let Some(previous) = state.shape
        && previous != shape
    {
        return Err(ReconstructionError::ShapeChangeRequiresReset {
            previous,
            current: shape,
        });
    }

    let mut decorr_enabled = [false; MAX_DECORRELATORS];
    for (decorrelator, enabled) in decorr_enabled.iter_mut().enumerate().take(num_decorr) {
        *enabled = ajoc.decorr_enable(decorrelator).unwrap_or(false);
    }

    validate_input(input, num_dmx, timeslots)?;

    Ok(FrameDimensions {
        objects,
        num_dpoints,
        num_dmx,
        num_decorr,
        decorr_enabled,
        timeslots,
        shape,
    })
}

#[cfg_attr(feature = "ajoc-reconstruction-split-profile", inline(never))]
fn validate_input(
    input: &[QmfChannelFrame],
    num_dmx: usize,
    timeslots: usize,
) -> Result<(), ReconstructionError> {
    for channel in 0..num_dmx {
        for timeslot in 0..timeslots {
            for subband in 0..NUM_QMF_SUBBANDS {
                if !input[channel][timeslot].re[subband].is_finite()
                    || !input[channel][timeslot].im[subband].is_finite()
                {
                    return Err(ReconstructionError::NonFiniteInput {
                        channel,
                        timeslot,
                        subband,
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg_attr(feature = "ajoc-reconstruction-split-profile", inline(never))]
fn prepare_targets(
    controls: &[AjocObjectControl],
    dimensions: FrameDimensions,
    workspace: &mut AjocWorkspace,
) -> Result<[Option<AjocBandMap>; MAX_RECONSTRUCTED_OBJECTS], ReconstructionError> {
    let mut maps = [None; MAX_RECONSTRUCTED_OBJECTS];
    for object in 0..dimensions.objects {
        let control = &controls[object];
        if !control.present || dimensions.num_dpoints == 0 {
            continue;
        }
        let map = AjocBandMap::for_num_bands(control.num_bands).ok_or(
            ReconstructionError::MissingBandMap {
                object,
                num_bands: control.num_bands,
            },
        )?;
        maps[object] = Some(map);

        for data_point in 0..dimensions.num_dpoints {
            for channel in 0..dimensions.num_dmx {
                for band in 0..usize::from(control.num_bands) {
                    let quantized = workspace.quantized[object]
                        .dry(data_point, channel, band)
                        .ok_or(ReconstructionError::MissingQuantizedValue {
                            kind: MatrixKind::Dry,
                            object,
                            data_point,
                            row: channel,
                            band,
                        })?;
                    workspace.dry_targets[dry_target_index(data_point, object, channel, band)] =
                        dequantise(MatrixKind::Dry, control.coarse, quantized).map_err(
                            |source| ReconstructionError::Dequant {
                                object,
                                data_point,
                                row: channel,
                                band,
                                source,
                            },
                        )?;
                }
            }
            for decorrelator in 0..dimensions.num_decorr {
                for band in 0..usize::from(control.num_bands) {
                    let quantized = workspace.quantized[object]
                        .wet(data_point, decorrelator, band)
                        .ok_or(ReconstructionError::MissingQuantizedValue {
                            kind: MatrixKind::Wet,
                            object,
                            data_point,
                            row: decorrelator,
                            band,
                        })?;
                    workspace.wet_targets
                        [wet_target_index(data_point, object, decorrelator, band)] =
                        dequantise(MatrixKind::Wet, control.coarse, quantized).map_err(
                            |source| ReconstructionError::Dequant {
                                object,
                                data_point,
                                row: decorrelator,
                                band,
                                source,
                            },
                        )?;
                }
            }
        }
    }

    calculate_pre_targets(controls, dimensions, &maps, workspace)?;
    Ok(maps)
}

#[cfg_attr(feature = "ajoc-reconstruction-split-profile", inline(never))]
fn calculate_pre_targets(
    controls: &[AjocObjectControl],
    dimensions: FrameDimensions,
    maps: &[Option<AjocBandMap>; MAX_RECONSTRUCTED_OBJECTS],
    workspace: &mut AjocWorkspace,
) -> Result<(), ReconstructionError> {
    for data_point in 0..dimensions.num_dpoints {
        for subband in 0..NUM_QMF_SUBBANDS {
            for decorrelator in 0..dimensions.num_decorr {
                for channel in 0..dimensions.num_dmx {
                    let mut sum = 0.0f64;
                    for object in 0..dimensions.objects {
                        if !controls[object].present {
                            continue;
                        }
                        let map = maps[object].ok_or(ReconstructionError::MissingBandMap {
                            object,
                            num_bands: controls[object].num_bands,
                        })?;
                        let band = usize::from(map.column()[subband]);
                        let dry = workspace.dry_targets
                            [dry_target_index(data_point, object, channel, band)];
                        let wet = workspace.wet_targets
                            [wet_target_index(data_point, object, decorrelator, band)];
                        sum += f64::from(wet.abs()) * f64::from(dry);
                    }
                    let value = sum as f32;
                    if !sum.is_finite() || !value.is_finite() {
                        return Err(ReconstructionError::NonFinitePreTarget {
                            data_point,
                            decorrelator,
                            channel,
                            subband,
                        });
                    }
                    workspace.pre_targets
                        [pre_target_index(data_point, decorrelator, channel, subband)] = value;
                }
            }
        }
    }
    Ok(())
}

#[cfg_attr(feature = "ajoc-reconstruction-split-profile", inline(never))]
fn process_timeslots(
    controls: &[AjocObjectControl],
    dimensions: FrameDimensions,
    schedule: &InterpolationSchedule,
    maps: &[Option<AjocBandMap>; MAX_RECONSTRUCTED_OBJECTS],
    input: &[QmfChannelFrame],
    workspace: &mut AjocWorkspace,
    output: &mut [QmfChannelFrame],
) -> Result<(), ReconstructionError> {
    let AjocWorkspace {
        candidate,
        dry_targets,
        wet_targets,
        pre_targets,
        u,
        y,
        output_pair,
        ..
    } = workspace;
    let AjocReconstructionState {
        interpolation,
        dry,
        wet,
        pre,
        decorrelators,
        ..
    } = &mut **candidate;

    let mut rolling = RollingCoefficients::with_layout(
        &mut *dry,
        &mut *wet,
        &mut *pre,
        rolling_layout(dimensions),
    );
    for timeslot in 0..dimensions.timeslots {
        let non_finite = interpolation.interpolate_timeslot_checked(
            u8::try_from(timeslot).unwrap_or(u8::MAX),
            schedule,
            &mut rolling,
            |group, data_point, index| {
                target_for(
                    group,
                    data_point,
                    index,
                    controls,
                    dimensions,
                    maps,
                    dry_targets,
                    wet_targets,
                    pre_targets,
                )
            },
        )?;
        if let Some(non_finite) = non_finite {
            return Err(ReconstructionError::NonFiniteCoefficient {
                group: non_finite.group,
                index: non_finite.index,
            });
        }

        for decorrelator in 0..dimensions.num_decorr {
            u[decorrelator] =
                decorrelator_input(decorrelator, timeslot, dimensions, rolling.pre(), input)?;
            let kind = kind_for_ajoc_index(decorrelator).ok_or(
                ReconstructionError::DimensionOutOfRange {
                    objects: u32::from(dimensions.shape.objects),
                    num_dpoints: u8::try_from(dimensions.num_dpoints).unwrap_or(u8::MAX),
                    num_dmx: dimensions.shape.num_dmx,
                    num_decorr: dimensions.shape.num_decorr,
                },
            )?;
            super::decorrelator::process_timeslot(
                kind,
                &u[decorrelator],
                &mut decorrelators[decorrelator],
                &mut y[decorrelator],
            )
            .map_err(|source| ReconstructionError::Decorrelator {
                decorrelator,
                source,
            })?;
        }

        reconstruct_output(
            timeslot,
            dimensions,
            rolling.dry(),
            rolling.wet(),
            input,
            y,
            output_pair,
            output,
        )?;
    }
    Ok(())
}

fn rolling_layout(dimensions: FrameDimensions) -> RollingLayout {
    RollingLayout {
        dry: CoefficientLayout::strided(
            dimensions.objects,
            dimensions.num_dmx * NUM_QMF_SUBBANDS,
            MAX_AJOC_DMX_SIGNALS * NUM_QMF_SUBBANDS,
        ),
        wet: CoefficientLayout::strided(
            dimensions.objects,
            dimensions.num_decorr * NUM_QMF_SUBBANDS,
            MAX_DECORRELATORS * NUM_QMF_SUBBANDS,
        ),
        pre: CoefficientLayout::strided(
            dimensions.num_decorr,
            dimensions.num_dmx * NUM_QMF_SUBBANDS,
            MAX_AJOC_DMX_SIGNALS * NUM_QMF_SUBBANDS,
        ),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "插值 closure 只能返回值；把三组已预检 target 和形状显式传入，保证热路径无失败分支"
)]
fn target_for(
    group: CoefficientGroup,
    data_point: usize,
    index: usize,
    controls: &[AjocObjectControl],
    dimensions: FrameDimensions,
    maps: &[Option<AjocBandMap>; MAX_RECONSTRUCTED_OBJECTS],
    dry_targets: &[f32],
    wet_targets: &[f32],
    pre_targets: &[f32],
) -> f32 {
    match group {
        CoefficientGroup::Dry => {
            let subband = index % NUM_QMF_SUBBANDS;
            let row = index / NUM_QMF_SUBBANDS;
            let channel = row % MAX_AJOC_DMX_SIGNALS;
            let object = row / MAX_AJOC_DMX_SIGNALS;
            if object >= dimensions.objects
                || channel >= dimensions.num_dmx
                || !controls[object].present
            {
                return 0.0;
            }
            let Some(map) = maps[object] else {
                return 0.0;
            };
            let band = usize::from(map.column()[subband]);
            dry_targets[dry_target_index(data_point, object, channel, band)]
        }
        CoefficientGroup::Wet => {
            let subband = index % NUM_QMF_SUBBANDS;
            let row = index / NUM_QMF_SUBBANDS;
            let decorrelator = row % MAX_DECORRELATORS;
            let object = row / MAX_DECORRELATORS;
            if object >= dimensions.objects
                || decorrelator >= dimensions.num_decorr
                || !controls[object].present
            {
                return 0.0;
            }
            let Some(map) = maps[object] else {
                return 0.0;
            };
            let band = usize::from(map.column()[subband]);
            wet_targets[wet_target_index(data_point, object, decorrelator, band)]
        }
        CoefficientGroup::Pre => {
            let subband = index % NUM_QMF_SUBBANDS;
            let row = index / NUM_QMF_SUBBANDS;
            let channel = row % MAX_AJOC_DMX_SIGNALS;
            let decorrelator = row / MAX_AJOC_DMX_SIGNALS;
            if decorrelator >= dimensions.num_decorr || channel >= dimensions.num_dmx {
                return 0.0;
            }
            pre_targets[pre_target_index(data_point, decorrelator, channel, subband)]
        }
    }
}

#[cfg_attr(feature = "ajoc-reconstruction-split-profile", inline(never))]
fn decorrelator_input(
    decorrelator: usize,
    timeslot: usize,
    dimensions: FrameDimensions,
    pre: &[RollingCoefficient],
    input: &[QmfChannelFrame],
) -> Result<QmfSlot, ReconstructionError> {
    let mut out = QmfSlot::zero();
    for subband in 0..NUM_QMF_SUBBANDS {
        let mut re = 0.0f64;
        let mut im = 0.0f64;
        for channel in 0..dimensions.num_dmx {
            let coefficient = pre[pre_rolling_index(decorrelator, channel, subband)].current();
            re += f64::from(coefficient) * f64::from(input[channel][timeslot].re[subband]);
            im += f64::from(coefficient) * f64::from(input[channel][timeslot].im[subband]);
        }
        out.re[subband] = re as f32;
        out.im[subband] = im as f32;
        if !re.is_finite()
            || !im.is_finite()
            || !out.re[subband].is_finite()
            || !out.im[subband].is_finite()
        {
            return Err(ReconstructionError::NonFiniteDecorrelatorInput {
                decorrelator,
                timeslot,
                subband,
            });
        }
    }
    Ok(out)
}

#[cfg_attr(feature = "ajoc-reconstruction-split-profile", inline(never))]
#[expect(
    clippy::too_many_arguments,
    reason = "输出热核显式区分持久 rolling、局部 AoSoA、共享输入和调用方输出"
)]
fn reconstruct_output(
    timeslot: usize,
    dimensions: FrameDimensions,
    dry: &[RollingCoefficient],
    wet: &[RollingCoefficient],
    input: &[QmfChannelFrame],
    y: &[QmfSlot; MAX_DECORRELATORS],
    coefficient_pair: &mut OutputPairWorkspace,
    output: &mut [QmfChannelFrame],
) -> Result<(), ReconstructionError> {
    let paired_objects = dimensions.objects / OUTPUT_OBJECT_LANES * OUTPUT_OBJECT_LANES;
    let (paired_output, scalar_output) = output[..dimensions.objects].split_at_mut(paired_objects);
    let paired_dry = &dry[..paired_objects * DRY_OBJECT_STRIDE];
    let paired_wet = &wet[..paired_objects * WET_OBJECT_STRIDE];

    let pairs = paired_output
        .as_chunks_mut::<OUTPUT_OBJECT_LANES>()
        .0
        .iter_mut()
        .zip(
            paired_dry
                .as_chunks::<{ OUTPUT_OBJECT_LANES * DRY_OBJECT_STRIDE }>()
                .0
                .iter(),
        )
        .zip(
            paired_wet
                .as_chunks::<{ OUTPUT_OBJECT_LANES * WET_OBJECT_STRIDE }>()
                .0
                .iter(),
        );
    for (pair_index, ((output_pair, dry_pair), wet_pair)) in pairs.enumerate() {
        let [first_output, second_output] = output_pair;
        let (first_dry, second_dry) = dry_pair.split_at(DRY_OBJECT_STRIDE);
        let (first_wet, second_wet) = wet_pair.split_at(WET_OBJECT_STRIDE);
        reconstruct_output_pair(
            pair_index * OUTPUT_OBJECT_LANES,
            timeslot,
            dimensions,
            [first_dry, second_dry],
            [first_wet, second_wet],
            input,
            y,
            coefficient_pair,
            [&mut first_output[timeslot], &mut second_output[timeslot]],
        )?;
    }

    if let Some(output) = scalar_output.first_mut() {
        let object = paired_objects;
        let dry_start = object * DRY_OBJECT_STRIDE;
        let wet_start = object * WET_OBJECT_STRIDE;
        reconstruct_single_output(
            object,
            timeslot,
            dimensions,
            &dry[dry_start..dry_start + DRY_OBJECT_STRIDE],
            &wet[wet_start..wet_start + WET_OBJECT_STRIDE],
            input,
            y,
            &mut output[timeslot],
        )?;
    }
    Ok(())
}

/// 一个子带上两个对象的独立复数 `f64` 累加器。
///
/// 固定宽度和 16 字节对齐让 stable Rust 的循环向量器可以把对象维映射到
/// ARM64 NEON / x86-64 SSE2；每个 lane 仍是各自的标量加法时间线。
#[repr(C, align(16))]
struct OutputObjectPair {
    re: [f64; OUTPUT_OBJECT_LANES],
    im: [f64; OUTPUT_OBJECT_LANES],
}

impl OutputObjectPair {
    const fn new() -> Self {
        Self {
            re: [0.0; OUTPUT_OBJECT_LANES],
            im: [0.0; OUTPUT_OBJECT_LANES],
        }
    }

    #[inline(always)]
    fn accumulate(
        &mut self,
        coefficients: [f32; OUTPUT_OBJECT_LANES],
        input_re: f64,
        input_im: f64,
    ) {
        let coefficients = [f64::from(coefficients[0]), f64::from(coefficients[1])];
        for lane in 0..OUTPUT_OBJECT_LANES {
            self.re[lane] += coefficients[lane] * input_re;
            self.im[lane] += coefficients[lane] * input_im;
        }
    }
}

/// 两个互不相关的对象并排执行最终 dry/wet 矩阵。
///
/// 每个对象 lane 内仍先按 channel、再按 decorrelator 的既有顺序更新独立的
/// `f64` 累加器；共享的输入只提升到 `f64` 一次，允许 LLVM 在 safe Rust 下生成
/// 2×f64 垂直 SIMD。对象间没有归约，也不使用 FMA 或重排对象内加法树。
#[inline(always)]
#[expect(
    clippy::too_many_arguments,
    reason = "双对象热内核显式接收 fixed-stride 行、共享输入和两个输出，避免伪装成可变形 DTO"
)]
fn reconstruct_output_pair(
    first_object: usize,
    timeslot: usize,
    dimensions: FrameDimensions,
    dry: [&[RollingCoefficient]; OUTPUT_OBJECT_LANES],
    wet: [&[RollingCoefficient]; OUTPUT_OBJECT_LANES],
    input: &[QmfChannelFrame],
    y: &[QmfSlot; MAX_DECORRELATORS],
    coefficient_pair: &mut OutputPairWorkspace,
    output: [&mut QmfSlot; OUTPUT_OBJECT_LANES],
) -> Result<(), ReconstructionError> {
    let [first_output, second_output] = output;
    let mut first_non_finite = None;
    let mut second_non_finite = None;
    coefficient_pair.load(dry, wet, dimensions);

    for subband in 0..NUM_QMF_SUBBANDS {
        let mut accumulator = OutputObjectPair::new();
        for channel in 0..dimensions.num_dmx {
            let coefficient_index = channel * NUM_QMF_SUBBANDS + subband;
            let input_re = f64::from(input[channel][timeslot].re[subband]);
            let input_im = f64::from(input[channel][timeslot].im[subband]);
            accumulator.accumulate(coefficient_pair.dry[coefficient_index], input_re, input_im);
        }
        for decorrelator in 0..dimensions.num_decorr {
            // P2 6.3.6.2.1：dense 语法即使在该路禁用时仍携带 wet 段，
            // 因此不能靠系数自然为零。rolling 与 decorrelator 继续推进历史，
            // 这里只把禁用路从可观察的对象输出中硬性移除。
            if !dimensions.decorr_enabled[decorrelator] {
                continue;
            }
            let coefficient_index = decorrelator * NUM_QMF_SUBBANDS + subband;
            // 只有 pre target 对 wet 取绝对值；最终输出必须保留原始 wet 符号。
            let input_re = f64::from(y[decorrelator].re[subband]);
            let input_im = f64::from(y[decorrelator].im[subband]);
            accumulator.accumulate(coefficient_pair.wet[coefficient_index], input_re, input_im);
        }

        let [first_re, second_re] = accumulator.re;
        let [first_im, second_im] = accumulator.im;
        let first_out_re = first_re as f32;
        let first_out_im = first_im as f32;
        let second_out_re = second_re as f32;
        let second_out_im = second_im as f32;
        first_output.re[subband] = first_out_re;
        first_output.im[subband] = first_out_im;
        second_output.re[subband] = second_out_re;
        second_output.im[subband] = second_out_im;
        if first_non_finite.is_none()
            && (!first_re.is_finite()
                || !first_im.is_finite()
                || !first_out_re.is_finite()
                || !first_out_im.is_finite())
        {
            first_non_finite = Some(subband);
        }
        if second_non_finite.is_none()
            && (!second_re.is_finite()
                || !second_im.is_finite()
                || !second_out_re.is_finite()
                || !second_out_im.is_finite())
        {
            second_non_finite = Some(subband);
        }
    }

    // 原标量遍历按 object 再按 subband 报错；即使第二 lane 更早溢出，第一
    // lane 的任意错误仍须优先，不能让并排执行改变错误上下文。
    if let Some(subband) = first_non_finite {
        return Err(ReconstructionError::NonFiniteOutput {
            object: first_object,
            timeslot,
            subband,
        });
    }
    if let Some(subband) = second_non_finite {
        return Err(ReconstructionError::NonFiniteOutput {
            object: first_object + 1,
            timeslot,
            subband,
        });
    }
    Ok(())
}

/// 奇数对象尾部与测试参考共用的原始标量加法树。
#[inline(always)]
#[expect(
    clippy::too_many_arguments,
    reason = "标量尾部显式接收一个对象的 fixed-stride 行和共享输入"
)]
fn reconstruct_single_output(
    object: usize,
    timeslot: usize,
    dimensions: FrameDimensions,
    dry: &[RollingCoefficient],
    wet: &[RollingCoefficient],
    input: &[QmfChannelFrame],
    y: &[QmfSlot; MAX_DECORRELATORS],
    output: &mut QmfSlot,
) -> Result<(), ReconstructionError> {
    for subband in 0..NUM_QMF_SUBBANDS {
        let mut re = 0.0f64;
        let mut im = 0.0f64;
        for channel in 0..dimensions.num_dmx {
            let coefficient = dry[channel * NUM_QMF_SUBBANDS + subband].current();
            re += f64::from(coefficient) * f64::from(input[channel][timeslot].re[subband]);
            im += f64::from(coefficient) * f64::from(input[channel][timeslot].im[subband]);
        }
        for decorrelator in 0..dimensions.num_decorr {
            if !dimensions.decorr_enabled[decorrelator] {
                continue;
            }
            let coefficient = wet[decorrelator * NUM_QMF_SUBBANDS + subband].current();
            re += f64::from(coefficient) * f64::from(y[decorrelator].re[subband]);
            im += f64::from(coefficient) * f64::from(y[decorrelator].im[subband]);
        }
        output.re[subband] = re as f32;
        output.im[subband] = im as f32;
        if !re.is_finite()
            || !im.is_finite()
            || !output.re[subband].is_finite()
            || !output.im[subband].is_finite()
        {
            return Err(ReconstructionError::NonFiniteOutput {
                object,
                timeslot,
                subband,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
const fn dry_rolling_index(object: usize, channel: usize, subband: usize) -> usize {
    (object * MAX_AJOC_DMX_SIGNALS + channel) * NUM_QMF_SUBBANDS + subband
}

#[cfg(test)]
const fn wet_rolling_index(object: usize, decorrelator: usize, subband: usize) -> usize {
    (object * MAX_DECORRELATORS + decorrelator) * NUM_QMF_SUBBANDS + subband
}

const fn pre_rolling_index(decorrelator: usize, channel: usize, subband: usize) -> usize {
    (decorrelator * MAX_AJOC_DMX_SIGNALS + channel) * NUM_QMF_SUBBANDS + subband
}

const fn dry_target_index(data_point: usize, object: usize, channel: usize, band: usize) -> usize {
    ((data_point * MAX_RECONSTRUCTED_OBJECTS + object) * MAX_AJOC_DMX_SIGNALS + channel)
        * MAX_AJOC_BANDS
        + band
}

const fn wet_target_index(
    data_point: usize,
    object: usize,
    decorrelator: usize,
    band: usize,
) -> usize {
    ((data_point * MAX_RECONSTRUCTED_OBJECTS + object) * MAX_DECORRELATORS + decorrelator)
        * MAX_AJOC_BANDS
        + band
}

const fn pre_target_index(
    data_point: usize,
    decorrelator: usize,
    channel: usize,
    subband: usize,
) -> usize {
    data_point * PRE_ROLLING_LEN + pre_rolling_index(decorrelator, channel, subband)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::thread;
    use std::vec;
    use std::vec::Vec;

    const TIMESLOTS: u8 = 24;

    fn on_large_stack(test: impl FnOnce() + Send + 'static) {
        thread::Builder::new()
            .name(std::string::String::from("ajoc-reconstruction-test"))
            .stack_size(16 * 1_024 * 1_024)
            .spawn(test)
            .expect("应能建立大工作区测试线程")
            .join()
            .expect("大工作区测试不应 panic");
    }

    fn control(
        present: bool,
        num_bands: u8,
        num_dmx: usize,
        num_decorr: usize,
    ) -> AjocObjectControl {
        AjocObjectControl::for_test(
            present,
            num_bands,
            true,
            false,
            &vec![true; num_dmx],
            &vec![true; num_decorr],
        )
    }

    fn frequency_symbols(values: &[i16], levels: i16) -> Vec<i16> {
        let mut out = Vec::with_capacity(values.len());
        for (index, &value) in values.iter().enumerate() {
            if index == 0 {
                out.push(value);
            } else {
                out.push((value - values[index - 1]).rem_euclid(levels));
            }
        }
        out
    }

    fn constant_symbols(value: i16, bands: usize) -> Vec<i16> {
        let mut values = vec![0; bands];
        if let Some(first) = values.first_mut() {
            *first = value;
        }
        values
    }

    fn matrix(
        num_bands: u8,
        num_dmx: u8,
        num_decorr: u8,
        dry_values: &[i16],
        wet_values: &[i16],
    ) -> AjocObjectMatrix {
        let mut out = AjocObjectMatrix::for_test(num_bands, 1, num_dmx, num_decorr);
        for channel in 0..usize::from(num_dmx) {
            out.set_dry_for_test(0, channel, dry_values, false);
        }
        for decorrelator in 0..usize::from(num_decorr) {
            out.set_wet_for_test(0, decorrelator, wet_values, false);
        }
        out
    }

    fn zero_inputs(count: usize) -> Vec<QmfChannelFrame> {
        vec![empty_channel_frame(); count]
    }

    fn sentinel_frame(value: f32) -> QmfChannelFrame {
        let mut frame = empty_channel_frame();
        for slot in &mut frame {
            slot.re.fill(value);
            slot.im.fill(-value);
        }
        frame
    }

    #[test]
    fn full_capacity_preserves_each_reconstructed_qmf_lane() {
        on_large_stack(|| {
            for objects in [24usize, 64, 128] {
                let ajoc = Ajoc::for_test(0, 1, objects as u32, 2);
                let controls = vec![control(true, 1, 2, 0); objects];
                let mut matrices = Vec::new();
                for object in 0..objects {
                    let mut coefficients = AjocObjectMatrix::for_test(1, 1, 2, 0);
                    coefficients.set_dry_for_test(0, 0, &[10 + (object % 32) as i16], false);
                    coefficients.set_dry_for_test(0, 1, &[10 + (object / 32) as i16], false);
                    matrices.push(coefficients);
                }
                let input = vec![sentinel_frame(0.25), sentinel_frame(2.0)];
                let mut output = vec![sentinel_frame(99.0); objects];
                let mut state = AjocReconstructionState::new();
                let mut workspace = AjocWorkspace::new();
                reconstruct_frame(&ajoc, &controls, &matrices, TIMESLOTS,
                    &input, &mut state, &mut workspace, &mut output).unwrap();
                assert_eq!(state.shape().unwrap().objects as usize, objects);
                for object in 0..objects {
                    let a = dequantise(MatrixKind::Dry, true, 10 + (object % 32) as i16).unwrap();
                    let b = dequantise(MatrixKind::Dry, true, 10 + (object / 32) as i16).unwrap();
                    let expected = (f64::from(a) * 0.25 + f64::from(b) * 2.0) as f32;
                    assert!((output[object][1].re[0] - expected).abs() < 0.00001,
                        "source {object}/{objects}: {} vs {expected}", output[object][1].re[0]);
                    assert!((output[object][1].im[0] + expected).abs() < 0.00001);
                    assert_eq!(output[object][usize::from(TIMESLOTS)], QmfSlot::zero());
                }
                state.reset();
                assert_eq!(state.shape(), None);
            }
        });
    }

    #[test]
    fn dry_only_reconstructs_complex_qmf_and_clears_unused_slots() {
        on_large_stack(|| {
            let ajoc = Ajoc::for_test(0, 1, 1, 1);
            let controls = [control(true, 1, 1, 0)];
            let raw = [matrix(1, 1, 0, &[30], &[])];
            let mut input = zero_inputs(1);
            input[0][1].re[0] = 2.0;
            input[0][1].im[0] = -3.0;
            let mut output = vec![sentinel_frame(99.0)];
            let mut state = AjocReconstructionState::new();
            let mut workspace = AjocWorkspace::new();

            reconstruct_frame(
                &ajoc,
                &controls,
                &raw,
                TIMESLOTS,
                &input,
                &mut state,
                &mut workspace,
                &mut output,
            )
            .expect("dry-only 重建");

            let gain = dequantise(MatrixKind::Dry, true, 30).expect("表内 dry");
            assert_eq!(output[0][0], QmfSlot::zero(), "新 target 在时隙 0 之后安装");
            assert_eq!(
                output[0][1].re[0].to_bits(),
                ((f64::from(gain) * 2.0) as f32).to_bits()
            );
            assert_eq!(
                output[0][1].im[0].to_bits(),
                ((f64::from(gain) * -3.0) as f32).to_bits()
            );
            assert_eq!(output[0][usize::from(TIMESLOTS)], QmfSlot::zero());
            assert_eq!(
                state.shape(),
                Some(ReconstructionShape {
                    objects: 1,
                    num_dmx: 1,
                    num_decorr: 0,
                })
            );
        });
    }

    #[test]
    fn wet_path_is_nonzero_and_only_the_final_wet_term_keeps_its_sign() {
        on_large_stack(|| {
            fn run(wet_q: i16) -> (Vec<QmfChannelFrame>, AjocReconstructionState) {
                let ajoc = Ajoc::for_test(1, 1, 1, 1);
                let controls = [control(true, 1, 1, 1)];
                let raw = [matrix(1, 1, 1, &[30], &[wet_q])];
                let mut input = zero_inputs(1);
                input[0][1].re[0] = 1.0;
                let mut output = vec![empty_channel_frame()];
                let mut state = AjocReconstructionState::new();
                let mut workspace = AjocWorkspace::new();
                reconstruct_frame(
                    &ajoc,
                    &controls,
                    &raw,
                    TIMESLOTS,
                    &input,
                    &mut state,
                    &mut workspace,
                    &mut output,
                )
                .expect("wet 重建");
                (output, state)
            }

            let (positive, positive_state) = run(15);
            let (negative, negative_state) = run(5);
            let delayed = 8usize;
            assert_ne!(positive[0][delayed].re[0].to_bits(), 0);
            assert_eq!(
                positive[0][delayed].re[0].to_bits(),
                (-negative[0][delayed].re[0]).to_bits(),
                "pre 必须取 abs(wet)，最终 wet 项才保留符号"
            );
            assert_eq!(
                positive_state.decorrelators[0], negative_state.decorrelators[0],
                "正负等幅 wet 应产生相同 decorrelator 输入与状态"
            );
        });
    }

    #[test]
    fn disabled_dense_decorrelator_keeps_history_but_never_reaches_output() {
        on_large_stack(|| {
            fn run(enabled: bool, wet_q: i16) -> (Vec<QmfChannelFrame>, AjocReconstructionState) {
                let mut ajoc = Ajoc::for_test(1, 1, 1, 1);
                ajoc.set_decorr_enable_for_test(0, enabled);
                let controls = [control(true, 1, 1, 1)];
                let raw = [matrix(1, 1, 1, &[30], &[wet_q])];
                let mut input = zero_inputs(1);
                input[0][1].re[0] = 1.0;
                let mut output = vec![empty_channel_frame()];
                let mut state = AjocReconstructionState::new();
                let mut workspace = AjocWorkspace::new();
                reconstruct_frame(
                    &ajoc,
                    &controls,
                    &raw,
                    TIMESLOTS,
                    &input,
                    &mut state,
                    &mut workspace,
                    &mut output,
                )
                .expect("dense decorrelator gate");
                (output, state)
            }

            let (enabled_output, enabled_state) = run(true, 15);
            let (disabled_output, disabled_state) = run(false, 15);
            let (zero_wet_output, _) = run(true, 10);

            assert_ne!(
                enabled_output, disabled_output,
                "夹具必须让启用路产生可观察的 wet 输出"
            );
            assert_eq!(
                disabled_output, zero_wet_output,
                "禁用路即使在 dense 模式携带非零 wet，也不得混入对象输出"
            );
            assert_eq!(
                disabled_state, enabled_state,
                "输出门控不得中断量化、rolling 或 decorrelator 的跨帧历史"
            );
        });
    }

    #[test]
    fn inactive_object_targets_zero_without_destroying_diff_history() {
        on_large_stack(|| {
            let ajoc = Ajoc::for_test(0, 1, 1, 1);
            let active = [control(true, 1, 1, 0)];
            let active_raw = [matrix(1, 1, 0, &[30], &[])];
            let mut state = AjocReconstructionState::new();
            let mut workspace = AjocWorkspace::new();
            let mut output = vec![empty_channel_frame()];
            reconstruct_frame(
                &ajoc,
                &active,
                &active_raw,
                TIMESLOTS,
                &zero_inputs(1),
                &mut state,
                &mut workspace,
                &mut output,
            )
            .expect("建立 active 历史");
            let diff_before = state.diff[0];

            let inactive = [control(false, 0, 1, 0)];
            let inactive_raw = [AjocObjectMatrix::new()];
            let mut input = zero_inputs(1);
            for slot in input[0].iter_mut().take(usize::from(TIMESLOTS)) {
                slot.re[0] = 1.0;
            }
            output[0] = sentinel_frame(77.0);
            reconstruct_frame(
                &ajoc,
                &inactive,
                &inactive_raw,
                TIMESLOTS,
                &input,
                &mut state,
                &mut workspace,
                &mut output,
            )
            .expect("inactive target");

            assert_ne!(output[0][0].re[0].to_bits(), 0, "时隙 0 仍使用旧 target");
            assert_eq!(output[0][1].re[0].to_bits(), 0, "一步 ramp 后应归零");
            assert_eq!(state.diff[0], diff_before, "inactive 不得销毁最后传输历史");
        });
    }

    #[test]
    fn fixed_stride_view_leaves_unreachable_rows_and_columns_untouched() {
        on_large_stack(|| {
            let ajoc = Ajoc::for_test(0, 1, 1, 1);
            let controls = [control(true, 1, 1, 0)];
            let raw = [matrix(1, 1, 0, &[30], &[])];
            let mut state = AjocReconstructionState::new();
            state.shape = Some(ReconstructionShape {
                objects: 1,
                num_dmx: 1,
                num_decorr: 0,
            });
            let sentinel = RollingCoefficient::new(7.0);
            let unused_dry_channel = dry_rolling_index(0, 1, 0);
            let unused_dry_object = dry_rolling_index(1, 0, 0);
            state.dry[unused_dry_channel] = sentinel;
            state.dry[unused_dry_object] = sentinel;
            state.wet[0] = sentinel;
            state.pre[0] = sentinel;

            let mut workspace = AjocWorkspace::new();
            let mut output = vec![empty_channel_frame()];
            reconstruct_frame(
                &ajoc,
                &controls,
                &raw,
                TIMESLOTS,
                &zero_inputs(1),
                &mut state,
                &mut workspace,
                &mut output,
            )
            .expect("活动拓扑不应读取 fixed-stride 尾部");

            assert_eq!(state.dry[unused_dry_channel], sentinel);
            assert_eq!(state.dry[unused_dry_object], sentinel);
            assert_eq!(state.wet[0], sentinel);
            assert_eq!(state.pre[0], sentinel);
        });
    }

    #[test]
    fn paired_output_is_bit_exact_to_scalar_for_even_and_odd_object_counts() {
        on_large_stack(|| {
            let num_dmx = 3usize;
            let num_decorr = 3usize;
            let mut decorr_enabled = [false; MAX_DECORRELATORS];
            decorr_enabled[0] = true;
            decorr_enabled[2] = true;

            let mut input = zero_inputs(num_dmx);
            for (channel, frame) in input.iter_mut().enumerate() {
                for subband in 0..NUM_QMF_SUBBANDS {
                    frame[0].re[subband] = (channel * 17 + subband % 13) as f32 / 16.0 - 1.5;
                    frame[0].im[subband] = (channel * 11 + subband % 7) as f32 / 8.0 - 2.0;
                }
            }
            let mut y = [QmfSlot::zero(); MAX_DECORRELATORS];
            for (decorrelator, slot) in y.iter_mut().enumerate().take(num_decorr) {
                for subband in 0..NUM_QMF_SUBBANDS {
                    slot.re[subband] = (decorrelator * 7 + subband % 5) as f32 / 32.0 - 0.5;
                    slot.im[subband] = (decorrelator * 5 + subband % 11) as f32 / 64.0 - 0.75;
                }
            }

            let mut dry = vec![RollingCoefficient::ZERO; DRY_ROLLING_LEN];
            let mut wet = vec![RollingCoefficient::ZERO; WET_ROLLING_LEN];
            for object in 0..3 {
                for channel in 0..num_dmx {
                    for subband in 0..NUM_QMF_SUBBANDS {
                        let value =
                            (object * 19 + channel * 7 + subband % 17) as f32 / 128.0 - 0.625;
                        dry[dry_rolling_index(object, channel, subband)] =
                            RollingCoefficient::new(value);
                    }
                }
                for decorrelator in 0..num_decorr {
                    for subband in 0..NUM_QMF_SUBBANDS {
                        let value =
                            (object * 13 + decorrelator * 3 + subband % 19) as f32 / 256.0 - 0.25;
                        wet[wet_rolling_index(object, decorrelator, subband)] =
                            RollingCoefficient::new(value);
                    }
                }
            }

            for objects in [2usize, 3] {
                let dimensions = FrameDimensions {
                    objects,
                    num_dpoints: 0,
                    num_dmx,
                    num_decorr,
                    decorr_enabled,
                    timeslots: 1,
                    shape: ReconstructionShape {
                        objects: u8::try_from(objects).expect("测试对象数在 u8 内"),
                        num_dmx: u8::try_from(num_dmx).expect("测试输入数在 u8 内"),
                        num_decorr: u8::try_from(num_decorr).expect("测试去相关器数在 u8 内"),
                    },
                };
                let sentinel = sentinel_frame(91.0);
                let mut paired = vec![empty_channel_frame(); objects];
                paired.push(sentinel);
                let mut scalar = paired.clone();
                let mut coefficient_pair = OutputPairWorkspace::new();

                reconstruct_output(
                    0,
                    dimensions,
                    &dry,
                    &wet,
                    &input,
                    &y,
                    &mut coefficient_pair,
                    &mut paired,
                )
                .expect("双对象输出");
                for object in 0..objects {
                    let dry_start = object * DRY_OBJECT_STRIDE;
                    let wet_start = object * WET_OBJECT_STRIDE;
                    reconstruct_single_output(
                        object,
                        0,
                        dimensions,
                        &dry[dry_start..dry_start + DRY_OBJECT_STRIDE],
                        &wet[wet_start..wet_start + WET_OBJECT_STRIDE],
                        &input,
                        &y,
                        &mut scalar[object][0],
                    )
                    .expect("标量参考输出");
                }

                for object in 0..objects {
                    for subband in 0..NUM_QMF_SUBBANDS {
                        assert_eq!(
                            paired[object][0].re[subband].to_bits(),
                            scalar[object][0].re[subband].to_bits(),
                            "objects={objects}, object={object}, re[{subband}]"
                        );
                        assert_eq!(
                            paired[object][0].im[subband].to_bits(),
                            scalar[object][0].im[subband].to_bits(),
                            "objects={objects}, object={object}, im[{subband}]"
                        );
                    }
                }
                assert_eq!(paired[objects], sentinel, "形状外对象不得被改写");
            }
        });
    }

    #[test]
    fn paired_output_preserves_object_major_non_finite_error_order() {
        on_large_stack(|| {
            let dimensions = FrameDimensions {
                objects: 2,
                num_dpoints: 0,
                num_dmx: 1,
                num_decorr: 0,
                decorr_enabled: [false; MAX_DECORRELATORS],
                timeslots: 1,
                shape: ReconstructionShape {
                    objects: 2,
                    num_dmx: 1,
                    num_decorr: 0,
                },
            };
            let mut dry = vec![RollingCoefficient::ZERO; DRY_ROLLING_LEN];
            let wet = vec![RollingCoefficient::ZERO; WET_ROLLING_LEN];
            // 第二对象先在 sb=0 溢出；第一对象随后在 sb=1 溢出。原 object-major
            // 标量顺序必须仍报告第一对象，而不能报告更早算到的第二 lane。
            dry[dry_rolling_index(1, 0, 0)] = RollingCoefficient::new(f32::MAX);
            dry[dry_rolling_index(0, 0, 1)] = RollingCoefficient::new(f32::MAX);
            let mut input = zero_inputs(1);
            input[0][0].re[0] = f32::MAX;
            input[0][0].re[1] = f32::MAX;
            let y = [QmfSlot::zero(); MAX_DECORRELATORS];
            let mut output = vec![empty_channel_frame(); 2];
            let mut coefficient_pair = OutputPairWorkspace::new();

            assert_eq!(
                reconstruct_output(
                    0,
                    dimensions,
                    &dry,
                    &wet,
                    &input,
                    &y,
                    &mut coefficient_pair,
                    &mut output,
                ),
                Err(ReconstructionError::NonFiniteOutput {
                    object: 0,
                    timeslot: 0,
                    subband: 1,
                })
            );
        });
    }

    #[test]
    fn pre_target_uses_each_objects_own_parameter_band_grid() {
        on_large_stack(|| {
            let ajoc = Ajoc::for_test(1, 1, 2, 1);
            let controls = [control(true, 1, 1, 1), control(true, 23, 1, 1)];
            let object0 = matrix(1, 1, 1, &[30], &[15]);
            let object1_dry: Vec<i16> = (25..=47).collect();
            let object1 = matrix(
                23,
                1,
                1,
                &frequency_symbols(&object1_dry, 51),
                &constant_symbols(15, 23),
            );
            let raw = [object0, object1];
            let mut state = AjocReconstructionState::new();
            let mut workspace = AjocWorkspace::new();
            let mut output = vec![empty_channel_frame(); 2];
            reconstruct_frame(
                &ajoc,
                &controls,
                &raw,
                TIMESLOTS,
                &zero_inputs(1),
                &mut state,
                &mut workspace,
                &mut output,
            )
            .expect("混合频带网格");

            let dry0 = dequantise(MatrixKind::Dry, true, 30).expect("dry0");
            let wet = dequantise(MatrixKind::Wet, true, 15).expect("wet");
            let object1_low = dequantise(MatrixKind::Dry, true, 25).expect("low");
            let object1_high = dequantise(MatrixKind::Dry, true, 47).expect("high");
            let expected_low = (f64::from(wet.abs()) * f64::from(dry0)
                + f64::from(wet.abs()) * f64::from(object1_low))
                as f32;
            let expected_high = (f64::from(wet.abs()) * f64::from(dry0)
                + f64::from(wet.abs()) * f64::from(object1_high))
                as f32;
            let low = workspace.pre_targets[pre_target_index(0, 0, 0, 0)];
            let high = workspace.pre_targets[pre_target_index(0, 0, 0, 63)];
            assert_eq!(low.to_bits(), expected_low.to_bits());
            assert_eq!(high.to_bits(), expected_high.to_bits());
            assert_ne!(low.to_bits(), high.to_bits());
        });
    }

    #[test]
    fn every_frame_clears_pre_targets_and_the_complete_output_buffer() {
        on_large_stack(|| {
            let ajoc = Ajoc::for_test(1, 1, 1, 1);
            let controls = [control(true, 1, 1, 1)];
            let mut state = AjocReconstructionState::new();
            let mut workspace = AjocWorkspace::new();
            let mut output = vec![empty_channel_frame(), sentinel_frame(66.0)];
            reconstruct_frame(
                &ajoc,
                &controls,
                &[matrix(1, 1, 1, &[30], &[15])],
                TIMESLOTS,
                &zero_inputs(1),
                &mut state,
                &mut workspace,
                &mut output,
            )
            .expect("非零 pre target");
            assert_ne!(workspace.pre_targets[pre_target_index(0, 0, 0, 0)], 0.0);

            output[0] = sentinel_frame(55.0);
            reconstruct_frame(
                &ajoc,
                &controls,
                &[matrix(1, 1, 1, &[30], &[10])],
                TIMESLOTS,
                &zero_inputs(1),
                &mut state,
                &mut workspace,
                &mut output,
            )
            .expect("零 wet frame");
            assert_eq!(workspace.pre_targets[pre_target_index(0, 0, 0, 0)], 0.0);
            assert!(
                output.iter().flatten().all(|slot| *slot == QmfSlot::zero()),
                "有效对象、固定容量尾部和多余对象工作区都必须覆盖旧输出"
            );
        });
    }

    #[test]
    fn dimensions_band_maps_and_shape_changes_fail_closed() {
        on_large_stack(|| {
            let ajoc = Ajoc::for_test(0, 1, 1, 1);
            let controls = [control(true, 1, 1, 0)];
            let raw = [matrix(1, 1, 0, &[30], &[])];
            let mut state = AjocReconstructionState::new();
            let before = state.clone();
            let mut workspace = AjocWorkspace::new();
            let mut output = vec![empty_channel_frame()];
            assert!(matches!(
                reconstruct_frame(
                    &ajoc,
                    &controls,
                    &raw,
                    TIMESLOTS,
                    &[],
                    &mut state,
                    &mut workspace,
                    &mut output,
                ),
                Err(ReconstructionError::WorkspaceTooSmall { input: 0, .. })
            ));
            assert_eq!(state, before);

            let too_many = Ajoc::for_test(0, 1, 129, 1);
            assert!(matches!(
                reconstruct_frame(
                    &too_many,
                    &[],
                    &[],
                    TIMESLOTS,
                    &[],
                    &mut state,
                    &mut workspace,
                    &mut [],
                ),
                Err(ReconstructionError::DimensionOutOfRange { objects: 129, .. })
            ));
            assert_eq!(state, before);

            let invalid_control = [control(true, 2, 1, 0)];
            let invalid_raw = [matrix(2, 1, 0, &[30, 0], &[])];
            assert_eq!(
                reconstruct_frame(
                    &ajoc,
                    &invalid_control,
                    &invalid_raw,
                    TIMESLOTS,
                    &zero_inputs(1),
                    &mut state,
                    &mut workspace,
                    &mut output,
                ),
                Err(ReconstructionError::MissingBandMap {
                    object: 0,
                    num_bands: 2,
                })
            );
            assert_eq!(state, before);

            reconstruct_frame(
                &ajoc,
                &controls,
                &raw,
                TIMESLOTS,
                &zero_inputs(1),
                &mut state,
                &mut workspace,
                &mut output,
            )
            .expect("建立 shape");
            let committed = state.clone();
            let changed = Ajoc::for_test(0, 1, 1, 2);
            assert!(matches!(
                reconstruct_frame(
                    &changed,
                    &[control(true, 1, 2, 0)],
                    &[matrix(1, 2, 0, &[30], &[])],
                    TIMESLOTS,
                    &zero_inputs(2),
                    &mut state,
                    &mut workspace,
                    &mut output,
                ),
                Err(ReconstructionError::ShapeChangeRequiresReset { .. })
            ));
            assert_eq!(state, committed);
        });
    }

    #[test]
    fn numeric_failure_after_partial_candidate_progress_does_not_commit_state() {
        on_large_stack(|| {
            let ajoc = Ajoc::for_test(0, 1, 1, 1);
            let controls = [control(true, 1, 1, 0)];
            let raw = [matrix(1, 1, 0, &[30], &[])];
            let mut state = AjocReconstructionState::new();
            let mut workspace = AjocWorkspace::new();
            let mut output = vec![empty_channel_frame()];
            reconstruct_frame(
                &ajoc,
                &controls,
                &raw,
                TIMESLOTS,
                &zero_inputs(1),
                &mut state,
                &mut workspace,
                &mut output,
            )
            .expect("先建立跨帧状态");
            let before = state.clone();

            let mut input = zero_inputs(1);
            input[0][0].re[0] = f32::MAX;
            assert_eq!(
                reconstruct_frame(
                    &ajoc,
                    &controls,
                    &raw,
                    TIMESLOTS,
                    &input,
                    &mut state,
                    &mut workspace,
                    &mut output,
                ),
                Err(ReconstructionError::NonFiniteOutput {
                    object: 0,
                    timeslot: 0,
                    subband: 0,
                })
            );
            assert_eq!(state, before, "候选 diff/interp 不得在错误后提交");
        });
    }
}
