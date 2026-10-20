//! A-SPX 通路的编排。
//!
//! `5.7.6.1` 图 6 与 `5.7.7` 图 7 给出链路位置：
//! `ASF/SSF 解码 → PCM → QMF 分析 → A-SPX → 下游 QMF 域工具 → QMF 合成 → PCM`。
//! 本模块负责把 `5.7.6` 各工具按那条链串起来，跨帧状态取自
//! [`AspxChannelState`]，帧内缓冲取自 [`AspxWorkspace`]。
//!
//! # 先立骨架：`Y = 0` 参照路径
//!
//! [`bypass_frame_qmf`] 走的是 A-SPX 通路去掉中间参数环节后剩下的那条：
//!
//! ```text
//! PCM → QMF 分析 → Q_in ─┐
//!                        ├→ 5.7.6.5.3 合并 → Q_out → 下游 QMF 域工具
//!            Y ≡ 0 ──────┘
//! ```
//!
//! `Y` 恒为零，因此 `Q_out` 就是延迟后的 `Q_in`。它是活动 A-SPX 通路内部的
//! 接线诊断参照，用来固定缓冲的时隙基准、状态推进顺序与 `delta_ASPX`；不能据此
//! 推出 A-SPX 未启用的模式仍要经过本通路。SIMPLE 等未启用模式应在进入 A-SPX
//! 之前分流，不引入这里的延迟。
//!
//! # 四段通路，标度因子分单双声道入口
//!
//! | QMF 域入口 | PCM 终端包装 | 加进来的工具 | `Q_out,ASPX` |
//! | --- | --- | --- | --- |
//! | [`bypass_frame_qmf`] | [`bypass_frame`] | 分析、`5.7.6.5.3` | 延迟后的输入 |
//! | [`frame_with_low_band_qmf`] | [`frame_with_low_band`] | `5.7.6.3.2` | 与参照逐位相同 |
//! | [`frame_with_hf_generation_qmf`] | [`frame_with_hf_generation`] | `5.7.6.4.1.2`–`5.7.6.4.1.4` | 与参照逐位相同 |
//! | [`frame_with_scale_factors_qmf`] | [`frame_with_scale_factors`] | `5.7.6.3.4`–`5.7.6.3.5`、`5.7.6.4.2`–`5.7.6.4.5` | 延迟后的输入**加 `Y`** |
//! | [`frame_with_balanced_scale_factors_qmf`] | [`frame_with_balanced_scale_factors`] | 上一行的平衡式双声道变体 | 两路各自加上自己的 `Y` |
//!
//! 中间两条的输出仍等于旁路：`Q_low` 与 `Q_high` 都只喂下游，不进输出。在
//! `5.7.6.4.5` 接上之前整条通路都是这样，而 `Y` 恒零时「没改输出」对全错的接线
//! 同样成立。因此每加一段都必须自带**不看输出也能生效**的判据（延迟线互校、时
//! 隙对齐、跨帧状态逐个隔离观察、逐包络的组数与量化档）。最后一段落地后这条阶
//! 梯才收口：`Q_out` 减去旁路的输出必须逐位等于本帧的 `Y`。
//!
//! # `5.7.6` 已经接全
//!
//! [`AspxChannelState`] 的十一个跨帧状态有明确的边界：五个 `_qmf` 入口推进 QMF
//! 分析及 A-SPX 的十个状态，五个 PCM 包装器才额外推进终端 `synthesis`。这样
//! `Q_out,ASPX` 可以直接交给 A-JOC 等下游 QMF 域工具，不会先合成再二次分析。
//!
//! `Y` 非零之后，
//! [`AspxWorkspace::prepare_frame`] 清 `y` 这件事也第一次可观察——`5.7.6.4.5` 只
//! 写 `[sbx, sbx + num_sb_aspx)`，而 `5.7.6.5.3` 的相加遍历全部 64 个子带。
//!
//! 语法到参数这一层由 [`ChannelInputs::from_parsed`] 接上：它从 `aspx_config()`
//! 与 `aspx_data_*()` 取出三组逐帧输入。只有三个值取不到——`base_samp_freq_48`
//! 在 TOC 里，`master_reset` 要跨 I 帧比对（见 [`MasterResetTracker`]），
//! `first_frame` 是解码器初始化标志。
//!
//! 解码起点落在区间中段时，调用方要先声明前置静音——缺多少历史是它的判断，
//! `5.7.6.4.5` 不猜，不等就报错。
//!
//! 区间这一维已经完整接入两条标度因子入口：包络数与逐包络分辨率取自
//! `AspxChannelFraming` 给出的**那一个** `AspxInterval`，`Q_high` 与预平坦化也
//! 按 `[atsg_sig[0], atsg_sig[num]) · num_ts_in_ats` 生成，不再是整帧。
//! [`frame_with_hf_generation`] 仍写死整帧——它没有成帧输入，那是它的占位。
//!
//! `5.7.6.3.3` 推导的是该区间内部的多个信号与噪声包络边界，不会产生多个区间供
//! 通路循环；包络循环由各下游工具按同一份 `AspxInterval` 自己完成。
//!
//! # 两种出口的延迟边界不同
//!
//! `_qmf` 出口已经包含 QMF 分析历史及 `5.7.6.5.3` 的
//! `delta_ASPX = ts_offset_hfgen` 个 QMF 时隙（每时隙 64 个样本），但尚未产生
//! 合成滤波器延迟。PCM 包装器才再加 `5.7.4` 的 QMF 合成，因此其输出相对输入
//! 滞后分析/合成滤波器组与 `delta_ASPX` 两部分。延迟判据直接量最终 PCM 的总和，
//! 不在测试里重抄两段各是多少——那样只会证明抄对了。

use crate::aspx::bands::AspxBandTables;
use crate::aspx::dequant::{DequantError, ScaleFactors, dequantise, dequantise_pair};
use crate::aspx::envelope::{
    EnvelopeDeltas, EnvelopeError, EnvelopeHistory, EnvelopeScaleFactors, SbgIndexMap,
    decode as decode_envelopes,
};
use crate::aspx::frames::{AspxInterval, MAX_ATSG_NOISE, MAX_ATSG_SIG};
use crate::aspx::hfadjust::{EnvelopeEstimate, HfAdjustError, SinePlacement, SineState, estimate};
use crate::aspx::hfassemble::{AssembleError, assemble};
use crate::aspx::hfgain::{AdjustedGains, GainError, LimiterMode, adjust};
use crate::aspx::hfgen::{HfGenError, hf_generate};
use crate::aspx::interleave::{InterleaveError, combine};
use crate::aspx::limiter::{LimiterError, LimiterTable};
use crate::aspx::lowband::{LowBandError, low_band};
use crate::aspx::noisegen::{NoiseError, generate as generate_noise};
use crate::aspx::patches::{PatchError, PatchTable};
use crate::aspx::preflatten::{PreFlattenError, PreFlattenGains, pre_flatten};
use crate::aspx::qmf::{
    QmfError, QmfSlot, analyse_ac4_pcm, analyse_ac4_pcm_pair, synthesise_ac4_pcm,
};
use crate::aspx::state::AspxChannelState;
use crate::aspx::syntax::{AspxChannelFraming, AspxConfig, AspxData, AspxEnvelopes};
use crate::aspx::tables::{
    EnvelopeKind, MAX_SBG_NOISE, NUM_QMF_SUBBANDS, num_ts_in_ats, qmf_timeslots_for_aspx_layout,
    ts_offset_hfgen,
};
use crate::aspx::tna::{
    ExtendedLowBand, TnaError, TnaFilters, chirp_factors, prediction_filters, validate_chirp_modes,
};
use crate::aspx::tonegen::{ToneError, generate as generate_tone};
use crate::aspx::workspace::{AspxWorkspace, MAX_EXTENDED_TIMESLOTS, MAX_QMF_TIMESLOTS};

/// 子带数，`5.7.3.2` 规定恒为 64，也是每时隙的样本数。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// 通路无法执行的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineError {
    /// 输入样本数不是 64 的整数倍。
    UnalignedInput { samples: usize },
    /// 输出样本数与输入不同。
    OutputLengthMismatch { expected: usize, provided: usize },
    /// 本帧时隙数超出工作区容量，即超出表 189 的最大一档。
    FrameTooLong { timeslots: usize, capacity: usize },
    /// 样本数虽按 64 对齐，但不是表 189/192 的八种合法帧长之一。
    UnsupportedFrameLength { samples: usize },
    /// QMF 分析或合成失败。
    Qmf(QmfError),
    /// `5.7.6.5.3` 的合并失败。
    Interleave(InterleaveError),
    /// `5.7.6.3.2` 的低带滤波失败。
    LowBand(LowBandError),
    /// `5.7.6.3.1.4` 的 patch 表推导失败。
    Patch(PatchError),
    /// `5.7.6.4.1.3` 的预测器或 chirp 因子失败。
    Tna(TnaError),
    /// `5.7.6.4.1.2` 的预平坦化失败。
    PreFlatten(PreFlattenError),
    /// `5.7.6.4.1.4` 的 HF 信号创建失败。
    HfGen(HfGenError),
    /// 噪声子带组数超出表 190 的上限，`sbg_noise` 装不下。
    TooManyNoiseGroups { groups: usize },
    /// 频带表声明的噪声子带组数与它能给出的边界数不符。
    MissingNoiseBorder { index: usize },
    /// 逐噪声子带组的 `aspx_tna_mode` 个数与频带表不符。
    ChirpModeCountMismatch { expected: usize, provided: usize },
    /// `5.7.6.3.4` 的包络解码失败。
    Envelope(EnvelopeError),
    /// `5.7.6.3.5` 的反量化失败。
    Dequant(DequantError),
    /// 区间声明的包络数与 `aspx_ec_data()`／`aspx_framing()` 给出的不符。
    ///
    /// 三处来源必须同时有第 `envelope` 个包络：差分符号、差分方向与频率分辨率。
    /// 缺任一处都在此报错，而不是按较短的一处截断——截断会让本区间少解一个
    /// 包络，后续按区间边界索引时错位，且没有任何一处会说明起因。
    EnvelopeDataMismatch { kind: EnvelopeKind, envelope: usize },
    /// `aspx_balance = 1` 的两声道没有共用同一份成帧与量化模式。
    BalancedFramingMismatch,
    /// `aspx_balance = 1` 的两声道 PCM 帧长不同。
    BalancedFrameLengthMismatch { first: usize, second: usize },
    /// 独立双声道的 PCM 帧长不同，无法共用垂直分析内核。
    PairedFrameLengthMismatch { first: usize, second: usize },
    /// `num_ts_in_ats` 不是表 192 定义的 1 或 2。
    TimeslotFactorOutOfRange { factor: u8 },
    /// 区间的名义时隙数与倍率不是表 189/192 的同一行。
    UnsupportedTimeslotLayout { num_aspx_timeslots: u8, factor: u8 },
    /// 区间来自另一种帧布局：它推出的 QMF 时隙数与本帧不符。
    IntervalFrameMismatch { expected: u8, frame: usize },
    /// 区间取不到该下标的信号边界。
    MissingIntervalBorder { index: usize },
    /// 区间的时隙范围为空或首尾颠倒。
    EmptyAspxInterval { first: i16, last: i16 },
    /// `5.7.6.4.2.1` 的包络估计失败。
    HfAdjust(HfAdjustError),
    /// `5.7.6.4.2.2` 的补偿增益失败。
    Gain(GainError),
    /// `5.7.6.3.1.5` 的限幅器表推导失败。
    Limiter(LimiterError),
    /// `5.7.6.4.3` 的噪声生成失败。
    Noise(NoiseError),
    /// `5.7.6.4.4` 的音调生成失败。
    Tone(ToneError),
    /// `5.7.6.4.5` 的高频组装失败。
    Assemble(AssembleError),
    /// `aspx_data_*()` 里没有这一路的解析结果。
    MissingChannel { channel: usize },
}

impl From<LowBandError> for PipelineError {
    fn from(error: LowBandError) -> Self {
        Self::LowBand(error)
    }
}

impl From<PatchError> for PipelineError {
    fn from(error: PatchError) -> Self {
        Self::Patch(error)
    }
}

impl From<TnaError> for PipelineError {
    fn from(error: TnaError) -> Self {
        Self::Tna(error)
    }
}

impl From<PreFlattenError> for PipelineError {
    fn from(error: PreFlattenError) -> Self {
        Self::PreFlatten(error)
    }
}

impl From<HfGenError> for PipelineError {
    fn from(error: HfGenError) -> Self {
        Self::HfGen(error)
    }
}

impl From<QmfError> for PipelineError {
    fn from(error: QmfError) -> Self {
        Self::Qmf(error)
    }
}

impl From<InterleaveError> for PipelineError {
    fn from(error: InterleaveError) -> Self {
        Self::Interleave(error)
    }
}

impl From<EnvelopeError> for PipelineError {
    fn from(error: EnvelopeError) -> Self {
        Self::Envelope(error)
    }
}

impl From<DequantError> for PipelineError {
    fn from(error: DequantError) -> Self {
        Self::Dequant(error)
    }
}

impl From<HfAdjustError> for PipelineError {
    fn from(error: HfAdjustError) -> Self {
        Self::HfAdjust(error)
    }
}

impl From<GainError> for PipelineError {
    fn from(error: GainError) -> Self {
        Self::Gain(error)
    }
}

impl From<LimiterError> for PipelineError {
    fn from(error: LimiterError) -> Self {
        Self::Limiter(error)
    }
}

impl From<NoiseError> for PipelineError {
    fn from(error: NoiseError) -> Self {
        Self::Noise(error)
    }
}

impl From<ToneError> for PipelineError {
    fn from(error: ToneError) -> Self {
        Self::Tone(error)
    }
}

impl From<AssembleError> for PipelineError {
    fn from(error: AssembleError) -> Self {
        Self::Assemble(error)
    }
}

/// A-SPX 的 `Y = 0` 参照通路，并在末尾执行 QMF 合成。
///
/// `pcm` 与 `out` 等长，且样本数必须是表 189/192 的八种合法帧长之一。
/// `ts_offset_hfgen` 由该帧长直接查表，不由调用方重复传入。`Y` 取零，故输出
/// 是延迟后的输入；延迟见模块文档。这里只用于活动 A-SPX 通路的接线诊断，
/// A-SPX 未启用的模式不应经由本函数引入 `delta_ASPX`。
///
/// 下游仍有 A-JOC 等 QMF 域工具时应调用 [`bypass_frame_qmf`]，不要先合成为 PCM
/// 再重新分析。
///
/// # Errors
///
/// 见 [`PipelineError`]。所有能由调用参数判断的错误都在状态与工作区改写前
/// 返回。通路开始后若某个内部工具仍意外失败，`state` 可能已被部分推进；调用
/// 方遇到这种错误应当 [`AspxChannelState::reset`] 后从随机访问点重新起解。
pub fn bypass_frame(
    pcm: &[f32],
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
    out: &mut [f32],
) -> Result<(), PipelineError> {
    let q_out = bypass_frame_impl(pcm, state, workspace, Some(out.len()))?;
    synthesise_ac4_pcm(q_out, &mut state.synthesis, out)?;
    Ok(())
}

/// A-SPX 的 `Y = 0` 参照通路，停在 `Q_out,ASPX`。
///
/// 返回切片可直接交给 A-JOC 或其他下游 QMF 域工具；它借用 `workspace`，在下次
/// 使用该工作区之前有效。本函数不执行 QMF 合成，也不推进
/// [`AspxChannelState::synthesis`]。
///
/// # Errors
///
/// 见 [`PipelineError`]。错误与状态契约同 [`bypass_frame`]。
pub fn bypass_frame_qmf<'a>(
    pcm: &[f32],
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
) -> Result<&'a [QmfSlot], PipelineError> {
    bypass_frame_impl(pcm, state, workspace, None)
}

fn bypass_frame_impl<'a>(
    pcm: &[f32],
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
    output_len: Option<usize>,
) -> Result<&'a [QmfSlot], PipelineError> {
    let (timeslots, ts_offset_hfgen) = prepare(pcm, output_len, workspace)?;

    let Some(q_in) = workspace.q_in.get_mut(..timeslots) else {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    analyse_ac4_pcm(pcm, &mut state.analysis, q_in)?;

    // `prepare_frame` 已把 `y` 清零，旁路路径不再写它。
    //
    // **旁路下这次调用仍是无操作**：它只清 `q_low` 与 `y`，而旁路两块都不写。
    // 但它在整条通路上已经不再是等价变体——`5.7.6.4.5` 只写
    // `[sbx, sbx + num_sb_aspx)`，范围外的残留会经下面的相加直接进输出。
    // 由 `the_low_band_of_y_is_cleared_every_frame` 锁住，那条待办已兑现。
    let (Some(q_in), Some(y), Some(q_out)) = (
        workspace.q_in.get(..timeslots),
        workspace.y.get(..timeslots),
        workspace.q_out.get_mut(..timeslots),
    ) else {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    combine(q_in, y, ts_offset_hfgen, &mut state.output, q_out)?;

    qmf_output(workspace, timeslots)
}

/// 两条通路共用的前置检查：核对长度、查出帧延迟、清理工作区。
///
/// 返回本帧时隙数与 `ts_offset_hfgen`。**所有能由调用参数判断的错误都在这里
/// 返回**，此时状态与工作区都还没被改写。
fn prepare(
    pcm: &[f32],
    output_len: Option<usize>,
    workspace: &mut AspxWorkspace,
) -> Result<(usize, u8), PipelineError> {
    let layout = validate_frame(pcm, output_len)?;
    workspace.prepare_frame();
    Ok(layout)
}

/// 只核对帧布局，不改写状态或工作区；双声道入口用它先验完两路再一起准备。
fn validate_frame(pcm: &[f32], output_len: Option<usize>) -> Result<(usize, u8), PipelineError> {
    if !pcm.len().is_multiple_of(SUBBANDS) {
        return Err(PipelineError::UnalignedInput { samples: pcm.len() });
    }
    if let Some(provided) = output_len
        && provided != pcm.len()
    {
        return Err(PipelineError::OutputLengthMismatch {
            expected: pcm.len(),
            provided,
        });
    }
    let timeslots = pcm.len() / SUBBANDS;
    if timeslots > MAX_QMF_TIMESLOTS {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    }
    let Ok(frame_len_base) = u16::try_from(pcm.len()) else {
        return Err(PipelineError::UnsupportedFrameLength { samples: pcm.len() });
    };
    let Some(delay) = ts_offset_hfgen(frame_len_base) else {
        return Err(PipelineError::UnsupportedFrameLength { samples: pcm.len() });
    };
    Ok((timeslots, delay))
}

/// 旁路加低带滤波：在 [`bypass_frame`] 的基础上多跑一步 `5.7.6.3.2`。
///
/// 结果与 [`bypass_frame`] **逐位相同**——低带滤波的产物 `Q_low` 是喂给高频
/// 生成器的，本身不进输出。加这一步是为了让两条延迟线开始同时推进：
/// `5.7.6.3.2` 的 `LowBandDelay` 与 `5.7.6.5.3` 的 `AspxOutputDelay` 都按
/// `ts_offset_hfgen` 延迟，却由不同模块各自维护，同步与否只能靠交叉判据看出。
///
/// # Errors
///
/// 见 [`PipelineError`]。
pub fn frame_with_low_band(
    pcm: &[f32],
    bands: &AspxBandTables,
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
    out: &mut [f32],
) -> Result<(), PipelineError> {
    let q_out = frame_with_low_band_impl(pcm, bands, state, workspace, Some(out.len()))?;
    synthesise_ac4_pcm(q_out, &mut state.synthesis, out)?;
    Ok(())
}

/// [`frame_with_low_band`] 的 QMF 域出口，不执行终端 QMF 合成。
///
/// 返回值与生命周期规则同 [`bypass_frame_qmf`]。
///
/// # Errors
///
/// 见 [`PipelineError`]。
pub fn frame_with_low_band_qmf<'a>(
    pcm: &[f32],
    bands: &AspxBandTables,
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
) -> Result<&'a [QmfSlot], PipelineError> {
    frame_with_low_band_impl(pcm, bands, state, workspace, None)
}

/// 在首份 QMF 控制数据到期之前预热分析、低带与输出历史。
///
/// `5.7.2` 要求控制数据按表 188 暂存整数个 codec frame；这段时间内已经到达的
/// frame-aligned PCM 仍必须推进 QMF 分析、`5.7.6.3.2` 的低带延迟、TNA 输入
/// 历史和 `5.7.6.5.3` 的输出延迟。否则第一份到期控制会从全零滤波器状态启动，
/// 把本应位于 priming 区间的瞬态泄漏到节目起点。
///
/// 本入口不解码任何包络，不生成 `Y`，也不推进 chirp、noise、tone、sine 或 HF
/// carry-over；返回的是延迟后的低带参照输出。调用方可以把它交给终端 QMF 合成，
/// 以同样预热合成滤波器历史。
///
/// # Errors
///
/// 见 [`PipelineError`]。错误后的状态契约同 [`frame_with_low_band_qmf`]。
pub fn prime_control_delay_qmf<'a>(
    pcm: &[f32],
    bands: &AspxBandTables,
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
) -> Result<&'a [QmfSlot], PipelineError> {
    let (timeslots, ts_offset) = prepare(pcm, None, workspace)?;
    let extended = analyse_and_filter_low_band(pcm, bands, state, workspace, timeslots, ts_offset)?;
    let Some(q_low) = workspace.q_low.get(..extended) else {
        return Err(PipelineError::FrameTooLong {
            timeslots: extended,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    state.tna.advance(q_low, timeslots)?;
    combine_to_qmf(state, workspace, timeslots, ts_offset)
}

fn frame_with_low_band_impl<'a>(
    pcm: &[f32],
    bands: &AspxBandTables,
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
    output_len: Option<usize>,
) -> Result<&'a [QmfSlot], PipelineError> {
    let (timeslots, ts_offset) = prepare(pcm, output_len, workspace)?;
    analyse_and_filter_low_band(pcm, bands, state, workspace, timeslots, ts_offset)?;
    combine_to_qmf(state, workspace, timeslots, ts_offset)
}

/// `5.7.6.4.1` 需要而频带表不提供的逐帧参数。
///
/// 这些值分别来自 `aspx_config()` 的 `master_freq_scale`、TOC 的 `fs_index` 与
/// `aspx_tna_data()`。调用方可以直接给出，也可以由 [`ChannelInputs::from_parsed`]
/// 从语法结果转接。
#[derive(Debug, Clone, Copy)]
pub struct HfGenParams<'a> {
    /// `aspx_master_freq_scale`。
    pub master_freq_scale: bool,
    /// 基础采样率是否为 48 kHz（TOC `fs_index == 1`）。
    pub base_samp_freq_48: bool,
    /// 逐噪声子带组的 `aspx_tna_mode`，个数必须等于 `num_sbg_noise`。
    pub chirp_modes: &'a [u8],
    /// 是否启用 `5.7.6.4.1.2` 的预平坦化。
    pub pre_flattening: bool,
}

/// 低带滤波再加 `5.7.6.4.1` 的 HF 信号创建。
///
/// ```text
/// PCM → QMF 分析 → Q_in ─┬──────────────────────────┐
///                        └→ 5.7.6.3.2 → Q_low ─┬→ 5.7.6.4.1.3 预测器 + chirp
///                                              ├→ 5.7.6.4.1.2 预平坦化
///                                              └→ 5.7.6.4.1.4 → Q_high
///                                                              ├→ 5.7.6.5.3 合并 → Q_out → 合成
///                                              Y ≡ 0 ──────────┘
/// ```
///
/// 输出仍与 [`bypass_frame`] **逐位相同**：`Q_high` 要经 `5.7.6.4.2`–`5.7.6.4.5`
/// 才汇入 `Y`，本步只把它算出来放进工作区。
///
/// # 整帧当作一个 A-SPX 区间
///
/// `first = 0`、`last = num_qmf_timeslots` 是临时占位。真实区间由 `5.7.6.3.3`
/// 的成帧给出，**每声道每帧一个**，通路里不存在区间循环。接语法之前写死成整帧，
/// 比让调用方传一对无处校验的下标更难出错。
///
/// 换成真实区间时这两个端点都会动：左边界 `atsg_sig[0] · num_ts_in_ats` 不必为
/// 零，右边界还可以越过帧尾（`aspx_var_bord_right`，见 [`super::hfassemble`]）。
///
/// # Errors
///
/// 见 [`PipelineError`]。patch 表、噪声边界与 chirp 个数都在推进任何状态之前
/// 核对；其余同 [`frame_with_low_band`]。
pub fn frame_with_hf_generation(
    pcm: &[f32],
    bands: &AspxBandTables,
    params: HfGenParams<'_>,
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
    out: &mut [f32],
) -> Result<(), PipelineError> {
    let q_out =
        frame_with_hf_generation_impl(pcm, bands, params, state, workspace, Some(out.len()))?;
    synthesise_ac4_pcm(q_out, &mut state.synthesis, out)?;
    Ok(())
}

/// [`frame_with_hf_generation`] 的 QMF 域出口，不执行终端 QMF 合成。
///
/// 返回值与生命周期规则同 [`bypass_frame_qmf`]。
///
/// # Errors
///
/// 见 [`PipelineError`]。
pub fn frame_with_hf_generation_qmf<'a>(
    pcm: &[f32],
    bands: &AspxBandTables,
    params: HfGenParams<'_>,
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
) -> Result<&'a [QmfSlot], PipelineError> {
    frame_with_hf_generation_impl(pcm, bands, params, state, workspace, None)
}

fn frame_with_hf_generation_impl<'a>(
    pcm: &[f32],
    bands: &AspxBandTables,
    params: HfGenParams<'_>,
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
    output_len: Option<usize>,
) -> Result<&'a [QmfSlot], PipelineError> {
    let (timeslots, ts_offset) = validate_frame(pcm, output_len)?;
    let tables = hf_gen_tables(bands, params)?;
    let aspx = HighBandInterval {
        first: 0,
        last: timeslots,
    };
    workspace.prepare_frame();
    generate_hf(
        pcm, bands, params, &tables, aspx, state, workspace, timeslots, ts_offset,
    )?;
    combine_to_qmf(state, workspace, timeslots, ts_offset)
}

/// 表 192 的 `num_ts_in_ats`，由帧长查出。
///
/// 帧长在 [`validate_frame`] 里已经核对过是表 189/192 的八种之一，这里再查一次
/// 只会得到同一行；查不到按不支持的帧长报，不假定倍率为 1。
fn frame_timeslot_factor(samples: usize) -> Result<u8, PipelineError> {
    let Ok(frame_len_base) = u16::try_from(samples) else {
        return Err(PipelineError::UnsupportedFrameLength { samples });
    };
    num_ts_in_ats(frame_len_base).ok_or(PipelineError::UnsupportedFrameLength { samples })
}

/// `Q_high` 与预平坦化共用的 QMF 时隙范围，左闭右开，相对本帧起点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HighBandInterval {
    first: usize,
    last: usize,
}

impl HighBandInterval {
    /// 时隙数，即 `Q_high` 应有的长度。
    const fn timeslots(self) -> usize {
        self.last.saturating_sub(self.first)
    }
}

/// 由 A-SPX 区间与帧布局算出 `Q_high` 覆盖的 QMF 时隙范围。
///
/// `Pseudocode 85` 与 `Pseudocode 89` 的 `ts` 都从 `atsg_sig[0] · num_ts_in_ats`
/// 起，止于 `atsg_sig[num_atsg_sig] · num_ts_in_ats`。
///
/// # 右端可以越过帧尾，而余量恰好够
///
/// 越帧量是 `aspx_var_bord_right · num_ts_in_ats`：表 53 给该字段 2 比特，故至
/// 多 3；倍率至多 2；上界因此是 6。而 `Q_low` 比本帧长 `ts_offset_hfgen` 个时
/// 隙，表 192 里倍率 1 恒配 3、倍率 2 恒配 6——两者取自**同一张表的同一行**，越
/// 帧部分总落在已经算出的 `Q_low_ext` 上。
///
/// 这个界是紧的：不是留了余量，而是那组配对使然。倍率与偏移若不再同步，
/// [`hf_generate`] 会报 `IntervalOutOfRange` 而不是静默读到未定义的时隙。
fn high_band_interval(
    interval: &AspxInterval,
    timeslots: usize,
    num_ts_in_ats: u8,
) -> Result<HighBandInterval, PipelineError> {
    if !matches!(num_ts_in_ats, 1 | 2) {
        return Err(PipelineError::TimeslotFactorOutOfRange {
            factor: num_ts_in_ats,
        });
    }
    // 可变右边界会让 `stop_pos` 大于名义时隙数，故不能从边界反推帧长；用区间
    // 自带的来源值与倍率重新配回表 189/192 的一行。
    let num_aspx_timeslots = interval.source_num_aspx_timeslots();
    let Some(expected) = qmf_timeslots_for_aspx_layout(num_aspx_timeslots, num_ts_in_ats) else {
        return Err(PipelineError::UnsupportedTimeslotLayout {
            num_aspx_timeslots,
            factor: num_ts_in_ats,
        });
    };
    if usize::from(expected) != timeslots {
        return Err(PipelineError::IntervalFrameMismatch {
            expected,
            frame: timeslots,
        });
    }

    let envelopes = usize::from(interval.num_atsg_sig());
    let (Some(first_border), Some(last_border)) =
        (interval.sig_border(0), interval.sig_border(envelopes))
    else {
        return Err(PipelineError::MissingIntervalBorder { index: envelopes });
    };
    if last_border <= first_border || first_border < 0 {
        return Err(PipelineError::EmptyAspxInterval {
            first: first_border,
            last: last_border,
        });
    }
    let factor = usize::from(num_ts_in_ats);
    let (Ok(first), Ok(last)) = (usize::try_from(first_border), usize::try_from(last_border))
    else {
        return Err(PipelineError::EmptyAspxInterval {
            first: first_border,
            last: last_border,
        });
    };
    let (Some(first), Some(last)) = (first.checked_mul(factor), last.checked_mul(factor)) else {
        return Err(PipelineError::EmptyAspxInterval {
            first: first_border,
            last: last_border,
        });
    };
    Ok(HighBandInterval { first, last })
}

/// `5.7.6.4.1` 里只由频带表与 [`HfGenParams`] 决定的三样东西。
#[derive(Debug)]
struct HfGenTables {
    patches: PatchTable,
    noise_borders: [u8; MAX_SBG_NOISE as usize + 1],
    noise_groups: usize,
}

impl HfGenTables {
    /// `sbg_noise`，长度为噪声子带组数加一。
    fn borders(&self) -> Result<&[u8], PipelineError> {
        self.noise_borders
            .get(..=self.noise_groups)
            .ok_or(PipelineError::MissingNoiseBorder {
                index: self.noise_groups,
            })
    }
}

/// 推出 patch 表与噪声子带组边界，并核对两处个数。
///
/// **必须排在 [`AspxWorkspace::prepare_frame`] 之前。** 这里的每一条都由调用参数
/// 决定，而 [`bypass_frame`] 的契约要求这类错误在改写状态**与工作区**之前返回；
/// 清过的工作区已经不是原样，调用方拿到错误后无从原地重试。
fn hf_gen_tables(
    bands: &AspxBandTables,
    params: HfGenParams<'_>,
) -> Result<HfGenTables, PipelineError> {
    let patches = PatchTable::derive(bands, params.master_freq_scale, params.base_samp_freq_48)?;
    let noise_groups = usize::from(bands.num_sbg_noise());
    if noise_groups == 0 || noise_groups > MAX_SBG_NOISE as usize {
        return Err(PipelineError::TooManyNoiseGroups {
            groups: noise_groups,
        });
    }
    if params.chirp_modes.len() != noise_groups {
        return Err(PipelineError::ChirpModeCountMismatch {
            expected: noise_groups,
            provided: params.chirp_modes.len(),
        });
    }
    validate_chirp_modes(params.chirp_modes)?;
    let mut noise_borders = [0u8; MAX_SBG_NOISE as usize + 1];
    for (index, slot) in noise_borders
        .iter_mut()
        .take(noise_groups.saturating_add(1))
        .enumerate()
    {
        *slot = bands
            .noise_border(index)
            .ok_or(PipelineError::MissingNoiseBorder { index })?;
    }
    let tables = HfGenTables {
        patches,
        noise_borders,
        noise_groups,
    };
    tables.borders()?;
    Ok(tables)
}

/// 分析、低带滤波与 `5.7.6.4.1` 的 HF 信号创建，止于 `Q_high` 就绪。
///
/// 抽出来是为了让后续通路能在**合并之前**插进 `5.7.6.3.4` 之后的各段：输出
/// 合并必须留在 A-SPX 最后，终端合成还要留到下游 QMF 域工具之后；`Q_high`
/// 一就绪就得让位给 `5.7.6.4.2`–`5.7.6.4.5`。
///
/// `aspx` 是 `Q_high` 覆盖的时隙范围。它与 `timeslots` 是两回事：前者可以从帧
/// 内某处起、并越过帧尾，后者恒是本帧的 QMF 时隙数，只用于低带滤波与延迟线。
///
/// 三张表由 [`hf_gen_tables`] 在工作区清理之前算好后传进来，本函数不再自己推。
#[expect(
    clippy::too_many_arguments,
    reason = "`5.7.6.4.1` 这一段本来就要这几路：三张表、区间、跨帧状态、帧内缓冲\
              与两个帧布局量。聚成结构体只是把同一组参数换个地方写"
)]
fn generate_hf(
    pcm: &[f32],
    bands: &AspxBandTables,
    params: HfGenParams<'_>,
    tables: &HfGenTables,
    aspx: HighBandInterval,
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
    timeslots: usize,
    ts_offset: u8,
) -> Result<(), PipelineError> {
    let extended = analyse_and_filter_low_band(pcm, bands, state, workspace, timeslots, ts_offset)?;
    generate_hf_from_low_band(
        bands, params, tables, aspx, state, workspace, timeslots, extended,
    )
}

/// 已有 `Q_low` 后执行 `5.7.6.4.1` 的其余步骤。
///
/// 分开这一层让双声道入口可以先用同一个垂直 SIMD 内核完成两路 QMF
/// 分析，再各自沿规范的低带、预测器与 HF 创建时间线前进。
#[expect(
    clippy::too_many_arguments,
    reason = "参数与 `generate_hf` 相同，只把 PCM/offset 换成已核对的 Q_low 长度"
)]
fn generate_hf_from_low_band(
    bands: &AspxBandTables,
    params: HfGenParams<'_>,
    tables: &HfGenTables,
    aspx: HighBandInterval,
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
    timeslots: usize,
    extended: usize,
) -> Result<(), PipelineError> {
    let HfGenTables {
        patches,
        noise_groups,
        ..
    } = tables;
    let noise_groups = *noise_groups;
    let noise_borders = tables.borders()?;

    let mut chirp = [0.0f32; MAX_SBG_NOISE as usize];
    let Some(chirp) = chirp.get_mut(..noise_groups) else {
        return Err(PipelineError::TooManyNoiseGroups {
            groups: noise_groups,
        });
    };
    chirp_factors(params.chirp_modes, &mut state.chirp, chirp)?;

    let Some(q_low) = workspace.q_low.get(..extended) else {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    // `Q_low_ext` 要拿到完整的 `timeslots + ts_offset_hfgen`——`prediction_filters`
    // 由它与 `num_qmf_timeslots` 反算 `ts_offset_hfgen` 并要求落在 {3, 6}，
    // 切错长度会直接报 `UnsupportedOffset`，不会静默算出错的协方差。
    let ext = ExtendedLowBand::new(&state.tna, q_low);

    let mut filters = TnaFilters::new();
    prediction_filters(ext, timeslots, bands.sba(), &mut filters)?;

    let mut flatten = PreFlattenGains::new();
    let gains = if params.pre_flattening {
        pre_flatten(
            q_low,
            usize::from(bands.sbx()),
            aspx.first,
            aspx.last,
            &mut flatten,
        )?;
        Some(&flatten)
    } else {
        None
    };

    let Some(q_high) = workspace.q_high.get_mut(..aspx.timeslots()) else {
        return Err(PipelineError::FrameTooLong {
            timeslots: aspx.timeslots(),
            capacity: MAX_EXTENDED_TIMESLOTS,
        });
    };
    hf_generate(
        ext,
        patches,
        &filters,
        chirp,
        noise_borders,
        gains,
        aspx.first,
        aspx.last,
        q_high,
    )?;

    // `Pseudocode 71` 会把不足三个子带的最后一段 patch 丢掉，此时
    // `Pseudocode 89` 不会覆写 `[patch_end, sbz)`。工作区跨帧复用，若不在
    // 成功生成后把这段定义为零，HF 调整就会把上一帧残值当成本帧估计谱。
    let patch_end = patches
        .border(usize::from(patches.count()))
        .map_or(usize::from(bands.sbx()), usize::from);
    let sbz = usize::from(bands.sbz());
    for slot in q_high {
        for sample in slot.re.iter_mut().take(sbz).skip(patch_end) {
            *sample = 0.0;
        }
        for sample in slot.im.iter_mut().take(sbz).skip(patch_end) {
            *sample = 0.0;
        }
    }

    // 两处消费（预测器与 HF 创建）都取完样之后才推进延迟线，见 `TnaDelay`。
    state.tna.advance(q_low, timeslots)?;
    Ok(())
}

/// `5.7.6.3.4` 与 `5.7.6.3.5` 要用的逐帧逐声道输入。
///
/// 与 [`HfGenParams`] 不同，这里直接收两个**已解析的结构**而不是拆散的标量。
/// 那边的四个值分别来自 `aspx_config()`、TOC 与 `aspx_tna_data()` 三处，凑成
/// 一个结构本身就是接线的一部分；这边的区间、逐包络方向与差分符号都是
/// `aspx_framing()`/`aspx_ec_data()` 一次解析的产物，拆开重传只会多出「区间说
/// 有三个包络、方向数组只给了两个」这类本来不可能发生的失配。
#[derive(Debug, Clone, Copy)]
pub struct EnvelopeInput<'a> {
    /// `aspx_framing()` 的结果，同时给出本区间与逐包络的差分方向。
    pub framing: &'a AspxChannelFraming,
    /// `aspx_ec_data()` 解出的差分符号。
    pub data: &'a AspxEnvelopes,
}

/// 在临时历史上完成 `5.7.6.3.4`，使包络错误不会污染已经接入通路的其他状态。
fn decode_envelope_input(
    bands: &AspxBandTables,
    envelopes: EnvelopeInput<'_>,
    delta_is_two: bool,
    history: EnvelopeHistory,
) -> Result<(EnvelopeHistory, EnvelopeScaleFactors), PipelineError> {
    let map = SbgIndexMap::derive(bands)?;
    let interval = &envelopes.framing.interval;

    const EMPTY: EnvelopeDeltas<'static> = EnvelopeDeltas {
        data: &[],
        time_direction: false,
        high_resolution: false,
    };
    let mut sig = [EMPTY; MAX_ATSG_SIG];
    let num_sig = usize::from(interval.num_atsg_sig());
    for (index, slot) in sig.iter_mut().take(num_sig).enumerate() {
        let missing = PipelineError::EnvelopeDataMismatch {
            kind: EnvelopeKind::Signal,
            envelope: index,
        };
        *slot = EnvelopeDeltas {
            data: envelopes.data.sig_slice(index).ok_or(missing)?,
            time_direction: envelopes.framing.sig_delta_dir(index).ok_or(missing)?,
            // 逐包络的频率分辨率只有区间知道；`aspx_ec_data()` 存的组数是它的
            // 结果而非来源，拿组数反推分辨率在两档组数相等时会失效。
            high_resolution: interval.freq_res(index).ok_or(missing)?,
        };
    }
    let Some(sig) = sig.get(..num_sig) else {
        return Err(PipelineError::EnvelopeDataMismatch {
            kind: EnvelopeKind::Signal,
            envelope: num_sig,
        });
    };

    let mut noise = [EMPTY; MAX_ATSG_NOISE];
    let num_noise = usize::from(interval.num_atsg_noise());
    for (index, slot) in noise.iter_mut().take(num_noise).enumerate() {
        let missing = PipelineError::EnvelopeDataMismatch {
            kind: EnvelopeKind::Noise,
            envelope: index,
        };
        *slot = EnvelopeDeltas {
            data: envelopes.data.noise_slice(index).ok_or(missing)?,
            time_direction: envelopes.framing.noise_delta_dir(index).ok_or(missing)?,
            high_resolution: false,
        };
    }
    let Some(noise) = noise.get(..num_noise) else {
        return Err(PipelineError::EnvelopeDataMismatch {
            kind: EnvelopeKind::Noise,
            envelope: num_noise,
        });
    };

    let mut next_history = history;
    let mut qscf = EnvelopeScaleFactors::new();
    decode_envelopes(sig, noise, &map, delta_is_two, &mut next_history, &mut qscf)?;
    Ok((next_history, qscf))
}

/// `5.7.6.4.2`–`5.7.6.4.4` 需要而前几段不提供的逐帧参数。
#[derive(Debug, Clone, Copy)]
pub struct HfAdjustParams<'a> {
    /// `aspx_add_harmonic`，逐高分辨率信号子带组一项。
    ///
    /// 传**解析出的那一段**，不是定长数组的全长：`5.7.6.4.2.1` 用
    /// `add_harmonic.len() < num_sbg_sig_highres` 判断解析与频带表是否脱节，
    /// 递整条数组过去会让那条检查永不触发。
    pub add_harmonic: &'a [bool],
    /// `aspx_interpolation`。
    pub interpolation: bool,
    /// `aspx_limiter`：假时只到 `Pseudocode 95`，真时走 `96`–`101`。
    pub limiter: bool,
    /// `5.7.6.3.1.1` 的 `master_reset`，供 `5.7.6.4.3` 的噪声表游标。
    ///
    /// `aspx_master_freq_scale`、`aspx_start_freq` 或 `aspx_stop_freq` 相对上一个
    /// I 帧变化时为真。帧函数不自行推进配置历史；调用方应把同一份
    /// [`MasterResetDecision`] 的结果交给该帧全部声道，成功后再提交判定。
    pub master_reset: bool,
    /// `5.7.6.4.4` 的 `first_frame`：**只在编解码初始化时**为真。
    ///
    /// 与 `master_reset` 不是同一个条件，正文分别定义，不要合并成一个标志。
    pub first_frame: bool,
}

/// 核对 `5.7.6.4.2` 的逐帧参数，并在启用时提前推出限幅器表。
///
/// 与 [`hf_gen_tables`] 一样，本函数必须在工作区清理与跨帧状态推进之前调用：这些
/// 结果都只由调用参数和频带表决定，失败后调用方应能原地修正并重试。
fn hf_adjust_limiter(
    bands: &AspxBandTables,
    tables: &HfGenTables,
    params: HfAdjustParams<'_>,
) -> Result<Option<LimiterTable>, PipelineError> {
    let needed = usize::from(bands.num_sbg_sig_highres());
    if params.add_harmonic.len() < needed {
        return Err(HfAdjustError::HarmonicDataTooShort {
            needed,
            provided: params.add_harmonic.len(),
        }
        .into());
    }
    params
        .limiter
        .then(|| LimiterTable::derive(bands, &tables.patches))
        .transpose()
        .map_err(Into::into)
}

/// 核对本区间的前后携带量，以及上一帧留下的高频成品是否恰好覆盖前缀。
///
/// 三项都只依赖区间、帧布局与跨帧状态，必须在工作区清理与任何状态推进之前执行；
/// 否则调用方补上起解点的前置静音后，也无法安全地用原帧重试。
fn validate_hf_carryover(
    aspx: HighBandInterval,
    state: &AspxChannelState,
    num_ts_in_ats: u8,
    num_qmf_timeslots: usize,
) -> Result<(), PipelineError> {
    // 区间右界短于帧长这一条**没能构造出可达的输入**：四个间隔类别里 FIXFIX 与
    // VARFIX 的终止边界恒等于名义时隙数，FIXVAR 与 VARVAR 则是它加上非负的
    // `aspx_var_bord_right`。注入它不会有判据响，属纵深防御而非判据缺口。
    // 逐类核对得出的结论，不是穷举证明——若日后新增间隔类别要回来重看。
    let Some(trailing_carryover) = aspx.last.checked_sub(num_qmf_timeslots) else {
        return Err(AssembleError::TimeslotsBeyondInterval {
            timeslots: num_qmf_timeslots,
            stop: aspx.last,
        }
        .into());
    };
    // `aspx_var_bord_left/right` 都是 2 比特，故各自至多 3 个 ATS；换到
    // QMF 时间轴后再乘表 192 的倍率。`assemble` 保留同一条防御性复核。
    let max_carryover = usize::from(num_ts_in_ats).saturating_mul(3);
    for carryover in [aspx.first, trailing_carryover] {
        if carryover > max_carryover {
            return Err(AssembleError::CarryoverOutOfRange { carryover }.into());
        }
    }
    let carried = usize::from(state.hf.carried());
    if carried != aspx.first {
        return Err(AssembleError::CarryoverMismatch {
            carried,
            required: aspx.first,
        }
        .into());
    }
    Ok(())
}

/// `5.7.6.4.3`–`5.7.6.4.5`：噪声、音调与组装，把 `Y` 写进工作区。
///
/// 三段共用同一条时间轴：`Q_high`、`qmf_noise` 与 `qmf_sine` 的第 0 项都对应
/// `atsg_sig[0] · num_ts_in_ats`，而 `Y` 的第 0 项对应时隙 0——差出来的前缀由
/// `5.7.6.4.5` 从 [`AspxChannelState::hf`] 取回，那是上一帧越过帧尾的成品。
///
/// 两个游标先在副本上推进，组装成功后才提交。入口已用 [`validate_hf_carryover`]
/// 前置核对携带量；`assemble` 仍会防御性复核，任何失败都不该让两张表走掉一段。
///
/// **这条顺序目前没有判据护着。** 携带量的三项检查前移之后，我逐条核对了
/// `NoiseError`、`ToneError` 与 `AssembleError` 的全部变体，没能构造出在这里
/// 仍然可达的失败：包络数、频带布局、区间来源、倍率与三路缓冲长度都已在前面
/// 定死，而 `AspxInterval::derive` 的单调性检查排除了空包络。因此「提交排到组
/// 装之前」是等价变体，注入它不会有判据响——那不是判据缺口。这里仍按事务顺序
/// 写，是因为下一个失败模式一旦出现，顺序错了就会静默丢帧；**新增失败路径时
/// 要回来给这条补判据**。
#[expect(
    clippy::too_many_arguments,
    reason = "三段伪码的输入合起来就是这几路：增益、两张表的来源、逐帧参数、区间、\
              帧布局、跨帧状态与帧内缓冲"
)]
fn assemble_high_band(
    gains: &AdjustedGains,
    bands: &AspxBandTables,
    interval: &AspxInterval,
    params: HfAdjustParams<'_>,
    aspx: HighBandInterval,
    layout: (u8, u8),
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
) -> Result<(), PipelineError> {
    let (num_ts_in_ats, num_qmf_timeslots) = layout;
    let body = aspx.timeslots();
    let AspxWorkspace {
        q_high,
        noise,
        sine,
        y,
        ..
    } = workspace;
    let (Some(q_high), Some(noise), Some(sine), Some(y)) = (
        q_high.get(..body),
        noise.get_mut(..body),
        sine.get_mut(..body),
        y.get_mut(..aspx.last),
    ) else {
        return Err(PipelineError::FrameTooLong {
            timeslots: aspx.last,
            capacity: MAX_EXTENDED_TIMESLOTS,
        });
    };

    let mut next_noise = state.noise;
    let mut next_tone = state.tone;
    generate_noise(
        gains,
        bands,
        interval,
        num_ts_in_ats,
        params.master_reset,
        &mut next_noise,
        noise,
    )?;
    generate_tone(
        gains,
        bands,
        interval,
        num_ts_in_ats,
        params.first_frame,
        &mut next_tone,
        sine,
    )?;
    // `Pseudocode 107`/`108` 把两者依次加到同一个累加器上，因此**对调这两路
    // 是等价变体**：`a + n + s` 与 `a + s + n` 只差一次加法的舍入顺序，注入它
    // 不会有判据响，那不是判据缺口。两路的语义差别在生成侧，由各自的判据管。
    assemble(
        gains,
        bands,
        interval,
        num_ts_in_ats,
        num_qmf_timeslots,
        q_high,
        noise,
        sine,
        &mut state.hf,
        y,
    )?;
    state.noise = next_noise;
    state.tone = next_tone;
    Ok(())
}

/// 一帧尚未提交的 `master_reset` 判定。
///
/// 同一份判定可以复制给该帧的所有 A-SPX 声道，也可以在修正前置错误后原样重试；
/// 只有 [`MasterResetTracker::commit`] 才推进配置历史。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasterResetDecision {
    current: Option<(bool, u8, u8)>,
    reset: bool,
}

impl MasterResetDecision {
    /// 本帧是否设置 `master_reset`。
    #[must_use]
    pub const fn is_reset(self) -> bool {
        self.reset
    }
}

/// `5.7.6.3.1.1` 的 `master_reset` 判据所需的最小已提交历史。
///
/// 正文只说三个字段「相对上一个 I 帧」变化时为真，**没有定义没有上一个 I 帧时
/// 取什么**。起解首帧两种读法不可区分：取真时 `Pseudocode 103` 的基址是 0，而
/// 全新的 `NoiseCursor` 留下的 `previous` 同样是 0。此处取真，并把这条不可区分
/// 性记在这里，免得后来者以为选了一边就排除了另一边。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MasterResetTracker {
    previous: Option<(bool, u8, u8)>,
}

impl MasterResetTracker {
    /// 尚未见过任何 I 帧。
    #[must_use]
    pub const fn new() -> Self {
        Self { previous: None }
    }

    /// 算出本帧的 `master_reset`，但不推进历史；`config` 只在 I 帧有值。
    ///
    /// 非 I 帧不重传 `aspx_config()`，三个字段按定义就是上一个 I 帧那份，因此
    /// 恒不变——不是「没查」，是查了必然相等。同一帧的全部声道与失败重试必须
    /// 复用返回的判定；整帧成功后再调用 [`Self::commit`]。
    #[must_use]
    pub fn frame(&self, config: Option<&AspxConfig>) -> MasterResetDecision {
        let current = config.map(|config| {
            (
                config.master_freq_scale,
                config.start_freq,
                config.stop_freq,
            )
        });
        MasterResetDecision {
            current,
            reset: current.is_some() && self.previous != current,
        }
    }

    /// 在整帧所有 A-SPX 声道都成功后提交这次判定。
    ///
    /// 非 I 帧的判定不带配置，提交是无操作。失败的帧不要提交，修正输入后仍可复用
    /// 同一个 [`MasterResetDecision`]。
    pub fn commit(&mut self, decision: MasterResetDecision) {
        if let Some(current) = decision.current {
            self.previous = Some(current);
        }
    }
}

/// 一条声道在本帧的全部通路输入。
#[derive(Debug, Clone, Copy)]
pub struct ChannelInputs<'a> {
    /// `5.7.6.4.1` 的逐帧参数。
    pub hf_gen: HfGenParams<'a>,
    /// `5.7.6.4.2`–`5.7.6.4.4` 的逐帧参数。
    pub hf_adjust: HfAdjustParams<'a>,
    /// `5.7.6.3.4`/`5.7.6.3.5` 的输入。
    pub envelopes: EnvelopeInput<'a>,
}

impl<'a> ChannelInputs<'a> {
    /// 从 `aspx_config()` 与 `aspx_data_*()` 的解析结果取出一条声道的输入。
    ///
    /// 只做取值与转接，不做判读：`base_samp_freq_48` 来自 TOC 的 `fs_index`，
    /// `master_reset` 取自 [`MasterResetDecision::is_reset`]，`first_frame` 是解码器
    /// 初始化标志——三者都不在 `aspx_data_*()` 里，只能由调用方带进来。
    ///
    /// # Errors
    ///
    /// `channel` 超出本元素的声道数，或某一路解析结果缺项时返回
    /// [`PipelineError::MissingChannel`]。
    pub fn from_parsed(
        data: &'a AspxData,
        config: &AspxConfig,
        channel: usize,
        base_samp_freq_48: bool,
        master_reset: bool,
        first_frame: bool,
    ) -> Result<Self, PipelineError> {
        // 三个访问器各自都以 `channels` 为界，越界一律给 `None`，因此这里不再
        // 单独挡一次：多一层只会让针对它的注入被另外几层兜住，谁也验不出来。
        let missing = PipelineError::MissingChannel { channel };
        let hfgen = data.hfgen(channel).ok_or(missing)?;
        Ok(Self {
            hf_gen: HfGenParams {
                master_freq_scale: config.master_freq_scale,
                base_samp_freq_48,
                chirp_modes: hfgen.tna_mode_slice().ok_or(missing)?,
                pre_flattening: config.preflat,
            },
            hf_adjust: HfAdjustParams {
                add_harmonic: hfgen.add_harmonic_slice().ok_or(missing)?,
                interpolation: config.interpolation,
                limiter: config.limiter,
                master_reset,
                first_frame,
            },
            envelopes: EnvelopeInput {
                framing: data.framing(channel).ok_or(missing)?,
                data: data.envelopes(channel).ok_or(missing)?,
            },
        })
    }
}

/// 完整通路对外保留的诊断中间量。
///
/// 三项都已在同一次调用中被下游消费；仍递出来是为了逐段核对包络解码、估计与补偿
/// 增益，而不必从最终 PCM 反推内部接线。
#[derive(Debug, Default)]
pub struct AspxIntermediates {
    /// `5.7.6.3.5` 的线性标度因子。
    pub scale_factors: ScaleFactors,
    /// `5.7.6.4.2.1` 的七张「子带 × 包络」矩阵。
    pub estimate: EnvelopeEstimate,
    /// `5.7.6.4.2.2` 的补偿增益。
    pub gains: AdjustedGains,
}

/// `5.7.6.4.2` 的两步：包络估计与补偿增益。
///
/// 全部结果先落在局部量上，成功后由调用方一次性提交——`estimate` 会推进
/// [`AspxChannelState::sine`]，因此 `sines` 收的是副本，返回推进后的新值。
#[expect(
    clippy::too_many_arguments,
    reason = "`5.7.6.4.2` 的输入就是这几路：`Q_high`、两张表的来源、两组逐帧参数、\
              上一段的标度因子、帧布局倍率与跨帧的正弦延续"
)]
fn adjust_high_band(
    q_high: &[QmfSlot],
    bands: &AspxBandTables,
    tables: &HfGenTables,
    input: EnvelopeInput<'_>,
    params: HfAdjustParams<'_>,
    limiter: Option<&LimiterTable>,
    scale_factors: &ScaleFactors,
    num_ts_in_ats: u8,
    sines: SineState,
) -> Result<(SineState, EnvelopeEstimate, AdjustedGains), PipelineError> {
    let mut next_sines = sines;
    let mut estimated = EnvelopeEstimate::new();
    estimate(
        q_high,
        bands,
        &input.framing.interval,
        scale_factors,
        params.add_harmonic,
        SinePlacement::from_params(input.framing.params.tsg_ptr),
        params.interpolation,
        num_ts_in_ats,
        &mut next_sines,
        &mut estimated,
    )?;

    // `aspx_limiter == 0` 时没有表：表 122 规定那种流只执行 `Pseudocode 95`。
    let mode = match limiter {
        Some(table) => LimiterMode::On {
            table,
            patches: &tables.patches,
        },
        None => LimiterMode::Off,
    };
    let mut gains = AdjustedGains::new();
    adjust(&estimated, bands, mode, &mut gains)?;
    Ok((next_sines, estimated, gains))
}

/// 单声道／独立双声道的完整 A-SPX 参数通路。
///
/// ```text
/// PCM → QMF 分析 → Q_low → Q_high ────────────────┐
/// framing/ec_data → qscf → scale_factors → gains ├→ 噪声/音调/组装 → Y
/// 延迟后的 Q_in ─────────────────────────────────┴→ 合并 → Q_out → 合成
/// ```
///
/// 输出是 [`bypass_frame`] 的延迟输入加上 `5.7.6.4.5` 组装出的 `Y`；三段诊断结果
/// 另通过 [`AspxIntermediates`] 递出。
/// `aspx_balance = 1` 必须改用 [`frame_with_balanced_scale_factors`]，因为
/// `Pseudocode 84` 要把两声道的 `qscf` 放在一起反量化。
///
/// # 区间与 `Q_high` 走同一条时间轴
///
/// 包络侧的包络数与逐包络分辨率、`Q_high` 与预平坦化的起止，都取自 `framing`
/// 里那一个区间。`Q_high[0]` 因此对应 `atsg_sig[0] · num_ts_in_ats`，正是
/// `5.7.6.4.2.1` 索引它时假定的基准。
///
/// 右边界可以越过帧尾，越帧量的上界与 `Q_low` 的延长量取自表 192 的同一行，因
/// 此恰好够用；这条配对关系写在 `high_band_interval` 的文档里。
///
/// # Errors
///
/// 见 [`PipelineError`]。包络侧只依赖入参的检查（子带组映射、区间与差分数据的
/// 一致性）以及高频成品携带量的核对，都在工作区清理之前完成，此时状态也还没动。
#[expect(
    clippy::too_many_arguments,
    reason = "单声道通路要同时接收 HF 生成、包络、HF 调整、三组可变状态与输出；\
              聚成上下文结构只会把这一层的真实接线藏起来"
)]
pub fn frame_with_scale_factors(
    pcm: &[f32],
    bands: &AspxBandTables,
    params: HfGenParams<'_>,
    envelopes: EnvelopeInput<'_>,
    hf_adjust: HfAdjustParams<'_>,
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
    intermediates: &mut AspxIntermediates,
    out: &mut [f32],
) -> Result<(), PipelineError> {
    let q_out = frame_with_scale_factors_impl(
        pcm,
        bands,
        params,
        envelopes,
        hf_adjust,
        state,
        workspace,
        intermediates,
        Some(out.len()),
    )?;
    synthesise_ac4_pcm(q_out, &mut state.synthesis, out)?;
    Ok(())
}

/// [`frame_with_scale_factors`] 的规范 QMF 域出口。
///
/// 返回的 `Q_out,ASPX` 可直接交给 A-JOC 或其他后续 QMF 域工具。本函数不执行
/// QMF 合成，也不推进 [`AspxChannelState::synthesis`]；返回切片的生命周期规则
/// 同 [`bypass_frame_qmf`]。
///
/// # Errors
///
/// 见 [`PipelineError`]。
#[expect(
    clippy::too_many_arguments,
    reason = "QMF 出口与 PCM 包装器接收同一组规范输入，只省去终端输出缓冲"
)]
pub fn frame_with_scale_factors_qmf<'a>(
    pcm: &[f32],
    bands: &AspxBandTables,
    params: HfGenParams<'_>,
    envelopes: EnvelopeInput<'_>,
    hf_adjust: HfAdjustParams<'_>,
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
    intermediates: &mut AspxIntermediates,
) -> Result<&'a [QmfSlot], PipelineError> {
    frame_with_scale_factors_impl(
        pcm,
        bands,
        params,
        envelopes,
        hf_adjust,
        state,
        workspace,
        intermediates,
        None,
    )
}

/// 两条互相独立的单声道参数通路，共用一次垂直 QMF 分析。
///
/// 这不是 `aspx_balance = 1`：两路各自解包络、反量化并使用自己的频带表与状态，
/// 只有逐 PCM 样本完全同构的 QMF 分析并排执行。输出与分别调用两次
/// [`frame_with_scale_factors_qmf`] 逐位相同。
///
/// # Errors
///
/// 两路帧长必须相同。所有只依赖输入的检查都在任一路状态或工作区改写之前完成；
/// 进入通路后的错误契约与单声道入口相同。
#[expect(
    clippy::too_many_arguments,
    reason = "独立双声道入口的数组分别对应逐声道输入、状态、工作区与输出"
)]
pub fn frame_with_scale_factors_pair_qmf<'a>(
    pcm: [&[f32]; 2],
    bands: [&AspxBandTables; 2],
    params: [HfGenParams<'_>; 2],
    envelopes: [EnvelopeInput<'_>; 2],
    hf_adjust: [HfAdjustParams<'_>; 2],
    states: [&mut AspxChannelState; 2],
    workspaces: [&'a mut AspxWorkspace; 2],
    intermediates: [&mut AspxIntermediates; 2],
) -> Result<[&'a [QmfSlot]; 2], PipelineError> {
    let [first_pcm, second_pcm] = pcm;
    let [first_bands, second_bands] = bands;
    let [first_params, second_params] = params;
    let [first_envelopes, second_envelopes] = envelopes;
    let [first_adjust, second_adjust] = hf_adjust;
    let [first_state, second_state] = states;
    let [first_workspace, second_workspace] = workspaces;
    let [first_intermediates, second_intermediates] = intermediates;

    if first_pcm.len() != second_pcm.len() {
        return Err(PipelineError::PairedFrameLengthMismatch {
            first: first_pcm.len(),
            second: second_pcm.len(),
        });
    }

    let (next_first_history, first_qscf) =
        decode_envelope_input(first_bands, first_envelopes, false, first_state.envelope)?;
    let (next_second_history, second_qscf) =
        decode_envelope_input(second_bands, second_envelopes, false, second_state.envelope)?;
    let mut next_first_factors = ScaleFactors::new();
    let mut next_second_factors = ScaleFactors::new();
    dequantise(
        &first_qscf,
        first_envelopes.framing.qmode_env,
        &mut next_first_factors,
    )?;
    dequantise(
        &second_qscf,
        second_envelopes.framing.qmode_env,
        &mut next_second_factors,
    )?;

    let (first_timeslots, first_offset) = validate_frame(first_pcm, None)?;
    let (second_timeslots, second_offset) = validate_frame(second_pcm, None)?;
    let first_tables = hf_gen_tables(first_bands, first_params)?;
    let second_tables = hf_gen_tables(second_bands, second_params)?;
    let first_factor = frame_timeslot_factor(first_pcm.len())?;
    let second_factor = frame_timeslot_factor(second_pcm.len())?;
    let first_aspx = high_band_interval(
        &first_envelopes.framing.interval,
        first_timeslots,
        first_factor,
    )?;
    let second_aspx = high_band_interval(
        &second_envelopes.framing.interval,
        second_timeslots,
        second_factor,
    )?;
    let first_limiter = hf_adjust_limiter(first_bands, &first_tables, first_adjust)?;
    let second_limiter = hf_adjust_limiter(second_bands, &second_tables, second_adjust)?;
    validate_hf_carryover(first_aspx, first_state, first_factor, first_timeslots)?;
    validate_hf_carryover(second_aspx, second_state, second_factor, second_timeslots)?;

    first_workspace.prepare_frame();
    second_workspace.prepare_frame();
    let [first_extended, second_extended] = analyse_and_filter_low_band_pair(
        [first_pcm, second_pcm],
        [first_bands, second_bands],
        [&mut *first_state, &mut *second_state],
        [&mut *first_workspace, &mut *second_workspace],
        first_timeslots,
        [first_offset, second_offset],
    )?;
    generate_hf_from_low_band(
        first_bands,
        first_params,
        &first_tables,
        first_aspx,
        first_state,
        first_workspace,
        first_timeslots,
        first_extended,
    )?;
    let first_qmf = {
        let first_q_high = first_workspace.q_high.get(..first_aspx.timeslots()).ok_or(
            PipelineError::FrameTooLong {
                timeslots: first_aspx.timeslots(),
                capacity: MAX_EXTENDED_TIMESLOTS,
            },
        )?;
        let (next_first_sines, first_estimate, first_gains) = adjust_high_band(
            first_q_high,
            first_bands,
            &first_tables,
            first_envelopes,
            first_adjust,
            first_limiter.as_ref(),
            &next_first_factors,
            first_factor,
            first_state.sine,
        )?;
        let first_qmf_timeslots =
            u8::try_from(first_timeslots).map_err(|_| PipelineError::FrameTooLong {
                timeslots: first_timeslots,
                capacity: MAX_QMF_TIMESLOTS,
            })?;
        assemble_high_band(
            &first_gains,
            first_bands,
            &first_envelopes.framing.interval,
            first_adjust,
            first_aspx,
            (first_factor, first_qmf_timeslots),
            first_state,
            first_workspace,
        )?;
        first_state.envelope = next_first_history;
        first_state.sine = next_first_sines;
        first_intermediates.scale_factors = next_first_factors;
        first_intermediates.estimate = first_estimate;
        first_intermediates.gains = first_gains;
        combine_to_qmf(first_state, first_workspace, first_timeslots, first_offset)?
    };

    // 第一声道的大型包络矩阵已经移入诊断输出；再处理第二声道，让两路临时矩阵
    // 共用同一段栈，而不是同时存活到函数末尾。
    generate_hf_from_low_band(
        second_bands,
        second_params,
        &second_tables,
        second_aspx,
        second_state,
        second_workspace,
        second_timeslots,
        second_extended,
    )?;
    let second_q_high = second_workspace
        .q_high
        .get(..second_aspx.timeslots())
        .ok_or(PipelineError::FrameTooLong {
            timeslots: second_aspx.timeslots(),
            capacity: MAX_EXTENDED_TIMESLOTS,
        })?;
    let (next_second_sines, second_estimate, second_gains) = adjust_high_band(
        second_q_high,
        second_bands,
        &second_tables,
        second_envelopes,
        second_adjust,
        second_limiter.as_ref(),
        &next_second_factors,
        second_factor,
        second_state.sine,
    )?;
    let second_qmf_timeslots =
        u8::try_from(second_timeslots).map_err(|_| PipelineError::FrameTooLong {
            timeslots: second_timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        })?;
    assemble_high_band(
        &second_gains,
        second_bands,
        &second_envelopes.framing.interval,
        second_adjust,
        second_aspx,
        (second_factor, second_qmf_timeslots),
        second_state,
        second_workspace,
    )?;
    second_state.envelope = next_second_history;
    second_state.sine = next_second_sines;
    second_intermediates.scale_factors = next_second_factors;
    second_intermediates.estimate = second_estimate;
    second_intermediates.gains = second_gains;
    let second_qmf = combine_to_qmf(
        second_state,
        second_workspace,
        second_timeslots,
        second_offset,
    )?;
    Ok([first_qmf, second_qmf])
}

#[expect(
    clippy::too_many_arguments,
    reason = "内部实现同时服务 QMF 出口与带输出长度先验检查的 PCM 包装器"
)]
fn frame_with_scale_factors_impl<'a>(
    pcm: &[f32],
    bands: &AspxBandTables,
    params: HfGenParams<'_>,
    envelopes: EnvelopeInput<'_>,
    hf_adjust: HfAdjustParams<'_>,
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
    intermediates: &mut AspxIntermediates,
    output_len: Option<usize>,
) -> Result<&'a [QmfSlot], PipelineError> {
    let (next_history, qscf) = decode_envelope_input(bands, envelopes, false, state.envelope)?;
    let mut next_factors = ScaleFactors::new();
    dequantise(&qscf, envelopes.framing.qmode_env, &mut next_factors)?;

    let (timeslots, ts_offset) = validate_frame(pcm, output_len)?;
    let tables = hf_gen_tables(bands, params)?;
    let factor = frame_timeslot_factor(pcm.len())?;
    let aspx = high_band_interval(&envelopes.framing.interval, timeslots, factor)?;
    let limiter = hf_adjust_limiter(bands, &tables, hf_adjust)?;
    validate_hf_carryover(aspx, state, factor, timeslots)?;
    workspace.prepare_frame();
    generate_hf(
        pcm, bands, params, &tables, aspx, state, workspace, timeslots, ts_offset,
    )?;

    let Some(q_high) = workspace.q_high.get(..aspx.timeslots()) else {
        return Err(PipelineError::FrameTooLong {
            timeslots: aspx.timeslots(),
            capacity: MAX_EXTENDED_TIMESLOTS,
        });
    };
    let (next_sines, estimated, gains) = adjust_high_band(
        q_high,
        bands,
        &tables,
        envelopes,
        hf_adjust,
        limiter.as_ref(),
        &next_factors,
        factor,
        state.sine,
    )?;

    let Ok(num_qmf_timeslots) = u8::try_from(timeslots) else {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    assemble_high_band(
        &gains,
        bands,
        &envelopes.framing.interval,
        hf_adjust,
        aspx,
        (factor, num_qmf_timeslots),
        state,
        workspace,
    )?;

    state.envelope = next_history;
    state.sine = next_sines;
    intermediates.scale_factors = next_factors;
    intermediates.estimate = estimated;
    intermediates.gains = gains;

    combine_to_qmf(state, workspace, timeslots, ts_offset)
}

/// `aspx_balance = 1` 的双声道包络解码与联合反量化通路。
///
/// 第一声道按普通步长解出和参数，第二声道按 `Pseudocode 80`/`81` 的双倍步长
/// 解出平衡参数；随后 `Pseudocode 84` 同时产出两声道的线性标度因子。两声道
/// 必须共用同一个包络区间与 `aspx_qmode_env`，且 PCM 帧长必须相同。
///
/// # Errors
///
/// 所有包络解码、联合反量化、帧布局与两路高频携带量错误，都在任一声道状态或
/// 工作区改写前返回。
#[expect(
    clippy::too_many_arguments,
    reason = "双声道入口的四组数组分别对应逐声道输入、状态、工作区与输出"
)]
pub fn frame_with_balanced_scale_factors(
    pcm: [&[f32]; 2],
    bands: &AspxBandTables,
    params: [HfGenParams<'_>; 2],
    envelopes: [EnvelopeInput<'_>; 2],
    hf_adjust: [HfAdjustParams<'_>; 2],
    states: [&mut AspxChannelState; 2],
    workspaces: [&mut AspxWorkspace; 2],
    intermediates: [&mut AspxIntermediates; 2],
    out: [&mut [f32]; 2],
) -> Result<(), PipelineError> {
    let [first_state, second_state] = states;
    let [first_workspace, second_workspace] = workspaces;
    let [first_intermediates, second_intermediates] = intermediates;
    let [first_out, second_out] = out;
    let output_lengths = [first_out.len(), second_out.len()];
    let [first_qmf, second_qmf] = frame_with_balanced_scale_factors_impl(
        pcm,
        bands,
        params,
        envelopes,
        hf_adjust,
        [&mut *first_state, &mut *second_state],
        [&mut *first_workspace, &mut *second_workspace],
        [&mut *first_intermediates, &mut *second_intermediates],
        Some(output_lengths),
    )?;
    synthesise_ac4_pcm(first_qmf, &mut first_state.synthesis, first_out)?;
    synthesise_ac4_pcm(second_qmf, &mut second_state.synthesis, second_out)?;
    Ok(())
}

/// [`frame_with_balanced_scale_factors`] 的双声道 QMF 域出口。
///
/// 两个返回切片分别借用对应的工作区，可直接按声道传给下游 QMF 域工具。本函数
/// 不执行两路 QMF 合成，也不推进任一路 [`AspxChannelState::synthesis`]。
///
/// # Errors
///
/// 见 [`PipelineError`]。
#[expect(
    clippy::too_many_arguments,
    reason = "双声道 QMF 出口与 PCM 包装器接收同一组规范输入，只省去终端缓冲"
)]
pub fn frame_with_balanced_scale_factors_qmf<'a>(
    pcm: [&[f32]; 2],
    bands: &AspxBandTables,
    params: [HfGenParams<'_>; 2],
    envelopes: [EnvelopeInput<'_>; 2],
    hf_adjust: [HfAdjustParams<'_>; 2],
    states: [&mut AspxChannelState; 2],
    workspaces: [&'a mut AspxWorkspace; 2],
    intermediates: [&mut AspxIntermediates; 2],
) -> Result<[&'a [QmfSlot]; 2], PipelineError> {
    frame_with_balanced_scale_factors_impl(
        pcm,
        bands,
        params,
        envelopes,
        hf_adjust,
        states,
        workspaces,
        intermediates,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "内部实现同时服务双声道 QMF 出口与带长度先验检查的 PCM 包装器"
)]
fn frame_with_balanced_scale_factors_impl<'a>(
    pcm: [&[f32]; 2],
    bands: &AspxBandTables,
    params: [HfGenParams<'_>; 2],
    envelopes: [EnvelopeInput<'_>; 2],
    hf_adjust: [HfAdjustParams<'_>; 2],
    states: [&mut AspxChannelState; 2],
    workspaces: [&'a mut AspxWorkspace; 2],
    intermediates: [&mut AspxIntermediates; 2],
    output_lengths: Option<[usize; 2]>,
) -> Result<[&'a [QmfSlot]; 2], PipelineError> {
    let [first_pcm, second_pcm] = pcm;
    let [first_params, second_params] = params;
    let [first_envelopes, second_envelopes] = envelopes;
    let [first_adjust, second_adjust] = hf_adjust;
    let [first_state, second_state] = states;
    let [first_workspace, second_workspace] = workspaces;
    let [first_intermediates, second_intermediates] = intermediates;
    let (first_output_len, second_output_len) = match output_lengths {
        Some([first, second]) => (Some(first), Some(second)),
        None => (None, None),
    };

    if first_envelopes.framing.interval != second_envelopes.framing.interval
        || first_envelopes.framing.qmode_env != second_envelopes.framing.qmode_env
    {
        return Err(PipelineError::BalancedFramingMismatch);
    }
    if first_pcm.len() != second_pcm.len() {
        return Err(PipelineError::BalancedFrameLengthMismatch {
            first: first_pcm.len(),
            second: second_pcm.len(),
        });
    }

    let (next_first_history, first_qscf) =
        decode_envelope_input(bands, first_envelopes, false, first_state.envelope)?;
    let (next_second_history, second_qscf) =
        decode_envelope_input(bands, second_envelopes, true, second_state.envelope)?;
    let mut next_first_factors = ScaleFactors::new();
    let mut next_second_factors = ScaleFactors::new();
    dequantise_pair(
        &first_qscf,
        &second_qscf,
        first_envelopes.framing.qmode_env,
        &mut next_first_factors,
        &mut next_second_factors,
    )?;

    // 两路的时隙数与延迟档由上面的帧长相等检查绑成同一个值，各算一次只是为了
    // 让 PCM 包装器的两路输出长度各自过一遍 `OutputLengthMismatch`。**不要据此以为两
    // 路可以各走各的时间轴**：拿第二路的时隙数去合成第一路是等价变体，注入它
    // 不会有判据响，那不是判据缺口。
    let (first_timeslots, first_offset) = validate_frame(first_pcm, first_output_len)?;
    let (second_timeslots, second_offset) = validate_frame(second_pcm, second_output_len)?;
    let factor = frame_timeslot_factor(first_pcm.len())?;
    let aspx = high_band_interval(&first_envelopes.framing.interval, first_timeslots, factor)?;
    let first_tables = hf_gen_tables(bands, first_params)?;
    let second_tables = hf_gen_tables(bands, second_params)?;
    let first_limiter = hf_adjust_limiter(bands, &first_tables, first_adjust)?;
    let second_limiter = hf_adjust_limiter(bands, &second_tables, second_adjust)?;
    validate_hf_carryover(aspx, first_state, factor, first_timeslots)?;
    validate_hf_carryover(aspx, second_state, factor, second_timeslots)?;
    first_workspace.prepare_frame();
    second_workspace.prepare_frame();
    let [first_extended, second_extended] = analyse_and_filter_low_band_pair(
        [first_pcm, second_pcm],
        [bands, bands],
        [&mut *first_state, &mut *second_state],
        [&mut *first_workspace, &mut *second_workspace],
        first_timeslots,
        [first_offset, second_offset],
    )?;
    generate_hf_from_low_band(
        bands,
        first_params,
        &first_tables,
        aspx,
        first_state,
        first_workspace,
        first_timeslots,
        first_extended,
    )?;
    generate_hf_from_low_band(
        bands,
        second_params,
        &second_tables,
        aspx,
        second_state,
        second_workspace,
        second_timeslots,
        second_extended,
    )?;

    let (Some(first_q_high), Some(second_q_high)) = (
        first_workspace.q_high.get(..aspx.timeslots()),
        second_workspace.q_high.get(..aspx.timeslots()),
    ) else {
        return Err(PipelineError::FrameTooLong {
            timeslots: aspx.timeslots(),
            capacity: MAX_EXTENDED_TIMESLOTS,
        });
    };
    let (next_first_sines, first_estimate, first_gains) = adjust_high_band(
        first_q_high,
        bands,
        &first_tables,
        first_envelopes,
        first_adjust,
        first_limiter.as_ref(),
        &next_first_factors,
        factor,
        first_state.sine,
    )?;
    let (next_second_sines, second_estimate, second_gains) = adjust_high_band(
        second_q_high,
        bands,
        &second_tables,
        second_envelopes,
        second_adjust,
        second_limiter.as_ref(),
        &next_second_factors,
        factor,
        second_state.sine,
    )?;

    let Ok(num_qmf_timeslots) = u8::try_from(first_timeslots) else {
        return Err(PipelineError::FrameTooLong {
            timeslots: first_timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    assemble_high_band(
        &first_gains,
        bands,
        &first_envelopes.framing.interval,
        first_adjust,
        aspx,
        (factor, num_qmf_timeslots),
        first_state,
        first_workspace,
    )?;
    assemble_high_band(
        &second_gains,
        bands,
        &second_envelopes.framing.interval,
        second_adjust,
        aspx,
        (factor, num_qmf_timeslots),
        second_state,
        second_workspace,
    )?;

    first_state.envelope = next_first_history;
    second_state.envelope = next_second_history;
    first_state.sine = next_first_sines;
    second_state.sine = next_second_sines;
    first_intermediates.scale_factors = next_first_factors;
    second_intermediates.scale_factors = next_second_factors;
    first_intermediates.estimate = first_estimate;
    second_intermediates.estimate = second_estimate;
    first_intermediates.gains = first_gains;
    second_intermediates.gains = second_gains;

    let first_qmf = combine_to_qmf(first_state, first_workspace, first_timeslots, first_offset)?;
    let second_qmf = combine_to_qmf(
        second_state,
        second_workspace,
        second_timeslots,
        second_offset,
    )?;
    Ok([first_qmf, second_qmf])
}

/// QMF 分析加 `5.7.6.3.2`，返回 `Q_low` 的时隙数。
fn analyse_and_filter_low_band(
    pcm: &[f32],
    bands: &AspxBandTables,
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
    timeslots: usize,
    ts_offset: u8,
) -> Result<usize, PipelineError> {
    let Some(q_in) = workspace.q_in.get_mut(..timeslots) else {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    analyse_ac4_pcm(pcm, &mut state.analysis, q_in)?;
    filter_low_band_after_analysis(bands, state, workspace, timeslots, ts_offset)
}

/// 两声道先并排完成 QMF 分析，再分别建立各自的 `Q_low`。
fn analyse_and_filter_low_band_pair(
    pcm: [&[f32]; 2],
    bands: [&AspxBandTables; 2],
    states: [&mut AspxChannelState; 2],
    workspaces: [&mut AspxWorkspace; 2],
    timeslots: usize,
    ts_offsets: [u8; 2],
) -> Result<[usize; 2], PipelineError> {
    let [first_state, second_state] = states;
    let [first_workspace, second_workspace] = workspaces;
    let [first_bands, second_bands] = bands;
    let [first_offset, second_offset] = ts_offsets;
    let first_q_in =
        first_workspace
            .q_in
            .get_mut(..timeslots)
            .ok_or(PipelineError::FrameTooLong {
                timeslots,
                capacity: MAX_QMF_TIMESLOTS,
            })?;
    let second_q_in =
        second_workspace
            .q_in
            .get_mut(..timeslots)
            .ok_or(PipelineError::FrameTooLong {
                timeslots,
                capacity: MAX_QMF_TIMESLOTS,
            })?;
    analyse_ac4_pcm_pair(
        pcm,
        [&mut first_state.analysis, &mut second_state.analysis],
        [first_q_in, second_q_in],
    )?;
    let first_extended = filter_low_band_after_analysis(
        first_bands,
        first_state,
        first_workspace,
        timeslots,
        first_offset,
    )?;
    let second_extended = filter_low_band_after_analysis(
        second_bands,
        second_state,
        second_workspace,
        timeslots,
        second_offset,
    )?;
    Ok([first_extended, second_extended])
}

/// 已完成 QMF 分析后执行 `5.7.6.3.2`。
fn filter_low_band_after_analysis(
    bands: &AspxBandTables,
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
    timeslots: usize,
    ts_offset: u8,
) -> Result<usize, PipelineError> {
    // `Q_low` 比本帧长 ts_offset 个时隙：前缀取自上一帧尾部。
    let extended =
        timeslots
            .checked_add(usize::from(ts_offset))
            .ok_or(PipelineError::FrameTooLong {
                timeslots,
                capacity: MAX_QMF_TIMESLOTS,
            })?;
    let (Some(q_in), Some(q_low)) = (
        workspace.q_in.get(..timeslots),
        workspace.q_low.get_mut(..extended),
    ) else {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    low_band(q_in, &mut state.low_band, bands.sbx(), ts_offset, q_low)?;
    Ok(extended)
}

/// `5.7.6.5.3` 的公共尾段，停在可直接交给下游工具的 `Q_out,ASPX`。
fn combine_to_qmf<'a>(
    state: &mut AspxChannelState,
    workspace: &'a mut AspxWorkspace,
    timeslots: usize,
    ts_offset: u8,
) -> Result<&'a [QmfSlot], PipelineError> {
    let (Some(q_in), Some(y), Some(q_out)) = (
        workspace.q_in.get(..timeslots),
        workspace.y.get(..timeslots),
        workspace.q_out.get_mut(..timeslots),
    ) else {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    combine(q_in, y, ts_offset, &mut state.output, q_out)?;

    qmf_output(workspace, timeslots)
}

/// 从工作区递出本帧有效的 `Q_out,ASPX`，不触碰 QMF 合成状态。
fn qmf_output(workspace: &AspxWorkspace, timeslots: usize) -> Result<&[QmfSlot], PipelineError> {
    let Some(q_out) = workspace.q_out.get(..timeslots) else {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        });
    };
    Ok(q_out)
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "下标由同一用例构造的帧长派生，越界即是该用例要报告的失败"
)]
mod tests {
    extern crate std;

    use super::*;
    use crate::aspx::frames::{AspxInterval, AspxIntervalParams};
    use crate::aspx::hfassemble::HfDelay;
    use crate::aspx::lowband::LowBandDelay;
    use crate::aspx::noisegen::NoiseCursor;
    use crate::aspx::qmf::{QmfAnalysisState, QmfSlot, QmfSynthesisState};
    use crate::aspx::syntax::AspxHfGen;
    use crate::aspx::tables::IntervalClass;
    use crate::aspx::tna::{ChirpState, TnaDelay};
    use crate::aspx::tonegen::ToneCursor;
    use std::vec;
    use std::vec::Vec;

    const SLOTS: usize = 16;
    const FRAME: usize = SLOTS * SUBBANDS;

    /// 一个脉冲落在指定样本上，其余为零：延迟一目了然。
    fn impulse(len: usize, at: usize) -> Vec<f32> {
        let mut pcm = vec![0.0f32; len];
        pcm[at] = 1.0;
        pcm
    }

    /// 连跑若干帧，返回拼接后的输出。
    fn run(frames: &[Vec<f32>]) -> Vec<f32> {
        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        let mut all = Vec::new();
        for pcm in frames {
            let mut out = vec![0.0f32; pcm.len()];
            bypass_frame(pcm, &mut state, &mut ws, &mut out).expect("旁路应能跑通");
            all.extend_from_slice(&out);
        }
        all
    }

    fn impulse_frames(frame_len: usize, count: usize) -> Vec<Vec<f32>> {
        (0..count)
            .map(|frame| {
                if frame == 0 {
                    impulse(frame_len, 0)
                } else {
                    vec![0.0; frame_len]
                }
            })
            .collect()
    }

    #[test]
    fn the_bypass_delays_the_input_by_the_filterbank_plus_delta_aspx() {
        // 总延迟 = 滤波器组往返 + δ_ASPX·64。两个 δ 各跑一次，差值必须恰好是
        // (6−3)·64 = 192 个样本——这一条不依赖滤波器组延迟是多少，因此不必在
        // 判据里重抄它。
        // 1 024 样本帧合法对应 δ=3；1 536 样本帧合法对应 δ=6。两组总输入
        // 都是 6 144 个样本，唯一应改变峰值位置的就是表 192 的延迟档位。
        let short = run(&impulse_frames(FRAME, 6));
        let long = run(&impulse_frames(24 * SUBBANDS, 4));

        let peak = |v: &[f32]| {
            v.iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|(i, _)| i)
                .expect("非空")
        };
        let (a, b) = (peak(&short), peak(&long));
        assert!(short[a].abs() > 0.1, "脉冲应能穿过旁路");
        assert_eq!(b - a, 3 * SUBBANDS, "δ_ASPX 每多一个时隙就多 64 个样本");
    }

    #[test]
    fn silence_stays_silent_across_frames() {
        let frames: Vec<Vec<f32>> = (0..4).map(|_| vec![0.0f32; FRAME]).collect();
        for value in run(&frames) {
            assert_eq!(value, 0.0, "全零输入不应产生任何输出");
        }
    }

    #[test]
    fn every_table_189_frame_length_is_accepted() {
        for samples in [2048usize, 1920, 1536, 1024, 960, 768, 512, 384] {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            let pcm = vec![0.0f32; samples];
            let mut out = vec![0.0f32; samples];
            bypass_frame(&pcm, &mut state, &mut ws, &mut out)
                .unwrap_or_else(|error| panic!("合法帧长 {samples} 被拒绝：{error:?}"));
        }
    }

    #[test]
    fn splitting_a_signal_into_two_frames_matches_one_shot() {
        // **判据是「分帧 == 一次性」，不是「延续 ≠ 重启」。** 后者只要三个状态
        // （分析、合成、合并延迟）里任一个延续就成立，另外两个不推进也照样通
        // 过——注入实测三条都不响，是典型的防御互相掩护。逐位对齐一次性结果
        // 才能让任一处失延续显形。
        // 1 024 样本整帧与两个 512 样本帧都在表 189 中，且同属 δ=3 档。
        let total = FRAME;
        let half = total / 2;
        let long: Vec<f32> = (0..total).map(|i| ((i % 97) as f32) * 0.01 - 0.5).collect();

        let one_shot = {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            let mut out = vec![0.0f32; total];
            bypass_frame(&long, &mut state, &mut ws, &mut out).expect("整段");
            out
        };
        let split = {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            let mut a = vec![0.0f32; half];
            let mut b = vec![0.0f32; half];
            bypass_frame(&long[..half], &mut state, &mut ws, &mut a).expect("前半");
            bypass_frame(&long[half..], &mut state, &mut ws, &mut b).expect("后半");
            let mut all = a;
            all.extend_from_slice(&b);
            all
        };
        assert_eq!(split, one_shot, "分帧结果必须与一次性跑完整段逐位相同");
        // 反面：输出确实有内容，否则「相等」可能来自两边都是零。
        assert!(
            one_shot.iter().any(|v| v.abs() > 0.01),
            "输出必须非零，否则判据无鉴别力"
        );
    }

    #[test]
    fn restarting_mid_signal_changes_the_seam() {
        // 与上一条互补：确认「延续」本身可观察。上一条若因某种原因两边都退化
        // 成同一个平凡结果，这条会失败。
        let total = FRAME * 2;
        let long: Vec<f32> = (0..total).map(|i| ((i % 97) as f32) * 0.01 - 0.5).collect();
        let mut ws = AspxWorkspace::new();
        let mut carried = vec![0.0f32; FRAME];
        let mut restarted = vec![0.0f32; FRAME];

        let mut state = AspxChannelState::new();
        let mut head = vec![0.0f32; FRAME];
        bypass_frame(&long[..FRAME], &mut state, &mut ws, &mut head).expect("前半");
        bypass_frame(&long[FRAME..], &mut state, &mut ws, &mut carried).expect("续跑");

        let mut fresh = AspxChannelState::new();
        bypass_frame(&long[FRAME..], &mut fresh, &mut ws, &mut restarted).expect("重启");
        assert_ne!(carried, restarted, "丢掉历史必须改变接缝处的输出");
    }

    #[test]
    fn the_low_band_path_matches_the_bypass_bit_for_bit() {
        // `Q_low` 喂的是高频生成器，不进输出，因此多跑这一步不该改变结果。
        let bands = AspxBandTables::derive(false, 0, 0, 0, 0).expect("频带表");
        let long: Vec<f32> = (0..FRAME * 2)
            .map(|i| ((i % 89) as f32) * 0.02 - 0.8)
            .collect();

        let mut plain = (AspxChannelState::new(), AspxWorkspace::new(), Vec::new());
        let mut withlb = (AspxChannelState::new(), AspxWorkspace::new(), Vec::new());
        for chunk in long.chunks(FRAME) {
            let mut a = vec![0.0f32; chunk.len()];
            bypass_frame(chunk, &mut plain.0, &mut plain.1, &mut a).expect("旁路");
            plain.2.extend_from_slice(&a);
            let mut b = vec![0.0f32; chunk.len()];
            frame_with_low_band(chunk, &bands, &mut withlb.0, &mut withlb.1, &mut b)
                .expect("低带路径");
            withlb.2.extend_from_slice(&b);
        }
        assert_eq!(plain.2, withlb.2, "低带滤波不该改变旁路输出");
        assert!(plain.2.iter().any(|v| v.abs() > 0.01), "输出必须非零");
    }

    #[test]
    fn control_delay_priming_advances_only_signal_path_histories() {
        let bands = AspxBandTables::derive(false, 0, 0, 0, 0).expect("频带表");
        let pcm: Vec<f32> = (0..FRAME)
            .map(|index| ((index % 101) as f32) * 0.015 - 0.7)
            .collect();

        let mut primed = AspxChannelState::new();
        let mut prime_ws = AspxWorkspace::new();
        let prime_out = prime_control_delay_qmf(&pcm, &bands, &mut primed, &mut prime_ws)
            .expect("控制延迟预热")
            .to_vec();

        let mut low_only = AspxChannelState::new();
        let mut low_ws = AspxWorkspace::new();
        let low_out = frame_with_low_band_qmf(&pcm, &bands, &mut low_only, &mut low_ws)
            .expect("低带参照")
            .to_vec();
        assert_eq!(prime_out, low_out, "预热输出仍应是同一份 Y=0 低带参照");

        let mut expected_tna = TnaDelay::new();
        expected_tna
            .advance(&prime_ws.q_low[..SLOTS + 3], SLOTS)
            .expect("按本帧时隙推进 TNA 历史");
        assert_eq!(primed.tna, expected_tna, "预热必须保存预测器输入历史");
        assert_ne!(primed.tna, low_only.tna, "普通低带诊断入口不推进 TNA");

        let fresh = AspxChannelState::new();
        assert_eq!(primed.envelope, fresh.envelope, "不得提前消费包络");
        assert_eq!(primed.chirp, fresh.chirp, "不得提前推进 chirp");
        assert_eq!(primed.sine, fresh.sine, "不得提前推进正弦延续");
        assert_eq!(primed.noise, fresh.noise, "不得提前推进噪声游标");
        assert_eq!(primed.tone, fresh.tone, "不得提前推进音调游标");
        assert_eq!(primed.hf, fresh.hf, "不得提前产生 HF carry-over");
    }

    #[test]
    fn the_two_delay_lines_stay_in_step() {
        // `5.7.6.3.2` 的 LowBandDelay 与 `5.7.6.5.3` 的 AspxOutputDelay 都按
        // ts_offset_hfgen 延迟，却由不同模块各自维护。两者同步的可观察后果是：
        // Q_low 的低带与 Q_out 的低带（此时 Y ≡ 0）必须逐位相同。任一条延迟线
        // 取错时隙、存错位置或跨帧丢历史，这个等式立刻破。
        let bands = AspxBandTables::derive(false, 0, 0, 0, 0).expect("频带表");
        let sbx = usize::from(bands.sbx());
        assert!(sbx > 0, "本判据要求交叉子带非零，否则低带为空");
        let long: Vec<f32> = (0..FRAME * 2)
            .map(|i| ((i % 89) as f32) * 0.02 - 0.8)
            .collect();

        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        for (frame, chunk) in long.chunks(FRAME).enumerate() {
            let mut out = vec![0.0f32; chunk.len()];
            frame_with_low_band(chunk, &bands, &mut state, &mut ws, &mut out).expect("低带路径");
            let slots = chunk.len() / SUBBANDS;
            let mut compared = 0usize;
            for ts in 0..slots {
                for sb in 0..sbx {
                    assert_eq!(
                        ws.q_low[ts].re[sb], ws.q_out[ts].re[sb],
                        "帧 {frame} 时隙 {ts} 子带 {sb} 的两条延迟线不同步"
                    );
                    assert_eq!(ws.q_low[ts].im[sb], ws.q_out[ts].im[sb]);
                    compared += 1;
                }
            }
            assert!(compared > 0, "本判据必须真的比较过样本");
        }
        // 反面：低带确实有内容，否则上面比的是两串零。
        assert!(
            (0..sbx).any(|sb| ws.q_low[SLOTS - 1].re[sb] != 0.0),
            "低带必须非零，否则判据无鉴别力"
        );
    }

    /// HF 通路判据共用的频带表。
    ///
    /// 取 `xover = 1` 保留 `sba < sbx` 的非退化几何（sba=10、sbx=11），但它
    /// **不能**区分 `prediction_filters` 传 `sba` 还是 `sbx`：patch 源恒落在
    /// `[0, sba)`，传较大的 `sbx` 只会多算没人读取的系数。真正可观察的边界是
    /// 少算一个子带；下面另钉住至少一段 patch 读到 `sba - 1`。
    fn hf_bands() -> AspxBandTables {
        let bands = AspxBandTables::derive(false, 0, 0, 0, 1).expect("频带表");
        assert_ne!(
            bands.sba(),
            bands.sbx(),
            "夹具前提：源起点与交叉子带必须不同"
        );
        let patches = PatchTable::derive(&bands, false, true).expect("patch 表");
        assert!(
            (0..usize::from(patches.count())).any(|index| {
                patches
                    .start_sb(index)
                    .zip(patches.num_sb(index))
                    .is_some_and(|(start, count)| start.saturating_add(count) == bands.sba())
            }),
            "夹具必须读取 sba - 1，否则预测器少算一带的注入仍不可观察"
        );
        bands
    }

    /// 逐噪声子带组的 `aspx_tna_mode`。
    ///
    /// **不能取 0**：表 195 的模式 0 给出 chirp 因子 0，而 chirp 是 TNA 预测项
    /// 的乘子，取零会让整条预测支路恒等于零。那样一来 `TnaDelay` 与
    /// `ChirpState` 两个跨帧状态同时不可观察，「延续」类判据会一起失去鉴别力
    /// ——第一版就是这么写的，`TnaDelay` 那条注入沉默才发现。
    fn chirp_modes(bands: &AspxBandTables) -> Vec<u8> {
        vec![2u8; usize::from(bands.num_sbg_noise())]
    }

    fn hf_params<'a>(modes: &'a [u8]) -> HfGenParams<'a> {
        HfGenParams {
            master_freq_scale: false,
            base_samp_freq_48: true,
            chirp_modes: modes,
            pre_flattening: false,
        }
    }

    fn ramp(len: usize) -> Vec<f32> {
        (0..len).map(|i| ((i % 89) as f32) * 0.02 - 0.8).collect()
    }

    #[test]
    fn the_hf_generation_path_matches_the_bypass_bit_for_bit() {
        // `Q_high` 要经 5.7.6.4.2–5.7.6.4.5 才汇入 Y，本步不该改变输出。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let long = ramp(FRAME * 2);

        let mut plain = (AspxChannelState::new(), AspxWorkspace::new(), Vec::new());
        let mut hf = (AspxChannelState::new(), AspxWorkspace::new(), Vec::new());
        for chunk in long.chunks(FRAME) {
            let mut a = vec![0.0f32; chunk.len()];
            bypass_frame(chunk, &mut plain.0, &mut plain.1, &mut a).expect("旁路");
            plain.2.extend_from_slice(&a);
            let mut b = vec![0.0f32; chunk.len()];
            frame_with_hf_generation(
                chunk,
                &bands,
                hf_params(&modes),
                &mut hf.0,
                &mut hf.1,
                &mut b,
            )
            .expect("HF 生成路径");
            hf.2.extend_from_slice(&b);
        }
        assert_eq!(plain.2, hf.2, "HF 生成不该改变旁路输出");
        assert!(plain.2.iter().any(|v| v.abs() > 0.01), "输出必须非零");
        // 反面：`Q_high` 确实被算了出来，否则上面的相等可能只是因为什么都没做。
        let sbx = usize::from(bands.sbx());
        assert!(
            (0..SLOTS).any(|ts| (sbx..SUBBANDS).any(|sb| hf.1.q_high[ts].re[sb] != 0.0)),
            "Q_high 必须非零，否则这条判据没有区分力"
        );
    }

    #[test]
    fn hf_generation_never_writes_below_the_crossover() {
        // `Pseudocode 89` 只写 `[sbx, sbx + Σ patch_num_sb)`；低带留给调用方。
        // 哨兵放进函数真会改的那块缓冲，再整体快照比对——写错输出切片或用错
        // `sbx` 都会踩到低带。
        let bands = hf_bands();
        let sbx = usize::from(bands.sbx());
        assert!(sbx > 0, "本判据要求交叉子带非零");
        let modes = chirp_modes(&bands);
        let pcm = ramp(FRAME);

        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        for ts in 0..SLOTS {
            for sb in 0..SUBBANDS {
                ws.q_high[ts].re[sb] = 3.5;
                ws.q_high[ts].im[sb] = -3.5;
            }
        }
        let mut out = vec![0.0f32; FRAME];
        frame_with_hf_generation(
            &pcm,
            &bands,
            hf_params(&modes),
            &mut state,
            &mut ws,
            &mut out,
        )
        .expect("HF 生成路径");

        let mut touched = 0usize;
        for ts in 0..SLOTS {
            for sb in 0..sbx {
                assert_eq!(
                    ws.q_high[ts].re[sb], 3.5,
                    "时隙 {ts} 子带 {sb} 的低带被写了"
                );
                assert_eq!(ws.q_high[ts].im[sb], -3.5);
            }
            for sb in sbx..SUBBANDS {
                if ws.q_high[ts].re[sb] != 3.5 || ws.q_high[ts].im[sb] != -3.5 {
                    touched += 1;
                }
            }
        }
        assert!(
            touched > 0,
            "A-SPX 范围内必须真的被写过，否则哨兵判据恒成立"
        );
    }

    #[test]
    fn a_trimmed_last_patch_zeros_the_uncovered_high_band_tail() {
        // 这组合法配置的最后一段只有两个子带，`Pseudocode 71` 会把它从 patch
        // 表删除：A-SPX 范围到 sbz，但 HF 创建只生成到 patch_end。先把整块
        // `Q_high` 填成哨兵，才能看见尾带究竟是本帧的零还是上一帧残值。
        let bands = AspxBandTables::derive(false, 0, 3, 0, 0).expect("频带表");
        let patches = PatchTable::derive(&bands, false, true).expect("patch 表");
        let patch_end = usize::from(
            patches
                .border(usize::from(patches.count()))
                .expect("末端 patch 边界"),
        );
        let sbx = usize::from(bands.sbx());
        let sbz = usize::from(bands.sbz());
        assert!(sbx < patch_end, "夹具必须真的生成一段高带");
        assert!(patch_end < sbz, "夹具必须留下未被 patch 覆盖的尾带");
        assert!(sbz < SUBBANDS, "夹具要留出范围外哨兵以约束清零边界");

        let modes = chirp_modes(&bands);
        let pcm = ramp(FRAME);
        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        for slot in &mut ws.q_high[..SLOTS] {
            slot.re.fill(3.5);
            slot.im.fill(-3.5);
        }
        let mut out = vec![0.0f32; FRAME];
        frame_with_hf_generation(
            &pcm,
            &bands,
            hf_params(&modes),
            &mut state,
            &mut ws,
            &mut out,
        )
        .expect("HF 生成路径");

        for (ts, slot) in ws.q_high[..SLOTS].iter().enumerate() {
            for sb in patch_end..sbz {
                assert_eq!(slot.re[sb], 0.0, "时隙 {ts} 子带 {sb} 留下旧实部");
                assert_eq!(slot.im[sb], 0.0, "时隙 {ts} 子带 {sb} 留下旧虚部");
            }
            assert_eq!(slot.re[sbx - 1], 3.5, "交叉子带以下不应被清零");
            assert_eq!(slot.im[sbx - 1], -3.5);
            assert_eq!(slot.re[sbz], 3.5, "A-SPX 范围以上不应被清零");
            assert_eq!(slot.im[sbz], -3.5);
        }

        // **另一侧**：patch 覆盖的每个子带都必须留有生成的信号。只验「尾带是
        // 零」分不出「只清了尾带」与「把生成的高带也一起清了」；逐子带查而不是
        // 整段查，是因为 `patch_end` 少算一格只会多抹掉一个子带。
        for sb in sbx..patch_end {
            assert!(
                ws.q_high[..SLOTS]
                    .iter()
                    .any(|slot| slot.re[sb] != 0.0 || slot.im[sb] != 0.0),
                "patch 覆盖的子带 {sb} 被误清零"
            );
        }
    }

    #[test]
    fn generated_hf_lines_up_in_time_with_the_low_band() {
        // `Pseudocode 89` 把 `Q_low_ext[ts + ts_offset_hfadj]` 搬到 `Q_high[ts]`，
        // 因此两者的时隙对齐必须精确为 0。区间起止各偏一格都不会改变
        // `last − first`，长度检查看不见，只有对齐能看见——而错一个时隙的高频
        // 信号最终会变成可听的伪声。
        let bands = hf_bands();
        let sbx = usize::from(bands.sbx());
        let modes = chirp_modes(&bands);
        let mut pcm = vec![0.0f32; FRAME];
        pcm[FRAME / 2] = 1.0;

        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        let mut out = vec![0.0f32; FRAME];
        frame_with_hf_generation(
            &pcm,
            &bands,
            hf_params(&modes),
            &mut state,
            &mut ws,
            &mut out,
        )
        .expect("HF 生成路径");

        let energy = |slot: &QmfSlot, from: usize, to: usize| -> f64 {
            (from..to)
                .map(|sb| {
                    f64::from(slot.re[sb]) * f64::from(slot.re[sb])
                        + f64::from(slot.im[sb]) * f64::from(slot.im[sb])
                })
                .sum()
        };
        let peak = |pick: &dyn Fn(usize) -> f64| -> usize {
            (0..SLOTS)
                .max_by(|a, b| pick(*a).total_cmp(&pick(*b)))
                .expect("非空")
        };
        let low_peak = peak(&|ts| energy(&ws.q_low[ts], 0, sbx));
        let high_peak = peak(&|ts| energy(&ws.q_high[ts], sbx, SUBBANDS));

        assert!(
            energy(&ws.q_low[low_peak], 0, sbx) > 0.0,
            "低带必须有能量，否则峰值位置无意义"
        );
        assert!(
            energy(&ws.q_high[high_peak], sbx, SUBBANDS) > 0.0,
            "高带必须有能量，否则峰值位置无意义"
        );
        assert_eq!(high_peak, low_peak, "Q_high 与 Q_low 的时隙必须逐格对齐");
    }

    #[test]
    fn the_predictor_history_holds_the_last_slots_of_the_interval() {
        // `TnaDelay` 存的是 `Q_low_prev[N−4, N)`，N 是**区间**的时隙数，不是
        // `Q_low` 缓冲的长度——后者还多出 ts_offset_hfgen 个前瞻时隙。两者都能
        // 通过 `advance` 的长度检查，取错时下一帧的 `Q_low_ext` 前缀整体走样，
        // 而「重置 TnaDelay 会改变 Q_high」那条判据对此沉默：错的尾巴同样非零。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let pcm = ramp(FRAME);

        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        let mut out = vec![0.0f32; FRAME];
        frame_with_hf_generation(
            &pcm,
            &bands,
            hf_params(&modes),
            &mut state,
            &mut ws,
            &mut out,
        )
        .expect("HF 生成路径");

        let mut expected = TnaDelay::new();
        expected
            .advance(&ws.q_low[..SLOTS], SLOTS)
            .expect("按区间长度前进");
        assert_eq!(state.tna, expected, "预测器历史必须取区间末尾而非缓冲末尾");
        // 反面：前瞻那几个时隙确实不同，否则上面的相等来自它们碰巧相等。
        let mut lookahead = TnaDelay::new();
        let extended = SLOTS + 3;
        lookahead
            .advance(&ws.q_low[..extended], extended)
            .expect("按缓冲长度前进");
        assert_ne!(
            expected, lookahead,
            "两种取法必须真的不同，否则判据无鉴别力"
        );
    }

    /// 跑两帧，返回第二帧的 `Q_high`；`disturb` 在两帧之间只动一个跨帧状态。
    fn second_frame_q_high(
        bands: &AspxBandTables,
        modes: &[u8],
        signal: &[f32],
        disturb: fn(&mut AspxChannelState),
    ) -> Vec<QmfSlot> {
        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        let mut out = vec![0.0f32; FRAME];
        frame_with_hf_generation(
            &signal[..FRAME],
            bands,
            hf_params(modes),
            &mut state,
            &mut ws,
            &mut out,
        )
        .expect("首帧");
        disturb(&mut state);
        frame_with_hf_generation(
            &signal[FRAME..FRAME * 2],
            bands,
            hf_params(modes),
            &mut state,
            &mut ws,
            &mut out,
        )
        .expect("次帧");
        ws.q_high[..SLOTS].to_vec()
    }

    #[test]
    fn each_cross_frame_state_on_the_hf_path_is_observable() {
        // **一次只重置一个状态。** 整组重启也能让 Q_high 变，但那种判据里任一
        // 状态失延续都被另外两个兜住；逐个隔离才能让每一条接线单独显形。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let long = ramp(FRAME * 2);

        // 前置条件：chirp 因子非零，且逐区间在变。它是 TNA 预测项的乘子，取零
        // 会同时抹掉 `TnaDelay` 与 `ChirpState` 两条支路，下面四条断言会一起
        // 变成恒真。这条前提由夹具的 `chirp_modes` 决定，必须在此钉住。
        let mut probe = ChirpState::new();
        let mut factors = vec![0.0f32; modes.len()];
        chirp_factors(&modes, &mut probe, &mut factors).expect("chirp");
        let first = factors[0];
        chirp_factors(&modes, &mut probe, &mut factors).expect("chirp");
        assert!(first != 0.0, "chirp 为零会让 TNA 预测项恒等于零");
        assert!(
            factors[0] != first,
            "chirp 必须跨区间演进，否则其状态不可观察"
        );

        let baseline = second_frame_q_high(&bands, &modes, &long, |_| {});

        /// 只丢掉一个跨帧状态的扰动。
        type Disturbance = (&'static str, fn(&mut AspxChannelState));

        let cases: [Disturbance; 4] = [
            ("QMF 分析历史", |s| s.analysis = QmfAnalysisState::new()),
            ("低带延迟", |s| s.low_band = LowBandDelay::new()),
            ("预测器历史", |s| s.tna = TnaDelay::new()),
            ("chirp 延续", |s| s.chirp = ChirpState::new()),
        ];
        for (name, disturb) in cases {
            let disturbed = second_frame_q_high(&bands, &modes, &long, disturb);
            assert_ne!(disturbed, baseline, "只丢掉{name}也必须改变 Q_high");
        }
    }

    #[test]
    fn both_delay_tiers_reach_hf_generation() {
        // `prediction_filters` 由 `Q_low_ext` 的长度反算 ts_offset_hfgen 并要求
        // 落在 {3, 6}。两档各跑一次即可证明本模块传给它的切片长度是对的——切
        // 短或切长都会变成 UnsupportedOffset，而不是悄悄算出错的协方差。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        for samples in [1024usize, 1536] {
            assert!(
                matches!(ts_offset_hfgen(samples as u16), Some(3) | Some(6)),
                "{samples} 应落在两个延迟档之一"
            );
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            let pcm = ramp(samples);
            let mut out = vec![0.0f32; samples];
            frame_with_hf_generation(
                &pcm,
                &bands,
                hf_params(&modes),
                &mut state,
                &mut ws,
                &mut out,
            )
            .unwrap_or_else(|error| panic!("{samples} 样本帧被拒绝：{error:?}"));
        }
    }

    #[test]
    fn hf_parameters_are_checked_before_any_state_moves() {
        // **三条入口都要验。** 表推导只写一次，却由三处各自决定排在
        // `prepare_frame` 之前还是之后；只验 `frame_with_hf_generation` 时，把另
        // 外两处的推导挪到清理之后一条判据都不响——注入实测如此。
        let bands = hf_bands();
        let groups = usize::from(bands.num_sbg_noise());
        let short = vec![0u8; groups.saturating_sub(1)];
        let mut invalid = chirp_modes(&bands);
        invalid[0] = 4;
        let cases = [
            (
                "模式数量",
                short.as_slice(),
                PipelineError::ChirpModeCountMismatch {
                    expected: groups,
                    provided: groups.saturating_sub(1),
                },
            ),
            (
                "模式取值",
                invalid.as_slice(),
                PipelineError::Tna(TnaError::ModeOutOfRange { group: 0, mode: 4 }),
            ),
        ];
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let pcm = ramp(FRAME);

        for (label, modes, expected) in cases {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            // 工作区也算「被改写」：`prepare_frame` 一旦跑过就不是原样，而这条错误
            // 完全由调用参数决定，`bypass_frame` 的契约要求它在此之前返回。
            plant_sentinels(&mut ws);
            let mut out = vec![0.0f32; FRAME];
            assert_eq!(
                frame_with_hf_generation(
                    &pcm,
                    &bands,
                    hf_params(modes),
                    &mut state,
                    &mut ws,
                    &mut out
                ),
                Err(expected),
                "{label}"
            );
            assert_eq!(
                state,
                AspxChannelState::new(),
                "{label}：HF 入口不应推进状态"
            );
            assert!(sentinels_intact(&ws), "{label}：HF 入口不应准备工作区");

            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            plant_sentinels(&mut ws);
            let mut intermediates = AspxIntermediates::default();
            assert_eq!(
                frame_with_scale_factors(
                    &pcm,
                    &bands,
                    hf_params(modes),
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    adjust_params(&bands),
                    &mut state,
                    &mut ws,
                    &mut intermediates,
                    &mut out,
                ),
                Err(expected),
                "{label}"
            );
            assert_eq!(
                state,
                AspxChannelState::new(),
                "{label}：标度因子入口不应推进状态"
            );
            assert!(sentinels_intact(&ws), "{label}：标度因子入口不应准备工作区");

            let mut states = [AspxChannelState::new(), AspxChannelState::new()];
            let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
            for workspace in &mut workspaces {
                plant_sentinels(workspace);
            }
            let [first_state, second_state] = &mut states;
            let [first_workspace, second_workspace] = &mut workspaces;
            assert_eq!(
                balanced_frame(
                    &bands,
                    modes,
                    &pcm,
                    [
                        EnvelopeInput {
                            framing: &frame,
                            data: &data,
                        },
                        EnvelopeInput {
                            framing: &frame,
                            data: &data,
                        },
                    ],
                    [first_state, second_state],
                    [first_workspace, second_workspace],
                )
                .err(),
                Some(expected),
                "{label}"
            );
            assert!(
                states.iter().all(|state| *state == AspxChannelState::new()),
                "{label}：双声道入口不应推进状态"
            );
            assert!(
                workspaces.iter().all(sentinels_intact),
                "{label}：双声道入口不应准备工作区"
            );
        }
    }

    #[test]
    fn rejected_input_is_reported_before_any_work() {
        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        ws.q_low[0].re[0] = 7.0;
        ws.y[0].im[0] = -7.0;
        let mut out = vec![0.0f32; FRAME];
        assert_eq!(
            bypass_frame(&[0.0; 63], &mut state, &mut ws, &mut out),
            Err(PipelineError::UnalignedInput { samples: 63 })
        );
        assert_eq!(
            bypass_frame(&[0.0; FRAME], &mut state, &mut ws, &mut vec![0.0; 64]),
            Err(PipelineError::OutputLengthMismatch {
                expected: FRAME,
                provided: 64
            })
        );
        let too_long = vec![0.0f32; (MAX_QMF_TIMESLOTS + 1) * SUBBANDS];
        let mut too_long_out = vec![0.0f32; too_long.len()];
        assert_eq!(
            bypass_frame(&too_long, &mut state, &mut ws, &mut too_long_out),
            Err(PipelineError::FrameTooLong {
                timeslots: MAX_QMF_TIMESLOTS + 1,
                capacity: MAX_QMF_TIMESLOTS
            })
        );
        // 7 个 QMF 时隙按 64 对齐且不超容量，但不存在于表 189。用非零信号保证
        // 若检查晚于 QMF 分析，下面的状态相等断言一定能看见污染。
        let unsupported = vec![1.0f32; 7 * SUBBANDS];
        let mut unsupported_out = vec![0.0f32; unsupported.len()];
        assert_eq!(
            bypass_frame(&unsupported, &mut state, &mut ws, &mut unsupported_out),
            Err(PipelineError::UnsupportedFrameLength {
                samples: 7 * SUBBANDS
            })
        );
        assert_eq!(state, AspxChannelState::new(), "被拒绝的输入不应推进状态");
        assert_eq!(ws.q_low[0].re[0], 7.0, "拒绝前不应准备工作区");
        assert_eq!(ws.y[0].im[0], -7.0, "拒绝前不应准备工作区");
    }
    // ---- 5.7.6.3.4 包络解码与 5.7.6.3.5 反量化 ----

    /// 一个 FIXVAR 区间：两个包络，分辨率一高一低。
    ///
    /// **不能用 FIXFIX**：`Pseudocode 77` 对 FIXFIX 只算一次分辨率再复制到全部
    /// 包络，那样「逐包络问区间」与「问一次用到底」两种实现给出相同结果，
    /// 下面按分辨率区分包络的判据会一起失去鉴别力。
    fn split_interval(slots: u8) -> (AspxIntervalParams, AspxInterval) {
        split_interval_at(slots, slots / 2)
    }

    /// 同上，但把 `aspx_tsg_ptr` 指到第 `pointer` 个包络。
    ///
    /// **默认的 `−1` 会让正弦的跨区间延续完全不可观察。** `Pseudocode 92` 的
    /// 判据是 `starts_by(atsg) || prev_at_end || carried`，而 `starts_by` 对
    /// `−1` 恒真，`||` 一短路，后两项就永远轮不到。指到第 1 个包络后，第 0 个
    /// 包络的 `starts_by` 为假，`carried` 才成为唯一决定项。
    fn pointed_interval(slots: u8, pointer: i8) -> (AspxIntervalParams, AspxInterval) {
        let (mut params, _) = split_interval(slots);
        params.tsg_ptr = pointer;
        let interval =
            AspxInterval::derive(&params, slots, 0, true, i16::from(slots)).expect("区间");
        assert!(
            !SinePlacement::from_params(pointer).starts_by_for_test(0),
            "夹具前提：第 0 个包络不得自带正弦，否则跨帧延续被短路掉"
        );
        (params, interval)
    }

    /// 同上，但把中间那条边界挪到距右端 `step` 个时隙处。
    ///
    /// 只有边界不同、包络数与逐包络分辨率都相同的两个区间，才能验出「两声道共
    /// 用同一区间」这条检查：`Pseudocode 84` 的网格核对只看包络数与组数，边界
    /// 不同它一概看不见，去掉检查后不会有任何一处报错。
    fn split_interval_at(slots: u8, step: u8) -> (AspxIntervalParams, AspxInterval) {
        let mut params = AspxIntervalParams::fixfix(2);
        params.int_class = IntervalClass::FixVar;
        params.num_rel_right = 1;
        params.rel_bord_right[0] = step;
        params.var_bord_right = Some(0);
        params.freq_res = [true, false, false, false, false];
        let interval =
            AspxInterval::derive(&params, slots, 0, true, i16::from(slots)).expect("区间");
        assert_eq!(interval.num_atsg_sig(), 2, "夹具前提：两个信号包络");
        assert_eq!(
            (interval.freq_res(0), interval.freq_res(1)),
            (Some(true), Some(false)),
            "夹具前提：两个包络的分辨率必须不同"
        );
        (params, interval)
    }

    /// 与 [`split_interval`] 配套的差分符号。
    ///
    /// 每条长度取该包络分辨率对应的组数，与 `Pseudocode 78` 一致。取值逐组不同
    /// 且两个包络不同，任一处错位都会改变结果。
    fn envelope_data(bands: &AspxBandTables, seed: i16) -> AspxEnvelopes {
        let ramp = |count: u8, offset: i16| -> Vec<i16> {
            (0..i16::from(count))
                .map(|index| index.wrapping_add(offset).rem_euclid(5).wrapping_sub(2))
                .collect()
        };
        let high = ramp(bands.num_sbg_sig_highres(), seed);
        let low = ramp(bands.num_sbg_sig_lowres(), seed.wrapping_add(2));
        let noise_a = ramp(bands.num_sbg_noise(), seed.wrapping_add(1));
        let noise_b = ramp(bands.num_sbg_noise(), seed.wrapping_add(3));
        AspxEnvelopes::for_test(&[&high, &low], &[&noise_a, &noise_b])
    }

    /// 逐包络的差分方向；`time_first` 决定首个信号包络是否引用上一区间。
    fn framing(
        params: AspxIntervalParams,
        interval: AspxInterval,
        coarse: bool,
        time_first: bool,
    ) -> AspxChannelFraming {
        framing_dirs(params, interval, coarse, [time_first, false])
    }

    /// 同上，但逐包络指定差分方向。
    fn framing_dirs(
        params: AspxIntervalParams,
        interval: AspxInterval,
        coarse: bool,
        sig_dirs: [bool; 2],
    ) -> AspxChannelFraming {
        AspxChannelFraming::for_test(params, interval, coarse, &sig_dirs, &[false, false])
    }

    /// 跑一帧标度因子通路，返回输出与解出的标度因子。
    fn scale_factor_frame(
        bands: &AspxBandTables,
        modes: &[u8],
        pcm: &[f32],
        framing: &AspxChannelFraming,
        data: &AspxEnvelopes,
        state: &mut AspxChannelState,
        ws: &mut AspxWorkspace,
    ) -> Result<(Vec<f32>, AspxIntermediates), PipelineError> {
        let mut out = vec![0.0f32; pcm.len()];
        let mut intermediates = AspxIntermediates::default();
        frame_with_scale_factors(
            pcm,
            bands,
            hf_params(modes),
            EnvelopeInput { framing, data },
            adjust_params(bands),
            state,
            ws,
            &mut intermediates,
            &mut out,
        )?;
        Ok((out, intermediates))
    }

    /// `5.7.6.4.2` 的逐帧参数：全部子带组都加谐波，插值与限幅都开。
    ///
    /// 三项都取「会执行更多东西」的那一档：
    ///
    /// - **限幅开**——`aspx_limiter == 0` 只走 `Pseudocode 95`，limiter 表推导与
    ///   `96`–`101` 整段都不执行，那一侧接错了无从显形；
    /// - **加谐波**——`aspx_add_harmonic` 全假时没有任何正弦，
    ///   [`AspxChannelState::sine`] 停在初值，跨帧延续因而不可观察。
    fn adjust_params(bands: &AspxBandTables) -> HfAdjustParams<'static> {
        static HARMONICS: [bool; 32] = [true; 32];
        HfAdjustParams {
            add_harmonic: &HARMONICS[..usize::from(bands.num_sbg_sig_highres())],
            interpolation: true,
            limiter: true,
            master_reset: false,
            first_frame: false,
        }
    }

    /// 取出全部 A-SPX 子带在第 `envelope` 个包络上的补偿增益。
    fn gain_row(
        intermediates: &AspxIntermediates,
        bands: &AspxBandTables,
        envelope: usize,
    ) -> Vec<f32> {
        let subbands = usize::from(bands.num_sb_aspx());
        let row = (0..subbands)
            .map(|index| {
                intermediates
                    .gains
                    .signal_gain(index, envelope)
                    .expect("全部相对 A-SPX 子带都应有补偿增益")
            })
            .collect::<Vec<_>>();
        assert_eq!(row.len(), subbands, "补偿增益必须覆盖全部 A-SPX 子带");
        row
    }

    /// 逐包络逐组取出信号标度因子，附带各包络的组数。
    fn sig_rows(factors: &ScaleFactors) -> Vec<Vec<f32>> {
        let (envelopes, _) = factors.counts();
        (0..usize::from(envelopes))
            .map(|env| {
                (0..)
                    .map_while(|group| factors.sig(env, group))
                    .collect::<Vec<f32>>()
            })
            .collect()
    }

    #[test]
    fn the_output_is_the_delayed_input_plus_y() {
        // `5.7.6.5.3` 的 `Q_out(m,i) = Q_in(m, i−δ) + Y(m,i)`。旁路把 `Y` 取零，
        // 因此它的 `Q_out` 恰是被延迟的 `Q_in`；整条通路的 `Q_out` 减去它，剩下
        // 的必须逐位等于本帧写进工作区的 `Y`。
        //
        // 这条是整条阶梯的收口：在此之前每一段都只能证明「没改输出」，`Y` 恒零
        // 时那句话对全错的接线同样成立。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let long = ramp(FRAME * 2);

        let mut plain = (AspxChannelState::new(), AspxWorkspace::new());
        let mut full = (AspxChannelState::new(), AspxWorkspace::new());
        prime_carryover(&mut full.0, &interval, 1);
        let mut moved = false;
        for chunk in long.chunks(FRAME) {
            let mut a = vec![0.0f32; chunk.len()];
            bypass_frame(chunk, &mut plain.0, &mut plain.1, &mut a).expect("旁路");
            let mut intermediates = AspxIntermediates::default();
            let mut b = vec![0.0f32; chunk.len()];
            frame_with_scale_factors(
                chunk,
                &bands,
                hf_params(&modes),
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
                adjust_params(&bands),
                &mut full.0,
                &mut full.1,
                &mut intermediates,
                &mut b,
            )
            .expect("整条通路");
            assert_ne!(a, b, "Y 非零时输出不该再等于旁路");

            for ts in 0..SLOTS {
                for sb in 0..SUBBANDS {
                    let expected = plain.1.q_out[ts].re[sb] + full.1.y[ts].re[sb];
                    assert_eq!(full.1.q_out[ts].re[sb], expected, "实部 ts={ts} sb={sb}");
                    let expected = plain.1.q_out[ts].im[sb] + full.1.y[ts].im[sb];
                    assert_eq!(full.1.q_out[ts].im[sb], expected, "虚部 ts={ts} sb={sb}");
                    moved |= full.1.y[ts].re[sb] != 0.0 || full.1.y[ts].im[sb] != 0.0;
                }
            }
        }
        // 前提：`Y` 真的非零，否则上面的等式对「Y 从未接上」也成立。
        assert!(moved, "夹具前提：Y 必须有非零样本");
    }

    #[test]
    fn the_low_band_of_y_is_cleared_every_frame() {
        // `5.7.6.4.5` 只写 `[sbx, sbx + num_sb_aspx)`，而 `5.7.6.5.3` 的相加遍历
        // 全部 64 个子带。工作区跨帧复用，若 `prepare_frame` 不清 `y`，A-SPX 范围
        // 外的残值会直接加进输出。
        //
        // 这就是 `bypass_frame` 里那段注释留下的待办：`Y` 恒零时清不清都一样，
        // 到这一步才第一次可观察。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let sbx = usize::from(bands.sbx());
        let sbz = usize::from(bands.sbz());
        assert!(
            sbx > 0 && sbz < SUBBANDS,
            "夹具前提：A-SPX 范围两侧都要有余量"
        );

        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        prime_carryover(&mut state, &interval, 1);
        // 上一帧的残值：A-SPX 范围之外埋进非零值。
        for slot in ws.y.iter_mut() {
            slot.re[0] = 9.0;
            slot.im[sbz] = -9.0;
        }
        let mut intermediates = AspxIntermediates::default();
        let mut out = vec![0.0f32; FRAME];
        frame_with_scale_factors(
            &ramp(FRAME),
            &bands,
            hf_params(&modes),
            EnvelopeInput {
                framing: &frame,
                data: &data,
            },
            adjust_params(&bands),
            &mut state,
            &mut ws,
            &mut intermediates,
            &mut out,
        )
        .expect("整条通路");

        let mut wrote_inside = false;
        for ts in 0..SLOTS {
            for sb in 0..SUBBANDS {
                if (sbx..sbz).contains(&sb) {
                    wrote_inside |= ws.y[ts].re[sb] != 0.0 || ws.y[ts].im[sb] != 0.0;
                    continue;
                }
                assert_eq!(ws.y[ts].re[sb], 0.0, "范围外的实部 ts={ts} sb={sb}");
                assert_eq!(ws.y[ts].im[sb], 0.0, "范围外的虚部 ts={ts} sb={sb}");
            }
        }
        // 前提：范围内确实写过，否则「范围外为零」对「整块都没写」也成立。
        assert!(wrote_inside, "夹具前提：A-SPX 范围内必须写过 Y");
    }

    #[test]
    fn each_envelope_takes_its_resolution_from_the_interval() {
        // 区间给第 0 个包络高分辨率、第 1 个低分辨率。若实现只问一次区间再套用
        // 到全部包络，两个包络的组数就会相同，这条断言立刻失败。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let high = usize::from(bands.num_sbg_sig_highres());
        let low = usize::from(bands.num_sbg_sig_lowres());
        assert_ne!(high, low, "夹具前提：两档组数必须不同");

        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        let (_, factors) = scale_factor_frame(
            &bands,
            &modes,
            &ramp(FRAME),
            &frame,
            &data,
            &mut state,
            &mut ws,
        )
        .expect("标度因子路径");

        let rows = sig_rows(&factors.scale_factors);
        assert_eq!(rows.len(), 2, "应解出两个信号包络");
        assert_eq!(rows[0].len(), high, "第 0 个包络按高分辨率取组数");
        assert_eq!(rows[1].len(), low, "第 1 个包络按低分辨率取组数");
    }

    #[test]
    fn the_envelope_history_carries_a_value_not_just_a_flag() {
        // 第二帧首个包络取时间方向，于是它逐组加到上一帧末包络上。两次只改**第
        // 一帧**的差分符号、第二帧完全相同，第二帧的标度因子仍必须不同——这既
        // 排除了「历史没接上」，也排除了「只记了 primed 标志」。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let first_frame = framing(params, interval, false, false);
        let second_frame = framing(params, interval, false, true);
        let long = ramp(FRAME * 2);
        let second_data = envelope_data(&bands, 0);

        let second_factors = |seed: i16| {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            scale_factor_frame(
                &bands,
                &modes,
                &long[..FRAME],
                &first_frame,
                &envelope_data(&bands, seed),
                &mut state,
                &mut ws,
            )
            .expect("首帧");
            let (_, factors) = scale_factor_frame(
                &bands,
                &modes,
                &long[FRAME..FRAME * 2],
                &second_frame,
                &second_data,
                &mut state,
                &mut ws,
            )
            .expect("次帧");
            sig_rows(&factors.scale_factors)
        };

        assert_ne!(
            second_factors(0),
            second_factors(1),
            "上一帧的包络值必须影响本帧的时间方向差分"
        );
    }

    #[test]
    fn the_first_envelope_may_not_reference_a_history_that_does_not_exist() {
        // 起解首帧就声明时间方向时必须报错，且报的是「缺历史」而不是别的原因。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, true);
        let data = envelope_data(&bands, 0);
        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        ws.q_low[0].re[0] = 7.0;
        ws.y[0].im[0] = -7.0;
        assert_eq!(
            scale_factor_frame(
                &bands,
                &modes,
                &ramp(FRAME),
                &frame,
                &data,
                &mut state,
                &mut ws
            )
            .err(),
            Some(PipelineError::Envelope(EnvelopeError::MissingHistory {
                envelope: 0
            }))
        );
        assert_eq!(state, AspxChannelState::new(), "缺历史不应推进其他状态");
        assert_eq!(ws.q_low[0].re[0], 7.0, "缺历史不应准备工作区");
        assert_eq!(ws.y[0].im[0], -7.0, "缺历史不应准备工作区");
    }

    #[test]
    fn qmode_env_reaches_the_dequantiser() {
        // 两档量化步长之比恒为 2：`scf = 64·2^(qscf/a)`，a 取 2 或 1。因此
        // 细档的平方除以 64 必须等于粗档，这条关系不依赖 qscf 是多少，也就不必
        // 在判据里重算一遍标度因子。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let data = envelope_data(&bands, 0);

        let run = |coarse: bool| {
            let frame = framing(params, interval, coarse, false);
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            let (_, factors) = scale_factor_frame(
                &bands,
                &modes,
                &ramp(FRAME),
                &frame,
                &data,
                &mut state,
                &mut ws,
            )
            .expect("标度因子路径");
            sig_rows(&factors.scale_factors)
        };
        let fine = run(false);
        let coarse = run(true);

        // 前提：qscf 不全为零，否则两档都给 64，翻转 qmode_env 不可观察。
        assert!(
            fine.iter().flatten().any(|value| *value != 64.0),
            "夹具前提：至少一组标度因子的 qscf 非零"
        );
        assert_ne!(fine, coarse, "qmode_env 必须传到 5.7.6.3.5");
        for (fine_row, coarse_row) in fine.iter().zip(coarse.iter()) {
            for (fine_value, coarse_value) in fine_row.iter().zip(coarse_row.iter()) {
                let expected = fine_value * fine_value / 64.0;
                assert!(
                    (expected - coarse_value).abs() <= coarse_value.abs() * 1e-5,
                    "细档平方除以 64 应得粗档：{fine_value} → {expected} vs {coarse_value}"
                );
            }
        }
    }

    #[test]
    fn the_interval_and_the_parsed_data_must_agree_on_the_envelope_count() {
        // 区间说两个包络，三处来源各缺一次，每次都必须报**失配**而不是按短的
        // 那一处截断——少解一个包络不会有任何一处报错，却让后续按区间边界索引
        // 时整体错位。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let full = envelope_data(&bands, 0);

        let high: Vec<i16> = (0..i16::from(bands.num_sbg_sig_highres())).collect();
        let noise: Vec<i16> = (0..i16::from(bands.num_sbg_noise())).collect();
        let one_signal = AspxEnvelopes::for_test(&[&high], &[&noise, &noise]);
        let one_noise = AspxEnvelopes::for_test(&[&high, &high], &[&noise]);

        // 方向数组由 `params.num_env` 定长，把它调小即可造出「区间有两个包络、
        // 方向只给了一个」。
        let mut short_params = params;
        short_params.num_env = 1;
        let short_dirs = AspxChannelFraming::for_test(
            short_params,
            interval,
            false,
            &[false, false],
            &[false, false],
        );

        let cases: [(&str, &AspxChannelFraming, &AspxEnvelopes, PipelineError); 3] = [
            (
                "信号差分符号只有一个包络",
                &framing(params, interval, false, false),
                &one_signal,
                PipelineError::EnvelopeDataMismatch {
                    kind: EnvelopeKind::Signal,
                    envelope: 1,
                },
            ),
            (
                "噪声差分符号只有一个包络",
                &framing(params, interval, false, false),
                &one_noise,
                PipelineError::EnvelopeDataMismatch {
                    kind: EnvelopeKind::Noise,
                    envelope: 1,
                },
            ),
            (
                "差分方向只覆盖一个包络",
                &short_dirs,
                &full,
                PipelineError::EnvelopeDataMismatch {
                    kind: EnvelopeKind::Signal,
                    envelope: 1,
                },
            ),
        ];
        for (label, frame, data, expected) in cases {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            assert_eq!(
                scale_factor_frame(
                    &bands,
                    &modes,
                    &ramp(FRAME),
                    frame,
                    data,
                    &mut state,
                    &mut ws
                )
                .err(),
                Some(expected),
                "{label}"
            );
            assert_eq!(
                state,
                AspxChannelState::new(),
                "{label}：入参就能判断的失配不该推进状态"
            );
        }
    }
    #[test]
    fn balanced_stereo_decodes_the_pair_jointly() {
        // 细量化下让信号和参数为 2、平衡参数为 26：后者由平衡声道首个差分符号
        // 13 乘双倍步长得到。Pseudocode 84 给总能量 256，并按 2:1 分给两路。
        // 噪声侧取和参数 0、中心平衡参数 12（首符号 6），应精确等分成 64/64。
        //
        // 若漏掉双倍步长，信号不会是 2:1；若把两声道分别送进 `dequantise`，
        // 第一声道只有 128，第二声道还会把平衡参数误当绝对能量。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let sum_high = frequency_deltas(bands.num_sbg_sig_highres(), 2);
        let sum_low = frequency_deltas(bands.num_sbg_sig_lowres(), 2);
        let sum_noise = frequency_deltas(bands.num_sbg_noise(), 0);
        let balance_high = frequency_deltas(bands.num_sbg_sig_highres(), 13);
        let balance_low = frequency_deltas(bands.num_sbg_sig_lowres(), 13);
        let balance_noise = frequency_deltas(bands.num_sbg_noise(), 6);
        let sum = AspxEnvelopes::for_test(&[&sum_high, &sum_low], &[&sum_noise, &sum_noise]);
        let balance = AspxEnvelopes::for_test(
            &[&balance_high, &balance_low],
            &[&balance_noise, &balance_noise],
        );

        let pcm = ramp(FRAME);
        let mut first_state = AspxChannelState::new();
        let mut second_state = AspxChannelState::new();
        let mut first_workspace = AspxWorkspace::new();
        let mut second_workspace = AspxWorkspace::new();
        let mut first = AspxIntermediates::default();
        let mut second = AspxIntermediates::default();
        let mut first_out = vec![0.0f32; FRAME];
        let mut second_out = vec![0.0f32; FRAME];
        frame_with_balanced_scale_factors(
            [&pcm, &pcm],
            &bands,
            [hf_params(&modes), hf_params(&modes)],
            [
                EnvelopeInput {
                    framing: &frame,
                    data: &sum,
                },
                EnvelopeInput {
                    framing: &frame,
                    data: &balance,
                },
            ],
            [adjust_params(&bands), adjust_params(&bands)],
            [&mut first_state, &mut second_state],
            [&mut first_workspace, &mut second_workspace],
            [&mut first, &mut second],
            [&mut first_out, &mut second_out],
        )
        .expect("平衡式双声道标度因子路径");
        let (first_factors, second_factors) = (&first.scale_factors, &second.scale_factors);

        let (signal_envelopes, noise_envelopes) = first_factors.counts();
        assert_eq!(second_factors.counts(), (signal_envelopes, noise_envelopes));
        for envelope in 0..usize::from(signal_envelopes) {
            for group in 0.. {
                let (Some(first), Some(second)) = (
                    first_factors.sig(envelope, group),
                    second_factors.sig(envelope, group),
                ) else {
                    break;
                };
                assert!((first + second - 256.0).abs() < 1e-4, "总能量应为 256");
                assert!((first - 2.0 * second).abs() < 1e-4, "两路应按 2:1 分配");
            }
        }
        for factors in [&first_factors, &second_factors] {
            for envelope in 0..usize::from(noise_envelopes) {
                for group in 0..usize::from(bands.num_sbg_noise()) {
                    assert_eq!(
                        factors.noise(envelope, group),
                        Some(64.0),
                        "噪声能量应在两声道间等分"
                    );
                }
            }
        }
    }

    #[test]
    fn a_short_envelope_row_is_rejected_rather_than_padded() {
        // `AspxEnvelopes::sig_slice` 递出的是解析时确定的组数那一段。若改成递出
        // 整条定长数组，`5.7.6.3.4` 的 `data.len() < groups` 就永远为假，解析出
        // 的组数比频带表少时会被尾部的零补齐，没有任何一处报错。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let high = usize::from(bands.num_sbg_sig_highres());
        let low = usize::from(bands.num_sbg_sig_lowres());
        let short: Vec<i16> = (0..i16::try_from(high).expect("组数").wrapping_sub(1)).collect();
        let full: Vec<i16> = (0..i16::try_from(low).expect("组数")).collect();
        let noise: Vec<i16> = (0..i16::from(bands.num_sbg_noise())).collect();
        let data = AspxEnvelopes::for_test(&[&short, &full], &[&noise, &noise]);

        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        ws.q_low[0].re[0] = 7.0;
        ws.y[0].im[0] = -7.0;
        assert_eq!(
            scale_factor_frame(
                &bands,
                &modes,
                &ramp(FRAME),
                &frame,
                &data,
                &mut state,
                &mut ws
            )
            .err(),
            Some(PipelineError::Envelope(EnvelopeError::OutputTooSmall {
                needed: high,
                provided: short.len(),
            })),
            "组数不足必须报错，而不是补零"
        );
        assert_eq!(state, AspxChannelState::new(), "短行不应推进其他状态");
        assert_eq!(ws.q_low[0].re[0], 7.0, "短行不应准备工作区");
        assert_eq!(ws.y[0].im[0], -7.0, "短行不应准备工作区");
    }
    /// 跑一帧平衡式双声道通路，返回两路的标度因子。
    fn balanced_frame(
        bands: &AspxBandTables,
        modes: &[u8],
        pcm: &[f32],
        envelopes: [EnvelopeInput<'_>; 2],
        states: [&mut AspxChannelState; 2],
        workspaces: [&mut AspxWorkspace; 2],
    ) -> Result<[AspxIntermediates; 2], PipelineError> {
        balanced_frame_split(bands, modes, [pcm, pcm], envelopes, states, workspaces)
            .map(|(intermediates, _)| intermediates)
    }

    /// 同上，但两路各给一段 PCM，并把两路输出一并返回。
    fn balanced_frame_split(
        bands: &AspxBandTables,
        modes: &[u8],
        pcm: [&[f32]; 2],
        envelopes: [EnvelopeInput<'_>; 2],
        states: [&mut AspxChannelState; 2],
        workspaces: [&mut AspxWorkspace; 2],
    ) -> Result<([AspxIntermediates; 2], [Vec<f32>; 2]), PipelineError> {
        let mut intermediates = [AspxIntermediates::default(), AspxIntermediates::default()];
        let mut out = [vec![0.0f32; pcm[0].len()], vec![0.0f32; pcm[1].len()]];
        {
            let [first, second] = &mut intermediates;
            let [first_out, second_out] = &mut out;
            frame_with_balanced_scale_factors(
                pcm,
                bands,
                [hf_params(modes), hf_params(modes)],
                envelopes,
                [adjust_params(bands), adjust_params(bands)],
                states,
                workspaces,
                [first, second],
                [first_out, second_out],
            )?;
        }
        Ok((intermediates, out))
    }

    /// 左边界不为零的 VARFIX 区间：`atsg_sig[0] = left`，右端仍是帧尾。
    fn left_shifted_interval(slots: u8, left: u8) -> (AspxIntervalParams, AspxInterval) {
        let mut params = AspxIntervalParams::fixfix(2);
        params.int_class = IntervalClass::VarFix;
        params.var_bord_left = Some(left);
        params.num_rel_left = 1;
        params.rel_bord_left[0] = (slots / 2).wrapping_sub(left);
        params.freq_res = [true, false, false, false, false];
        let interval =
            AspxInterval::derive(&params, slots, 0, true, i16::from(slots)).expect("区间");
        assert_eq!(
            (interval.sig_border(0), interval.sig_border(2)),
            (Some(i16::from(left)), Some(i16::from(slots))),
            "夹具前提：左边界应落在 left、右边界仍是帧尾"
        );
        (params, interval)
    }

    /// 逐帧自洽的 VARVAR 区间：左右各偏 `edge` 个 ATS。
    ///
    /// 越帧的时隙数必须与下一帧要取回的相等，否则第二帧会直接报
    /// `CarryoverMismatch`。左偏而右不偏（或反之）都不自洽——`stop_pos` 就是下一
    /// 区间的 `previous_stop_pos`，两端本来就由同一条链绑在一起。
    fn steady_state_interval(slots: u8, edge: u8) -> (AspxIntervalParams, AspxInterval) {
        let mut params = AspxIntervalParams::fixfix(2);
        params.int_class = IntervalClass::VarVar;
        params.var_bord_left = Some(edge);
        params.var_bord_right = Some(edge);
        params.num_rel_right = 1;
        params.rel_bord_right[0] = slots / 2;
        params.freq_res = [true, false, false, false, false];
        let interval =
            AspxInterval::derive(&params, slots, 0, true, i16::from(slots)).expect("区间");
        let head = i16::from(edge);
        let stop = i16::from(slots).wrapping_add(head);
        assert_eq!(
            (interval.sig_border(0), interval.sig_border(2)),
            (Some(head), Some(stop)),
            "夹具前提：两端各偏 {edge} 个 ATS，越帧量与取回量才相等"
        );
        (params, interval)
    }

    /// 右边界越过帧尾的 FIXVAR 区间：`atsg_sig[num] = slots + right`。
    fn right_overrun_interval(slots: u8, right: u8) -> (AspxIntervalParams, AspxInterval) {
        let mut params = AspxIntervalParams::fixfix(2);
        params.int_class = IntervalClass::FixVar;
        params.var_bord_right = Some(right);
        params.num_rel_right = 1;
        params.rel_bord_right[0] = slots / 2;
        params.freq_res = [true, false, false, false, false];
        let interval =
            AspxInterval::derive(&params, slots, 0, true, i16::from(slots)).expect("区间");
        assert_eq!(
            interval.sig_border(2),
            Some(i16::from(slots).wrapping_add(i16::from(right))),
            "夹具前提：右边界必须越过帧尾"
        );
        (params, interval)
    }

    /// 跑一帧标度因子通路，返回工作区里的 `Q_high` 前 `slots` 个时隙。
    fn scale_factor_q_high(
        bands: &AspxBandTables,
        modes: &[u8],
        pcm: &[f32],
        frame: &AspxChannelFraming,
        data: &AspxEnvelopes,
        pre_flattening: bool,
        slots: usize,
    ) -> Vec<QmfSlot> {
        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        prime_carryover(
            &mut state,
            &frame.interval,
            frame_timeslot_factor(pcm.len()).expect("帧长"),
        );
        let mut intermediates = AspxIntermediates::default();
        let mut out = vec![0.0f32; pcm.len()];
        let mut params = hf_params(modes);
        params.pre_flattening = pre_flattening;
        frame_with_scale_factors(
            pcm,
            bands,
            params,
            EnvelopeInput {
                framing: frame,
                data,
            },
            adjust_params(bands),
            &mut state,
            &mut ws,
            &mut intermediates,
            &mut out,
        )
        .expect("标度因子路径");
        ws.q_high[..slots].to_vec()
    }

    /// 只有首项非零的一条差分符号：频率方向下它使整条 `qscf` 恒等于 `first`。
    fn frequency_deltas(count: u8, first: i16) -> Vec<i16> {
        let mut row = vec![0i16; usize::from(count)];
        if let Some(slot) = row.first_mut() {
            *slot = first;
        }
        row
    }

    /// 解码起点落在区间中段时声明前置静音。
    ///
    /// `5.7.6.4.5` 要求 `HfDelay::carried` 精确等于本区间左边界：缺多少历史是调
    /// 用方的判断，`assemble` 不猜，不等就报 `CarryoverMismatch`。左边界为零时
    /// 这一步是无操作。
    fn prime_carryover(state: &mut AspxChannelState, interval: &AspxInterval, factor: u8) {
        let head = usize::try_from(interval.sig_border(0).unwrap_or(0))
            .unwrap_or(0)
            .saturating_mul(usize::from(factor));
        assert!(state.hf.prefill_silence(head), "前置静音应可声明");
    }

    /// 在两块工作区里各埋一个哨兵，`prepare_frame` 一旦跑过就会被清掉。
    fn plant_sentinels(workspace: &mut AspxWorkspace) {
        workspace.q_low[0].re[0] = 7.0;
        workspace.y[0].im[0] = -7.0;
    }

    fn sentinels_intact(workspace: &AspxWorkspace) -> bool {
        workspace.q_low[0].re[0] == 7.0 && workspace.y[0].im[0] == -7.0
    }

    #[test]
    fn balanced_channels_must_share_one_interval_and_quantiser() {
        // `Pseudocode 84` 逐组把两声道配成一对，前提是两路指的是同一块时频网格。
        // 它自己只核对包络数与组数：边界不同、量化档不同它都看不见，因此这条检
        // 查去掉后不会有任何一处报错，只会算出配错的标度因子。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let (other_params, other_interval) = split_interval_at(SLOTS as u8, SLOTS as u8 / 4);
        assert_ne!(interval, other_interval, "夹具前提：两个区间必须不同");
        assert_eq!(
            (
                interval.num_atsg_sig(),
                interval.freq_res(0),
                interval.freq_res(1)
            ),
            (
                other_interval.num_atsg_sig(),
                other_interval.freq_res(0),
                other_interval.freq_res(1)
            ),
            "夹具前提：只有边界不同，否则联合反量化自己就会报网格失配"
        );

        let base = framing(params, interval, false, false);
        let other_borders = framing(other_params, other_interval, false, false);
        let other_qmode = framing(params, interval, true, false);
        let data = envelope_data(&bands, 0);

        for (label, second) in [("边界不同", &other_borders), ("量化档不同", &other_qmode)]
        {
            let mut states = [AspxChannelState::new(), AspxChannelState::new()];
            let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
            for workspace in &mut workspaces {
                plant_sentinels(workspace);
            }
            let [first_state, second_state] = &mut states;
            let [first_workspace, second_workspace] = &mut workspaces;
            assert_eq!(
                balanced_frame(
                    &bands,
                    &modes,
                    &ramp(FRAME),
                    [
                        EnvelopeInput {
                            framing: &base,
                            data: &data,
                        },
                        EnvelopeInput {
                            framing: second,
                            data: &data,
                        },
                    ],
                    [first_state, second_state],
                    [first_workspace, second_workspace],
                )
                .err(),
                Some(PipelineError::BalancedFramingMismatch),
                "{label}"
            );
            assert!(
                workspaces.iter().all(sentinels_intact),
                "{label}：入参就能判断的失配不该准备工作区"
            );
            assert!(
                states.iter().all(|state| *state == AspxChannelState::new()),
                "{label}：入参就能判断的失配不该推进状态"
            );
        }
    }

    #[test]
    fn balanced_channels_must_have_the_same_frame_length() {
        // 两路帧长不同，`Pseudocode 84` 配出的标度因子会跨到不同的时间轴上。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);

        let first_pcm = ramp(FRAME);
        let second_pcm = ramp(FRAME + SUBBANDS * 8);
        let mut first_out = vec![0.0f32; first_pcm.len()];
        let mut second_out = vec![0.0f32; second_pcm.len()];
        let mut states = [AspxChannelState::new(), AspxChannelState::new()];
        let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        for workspace in &mut workspaces {
            plant_sentinels(workspace);
        }
        let mut intermediates = [AspxIntermediates::default(), AspxIntermediates::default()];
        let [first_state, second_state] = &mut states;
        let [first_workspace, second_workspace] = &mut workspaces;
        let [first_factors, second_factors] = &mut intermediates;
        assert_eq!(
            frame_with_balanced_scale_factors(
                [&first_pcm, &second_pcm],
                &bands,
                [hf_params(&modes), hf_params(&modes)],
                [
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                ],
                [adjust_params(&bands), adjust_params(&bands)],
                [first_state, second_state],
                [first_workspace, second_workspace],
                [first_factors, second_factors],
                [&mut first_out, &mut second_out],
            )
            .err(),
            Some(PipelineError::BalancedFrameLengthMismatch {
                first: FRAME,
                second: FRAME + SUBBANDS * 8,
            })
        );
        assert!(
            workspaces.iter().all(sentinels_intact),
            "帧长不符不该准备工作区"
        );
    }

    #[test]
    fn neither_workspace_is_prepared_until_both_channels_validate() {
        // 两路帧长相同，但第二路的输出切片长度不符。第一路的工作区不该已经清过
        // ——一旦清了，调用方拿到错误后要么重跑要么丢帧，而工作区已经不是原样。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);

        let pcm = ramp(FRAME);
        let mut first_out = vec![0.0f32; FRAME];
        let mut second_out = vec![0.0f32; FRAME - SUBBANDS];
        let mut states = [AspxChannelState::new(), AspxChannelState::new()];
        let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        for workspace in &mut workspaces {
            plant_sentinels(workspace);
        }
        let mut intermediates = [AspxIntermediates::default(), AspxIntermediates::default()];
        let [first_state, second_state] = &mut states;
        let [first_workspace, second_workspace] = &mut workspaces;
        let [first_factors, second_factors] = &mut intermediates;
        assert_eq!(
            frame_with_balanced_scale_factors(
                [&pcm, &pcm],
                &bands,
                [hf_params(&modes), hf_params(&modes)],
                [
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                ],
                [adjust_params(&bands), adjust_params(&bands)],
                [first_state, second_state],
                [first_workspace, second_workspace],
                [first_factors, second_factors],
                [&mut first_out, &mut second_out],
            )
            .err(),
            Some(PipelineError::OutputLengthMismatch {
                expected: FRAME,
                provided: FRAME - SUBBANDS,
            })
        );
        assert!(
            workspaces.iter().all(sentinels_intact),
            "第二路验不过时第一路的工作区也不该已经清过"
        );
    }

    #[test]
    fn each_balanced_channel_reads_its_own_envelope_history() {
        // 第二帧里和声道走频率方向、平衡声道走时间方向：此时和声道的历史无人
        // 读取，平衡声道只该读自己的。因此只改**和声道第一帧**的差分符号，平衡
        // 声道第二帧的标度因子必须逐位不变。
        //
        // 这是「应当无关」判据，方向不能倒过来——`Pseudocode 84` 的 `nom` 由和
        // 声道给出，所以平衡声道的结果本来就依赖和声道的**当帧**数据，只有历史
        // 这一路才该无关。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frequency = framing(params, interval, false, false);
        let time_first = framing(params, interval, false, true);
        let long = ramp(FRAME * 2);
        let balance_data = envelope_data(&bands, 3);
        // 次帧的和声道数据固定：它经 `nom` 影响两路，只有第一帧那份才是要隔离
        // 的自变量。第一版把同一份数据同时用在两帧上，于是和声道的**当帧**输入
        // 也跟着变，判据抓到的是那条合法依赖而不是历史串路。
        let sum_second = envelope_data(&bands, 7);

        let run = |sum_seed: i16| {
            let mut states = [AspxChannelState::new(), AspxChannelState::new()];
            let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
            let sum_first = envelope_data(&bands, sum_seed);
            {
                let [first_state, second_state] = &mut states;
                let [first_workspace, second_workspace] = &mut workspaces;
                balanced_frame(
                    &bands,
                    &modes,
                    &long[..FRAME],
                    [
                        EnvelopeInput {
                            framing: &frequency,
                            data: &sum_first,
                        },
                        EnvelopeInput {
                            framing: &frequency,
                            data: &balance_data,
                        },
                    ],
                    [first_state, second_state],
                    [first_workspace, second_workspace],
                )
                .expect("首帧");
            }
            let history = [states[0].envelope, states[1].envelope];
            let [first_state, second_state] = &mut states;
            let [first_workspace, second_workspace] = &mut workspaces;
            let factors = balanced_frame(
                &bands,
                &modes,
                &long[FRAME..FRAME * 2],
                [
                    EnvelopeInput {
                        framing: &frequency,
                        data: &sum_second,
                    },
                    EnvelopeInput {
                        framing: &time_first,
                        data: &balance_data,
                    },
                ],
                [first_state, second_state],
                [first_workspace, second_workspace],
            )
            .expect("次帧");
            (history, sig_rows(&factors[1].scale_factors))
        };

        let (base_history, base_rows) = run(0);
        let (other_history, other_rows) = run(1);
        // 前提：改和声道的第一帧确实改了它的历史，否则下面的断言恒真。
        assert_ne!(
            base_history[0], other_history[0],
            "夹具前提：和声道的历史必须真的变了"
        );
        assert_eq!(
            base_history[1], other_history[1],
            "平衡声道的历史不该被和声道带偏"
        );
        assert_eq!(base_rows, other_rows, "平衡声道只该读自己的包络历史");
    }

    #[test]
    fn a_failure_after_the_envelope_decode_does_not_commit_the_history() {
        // 包络解码成功、HF 生成失败时，历史必须停在上一帧。`chirp_modes` 个数不
        // 符是在 `generate_hf` 里报的，正好落在解码之后。
        //
        // **次帧的两个包络都要走时间方向。** 存进历史的是本区间**最后**一个包
        // 络，只让第 0 个走时间方向时，第 1 个仍是自身数据的前缀和——两帧数据相
        // 同它就逐位相同，历史根本不会变，下面的相等断言会变成恒真。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let mut extra_modes = modes.clone();
        extra_modes.push(2);
        let (params, interval) = split_interval(SLOTS as u8);
        let first = framing(params, interval, false, false);
        let second = framing_dirs(params, interval, false, [true, true]);
        let data = envelope_data(&bands, 0);
        let long = ramp(FRAME * 2);

        let second_frame = |second_modes: &[u8]| {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            scale_factor_frame(
                &bands,
                &modes,
                &long[..FRAME],
                &first,
                &data,
                &mut state,
                &mut ws,
            )
            .expect("首帧");
            let after_first = state.envelope;
            let result = scale_factor_frame(
                &bands,
                second_modes,
                &long[FRAME..FRAME * 2],
                &second,
                &data,
                &mut state,
                &mut ws,
            );
            (after_first, state.envelope, result.err())
        };

        let (after_first, committed, ok) = second_frame(&modes);
        assert_eq!(ok, None, "基线：足量 chirp 模式时次帧应成功");
        // 前提：这一帧若成功，历史确实会变，否则下面的相等断言恒真。
        assert_ne!(after_first, committed, "夹具前提：成功的次帧必须改写历史");

        let (baseline, unchanged, failed) = second_frame(&extra_modes);
        assert_eq!(
            failed,
            Some(PipelineError::ChirpModeCountMismatch {
                expected: modes.len(),
                provided: extra_modes.len(),
            })
        );
        assert_eq!(baseline, after_first, "两次首帧应得到同一份历史");
        assert_eq!(unchanged, after_first, "失败的帧不得提交包络历史");
    }
    #[test]
    fn the_qmf_exit_skips_synthesis_and_the_pcm_wrapper_is_terminal_only() {
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let pcm = ramp(FRAME);

        let mut qmf_state = AspxChannelState::new();
        let mut qmf_workspace = AspxWorkspace::new();
        let mut qmf_intermediates = AspxIntermediates::default();
        let qmf = frame_with_scale_factors_qmf(
            &pcm,
            &bands,
            hf_params(&modes),
            EnvelopeInput {
                framing: &frame,
                data: &data,
            },
            adjust_params(&bands),
            &mut qmf_state,
            &mut qmf_workspace,
            &mut qmf_intermediates,
        )
        .expect("QMF 域出口")
        .to_vec();
        assert_eq!(qmf.len(), SLOTS, "出口应只递出本帧的有效时隙");
        assert!(
            qmf.iter()
                .any(|slot| slot.re != [0.0; SUBBANDS] || slot.im != [0.0; SUBBANDS]),
            "夹具前提：QMF 出口必须非零，才看得见误跑合成"
        );
        assert_eq!(
            qmf_state.synthesis,
            QmfSynthesisState::new(),
            "QMF 域出口不得推进终端合成状态"
        );

        let mut expected = vec![0.0f32; FRAME];
        let mut expected_synthesis = QmfSynthesisState::new();
        synthesise_ac4_pcm(&qmf, &mut expected_synthesis, &mut expected).expect("独立终端合成");

        let mut pcm_state = AspxChannelState::new();
        let mut pcm_workspace = AspxWorkspace::new();
        let mut pcm_intermediates = AspxIntermediates::default();
        let mut out = vec![0.0f32; FRAME];
        frame_with_scale_factors(
            &pcm,
            &bands,
            hf_params(&modes),
            EnvelopeInput {
                framing: &frame,
                data: &data,
            },
            adjust_params(&bands),
            &mut pcm_state,
            &mut pcm_workspace,
            &mut pcm_intermediates,
            &mut out,
        )
        .expect("PCM 包装器");
        assert_eq!(
            &pcm_workspace.q_out[..SLOTS],
            qmf.as_slice(),
            "PCM 包装器应复用同一个 QMF 出口"
        );
        assert_eq!(out, expected, "包装器只能在 QMF 出口之后增加一次合成");
        assert_eq!(pcm_state.synthesis, expected_synthesis);
    }

    #[test]
    fn independent_channel_pair_matches_two_scalar_calls_bit_for_bit() {
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let framing = framing(params, interval, false, false);
        let first_data = envelope_data(&bands, 0);
        let second_data = envelope_data(&bands, 3);
        let first_pcm = ramp(FRAME * 2);
        let second_pcm: Vec<f32> = ramp(FRAME * 2)
            .into_iter()
            .enumerate()
            .map(|(index, sample)| sample * -0.625 + (index % 17) as f32 * 0.003)
            .collect();

        let mut scalar_states = [AspxChannelState::new(), AspxChannelState::new()];
        let mut paired_states = [AspxChannelState::new(), AspxChannelState::new()];
        let mut scalar_workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        let mut paired_workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        let mut scalar_intermediates = [AspxIntermediates::default(), AspxIntermediates::default()];
        let mut paired_intermediates = [AspxIntermediates::default(), AspxIntermediates::default()];

        for frame in 0..2 {
            let start = frame * FRAME;
            let end = start + FRAME;
            let inputs = [&first_pcm[start..end], &second_pcm[start..end]];
            let scalar_first = frame_with_scale_factors_qmf(
                inputs[0],
                &bands,
                hf_params(&modes),
                EnvelopeInput {
                    framing: &framing,
                    data: &first_data,
                },
                adjust_params(&bands),
                &mut scalar_states[0],
                &mut scalar_workspaces[0],
                &mut scalar_intermediates[0],
            )
            .expect("第一路标量通路")
            .to_vec();
            let scalar_second = frame_with_scale_factors_qmf(
                inputs[1],
                &bands,
                hf_params(&modes),
                EnvelopeInput {
                    framing: &framing,
                    data: &second_data,
                },
                adjust_params(&bands),
                &mut scalar_states[1],
                &mut scalar_workspaces[1],
                &mut scalar_intermediates[1],
            )
            .expect("第二路标量通路")
            .to_vec();

            let paired = {
                let [first_state, second_state] = &mut paired_states;
                let [first_workspace, second_workspace] = &mut paired_workspaces;
                let [first_intermediates, second_intermediates] = &mut paired_intermediates;
                let [first, second] = frame_with_scale_factors_pair_qmf(
                    inputs,
                    [&bands, &bands],
                    [hf_params(&modes), hf_params(&modes)],
                    [
                        EnvelopeInput {
                            framing: &framing,
                            data: &first_data,
                        },
                        EnvelopeInput {
                            framing: &framing,
                            data: &second_data,
                        },
                    ],
                    [adjust_params(&bands), adjust_params(&bands)],
                    [first_state, second_state],
                    [first_workspace, second_workspace],
                    [first_intermediates, second_intermediates],
                )
                .expect("独立双声道通路");
                [first.to_vec(), second.to_vec()]
            };

            assert_eq!(paired[0], scalar_first, "第 {frame} 帧第一路输出");
            assert_eq!(paired[1], scalar_second, "第 {frame} 帧第二路输出");
            assert_eq!(paired_states, scalar_states, "第 {frame} 帧跨帧状态");
            for channel in 0..2 {
                let scalar = &scalar_workspaces[channel];
                let paired = &paired_workspaces[channel];
                assert_eq!(paired.q_in, scalar.q_in, "声道 {channel} 的 Q_in");
                assert_eq!(paired.q_low, scalar.q_low, "声道 {channel} 的 Q_low");
                assert_eq!(paired.q_high, scalar.q_high, "声道 {channel} 的 Q_high");
                assert_eq!(paired.noise, scalar.noise, "声道 {channel} 的噪声");
                assert_eq!(paired.sine, scalar.sine, "声道 {channel} 的音调");
                assert_eq!(paired.y, scalar.y, "声道 {channel} 的 Y");
                assert_eq!(paired.q_out, scalar.q_out, "声道 {channel} 的 Q_out");
            }
        }
    }

    #[test]
    fn the_balanced_qmf_exit_skips_both_synthesis_states() {
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let first_pcm = ramp(FRAME);
        let second_pcm: Vec<f32> = first_pcm.iter().rev().map(|value| value * 0.5).collect();
        let mut states = [AspxChannelState::new(), AspxChannelState::new()];
        let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        let mut intermediates = [AspxIntermediates::default(), AspxIntermediates::default()];

        {
            let [first_state, second_state] = &mut states;
            let [first_workspace, second_workspace] = &mut workspaces;
            let [first_intermediates, second_intermediates] = &mut intermediates;
            let [first_qmf, second_qmf] = frame_with_balanced_scale_factors_qmf(
                [&first_pcm, &second_pcm],
                &bands,
                [hf_params(&modes), hf_params(&modes)],
                [
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                ],
                [adjust_params(&bands), adjust_params(&bands)],
                [first_state, second_state],
                [first_workspace, second_workspace],
                [first_intermediates, second_intermediates],
            )
            .expect("平衡式 QMF 域出口");
            assert_eq!(first_qmf.len(), SLOTS);
            assert_eq!(second_qmf.len(), SLOTS);
            assert_ne!(first_qmf, second_qmf, "两路不同输入不应串成同一输出");
        }

        assert!(
            states
                .iter()
                .all(|state| state.synthesis == QmfSynthesisState::new()),
            "QMF 域出口不得推进任一路终端合成状态"
        );
    }

    /// 平衡式包装器的两路终端合成必须各归各路。
    ///
    /// **一帧看不出来**：两路的合成历史都从全零起，把它们对调在第一帧上逐位
    /// 等价。注入实测——交换 `synthesise` 的两个状态实参，此前全部判据无一响。
    /// 要让它显形得跑到第二帧，且第一帧结束时两路历史必须已经不同，故两路四
    /// 段 PCM 全不相同，并把这条前提写成前置断言。
    ///
    /// 判据是「包装器等于逐路各自独立合成」：参照侧走 QMF 域出口，自己持有两
    /// 份 [`QmfSynthesisState`]，串线在这里没有藏身处。
    #[test]
    fn each_balanced_channel_keeps_its_own_synthesis_history() {
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);

        let long = ramp(FRAME * 2);
        let first: [Vec<f32>; 2] = [long[..FRAME].to_vec(), long[FRAME..].to_vec()];
        let second: [Vec<f32>; 2] = [
            first[0].iter().rev().map(|value| value * 0.5).collect(),
            first[1].iter().rev().map(|value| value * 0.25).collect(),
        ];
        let segments = [&first[0], &first[1], &second[0], &second[1]];
        for (index, left) in segments.iter().enumerate() {
            for right in segments.iter().skip(index.saturating_add(1)) {
                assert_ne!(left, right, "夹具前提：四段 PCM 必须两两不同");
            }
        }

        let inputs = || {
            [
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
            ]
        };

        // 参照侧：QMF 域出口加两份自己持有的合成状态。
        let mut ref_states = [AspxChannelState::new(), AspxChannelState::new()];
        let mut ref_workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        let mut ref_intermediates = [AspxIntermediates::default(), AspxIntermediates::default()];
        let mut synthesis = [QmfSynthesisState::new(), QmfSynthesisState::new()];
        let mut expected = [vec![0.0f32; FRAME], vec![0.0f32; FRAME]];
        for pcm in [&first, &second] {
            let [first_state, second_state] = &mut ref_states;
            let [first_workspace, second_workspace] = &mut ref_workspaces;
            let [first_mid, second_mid] = &mut ref_intermediates;
            let [first_qmf, second_qmf] = frame_with_balanced_scale_factors_qmf(
                [&pcm[0], &pcm[1]],
                &bands,
                [hf_params(&modes), hf_params(&modes)],
                inputs(),
                [adjust_params(&bands), adjust_params(&bands)],
                [first_state, second_state],
                [first_workspace, second_workspace],
                [first_mid, second_mid],
            )
            .expect("参照侧的 QMF 域出口");
            for (channel, qmf) in [first_qmf, second_qmf].into_iter().enumerate() {
                synthesise_ac4_pcm(qmf, &mut synthesis[channel], &mut expected[channel])
                    .expect("逐路独立合成");
            }
        }
        assert_ne!(
            synthesis[0], synthesis[1],
            "夹具前提：两路的合成历史必须不同，否则对调它们不可观察"
        );

        // 被测侧：同样两帧走 PCM 包装器。
        let mut states = [AspxChannelState::new(), AspxChannelState::new()];
        let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        let mut out = [Vec::new(), Vec::new()];
        for pcm in [&first, &second] {
            let [first_state, second_state] = &mut states;
            let [first_workspace, second_workspace] = &mut workspaces;
            let (_, produced) = balanced_frame_split(
                &bands,
                &modes,
                [&pcm[0], &pcm[1]],
                inputs(),
                [first_state, second_state],
                [first_workspace, second_workspace],
            )
            .expect("平衡式 PCM 包装器");
            out = produced;
        }

        for channel in 0..2 {
            assert_eq!(
                out[channel], expected[channel],
                "第 {channel} 路必须用自己那份合成历史"
            );
            // 只比 PCM 还不够：把两路的 `synthesis` 实参**整体**对调是逐帧一致
            // 的重命名，两路输出逐位不变，注入实测无一判据响。它的后果在状态
            // 侧——历史落进了另一路的 [`AspxChannelState`]，等到按声道重置或
            // 重新配对时清掉的就是错的那份。故这里还要认状态的归属。
            assert_eq!(
                states[channel].synthesis, synthesis[channel],
                "第 {channel} 路的合成历史必须留在自己的状态里"
            );
        }
    }

    #[test]
    fn the_balanced_path_matches_the_bypass_on_both_channels() {
        // 两路各给一段**不同**的信号：相同信号会让两路串线完全不可观察。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);

        let first_pcm = ramp(FRAME);
        let second_pcm: Vec<f32> = first_pcm.iter().rev().map(|value| value * 0.5).collect();
        assert_ne!(first_pcm, second_pcm, "夹具前提：两路信号必须不同");

        let mut states = [AspxChannelState::new(), AspxChannelState::new()];
        let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        let (_, out) = {
            let [first_state, second_state] = &mut states;
            let [first_workspace, second_workspace] = &mut workspaces;
            balanced_frame_split(
                &bands,
                &modes,
                [&first_pcm, &second_pcm],
                [
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                ],
                [first_state, second_state],
                [first_workspace, second_workspace],
            )
            .expect("平衡式双声道路径")
        };

        // 两路各自等于「自己那段被延迟的输入」加上自己的 `Y`。串线会让某一路
        // 的差值对不上自己的 `Q_in`——两路信号不同，这一点才有鉴别力。
        for (index, pcm) in [&first_pcm, &second_pcm].into_iter().enumerate() {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            let mut delayed = vec![0.0f32; pcm.len()];
            bypass_frame(pcm, &mut state, &mut ws, &mut delayed).expect("旁路");
            assert_ne!(out[index], delayed, "第 {index} 路的 Y 必须已经汇入输出");

            let mut synthesis = QmfSynthesisState::new();
            let mut expected = vec![0.0f32; pcm.len()];
            let mut summed = ws.q_out[..SLOTS].to_vec();
            for (slot, y) in summed.iter_mut().zip(workspaces[index].y.iter()) {
                for sb in 0..SUBBANDS {
                    slot.re[sb] += y.re[sb];
                    slot.im[sb] += y.im[sb];
                }
            }
            synthesise_ac4_pcm(&summed, &mut synthesis, &mut expected).expect("合成");
            assert_eq!(out[index], expected, "第 {index} 路应是延迟输入加自己的 Y");
        }
    }
    #[test]
    fn the_mono_path_uses_the_single_delta_step() {
        // `Pseudocode 80`/`81` 的双倍步长只属于 `ch == 1 && aspx_balance == 1`，
        // 那种流必须走 [`frame_with_balanced_scale_factors`]。本入口恒用步长 1。
        //
        // 频率方向、首个差分符号取 2、其余为零 → `qscf` 恒为 2；细档
        // `scf = 2^(2/2) · 64 = 128`，且指数为整数，等号是精确的。步长若取 2，
        // `qscf` 变 4，`scf` 变 256。期望值从 `Pseudocode 82` 抄来，不引用实现
        // 里的任何常量。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let high = frequency_deltas(bands.num_sbg_sig_highres(), 2);
        let low = frequency_deltas(bands.num_sbg_sig_lowres(), 2);
        let noise = frequency_deltas(bands.num_sbg_noise(), 0);
        let data = AspxEnvelopes::for_test(&[&high, &low], &[&noise, &noise]);

        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        let (_, factors) = scale_factor_frame(
            &bands,
            &modes,
            &ramp(FRAME),
            &frame,
            &data,
            &mut state,
            &mut ws,
        )
        .expect("标度因子路径");

        let rows = sig_rows(&factors.scale_factors);
        assert_eq!(rows.len(), 2, "应解出两个信号包络");
        for (envelope, row) in rows.iter().enumerate() {
            assert!(!row.is_empty(), "第 {envelope} 个包络不该为空");
            assert_eq!(*row, vec![128.0; row.len()], "第 {envelope} 个包络");
        }
    }
    /// 表 192 的两档倍率各取一档帧长：倍率 1 配 1 024 样本、倍率 2 配 1 536。
    ///
    /// **两档都要跑。** 只跑倍率 1 时，「边界忘乘倍率」与「倍率写死为 1」这两类
    /// 缺陷全都恒等于正确写法，注入实测两条判据都不响。
    const FACTOR_TIERS: [(usize, usize, u8, u8); 2] =
        [(FRAME, SLOTS, 1, SLOTS as u8), (24 * SUBBANDS, 24, 2, 12)];

    #[test]
    fn the_high_band_follows_the_interval_not_the_frame() {
        // `Pseudocode 89` 的 `ts` 从 `atsg_sig[0] · num_ts_in_ats` 起，`Q_high[0]`
        // 对应的就是那一格。左边界挪 `shift` 个 ATS 后，两次运行的 `Q_high` 只该
        // 整体错开 `shift · num_ts_in_ats` 格——同一个 `Q_low_ext` 时隙在两边都读
        // 到，逐位相同。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let data = envelope_data(&bands, 0);
        const SHIFT: u8 = 2;

        for (samples, slots, factor, aspx_slots) in FACTOR_TIERS {
            let pcm = ramp(samples);
            let shift = usize::from(SHIFT) * usize::from(factor);
            let (whole_params, whole_interval) = split_interval(aspx_slots);
            let (shift_params, shift_interval) = left_shifted_interval(aspx_slots, SHIFT);
            let whole = framing(whole_params, whole_interval, false, false);
            let shifted = framing(shift_params, shift_interval, false, false);

            let base = scale_factor_q_high(&bands, &modes, &pcm, &whole, &data, false, slots);
            let moved =
                scale_factor_q_high(&bands, &modes, &pcm, &shifted, &data, false, slots - shift);
            assert_eq!(
                moved,
                base[shift..].to_vec(),
                "倍率 {factor}：左边界挪 {SHIFT} 个 ATS，Q_high 应错开 {shift} 格"
            );
            // 前提：错开确实改变内容，否则「区间被忽略」时上面的相等也成立。
            assert_ne!(base[..slots - shift].to_vec(), base[shift..].to_vec());

            // 预平坦化的增益按同一区间算，换了区间就不该再有那条错位相等关系。
            let flat_base = scale_factor_q_high(&bands, &modes, &pcm, &whole, &data, true, slots);
            let flat_moved =
                scale_factor_q_high(&bands, &modes, &pcm, &shifted, &data, true, slots - shift);
            assert_ne!(
                flat_base[..slots - shift].to_vec(),
                flat_base[shift..].to_vec(),
                "倍率 {factor} 的夹具前提：预平坦化下整段仍非平凡"
            );
            assert_ne!(
                flat_moved,
                flat_base[shift..].to_vec(),
                "倍率 {factor}：预平坦化增益也要按区间算"
            );
        }
    }

    #[test]
    fn a_right_border_past_the_frame_end_still_fits_in_q_low() {
        // `aspx_var_bord_right` 至多 3（表 53 给它 2 比特），越帧量是它乘倍率；
        // `Q_low` 比本帧长 `ts_offset_hfgen`，表 192 里倍率 1 恒配 3、倍率 2 恒配
        // 6。取满 3 就是两档各自的紧边界，再多一格 `hf_generate` 就会报
        // `IntervalOutOfRange`。两档都要跑：只跑倍率 1 时越帧量恒为 3，倍率这一
        // 维在越帧上也不可观察。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let data = envelope_data(&bands, 0);
        const OVERRUN: u8 = 3;

        for (samples, slots, factor, aspx_slots) in FACTOR_TIERS {
            let overrun = usize::from(OVERRUN) * usize::from(factor);
            assert_eq!(
                ts_offset_hfgen(samples as u16),
                Some(overrun as u8),
                "夹具前提：倍率 {factor} 的 ts_offset_hfgen 应恰等于越帧上界"
            );

            let (params, interval) = right_overrun_interval(aspx_slots, OVERRUN);
            let frame = framing(params, interval, false, false);

            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            // 哨兵落在越帧的那几格：`prepare_frame` 不清 `q_high`，只有真的生成
            // 到那里才会被覆盖。
            for slot in ws.q_high[slots..slots + overrun].iter_mut() {
                slot.re[usize::from(bands.sbx())] = 12_345.0;
            }
            let mut intermediates = AspxIntermediates::default();
            let mut out = vec![0.0f32; samples];
            frame_with_scale_factors(
                &ramp(samples),
                &bands,
                hf_params(&modes),
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
                adjust_params(&bands),
                &mut state,
                &mut ws,
                &mut intermediates,
                &mut out,
            )
            .expect("右边界越过帧尾仍应生成得出");
            for (index, slot) in ws.q_high[slots..slots + overrun].iter().enumerate() {
                assert_ne!(
                    slot.re[usize::from(bands.sbx())],
                    12_345.0,
                    "倍率 {factor}：越帧的第 {index} 格必须真的生成过"
                );
            }
        }
    }

    #[test]
    fn an_interval_from_another_frame_layout_is_rejected() {
        // 可变右边界让 `stop_pos` 大于名义时隙数，故不能从边界反推帧长。区间自
        // 带的来源值与倍率配回表 189/192 后若不是本帧的时隙数，直接报错。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let data = envelope_data(&bands, 0);
        let (params, interval) = split_interval(12);
        let frame = framing(params, interval, false, false);

        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        plant_sentinels(&mut ws);
        let mut intermediates = AspxIntermediates::default();
        let mut out = vec![0.0f32; FRAME];
        assert_eq!(
            frame_with_scale_factors(
                &ramp(FRAME),
                &bands,
                hf_params(&modes),
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
                adjust_params(&bands),
                &mut state,
                &mut ws,
                &mut intermediates,
                &mut out,
            )
            .err(),
            Some(PipelineError::IntervalFrameMismatch {
                expected: 12,
                frame: SLOTS,
            })
        );
        assert!(sentinels_intact(&ws), "区间布局不符不该准备工作区");
    }
    // ---- 5.7.6.4.2 包络估计与补偿增益 ----

    #[test]
    fn hf_adjust_parameters_are_checked_before_any_state_moves() {
        // 第二路故意无效：两路的 HF 调整参数必须都在任一工作区清理前验完，不能让
        // 第一声道先推进一半。单声道另验一次，防止只有平衡入口前移了检查。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let pcm = ramp(FRAME);
        let valid = adjust_params(&bands);
        let invalid = HfAdjustParams {
            add_harmonic: &[],
            ..valid
        };
        let expected = PipelineError::HfAdjust(HfAdjustError::HarmonicDataTooShort {
            needed: usize::from(bands.num_sbg_sig_highres()),
            provided: 0,
        });

        let mut state = AspxChannelState::new();
        let mut workspace = AspxWorkspace::new();
        plant_sentinels(&mut workspace);
        let mut intermediates = AspxIntermediates::default();
        let mut out = vec![0.0f32; FRAME];
        assert_eq!(
            frame_with_scale_factors(
                &pcm,
                &bands,
                hf_params(&modes),
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
                invalid,
                &mut state,
                &mut workspace,
                &mut intermediates,
                &mut out,
            ),
            Err(expected)
        );
        assert_eq!(state, AspxChannelState::new(), "单声道不应推进状态");
        assert!(sentinels_intact(&workspace), "单声道不应准备工作区");

        let mut states = [AspxChannelState::new(), AspxChannelState::new()];
        let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        for workspace in &mut workspaces {
            plant_sentinels(workspace);
        }
        let mut intermediates = [AspxIntermediates::default(), AspxIntermediates::default()];
        let mut out = [vec![0.0f32; FRAME], vec![0.0f32; FRAME]];
        let [first_state, second_state] = &mut states;
        let [first_workspace, second_workspace] = &mut workspaces;
        let [first_intermediates, second_intermediates] = &mut intermediates;
        let [first_out, second_out] = &mut out;
        assert_eq!(
            frame_with_balanced_scale_factors(
                [&pcm, &pcm],
                &bands,
                [hf_params(&modes), hf_params(&modes)],
                [
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                ],
                [valid, invalid],
                [first_state, second_state],
                [first_workspace, second_workspace],
                [first_intermediates, second_intermediates],
                [first_out, second_out],
            ),
            Err(expected)
        );
        assert!(
            states.iter().all(|state| *state == AspxChannelState::new()),
            "第二路参数无效时两路状态都不应推进"
        );
        assert!(
            workspaces.iter().all(sentinels_intact),
            "第二路参数无效时两块工作区都不应准备"
        );
    }

    #[test]
    fn every_hf_adjust_parameter_reaches_its_consumer() {
        // 三个开关各翻一次，补偿增益都必须变。它们分别落在 `Pseudocode 92`
        // （正弦标记）、`90`（时间插值）与 `96`–`101`（限幅与 boost）；直接看中间
        // 增益能把每一条接线单独定位，不依赖后续组装后的综合变化。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = split_interval(SLOTS as u8);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let groups = usize::from(bands.num_sbg_sig_highres());

        let run = |adjust: HfAdjustParams<'_>| {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            let mut intermediates = AspxIntermediates::default();
            let mut out = vec![0.0f32; FRAME];
            frame_with_scale_factors(
                &ramp(FRAME),
                &bands,
                hf_params(&modes),
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
                adjust,
                &mut state,
                &mut ws,
                &mut intermediates,
                &mut out,
            )
            .expect("标度因子路径");
            gain_row(&intermediates, &bands, 0)
        };

        let base = run(adjust_params(&bands));
        assert!(
            base.iter().any(|value| *value != 0.0),
            "夹具前提：增益不能恒为零"
        );

        let no_harmonics = vec![false; groups];
        let mut without = adjust_params(&bands);
        without.add_harmonic = &no_harmonics;
        assert_ne!(base, run(without), "aspx_add_harmonic 必须传到 5.7.6.4.2.1");

        let mut no_interp = adjust_params(&bands);
        no_interp.interpolation = false;
        assert_ne!(
            base,
            run(no_interp),
            "aspx_interpolation 必须传到 5.7.6.4.2.1"
        );

        let mut no_limit = adjust_params(&bands);
        no_limit.limiter = false;
        assert_ne!(base, run(no_limit), "aspx_limiter 必须传到 5.7.6.4.2.2");
    }

    #[test]
    fn the_sine_continuation_carries_across_frames() {
        // `Pseudocode 92` 的 `p_sine_idx` 取上一区间最后一列。只丢掉
        // `AspxChannelState::sine`、其余状态照常延续，次帧的增益就必须变。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = pointed_interval(SLOTS as u8, 1);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let long = ramp(FRAME * 2);

        let second_frame = |drop_sine: bool| {
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            let mut intermediates = AspxIntermediates::default();
            let mut out = vec![0.0f32; FRAME];
            let mut run = |pcm: &[f32], state: &mut AspxChannelState| {
                frame_with_scale_factors(
                    pcm,
                    &bands,
                    hf_params(&modes),
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    adjust_params(&bands),
                    state,
                    &mut ws,
                    &mut intermediates,
                    &mut out,
                )
                .expect("标度因子路径");
            };
            run(&long[..FRAME], &mut state);
            // 前提：首帧确实在这个状态里留下了东西，否则「丢掉」什么也没丢。
            assert_ne!(
                state.sine,
                SineState::new(),
                "夹具前提：首帧必须写过正弦延续状态"
            );
            if drop_sine {
                state.sine = SineState::new();
            }
            run(&long[FRAME..FRAME * 2], &mut state);
            gain_row(&intermediates, &bands, 0)
        };

        assert_ne!(
            second_frame(false),
            second_frame(true),
            "只丢掉正弦延续也必须改变补偿增益"
        );
    }

    #[test]
    fn the_gains_carry_this_frames_timeslot_factor() {
        // `AdjustedGains` 自带推导时用的倍率，`5.7.6.4.5` 会拿它跟成帧来源核对。
        // 通路必须把表 192 查出的那个值传进 `5.7.6.4.2.1`，而不是复用帧时隙数或
        // 写死成 1。
        //
        // **两档都要跑**：只跑倍率 1 时「写死为 1」与正确写法逐位相同，注入实测
        // 一条判据都不响。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let data = envelope_data(&bands, 0);

        for (samples, _, factor, aspx_slots) in FACTOR_TIERS {
            let (params, interval) = split_interval(aspx_slots);
            let frame = framing(params, interval, false, false);
            let mut state = AspxChannelState::new();
            let mut ws = AspxWorkspace::new();
            let mut intermediates = AspxIntermediates::default();
            let mut out = vec![0.0f32; samples];
            frame_with_scale_factors(
                &ramp(samples),
                &bands,
                hf_params(&modes),
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
                adjust_params(&bands),
                &mut state,
                &mut ws,
                &mut intermediates,
                &mut out,
            )
            .expect("标度因子路径");
            assert_eq!(
                intermediates.gains.source_num_ts_in_ats(),
                factor,
                "{samples} 样本帧的倍率应为 {factor}"
            );
        }
    }

    #[test]
    fn hf_carryover_is_checked_before_any_state_moves() {
        // 起解点缺前置静音是调用方可修正的状态前提。单声道要原样返回；平衡入口
        // 则故意让第一路合法、第二路缺失，证明两路都在第一路开始工作前验完。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = steady_state_interval(SLOTS as u8, 2);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let pcm = ramp(FRAME);
        let expected = PipelineError::Assemble(AssembleError::CarryoverMismatch {
            carried: 0,
            required: 2,
        });

        let mut state = AspxChannelState::new();
        let mut workspace = AspxWorkspace::new();
        plant_sentinels(&mut workspace);
        let mut intermediates = AspxIntermediates::default();
        let mut out = vec![0.0f32; FRAME];
        assert_eq!(
            frame_with_scale_factors(
                &pcm,
                &bands,
                hf_params(&modes),
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
                adjust_params(&bands),
                &mut state,
                &mut workspace,
                &mut intermediates,
                &mut out,
            ),
            Err(expected)
        );
        assert_eq!(state, AspxChannelState::new(), "单声道不应推进任何状态");
        assert!(sentinels_intact(&workspace), "单声道不应准备工作区");

        let mut states = [AspxChannelState::new(), AspxChannelState::new()];
        assert!(states[0].hf.prefill_silence(2), "第一路应满足携带量前提");
        let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
        for workspace in &mut workspaces {
            plant_sentinels(workspace);
        }
        let mut intermediates = [AspxIntermediates::default(), AspxIntermediates::default()];
        let mut out = [vec![0.0f32; FRAME], vec![0.0f32; FRAME]];
        let [first_state, second_state] = &mut states;
        let [first_workspace, second_workspace] = &mut workspaces;
        let [first_intermediates, second_intermediates] = &mut intermediates;
        let [first_out, second_out] = &mut out;
        assert_eq!(
            frame_with_balanced_scale_factors(
                [&pcm, &pcm],
                &bands,
                [hf_params(&modes), hf_params(&modes)],
                [
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                ],
                [adjust_params(&bands), adjust_params(&bands)],
                [first_state, second_state],
                [first_workspace, second_workspace],
                [first_intermediates, second_intermediates],
                [first_out, second_out],
            ),
            Err(expected)
        );
        let mut expected_first = AspxChannelState::new();
        assert!(expected_first.hf.prefill_silence(2));
        assert_eq!(
            states[0], expected_first,
            "第二路失败时第一路状态应保持原样"
        );
        assert_eq!(
            states[1],
            AspxChannelState::new(),
            "第二路失败时自身状态也应保持原样"
        );
        assert!(
            workspaces.iter().all(sentinels_intact),
            "第二路失败时两块工作区都不应准备"
        );
    }

    /// 跑两帧，返回第二帧的 `Y`；`disturb` 在两帧之间只动一个跨帧状态。
    ///
    /// 用两端各偏两格的区间：`HfDelay` 携带的是越帧成品，左边界为零时它恒空，
    /// 「只丢掉 hf」根本丢不掉任何东西；而只偏左端会让第二帧直接报携带量不符。
    fn second_frame_y(
        bands: &AspxBandTables,
        modes: &[u8],
        second_adjust: HfAdjustParams<'_>,
        disturb: fn(&mut AspxChannelState),
    ) -> Vec<QmfSlot> {
        // **帧长取 15 个 QMF 时隙，不是 16。** 音调表只有四项（`i` 的前四个幂），
        // 区间长度是 4 的倍数时游标每帧都绕回原处，`ToneCursor` 与 `first_frame`
        // 两条判据会一起失效——注入实测 16 时隙下它们都不响。
        const SLOTS15: usize = 15;
        const FRAME15: usize = SLOTS15 * SUBBANDS;
        assert_ne!(SLOTS15 % 4, 0, "夹具前提：区间长度不得是音调表长的倍数");

        let (params, interval) = steady_state_interval(SLOTS15 as u8, 2);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(bands, 0);
        let long = ramp(FRAME15 * 2);
        let mut state = AspxChannelState::new();
        let mut ws = AspxWorkspace::new();
        prime_carryover(&mut state, &interval, 1);
        let mut intermediates = AspxIntermediates::default();
        let mut out = vec![0.0f32; FRAME15];
        let mut run = |pcm: &[f32], adjust: HfAdjustParams<'_>, state: &mut AspxChannelState| {
            frame_with_scale_factors(
                pcm,
                bands,
                hf_params(modes),
                EnvelopeInput {
                    framing: &frame,
                    data: &data,
                },
                adjust,
                state,
                &mut ws,
                &mut intermediates,
                &mut out,
            )
            .expect("整条通路");
        };
        run(&long[..FRAME15], adjust_params(bands), &mut state);
        disturb(&mut state);
        run(&long[FRAME15..FRAME15 * 2], second_adjust, &mut state);
        ws.y[..SLOTS15].to_vec()
    }

    #[test]
    fn each_cross_frame_state_on_the_assembly_path_is_observable() {
        // 一次只丢一个。三者分别是噪声表游标、音调表游标与越帧的已组装信号，
        // 整组重启也能让 `Y` 变，但那种判据里任一条失延续都被另外两条兜住。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let baseline = second_frame_y(&bands, &modes, adjust_params(&bands), |_| {});

        type Disturbance = (&'static str, fn(&mut AspxChannelState));
        let cases: [Disturbance; 3] = [
            ("噪声表游标", |s| s.noise = NoiseCursor::new()),
            ("音调表游标", |s| s.tone = ToneCursor::new()),
            ("越帧的已组装信号", |s| {
                s.hf = HfDelay::new();
                assert!(s.hf.prefill_silence(2), "改成前置静音以便本帧仍能组装");
            }),
        ];
        for (name, disturb) in cases {
            let disturbed = second_frame_y(&bands, &modes, adjust_params(&bands), disturb);
            assert_ne!(disturbed, baseline, "只丢掉{name}也必须改变 Y");
        }
    }

    #[test]
    fn the_two_reset_conditions_reach_their_generators() {
        // `5.7.6.3.1.1` 的 `master_reset` 与正文的 `first_frame` **不是同一个条
        // 件**，各自只影响一张表的基址。两者在首帧都与「不重置」无从区分——游标
        // 本来就是初值——因此必须在第二帧上翻。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let baseline = second_frame_y(&bands, &modes, adjust_params(&bands), |_| {});

        let mut reset_noise = adjust_params(&bands);
        reset_noise.master_reset = true;
        assert_ne!(
            second_frame_y(&bands, &modes, reset_noise, |_| {}),
            baseline,
            "master_reset 必须传到 5.7.6.4.3"
        );

        let mut reset_tone = adjust_params(&bands);
        reset_tone.first_frame = true;
        assert_ne!(
            second_frame_y(&bands, &modes, reset_tone, |_| {}),
            baseline,
            "first_frame 必须传到 5.7.6.4.4"
        );
    }

    #[test]
    fn each_balanced_channel_keeps_its_own_carryover() {
        // 两路各有一份越帧成品。只动第一路的，第二路的 `Y` 必须逐位不变。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let (params, interval) = steady_state_interval(SLOTS as u8, 2);
        let frame = framing(params, interval, false, false);
        let data = envelope_data(&bands, 0);
        let pcm = ramp(FRAME);

        let run = |disturb_first: bool| {
            let mut states = [AspxChannelState::new(), AspxChannelState::new()];
            for state in &mut states {
                prime_carryover(state, &interval, 1);
            }
            if disturb_first {
                // 第一路改成「起点没有前置静音，而是一段非零残留」。
                for ts in 0..2 {
                    states[0].hf.tail_mut_for_test(ts).re[usize::from(bands.sbx())] = 0.25;
                }
            }
            let mut workspaces = [AspxWorkspace::new(), AspxWorkspace::new()];
            let [first_state, second_state] = &mut states;
            let [first_workspace, second_workspace] = &mut workspaces;
            balanced_frame(
                &bands,
                &modes,
                &pcm,
                [
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                ],
                [first_state, second_state],
                [first_workspace, second_workspace],
            )
            .expect("平衡式双声道路径");
            let [first_state, second_state] = states;
            (
                workspaces[0].y[..SLOTS].to_vec(),
                workspaces[1].y[..SLOTS].to_vec(),
                first_state.hf,
                second_state.hf,
            )
        };

        let (base_first, base_second, first_tail, second_tail) = run(false);
        let (moved_first, moved_second, _, _) = run(true);
        // 前提：动过的那一路确实变了，否则「另一路不变」恒成立。
        assert_ne!(base_first, moved_first, "夹具前提：第一路必须真的被改动");
        assert_eq!(base_second, moved_second, "第二路不该读到第一路的越帧成品");
        // 两路各留下自己的越帧成品。**光比两者不等还不够**：共用一份状态时第二
        // 路那份根本不会被写，停在前置静音上，而它与第一路算出的成品当然也不
        // 等。要比的是它有没有离开起点——注入实测只写不等的那一版是沉默的。
        let mut untouched = HfDelay::new();
        assert!(untouched.prefill_silence(2), "参照的前置静音应可声明");
        assert_ne!(second_tail, untouched, "第二路的越帧成品必须由它自己写过");
        assert_ne!(first_tail, second_tail, "两路的越帧成品不该是同一份");
    }
    #[test]
    fn out_of_range_carryover_is_checked_before_any_state_moves() {
        // 左右可变边界都是 2 比特，倍率 1 时最多携带 3 个 QMF 时隙。两边各越界
        // 一次：只挡左边会让右边仍在 HF 生成推进状态后才由组装器拒绝，反之亦然。
        let bands = hf_bands();
        let modes = chirp_modes(&bands);
        let data = envelope_data(&bands, 0);
        let cases: [(&str, u8, u8, usize); 2] = [("左边界", 5, 2, 5), ("右边界", 0, 4, 4)];

        for (name, left, right, carryover) in cases {
            let mut params = AspxIntervalParams::fixfix(2);
            params.int_class = IntervalClass::VarVar;
            params.var_bord_left = Some(left);
            params.var_bord_right = Some(right);
            params.num_rel_right = 1;
            params.rel_bord_right[0] = 8;
            params.freq_res = [true, false, false, false, false];
            let interval =
                AspxInterval::derive(&params, SLOTS as u8, 0, true, SLOTS as i16).expect("区间");

            let frame = framing(params, interval, false, false);
            let mut state = AspxChannelState::new();
            assert!(
                state.hf.prefill_silence(usize::from(left)),
                "{name}夹具的前置静音应可声明"
            );
            let mut expected_state = AspxChannelState::new();
            assert!(expected_state.hf.prefill_silence(usize::from(left)));
            let mut workspace = AspxWorkspace::new();
            plant_sentinels(&mut workspace);
            let mut intermediates = AspxIntermediates::default();
            let mut out = vec![0.0f32; FRAME];
            assert_eq!(
                frame_with_scale_factors(
                    &ramp(FRAME),
                    &bands,
                    hf_params(&modes),
                    EnvelopeInput {
                        framing: &frame,
                        data: &data,
                    },
                    adjust_params(&bands),
                    &mut state,
                    &mut workspace,
                    &mut intermediates,
                    &mut out,
                ),
                Err(PipelineError::Assemble(
                    AssembleError::CarryoverOutOfRange { carryover }
                )),
                "{name}应在状态推进前被拒绝"
            );
            assert_eq!(state, expected_state, "{name}失败不得推进任何状态");
            assert!(sentinels_intact(&workspace), "{name}失败不得准备工作区");
        }
    }
    // ---- 语法到参数 ----

    /// 一份 `aspx_config()`，四个开关按 `pattern` 逐位置真。
    ///
    /// 位 0 是 `master_freq_scale`、位 1 是 `interpolation`、位 2 是 `preflat`、
    /// 位 3 是 `limiter`。**逐个单独置真**才能把四条接线两两区分开：一次全真或
    /// 全假时，任意两条互换都看不出来。
    fn config_with(pattern: u8) -> AspxConfig {
        AspxConfig {
            quant_mode_env: false,
            start_freq: 0,
            stop_freq: 0,
            master_freq_scale: pattern & 1 != 0,
            interpolation: pattern & 2 != 0,
            preflat: pattern & 4 != 0,
            limiter: pattern & 8 != 0,
            noise_sbg: 0,
            num_env_bits_fixfix: false,
            freq_res_mode: 0,
        }
    }

    #[test]
    fn each_config_flag_lands_on_its_own_parameter() {
        // 期望值是位模式本身，不从 `config` 的字段读回来——那样只能证明抄对了。
        let bands = hf_bands();
        let (params, interval) = split_interval(SLOTS as u8);
        let framing = framing(params, interval, false, false);
        let hfgen = AspxHfGen::for_test(&[2], &[true, false]);
        let data = AspxData::for_test(
            1,
            bands,
            [framing, framing],
            [hfgen, hfgen],
            [envelope_data(&bands, 0), envelope_data(&bands, 0)],
        );

        for pattern in 0..16u8 {
            let config = config_with(pattern);
            let inputs =
                ChannelInputs::from_parsed(&data, &config, 0, false, false, false).expect("取参数");
            assert_eq!(
                inputs.hf_gen.master_freq_scale,
                pattern & 1 != 0,
                "master_freq_scale，pattern={pattern}"
            );
            assert_eq!(
                inputs.hf_adjust.interpolation,
                pattern & 2 != 0,
                "interpolation，pattern={pattern}"
            );
            assert_eq!(
                inputs.hf_gen.pre_flattening,
                pattern & 4 != 0,
                "preflat，pattern={pattern}"
            );
            assert_eq!(
                inputs.hf_adjust.limiter,
                pattern & 8 != 0,
                "limiter，pattern={pattern}"
            );
        }
    }

    #[test]
    fn the_three_caller_side_flags_pass_straight_through() {
        // 采样率、`master_reset` 与 `first_frame` 都不在 `aspx_data_*()` 里，
        // 只能由调用方带进来；三者互换都必须看得见，故逐个单独置真。
        let bands = hf_bands();
        let (params, interval) = split_interval(SLOTS as u8);
        let framing = framing(params, interval, false, false);
        let hfgen = AspxHfGen::for_test(&[2], &[true, false]);
        let data = AspxData::for_test(
            1,
            bands,
            [framing, framing],
            [hfgen, hfgen],
            [envelope_data(&bands, 0), envelope_data(&bands, 0)],
        );
        let config = config_with(0);

        for pattern in 0..8u8 {
            let inputs = ChannelInputs::from_parsed(
                &data,
                &config,
                0,
                pattern & 1 != 0,
                pattern & 2 != 0,
                pattern & 4 != 0,
            )
            .expect("取参数");
            assert_eq!(inputs.hf_gen.base_samp_freq_48, pattern & 1 != 0);
            assert_eq!(inputs.hf_adjust.master_reset, pattern & 2 != 0);
            assert_eq!(inputs.hf_adjust.first_frame, pattern & 4 != 0);
        }
    }

    #[test]
    fn the_two_per_channel_vectors_come_from_that_channel() {
        // `aspx_tna_mode` 与 `aspx_add_harmonic` 逐声道传输。取错声道时长度可能
        // 一样，故两路给不同的**取值**，并逐项比对。
        let bands = hf_bands();
        let (params, interval) = split_interval(SLOTS as u8);
        let framing = framing(params, interval, false, false);
        let first = AspxHfGen::for_test(&[1], &[true, false, true]);
        let second = AspxHfGen::for_test(&[3], &[false, true, false]);
        assert_ne!(first, second, "夹具前提：两路的 HF 生成参数必须不同");
        let data = AspxData::for_test(
            2,
            bands,
            [framing, framing],
            [first, second],
            [envelope_data(&bands, 0), envelope_data(&bands, 1)],
        );
        let config = config_with(0);

        let a =
            ChannelInputs::from_parsed(&data, &config, 0, false, false, false).expect("第 0 路");
        let b =
            ChannelInputs::from_parsed(&data, &config, 1, false, false, false).expect("第 1 路");
        assert_eq!(a.hf_gen.chirp_modes, &[1]);
        assert_eq!(b.hf_gen.chirp_modes, &[3]);
        assert_eq!(a.hf_adjust.add_harmonic, &[true, false, true]);
        assert_eq!(b.hf_adjust.add_harmonic, &[false, true, false]);
        assert_ne!(
            a.envelopes.data.sig(0, 0),
            b.envelopes.data.sig(0, 0),
            "包络数据也应逐声道取"
        );

        // 单声道元素只有第 0 路。
        let mono = AspxData::for_test(
            1,
            bands,
            [framing, framing],
            [first, second],
            [envelope_data(&bands, 0), envelope_data(&bands, 1)],
        );
        assert_eq!(
            ChannelInputs::from_parsed(&mono, &config, 1, false, false, false).err(),
            Some(PipelineError::MissingChannel { channel: 1 })
        );
    }

    #[test]
    fn master_reset_compares_against_the_previous_i_frame() {
        // `5.7.6.3.1.1`：三个字段相对上一个 I 帧变化时为真。非 I 帧不重传配置，
        // 按定义恒不变。首帧没有上一个 I 帧，正文未定义，此处取真——见
        // `MasterResetTracker` 的文档，那一档两种读法不可区分。
        let mut tracker = MasterResetTracker::new();
        let base = config_with(0);
        let first = tracker.frame(Some(&base));
        assert!(first.is_reset(), "首个 I 帧取真");
        tracker.commit(first);
        let same = tracker.frame(Some(&base));
        assert!(!same.is_reset(), "配置未变则为假");
        tracker.commit(same);
        let inter = tracker.frame(None);
        assert!(!inter.is_reset(), "非 I 帧恒为假");
        tracker.commit(inter);
        assert!(
            !tracker.frame(Some(&base)).is_reset(),
            "非 I 帧不该扰动历史"
        );

        // 三个字段各自单独变化都要能触发。
        let mut scale = base;
        scale.master_freq_scale = true;
        let scale_frame = tracker.frame(Some(&scale));
        assert!(scale_frame.is_reset(), "master_freq_scale 变化");
        tracker.commit(scale_frame);
        let mut start = scale;
        start.start_freq = 3;
        let start_frame = tracker.frame(Some(&start));
        assert!(start_frame.is_reset(), "start_freq 变化");
        tracker.commit(start_frame);
        let mut stop = start;
        stop.stop_freq = 2;
        let stop_frame = tracker.frame(Some(&stop));
        assert!(stop_frame.is_reset(), "stop_freq 变化");
        tracker.commit(stop_frame);

        // 不在判据里的字段变化不得触发。
        let mut other = stop;
        other.limiter = !other.limiter;
        other.interpolation = !other.interpolation;
        other.preflat = !other.preflat;
        other.noise_sbg = 1;
        other.freq_res_mode = 2;
        assert!(
            !tracker.frame(Some(&other)).is_reset(),
            "判据只看那三个字段"
        );
    }

    #[test]
    fn master_reset_decision_survives_retry_and_applies_to_every_channel() {
        // 判定不能在查询时消费配置变化：前置错误后的同帧重试仍应取真；同一份判定
        // 交给两路声道时也必须都是同一个值。只有整帧成功后的显式提交才推进历史。
        let mut tracker = MasterResetTracker::new();
        let base = config_with(0);
        let first = tracker.frame(Some(&base));
        tracker.commit(first);

        let mut changed = base;
        changed.start_freq = 1;
        let attempt = tracker.frame(Some(&changed));
        assert!(attempt.is_reset(), "配置变化应触发重置");
        // `frame` 取 `&self`，因此同一帧无论问多少次——重试一次，或双声道元素
        // 逐路问一次——都得到同一份判定。把同一个值取两次再比，比的是它自己，
        // 那种写法恒真。
        assert_eq!(
            tracker.frame(Some(&changed)),
            attempt,
            "未提交时重复查询必须给出同一份判定"
        );

        tracker.commit(attempt);
        assert!(
            !tracker.frame(Some(&changed)).is_reset(),
            "成功提交后同一配置才应变为未变化"
        );
    }
}
