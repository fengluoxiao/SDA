//! 统一 Full A-JOC 语法、ASF 核心带、QMF 域出口与 full 重建的无 sink 状态。
//!
//! 表 188 的控制 FIFO 拥有同一 raw frame 的 element/A-SPX/config、A-JOC 控制与
//! 矩阵、raw OAMD、group common/timing、AU provenance、full 支持凭证和 LFE
//! 插回位置。OAMD 对象继承也只按控制到期顺序推进，并保留帧前、逐块与帧末
//! 状态；有效 timing 同时按显式值、group、derive 标志和到期历史合并。控制
//! 到期后，A-SPX 的 `Q_out,ASPX` 先按 `Pseudocode 14a` 形成 `Qin_AJOC`，再经对象矩阵
//! 重建、`Pseudocode 15` LFE 插回和终端 QMF 合成。诊断出口仍把 `Qin_AJOC`
//! 直接合成，并与 full 对象出口共用同一条 AC-4 PCM/QMF 增益边界；两者的
//! 声道语义保持独立。
//!
//! # 终端合成属于这一层
//!
//! `element_drive` 停在 `Q_out,ASPX`，那是规范要求的 A-SPX 出口（`5.7.6.5.3`
//! 把它交给下游 QMF 域工具）。这里持有两组互不借用的 [`QmfSynthesisState`]：
//! 一组只冻结既有 A-SPX 诊断出口，另一组属于重建对象加可选 LFE 的最终出口。
//! `AspxChannelState::synthesis` 是 A-SPX 自己的链路状态，也不能挪作任一出口。
//!
//! # 逐声道状态的生命周期与 overlap 同规
//!
//! 声道级失败必须让该声道的历史一起失效，否则下一帧会跨过缺口继续；这与
//! `5.19c` 给 IMDCT overlap 定的规矩相同。substream 级重置则整条丢弃。

use super::{
    AspxBlocker, FullAjocBlocker, SupportedAjocFullFrame, SupportedAspxFrame,
    asf::{
        DecodedFullAjocAsfFrame, FullAjocAsfDecoder, FullAjocAsfError, FullAjocAsfFrameInput,
        FullAjocAsfFrameObservation,
    },
    syntax::{
        DecodedFullAjocSyntaxFrame, FullAjocSyntaxDecoder, FullAjocSyntaxError,
        FullAjocSyntaxFrameInput, FullAjocSyntaxObservation,
    },
};
use crate::{
    ajoc::{
        Ajoc, AjocObjectControl, AjocObjectMatrix,
        reconstruction::{
            AjocReconstructionState, AjocWorkspace as AjocReconstructionWorkspace,
            MAX_RECONSTRUCTED_OBJECTS, reconstruct_frame,
        },
    },
    aspx::{
        pipeline::{AspxIntermediates, MasterResetTracker},
        qmf::{QmfSynthesisState, synthesise_ac4_pcm, synthesise_ac4_pcm_channels},
        syntax::{AspxConfig, AspxData},
        workspace::AspxWorkspace,
    },
    element_drive::{
        DriveError, DriveWorkspace, ElementChannelState, ElementParams, QmfChannelFrame,
        drive_element, empty_channel_frame, prime_control_delay_element,
    },
    frame_alignment::{FrameAlignment, FrameAlignmentState, MAX_CONTROL_ALIGNMENT_DELAY_FRAMES},
    substream_audio::Ac4SubstreamAjoc,
    var_element::{MAX_ASPX_ELEMENTS, MAX_SIGNALS, VarChannelElement},
};
use alloc::{boxed::Box, collections::VecDeque, format, string::String, vec::Vec};
use macindecode_ac4_bitstream::{
    oamd::{
        AdditionalObjectMetadata, MAX_OAMD_METADATA_BLOCKS, OamdCommonData, OamdMetadataBlock,
        OamdState, OamdStateError, OamdTimingData, ObjectMetadataState,
    },
    topology::{MAX_SUBSTREAM_GROUPS, MAX_SUBSTREAMS},
};

/// 最大四帧在 FIFO，另留一槽让刚到期的 side information 借用到下一次调用。
const CONTROL_SNAPSHOT_SLOTS: usize = MAX_CONTROL_ALIGNMENT_DELAY_FRAMES + 1;

/// Full A-JOC 语法与 ASF 前端的组合失败。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullAjocFrontendError {
    /// `ac4_substream()` 或 `audio_data_ajoc()` 解析失败。
    Syntax(FullAjocSyntaxError),
    /// ASF 反量化、矩阵、解组或 IMDCT 失败。
    Asf(FullAjocAsfError),
}

impl core::fmt::Display for FullAjocFrontendError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Syntax(error) => write!(formatter, "Audio syntax: {error}"),
            Self::Asf(error) => write!(formatter, "ASF front end: {error}"),
        }
    }
}

impl core::error::Error for FullAjocFrontendError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Syntax(error) => Some(error),
            Self::Asf(error) => Some(error),
        }
    }
}

impl FullAjocFrontendError {
    /// 语法阶段是否只因传入的有界载荷耗尽而失败。
    #[must_use]
    pub fn is_input_exhausted(&self) -> bool {
        matches!(self, Self::Syntax(error) if error.is_input_exhausted())
    }
}

/// Full A-JOC 单次音频帧事务的结构化失败。
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FullAjocAudioFrameError {
    /// `ac4_substream()` 或 `audio_data_ajoc()` 解析失败。
    Syntax(FullAjocSyntaxError),
    /// ASF 反量化、矩阵、解组或 IMDCT 失败。
    Asf(FullAjocAsfError),
    /// 表 188 对齐、A-SPX/QMF 或 Full A-JOC 重建失败。
    Decode(FullAjocDecodeError),
}

impl core::fmt::Display for FullAjocAudioFrameError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Syntax(error) => write!(formatter, "Audio syntax: {error}"),
            Self::Asf(error) => write!(formatter, "ASF front end: {error}"),
            Self::Decode(error) => write!(formatter, "Alignment/OAMD/QMF/Full: {error}"),
        }
    }
}

impl core::error::Error for FullAjocAudioFrameError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Syntax(error) => Some(error),
            Self::Asf(error) => Some(error),
            Self::Decode(error) => Some(error),
        }
    }
}

impl FullAjocAudioFrameError {
    /// 语法阶段是否只因传入的有界载荷耗尽而失败。
    #[must_use]
    pub fn is_input_exhausted(&self) -> bool {
        matches!(self, Self::Syntax(error) if error.is_input_exhausted())
    }
}

/// 一次借用的 Full A-JOC 语法、OAMD 与核心带 ASF 帧。
#[derive(Debug)]
pub struct DecodedFullAjocFrontendFrame<'a> {
    syntax: DecodedFullAjocSyntaxFrame<'a>,
    asf: DecodedFullAjocAsfFrame<'a>,
}

impl<'a> DecodedFullAjocFrontendFrame<'a> {
    /// 当前帧的语法、控制和 OAMD 借用快照。
    #[must_use]
    pub const fn syntax(&self) -> &DecodedFullAjocSyntaxFrame<'a> {
        &self.syntax
    }

    /// 当前帧借用的核心带 planar PCM 与逐路观察。
    #[must_use]
    pub const fn asf(&self) -> &DecodedFullAjocAsfFrame<'a> {
        &self.asf
    }
}

/// 一份已解析 QMF 控制帧的所有权快照。
///
/// 三个解析工作区都会在下一 raw frame 被覆盖，不能把借用直接放进表 188 的控制
/// FIFO。配置是该帧解析结束时的有效值，非 I 帧也已经包含沿用结果；full 凭证与
/// LFE 位置、raw OAMD 与 AU provenance 一同快照，不能在到期时拿当前 TOC 重新
/// 推导。变长载荷来自预分配的五槽缓冲池：最多四槽在 FIFO，一槽由借用输出保留
/// 到下一次调用；稳定解码只清空长度并复制进既有容量，不逐帧重新分配。
#[derive(Debug)]
struct QueuedQmfControl {
    element: VarChannelElement,
    aspx: Vec<AspxData>,
    config: Option<AspxConfig>,
    frame_length: u16,
    sampling_frequency_hz: u32,
    physical_substreams: usize,
    dialogue_objects: u8,
    aspx_support: SupportedAspxFrame,
    ajoc: Ajoc,
    object_controls: Vec<AjocObjectControl>,
    matrices: Vec<AjocObjectMatrix>,
    dmx_oamd_blocks: Vec<OamdMetadataBlock>,
    umx_oamd_blocks: Vec<OamdMetadataBlock>,
    dmx_oamd_timing: Option<OamdTimingData>,
    umx_oamd_timing: Option<OamdTimingData>,
    derive_timing_from_dmx: Option<bool>,
    dmx_num_obj_info_blocks: u8,
    umx_num_obj_info_blocks: u8,
    provenance: Option<FullAjocFrameProvenance>,
    full_support: Result<SupportedAjocFullFrame, FullAjocBlocker>,
    lfe_position: Option<u32>,
}

/// 一份控制槽中需要预分配并循环复用的五个变长载荷。
#[derive(Debug)]
struct QueuedQmfControlBuffers {
    aspx: Vec<AspxData>,
    object_controls: Vec<AjocObjectControl>,
    matrices: Vec<AjocObjectMatrix>,
    dmx_oamd_blocks: Vec<OamdMetadataBlock>,
    umx_oamd_blocks: Vec<OamdMetadataBlock>,
}

impl QueuedQmfControlBuffers {
    fn new() -> Self {
        Self {
            aspx: Vec::with_capacity(MAX_ASPX_ELEMENTS),
            object_controls: Vec::with_capacity(MAX_RECONSTRUCTED_OBJECTS),
            matrices: Vec::with_capacity(MAX_RECONSTRUCTED_OBJECTS),
            dmx_oamd_blocks: Vec::with_capacity(MAX_OAMD_METADATA_BLOCKS),
            umx_oamd_blocks: Vec::with_capacity(MAX_OAMD_METADATA_BLOCKS),
        }
    }
}

/// 当前解析工作区中即将提交到表 188 FIFO 的借用视图。
struct QmfControlSnapshot<'a> {
    element: &'a VarChannelElement,
    aspx: &'a [AspxData],
    config: Option<&'a AspxConfig>,
    frame_length: u16,
    sampling_frequency_hz: u32,
    physical_substreams: usize,
    dialogue_objects: u8,
    aspx_support: SupportedAspxFrame,
    ajoc: &'a Ajoc,
    object_controls: &'a [AjocObjectControl],
    matrices: &'a [AjocObjectMatrix],
    dmx_oamd_blocks: &'a [OamdMetadataBlock],
    umx_oamd_blocks: &'a [OamdMetadataBlock],
    dmx_oamd_timing: Option<OamdTimingData>,
    umx_oamd_timing: Option<OamdTimingData>,
    derive_timing_from_dmx: Option<bool>,
    dmx_num_obj_info_blocks: u8,
    umx_num_obj_info_blocks: u8,
    provenance: Option<FullAjocFrameProvenance>,
    full_support: Result<SupportedAjocFullFrame, FullAjocBlocker>,
    lfe_position: Option<u32>,
}

impl QueuedQmfControl {
    fn capture(mut buffers: QueuedQmfControlBuffers, source: QmfControlSnapshot<'_>) -> Self {
        buffers.aspx.clear();
        buffers.aspx.extend_from_slice(source.aspx);
        buffers.object_controls.clear();
        buffers
            .object_controls
            .extend_from_slice(source.object_controls);
        buffers.matrices.clear();
        buffers.matrices.extend_from_slice(source.matrices);
        buffers.dmx_oamd_blocks.clear();
        buffers
            .dmx_oamd_blocks
            .extend_from_slice(source.dmx_oamd_blocks);
        buffers.umx_oamd_blocks.clear();
        buffers
            .umx_oamd_blocks
            .extend_from_slice(source.umx_oamd_blocks);

        Self {
            element: *source.element,
            aspx: buffers.aspx,
            config: source.config.copied(),
            frame_length: source.frame_length,
            sampling_frequency_hz: source.sampling_frequency_hz,
            physical_substreams: source.physical_substreams,
            dialogue_objects: source.dialogue_objects,
            aspx_support: source.aspx_support,
            ajoc: *source.ajoc,
            object_controls: buffers.object_controls,
            matrices: buffers.matrices,
            dmx_oamd_blocks: buffers.dmx_oamd_blocks,
            umx_oamd_blocks: buffers.umx_oamd_blocks,
            dmx_oamd_timing: source.dmx_oamd_timing,
            umx_oamd_timing: source.umx_oamd_timing,
            derive_timing_from_dmx: source.derive_timing_from_dmx,
            dmx_num_obj_info_blocks: source.dmx_num_obj_info_blocks,
            umx_num_obj_info_blocks: source.umx_num_obj_info_blocks,
            provenance: source.provenance,
            full_support: source.full_support,
            lfe_position: source.lfe_position,
        }
    }

    fn into_buffers(self) -> QueuedQmfControlBuffers {
        let Self {
            mut aspx,
            mut object_controls,
            mut matrices,
            mut dmx_oamd_blocks,
            mut umx_oamd_blocks,
            ..
        } = self;
        aspx.clear();
        object_controls.clear();
        matrices.clear();
        dmx_oamd_blocks.clear();
        umx_oamd_blocks.clear();
        QueuedQmfControlBuffers {
            aspx,
            object_controls,
            matrices,
            dmx_oamd_blocks,
            umx_oamd_blocks,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FullAjocInputSideInformation<'a> {
    provenance: Option<FullAjocFrameProvenance>,
    dmx_oamd_blocks: &'a [OamdMetadataBlock],
    umx_oamd_blocks: &'a [OamdMetadataBlock],
    dmx_oamd_timing: Option<OamdTimingData>,
    umx_oamd_timing: Option<OamdTimingData>,
    derive_timing_from_dmx: Option<bool>,
    dmx_num_obj_info_blocks: u8,
    umx_num_obj_info_blocks: u8,
}

impl FullAjocInputSideInformation<'_> {
    const fn empty() -> Self {
        Self {
            provenance: None,
            dmx_oamd_blocks: &[],
            umx_oamd_blocks: &[],
            dmx_oamd_timing: None,
            umx_oamd_timing: None,
            derive_timing_from_dmx: None,
            dmx_num_obj_info_blocks: 0,
            umx_num_obj_info_blocks: 0,
        }
    }
}

/// 会改变 frame-alignment 声道所有权的输入拓扑。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QmfInputTopology {
    fullband: usize,
    lfe: bool,
}

impl QmfInputTopology {
    fn from_element(element: &VarChannelElement) -> Self {
        Self {
            fullband: usize::from(element.n_dmx_signals),
            lfe: element.b_has_lfe,
        }
    }

    fn channels(self) -> usize {
        self.fullband.saturating_add(usize::from(self.lfe))
    }
}

/// `Pseudocode 15` 之后的固定输出拓扑。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObjectOutputTopology {
    objects: usize,
    lfe_position: Option<usize>,
}

impl ObjectOutputTopology {
    fn checked(
        objects: u32,
        lfe_position: Option<u32>,
        element_has_lfe: bool,
        substream: u32,
    ) -> Result<Self, String> {
        let objects = usize::try_from(objects).map_err(|_| {
            format!("Substream {substream}: A-JOC object count {objects} cannot be represented as a native index")
        })?;
        if objects > MAX_RECONSTRUCTED_OBJECTS {
            return Err(format!(
                "Substream {substream}: A-JOC object count {objects} exceeds full limit {MAX_RECONSTRUCTED_OBJECTS}"
            ));
        }
        if element_has_lfe != lfe_position.is_some() {
            return Err(format!(
                "Substream {substream}: element LFE={}, but Pseudocode 15 reinsertion position is {lfe_position:?}",
                element_has_lfe
            ));
        }
        let lfe_position = lfe_position
            .map(|position| {
                usize::try_from(position).map_err(|_| {
                    format!("Substream {substream}: LFE reinsertion position {position} cannot be represented as a native index")
                })
            })
            .transpose()?;
        if lfe_position.is_some_and(|position| position > objects) {
            return Err(format!(
                "Substream {substream}: LFE reinsertion position {} exceeds {objects} A-JOC objects",
                lfe_position.unwrap_or(usize::MAX)
            ));
        }
        Ok(Self {
            objects,
            lfe_position,
        })
    }

    fn channels(self) -> usize {
        self.objects
            .saturating_add(usize::from(self.lfe_position.is_some()))
    }

    fn source(self, output: usize) -> Option<FullAjocPcmSource> {
        if output >= self.channels() {
            return None;
        }
        match self.lfe_position {
            Some(position) if output == position => Some(FullAjocPcmSource::Lfe),
            Some(position) if output > position => {
                Some(FullAjocPcmSource::AjocObject(output.saturating_sub(1)))
            }
            _ => Some(FullAjocPcmSource::AjocObject(output)),
        }
    }
}

/// 一路帧级 PCM 的来源语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullAjocPcmSource {
    /// A-SPX 出口按 `Pseudocode 14a` 排列的一路 A-JOC 输入。
    AjocInput,
    /// Full A-JOC 重建出的空间对象组分量。
    AjocObject(usize),
    /// 按 `Pseudocode 15` 插回的原生 LFE 分量。
    Lfe,
}

/// full 对象驱动失败的稳定分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FullAjocDecodeErrorKind {
    /// 未归入更具体类别的内部驱动错误。
    Other,
    /// 码流属于当前未覆盖的合法分支。
    Unsupported,
    /// 同一物理 substream 未经重置改变了解码模式。
    DecodeModeMismatch,
    /// A-JOC 矩阵、差分、rolling 或 decorrelator 重建失败。
    Reconstruction,
    /// 与 PCM 对齐的 OAMD 更新无法从前序状态完整解析。
    OamdState,
    /// 对象终端 PCM 含非有限值。
    ObjectsNonFinite,
    /// 输入或输出拓扑、容量或来源形状不一致。
    ObjectShapeMismatch,
}

/// Full engine 明确识别出的合法但未覆盖分支。
///
/// 该值与 [`FullAjocDecodeErrorKind::Unsupported`] 一起返回，使 Session 与 CLI
/// 可以使用稳定类型判断原因，同时继续保留既有的可读 `detail` 文本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FullAjocUnsupported {
    /// A-SPX/QMF 前置路径的支持门禁。
    Aspx(AspxBlocker),
    /// Full A-JOC 重建路径的支持门禁。
    Full(FullAjocBlocker),
}

impl core::fmt::Display for FullAjocUnsupported {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Aspx(reason) => write!(formatter, "{reason}"),
            Self::Full(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl core::error::Error for FullAjocUnsupported {}

/// 对齐、OAMD、QMF/full 驱动错误；分类与文本同源返回，避免 CLI 靠消息字符串猜诊断码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullAjocDecodeError {
    kind: FullAjocDecodeErrorKind,
    unsupported: Option<FullAjocUnsupported>,
    detail: String,
}

impl FullAjocDecodeError {
    /// 建立合法但当前不受支持的分支错误。
    pub fn unsupported(detail: impl Into<String>) -> Self {
        Self {
            kind: FullAjocDecodeErrorKind::Unsupported,
            unsupported: None,
            detail: detail.into(),
        }
    }

    fn unsupported_aspx(reason: AspxBlocker, substream: u32) -> Self {
        Self {
            kind: FullAjocDecodeErrorKind::Unsupported,
            unsupported: Some(FullAjocUnsupported::Aspx(reason)),
            detail: format!("Substream {substream}: {}", reason.detail()),
        }
    }

    fn unsupported_full(reason: FullAjocBlocker, substream: u32) -> Self {
        Self {
            kind: FullAjocDecodeErrorKind::Unsupported,
            unsupported: Some(FullAjocUnsupported::Full(reason)),
            detail: format!("Substream {substream}: {}", reason.detail()),
        }
    }

    /// 建立未经重置改变解码模式的错误。
    fn decode_mode_mismatch(detail: impl Into<String>) -> Self {
        Self {
            kind: FullAjocDecodeErrorKind::DecodeModeMismatch,
            unsupported: None,
            detail: detail.into(),
        }
    }

    /// 建立 A-JOC 重建错误。
    pub fn reconstruction(detail: impl Into<String>) -> Self {
        Self {
            kind: FullAjocDecodeErrorKind::Reconstruction,
            unsupported: None,
            detail: detail.into(),
        }
    }

    /// 建立 OAMD 状态延续错误。
    fn oamd_state(detail: impl Into<String>) -> Self {
        Self {
            kind: FullAjocDecodeErrorKind::OamdState,
            unsupported: None,
            detail: detail.into(),
        }
    }

    /// 建立对象 PCM 非有限值错误。
    pub fn objects_nonfinite(detail: impl Into<String>) -> Self {
        Self {
            kind: FullAjocDecodeErrorKind::ObjectsNonFinite,
            unsupported: None,
            detail: detail.into(),
        }
    }

    /// 建立对象或输入拓扑形状错误。
    pub fn object_shape(detail: impl Into<String>) -> Self {
        Self {
            kind: FullAjocDecodeErrorKind::ObjectShapeMismatch,
            unsupported: None,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> FullAjocDecodeErrorKind {
        self.kind
    }

    /// 当前错误携带的类型化不支持原因；非支持门禁失败返回 `None`。
    #[must_use]
    pub const fn unsupported_reason(&self) -> Option<FullAjocUnsupported> {
        self.unsupported
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl From<String> for FullAjocDecodeError {
    fn from(detail: String) -> Self {
        Self {
            kind: FullAjocDecodeErrorKind::Other,
            unsupported: None,
            detail,
        }
    }
}

impl core::fmt::Display for FullAjocDecodeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl core::error::Error for FullAjocDecodeError {}

/// 一次 QMF 驱动对 full 路径的可观测结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FullAjocObservation {
    warmup: bool,
    reconstructed: bool,
    wet: bool,
}

impl FullAjocObservation {
    /// 建立一份路径覆盖观察；主要供无状态 adapter 测试与统计合并使用。
    #[must_use]
    pub const fn new(warmup: bool, reconstructed: bool, wet: bool) -> Self {
        Self {
            warmup,
            reconstructed,
            wet,
        }
    }

    /// 当前输出是否仍处于表 188 控制预热阶段。
    #[must_use]
    pub const fn warmup(self) -> bool {
        self.warmup
    }

    /// 当前输出是否应用了一份到期的 Full A-JOC 控制快照。
    #[must_use]
    pub const fn reconstructed(self) -> bool {
        self.reconstructed
    }

    /// 到期 A-JOC 控制是否实际启用了 wet/decorrelator 路径。
    #[must_use]
    pub const fn wet(self) -> bool {
        self.wet
    }
}

/// 一条物理 substream 是否执行 full 对象路径，以及是否把它当作必需产物。
///
/// A-SPX 诊断只需要输入出口；census 可以 opportunistic 执行 Full；对象出口则
/// 要求每帧都落在当前 Full 子集内。同一 substream 的首次成功调用会绑定模式，
/// 后续必须先 [`FullAjocDecoder::reset_substream`] 才能切换，避免跨过未推进的
/// A-JOC 差分、decorrelator 或终端 QMF 历史。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullAjocDecodeMode {
    /// 仅产生 A-SPX/A-JOC 输入诊断 PCM。
    AspxOnly,
    /// 产生可作为 A-JOC Core 对象使用的 A-SPX PCM，并拒绝未应用的对话增强或 alternative OAMD。
    RequireCore,
    /// 支持时执行 Full；Full blocker 不抹掉诊断 PCM，下游失败保留前端 census 历史。
    ObserveFull,
    /// Full blocker 立即报错，且不提交本帧状态。
    RequireFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputExhaustionPolicy {
    PreserveForRetry,
    InvalidateCompleteInput,
}

/// 在当前帧进入表 188 控制 FIFO 前核对所请求出口的凭证。
///
/// `RequireCore` 只提升会改变 core 对象语义的活动 DE 或未应用的 alternative OAMD；
/// `RequireFull` 提升全部 full blocker。到期控制仍会重复核对，防御 FIFO 所有权或
/// 对齐错误；但必需出口不能只等控制到期才拒绝，否则流末尾不足一个控制延迟的
/// blocker 会永远留在 FIFO 中。
#[expect(
    clippy::too_many_arguments,
    reason = "凭证必须与同一 raw frame 的 A-SPX、A-JOC、时间轴及物理拓扑逐项核对"
)]
fn validate_current_full_support(
    full_support: &Result<SupportedAjocFullFrame, FullAjocBlocker>,
    supported: SupportedAspxFrame,
    element: &VarChannelElement,
    frame_length: u16,
    sampling_frequency_hz: u32,
    physical_substreams: usize,
    ajoc: &Ajoc,
    dialogue_objects: u8,
    full_requirement: FullAjocDecodeMode,
    substream: u32,
) -> Result<(), FullAjocDecodeError> {
    reject_required_output_blocker(full_support, full_requirement, dialogue_objects, substream)?;
    match full_support {
        Ok(credential)
            if credential.aspx() != supported
                || !credential.matches(
                    element,
                    frame_length,
                    sampling_frequency_hz,
                    physical_substreams,
                    ajoc,
                    dialogue_objects,
                ) =>
        {
            Err(format!("Substream {substream}: A-JOC full token is misaligned with the current Table 188/A-SPX token").into())
        }
        Ok(_) | Err(_) => Ok(()),
    }
}

fn reject_required_output_blocker(
    full_support: &Result<SupportedAjocFullFrame, FullAjocBlocker>,
    full_requirement: FullAjocDecodeMode,
    dialogue_objects: u8,
    substream: u32,
) -> Result<(), FullAjocDecodeError> {
    let blocker = match full_requirement {
        FullAjocDecodeMode::RequireCore
            if matches!(
                full_support,
                Err(FullAjocBlocker::AlternativeObjectMetadata)
            ) =>
        {
            Some(FullAjocBlocker::AlternativeObjectMetadata)
        }
        FullAjocDecodeMode::RequireCore if dialogue_objects != 0 => {
            Some(FullAjocBlocker::ActiveDialogueEnhancement { dialogue_objects })
        }
        FullAjocDecodeMode::RequireFull => full_support.as_ref().err().copied(),
        _ => None,
    };
    if let Some(blocker) = blocker {
        return Err(FullAjocDecodeError::unsupported_full(blocker, substream));
    }
    Ok(())
}

/// 一个 substream 的 A-SPX 驱动状态。
#[derive(Debug)]
struct FullAjocSubstreamState {
    /// 逐声道跨帧状态，按传输顺序。
    states: Vec<ElementChannelState>,
    /// 逐声道诊断中间量，长度同 `states`。
    intermediates: Vec<AspxIntermediates>,
    /// 逐声道终端合成状态。
    synthesis: Vec<QmfSynthesisState>,
    /// A-JOC 差分、rolling 与 decorrelator 跨帧状态。
    ajoc: AjocReconstructionState,
    /// `Pseudocode 15` 后逐输出的终端合成状态。
    object_synthesis: Vec<QmfSynthesisState>,
    /// 表 188 的逐声道 PCM frame-alignment 状态。
    alignment: Vec<FrameAlignmentState>,
    /// 尚未走到对应 spectral frontend 信号的 QMF 控制帧。
    controls: VecDeque<QueuedQmfControl>,
    /// 刚与输出 PCM 一起到期、需要借用到下一次可变调用的完整 side information。
    aligned_control: Option<QueuedQmfControl>,
    /// Core/downmix 侧已经按表 188 到期顺序提交的 OAMD 对象状态。
    dmx_oamd: OamdState,
    /// Full/upmix 侧已经按表 188 到期顺序提交的 OAMD 对象状态。
    umx_oamd: OamdState,
    /// Core/downmix 侧已经按表 188 到期顺序提交的有效 timing。
    dmx_oamd_timing: Option<OamdTimingData>,
    /// Full/upmix 侧已经按表 188 到期顺序提交的有效 timing。
    umx_oamd_timing: Option<OamdTimingData>,
    /// 当前借用输出在应用本帧 Core OAMD 前的完整对象状态。
    aligned_dmx_oamd_start: OamdState,
    /// 当前借用输出在应用本帧 Full OAMD 前的完整对象状态。
    aligned_umx_oamd_start: OamdState,
    /// 当前借用输出对应的 Core/downmix 有效 timing 及刷新来源。
    aligned_dmx_oamd_timing: FullAjocOamdTimingState,
    /// 当前借用输出对应的 Full/upmix 有效 timing 及刷新来源。
    aligned_umx_oamd_timing: FullAjocOamdTimingState,
    /// Core OAMD 每个 raw block 应用后的有序状态快照。
    aligned_dmx_oamd_updates: Vec<FullAjocOamdUpdateSnapshot>,
    /// Full OAMD 每个 raw block 应用后的有序状态快照。
    aligned_umx_oamd_updates: Vec<FullAjocOamdUpdateSnapshot>,
    /// 尚未进入 FIFO 的预分配控制载荷槽；在帧调用边界与 `controls` 合计为表
    /// 188 上限加一个借用输出槽。处理期间到期槽会暂时由当前调用独占。
    control_buffers: Vec<QueuedQmfControlBuffers>,
    /// frame-alignment 建立后不允许无 reset 改变的输入拓扑。
    input_topology: Option<QmfInputTopology>,
    /// 对象终端合成建立后不允许无 reset 改变的输出拓扑。
    output_topology: Option<ObjectOutputTopology>,
    /// 已提交的表 188 延迟档；变化必须经 substream reset。
    alignment_config: Option<FrameAlignment>,
    /// 首次成功帧绑定的解码模式；切换必须经 substream reset。
    decode_mode: Option<FullAjocDecodeMode>,
    /// `aspx_config` 的跨 I 帧比对，逐 substream 一份。
    master_reset: MasterResetTracker,
    /// 是否还没成功驱动过任何一帧，即 `first_frame`。
    fresh: bool,
}

impl FullAjocSubstreamState {
    fn new() -> Self {
        Self {
            states: Vec::new(),
            intermediates: Vec::new(),
            synthesis: Vec::new(),
            ajoc: AjocReconstructionState::new(),
            object_synthesis: Vec::new(),
            alignment: Vec::new(),
            controls: VecDeque::new(),
            aligned_control: None,
            dmx_oamd: OamdState::new(),
            umx_oamd: OamdState::new(),
            dmx_oamd_timing: None,
            umx_oamd_timing: None,
            aligned_dmx_oamd_start: OamdState::new(),
            aligned_umx_oamd_start: OamdState::new(),
            aligned_dmx_oamd_timing: FullAjocOamdTimingState::EMPTY,
            aligned_umx_oamd_timing: FullAjocOamdTimingState::EMPTY,
            aligned_dmx_oamd_updates: Vec::new(),
            aligned_umx_oamd_updates: Vec::new(),
            control_buffers: Vec::new(),
            input_topology: None,
            output_topology: None,
            alignment_config: None,
            decode_mode: None,
            master_reset: MasterResetTracker::new(),
            fresh: true,
        }
    }

    /// 丢弃全部历史，供 seek、配置变化与解析失败后调用。
    fn reset(&mut self) {
        self.states.clear();
        self.intermediates.clear();
        self.synthesis.clear();
        self.ajoc.reset();
        self.object_synthesis.clear();
        self.alignment.clear();
        if let Some(control) = self.aligned_control.take() {
            self.control_buffers.push(control.into_buffers());
        }
        self.invalidate_oamd_history();
        while let Some(control) = self.controls.pop_front() {
            self.control_buffers.push(control.into_buffers());
        }
        if self.control_buffers.capacity() != 0 {
            while self.control_buffers.len() < CONTROL_SNAPSHOT_SLOTS {
                self.control_buffers.push(QueuedQmfControlBuffers::new());
            }
        }
        self.input_topology = None;
        self.output_topology = None;
        self.alignment_config = None;
        self.decode_mode = None;
        self.master_reset = MasterResetTracker::new();
        self.fresh = true;
    }

    #[cfg(test)]
    fn is_fresh(&self) -> bool {
        self.fresh
            && self.states.is_empty()
            && self.synthesis.is_empty()
            && self.ajoc.shape().is_none()
            && self.object_synthesis.is_empty()
            && self.alignment.is_empty()
            && self.controls.is_empty()
            && self.aligned_control.is_none()
            && self.dmx_oamd == OamdState::new()
            && self.umx_oamd == OamdState::new()
            && self.dmx_oamd_timing.is_none()
            && self.umx_oamd_timing.is_none()
            && self.aligned_dmx_oamd_start == OamdState::new()
            && self.aligned_umx_oamd_start == OamdState::new()
            && self.aligned_dmx_oamd_timing == FullAjocOamdTimingState::EMPTY
            && self.aligned_umx_oamd_timing == FullAjocOamdTimingState::EMPTY
            && self.aligned_dmx_oamd_updates.is_empty()
            && self.aligned_umx_oamd_updates.is_empty()
            && (self.control_buffers.is_empty()
                || self.control_buffers.len() == CONTROL_SNAPSHOT_SLOTS)
            && self.input_topology.is_none()
            && self.output_topology.is_none()
            && self.alignment_config.is_none()
            && self.decode_mode.is_none()
    }

    #[cfg(test)]
    fn mark_used_for_test(&mut self) {
        self.fresh = false;
    }

    fn bind_decode_mode(
        &mut self,
        requested: FullAjocDecodeMode,
        substream: u32,
    ) -> Result<(), FullAjocDecodeError> {
        match self.decode_mode {
            Some(bound) if bound != requested => {
                Err(FullAjocDecodeError::decode_mode_mismatch(format!(
                    "Substream {substream}: decode mode changed from {bound:?} to {requested:?}; reset is required"
                )))
            }
            Some(_) => Ok(()),
            None => {
                self.decode_mode = Some(requested);
                Ok(())
            }
        }
    }

    /// 在第一帧推进任何 DSP 状态前建立完整的表 188 所有权环。
    fn prepare_control_buffers(&mut self) {
        if self.controls.capacity() < MAX_CONTROL_ALIGNMENT_DELAY_FRAMES {
            self.controls
                .reserve(MAX_CONTROL_ALIGNMENT_DELAY_FRAMES.saturating_sub(self.controls.len()));
        }
        if self.control_buffers.capacity() < CONTROL_SNAPSHOT_SLOTS {
            self.control_buffers
                .reserve(CONTROL_SNAPSHOT_SLOTS.saturating_sub(self.control_buffers.len()));
        }
        if self.aligned_dmx_oamd_updates.capacity() < MAX_OAMD_METADATA_BLOCKS {
            self.aligned_dmx_oamd_updates.reserve(
                MAX_OAMD_METADATA_BLOCKS.saturating_sub(self.aligned_dmx_oamd_updates.len()),
            );
        }
        if self.aligned_umx_oamd_updates.capacity() < MAX_OAMD_METADATA_BLOCKS {
            self.aligned_umx_oamd_updates.reserve(
                MAX_OAMD_METADATA_BLOCKS.saturating_sub(self.aligned_umx_oamd_updates.len()),
            );
        }
        while self
            .control_buffers
            .len()
            .saturating_add(self.controls.len())
            .saturating_add(usize::from(self.aligned_control.is_some()))
            < CONTROL_SNAPSHOT_SLOTS
        {
            self.control_buffers.push(QueuedQmfControlBuffers::new());
        }
    }

    fn recycle_aligned_control(&mut self) {
        if let Some(control) = self.aligned_control.take() {
            self.control_buffers.push(control.into_buffers());
        }
        self.aligned_dmx_oamd_updates.clear();
        self.aligned_umx_oamd_updates.clear();
        self.aligned_dmx_oamd_timing = FullAjocOamdTimingState::EMPTY;
        self.aligned_umx_oamd_timing = FullAjocOamdTimingState::EMPTY;
    }

    /// parsed-PCM 入口没有当前 raw frame 的 OAMD；它对应的控制一旦到期，后续
    /// metadata 就不能再跨过这段盲区继承此前的对象状态。
    fn invalidate_oamd_history(&mut self) {
        self.dmx_oamd.reset();
        self.umx_oamd.reset();
        self.dmx_oamd_timing = None;
        self.umx_oamd_timing = None;
        self.aligned_dmx_oamd_start.reset();
        self.aligned_umx_oamd_start.reset();
        self.aligned_dmx_oamd_timing = FullAjocOamdTimingState::EMPTY;
        self.aligned_umx_oamd_timing = FullAjocOamdTimingState::EMPTY;
        self.aligned_dmx_oamd_updates.clear();
        self.aligned_umx_oamd_updates.clear();
    }

    fn aligned_side_information(&self) -> Option<FullAjocAlignedSideInformation<'_>> {
        let control = self.aligned_control.as_ref()?;
        let provenance = control.provenance?;
        Some(FullAjocAlignedSideInformation {
            control,
            provenance,
            dmx_oamd: FullAjocOamdFrameSnapshot {
                start: &self.aligned_dmx_oamd_start,
                end: &self.dmx_oamd,
                updates: &self.aligned_dmx_oamd_updates,
            },
            umx_oamd: FullAjocOamdFrameSnapshot {
                start: &self.aligned_umx_oamd_start,
                end: &self.umx_oamd,
                updates: &self.aligned_umx_oamd_updates,
            },
            dmx_oamd_timing: self.aligned_dmx_oamd_timing,
            umx_oamd_timing: self.aligned_umx_oamd_timing,
        })
    }

    fn ensure_input(&mut self, channels: usize) {
        if self.states.len() < channels {
            self.states.resize_with(channels, ElementChannelState::new);
        }
        if self.intermediates.len() < channels {
            self.intermediates
                .resize_with(channels, AspxIntermediates::default);
        }
        if self.synthesis.len() < channels {
            self.synthesis.resize_with(channels, QmfSynthesisState::new);
        }
        if self.alignment.len() < channels {
            self.alignment
                .resize_with(channels, FrameAlignmentState::new);
        }
    }

    fn ensure_output(&mut self, channels: usize) {
        if self.object_synthesis.len() < channels {
            self.object_synthesis
                .resize_with(channels, QmfSynthesisState::new);
        }
    }

    /// full 不受支持但诊断 A-SPX 仍可继续时，只丢弃不能跨越缺口的下游历史。
    fn reset_full(&mut self) {
        self.ajoc.reset();
        self.object_synthesis.clear();
        self.output_topology = None;
    }
}

fn oamd_object_state(state: &OamdState, index: usize) -> Option<FullAjocOamdObjectState> {
    Some(FullAjocOamdObjectState {
        metadata: state.object(index).copied()?,
        additional: state.object_additional(index).copied()?,
    })
}

/// 在一份状态副本上逐块应用，以便同时保留每个 raw block 后的完整状态。
///
/// `OamdState::apply_blocks` 自身保证单块事务；本函数直到整批成功才把返回的
/// state 交给调用方提交。失败时清空已写的借用快照，避免半帧可见。
fn resolve_oamd_updates(
    initial: OamdState,
    blocks: &[OamdMetadataBlock],
    num_obj_info_blocks: u8,
    snapshots: &mut Vec<FullAjocOamdUpdateSnapshot>,
) -> Result<OamdState, OamdStateError> {
    snapshots.clear();
    let mut next = initial;
    for raw in blocks {
        if let Err(error) = next.apply_blocks(core::slice::from_ref(raw), None) {
            snapshots.clear();
            return Err(error);
        }
        let index = usize::from(raw.object_index);
        let Some(state) = oamd_object_state(&next, index) else {
            snapshots.clear();
            return Err(OamdStateError::ObjectIndexOutOfRange {
                object_index: raw.object_index,
                limit: macindecode_ac4_bitstream::oamd::MAX_OAMD_OBJECTS,
            });
        };
        snapshots.push(FullAjocOamdUpdateSnapshot { raw: *raw, state });
    }
    if let Err(error) = next.apply_blocks(&[], Some(num_obj_info_blocks)) {
        snapshots.clear();
        return Err(error);
    }
    Ok(next)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedFullAjocOamdTimings {
    dmx: FullAjocOamdTimingState,
    umx: FullAjocOamdTimingState,
}

fn shared_group_oamd_timing(
    control: &QueuedQmfControl,
    substream: u32,
) -> Result<FullAjocOamdTimingState, FullAjocDecodeError> {
    let Some(provenance) = control.provenance.as_ref() else {
        return Ok(FullAjocOamdTimingState::EMPTY);
    };
    let states = provenance.group_oamd_states();
    let Some(first) = states.first() else {
        return Ok(FullAjocOamdTimingState::EMPTY);
    };
    let effective = first.effective_timing();
    if let Some(conflict) = states
        .iter()
        .find(|state| state.effective_timing() != effective)
    {
        return Err(FullAjocDecodeError::oamd_state(format!(
            "Substream {substream}: effective OAMD timing for presentation group {} differs from group {}",
            first.group_index(),
            conflict.group_index()
        )));
    }
    let updated = effective.is_some()
        && states
            .iter()
            .any(|state| state.timing_updated_in_source_access_unit());
    Ok(FullAjocOamdTimingState::new(effective, updated))
}

const fn select_effective_oamd_timing(
    explicit: Option<OamdTimingData>,
    group: FullAjocOamdTimingState,
    inherited: Option<OamdTimingData>,
) -> FullAjocOamdTimingState {
    match explicit {
        Some(timing) => FullAjocOamdTimingState::new(Some(timing), true),
        None if group.effective().is_some() => group,
        None => FullAjocOamdTimingState::new(inherited, false),
    }
}

fn validate_effective_oamd_timing(
    side: &str,
    timing: FullAjocOamdTimingState,
    num_obj_info_blocks: u8,
    substream: u32,
) -> Result<(), FullAjocDecodeError> {
    match timing.effective() {
        Some(effective) if effective.num_obj_info_blocks != num_obj_info_blocks => {
            Err(FullAjocDecodeError::oamd_state(format!(
                "Substream {substream}: {side} effective OAMD timing declares {} blocks, but dynamic data uses {num_obj_info_blocks}",
                effective.num_obj_info_blocks
            )))
        }
        Some(_) | None => Ok(()),
    }
}

fn resolve_effective_oamd_timings(
    drive: &FullAjocSubstreamState,
    control: &QueuedQmfControl,
    substream: u32,
) -> Result<ResolvedFullAjocOamdTimings, FullAjocDecodeError> {
    // 显式 timing 或显式 derive 已经自足时，presentation 内其它 group 的 timing
    // 不参与该模式；不能因无关 group 的差异提前拒绝当前 A-JOC 控制。
    let dmx_needs_group = control.dmx_oamd_timing.is_none();
    let umx_needs_group =
        control.umx_oamd_timing.is_none() && control.derive_timing_from_dmx != Some(true);
    let group = if dmx_needs_group || umx_needs_group {
        shared_group_oamd_timing(control, substream)?
    } else {
        FullAjocOamdTimingState::EMPTY
    };
    let dmx = select_effective_oamd_timing(control.dmx_oamd_timing, group, drive.dmx_oamd_timing);
    let umx = match control.umx_oamd_timing {
        Some(timing) => FullAjocOamdTimingState::new(Some(timing), true),
        None if control.derive_timing_from_dmx == Some(true) => dmx,
        None => select_effective_oamd_timing(None, group, drive.umx_oamd_timing),
    };
    validate_effective_oamd_timing(
        "Core/downmix",
        dmx,
        control.dmx_num_obj_info_blocks,
        substream,
    )?;
    validate_effective_oamd_timing(
        "Full/upmix",
        umx,
        control.umx_num_obj_info_blocks,
        substream,
    )?;
    Ok(ResolvedFullAjocOamdTimings { dmx, umx })
}

fn resolve_aligned_oamd(
    drive: &mut FullAjocSubstreamState,
    control: &QueuedQmfControl,
    substream: u32,
) -> Result<(), FullAjocDecodeError> {
    drive.aligned_dmx_oamd_updates.clear();
    drive.aligned_umx_oamd_updates.clear();
    drive.aligned_dmx_oamd_timing = FullAjocOamdTimingState::EMPTY;
    drive.aligned_umx_oamd_timing = FullAjocOamdTimingState::EMPTY;
    for (side, blocks) in [
        ("Core/downmix", control.dmx_oamd_blocks.as_slice()),
        ("Full/upmix", control.umx_oamd_blocks.as_slice()),
    ] {
        if blocks.len() > MAX_OAMD_METADATA_BLOCKS {
            return Err(FullAjocDecodeError::oamd_state(format!(
                "Substream {substream}: {side} OAMD has {} updates, exceeding limit {MAX_OAMD_METADATA_BLOCKS}",
                blocks.len()
            )));
        }
    }
    let timings = resolve_effective_oamd_timings(drive, control, substream)?;

    let dmx_start = drive.dmx_oamd;
    let umx_start = drive.umx_oamd;
    let dmx_end = resolve_oamd_updates(
        dmx_start,
        &control.dmx_oamd_blocks,
        control.dmx_num_obj_info_blocks,
        &mut drive.aligned_dmx_oamd_updates,
    )
    .map_err(|error| {
        FullAjocDecodeError::oamd_state(format!(
            "Substream {substream}: Core/downmix OAMD state continuation failed: {error}"
        ))
    })?;
    let umx_end = match resolve_oamd_updates(
        umx_start,
        &control.umx_oamd_blocks,
        control.umx_num_obj_info_blocks,
        &mut drive.aligned_umx_oamd_updates,
    ) {
        Ok(state) => state,
        Err(error) => {
            drive.aligned_dmx_oamd_updates.clear();
            drive.aligned_umx_oamd_updates.clear();
            return Err(FullAjocDecodeError::oamd_state(format!(
                "Substream {substream}: Full/upmix OAMD state continuation failed: {error}"
            )));
        }
    };

    drive.aligned_dmx_oamd_start = dmx_start;
    drive.aligned_umx_oamd_start = umx_start;
    drive.dmx_oamd = dmx_end;
    drive.umx_oamd = umx_end;
    drive.dmx_oamd_timing = timings.dmx.effective();
    drive.umx_oamd_timing = timings.umx.effective();
    drive.aligned_dmx_oamd_timing = timings.dmx;
    drive.aligned_umx_oamd_timing = timings.umx;
    Ok(())
}

fn resolve_due_oamd(
    drive: &mut FullAjocSubstreamState,
    control: Option<&QueuedQmfControl>,
    substream: u32,
) -> Result<(), FullAjocDecodeError> {
    let Some(control) = control else {
        return Ok(());
    };
    if control.provenance.is_some() {
        resolve_aligned_oamd(drive, control, substream)
    } else {
        drive.invalidate_oamd_history();
        Ok(())
    }
}

/// 一路只读帧级 PCM。
#[derive(Debug, Clone, Copy)]
pub struct FullAjocPcmChannel<'a> {
    source: FullAjocPcmSource,
    samples: &'a [f32],
}

impl<'a> FullAjocPcmChannel<'a> {
    /// 这一路样本的来源语义。
    #[must_use]
    pub const fn source(self) -> FullAjocPcmSource {
        self.source
    }

    /// 内部 AC-4 标量尺度的连续 PCM；只在下一次可变 decoder 调用前有效。
    #[must_use]
    pub const fn samples(self) -> &'a [f32] {
        self.samples
    }
}

/// 一次已经解析并完成核心带 ASF 合成的 Full A-JOC 帧输入。
///
/// `P` 通常是 `Vec<f32>`、`Box<[f32]>` 或其他实现 [`AsRef<[f32]>`] 的逐路
/// 缓冲；各路顺序必须与 [`VarChannelElement::signals`](crate::var_element::VarChannelElement::signals)
/// 一致。
#[derive(Debug)]
pub struct FullAjocFrameInput<'a, P> {
    /// 当前 substream 的解析摘要。
    pub parsed: &'a Ac4SubstreamAjoc,
    /// codec frame length，必须与支持凭证绑定的解析上下文一致。
    pub frame_length: u16,
    /// 当前 substream 采样率。
    pub sampling_frequency_hz: u32,
    /// 解析工作区中当前元素写入的 A-SPX 数据。
    pub aspx: &'a [AspxData],
    /// 当前解析状态中有效的 A-SPX 配置。
    pub aspx_config: Option<AspxConfig>,
    /// 当前元素写入的对象控制。
    pub object_controls: &'a [AjocObjectControl],
    /// 当前元素写入的量化矩阵。
    pub matrices: &'a [AjocObjectMatrix],
    /// 本帧的逐路核心带 PCM；每路样本数必须精确等于 `frame_length`。
    pub core_pcm: &'a [P],
    /// 物理 substream 的零基下标。
    pub substream_index: u32,
    /// 调用方当前输出范围内实际参与解码的物理 A-JOC substream 数。
    pub physical_substreams: usize,
    /// `Pseudocode 15` 的 LFE 插回位置。
    pub lfe_position: Option<u32>,
    /// 同一解析快照产生的 A-SPX 支持凭证。
    pub aspx_support: Result<SupportedAspxFrame, AspxBlocker>,
    /// 同一解析快照产生的 Full 支持凭证。
    pub full_support: Result<SupportedAjocFullFrame, FullAjocBlocker>,
    /// 这条 substream 需要的输出层级；首次成功帧后切换前须重置 decoder 状态。
    pub mode: FullAjocDecodeMode,
}

/// 一个 presentation group 随 Full 控制一起到期的 OAMD 状态。
///
/// common/timing 已由调用方按当前 raw AU 完成 group 级继承合并。Full engine
/// 不解释这些字段，只保证它们与同源控制、对象 OAMD 和 AU provenance 一起进入
/// 表 188 槽。字段保持私有，Scene 可以在到期后再建立自己的稳定数据契约。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullAjocGroupOamdState {
    group_index: u32,
    effective_common: Option<OamdCommonData>,
    common_updated_in_source_access_unit: bool,
    effective_timing: Option<OamdTimingData>,
    timing_updated_in_source_access_unit: bool,
}

impl FullAjocGroupOamdState {
    const EMPTY: Self = Self {
        group_index: 0,
        effective_common: None,
        common_updated_in_source_access_unit: false,
        effective_timing: None,
        timing_updated_in_source_access_unit: false,
    };

    /// 建立一条已经完成继承合并的 group OAMD 快照。
    #[must_use]
    pub const fn new(
        group_index: u32,
        effective_common: Option<OamdCommonData>,
        common_updated_in_source_access_unit: bool,
        effective_timing: Option<OamdTimingData>,
        timing_updated_in_source_access_unit: bool,
    ) -> Self {
        Self {
            group_index,
            effective_common,
            common_updated_in_source_access_unit,
            effective_timing,
            timing_updated_in_source_access_unit,
        }
    }

    #[must_use]
    pub const fn group_index(self) -> u32 {
        self.group_index
    }

    #[must_use]
    pub const fn effective_common(self) -> Option<OamdCommonData> {
        self.effective_common
    }

    #[must_use]
    pub const fn common_updated_in_source_access_unit(self) -> bool {
        self.common_updated_in_source_access_unit
    }

    #[must_use]
    pub const fn effective_timing(self) -> Option<OamdTimingData> {
        self.effective_timing
    }

    #[must_use]
    pub const fn timing_updated_in_source_access_unit(self) -> bool {
        self.timing_updated_in_source_access_unit
    }
}

/// 一条 Full A-JOC 输入帧需要随表 188 控制一起延迟的 AU provenance。
///
/// 这些字段由调用方提供，bitstream 层只按原值携带，不解释 MP4 edit、priming
/// 或随机访问提示。调用方已经合并的 group OAMD 状态也附着在同一快照中，避免
/// 到期时从当前 TOC 重新取值。字段私有，后续可以扩展而不冻结结构布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullAjocFrameProvenance {
    access_unit_index: u64,
    source_sample_start: Option<i64>,
    presentation_sample_start: Option<i64>,
    priming_samples: Option<u64>,
    random_access_hint: Option<bool>,
    discontinuity: bool,
    group_oamd_states: [FullAjocGroupOamdState; MAX_SUBSTREAM_GROUPS],
    group_oamd_states_len: usize,
}

impl FullAjocFrameProvenance {
    /// 为一条已定界的 raw AC-4 access unit 建立 provenance。
    #[must_use]
    pub const fn new(access_unit_index: u64) -> Self {
        Self {
            access_unit_index,
            source_sample_start: None,
            presentation_sample_start: None,
            priming_samples: None,
            random_access_hint: None,
            discontinuity: false,
            group_oamd_states: [FullAjocGroupOamdState::EMPTY; MAX_SUBSTREAM_GROUPS],
            group_oamd_states_len: 0,
        }
    }

    /// 追加一条按 `group_index` 严格递增的 group OAMD 状态。
    ///
    /// group 越过 topology 上限、顺序不递增或固定容量已满时返回 `None`；成功时
    /// 快照仍为 `Copy`，不会引入逐帧分配。
    #[must_use]
    pub fn try_with_group_oamd_state(mut self, state: FullAjocGroupOamdState) -> Option<Self> {
        let group_position = usize::try_from(state.group_index()).ok()?;
        if group_position >= MAX_SUBSTREAM_GROUPS
            || self
                .group_oamd_states()
                .last()
                .is_some_and(|previous| previous.group_index() >= state.group_index())
        {
            return None;
        }
        let slot = self.group_oamd_states.get_mut(self.group_oamd_states_len)?;
        *slot = state;
        self.group_oamd_states_len = self.group_oamd_states_len.saturating_add(1);
        Some(self)
    }

    /// 记录该 AU 在 source sample timeline 上的起点。
    #[must_use]
    pub const fn with_source_sample_start(mut self, value: i64) -> Self {
        self.source_sample_start = Some(value);
        self
    }

    /// 记录调用方已投影的 presentation sample 起点。
    #[must_use]
    pub const fn with_presentation_sample_start(mut self, value: i64) -> Self {
        self.presentation_sample_start = Some(value);
        self
    }

    /// 记录调用方传入的 priming 样本数。
    #[must_use]
    pub const fn with_priming_samples(mut self, value: u64) -> Self {
        self.priming_samples = Some(value);
        self
    }

    /// 记录调用方对该 AU 的可选随机访问提示。
    #[must_use]
    pub const fn with_random_access_hint(mut self, value: bool) -> Self {
        self.random_access_hint = Some(value);
        self
    }

    /// 标记该 AU 之前存在调用方声明的不连续。
    #[must_use]
    pub const fn with_discontinuity(mut self, value: bool) -> Self {
        self.discontinuity = value;
        self
    }

    /// 返回调用方分配的 access-unit 下标。
    #[must_use]
    pub const fn access_unit_index(self) -> u64 {
        self.access_unit_index
    }

    /// 返回 source sample timeline 起点。
    #[must_use]
    pub const fn source_sample_start(self) -> Option<i64> {
        self.source_sample_start
    }

    /// 返回调用方已投影的 presentation sample 起点。
    #[must_use]
    pub const fn presentation_sample_start(self) -> Option<i64> {
        self.presentation_sample_start
    }

    /// 返回调用方声明的 priming 样本数。
    #[must_use]
    pub const fn priming_samples(self) -> Option<u64> {
        self.priming_samples
    }

    /// 返回可选随机访问提示。
    #[must_use]
    pub const fn random_access_hint(self) -> Option<bool> {
        self.random_access_hint
    }

    /// 返回该 AU 之前是否存在调用方声明的不连续。
    #[must_use]
    pub const fn discontinuity(self) -> bool {
        self.discontinuity
    }

    /// 当前 raw AU 中已经合并完成、需要随控制延迟的 group OAMD 状态。
    #[must_use]
    pub fn group_oamd_states(&self) -> &[FullAjocGroupOamdState] {
        self.group_oamd_states
            .get(..self.group_oamd_states_len)
            .unwrap_or(&[])
    }
}

/// 一条 Full A-JOC 音频 substream 的完整解码输入。
///
/// 本入口从原始 substream 载荷开始，在 decoder 内依次完成音频语法、ASF、
/// 表 188 对齐、A-SPX/QMF 与 Full 重建。`lfe_position` 必须来自与 `syntax`
/// 相同的 topology 快照；decoder 不从 MP4 或文件容器推导它。
#[derive(Debug, Clone, Copy)]
pub struct FullAjocAudioFrameInput<'a> {
    /// 音频语法载荷、上下文与物理 substream provenance。
    pub syntax: FullAjocSyntaxFrameInput<'a>,
    /// 与本 raw 输入同源、需要随其控制和 OAMD 一起延迟的 AU provenance。
    pub provenance: FullAjocFrameProvenance,
    /// `Pseudocode 15` 的 LFE 插回位置。
    pub lfe_position: Option<u32>,
    /// 这条 substream 需要的输出层级。
    pub mode: FullAjocDecodeMode,
}

/// 一个 OAMD 对象在某个对齐时间点的完整量化状态。
///
/// `metadata` 包含 active/basic/render 的继承结果，`additional` 包含 trim、扩展
/// 位置和耳机字段。两者仍是规范码值；语义浮点换算留给 Scene 层。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullAjocOamdObjectState {
    metadata: ObjectMetadataState,
    additional: AdditionalObjectMetadata,
}

impl FullAjocOamdObjectState {
    /// 完整的 active/basic/render 状态。
    #[must_use]
    pub const fn metadata(self) -> ObjectMetadataState {
        self.metadata
    }

    /// 当前生效的附加对象元数据。
    #[must_use]
    pub const fn additional(self) -> AdditionalObjectMetadata {
        self.additional
    }
}

/// 一侧 OAMD 与输出 PCM 同槽到期的完整有效 timing。
///
/// `effective` 已按 `audio_data_ajoc` 显式 timing、group timing、derive 标志及
/// 前序到期帧完成继承合并。`updated_in_source_access_unit` 只说明产生当前控制
/// 快照的 raw AU 是否刷新了这份 timing；继承帧仍返回完整的 `effective` 值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullAjocOamdTimingState {
    effective: Option<OamdTimingData>,
    updated_in_source_access_unit: bool,
}

impl FullAjocOamdTimingState {
    const EMPTY: Self = Self {
        effective: None,
        updated_in_source_access_unit: false,
    };

    const fn new(effective: Option<OamdTimingData>, updated_in_source_access_unit: bool) -> Self {
        Self {
            effective,
            updated_in_source_access_unit,
        }
    }

    /// 当前帧实际生效的完整 timing；尚未建立自足历史时为 `None`。
    #[must_use]
    pub const fn effective(self) -> Option<OamdTimingData> {
        self.effective
    }

    /// 产生当前控制快照的 raw AU 是否显式刷新了这份 timing。
    #[must_use]
    pub const fn updated_in_source_access_unit(self) -> bool {
        self.updated_in_source_access_unit
    }
}

/// 一个 raw OAMD block 应用后的完整对象状态。
///
/// 快照按 raw block 的码流顺序排列；相同 `block_index` 不会在这一层重排。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullAjocOamdUpdateSnapshot {
    raw: OamdMetadataBlock,
    state: FullAjocOamdObjectState,
}

impl FullAjocOamdUpdateSnapshot {
    /// 原始量化更新，包括对象、块下标与所有已传输字段。
    #[must_use]
    pub const fn raw(self) -> OamdMetadataBlock {
        self.raw
    }

    /// 应用该 raw block 后的完整对象状态。
    #[must_use]
    pub const fn state(self) -> FullAjocOamdObjectState {
        self.state
    }
}

/// 一侧 OAMD 在一帧对齐 PCM 上的借用状态视图。
#[derive(Debug, Clone, Copy)]
pub struct FullAjocOamdFrameSnapshot<'a> {
    start: &'a OamdState,
    end: &'a OamdState,
    updates: &'a [FullAjocOamdUpdateSnapshot],
}

impl<'a> FullAjocOamdFrameSnapshot<'a> {
    /// 读取应用本帧第一个更新前的对象状态。
    #[must_use]
    pub fn object_at_start(self, index: usize) -> Option<FullAjocOamdObjectState> {
        oamd_object_state(self.start, index)
    }

    /// 读取应用本帧全部更新后的对象状态。
    #[must_use]
    pub fn object_at_end(self, index: usize) -> Option<FullAjocOamdObjectState> {
        oamd_object_state(self.end, index)
    }

    /// 按 raw 码流顺序返回每个 block 应用后的状态。
    #[must_use]
    pub const fn updates(self) -> &'a [FullAjocOamdUpdateSnapshot] {
        self.updates
    }
}

/// 已与当前输出 PCM 对齐的一份表 188 side-information 快照。
#[derive(Debug, Clone, Copy)]
pub struct FullAjocAlignedSideInformation<'a> {
    control: &'a QueuedQmfControl,
    provenance: FullAjocFrameProvenance,
    dmx_oamd: FullAjocOamdFrameSnapshot<'a>,
    umx_oamd: FullAjocOamdFrameSnapshot<'a>,
    dmx_oamd_timing: FullAjocOamdTimingState,
    umx_oamd_timing: FullAjocOamdTimingState,
}

impl<'a> FullAjocAlignedSideInformation<'a> {
    /// 产生这份到期控制与 OAMD 的原始 access unit。
    #[must_use]
    pub const fn provenance(self) -> FullAjocFrameProvenance {
        self.provenance
    }

    /// 与当前 PCM 同源并一起到期的 presentation group OAMD 状态。
    #[must_use]
    pub fn group_oamd_states(self) -> &'a [FullAjocGroupOamdState] {
        self.control
            .provenance
            .as_ref()
            .map_or(&[], FullAjocFrameProvenance::group_oamd_states)
    }

    /// Core/downmix 侧按码流顺序保留的 raw OAMD 更新。
    #[must_use]
    pub fn dmx_oamd_blocks(self) -> &'a [OamdMetadataBlock] {
        &self.control.dmx_oamd_blocks
    }

    /// Full/upmix 侧按码流顺序保留的 raw OAMD 更新。
    #[must_use]
    pub fn umx_oamd_blocks(self) -> &'a [OamdMetadataBlock] {
        &self.control.umx_oamd_blocks
    }

    /// Core/downmix 侧与当前 PCM 同源的帧起点、逐块和帧末 OAMD 状态。
    #[must_use]
    pub const fn dmx_oamd(self) -> FullAjocOamdFrameSnapshot<'a> {
        self.dmx_oamd
    }

    /// Full/upmix 侧与当前 PCM 同源的帧起点、逐块和帧末 OAMD 状态。
    #[must_use]
    pub const fn umx_oamd(self) -> FullAjocOamdFrameSnapshot<'a> {
        self.umx_oamd
    }

    /// Core/downmix 侧与当前 PCM 同源、已经完成继承合并的有效 timing。
    #[must_use]
    pub const fn dmx_effective_oamd_timing(self) -> FullAjocOamdTimingState {
        self.dmx_oamd_timing
    }

    /// Full/upmix 侧与当前 PCM 同源、已经完成继承合并的有效 timing。
    #[must_use]
    pub const fn umx_effective_oamd_timing(self) -> FullAjocOamdTimingState {
        self.umx_oamd_timing
    }

    /// Core/downmix 侧 control source access unit 显式刷新的 raw OAMD timing。
    ///
    /// `None` 表示 timing 需要按该模式的有效块数从 group 或前序帧继承。
    #[must_use]
    pub const fn dmx_oamd_timing(self) -> Option<OamdTimingData> {
        self.control.dmx_oamd_timing
    }

    /// Full/upmix 侧 control source access unit 显式刷新的 raw OAMD timing。
    #[must_use]
    pub const fn umx_oamd_timing(self) -> Option<OamdTimingData> {
        self.control.umx_oamd_timing
    }

    /// `b_derive_timing_from_dmx` 的 raw 值；独立携带 full timing 时为 `None`。
    #[must_use]
    pub const fn derive_timing_from_dmx(self) -> Option<bool> {
        self.control.derive_timing_from_dmx
    }

    /// Core/downmix 侧本帧实际生效的 `num_obj_info_blocks`。
    #[must_use]
    pub const fn dmx_num_obj_info_blocks(self) -> u8 {
        self.control.dmx_num_obj_info_blocks
    }

    /// Full/upmix 侧本帧实际生效的 `num_obj_info_blocks`。
    #[must_use]
    pub const fn umx_num_obj_info_blocks(self) -> u8 {
        self.control.umx_num_obj_info_blocks
    }

    /// 返回这份到期快照所属的 codec frame 样本数。
    #[must_use]
    pub const fn frame_length(self) -> u16 {
        self.control.frame_length
    }

    /// 返回这份到期快照的采样率。
    #[must_use]
    pub const fn sampling_frequency_hz(self) -> u32 {
        self.control.sampling_frequency_hz
    }

    /// 返回表 188 对 PCM 应用的样本级对齐延迟。
    #[must_use]
    pub const fn pcm_alignment_delay_samples(self) -> u16 {
        self.control.aspx_support.alignment().pcm_delay()
    }

    /// 返回表 188 控制与 OAMD 快照的帧级对齐延迟。
    #[must_use]
    pub const fn control_alignment_delay_frames(self) -> u8 {
        self.control.aspx_support.alignment().control_delay_frames()
    }
}

/// 一帧借用的 A-SPX 诊断与 Full 对象 PCM。
#[derive(Debug)]
pub struct DecodedFullAjocFrame<'a> {
    observation: FullAjocObservation,
    aligned_side_information: Option<FullAjocAlignedSideInformation<'a>>,
    diagnostic_sources: &'a [FullAjocPcmSource],
    diagnostic_pcm: &'a [Vec<f32>],
    reconstructed_sources: &'a [FullAjocPcmSource],
    reconstructed_pcm: &'a [Vec<f32>],
}

impl<'a> DecodedFullAjocFrame<'a> {
    /// 本帧的路径覆盖观察。
    #[must_use]
    pub const fn observation(&self) -> FullAjocObservation {
        self.observation
    }

    /// 与本帧输出 PCM 对齐的到期控制 provenance、raw OAMD 与有效状态。
    ///
    /// 表 188 warm-up 期间返回 `None`。旧的 parsed-PCM 入口没有提交 provenance，
    /// 因而也不承诺返回本视图。
    #[must_use]
    pub const fn aligned_side_information(&self) -> Option<FullAjocAlignedSideInformation<'a>> {
        self.aligned_side_information
    }

    /// A-SPX 诊断出口的路数。
    #[must_use]
    pub const fn diagnostic_channels(&self) -> usize {
        self.diagnostic_sources.len()
    }

    /// 读取一路 A-SPX 诊断 PCM。
    #[must_use]
    pub fn diagnostic_channel(&self, index: usize) -> Option<FullAjocPcmChannel<'a>> {
        Some(FullAjocPcmChannel {
            source: *self.diagnostic_sources.get(index)?,
            samples: self.diagnostic_pcm.get(index)?.as_slice(),
        })
    }

    /// Full 对象/LFE 出口的路数；opportunistic blocker 时可以为零。
    #[must_use]
    pub const fn reconstructed_channels(&self) -> usize {
        self.reconstructed_sources.len()
    }

    /// 读取一路 Full 对象或 LFE PCM。
    #[must_use]
    pub fn reconstructed_channel(&self, index: usize) -> Option<FullAjocPcmChannel<'a>> {
        Some(FullAjocPcmChannel {
            source: *self.reconstructed_sources.get(index)?,
            samples: self.reconstructed_pcm.get(index)?.as_slice(),
        })
    }
}

/// 一次输入事务的语法/OAMD、ASF 核心带与 QMF/Full 输出。
///
/// 三个视图都借用同一次 decoder 调用；任何可变 decoder 调用前必须先释放本值。
/// `frontend` 描述当前 raw 输入；`output.aligned_side_information()` 描述表 188
/// 中与输出 PCM 一起到期的控制来源与 OAMD。两者在 warm-up 之后通常来自不同 AU。
#[derive(Debug)]
pub struct DecodedFullAjocAudioFrame<'a> {
    frontend: DecodedFullAjocFrontendFrame<'a>,
    output: DecodedFullAjocFrame<'a>,
}

impl<'a> DecodedFullAjocAudioFrame<'a> {
    /// 当前 raw 输入的语法、OAMD 与 ASF 核心带快照。
    #[must_use]
    pub const fn frontend(&self) -> &DecodedFullAjocFrontendFrame<'a> {
        &self.frontend
    }

    /// 本次调用完成表 188 对齐后的 A-SPX 诊断与 Full 对象/LFE PCM。
    #[must_use]
    pub const fn output(&self) -> &DecodedFullAjocFrame<'a> {
        &self.output
    }
}

/// 两份工作区加输出缓冲，整个 decoder 共用一份。
///
/// `AspxWorkspace` 约 127 KiB，`QmfChannelFrame` 每路 16 KiB；逐 substream 各留
/// 一份没有必要——它们只在一次 `drive_element` 调用内有效。
#[derive(Debug)]
struct FullAjocWorkspace {
    first: Box<AspxWorkspace>,
    second: Box<AspxWorkspace>,
    out: Vec<QmfChannelFrame>,
    lfe: QmfChannelFrame,
    ajoc: Box<AjocReconstructionWorkspace>,
    objects: Vec<QmfChannelFrame>,
    zero: QmfChannelFrame,
    diagnostic_sources: Vec<FullAjocPcmSource>,
    diagnostic_pcm: Vec<Vec<f32>>,
    object_sources: Vec<FullAjocPcmSource>,
    object_pcm: Vec<Vec<f32>>,
    aligned: Vec<Vec<f32>>,
}

impl FullAjocWorkspace {
    fn new() -> Self {
        Self {
            first: Box::new(AspxWorkspace::new()),
            second: Box::new(AspxWorkspace::new()),
            out: Vec::new(),
            lfe: empty_channel_frame(),
            ajoc: Box::new(AjocReconstructionWorkspace::new()),
            objects: Vec::new(),
            zero: empty_channel_frame(),
            diagnostic_sources: Vec::new(),
            diagnostic_pcm: Vec::new(),
            object_sources: Vec::new(),
            object_pcm: Vec::new(),
            aligned: Vec::new(),
        }
    }

    fn clear_outputs(&mut self) {
        // 来源长度是输出可见性的唯一依据。保留逐路 PCM 的逻辑长度与容量，
        // 下一次同配置事务会完整覆写样本，失败/reset 不必释放稳定缓冲。
        self.diagnostic_sources.clear();
        self.object_sources.clear();
    }
}

/// 无整文件 sink 的逐帧 Full A-JOC QMF 解码器。
///
/// decoder 统一拥有各物理 substream 的 A-SPX、表 188、A-JOC、OAMD、LFE 与终端
/// QMF 跨帧状态；返回的 PCM 和 OAMD 快照借用内部帧缓冲，下一次可变调用会使其
/// 失效。每条实际使用的 substream 在首帧推进 DSP 前建立最大表 188 控制环与
/// OAMD 更新缓冲，后续成功帧循环复用到期槽。
#[derive(Debug)]
pub struct FullAjocDecoder {
    syntax: FullAjocSyntaxDecoder,
    asf: FullAjocAsfDecoder,
    substreams: Vec<FullAjocSubstreamState>,
    workspace: FullAjocWorkspace,
}

impl FullAjocDecoder {
    /// 建立没有任何历史的新 decoder。
    #[must_use]
    pub fn new() -> Self {
        Self {
            syntax: FullAjocSyntaxDecoder::new(),
            asf: FullAjocAsfDecoder::new(),
            substreams: Vec::new(),
            workspace: FullAjocWorkspace::new(),
        }
    }

    /// 丢弃一条物理 substream 的全部跨帧历史。
    pub fn reset_substream(&mut self, substream_index: u32) {
        self.clear_frontend_observations();
        self.syntax.reset_substream(substream_index);
        self.asf.reset_substream(substream_index);
        let index = usize::try_from(substream_index).unwrap_or(usize::MAX);
        if let Some(state) = self.substreams.get_mut(index) {
            state.reset();
        }
    }

    /// 丢弃全部物理 substream 的历史，保留已分配容量供后续配置复用。
    pub fn reset(&mut self) {
        self.syntax.reset();
        self.asf.reset();
        for state in &mut self.substreams {
            state.reset();
        }
        self.workspace.clear_outputs();
    }

    /// 最近一次可变解码调用中成功解析出的语法、控制与 raw OAMD。
    ///
    /// A-SPX/Full 下游失败不会抹掉该 observation，因此诊断调用方可以在不重跑
    /// `parse_substream_ajoc()` 的情况下记录语法 census。语法阶段本身失败、下一
    /// 次可变调用或 reset 会清空/替换它。
    #[must_use]
    pub fn last_syntax_observation(&self) -> Option<FullAjocSyntaxObservation<'_>> {
        self.syntax.last_observation()
    }

    /// 最近一次可变解码调用中成功重建出的 ASF 核心带 observation。
    ///
    /// A-SPX/Full 下游失败后仍可读取；ASF 阶段本身失败时为 `None`。返回切片借用
    /// decoder 工作区，仅在下一次可变调用前有效。
    #[must_use]
    pub fn last_asf_observation(&self) -> Option<FullAjocAsfFrameObservation<'_>> {
        self.asf.last_observation()
    }

    /// 任何可变入口在改写共享前端工作区前都必须使最近视图失效。
    fn clear_frontend_observations(&mut self) {
        self.syntax.clear_observation();
        self.asf.clear_observation();
    }

    #[cfg(test)]
    pub(super) fn downstream_is_fresh(&self, substream_index: u32) -> bool {
        let index = usize::try_from(substream_index).unwrap_or(usize::MAX);
        self.substreams
            .get(index)
            .is_none_or(FullAjocSubstreamState::is_fresh)
    }

    /// 为一条 Full A-JOC substream 预分配音频语法状态与最大帧工作区。
    ///
    /// 调用方可在配置建立时先执行本方法，使随后稳定解码和截断 AU 重试都不因
    /// 语法工作区扩容而改变缓冲地址。重复调用相同或更小配置不会重新分配。
    pub fn prepare_syntax_substream(
        &mut self,
        substream_index: u32,
        context: &crate::substream_audio::AjocSubstreamContext,
    ) -> Result<(), FullAjocSyntaxError> {
        self.clear_frontend_observations();
        self.syntax.prepare_substream(substream_index, context)
    }

    /// 为语法解析与 ASF 前端一次性预分配同一配置的工作区。
    pub fn prepare_frontend_substream(
        &mut self,
        substream_index: u32,
        context: &crate::substream_audio::AjocSubstreamContext,
    ) -> Result<(), FullAjocFrontendError> {
        self.clear_frontend_observations();
        if let Err(error) = self.syntax.prepare_substream(substream_index, context) {
            self.reset_substream(substream_index);
            return Err(FullAjocFrontendError::Syntax(error));
        }
        let frame_length = context.params.context.frame_len_base;
        if let Err(error) = self.asf.prepare_substream(substream_index, frame_length) {
            self.reset_substream(substream_index);
            return Err(FullAjocFrontendError::Asf(error));
        }
        Ok(())
    }

    /// 为一条 Full A-JOC substream 预分配 ASF overlap 与最大帧工作区。
    ///
    /// 重复调用相同帧长不会重新分配；未经 reset 改变帧长会结构化失败。
    #[cfg(test)]
    pub(super) fn prepare_asf_substream(
        &mut self,
        substream_index: u32,
        frame_length: u16,
    ) -> Result<(), FullAjocAsfError> {
        self.clear_frontend_observations();
        self.asf.prepare_substream(substream_index, frame_length)
    }

    /// 把已经解析的声道元素重建为借用的核心带 PCM。
    ///
    /// 反量化、联合声道矩阵、解组和 IMDCT 使用 decoder 自持工作区；任一路失败
    /// 都会同时清除该物理 substream 的语法、ASF 与下游 QMF/Full 历史。
    #[cfg(test)]
    pub(super) fn decode_asf_frame<'decoder>(
        &'decoder mut self,
        input: FullAjocAsfFrameInput<'_>,
    ) -> Result<DecodedFullAjocAsfFrame<'decoder>, FullAjocAsfError> {
        self.clear_frontend_observations();
        let substream_index = input.substream_index;
        let Self {
            syntax,
            asf,
            substreams,
            workspace,
        } = self;
        match asf.decode_frame(input) {
            Ok(decoded) => Ok(decoded),
            Err(error) => {
                syntax.reset_substream(substream_index);
                let index = usize::try_from(substream_index).unwrap_or(usize::MAX);
                if let Some(state) = substreams.get_mut(index) {
                    state.reset();
                }
                workspace.clear_outputs();
                Err(error)
            }
        }
    }

    /// 解析一条 Full A-JOC 音频 substream，并在同一所有权事务中完成核心带 ASF。
    ///
    /// 返回值同时借用语法/OAMD 工作区与 planar PCM；中间不复制大型
    /// [`crate::channel::ChannelElement`]。输入切片耗尽时不改变已提交历史，其他
    /// 阶段失败会让该物理 substream 的语法、ASF 与下游 Full 历史一起失效。
    /// 成功帧没有推进 QMF/Full，因此也会切断已有下游历史，避免后续统一入口跨过
    /// 这一帧继续。
    pub fn decode_frontend_frame<'decoder>(
        &'decoder mut self,
        input: FullAjocSyntaxFrameInput<'_>,
    ) -> Result<DecodedFullAjocFrontendFrame<'decoder>, FullAjocFrontendError> {
        self.clear_frontend_observations();
        let substream_index = input.substream_index;
        let Self {
            syntax,
            asf,
            substreams,
            workspace,
        } = self;
        let mut syntax_frame = match syntax.decode_frame(input) {
            Ok(decoded) => decoded,
            Err(error) => {
                if !error.is_input_exhausted() {
                    asf.reset_substream(substream_index);
                    let index = usize::try_from(substream_index).unwrap_or(usize::MAX);
                    if let Some(state) = substreams.get_mut(index) {
                        state.reset();
                    }
                    workspace.clear_outputs();
                }
                return Err(FullAjocFrontendError::Syntax(error));
            }
        };
        let frame_length = syntax_frame.context().params.context.frame_len_base;
        match asf.decode_frame(FullAjocAsfFrameInput {
            elements: syntax_frame.elements(),
            frame_length,
            substream_index,
        }) {
            Ok(asf_frame) => {
                reset_downstream_history(substreams, workspace, substream_index);
                Ok(DecodedFullAjocFrontendFrame {
                    syntax: syntax_frame,
                    asf: asf_frame,
                })
            }
            Err(error) => {
                syntax_frame.reset_state();
                let index = usize::try_from(substream_index).unwrap_or(usize::MAX);
                if let Some(state) = substreams.get_mut(index) {
                    state.reset();
                }
                workspace.clear_outputs();
                Err(FullAjocFrontendError::Asf(error))
            }
        }
    }

    /// 解析一条 Full A-JOC 音频 substream，返回借用的语法与控制快照。
    ///
    /// Session 可以先消费 OAMD/ASF 快照，再由后续统一帧入口驱动 QMF。syntax-only
    /// 成功帧不会推进 ASF/QMF/Full，因此会清除已有 overlap 与下游历史，避免之后
    /// 的组合入口跨帧拼接；解析失败则同时清除该物理 substream 已有的语法与 DSP
    /// 历史；输入切片耗尽则保留全部已提交历史，供调用方补全后重试。
    pub fn decode_syntax_frame<'decoder>(
        &'decoder mut self,
        input: FullAjocSyntaxFrameInput<'_>,
    ) -> Result<DecodedFullAjocSyntaxFrame<'decoder>, FullAjocSyntaxError> {
        self.clear_frontend_observations();
        let substream_index = input.substream_index;
        let Self {
            syntax,
            asf,
            substreams,
            workspace,
        } = self;
        match syntax.decode_frame(input) {
            Ok(decoded) => {
                asf.reset_substream(substream_index);
                reset_downstream_history(substreams, workspace, substream_index);
                Ok(decoded)
            }
            Err(error) => {
                if !error.is_input_exhausted() {
                    asf.reset_substream(substream_index);
                    let index = usize::try_from(substream_index).unwrap_or(usize::MAX);
                    if let Some(state) = substreams.get_mut(index) {
                        state.reset();
                    }
                    workspace.clear_outputs();
                }
                Err(error)
            }
        }
    }

    /// 从同一条 substream 载荷完成语法、ASF、表 188、QMF 与 Full 重建。
    ///
    /// 返回的语法/OAMD、核心带 PCM 和最终 PCM 都属于同一次原子调用。
    /// `frontend` 保留当前 raw 输入；`output` 中的 aligned side information 则保留
    /// 与当前 PCM 一起到期的 raw/effective OAMD 与 AU provenance，warm-up 期间为空。
    /// 输入切片耗尽时不改变该物理 substream 的已提交状态，调用方可补全同一帧后
    /// 重试；其他阶段失败会清空全部继承、overlap、控制 FIFO 与 DSP 历史。唯一例外是
    /// [`FullAjocDecodeMode::ObserveFull`]：语法与 ASF 已成功后的下游失败只回滚 QMF/Full，
    /// 保留可继续 census 的前端历史。任何失败都不会返回只完成前半段的帧。
    pub fn decode_audio_frame<'decoder>(
        &'decoder mut self,
        input: FullAjocAudioFrameInput<'_>,
    ) -> Result<DecodedFullAjocAudioFrame<'decoder>, FullAjocAudioFrameError> {
        self.decode_audio_frame_with_policy(input, InputExhaustionPolicy::PreserveForRetry)
    }

    /// 从调用方确认完整的物理 substream 载荷完成一次 Full 帧事务。
    ///
    /// 与 [`Self::decode_audio_frame`] 的区别仅在输入耗尽策略：本入口把任何
    /// `OutOfBounds` 或 `audio_size` 越界视为完整有界载荷内的坏帧，并立即清空
    /// 语法、ASF、控制 FIFO 与 DSP 历史。已经用外层 index table 验证 raw AU
    /// 完整性的 Session 应调用本入口；尚在累积 substream 切片的调用方仍使用
    /// 可重试入口。
    pub fn decode_complete_audio_frame<'decoder>(
        &'decoder mut self,
        input: FullAjocAudioFrameInput<'_>,
    ) -> Result<DecodedFullAjocAudioFrame<'decoder>, FullAjocAudioFrameError> {
        self.decode_audio_frame_with_policy(input, InputExhaustionPolicy::InvalidateCompleteInput)
    }

    fn decode_audio_frame_with_policy<'decoder>(
        &'decoder mut self,
        input: FullAjocAudioFrameInput<'_>,
        input_exhaustion: InputExhaustionPolicy,
    ) -> Result<DecodedFullAjocAudioFrame<'decoder>, FullAjocAudioFrameError> {
        self.clear_frontend_observations();
        let FullAjocAudioFrameInput {
            syntax: syntax_input,
            provenance,
            lfe_position,
            mode,
        } = input;
        let substream_index = syntax_input.substream_index;
        let physical_substreams = syntax_input.physical_substreams;
        let Self {
            syntax,
            asf,
            substreams,
            workspace,
        } = self;
        let syntax_result = match input_exhaustion {
            InputExhaustionPolicy::PreserveForRetry => syntax.decode_frame(syntax_input),
            InputExhaustionPolicy::InvalidateCompleteInput => {
                syntax.decode_complete_frame(syntax_input)
            }
        };
        let mut syntax_frame = match syntax_result {
            Ok(decoded) => decoded,
            Err(error) => {
                let preserve = error.is_input_exhausted()
                    && input_exhaustion == InputExhaustionPolicy::PreserveForRetry;
                if !preserve {
                    asf.reset_substream(substream_index);
                    let slot = usize::try_from(substream_index).unwrap_or(usize::MAX);
                    if let Some(state) = substreams.get_mut(slot) {
                        state.reset();
                    }
                    workspace.clear_outputs();
                }
                return Err(FullAjocAudioFrameError::Syntax(error));
            }
        };
        let state = match prepare_decode_state(substreams, substream_index) {
            Ok(state) => state,
            Err(error) => {
                syntax_frame.reset_state();
                asf.reset_substream(substream_index);
                workspace.clear_outputs();
                return Err(FullAjocAudioFrameError::Decode(error));
            }
        };
        let context = syntax_frame.context();
        let frame_length = context.params.context.frame_len_base;
        let mut asf_frame = match asf.decode_frame(FullAjocAsfFrameInput {
            elements: syntax_frame.elements(),
            frame_length,
            substream_index,
        }) {
            Ok(decoded) => decoded,
            Err(error) => {
                syntax_frame.reset_state();
                state.reset();
                workspace.clear_outputs();
                return Err(FullAjocAudioFrameError::Asf(error));
            }
        };

        let parsed = syntax_frame.parsed();
        let result = drive_frame(
            state,
            workspace,
            FullAjocFrameInput {
                parsed: &parsed,
                frame_length,
                sampling_frequency_hz: context.params.context.sampling_frequency_hz,
                aspx: syntax_frame.aspx(),
                aspx_config: syntax_frame.aspx_config(),
                object_controls: syntax_frame.object_controls(),
                matrices: syntax_frame.matrices(),
                core_pcm: asf_frame.pcm(),
                substream_index,
                physical_substreams,
                lfe_position,
                aspx_support: syntax_frame.aspx_support(),
                full_support: syntax_frame.full_support(),
                mode,
            },
            FullAjocInputSideInformation {
                provenance: Some(provenance),
                dmx_oamd_blocks: syntax_frame.dmx_blocks(),
                umx_oamd_blocks: syntax_frame.umx_blocks(),
                dmx_oamd_timing: parsed.audio.dmx_timing,
                umx_oamd_timing: parsed.audio.umx_timing,
                derive_timing_from_dmx: parsed.audio.derive_timing_from_dmx,
                dmx_num_obj_info_blocks: parsed.audio.dmx_num_obj_info_blocks,
                umx_num_obj_info_blocks: parsed.audio.umx_num_obj_info_blocks,
            },
        );
        let observation = match result {
            Ok(observation) => observation,
            Err(error) => {
                // ObserveFull 把已验证的语法/ASF 作为独立 census 前端；
                // 下游 blocker 不得让后续依赖帧丢失配置或 overlap。
                if mode != FullAjocDecodeMode::ObserveFull {
                    syntax_frame.reset_state();
                    asf_frame.reset_state();
                }
                state.reset();
                workspace.clear_outputs();
                return Err(FullAjocAudioFrameError::Decode(error));
            }
        };

        let aligned_side_information = state.aligned_side_information();
        Ok(DecodedFullAjocAudioFrame {
            frontend: DecodedFullAjocFrontendFrame {
                syntax: syntax_frame,
                asf: asf_frame,
            },
            output: DecodedFullAjocFrame {
                observation,
                aligned_side_information,
                diagnostic_sources: &workspace.diagnostic_sources,
                diagnostic_pcm: &workspace.diagnostic_pcm,
                reconstructed_sources: &workspace.object_sources,
                reconstructed_pcm: &workspace.object_pcm,
            },
        })
    }

    /// 驱动一帧并返回借用的 A-SPX/Full PCM。
    ///
    /// 任一失败都会让对应物理 substream 的历史失效，且不会返回半帧输出。
    pub fn decode_frame<'decoder, P>(
        &'decoder mut self,
        input: FullAjocFrameInput<'_, P>,
    ) -> Result<DecodedFullAjocFrame<'decoder>, FullAjocDecodeError>
    where
        P: AsRef<[f32]>,
    {
        self.clear_frontend_observations();
        let substream_index = input.substream_index;
        let Self {
            syntax,
            asf,
            substreams,
            workspace,
        } = self;
        let state = prepare_decode_state(substreams, substream_index)?;
        let result = drive_frame(
            state,
            workspace,
            input,
            FullAjocInputSideInformation::empty(),
        );
        let observation = match result {
            Ok(observation) => observation,
            Err(error) => {
                syntax.reset_substream(substream_index);
                asf.reset_substream(substream_index);
                state.reset();
                workspace.clear_outputs();
                return Err(error);
            }
        };
        let aligned_side_information = state.aligned_side_information();
        Ok(DecodedFullAjocFrame {
            observation,
            aligned_side_information,
            diagnostic_sources: &workspace.diagnostic_sources,
            diagnostic_pcm: &workspace.diagnostic_pcm,
            reconstructed_sources: &workspace.object_sources,
            reconstructed_pcm: &workspace.object_pcm,
        })
    }
}

impl Default for FullAjocDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// 部分帧入口成功时，语法或 ASF 已经越过一帧，但下游没有对应地推进。
/// 只切断 QMF/Full 历史，保留刚提交的前端继承状态供同一种部分入口继续使用。
fn reset_downstream_history(
    substreams: &mut [FullAjocSubstreamState],
    workspace: &mut FullAjocWorkspace,
    substream_index: u32,
) {
    let index = usize::try_from(substream_index).unwrap_or(usize::MAX);
    if let Some(state) = substreams.get_mut(index) {
        state.reset();
    }
    workspace.clear_outputs();
}

fn prepare_decode_state(
    substreams: &mut Vec<FullAjocSubstreamState>,
    substream_index: u32,
) -> Result<&mut FullAjocSubstreamState, FullAjocDecodeError> {
    let slot = usize::try_from(substream_index).map_err(|_| {
        FullAjocDecodeError::object_shape(format!(
            "Substream {substream_index}: index cannot be represented as native usize"
        ))
    })?;
    if slot >= MAX_SUBSTREAMS {
        return Err(FullAjocDecodeError::object_shape(format!(
            "Substream {substream_index}: index exceeds decoder capacity {MAX_SUBSTREAMS}"
        )));
    }
    if substreams.len() <= slot {
        substreams.resize_with(slot.saturating_add(1), FullAjocSubstreamState::new);
    }
    let available = substreams.len();
    substreams.get_mut(slot).ok_or_else(|| {
        FullAjocDecodeError::object_shape(format!(
            "Substream {substream_index}: state slot is still missing after decoder expansion ({available} slots available)"
        ))
    })
}

fn validate_pcm_shape<P>(
    frame_pcm: &[P],
    channels: usize,
    frame_length: u16,
    substream: u32,
) -> Result<usize, FullAjocDecodeError>
where
    P: AsRef<[f32]>,
{
    if frame_pcm.len() != channels {
        return Err(FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: frame retained {} PCM channels, but element declares {channels}",
            frame_pcm.len()
        )));
    }
    let samples = frame_pcm.first().map_or(0, |data| data.as_ref().len());
    let expected = usize::from(frame_length);
    if samples != expected {
        return Err(FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: channel 0 has {samples} samples in this frame, but token frame length is {expected}"
        )));
    }
    for (channel, data) in frame_pcm.iter().enumerate().skip(1) {
        let data = data.as_ref();
        if data.len() != samples {
            return Err(FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: channel {channel} has {} samples in this frame, while channel 0 has {samples}",
                data.len()
            )));
        }
    }
    Ok(samples)
}

fn validate_aspx_payload<'a>(
    aspx: &'a [AspxData],
    element: &VarChannelElement,
    substream: u32,
) -> Result<&'a [AspxData], FullAjocDecodeError> {
    let expected = usize::from(element.aspx_elements());
    if aspx.len() != expected || expected > MAX_ASPX_ELEMENTS {
        return Err(FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: element declares {expected} A-SPX data sets, but {} were provided",
            aspx.len()
        )));
    }
    Ok(aspx)
}

fn validate_object_payloads<'a>(
    object_controls: &'a [AjocObjectControl],
    matrices: &'a [AjocObjectMatrix],
    objects: usize,
    substream: u32,
) -> Result<(&'a [AjocObjectControl], &'a [AjocObjectMatrix]), FullAjocDecodeError> {
    let object_controls = object_controls.get(..objects).ok_or_else(|| {
        FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: A-JOC declares {objects} objects, but only {} controls were retained",
            object_controls.len()
        ))
    })?;
    let matrices = matrices.get(..objects).ok_or_else(|| {
        FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: A-JOC declares {objects} objects, but only {} matrices were retained",
            matrices.len()
        ))
    })?;
    Ok((object_controls, matrices))
}

/// 驱动一帧并把逐路 PCM 暂存在 decoder 工作区。
///
/// `frame_pcm` 是本帧本 substream 的逐路核心带 PCM，按 `(element, channel)`
/// 升序，即 `VarChannelElement::signals()` 的顺序。
///
/// 诊断输出按 **A-JOC 输入顺序**排列，LFE 在最后；Full 输出按
/// `Pseudocode 15` 插回 LFE。
///
/// # Errors
///
/// 驱动或合成失败时返回原因；调用方须据此让该 substream 的历史失效。
fn drive_frame<P>(
    drive: &mut FullAjocSubstreamState,
    scratch: &mut FullAjocWorkspace,
    input: FullAjocFrameInput<'_, P>,
    side_information: FullAjocInputSideInformation<'_>,
) -> Result<FullAjocObservation, FullAjocDecodeError>
where
    P: AsRef<[f32]>,
{
    let FullAjocFrameInput {
        parsed,
        frame_length,
        sampling_frequency_hz,
        aspx,
        aspx_config,
        object_controls,
        matrices,
        core_pcm: frame_pcm,
        substream_index: substream,
        physical_substreams,
        lfe_position,
        aspx_support,
        full_support,
        mode: full_requirement,
    } = input;
    let element = &parsed.audio.var_element;
    let ajoc = &parsed.audio.ajoc;
    let dialogue_objects = parsed.audio.dmx_de.num_dlg_obj;
    let config = aspx_config.as_ref();
    let supported = aspx_support
        .map_err(|blocker| FullAjocDecodeError::unsupported_aspx(blocker, substream))?;
    if !supported.matches(element, frame_length) {
        return Err(format!("Substream {substream}: A-SPX token does not belong to the current parsed element or frame length").into());
    }
    let alignment = supported.alignment();
    let control_delay = usize::from(alignment.control_delay_frames());
    if control_delay == 0 || control_delay > MAX_CONTROL_ALIGNMENT_DELAY_FRAMES {
        return Err(format!(
            "Substream {substream}: Table 188 control delay {control_delay} is outside 1..={MAX_CONTROL_ALIGNMENT_DELAY_FRAMES}"
        )
        .into());
    }
    if let Some(previous) = drive.alignment_config
        && previous != alignment
    {
        return Err(format!(
                "Substream {substream}: Table 188 delay configuration changed from {previous:?} to {alignment:?}; reset is required"
            )
            .into());
    }
    if drive.controls.len() > control_delay {
        return Err(format!(
            "Substream {substream}: QMF control FIFO contains {} frames, exceeding Table 188 delay of {control_delay}",
            drive.controls.len()
        )
        .into());
    }

    let input_topology = QmfInputTopology::from_element(element);
    if let Some(previous) = drive.input_topology
        && previous != input_topology
    {
        return Err(FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: QMF input topology changed from {previous:?} to {input_topology:?}; reset is required"
        )));
    }
    if ajoc.num_dmx_signals != element.n_dmx_signals {
        return Err(FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: A-JOC declares {} inputs, but element QMF output has {} channels",
            ajoc.num_dmx_signals, element.n_dmx_signals
        )));
    }
    if element.b_has_lfe != lfe_position.is_some() {
        return Err(FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: element LFE={}, but Pseudocode 15 reinsertion position is {lfe_position:?}",
            element.b_has_lfe
        )));
    }
    validate_current_full_support(
        &full_support,
        supported,
        element,
        frame_length,
        sampling_frequency_hz,
        physical_substreams,
        ajoc,
        dialogue_objects,
        full_requirement,
        substream,
    )?;

    let objects = usize::try_from(ajoc.num_umx_signals).map_err(|_| {
        FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: A-JOC object count {} cannot be represented as a native index",
            ajoc.num_umx_signals
        ))
    })?;
    let (object_controls, matrices) =
        validate_object_payloads(object_controls, matrices, objects, substream)?;
    let aspx = validate_aspx_payload(aspx, element, substream)?;

    let channels = input_topology.channels();
    let samples = validate_pcm_shape(frame_pcm, channels, frame_length, substream)?;
    drive.bind_decode_mode(full_requirement, substream)?;
    drive.recycle_aligned_control();
    drive.prepare_control_buffers();
    drive.ensure_input(channels);

    let FullAjocWorkspace {
        first,
        second,
        out,
        lfe,
        ajoc: ajoc_workspace,
        objects: object_qmf,
        zero,
        diagnostic_sources,
        diagnostic_pcm,
        object_sources,
        object_pcm,
        aligned,
    } = scratch;
    if aligned.len() < channels {
        aligned.resize_with(channels, Vec::new);
    }
    for (channel, (input, output)) in frame_pcm
        .iter()
        .map(AsRef::as_ref)
        .zip(aligned.iter_mut().take(channels))
        .enumerate()
    {
        output.resize(input.len(), 0.0);
        let Some(state) = drive.alignment.get_mut(channel) else {
            return Err(format!(
                "Substream {substream}: channel {channel} lacks frame-alignment state"
            )
            .into());
        };
        state.process(input, alignment, output).map_err(|error| {
            format!(
                "Substream {substream}: frame alignment failed for channel {channel}: {error:?}"
            )
        })?;
    }
    let mut pcm = [&[][..]; MAX_SIGNALS];
    for (target, source) in pcm.iter_mut().zip(aligned.iter().take(channels)) {
        *target = source.as_slice();
    }
    let pcm = pcm.get(..channels).ok_or_else(|| {
        FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: {channels} QMF inputs exceed fixed limit {MAX_SIGNALS}"
        ))
    })?;

    let due_control = take_due_control(&mut drive.controls, control_delay).map_err(|queued| {
        format!(
            "Substream {substream}: QMF control FIFO contains {queued} frames, exceeding Table 188 delay of {control_delay}"
        )
    })?;
    if let Some(queued) = due_control.as_ref() {
        let queued_topology = QmfInputTopology::from_element(&queued.element);
        if queued_topology != input_topology {
            return Err(format!(
                "Substream {substream}: due control belongs to {queued_topology:?}, but current frame-aligned PCM belongs to {input_topology:?}"
            )
            .into());
        }
        if queued.aspx_support.alignment() != alignment
            || !queued
                .aspx_support
                .matches(&queued.element, queued.frame_length)
        {
            return Err(format!("Substream {substream}: due A-SPX control token is misaligned with the current signal").into());
        }
        if let Ok(credential) = queued.full_support {
            if credential.aspx().alignment() != alignment
                || !credential.matches(
                    &queued.element,
                    queued.frame_length,
                    queued.sampling_frequency_hz,
                    queued.physical_substreams,
                    &queued.ajoc,
                    queued.dialogue_objects,
                )
            {
                return Err(format!(
                    "Substream {substream}: Table 188 configuration of due A-JOC control is misaligned with the current signal"
                )
                .into());
            }
        } else if full_requirement == FullAjocDecodeMode::RequireFull {
            let Err(blocker) = queued.full_support else {
                return Err(format!("Substream {substream}: A-JOC full token is missing").into());
            };
            return Err(FullAjocDecodeError::unsupported_full(blocker, substream));
        }
    }
    let pending_fullband = due_control
        .as_ref()
        .map_or(0, |queued| usize::from(queued.element.n_dmx_signals));
    let needed_fullband = usize::from(element.n_dmx_signals).max(pending_fullband);
    if out.len() < needed_fullband {
        out.resize_with(needed_fullband, empty_channel_frame);
    }
    let workspace = DriveWorkspace {
        aspx: [first, second],
        states: &mut drive.states,
        intermediates: &mut drive.intermediates,
    };
    let output_element = if let Some(queued) = due_control.as_ref() {
        // `master_reset` 按控制数据真正应用的顺序比对，而不是按解析到达顺序。
        let decision = drive.master_reset.frame(queued.config.as_ref());
        let params = ElementParams {
            base_samp_freq_48: queued.sampling_frequency_hz == 48_000,
            master_reset: decision.is_reset(),
            first_frame: drive.fresh,
        };
        let lfe_output = queued.element.b_has_lfe.then_some(&mut *lfe);
        drive_element(
            &queued.element,
            &queued.aspx,
            queued.config.as_ref(),
            params,
            pcm,
            workspace,
            out,
            lfe_output,
        )
        .map_err(|error| describe(substream, error))?;
        drive.master_reset.commit(decision);
        drive.fresh = false;
        queued.element
    } else {
        // 还没有到期控制：只预热 QMF、低带、TNA 输入和终端输出历史。当前控制
        // 留在 FIFO，绝不能在这里提前推进 envelope/noise/tone 等控制状态。
        let params = ElementParams {
            base_samp_freq_48: sampling_frequency_hz == 48_000,
            master_reset: false,
            first_frame: true,
        };
        let lfe_output = element.b_has_lfe.then_some(&mut *lfe);
        prime_control_delay_element(
            element, aspx, config, params, pcm, workspace, out, lfe_output,
        )
        .map_err(|error| describe(substream, error))?;
        *element
    };

    let timeslots = samples / 64;
    let qmf_timeslots = u8::try_from(timeslots).map_err(|_| {
        format!("Substream {substream}: {timeslots} QMF timeslots exceed the A-JOC frame interface")
    })?;

    // 诊断出口也先整帧暂存。后续 full 重建或任一路终端合成失败时，不会向 sink
    // 留下半帧；调用方随后 reset 所有已局部推进的状态。
    let fullband = usize::from(output_element.n_dmx_signals);
    let diagnostic_channels = fullband.saturating_add(usize::from(output_element.b_has_lfe));
    diagnostic_sources.clear();
    diagnostic_pcm.resize_with(diagnostic_channels, Vec::new);
    diagnostic_pcm.truncate(diagnostic_channels);
    // 来源语义与「哪个迭代器供出这一帧」同源：LFE 的标签就挂在追加它的那次
    // `then_some` 上，而不是回头拿 `slot` 与 `fullband` 再比一次。后者是同一
    // 件事的第二份推导，两处任一改动就会让 LFE 冒充 A-JOC 输入下标。
    for slot in 0..diagnostic_channels {
        let frame = if slot < fullband {
            out.get(slot).ok_or_else(|| {
                format!("Substream {substream}: A-JOC input QMF is missing for channel {slot}")
            })?
        } else {
            &*lfe
        };
        if drive.synthesis.get(slot).is_none() {
            return Err(
                format!("Substream {substream}: channel {slot} lacks synthesis state").into(),
            );
        }
        let target = diagnostic_pcm.get_mut(slot).ok_or_else(|| {
            format!("Substream {substream}: diagnostic PCM buffer is missing for channel {slot}")
        })?;
        target.resize(samples, 0.0);
        if frame.get(..timeslots).is_none() {
            return Err(
                format!("Substream {substream}: channel {slot} has too few timeslots").into(),
            );
        }
    }

    let states = drive.synthesis.get_mut(..diagnostic_channels).ok_or_else(|| {
        format!(
            "Substream {substream}: diagnostic synthesis workspace has fewer than {diagnostic_channels} channels"
        )
    })?;
    let outputs = diagnostic_pcm
        .get_mut(..diagnostic_channels)
        .ok_or_else(|| {
            format!(
                "Substream {substream}: diagnostic PCM workspace has fewer than {diagnostic_channels} channels"
            )
        })?;
    let inputs = out.get(..fullband).ok_or_else(|| {
        format!("Substream {substream}: A-JOC input QMF has fewer than {fullband} channels")
    })?;
    let (fullband_states, lfe_states) = states.split_at_mut(fullband);
    let (fullband_outputs, lfe_outputs) = outputs.split_at_mut(fullband);
    synthesise_ac4_pcm_channels(inputs, timeslots, fullband_states, fullband_outputs).map_err(
        |error| format!("Substream {substream}: diagnostic batch synthesis failed: {error:?}"),
    )?;
    if output_element.b_has_lfe {
        let Some(lfe_state) = lfe_states.first_mut() else {
            return Err(format!("Substream {substream}: diagnostic LFE state is missing").into());
        };
        let Some(lfe_output) = lfe_outputs.first_mut() else {
            return Err(format!("Substream {substream}: diagnostic LFE PCM is missing").into());
        };
        let lfe_frame = lfe.get(..timeslots).ok_or_else(|| {
            format!("Substream {substream}: diagnostic LFE has too few timeslots")
        })?;
        synthesise_ac4_pcm(lfe_frame, lfe_state, lfe_output).map_err(|error| {
            format!("Substream {substream}: diagnostic LFE synthesis failed: {error:?}")
        })?;
    }

    for (slot, target) in outputs.iter().enumerate() {
        if let Some(sample) = target.iter().position(|value| !value.is_finite()) {
            return Err(format!(
                "Substream {substream}: diagnostic PCM for channel {slot} is non-finite at sample {sample}"
            )
            .into());
        }
    }
    diagnostic_sources.extend((0..fullband).map(|_| FullAjocPcmSource::AjocInput));
    if output_element.b_has_lfe {
        diagnostic_sources.push(FullAjocPcmSource::Lfe);
    }

    let observation = if matches!(
        full_requirement,
        FullAjocDecodeMode::AspxOnly | FullAjocDecodeMode::RequireCore
    ) {
        object_sources.clear();
        object_pcm.clear();
        FullAjocObservation::default()
    } else {
        match due_control.as_ref() {
            Some(queued) => match queued.full_support {
                Ok(_) => {
                    drive_full_frame(
                        drive,
                        ajoc_workspace,
                        object_qmf,
                        object_sources,
                        object_pcm,
                        &queued.ajoc,
                        &queued.object_controls,
                        &queued.matrices,
                        queued.lfe_position,
                        &queued.element,
                        out,
                        lfe,
                        samples,
                        timeslots,
                        qmf_timeslots,
                        substream,
                    )?;
                    FullAjocObservation {
                        reconstructed: true,
                        wet: enabled_wet_path(&queued.ajoc),
                        ..FullAjocObservation::default()
                    }
                }
                Err(blocker) => {
                    if full_requirement == FullAjocDecodeMode::RequireFull {
                        return Err(FullAjocDecodeError::unsupported_full(blocker, substream));
                    }
                    drive.reset_full();
                    object_sources.clear();
                    object_pcm.clear();
                    FullAjocObservation::default()
                }
            },
            None => prime_pending_full_output(
                drive,
                object_sources,
                object_pcm,
                zero,
                lfe,
                full_support,
                ajoc,
                lfe_position,
                element,
                full_requirement,
                samples,
                timeslots,
                substream,
            )?,
        }
    };

    if object_sources.len() != object_pcm.len() {
        return Err(FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: object sources have {} channels, while staged PCM has {}",
            object_sources.len(),
            object_pcm.len()
        )));
    }

    // 只有完整音频入口提交 provenance，才承诺 OAMD 与这份 PCM 来自同一 raw
    // frame。parsed-PCM 控制到期意味中间存在 metadata 盲区，必须切断旧的继承链。
    resolve_due_oamd(drive, due_control.as_ref(), substream)?;

    let buffers = drive.control_buffers.pop().ok_or_else(|| {
        FullAjocDecodeError::from(format!(
            "Substream {substream}: preallocated Table 188 control slots are exhausted"
        ))
    })?;
    let current = QueuedQmfControl::capture(
        buffers,
        QmfControlSnapshot {
            element,
            aspx,
            config,
            frame_length,
            sampling_frequency_hz,
            physical_substreams,
            dialogue_objects,
            aspx_support: supported,
            ajoc,
            object_controls,
            matrices,
            dmx_oamd_blocks: side_information.dmx_oamd_blocks,
            umx_oamd_blocks: side_information.umx_oamd_blocks,
            dmx_oamd_timing: side_information.dmx_oamd_timing,
            umx_oamd_timing: side_information.umx_oamd_timing,
            derive_timing_from_dmx: side_information.derive_timing_from_dmx,
            dmx_num_obj_info_blocks: side_information.dmx_num_obj_info_blocks,
            umx_num_obj_info_blocks: side_information.umx_num_obj_info_blocks,
            provenance: side_information.provenance,
            full_support,
            lfe_position,
        },
    );
    drive.controls.push_back(current);
    drive.aligned_control = due_control;
    drive.alignment_config = Some(alignment);
    drive.input_topology = Some(input_topology);
    Ok(observation)
}

#[expect(
    clippy::too_many_arguments,
    reason = "full 帧事务显式绑定到期 side information、QMF 输入/LFE、两类状态与暂存输出"
)]
fn drive_full_frame(
    drive: &mut FullAjocSubstreamState,
    workspace: &mut AjocReconstructionWorkspace,
    output: &mut Vec<QmfChannelFrame>,
    sources: &mut Vec<FullAjocPcmSource>,
    pcm: &mut Vec<Vec<f32>>,
    ajoc: &Ajoc,
    controls: &[AjocObjectControl],
    matrices: &[AjocObjectMatrix],
    lfe_position: Option<u32>,
    element: &VarChannelElement,
    input: &[QmfChannelFrame],
    lfe: &QmfChannelFrame,
    samples: usize,
    timeslots: usize,
    qmf_timeslots: u8,
    substream: u32,
) -> Result<(), FullAjocDecodeError> {
    let topology = ObjectOutputTopology::checked(
        ajoc.num_umx_signals,
        lfe_position,
        element.b_has_lfe,
        substream,
    )
    .map_err(FullAjocDecodeError::object_shape)?;
    install_output_topology(drive, topology, substream)?;
    if output.len() < topology.objects {
        output.resize_with(topology.objects, empty_channel_frame);
    }
    let input = input
        .get(..usize::from(ajoc.num_dmx_signals))
        .ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: A-JOC requires {} QMF inputs, but only {} were produced",
                ajoc.num_dmx_signals,
                input.len()
            ))
        })?;
    let output_len = output.len();
    let output_slice = output.get_mut(..topology.objects).ok_or_else(|| {
        FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: A-JOC requires {} object-QMF channels, but only {} were reserved",
            topology.objects, output_len
        ))
    })?;
    reconstruct_frame(
        ajoc,
        controls,
        matrices,
        qmf_timeslots,
        input,
        &mut drive.ajoc,
        workspace,
        output_slice,
    )
    .map_err(|error| {
        FullAjocDecodeError::reconstruction(format!(
            "Substream {substream}: A-JOC object reconstruction failed: {error}"
        ))
    })?;
    synthesise_object_outputs(
        drive,
        topology,
        output_slice,
        lfe,
        false,
        sources,
        pcm,
        samples,
        timeslots,
        substream,
    )
}

#[derive(Debug, Clone, Copy)]
struct FullWarmupControl {
    support: Result<SupportedAjocFullFrame, FullAjocBlocker>,
    objects: u32,
    lfe_position: Option<u32>,
    element_has_lfe: bool,
}

/// 控制尚未到期时，终端预热属于 FIFO 中最早等待输出的控制，而不是刚解析的控制。
///
/// 例如两帧延迟下，受支持的 A 后跟 blocker B：B 到达时 A 仍需要第二帧终端预热；
/// 只有 B 自己到期形成输出缺口时才能清除 Full 历史。
#[expect(
    clippy::too_many_arguments,
    reason = "终端预热必须同时拿到最早待输出控制、当前回退控制、QMF 时间轴与借用输出缓冲"
)]
fn prime_pending_full_output(
    drive: &mut FullAjocSubstreamState,
    sources: &mut Vec<FullAjocPcmSource>,
    pcm: &mut Vec<Vec<f32>>,
    zero: &QmfChannelFrame,
    lfe: &QmfChannelFrame,
    current_support: Result<SupportedAjocFullFrame, FullAjocBlocker>,
    current_ajoc: &Ajoc,
    current_lfe_position: Option<u32>,
    current_element: &VarChannelElement,
    requirement: FullAjocDecodeMode,
    samples: usize,
    timeslots: usize,
    substream: u32,
) -> Result<FullAjocObservation, FullAjocDecodeError> {
    let pending = drive.controls.front().map_or(
        FullWarmupControl {
            support: current_support,
            objects: current_ajoc.num_umx_signals,
            lfe_position: current_lfe_position,
            element_has_lfe: current_element.b_has_lfe,
        },
        |queued| FullWarmupControl {
            support: queued.full_support,
            objects: queued.ajoc.num_umx_signals,
            lfe_position: queued.lfe_position,
            element_has_lfe: queued.element.b_has_lfe,
        },
    );

    match pending.support {
        Ok(_) => {
            prime_full_output(
                drive,
                sources,
                pcm,
                zero,
                lfe,
                pending.objects,
                pending.lfe_position,
                pending.element_has_lfe,
                samples,
                timeslots,
                substream,
            )?;
            Ok(FullAjocObservation {
                warmup: true,
                ..FullAjocObservation::default()
            })
        }
        Err(blocker) => {
            if requirement == FullAjocDecodeMode::RequireFull {
                return Err(FullAjocDecodeError::unsupported_full(blocker, substream));
            }
            drive.reset_full();
            sources.clear();
            pcm.clear();
            Ok(FullAjocObservation::default())
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "预热必须显式拿到已确认拓扑、对象零 QMF、真实 LFE QMF、终端状态和帧时间轴，且不得接触 A-JOC 控制状态"
)]
fn prime_full_output(
    drive: &mut FullAjocSubstreamState,
    sources: &mut Vec<FullAjocPcmSource>,
    pcm: &mut Vec<Vec<f32>>,
    zero: &QmfChannelFrame,
    lfe: &QmfChannelFrame,
    objects: u32,
    lfe_position: Option<u32>,
    element_has_lfe: bool,
    samples: usize,
    timeslots: usize,
    substream: u32,
) -> Result<(), FullAjocDecodeError> {
    let topology = ObjectOutputTopology::checked(objects, lfe_position, element_has_lfe, substream)
        .map_err(FullAjocDecodeError::object_shape)?;
    install_output_topology(drive, topology, substream)?;
    synthesise_object_outputs(
        drive,
        topology,
        core::slice::from_ref(zero),
        lfe,
        true,
        sources,
        pcm,
        samples,
        timeslots,
        substream,
    )
}

fn install_output_topology(
    drive: &mut FullAjocSubstreamState,
    topology: ObjectOutputTopology,
    substream: u32,
) -> Result<(), FullAjocDecodeError> {
    if let Some(previous) = drive.output_topology
        && previous != topology
    {
        return Err(FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: A-JOC output topology changed from {previous:?} to {topology:?}; reset is required"
        )));
    }
    drive.ensure_output(topology.channels());
    drive.output_topology = Some(topology);
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "终端合成同时需要拓扑、对象/LFE QMF、逐路状态与整帧暂存缓冲"
)]
fn synthesise_object_outputs(
    drive: &mut FullAjocSubstreamState,
    topology: ObjectOutputTopology,
    objects: &[QmfChannelFrame],
    lfe: &QmfChannelFrame,
    reuse_first_object: bool,
    sources: &mut Vec<FullAjocPcmSource>,
    pcm: &mut Vec<Vec<f32>>,
    samples: usize,
    timeslots: usize,
    substream: u32,
) -> Result<(), FullAjocDecodeError> {
    sources.clear();
    let channels = topology.channels();
    pcm.resize_with(channels, Vec::new);
    pcm.truncate(channels);

    // 先核对全部来源与容量；批量内核一旦开始便只做不会失败的定长 DSP。这样即使
    // 后一声道形状损坏，也不会先推进前面声道的 QMF 历史。
    for slot in 0..channels {
        let source = topology.source(slot).ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object output channel {slot} has no source semantics"
            ))
        })?;
        let frame = match source {
            FullAjocPcmSource::AjocObject(object) => objects
                .get(if reuse_first_object { 0 } else { object })
                .ok_or_else(|| {
                    FullAjocDecodeError::object_shape(format!(
                        "Substream {substream}: QMF output for object {object} is missing"
                    ))
                })?,
            FullAjocPcmSource::Lfe => lfe,
            FullAjocPcmSource::AjocInput => {
                return Err(FullAjocDecodeError::object_shape(format!(
                    "Substream {substream}: object-terminal channel {slot} is incorrectly marked as an A-JOC input"
                )));
            }
        };
        drive.object_synthesis.get(slot).ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object-terminal synthesis state is missing for channel {slot}"
            ))
        })?;
        let target = pcm.get_mut(slot).ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object PCM buffer is missing for channel {slot}"
            ))
        })?;
        target.resize(samples, 0.0);
        frame.get(..timeslots).ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object QMF channel {slot} has too few timeslots"
            ))
        })?;
    }

    let states = drive
        .object_synthesis
        .get_mut(..channels)
        .ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object-terminal synthesis state workspace has fewer than {channels} channels"
            ))
        })?;
    let outputs = pcm.get_mut(..channels).ok_or_else(|| {
        FullAjocDecodeError::object_shape(format!(
            "Substream {substream}: object PCM workspace has fewer than {channels} channels"
        ))
    })?;

    if reuse_first_object {
        // 控制预热把同一张零 QMF 帧广播给全部对象；它不进入稳态性能路径，保留
        // 标量实现可避免为广播形状另建一份描述数组。
        for slot in 0..channels {
            let source = topology.source(slot).ok_or_else(|| {
                FullAjocDecodeError::object_shape(format!(
                    "Substream {substream}: object output channel {slot} has no source semantics"
                ))
            })?;
            let frame = match source {
                FullAjocPcmSource::AjocObject(_) => objects.first().ok_or_else(|| {
                    FullAjocDecodeError::object_shape(format!(
                        "Substream {substream}: broadcast QMF output for object channel {slot} is missing"
                    ))
                })?,
                FullAjocPcmSource::Lfe => lfe,
                FullAjocPcmSource::AjocInput => {
                    return Err(FullAjocDecodeError::object_shape(format!(
                        "Substream {substream}: object-terminal channel {slot} is incorrectly marked as an A-JOC input"
                    )));
                }
            };
            let frame = frame.get(..timeslots).ok_or_else(|| {
                FullAjocDecodeError::object_shape(format!(
                    "Substream {substream}: object-terminal channel {slot} has too few QMF timeslots"
                ))
            })?;
            let state = states.get_mut(slot).ok_or_else(|| {
                FullAjocDecodeError::object_shape(format!(
                    "Substream {substream}: object-terminal channel {slot} lacks synthesis state"
                ))
            })?;
            let output = outputs.get_mut(slot).ok_or_else(|| {
                FullAjocDecodeError::object_shape(format!(
                    "Substream {substream}: object-terminal channel {slot} lacks PCM output"
                ))
            })?;
            synthesise_ac4_pcm(frame, state, output).map_err(|error| {
                FullAjocDecodeError::object_shape(format!(
                    "Substream {substream}: object-terminal synthesis failed for channel {slot}: {error:?}"
                ))
            })?;
        }
    } else if let Some(position) = topology.lfe_position {
        let (before_states, lfe_and_after_states) = states.split_at_mut(position);
        let Some((lfe_state, after_states)) = lfe_and_after_states.split_first_mut() else {
            return Err(FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: LFE synthesis state is missing at channel {position}"
            )));
        };
        let (before_outputs, lfe_and_after_outputs) = outputs.split_at_mut(position);
        let Some((lfe_output, after_outputs)) = lfe_and_after_outputs.split_first_mut() else {
            return Err(FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: LFE PCM output is missing at channel {position}"
            )));
        };
        let before_objects = objects.get(..position).ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object QMF prefix before LFE channel {position} is missing"
            ))
        })?;
        let after_objects = objects.get(position..topology.objects).ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object QMF suffix after LFE channel {position} is missing"
            ))
        })?;
        synthesise_ac4_pcm_channels(before_objects, timeslots, before_states, before_outputs)
            .map_err(|error| {
                FullAjocDecodeError::object_shape(format!(
                    "Substream {substream}: object-terminal batch before LFE failed: {error:?}"
                ))
            })?;
        let lfe_frame = lfe.get(..timeslots).ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object-terminal LFE has too few QMF timeslots"
            ))
        })?;
        synthesise_ac4_pcm(lfe_frame, lfe_state, lfe_output).map_err(|error| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object-terminal LFE synthesis failed: {error:?}"
            ))
        })?;
        synthesise_ac4_pcm_channels(after_objects, timeslots, after_states, after_outputs)
            .map_err(|error| {
                FullAjocDecodeError::object_shape(format!(
                    "Substream {substream}: object-terminal batch after LFE failed: {error:?}"
                ))
            })?;
    } else {
        let object_frames = objects.get(..topology.objects).ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object QMF output has fewer than {} channels",
                topology.objects
            ))
        })?;
        synthesise_ac4_pcm_channels(object_frames, timeslots, states, outputs).map_err(
            |error| {
                FullAjocDecodeError::object_shape(format!(
                    "Substream {substream}: object-terminal batch synthesis failed: {error:?}"
                ))
            },
        )?;
    }

    for (slot, target) in outputs.iter().enumerate() {
        if let Some(sample) = target.iter().position(|value| !value.is_finite()) {
            return Err(FullAjocDecodeError::objects_nonfinite(format!(
                "Substream {substream}: object PCM for channel {slot} is non-finite at sample {sample}"
            )));
        }
    }
    for slot in 0..channels {
        let source = topology.source(slot).ok_or_else(|| {
            FullAjocDecodeError::object_shape(format!(
                "Substream {substream}: object output channel {slot} has no source semantics"
            ))
        })?;
        sources.push(source);
    }
    Ok(())
}

fn enabled_wet_path(ajoc: &Ajoc) -> bool {
    (0..usize::from(ajoc.num_decorr)).any(|index| ajoc.decorr_enable(index) == Some(true))
}

fn describe(substream: u32, error: DriveError) -> String {
    format!("Substream {substream}: A-SPX drive failed: {error:?}")
}

/// 取出刚好到期的控制帧；未攒满时返回 `None`，超量时返回实际长度。
///
/// 当前帧只在后续全部处理成功后才压入，因此该函数把“读取到期控制”和“提交
/// 新控制”分开，失败路径不会把尚未产出 PCM 的控制误记成已提交。
fn take_due_control<T>(queue: &mut VecDeque<T>, delay: usize) -> Result<Option<T>, usize> {
    match queue.len().cmp(&delay) {
        core::cmp::Ordering::Less => Ok(None),
        core::cmp::Ordering::Equal => Ok(queue.pop_front()),
        core::cmp::Ordering::Greater => Err(queue.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::BitBuf;
    use alloc::vec;
    use macindecode_ac4_bitstream::{
        oamd::{InfoStatus, ObjectInfoBlock},
        reader::BitReader,
    };

    const fn group_state(group_index: u32) -> FullAjocGroupOamdState {
        FullAjocGroupOamdState::new(group_index, None, false, None, false)
    }

    fn oamd_timing(sample_offset: u8, blocks: u8) -> OamdTimingData {
        let mut bits = BitBuf::new();
        bits.push(true); // oa_sample_offset_type 的长分支。
        bits.push(true); // 显式 5 比特 sample_offset。
        bits.push_bits(u32::from(sample_offset), 5);
        bits.push_bits(u32::from(blocks), 3);
        for block in 0..blocks {
            bits.push_bits(u32::from(block), 6);
            bits.push_bits(0, 2); // 零 ramp。
        }
        OamdTimingData::parse(&mut BitReader::new(bits.as_slice())).expect("测试 timing 应可解析")
    }

    const fn timed_group_state(
        group_index: u32,
        timing: OamdTimingData,
        updated: bool,
    ) -> FullAjocGroupOamdState {
        FullAjocGroupOamdState::new(group_index, None, false, Some(timing), updated)
    }

    #[test]
    fn provenance_keeps_fixed_capacity_group_oamd_in_stream_order() {
        let provenance = FullAjocFrameProvenance::new(9)
            .try_with_group_oamd_state(group_state(0))
            .and_then(|value| value.try_with_group_oamd_state(group_state(2)))
            .expect("递增且未越界的 group 状态应进入固定快照");

        assert_eq!(
            provenance.group_oamd_states(),
            &[group_state(0), group_state(2)]
        );
        assert!(
            provenance
                .try_with_group_oamd_state(group_state(2))
                .is_none(),
            "重复或逆序 group 不得进入快照"
        );
        assert!(
            FullAjocFrameProvenance::new(9)
                .try_with_group_oamd_state(group_state(
                    u32::try_from(MAX_SUBSTREAM_GROUPS).unwrap_or(u32::MAX)
                ))
                .is_none(),
            "超出 topology 上限的 group 不得进入快照"
        );
    }

    #[test]
    fn control_fifo_releases_each_value_after_the_table_188_delay() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        struct ControlIdentity {
            element: usize,
            aspx: usize,
            ajoc: usize,
            matrix: usize,
            lfe_position: usize,
        }

        for delay in [1usize, 2, 4] {
            let mut queue = VecDeque::new();
            let mut released = Vec::new();
            for current in 0..8usize {
                released.push(take_due_control(&mut queue, delay).expect("FIFO 应合法"));
                queue.push_back(ControlIdentity {
                    element: current,
                    aspx: current.saturating_add(10),
                    ajoc: current.saturating_add(20),
                    matrix: current.saturating_add(30),
                    lfe_position: current.saturating_add(40),
                });
            }
            assert!(
                released
                    .get(..delay)
                    .is_some_and(|initial| initial.iter().all(Option::is_none))
            );
            for (output_frame, value) in released.iter().enumerate().skip(delay) {
                let control = output_frame - delay;
                assert_eq!(
                    *value,
                    Some(ControlIdentity {
                        element: control,
                        aspx: control + 10,
                        ajoc: control + 20,
                        matrix: control + 30,
                        lfe_position: control + 40,
                    }),
                    "延迟 {delay} 帧的所有控制字段必须来自同一份所有权快照"
                );
            }
            assert_eq!(queue.len(), delay, "尾部应恰好保留表 188 要求的帧数");
        }

        let mut corrupt = VecDeque::from([0usize, 1, 2]);
        assert_eq!(take_due_control(&mut corrupt, 2), Err(3));
        assert_eq!(corrupt, VecDeque::from([0, 1, 2]), "超量错误不得弹出数据");
    }

    #[test]
    fn required_outputs_reject_their_current_blocker_before_fifo_enqueue() {
        let blocker = FullAjocBlocker::ActiveDialogueEnhancement {
            dialogue_objects: 1,
        };
        let error =
            reject_required_output_blocker(&Err(blocker), FullAjocDecodeMode::RequireFull, 1, 2)
                .expect_err("Required 必须在当前 blocker 入 FIFO 前失败");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::Unsupported);
        assert_eq!(
            error.unsupported_reason(),
            Some(FullAjocUnsupported::Full(blocker))
        );
        assert!(error.detail().contains("Substream 2"));
        assert!(error.detail().contains("dialogue-enhancement"));

        for requirement in [
            FullAjocDecodeMode::AspxOnly,
            FullAjocDecodeMode::ObserveFull,
        ] {
            assert!(
                reject_required_output_blocker(&Err(blocker), requirement, 1, 2).is_ok(),
                "{requirement:?} 不应把仅 full 不受支持提升为帧错误"
            );
        }

        let error =
            reject_required_output_blocker(&Err(blocker), FullAjocDecodeMode::RequireCore, 1, 2)
                .expect_err("Core 对象出口不得忽略活动 dialogue enhancement");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::Unsupported);
        assert_eq!(
            error.unsupported_reason(),
            Some(FullAjocUnsupported::Full(blocker))
        );

        let full_only_blocker = FullAjocBlocker::UmxSignals {
            num_umx_signals: 129,
        };
        let error = reject_required_output_blocker(
            &Err(full_only_blocker),
            FullAjocDecodeMode::RequireCore,
            2,
            2,
        )
        .expect_err("更早的 Full-only blocker 不得遮蔽 Core dialogue enhancement");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::Unsupported);
        assert_eq!(
            error.unsupported_reason(),
            Some(FullAjocUnsupported::Full(
                FullAjocBlocker::ActiveDialogueEnhancement {
                    dialogue_objects: 2,
                }
            ))
        );

        let alternative = FullAjocBlocker::AlternativeObjectMetadata;
        for requirement in [
            FullAjocDecodeMode::RequireCore,
            FullAjocDecodeMode::RequireFull,
        ] {
            let error = reject_required_output_blocker(&Err(alternative), requirement, 0, 2)
                .expect_err("未应用的 alternative OAMD 不得进入必需对象出口");
            assert_eq!(error.kind(), FullAjocDecodeErrorKind::Unsupported);
            assert_eq!(
                error.unsupported_reason(),
                Some(FullAjocUnsupported::Full(alternative))
            );
        }
        assert!(
            reject_required_output_blocker(
                &Err(alternative),
                FullAjocDecodeMode::ObserveFull,
                0,
                2,
            )
            .is_ok(),
            "观察模式应继续保留 alternative 语法结果"
        );
    }

    #[test]
    fn pcm_shape_is_bound_to_the_credential_frame_length() {
        let short = [vec![0.0; 1_024]];
        let error =
            validate_pcm_shape(&short, 1, 2_048, 2).expect_err("短 PCM 不得借用长帧凭证进入 A-SPX");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::ObjectShapeMismatch);
        assert!(error.detail().contains("token frame length is 2048"));

        let missing: Vec<Vec<f32>> = Vec::new();
        let error =
            validate_pcm_shape(&missing, 1, 2_048, 2).expect_err("PCM 路数不足必须归入形状错误");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::ObjectShapeMismatch);

        let ragged = [vec![0.0; 2_048], vec![0.0; 1_984]];
        let error =
            validate_pcm_shape(&ragged, 2, 2_048, 2).expect_err("逐路帧长不一致必须归入形状错误");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::ObjectShapeMismatch);

        let valid = [vec![0.0; 2_048], vec![0.0; 2_048]];
        assert_eq!(
            validate_pcm_shape(&valid, 2, 2_048, 2).expect("匹配凭证的 PCM 应通过"),
            2_048
        );
    }

    #[test]
    fn object_payload_capacity_errors_keep_the_shape_classification() {
        let controls = [AjocObjectControl::default()];
        let no_controls: [AjocObjectControl; 0] = [];
        let no_matrices: [AjocObjectMatrix; 0] = [];

        let error = validate_object_payloads(&no_controls, &no_matrices, 1, 2)
            .expect_err("控制容量不足必须失败");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::ObjectShapeMismatch);
        assert!(error.detail().contains("only 0 controls"));

        let error = validate_object_payloads(&controls, &no_matrices, 1, 2)
            .expect_err("矩阵容量不足必须失败");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::ObjectShapeMismatch);
        assert!(error.detail().contains("only 0 matrices"));
    }

    #[test]
    fn aspx_payload_count_is_bound_to_the_parsed_element() {
        let element = VarChannelElement::for_test(true, None, 3, false, &[]);
        let one = [AspxData::empty()];
        let error =
            validate_aspx_payload(&one, &element, 2).expect_err("三路元素需要两份 A-SPX 数据");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::ObjectShapeMismatch);
        assert!(error.detail().contains("declares 2 A-SPX data sets"));

        let two = [AspxData::empty(), AspxData::empty()];
        assert_eq!(
            validate_aspx_payload(&two, &element, 2)
                .expect("匹配元素的 A-SPX 数据应通过")
                .len(),
            2
        );
    }

    fn control_snapshot<'a>(
        element: &'a VarChannelElement,
        aspx: &'a [AspxData],
        ajoc: &'a Ajoc,
        object_controls: &'a [AjocObjectControl],
        matrices: &'a [AjocObjectMatrix],
        side_information: FullAjocInputSideInformation<'a>,
    ) -> QmfControlSnapshot<'a> {
        let aspx_support = AspxBlocker::check(element, 2_048).expect("测试 A-SPX 应受支持");
        let full_support = SupportedAjocFullFrame::check_values(aspx_support, 48_000, 1, *ajoc, 0)
            .expect("测试 Full 应受支持");
        QmfControlSnapshot {
            element,
            aspx,
            config: None,
            frame_length: 2_048,
            sampling_frequency_hz: 48_000,
            physical_substreams: 1,
            dialogue_objects: 0,
            aspx_support,
            ajoc,
            object_controls,
            matrices,
            dmx_oamd_blocks: side_information.dmx_oamd_blocks,
            umx_oamd_blocks: side_information.umx_oamd_blocks,
            dmx_oamd_timing: side_information.dmx_oamd_timing,
            umx_oamd_timing: side_information.umx_oamd_timing,
            derive_timing_from_dmx: side_information.derive_timing_from_dmx,
            dmx_num_obj_info_blocks: side_information.dmx_num_obj_info_blocks,
            umx_num_obj_info_blocks: side_information.umx_num_obj_info_blocks,
            provenance: side_information.provenance,
            full_support: Ok(full_support),
            lfe_position: None,
        }
    }

    fn oamd_block(object_index: u8, block_index: u8) -> OamdMetadataBlock {
        OamdMetadataBlock {
            object_index,
            block_index,
            info: ObjectInfoBlock::default(),
        }
    }

    fn reused_oamd_block(object_index: u8, block_index: u8) -> OamdMetadataBlock {
        OamdMetadataBlock {
            object_index,
            block_index,
            info: ObjectInfoBlock {
                basic_info_status: InfoStatus::Reuse,
                render_info_status: InfoStatus::Reuse,
                ..ObjectInfoBlock::default()
            },
        }
    }

    #[test]
    fn recycled_control_snapshot_keeps_buffer_addresses_and_capacities() {
        let element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let aspx = [AspxData::empty()];
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [AjocObjectControl::default()];
        let matrices = [AjocObjectMatrix::new()];
        let dmx_oamd_blocks = [OamdMetadataBlock::default()];
        let umx_oamd_blocks = [OamdMetadataBlock::default(); 2];
        let provenance = Some(
            FullAjocFrameProvenance::new(7)
                .with_source_sample_start(-2_048)
                .with_random_access_hint(true)
                .try_with_group_oamd_state(group_state(1))
                .expect("测试 group 状态应可进入 provenance"),
        );

        let first = QueuedQmfControl::capture(
            QueuedQmfControlBuffers::new(),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance,
                    dmx_oamd_blocks: &dmx_oamd_blocks,
                    umx_oamd_blocks: &umx_oamd_blocks,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        let pointers = (
            first.aspx.as_ptr(),
            first.object_controls.as_ptr(),
            first.matrices.as_ptr(),
            first.dmx_oamd_blocks.as_ptr(),
            first.umx_oamd_blocks.as_ptr(),
        );
        let capacities = (
            first.aspx.capacity(),
            first.object_controls.capacity(),
            first.matrices.capacity(),
            first.dmx_oamd_blocks.capacity(),
            first.umx_oamd_blocks.capacity(),
        );

        let second = QueuedQmfControl::capture(
            first.into_buffers(),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance,
                    dmx_oamd_blocks: &dmx_oamd_blocks,
                    umx_oamd_blocks: &umx_oamd_blocks,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        assert!(core::ptr::eq(pointers.0, second.aspx.as_ptr()));
        assert!(core::ptr::eq(pointers.1, second.object_controls.as_ptr()));
        assert!(core::ptr::eq(pointers.2, second.matrices.as_ptr()));
        assert!(core::ptr::eq(pointers.3, second.dmx_oamd_blocks.as_ptr()));
        assert!(core::ptr::eq(pointers.4, second.umx_oamd_blocks.as_ptr()));
        assert_eq!(
            capacities,
            (
                second.aspx.capacity(),
                second.object_controls.capacity(),
                second.matrices.capacity(),
                second.dmx_oamd_blocks.capacity(),
                second.umx_oamd_blocks.capacity(),
            ),
            "稳定帧不得扩容控制快照"
        );
        assert_eq!(second.dmx_oamd_blocks, dmx_oamd_blocks);
        assert_eq!(second.umx_oamd_blocks, umx_oamd_blocks);
        assert_eq!(second.provenance, provenance);
        assert_eq!(
            second
                .provenance
                .as_ref()
                .map(FullAjocFrameProvenance::group_oamd_states),
            Some(&[group_state(1)][..])
        );
    }

    #[test]
    fn table_188_control_ring_is_preallocated_and_reclaimed_on_reset() {
        let mut drive = FullAjocSubstreamState::new();
        drive.prepare_control_buffers();
        assert!(drive.controls.capacity() >= MAX_CONTROL_ALIGNMENT_DELAY_FRAMES);
        assert_eq!(drive.control_buffers.len(), CONTROL_SNAPSHOT_SLOTS);
        let oamd_buffers = (
            drive.aligned_dmx_oamd_updates.as_ptr(),
            drive.aligned_umx_oamd_updates.as_ptr(),
            drive.aligned_dmx_oamd_updates.capacity(),
            drive.aligned_umx_oamd_updates.capacity(),
        );

        let element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let aspx = [AspxData::empty()];
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [AjocObjectControl::default()];
        let matrices = [AjocObjectMatrix::new()];
        let dmx_oamd_blocks = [OamdMetadataBlock::default()];
        let umx_oamd_blocks = [OamdMetadataBlock::default()];
        let buffer = drive.control_buffers.pop().expect("预分配槽必须存在");
        drive.controls.push_back(QueuedQmfControl::capture(
            buffer,
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(
                        FullAjocFrameProvenance::new(3)
                            .try_with_group_oamd_state(group_state(0))
                            .expect("测试 group 状态应可进入 provenance"),
                    ),
                    dmx_oamd_blocks: &dmx_oamd_blocks,
                    umx_oamd_blocks: &umx_oamd_blocks,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        ));
        drive.prepare_control_buffers();
        assert_eq!(
            drive.control_buffers.len(),
            CONTROL_SNAPSHOT_SLOTS.saturating_sub(1),
            "重复 prepare 不得把 FIFO 外的空闲池重新补到五槽"
        );
        drive.aligned_control = drive.controls.pop_front();
        let aligned = drive
            .aligned_side_information()
            .expect("带 provenance 的控制槽应形成到期视图");
        assert_eq!(aligned.group_oamd_states(), &[group_state(0)]);

        drive.reset();
        assert!(drive.controls.is_empty());
        assert!(drive.aligned_control.is_none());
        assert_eq!(
            drive.control_buffers.len(),
            CONTROL_SNAPSHOT_SLOTS,
            "reset 必须回收而不是释放延迟环与借用输出载荷"
        );

        let dropped_due_slot = drive
            .control_buffers
            .pop()
            .expect("模拟失败帧已弹出的到期槽");
        drop(dropped_due_slot);
        drive.reset();
        assert_eq!(
            drive.control_buffers.len(),
            CONTROL_SNAPSHOT_SLOTS,
            "失败帧丢弃到期槽后，reset 必须在重起解前补齐预分配环"
        );
        assert!(core::ptr::eq(
            oamd_buffers.0,
            drive.aligned_dmx_oamd_updates.as_ptr()
        ));
        assert!(core::ptr::eq(
            oamd_buffers.1,
            drive.aligned_umx_oamd_updates.as_ptr()
        ));
        assert_eq!(oamd_buffers.2, drive.aligned_dmx_oamd_updates.capacity());
        assert_eq!(oamd_buffers.3, drive.aligned_umx_oamd_updates.capacity());
    }

    #[test]
    fn aligned_oamd_exposes_frame_start_and_each_update_in_stream_order() {
        let mut drive = FullAjocSubstreamState::new();
        drive.prepare_control_buffers();
        let pointers = (
            drive.aligned_dmx_oamd_updates.as_ptr(),
            drive.aligned_umx_oamd_updates.as_ptr(),
        );
        let capacities = (
            drive.aligned_dmx_oamd_updates.capacity(),
            drive.aligned_umx_oamd_updates.capacity(),
        );

        let element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let aspx = [AspxData::empty()];
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [AjocObjectControl::default()];
        let matrices = [AjocObjectMatrix::new()];
        let dmx = [oamd_block(1, 0), oamd_block(0, 0)];
        let umx = [oamd_block(0, 0)];
        let control = QueuedQmfControl::capture(
            drive.control_buffers.pop().expect("应有预分配控制槽"),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(FullAjocFrameProvenance::new(7)),
                    dmx_oamd_blocks: &dmx,
                    umx_oamd_blocks: &umx,
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );

        resolve_aligned_oamd(&mut drive, &control, 2).expect("两侧自足 OAMD 应成功");
        drive.aligned_control = Some(control);
        let aligned = drive
            .aligned_side_information()
            .expect("带 provenance 的到期控制应公开 side information");
        assert_eq!(aligned.provenance().access_unit_index(), 7);
        assert!(
            aligned
                .dmx_oamd()
                .object_at_start(0)
                .is_some_and(|state| state.metadata().basic.is_none()),
            "帧起点必须是应用第一个 block 前的完整状态"
        );
        let updates = aligned.dmx_oamd().updates();
        assert_eq!(updates.len(), 2);
        assert_eq!(
            updates
                .iter()
                .map(|update| update.raw().object_index)
                .collect::<Vec<_>>(),
            vec![1, 0]
        );
        assert!(updates.iter().all(|update| {
            let metadata = update.state().metadata();
            metadata.active && metadata.basic.is_some() && metadata.render.is_some()
        }));
        assert!(
            aligned
                .dmx_oamd()
                .object_at_end(0)
                .is_some_and(|state| state.metadata().render.is_some())
        );
        assert_eq!(
            aligned
                .umx_oamd()
                .updates()
                .first()
                .map(|update| update.raw()),
            umx.first().copied()
        );
        assert!(core::ptr::eq(
            pointers.0,
            drive.aligned_dmx_oamd_updates.as_ptr()
        ));
        assert!(core::ptr::eq(
            pointers.1,
            drive.aligned_umx_oamd_updates.as_ptr()
        ));
        assert_eq!(
            capacities,
            (
                drive.aligned_dmx_oamd_updates.capacity(),
                drive.aligned_umx_oamd_updates.capacity(),
            ),
            "稳定帧的 OAMD 快照不得扩容"
        );
    }

    #[test]
    fn aligned_oamd_timing_uses_explicit_derive_group_and_history_precedence() {
        let mut drive = FullAjocSubstreamState::new();
        drive.prepare_control_buffers();
        let element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let aspx = [AspxData::empty()];
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [AjocObjectControl::default()];
        let matrices = [AjocObjectMatrix::new()];
        let initial = [oamd_block(0, 0)];
        let reused = [reused_oamd_block(0, 0)];
        let explicit = oamd_timing(8, 1);
        let group = oamd_timing(16, 1);
        let other_group = oamd_timing(24, 1);

        let first = QueuedQmfControl::capture(
            drive.control_buffers.pop().expect("应有预分配控制槽"),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(
                        FullAjocFrameProvenance::new(20)
                            .try_with_group_oamd_state(timed_group_state(0, group, true))
                            .and_then(|value| {
                                value.try_with_group_oamd_state(timed_group_state(
                                    1,
                                    other_group,
                                    true,
                                ))
                            })
                            .expect("冲突的 group timing 应进入 provenance"),
                    ),
                    dmx_oamd_blocks: &initial,
                    umx_oamd_blocks: &initial,
                    dmx_oamd_timing: Some(explicit),
                    derive_timing_from_dmx: Some(true),
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        resolve_aligned_oamd(&mut drive, &first, 4)
            .expect("显式/derive timing 应遮蔽无关的 group timing 冲突");
        drive.aligned_control = Some(first);
        let first_aligned = drive.aligned_side_information().expect("首帧应到期");
        for timing in [
            first_aligned.dmx_effective_oamd_timing(),
            first_aligned.umx_effective_oamd_timing(),
        ] {
            assert_eq!(timing.effective(), Some(explicit));
            assert!(timing.updated_in_source_access_unit());
        }
        assert_eq!(first_aligned.dmx_oamd_timing(), Some(explicit));
        assert_eq!(first_aligned.umx_oamd_timing(), None);
        assert_eq!(first_aligned.derive_timing_from_dmx(), Some(true));

        let first = drive
            .aligned_control
            .take()
            .expect("首帧控制应仍归 decoder");
        let second = QueuedQmfControl::capture(
            first.into_buffers(),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(
                        FullAjocFrameProvenance::new(21)
                            .try_with_group_oamd_state(timed_group_state(0, group, false))
                            .expect("继承的 group timing 应进入 provenance"),
                    ),
                    dmx_oamd_blocks: &reused,
                    umx_oamd_blocks: &reused,
                    derive_timing_from_dmx: Some(false),
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        resolve_aligned_oamd(&mut drive, &second, 4).expect("group timing 应覆盖旧 audio timing");
        drive.aligned_control = Some(second);
        let second_aligned = drive.aligned_side_information().expect("第二帧应到期");
        for timing in [
            second_aligned.dmx_effective_oamd_timing(),
            second_aligned.umx_effective_oamd_timing(),
        ] {
            assert_eq!(timing.effective(), Some(group));
            assert!(!timing.updated_in_source_access_unit());
        }

        let second = drive
            .aligned_control
            .take()
            .expect("第二帧控制应仍归 decoder");
        let third = QueuedQmfControl::capture(
            second.into_buffers(),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(FullAjocFrameProvenance::new(22)),
                    dmx_oamd_blocks: &reused,
                    umx_oamd_blocks: &reused,
                    derive_timing_from_dmx: Some(false),
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        resolve_aligned_oamd(&mut drive, &third, 4).expect("缺省帧应继承到期 timing 历史");
        drive.aligned_control = Some(third);
        let third_aligned = drive.aligned_side_information().expect("第三帧应到期");
        assert_eq!(
            third_aligned.umx_effective_oamd_timing().effective(),
            Some(group)
        );
        assert!(
            !third_aligned
                .umx_effective_oamd_timing()
                .updated_in_source_access_unit()
        );

        drive.reset();
        assert!(drive.is_fresh(), "reset 必须连同有效 timing 历史一起清空");
    }

    #[test]
    fn aligned_oamd_timing_conflict_and_count_mismatch_are_transactional() {
        let mut drive = FullAjocSubstreamState::new();
        drive.prepare_control_buffers();
        let element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let aspx = [AspxData::empty()];
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [AjocObjectControl::default()];
        let matrices = [AjocObjectMatrix::new()];
        let blocks = [oamd_block(0, 0)];
        let first_timing = oamd_timing(8, 1);
        let other_timing = oamd_timing(16, 1);
        let conflicting_provenance = FullAjocFrameProvenance::new(30)
            .try_with_group_oamd_state(timed_group_state(0, first_timing, true))
            .and_then(|value| {
                value.try_with_group_oamd_state(timed_group_state(1, other_timing, true))
            })
            .expect("两个递增 group 状态应进入 provenance");
        let conflict = QueuedQmfControl::capture(
            drive.control_buffers.pop().expect("应有预分配控制槽"),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(conflicting_provenance),
                    dmx_oamd_blocks: &blocks,
                    umx_oamd_blocks: &blocks,
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        let error = resolve_aligned_oamd(&mut drive, &conflict, 5)
            .expect_err("同一 presentation 的 group timing 冲突必须失败");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::OamdState);
        assert!(error.detail().contains("group 0"));
        assert!(error.detail().contains("group 1"));
        assert_eq!(drive.dmx_oamd, OamdState::new());
        assert_eq!(drive.umx_oamd, OamdState::new());
        assert_eq!(drive.dmx_oamd_timing, None);
        assert_eq!(drive.umx_oamd_timing, None);
        assert!(drive.aligned_dmx_oamd_updates.is_empty());
        assert!(drive.aligned_umx_oamd_updates.is_empty());

        let mismatched = oamd_timing(8, 2);
        let mismatch = QueuedQmfControl::capture(
            conflict.into_buffers(),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(FullAjocFrameProvenance::new(31)),
                    dmx_oamd_blocks: &blocks,
                    umx_oamd_blocks: &blocks,
                    dmx_oamd_timing: Some(mismatched),
                    umx_oamd_timing: Some(mismatched),
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        let error = resolve_aligned_oamd(&mut drive, &mismatch, 5)
            .expect_err("timing 块数与动态数据不一致必须失败");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::OamdState);
        assert!(error.detail().contains("declares 2 blocks"));
        assert_eq!(drive.dmx_oamd, OamdState::new());
        assert_eq!(drive.umx_oamd, OamdState::new());
        assert_eq!(drive.dmx_oamd_timing, None);
        assert_eq!(drive.umx_oamd_timing, None);
    }

    #[test]
    fn aligned_oamd_commits_core_and_full_as_one_transaction() {
        let mut drive = FullAjocSubstreamState::new();
        drive.prepare_control_buffers();
        let element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let aspx = [AspxData::empty()];
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [AjocObjectControl::default()];
        let matrices = [AjocObjectMatrix::new()];
        let initial = [oamd_block(0, 0)];
        let first = QueuedQmfControl::capture(
            drive.control_buffers.pop().expect("应有预分配控制槽"),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(FullAjocFrameProvenance::new(3)),
                    dmx_oamd_blocks: &initial,
                    umx_oamd_blocks: &initial,
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        resolve_aligned_oamd(&mut drive, &first, 0).expect("首帧应建立两侧历史");
        let dmx_before = drive.dmx_oamd;
        let umx_before = drive.umx_oamd;

        let valid_core = [reused_oamd_block(0, 0)];
        let invalid_full = [reused_oamd_block(1, 0)];
        let second = QueuedQmfControl::capture(
            first.into_buffers(),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(FullAjocFrameProvenance::new(4)),
                    dmx_oamd_blocks: &valid_core,
                    umx_oamd_blocks: &invalid_full,
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        let error = resolve_aligned_oamd(&mut drive, &second, 0)
            .expect_err("Full 侧缺历史时 Core 侧也不得提交");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::OamdState);
        assert!(error.detail().contains("Full/upmix"));
        assert_eq!(drive.dmx_oamd, dmx_before);
        assert_eq!(drive.umx_oamd, umx_before);
        assert!(drive.aligned_dmx_oamd_updates.is_empty());
        assert!(drive.aligned_umx_oamd_updates.is_empty());

        drive.reset();
        let error = resolve_aligned_oamd(&mut drive, &second, 0)
            .expect_err("reset 后不得继承此前建立的 Core 状态");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::OamdState);
        assert!(error.detail().contains("Core/downmix"));
    }

    #[test]
    fn metadata_blind_due_control_cuts_oamd_inheritance() {
        let mut drive = FullAjocSubstreamState::new();
        drive.prepare_control_buffers();
        let element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let aspx = [AspxData::empty()];
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [AjocObjectControl::default()];
        let matrices = [AjocObjectMatrix::new()];
        let initial = [oamd_block(0, 0)];
        let established = QueuedQmfControl::capture(
            drive.control_buffers.pop().expect("应有预分配控制槽"),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(FullAjocFrameProvenance::new(8)),
                    dmx_oamd_blocks: &initial,
                    umx_oamd_blocks: &initial,
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        resolve_due_oamd(&mut drive, Some(&established), 0).expect("完整入口应建立 OAMD 历史");
        assert_ne!(drive.dmx_oamd, OamdState::new());
        assert_ne!(drive.umx_oamd, OamdState::new());

        let blind = QueuedQmfControl::capture(
            established.into_buffers(),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation::empty(),
            ),
        );
        resolve_due_oamd(&mut drive, Some(&blind), 0)
            .expect("parsed-PCM 控制到期应只切断 metadata 历史");
        assert_eq!(drive.dmx_oamd, OamdState::new());
        assert_eq!(drive.umx_oamd, OamdState::new());

        let reused = [reused_oamd_block(0, 0)];
        let resumed = QueuedQmfControl::capture(
            blind.into_buffers(),
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation {
                    provenance: Some(FullAjocFrameProvenance::new(10)),
                    dmx_oamd_blocks: &reused,
                    umx_oamd_blocks: &reused,
                    dmx_num_obj_info_blocks: 1,
                    umx_num_obj_info_blocks: 1,
                    ..FullAjocInputSideInformation::empty()
                },
            ),
        );
        let error = resolve_due_oamd(&mut drive, Some(&resumed), 0)
            .expect_err("盲区后的 REUSE 不得继承盲区前状态");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::OamdState);
        assert!(error.detail().contains("Core/downmix"));
    }

    #[test]
    fn decode_mode_is_bound_until_substream_reset() {
        let mut drive = FullAjocSubstreamState::new();
        drive
            .bind_decode_mode(FullAjocDecodeMode::AspxOnly, 2)
            .expect("首次模式应绑定");
        drive
            .bind_decode_mode(FullAjocDecodeMode::AspxOnly, 2)
            .expect("同一模式应继续");

        let error = drive
            .bind_decode_mode(FullAjocDecodeMode::ObserveFull, 2)
            .expect_err("未经 reset 不得恢复未推进的 Full 状态");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::DecodeModeMismatch);
        assert!(error.detail().contains("reset is required"));

        drive.reset();
        drive
            .bind_decode_mode(FullAjocDecodeMode::ObserveFull, 2)
            .expect("reset 后应允许选择新模式");
    }

    #[test]
    fn full_capacity_terminal_topology_preserves_128_lanes_and_lfe() {
        for count in [24u32, 64, 128] {
            let topology = ObjectOutputTopology::checked(count, None, false, 0).unwrap();
            assert_eq!(topology.channels(), count as usize);
            for lane in 0..count as usize {
                assert_eq!(topology.source(lane), Some(FullAjocPcmSource::AjocObject(lane)));
            }
        }
        for lfe in [0, 64, 127] {
            let topology = ObjectOutputTopology::checked(127, Some(lfe), true, 0).unwrap();
            assert_eq!(topology.channels(), 128);
            assert_eq!(topology.source(lfe as usize), Some(FullAjocPcmSource::Lfe));
            assert_eq!(topology.source(128), None);
        }
        assert!(ObjectOutputTopology::checked(129, None, false, 0).is_err());
    }

    #[test]
    fn pseudocode_15_reinserts_lfe_at_zero_two_or_the_end() {
        let sources = |position| {
            let topology = ObjectOutputTopology::checked(3, position, position.is_some(), 2)
                .expect("位置应合法");
            (0..topology.channels())
                .map(|slot| topology.source(slot).expect("每路应有来源"))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            sources(Some(0)),
            vec![
                FullAjocPcmSource::Lfe,
                FullAjocPcmSource::AjocObject(0),
                FullAjocPcmSource::AjocObject(1),
                FullAjocPcmSource::AjocObject(2),
            ]
        );
        assert_eq!(
            sources(Some(2)),
            vec![
                FullAjocPcmSource::AjocObject(0),
                FullAjocPcmSource::AjocObject(1),
                FullAjocPcmSource::Lfe,
                FullAjocPcmSource::AjocObject(2),
            ]
        );
        assert_eq!(
            sources(Some(3)),
            vec![
                FullAjocPcmSource::AjocObject(0),
                FullAjocPcmSource::AjocObject(1),
                FullAjocPcmSource::AjocObject(2),
                FullAjocPcmSource::Lfe,
            ]
        );
        assert_eq!(
            sources(None),
            vec![
                FullAjocPcmSource::AjocObject(0),
                FullAjocPcmSource::AjocObject(1),
                FullAjocPcmSource::AjocObject(2),
            ],
            "无 LFE 时对象顺序必须原样保留"
        );
    }

    #[test]
    fn full_warmup_keeps_the_aligned_lfe_in_terminal_synthesis() {
        const TIMESLOTS: usize = 32;
        const SAMPLES: usize = TIMESLOTS * 64;

        let zero = empty_channel_frame();
        let mut lfe = empty_channel_frame();
        for (timeslot, slot) in lfe.iter_mut().take(TIMESLOTS).enumerate() {
            let value = (timeslot + 1) as f32;
            slot.re[0] = value;
            slot.im[1] = value * -0.25;
        }

        let mut expected_state = QmfSynthesisState::new();
        let mut expected_lfe = vec![0.0; SAMPLES];
        synthesise_ac4_pcm(&lfe[..TIMESLOTS], &mut expected_state, &mut expected_lfe)
            .expect("测试 LFE QMF 应可合成");
        assert!(
            expected_lfe.iter().any(|sample| *sample != 0.0),
            "测试输入必须能区分真实 LFE 与零 QMF"
        );

        let mut drive = FullAjocSubstreamState::new();
        let mut sources = Vec::new();
        let mut pcm = Vec::new();
        prime_full_output(
            &mut drive,
            &mut sources,
            &mut pcm,
            &zero,
            &lfe,
            1,
            Some(0),
            true,
            SAMPLES,
            TIMESLOTS,
            2,
        )
        .expect("full 预热应成功");

        assert_eq!(
            sources,
            vec![FullAjocPcmSource::Lfe, FullAjocPcmSource::AjocObject(0)]
        );
        assert_eq!(
            pcm.first(),
            Some(&expected_lfe),
            "LFE 预热必须送入真实对齐 QMF"
        );
        assert!(
            pcm.get(1)
                .is_some_and(|channel| channel.iter().all(|sample| *sample == 0.0)),
            "对象预热仍必须使用零 QMF"
        );
        assert_eq!(
            drive.object_synthesis.first(),
            Some(&expected_state),
            "LFE 终端合成历史必须与输出一同预热"
        );
    }

    #[test]
    fn current_blocker_does_not_reset_an_older_pending_full_warmup() {
        const TIMESLOTS: usize = 1;
        const SAMPLES: usize = TIMESLOTS * 64;

        let element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let aspx = [AspxData::empty()];
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [AjocObjectControl::default()];
        let matrices = [AjocObjectMatrix::new()];
        let mut drive = FullAjocSubstreamState::new();
        drive.prepare_control_buffers();
        let buffer = drive.control_buffers.pop().expect("应有预分配控制槽");
        drive.controls.push_back(QueuedQmfControl::capture(
            buffer,
            control_snapshot(
                &element,
                &aspx,
                &ajoc,
                &controls,
                &matrices,
                FullAjocInputSideInformation::empty(),
            ),
        ));

        let blocker = Err(FullAjocBlocker::ActiveDialogueEnhancement {
            dialogue_objects: 1,
        });
        let mut sources = Vec::new();
        let mut pcm = Vec::new();
        let zero = empty_channel_frame();
        let lfe = empty_channel_frame();
        let observation = prime_pending_full_output(
            &mut drive,
            &mut sources,
            &mut pcm,
            &zero,
            &lfe,
            blocker,
            &ajoc,
            None,
            &element,
            FullAjocDecodeMode::ObserveFull,
            SAMPLES,
            TIMESLOTS,
            2,
        )
        .expect("较早受支持控制应继续预热");
        assert!(observation.warmup());
        assert_eq!(drive.object_synthesis.len(), 1);
        assert_eq!(sources, vec![FullAjocPcmSource::AjocObject(0)]);

        drive.controls.clear();
        let observation = prime_pending_full_output(
            &mut drive,
            &mut sources,
            &mut pcm,
            &zero,
            &lfe,
            blocker,
            &ajoc,
            None,
            &element,
            FullAjocDecodeMode::ObserveFull,
            SAMPLES,
            TIMESLOTS,
            2,
        )
        .expect("当前 blocker 在没有较早控制时应形成 Full 缺口");
        assert!(!observation.warmup());
        assert!(drive.object_synthesis.is_empty());
        assert!(sources.is_empty());
    }

    #[test]
    fn lfe_position_and_output_topology_fail_closed_until_reset() {
        assert!(
            ObjectOutputTopology::checked(3, Some(4), true, 2)
                .expect_err("越过对象尾部必须失败")
                .contains("exceeds 3 A-JOC objects")
        );
        assert!(
            ObjectOutputTopology::checked(3, None, true, 2)
                .expect_err("元素与插回控制不一致必须失败")
                .contains("element LFE=true")
        );

        let mut drive = FullAjocSubstreamState::new();
        let first = ObjectOutputTopology::checked(3, Some(2), true, 2).expect("合法拓扑");
        install_output_topology(&mut drive, first, 2).expect("首次拓扑应提交");
        assert_eq!(drive.object_synthesis.len(), 4);
        let changed = ObjectOutputTopology::checked(3, Some(3), true, 2).expect("合法新拓扑");
        let error =
            install_output_topology(&mut drive, changed, 2).expect_err("无 reset 改拓扑必须失败");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::ObjectShapeMismatch);
        assert!(error.detail().contains("reset is required"));

        drive.fresh = false;
        drive.input_topology = Some(QmfInputTopology {
            fullband: 5,
            lfe: true,
        });
        drive.reset_full();
        assert!(drive.output_topology.is_none());
        assert!(drive.object_synthesis.is_empty());
        assert!(
            !drive.fresh,
            "诊断 A-SPX 状态不应被 opportunistic full 跳过清除"
        );
        assert!(drive.input_topology.is_some());

        drive.reset();
        assert!(
            drive.is_fresh(),
            "substream reset 必须清除 A-SPX/A-JOC/对象合成全部历史"
        );
    }

    #[test]
    fn nonfinite_object_synthesis_is_rejected_before_any_frame_can_be_committed() {
        let topology = ObjectOutputTopology::checked(1, None, false, 2).expect("合法拓扑");
        let mut drive = FullAjocSubstreamState::new();
        install_output_topology(&mut drive, topology, 2).expect("首次拓扑应提交");
        let mut qmf = empty_channel_frame();
        qmf[0].re[0] = f32::INFINITY;
        let mut sources = Vec::new();
        let mut pcm = Vec::new();

        let error = synthesise_object_outputs(
            &mut drive,
            topology,
            core::slice::from_ref(&qmf),
            &empty_channel_frame(),
            false,
            &mut sources,
            &mut pcm,
            64,
            1,
            2,
        )
        .expect_err("非有限对象 PCM 必须失败");
        assert_eq!(error.kind(), FullAjocDecodeErrorKind::ObjectsNonFinite);
        assert!(
            error
                .detail()
                .contains("object PCM for channel 0 is non-finite at sample"),
            "{error}"
        );
        assert!(sources.is_empty(), "失败帧不得形成可提交的来源列表");

        drive.reset();
        assert!(
            drive.is_fresh(),
            "数值失败后的 substream reset 必须清空全部状态"
        );
    }

    #[test]
    fn decoder_reset_isolates_one_substream_then_clears_all_history() {
        let mut decoder = FullAjocDecoder::new();
        decoder
            .substreams
            .resize_with(3, FullAjocSubstreamState::new);
        for index in [0, 2] {
            decoder
                .substreams
                .get_mut(index)
                .expect("测试刚建立三条 substream")
                .mark_used_for_test();
        }

        decoder.reset_substream(0);
        assert!(
            decoder
                .substreams
                .first()
                .is_some_and(FullAjocSubstreamState::is_fresh)
        );
        assert!(
            decoder
                .substreams
                .get(1)
                .is_some_and(FullAjocSubstreamState::is_fresh)
        );
        assert!(
            decoder
                .substreams
                .get(2)
                .is_some_and(|state| !state.is_fresh()),
            "单 substream reset 不得清除其他物理流历史"
        );

        decoder.reset();
        assert!(
            decoder
                .substreams
                .iter()
                .all(FullAjocSubstreamState::is_fresh)
        );
    }
}
