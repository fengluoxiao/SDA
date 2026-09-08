//! 场景 API 的所有权无关数据模型。

use alloc::vec::Vec;
use core::iter::FusedIterator;
use macindecode_ac4_bitstream::{
    Ac4PresentationSubstream, PresentationDrcConfiguration, PresentationDrcState,
    PresentationSubstreamContext, PresentationSubstreamError, PresentationSubstreamGroupGainCodes,
    oamd::{
        AdditionalObjectMetadata, OamdCommonData, OamdMetadataBlock, ObjInfoBlockTiming,
        ObjectMetadataState, SampleOffsetSource,
    },
};

/// 配置代次内稳定的场景元素标识。
///
/// 标识在同一配置代次内保持稳定；配置变化后分配新值，且同一会话内不复用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneElementId(u64);

impl SceneElementId {
    #[cfg(feature = "audio-decode")]
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    /// 返回数值表示。它只适合日志、映射表和序列化，不表达创作对象 identity。
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Session 的 presentation 选择策略。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresentationSelection {
    /// 只有一个可解码 presentation 时自动选择；多于一个时返回歧义错误。
    #[default]
    AutoUnique,
    /// 按零基 presentation 下标选择。
    Index(u32),
    /// 按码流声明的 `presentation_id` 选择。
    Id(u32),
}

/// 调用方 metadata 中 presentation 身份的可用状态。
///
/// 未知版本的 opaque body 不能把“尚未解析 ID”伪装成“明确没有 ID”；适配层应使用
/// [`PresentationSelectionMetadataIdentity::Unavailable`] 保持两者区别。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationSelectionMetadataIdentity {
    /// 已经应用扩展覆盖规则的 effective presentation ID。
    EffectiveId(u32),
    /// 已知语法明确没有 presentation ID。
    WithoutId,
    /// opaque 或未解释的语法无法确定是否存在 presentation ID。
    Unavailable,
}

/// 调用方提供的一项 presentation 选择 metadata。
///
/// Scene 只使用 effective presentation ID 把本项与已经由 TOC 选中的
/// [`ScenePresentation`] 关联，绝不使用 `source_index` 配置解码器或按数组顺序猜测
/// 身份。`T` 由适配层决定，可以直接保存 MP4 DSI 的只读借用视图；未知 presentation
/// 版本因此仍能保留已经按声明长度定界的原始 body，而不要求 Scene 理解其语法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationSelectionMetadata<T> {
    source_index: u32,
    identity: PresentationSelectionMetadataIdentity,
    version: u32,
    declared_bytes: u32,
    value: T,
}

impl<T> PresentationSelectionMetadata<T> {
    /// 创建一项调用方拥有的 presentation metadata。
    ///
    /// `identity` 中的 ID 应已经应用系统层定义的扩展覆盖规则；未知版本不得用
    /// `WithoutId` 代替 `Unavailable`。`declared_bytes` 是外层 envelope 验证后的 body
    /// 长度。Scene 不重新解释这些值。
    #[must_use]
    pub const fn new(
        source_index: u32,
        identity: PresentationSelectionMetadataIdentity,
        version: u32,
        declared_bytes: u32,
        value: T,
    ) -> Self {
        Self {
            source_index,
            identity,
            version,
            declared_bytes,
            value,
        }
    }

    /// metadata 来源数组中的下标，仅供检视，不参与身份关联。
    #[must_use]
    pub const fn source_index(&self) -> u32 {
        self.source_index
    }

    /// metadata 身份是否已知，以及已知时的 effective ID。
    #[must_use]
    pub const fn identity(&self) -> PresentationSelectionMetadataIdentity {
        self.identity
    }

    /// 已应用扩展覆盖规则的 presentation ID；明确无 ID 或身份不可用时为 `None`。
    ///
    /// 需要区分后两种状态时使用 [`PresentationSelectionMetadata::identity`]。
    #[must_use]
    pub const fn effective_presentation_id(&self) -> Option<u32> {
        match self.identity {
            PresentationSelectionMetadataIdentity::EffectiveId(value) => Some(value),
            PresentationSelectionMetadataIdentity::WithoutId
            | PresentationSelectionMetadataIdentity::Unavailable => None,
        }
    }

    /// presentation metadata envelope 的语法版本。
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// envelope 声明并已由适配层验证的 body 字节数。
    #[must_use]
    pub const fn declared_bytes(&self) -> u32 {
        self.declared_bytes
    }

    /// 调用方定义的只读 metadata 值。
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// 取回调用方定义的 metadata 值。
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }
}

/// Scene presentation 与调用方 metadata 的稳定关联依据。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationSelectionMetadataMatchBasis {
    /// 双方各自唯一的 effective presentation ID。
    EffectivePresentationId,
    /// 双方各自恰有一个无 ID presentation 时的唯一回退。
    SingleWithoutId,
}

/// 已选择 Scene presentation 与调用方 metadata 的只读关联结果。
///
/// `Missing`、`Ambiguous` 与 `Indeterminate` 都不会改变 Session 的 presentation 选择或
/// 解码状态；调用方可以把它们用于 UI 能力检视、日志或容器一致性门禁。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationSelectionMetadataMatch<'a, T> {
    /// 已按稳定身份找到唯一 metadata。
    Matched {
        /// 调用方原始 entry 的只读视图。
        metadata: &'a PresentationSelectionMetadata<T>,
        /// 本次关联采用的身份依据。
        basis: PresentationSelectionMetadataMatchBasis,
    },
    /// metadata 集合中没有相同 effective ID 的候选。
    Missing {
        /// 当前 Scene presentation 的 effective ID。
        effective_presentation_id: Option<u32>,
    },
    /// TOC 或 metadata 一侧的相同身份不唯一，不能安全关联。
    Ambiguous {
        /// 发生歧义的 effective ID；`None` 表示多路无 ID。
        effective_presentation_id: Option<u32>,
        /// 当前 TOC 中具有相同身份的 presentation 数量。
        scene_candidates: u32,
        /// 调用方 metadata 中具有相同身份的 entry 数量。
        metadata_candidates: usize,
    },
    /// metadata 集合含身份不可用项，无法证明已知候选唯一或确实缺失。
    Indeterminate {
        /// 当前 Scene presentation 的 effective ID。
        effective_presentation_id: Option<u32>,
        /// 调用方 metadata 中具有相同已知身份的 entry 数量。
        known_metadata_candidates: usize,
        /// 调用方 metadata 中身份不可用的 entry 数量。
        unavailable_metadata: usize,
    },
}

/// 解码会话配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ac4DecoderConfig {
    presentation: PresentationSelection,
    decode_mode: DecodeMode,
    core_band_diagnostics: bool,
    pub(crate) opaque_presentation_tail: bool,
}

impl Ac4DecoderConfig {
    /// 创建 A-JOC 解码配置；默认重建 Full 场景。
    #[must_use]
    pub const fn new(presentation: PresentationSelection) -> Self {
        Self {
            presentation,
            decode_mode: DecodeMode::Full,
            core_band_diagnostics: false,
            opaque_presentation_tail: false,
        }
    }

    /// 选择 A-JOC Core/downmix 或 Full/upmix 场景重建层级。
    #[must_use]
    pub const fn with_decode_mode(mut self, decode_mode: DecodeMode) -> Self {
        self.decode_mode = decode_mode;
        self
    }

    /// Accept an opaque single-byte independent object/DRC tail after strict syntax.
    #[must_use]
    pub const fn with_opaque_presentation_tail(mut self, enabled: bool) -> Self {
        self.opaque_presentation_tail = enabled;
        self
    }

    /// 是否为每个成功 AU 额外生成 pre-A-SPX 核心带诊断 PCM。
    ///
    /// 默认关闭，避免普通 renderer 为不消费的传输侧诊断信号分配和复制缓冲。
    #[must_use]
    pub const fn with_core_band_diagnostics(mut self, enabled: bool) -> Self {
        self.core_band_diagnostics = enabled;
        self
    }

    /// 当前 presentation 选择策略。
    #[must_use]
    pub const fn presentation(&self) -> PresentationSelection {
        self.presentation
    }

    /// 当前场景重建层级。
    #[must_use]
    pub const fn decode_mode(&self) -> DecodeMode {
        self.decode_mode
    }

    /// 是否生成 pre-A-SPX 核心带诊断 PCM。
    #[must_use]
    pub const fn core_band_diagnostics(&self) -> bool {
        self.core_band_diagnostics
    }
}

impl Default for Ac4DecoderConfig {
    fn default() -> Self {
        Self::new(PresentationSelection::AutoUnique)
    }
}

/// 调用方提供的通用外部采样时间。
///
/// Scene 层只透传这些值，不解释它们来自哪一种容器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessUnitContext {
    index: u64,
    source_sample_start: Option<i64>,
    presentation_sample_start: Option<i64>,
    priming_samples: Option<u64>,
    random_access_hint: Option<bool>,
    discontinuity: bool,
}

impl AccessUnitContext {
    /// 为一个已定界的 access unit 创建上下文。
    #[must_use]
    pub const fn new(index: u64) -> Self {
        Self {
            index,
            source_sample_start: None,
            presentation_sample_start: None,
            priming_samples: None,
            random_access_hint: None,
            discontinuity: false,
        }
    }

    /// 设置调用方媒体时间轴中的起始采样。
    #[must_use]
    pub const fn with_source_sample_start(mut self, value: i64) -> Self {
        self.source_sample_start = Some(value);
        self
    }

    /// 设置调用方应用 edit 后的呈现起始采样。
    #[must_use]
    pub const fn with_presentation_sample_start(mut self, value: i64) -> Self {
        self.presentation_sample_start = Some(value);
        self
    }

    /// 设置容器单独声明的 priming，不把它混入其他起点字段。
    #[must_use]
    pub const fn with_priming_samples(mut self, value: u64) -> Self {
        self.priming_samples = Some(value);
        self
    }

    /// 设置容器或传输层的随机访问提示。
    #[must_use]
    pub const fn with_random_access_hint(mut self, value: bool) -> Self {
        self.random_access_hint = Some(value);
        self
    }

    /// 标记 access unit 前存在外部不连续。
    #[must_use]
    pub const fn with_discontinuity(mut self, value: bool) -> Self {
        self.discontinuity = value;
        self
    }

    /// 原始 access unit 下标。
    #[must_use]
    pub const fn index(&self) -> u64 {
        self.index
    }

    /// 调用方媒体时间轴中的起始采样。
    #[must_use]
    pub const fn source_sample_start(&self) -> Option<i64> {
        self.source_sample_start
    }

    /// 应用外部 edit 后的呈现起始采样。
    #[must_use]
    pub const fn presentation_sample_start(&self) -> Option<i64> {
        self.presentation_sample_start
    }

    /// 容器 priming 采样数。
    #[must_use]
    pub const fn priming_samples(&self) -> Option<u64> {
        self.priming_samples
    }

    /// 容器或传输层的随机访问提示。
    #[must_use]
    pub const fn random_access_hint(&self) -> Option<bool> {
        self.random_access_hint
    }

    /// access unit 前是否存在外部不连续。
    #[must_use]
    pub const fn discontinuity(&self) -> bool {
        self.discontinuity
    }
}

impl Default for AccessUnitContext {
    fn default() -> Self {
        Self::new(0)
    }
}

/// 一个已经剥离 sync wrapper、由调用方定界的 `raw_ac4_frame`。
#[derive(Debug, Clone, Copy)]
pub struct AccessUnit<'a> {
    raw_frame: &'a [u8],
    context: AccessUnitContext,
}

impl<'a> AccessUnit<'a> {
    /// 创建 access unit；本构造器不复制输入切片。
    #[must_use]
    pub const fn new(raw_frame: &'a [u8], context: AccessUnitContext) -> Self {
        Self { raw_frame, context }
    }

    /// 完整 `raw_ac4_frame` 字节。
    #[must_use]
    pub const fn raw_frame(&self) -> &'a [u8] {
        self.raw_frame
    }

    /// 调用方提供的时间与连续性上下文。
    #[must_use]
    pub const fn context(&self) -> AccessUnitContext {
        self.context
    }
}

/// 场景编码路径。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenePath {
    ChannelBased,
    DirectObject,
    Ajoc,
    Mixed,
}

/// 场景重建层级。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeMode {
    /// A-SPX 后、A-JOC 上混前的 Core/downmix 对象场景。
    Core,
    /// A-JOC 上混后的 Full/upmix 对象场景。
    Full,
}

/// 规范解释后的 codec delay。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecDelay {
    ConstantBitRate,
    Frames(u8),
    VariableBitRate,
}

/// 解码状态被清空的原因。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetKind {
    Initial,
    SourceChange,
    ConfigurationChange,
    ParseFailure,
    ExternalDiscontinuity,
}

/// 当前 access unit 的处理状态。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeStatus {
    Decoded,
    WaitingForRandomAccess { reason: ResetKind },
}

/// 单个场景帧的整数采样时间轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneTimeline {
    pub(crate) sample_rate: u32,
    pub(crate) codec_sample_start: i64,
    pub(crate) source_sample_start: Option<i64>,
    pub(crate) presentation_sample_start: Option<i64>,
    pub(crate) duration_samples: u32,
    pub(crate) access_unit_index: u64,
    pub(crate) control_source_access_unit_index: Option<u64>,
    pub(crate) random_access: bool,
    pub(crate) configuration_generation: u32,
    pub(crate) priming_samples: Option<u64>,
    pub(crate) codec_delay: Option<CodecDelay>,
    pub(crate) pcm_alignment_delay_samples: u16,
    pub(crate) control_alignment_delay_frames: u8,
}

impl SceneTimeline {
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub const fn codec_sample_start(&self) -> i64 {
        self.codec_sample_start
    }

    #[must_use]
    pub const fn source_sample_start(&self) -> Option<i64> {
        self.source_sample_start
    }

    #[must_use]
    pub const fn presentation_sample_start(&self) -> Option<i64> {
        self.presentation_sample_start
    }

    #[must_use]
    pub const fn duration_samples(&self) -> u32 {
        self.duration_samples
    }

    #[must_use]
    pub const fn access_unit_index(&self) -> u64 {
        self.access_unit_index
    }

    #[must_use]
    pub const fn control_source_access_unit_index(&self) -> Option<u64> {
        self.control_source_access_unit_index
    }

    /// 当前场景帧是否来自完整随机访问点。
    #[must_use]
    pub const fn random_access(&self) -> bool {
        self.random_access
    }

    /// 当前解码拓扑的配置代次。
    #[must_use]
    pub const fn configuration_generation(&self) -> u32 {
        self.configuration_generation
    }

    #[must_use]
    pub const fn priming_samples(&self) -> Option<u64> {
        self.priming_samples
    }

    #[must_use]
    pub const fn codec_delay(&self) -> Option<CodecDelay> {
        self.codec_delay
    }

    #[must_use]
    pub const fn pcm_alignment_delay_samples(&self) -> u16 {
        self.pcm_alignment_delay_samples
    }

    #[must_use]
    pub const fn control_alignment_delay_frames(&self) -> u8 {
        self.control_alignment_delay_frames
    }
}

/// 当前 SceneFrame 的 presentation 来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePresentation {
    pub(crate) index: u32,
    pub(crate) id: Option<u32>,
    pub(crate) identity_occurrences: u32,
    pub(crate) version: u32,
    pub(crate) md_compat: Option<u8>,
    pub(crate) group_indices: Vec<u32>,
    pub(crate) substream_indices: Vec<u32>,
    pub(crate) path: ScenePath,
    pub(crate) mode: DecodeMode,
}

impl ScenePresentation {
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    #[must_use]
    pub const fn id(&self) -> Option<u32> {
        self.id
    }

    /// 当前 TOC 中具有相同 effective presentation ID 的 presentation 数量。
    ///
    /// 无 ID presentation 以 `None` 作为同一类计数。值大于一时，即使 Session 是按
    /// 显式下标选择，也不能把外部 metadata 猜测性绑定到其中一路。
    #[must_use]
    pub const fn identity_occurrences(&self) -> u32 {
        self.identity_occurrences
    }

    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub const fn md_compat(&self) -> Option<u8> {
        self.md_compat
    }

    #[must_use]
    pub fn group_indices(&self) -> &[u32] {
        &self.group_indices
    }

    #[must_use]
    pub fn substream_indices(&self) -> &[u32] {
        &self.substream_indices
    }

    #[must_use]
    pub const fn path(&self) -> ScenePath {
        self.path
    }

    #[must_use]
    pub const fn mode(&self) -> DecodeMode {
        self.mode
    }

    /// 将已经选中的 TOC presentation 与调用方 metadata 严格关联。
    ///
    /// 有 ID 时要求双方该 ID 都唯一；无 ID 时要求双方各自恰有一个无 ID 项。来源数组
    /// 下标不参与关联，重复 ID 或多路无 ID 返回
    /// [`PresentationSelectionMetadataMatch::Ambiguous`]；metadata 中仍有身份不可用项时，
    /// 无法证明候选唯一，返回 [`PresentationSelectionMetadataMatch::Indeterminate`]。
    #[must_use]
    pub fn match_selection_metadata<'a, T>(
        &self,
        metadata: &'a [PresentationSelectionMetadata<T>],
    ) -> PresentationSelectionMetadataMatch<'a, T> {
        let expected_identity = match self.id {
            Some(value) => PresentationSelectionMetadataIdentity::EffectiveId(value),
            None => PresentationSelectionMetadataIdentity::WithoutId,
        };
        let mut matched = None;
        let mut metadata_candidates = 0usize;
        let mut unavailable_metadata = 0usize;
        for entry in metadata {
            if entry.identity() == expected_identity {
                metadata_candidates = metadata_candidates.saturating_add(1);
                if matched.is_none() {
                    matched = Some(entry);
                }
            } else if entry.identity() == PresentationSelectionMetadataIdentity::Unavailable {
                unavailable_metadata = unavailable_metadata.saturating_add(1);
            }
        }

        if self.identity_occurrences != 1 || metadata_candidates > 1 {
            return PresentationSelectionMetadataMatch::Ambiguous {
                effective_presentation_id: self.id,
                scene_candidates: self.identity_occurrences,
                metadata_candidates,
            };
        }
        if unavailable_metadata > 0 {
            return PresentationSelectionMetadataMatch::Indeterminate {
                effective_presentation_id: self.id,
                known_metadata_candidates: metadata_candidates,
                unavailable_metadata,
            };
        }
        let Some(metadata) = matched else {
            return PresentationSelectionMetadataMatch::Missing {
                effective_presentation_id: self.id,
            };
        };
        let basis = if self.id.is_some() {
            PresentationSelectionMetadataMatchBasis::EffectivePresentationId
        } else {
            PresentationSelectionMetadataMatchBasis::SingleWithoutId
        };
        PresentationSelectionMetadataMatch::Matched { metadata, basis }
    }
}

/// PCM 样本格式。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmSampleFormat {
    F32,
}

/// PCM 布局。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmLayout {
    Planar,
}

/// 一路由 Session 拥有的 PCM plane。
#[derive(Debug, Default)]
pub struct PcmPlane {
    pub(crate) samples: Vec<f32>,
}

#[cfg(any(feature = "audio-decode", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NonFinitePcm {
    pub(crate) sample_index: usize,
}

impl PcmPlane {
    #[cfg(any(feature = "audio-decode", test))]
    pub(crate) fn with_capacity(samples: usize) -> Self {
        Self {
            samples: Vec::with_capacity(samples),
        }
    }

    /// 把 decoder 内部 `±32768` 标量精确映射到 normalized Scene PCM。
    ///
    /// 先验证整路输入，再改写可见长度；失败不会留下半路归一化的 plane。
    #[cfg(any(feature = "audio-decode", test))]
    pub(crate) fn copy_normalized_from(&mut self, source: &[f32]) -> Result<bool, NonFinitePcm> {
        if let Some(sample_index) = source.iter().position(|sample| !sample.is_finite()) {
            return Err(NonFinitePcm { sample_index });
        }
        const INTERNAL_TO_NORMALIZED: f32 = f32::from_bits(0x3800_0000);
        self.samples.clear();
        self.samples
            .extend(source.iter().map(|sample| *sample * INTERNAL_TO_NORMALIZED));
        Ok(self.samples.iter().any(|sample| *sample != 0.0))
    }

    /// 当前帧的有效样本。
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// 相邻有效样本的元素步长。首版 planar f32 恒为 1。
    #[must_use]
    pub const fn stride(&self) -> usize {
        1
    }
}

/// 一组 normalized planar PCM 的借用视图。
#[derive(Debug, Clone, Copy)]
pub struct PlanarPcm<'a> {
    planes: &'a [PcmPlane],
    samples_per_plane: usize,
}

impl<'a> PlanarPcm<'a> {
    #[must_use]
    pub const fn sample_format(&self) -> PcmSampleFormat {
        PcmSampleFormat::F32
    }

    #[must_use]
    pub const fn layout(&self) -> PcmLayout {
        PcmLayout::Planar
    }

    /// nominal full scale；样本可以合法超过此值。
    #[must_use]
    pub const fn nominal_full_scale(&self) -> f32 {
        1.0
    }

    #[must_use]
    pub const fn planes(&self) -> &'a [PcmPlane] {
        self.planes
    }

    #[must_use]
    pub const fn samples_per_plane(&self) -> usize {
        self.samples_per_plane
    }
}

/// 元数据中的 Cartesian 位置，三轴规范化到 `[-1, 1]`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CartesianPosition {
    x: f32,
    y: f32,
    z: f32,
}

impl CartesianPosition {
    pub(crate) const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    #[must_use]
    pub const fn x(&self) -> f32 {
        self.x
    }

    #[must_use]
    pub const fn y(&self) -> f32 {
        self.y
    }

    #[must_use]
    pub const fn z(&self) -> f32 {
        self.z
    }
}

/// 对象尺寸。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ObjectExtent {
    Uniform(f32),
    Cartesian { x: f32, y: f32, z: f32 },
}

/// 离散渲染区域约束。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneState {
    snap: bool,
    elevation: bool,
    mask: u8,
}

impl ZoneState {
    pub(crate) const fn new(snap: bool, elevation: bool, mask: u8) -> Self {
        Self {
            snap,
            elevation,
            mask,
        }
    }

    #[must_use]
    pub const fn snap(&self) -> bool {
        self.snap
    }

    #[must_use]
    pub const fn elevation(&self) -> bool {
        self.elevation
    }

    #[must_use]
    pub const fn mask(&self) -> u8 {
        self.mask
    }
}

/// 耳机渲染模式。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadphoneMode {
    Off,
    Near,
    Far,
    Mid,
    Reserved(u8),
}

/// 单对象耳机元数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadphoneState {
    mode: HeadphoneMode,
    head_tracking_disabled: bool,
}

impl HeadphoneState {
    pub(crate) const fn new(mode: HeadphoneMode, head_tracking_disabled: bool) -> Self {
        Self {
            mode,
            head_tracking_disabled,
        }
    }

    #[must_use]
    pub const fn mode(&self) -> HeadphoneMode {
        self.mode
    }

    #[must_use]
    pub const fn head_tracking_disabled(&self) -> bool {
        self.head_tracking_disabled
    }
}

/// presentation 所引用 group 的原始 OAMD common 状态。
///
/// 同一帧中的条目按 `group_index` 排序。`updated_in_source_access_unit` 为假时，
/// `effective` 来自前一有效更新；reset 后尚未取得自足状态时为 `None`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawOamdCommonState {
    group_index: u32,
    effective: Option<OamdCommonData>,
    updated_in_source_access_unit: bool,
}

impl RawOamdCommonState {
    #[cfg(feature = "audio-decode")]
    pub(crate) const fn new(
        group_index: u32,
        effective: Option<OamdCommonData>,
        updated_in_source_access_unit: bool,
    ) -> Self {
        Self {
            group_index,
            effective,
            updated_in_source_access_unit,
        }
    }

    /// presentation 中的零基 group 下标。
    #[must_use]
    pub const fn group_index(&self) -> u32 {
        self.group_index
    }

    /// 已合并复用后的有效 common 数据及其原始码值。
    #[must_use]
    pub const fn effective(&self) -> Option<OamdCommonData> {
        self.effective
    }

    /// 关联该状态的来源 access unit 是否显式刷新了 common 数据。
    #[must_use]
    pub const fn updated_in_source_access_unit(&self) -> bool {
        self.updated_in_source_access_unit
    }
}

/// 已合并复用后的原始逐对象 OAMD 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawOamdState {
    effective: ObjectMetadataState,
    additional: AdditionalObjectMetadata,
}

impl RawOamdState {
    pub(crate) const fn new(
        effective: ObjectMetadataState,
        additional: AdditionalObjectMetadata,
    ) -> Self {
        Self {
            effective,
            additional,
        }
    }

    #[must_use]
    pub const fn effective(&self) -> ObjectMetadataState {
        self.effective
    }

    #[must_use]
    pub const fn additional(&self) -> AdditionalObjectMetadata {
        self.additional
    }
}

/// 当前对象块生效的原始 OAMD timing 状态。
///
/// timing 可以由当前 access unit 刷新，也可以从前一帧继承。这里保留
/// `sample_offset` 的编码方式、块数量以及当前块的 offset/ramp 原始编码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawOamdTiming {
    offset_source: SampleOffsetSource,
    sample_offset: u16,
    num_obj_info_blocks: u8,
    block: ObjInfoBlockTiming,
    updated_in_source_access_unit: bool,
}

impl RawOamdTiming {
    #[cfg_attr(
        not(any(test, feature = "audio-decode")),
        expect(dead_code, reason = "构造器只供 audio-decode 场景组装使用")
    )]
    pub(crate) const fn new(
        offset_source: SampleOffsetSource,
        sample_offset: u16,
        num_obj_info_blocks: u8,
        block: ObjInfoBlockTiming,
        updated_in_source_access_unit: bool,
    ) -> Self {
        Self {
            offset_source,
            sample_offset,
            num_obj_info_blocks,
            block,
            updated_in_source_access_unit,
        }
    }

    #[must_use]
    pub const fn offset_source(&self) -> SampleOffsetSource {
        self.offset_source
    }

    #[must_use]
    pub const fn sample_offset(&self) -> u16 {
        self.sample_offset
    }

    #[must_use]
    pub const fn num_obj_info_blocks(&self) -> u8 {
        self.num_obj_info_blocks
    }

    #[must_use]
    pub const fn block(&self) -> ObjInfoBlockTiming {
        self.block
    }

    /// 关联该更新的 control source access unit 是否显式刷新了 OAMD timing。
    #[must_use]
    pub const fn updated_in_source_access_unit(&self) -> bool {
        self.updated_in_source_access_unit
    }
}

/// 当前对象块及其有效 timing 的原始 OAMD 码值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawOamdUpdate {
    block: OamdMetadataBlock,
    timing: RawOamdTiming,
    /// 产生本更新的表 188 control source access unit。
    control_source_access_unit_index: u64,
}

impl RawOamdUpdate {
    #[cfg_attr(
        not(any(test, feature = "audio-decode")),
        expect(dead_code, reason = "构造器只供 audio-decode 场景组装使用")
    )]
    pub(crate) const fn new(
        block: OamdMetadataBlock,
        timing: RawOamdTiming,
        control_source_access_unit_index: u64,
    ) -> Self {
        Self {
            block,
            timing,
            control_source_access_unit_index,
        }
    }

    #[must_use]
    pub const fn block(&self) -> OamdMetadataBlock {
        self.block
    }

    #[must_use]
    pub const fn timing(&self) -> RawOamdTiming {
        self.timing
    }

    /// 产生本更新的 control source access unit。
    #[must_use]
    pub const fn control_source_access_unit_index(&self) -> u64 {
        self.control_source_access_unit_index
    }
}

/// renderer 友好的完整对象状态，并列保留原始量化值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneObjectState {
    metadata_active: bool,
    position: Option<CartesianPosition>,
    linear_gain: Option<f32>,
    importance: Option<f32>,
    extent: Option<ObjectExtent>,
    zone: Option<ZoneState>,
    screen_factor: Option<f32>,
    depth_factor: Option<f32>,
    trim_disabled: bool,
    headphone: Option<HeadphoneState>,
    semantic_complete: bool,
    raw: RawOamdState,
}

/// Scene 内部完成 OAMD 语义换算后的一组对象状态字段。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SceneObjectStateParts {
    pub(crate) metadata_active: bool,
    pub(crate) position: Option<CartesianPosition>,
    pub(crate) linear_gain: Option<f32>,
    pub(crate) importance: Option<f32>,
    pub(crate) extent: Option<ObjectExtent>,
    pub(crate) zone: Option<ZoneState>,
    pub(crate) screen_factor: Option<f32>,
    pub(crate) depth_factor: Option<f32>,
    pub(crate) trim_disabled: bool,
    pub(crate) headphone: Option<HeadphoneState>,
    pub(crate) semantic_complete: bool,
    pub(crate) raw: RawOamdState,
}

impl SceneObjectState {
    pub(crate) const fn from_parts(parts: SceneObjectStateParts) -> Self {
        Self {
            metadata_active: parts.metadata_active,
            position: parts.position,
            linear_gain: parts.linear_gain,
            importance: parts.importance,
            extent: parts.extent,
            zone: parts.zone,
            screen_factor: parts.screen_factor,
            depth_factor: parts.depth_factor,
            trim_disabled: parts.trim_disabled,
            headphone: parts.headphone,
            semantic_complete: parts.semantic_complete,
            raw: parts.raw,
        }
    }

    #[must_use]
    pub const fn metadata_active(&self) -> bool {
        self.metadata_active
    }

    #[must_use]
    pub const fn position(&self) -> Option<CartesianPosition> {
        self.position
    }

    #[must_use]
    pub const fn linear_gain(&self) -> Option<f32> {
        self.linear_gain
    }

    #[must_use]
    pub const fn importance(&self) -> Option<f32> {
        self.importance
    }

    #[must_use]
    pub const fn extent(&self) -> Option<ObjectExtent> {
        self.extent
    }

    #[must_use]
    pub const fn zone(&self) -> Option<ZoneState> {
        self.zone
    }

    #[must_use]
    pub const fn screen_factor(&self) -> Option<f32> {
        self.screen_factor
    }

    #[must_use]
    pub const fn depth_factor(&self) -> Option<f32> {
        self.depth_factor
    }

    #[must_use]
    pub const fn trim_disabled(&self) -> bool {
        self.trim_disabled
    }

    #[must_use]
    pub const fn headphone(&self) -> Option<HeadphoneState> {
        self.headphone
    }

    /// 所有原始字段是否都有经过验证的通用场景映射。
    #[must_use]
    pub const fn semantic_complete(&self) -> bool {
        self.semantic_complete
    }

    #[must_use]
    pub const fn raw(&self) -> RawOamdState {
        self.raw
    }
}

/// 一次更新实际改变的字段集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetadataFields(u32);

impl MetadataFields {
    pub const ACTIVE: Self = Self(1 << 0);
    pub const GAIN: Self = Self(1 << 1);
    pub const IMPORTANCE: Self = Self(1 << 2);
    pub const POSITION: Self = Self(1 << 3);
    pub const EXTENT: Self = Self(1 << 4);
    pub const ZONE: Self = Self(1 << 5);
    pub const SCREEN_FACTOR: Self = Self(1 << 6);
    pub const DEPTH_FACTOR: Self = Self(1 << 7);
    pub const DISTANCE: Self = Self(1 << 8);
    pub const DIVERGENCE: Self = Self(1 << 9);
    pub const TRIM: Self = Self(1 << 10);
    pub const HEADPHONE: Self = Self(1 << 11);

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, field: Self) -> bool {
        self.0 & field.0 == field.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// 一个帧内元数据更新。
#[derive(Debug, Clone, Copy)]
pub struct SceneMetadataUpdate {
    element_id: SceneElementId,
    offset_samples: u32,
    ramp_duration_samples: u32,
    changed_fields: MetadataFields,
    state: SceneObjectState,
    raw: RawOamdUpdate,
    /// 仅用于 Session 内无分配排序；不属于首版公共语义字段。
    stream_order: u64,
}

impl PartialEq for SceneMetadataUpdate {
    fn eq(&self, other: &Self) -> bool {
        self.element_id == other.element_id
            && self.offset_samples == other.offset_samples
            && self.ramp_duration_samples == other.ramp_duration_samples
            && self.changed_fields == other.changed_fields
            && self.state == other.state
            && self.raw == other.raw
    }
}

impl SceneMetadataUpdate {
    #[cfg_attr(
        not(feature = "audio-decode"),
        expect(dead_code, reason = "构造器只供 audio-decode 场景组装使用")
    )]
    pub(crate) const fn new(
        element_id: SceneElementId,
        offset_samples: u32,
        ramp_duration_samples: u32,
        changed_fields: MetadataFields,
        state: SceneObjectState,
        raw: RawOamdUpdate,
        stream_order: u64,
    ) -> Self {
        Self {
            element_id,
            offset_samples,
            ramp_duration_samples,
            changed_fields,
            state,
            raw,
            stream_order,
        }
    }

    #[cfg_attr(
        not(feature = "audio-decode"),
        expect(dead_code, reason = "跨帧偏移只在 audio-decode 场景组装中改写")
    )]
    pub(crate) const fn with_offset_samples(mut self, offset_samples: u32) -> Self {
        self.offset_samples = offset_samples;
        self
    }

    #[cfg_attr(
        not(feature = "audio-decode"),
        expect(dead_code, reason = "排序键只在 audio-decode 场景组装中读取")
    )]
    pub(crate) const fn stream_order(&self) -> u64 {
        self.stream_order
    }

    #[must_use]
    pub const fn element_id(&self) -> SceneElementId {
        self.element_id
    }

    #[must_use]
    pub const fn offset_samples(&self) -> u32 {
        self.offset_samples
    }

    #[must_use]
    pub const fn ramp_duration_samples(&self) -> u32 {
        self.ramp_duration_samples
    }

    #[must_use]
    pub const fn changed_fields(&self) -> MetadataFields {
        self.changed_fields
    }

    #[must_use]
    pub const fn state(&self) -> SceneObjectState {
        self.state
    }

    #[must_use]
    pub const fn raw(&self) -> RawOamdUpdate {
        self.raw
    }

    /// 产生本更新的 control source access unit；跨帧排队不会改写该来源。
    #[must_use]
    pub const fn control_source_access_unit_index(&self) -> u64 {
        self.raw.control_source_access_unit_index()
    }
}

/// 场景元素的编码侧来源键。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneElementSource {
    AjocCoreObject {
        substream_index: u32,
        object_index: u8,
        input_index: u32,
    },
    AjocCoreLfe {
        substream_index: u32,
        object_index: u8,
        output_index: u32,
    },
    AjocObject {
        substream_index: u32,
        object_index: u8,
        output_index: u32,
    },
    AjocLfe {
        substream_index: u32,
        object_index: u8,
        reinsertion_index: u32,
    },
}

/// 对象输出种类。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    AjocCoreObject,
    AjocSpatialObjectGroup,
}

/// 原生 bed 种类。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BedKind {
    AjocCoreLfe,
    AjocLfe,
}

/// 扬声器/component 标签。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerLabel {
    Lfe,
    Unknown,
}

/// 一个 Session 拥有的对象输出。
#[derive(Debug)]
pub struct SceneObject {
    pub(crate) element_id: SceneElementId,
    pub(crate) kind: ObjectKind,
    pub(crate) source: SceneElementSource,
    pub(crate) content_classifier: Option<u8>,
    pub(crate) initial_state: Option<SceneObjectState>,
    pub(crate) has_signal: bool,
    pub(crate) planes: Vec<PcmPlane>,
    pub(crate) samples_per_plane: usize,
}

impl SceneObject {
    #[must_use]
    pub const fn element_id(&self) -> SceneElementId {
        self.element_id
    }

    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }

    #[must_use]
    pub const fn source(&self) -> SceneElementSource {
        self.source
    }

    #[must_use]
    pub const fn content_classifier(&self) -> Option<u8> {
        self.content_classifier
    }

    /// 帧起点生效的完整状态；状态尚未到期时为 `None`。
    #[must_use]
    pub const fn initial_state(&self) -> Option<SceneObjectState> {
        self.initial_state
    }

    /// 当前 PCM 是否至少包含一个非零有限样本。
    #[must_use]
    pub const fn has_signal(&self) -> bool {
        self.has_signal
    }

    #[must_use]
    pub fn pcm(&self) -> PlanarPcm<'_> {
        PlanarPcm {
            planes: &self.planes,
            samples_per_plane: self.samples_per_plane,
        }
    }
}

/// 一个 bed component。
#[derive(Debug)]
pub struct SceneBedComponent {
    pub(crate) speaker: SpeakerLabel,
    pub(crate) has_signal: bool,
    pub(crate) plane: PcmPlane,
}

impl SceneBedComponent {
    #[must_use]
    pub const fn speaker(&self) -> SpeakerLabel {
        self.speaker
    }

    #[must_use]
    pub const fn has_signal(&self) -> bool {
        self.has_signal
    }

    #[must_use]
    pub const fn plane(&self) -> &PcmPlane {
        &self.plane
    }
}

/// 一个原生 bed；首版支持的 Full A-JOC 子集最多产生独立 LFE bed。
#[derive(Debug)]
pub struct SceneBed {
    pub(crate) element_id: SceneElementId,
    pub(crate) kind: BedKind,
    pub(crate) source: SceneElementSource,
    pub(crate) content_classifier: Option<u8>,
    pub(crate) initial_state: Option<SceneObjectState>,
    pub(crate) components: Vec<SceneBedComponent>,
}

impl SceneBed {
    #[must_use]
    pub const fn element_id(&self) -> SceneElementId {
        self.element_id
    }

    #[must_use]
    pub const fn kind(&self) -> BedKind {
        self.kind
    }

    #[must_use]
    pub const fn source(&self) -> SceneElementSource {
        self.source
    }

    #[must_use]
    pub const fn content_classifier(&self) -> Option<u8> {
        self.content_classifier
    }

    #[must_use]
    pub const fn initial_state(&self) -> Option<SceneObjectState> {
        self.initial_state
    }

    #[must_use]
    pub fn components(&self) -> &[SceneBedComponent] {
        &self.components
    }
}

/// renderer 需要的最小帧级诊断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameDiagnostics {
    pub(crate) reset: Option<ResetKind>,
    pub(crate) random_access: bool,
    pub(crate) random_access_hint_mismatch: bool,
    pub(crate) configuration_changed: bool,
    pub(crate) discontinuity: bool,
    pub(crate) warmup: bool,
    pub(crate) state_complete: bool,
    pub(crate) concealed: bool,
    pub(crate) semantic_metadata_complete: bool,
}

impl FrameDiagnostics {
    #[must_use]
    pub const fn reset(&self) -> Option<ResetKind> {
        self.reset
    }

    #[must_use]
    pub const fn random_access(&self) -> bool {
        self.random_access
    }

    #[must_use]
    pub const fn random_access_hint_mismatch(&self) -> bool {
        self.random_access_hint_mismatch
    }

    #[must_use]
    pub const fn configuration_changed(&self) -> bool {
        self.configuration_changed
    }

    #[must_use]
    pub const fn discontinuity(&self) -> bool {
        self.discontinuity
    }

    #[must_use]
    pub const fn warmup(&self) -> bool {
        self.warmup
    }

    #[must_use]
    pub const fn state_complete(&self) -> bool {
        self.state_complete
    }

    /// 首版不执行 concealment，因此该值恒为 false。
    #[must_use]
    pub const fn concealed(&self) -> bool {
        self.concealed
    }

    #[must_use]
    pub const fn semantic_metadata_complete(&self) -> bool {
        self.semantic_metadata_complete
    }
}

/// Session 内复用的一份场景帧存储。
#[derive(Debug)]
pub(crate) struct SceneFrameStorage {
    pub(crate) timeline: SceneTimeline,
    pub(crate) presentation: ScenePresentation,
    pub(crate) oamd_common_states: Vec<RawOamdCommonState>,
    pub(crate) beds: Vec<SceneBed>,
    pub(crate) objects: Vec<SceneObject>,
    pub(crate) metadata_updates: Vec<SceneMetadataUpdate>,
    pub(crate) diagnostics: FrameDiagnostics,
}

/// 借用 Session 内部存储的场景帧。
#[derive(Debug, Clone, Copy)]
pub struct Ac4SceneFrame<'a> {
    storage: &'a SceneFrameStorage,
}

impl Ac4SceneFrame<'_> {
    #[must_use]
    pub const fn timeline(&self) -> &SceneTimeline {
        &self.storage.timeline
    }

    #[must_use]
    pub const fn presentation(&self) -> &ScenePresentation {
        &self.storage.presentation
    }

    /// presentation 所引用各 group 的有效 OAMD common 状态。
    #[must_use]
    pub fn oamd_common_states(&self) -> &[RawOamdCommonState] {
        &self.storage.oamd_common_states
    }

    #[must_use]
    pub fn beds(&self) -> &[SceneBed] {
        &self.storage.beds
    }

    #[must_use]
    pub fn objects(&self) -> &[SceneObject] {
        &self.storage.objects
    }

    #[must_use]
    pub fn metadata_updates(&self) -> &[SceneMetadataUpdate] {
        &self.storage.metadata_updates
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &FrameDiagnostics {
        &self.storage.diagnostics
    }
}

/// Session 复用的 pre-A-SPX 核心带诊断存储。
#[cfg(feature = "audio-decode")]
#[derive(Debug, Default)]
pub(crate) struct CoreBandPcmFrameStorage {
    pub(crate) generation: Option<u32>,
    pub(crate) sample_rate: u32,
    pub(crate) substream_index: u32,
    pub(crate) samples_per_channel: usize,
    pub(crate) channels: Vec<CoreBandPcmChannelStorage>,
}

#[cfg(feature = "audio-decode")]
impl CoreBandPcmFrameStorage {
    pub(crate) fn reset(&mut self) {
        self.generation = None;
        self.samples_per_channel = 0;
        for channel in &mut self.channels {
            channel.samples.clear();
        }
    }

    pub(crate) fn clear_samples(&mut self) {
        self.samples_per_channel = 0;
        for channel in &mut self.channels {
            channel.samples.clear();
        }
    }
}

#[cfg(feature = "audio-decode")]
#[derive(Debug, Default)]
pub(crate) struct CoreBandPcmChannelStorage {
    pub(crate) element_index: usize,
    pub(crate) channel_index: usize,
    pub(crate) samples: Vec<f32>,
}

/// 当前 AU 在 A-SPX 之前的 ASF 核心带诊断 PCM。
///
/// 该视图随 `audio-decode` feature 提供，并要求调用方通过
/// [`Ac4DecoderConfig::with_core_band_diagnostics`] 显式启用。它不是 [`Ac4SceneFrame`]，
/// 也不把传输侧 `(element, channel)` 声明成渲染对象。样本与 Scene PCM 一样使用
/// normalized planar `f32`，但不经过表 188 场景对齐。
#[cfg(feature = "audio-decode")]
#[derive(Debug, Clone, Copy)]
pub struct CoreBandPcmFrame<'a> {
    storage: &'a CoreBandPcmFrameStorage,
}

#[cfg(feature = "audio-decode")]
impl<'a> CoreBandPcmFrame<'a> {
    /// 当前核心带帧的采样率。
    #[must_use]
    pub const fn sample_rate(self) -> u32 {
        self.storage.sample_rate
    }

    /// 当前 selected presentation 使用的物理 A-JOC substream。
    #[must_use]
    pub const fn substream_index(self) -> u32 {
        self.storage.substream_index
    }

    /// 诊断 PCM 固定为 planar `f32`。
    #[must_use]
    pub const fn sample_format(self) -> PcmSampleFormat {
        PcmSampleFormat::F32
    }

    /// 诊断 PCM 固定为 planar 布局。
    #[must_use]
    pub const fn layout(self) -> PcmLayout {
        PcmLayout::Planar
    }

    /// `1.0` 表示 nominal full scale；合法 overrange 不削波。
    #[must_use]
    pub const fn nominal_full_scale(self) -> f32 {
        1.0
    }

    /// 当前帧每路的有效样本数。
    #[must_use]
    pub const fn samples_per_channel(self) -> usize {
        self.storage.samples_per_channel
    }

    /// 当前帧的传输侧核心带声道数。
    #[must_use]
    pub fn channel_count(self) -> usize {
        self.storage.channels.len()
    }

    /// 按传输顺序读取一路核心带 PCM。
    #[must_use]
    pub fn channel(self, index: usize) -> Option<CoreBandPcmChannel<'a>> {
        Some(CoreBandPcmChannel {
            storage: self.storage.channels.get(index)?,
        })
    }
}

/// 一路 pre-A-SPX 核心带诊断 PCM。
#[cfg(feature = "audio-decode")]
#[derive(Debug, Clone, Copy)]
pub struct CoreBandPcmChannel<'a> {
    storage: &'a CoreBandPcmChannelStorage,
}

#[cfg(feature = "audio-decode")]
impl<'a> CoreBandPcmChannel<'a> {
    /// 传输侧 `var_channel_element` 下标。
    #[must_use]
    pub const fn element_index(self) -> usize {
        self.storage.element_index
    }

    /// 元素内声道下标。
    #[must_use]
    pub const fn channel_index(self) -> usize {
        self.storage.channel_index
    }

    /// normalized `f32` 的有效样本。
    #[must_use]
    pub fn samples(self) -> &'a [f32] {
        &self.storage.samples
    }

    /// planar PCM 的相邻有效样本步长恒为 1。
    #[must_use]
    pub const fn stride(self) -> usize {
        1
    }
}

/// 所选 presentation 当前 AU 的完整 processing-metadata 侧车。
///
/// 该视图只观察码流原值，不执行 DRC、gain、pan、downmix、loudness correction 或任何
/// renderer 处理。`payload` 借用 Session 自持的已验证副本，因此与
/// [`DecodedAccessUnit`] 一样只在下一次 Session 可变调用前有效。
#[derive(Debug, Clone, Copy)]
pub struct PresentationSubstreamMetadata<'a> {
    payload: &'a [u8],
    record: PresentationSubstreamMetadataRecord,
}

impl<'a> PresentationSubstreamMetadata<'a> {
    /// 产生该 metadata 的 AU 下标。
    #[must_use]
    pub const fn access_unit_index(self) -> u64 {
        self.record.access_unit_index
    }

    /// Session 已选择的零基 presentation 下标。
    #[must_use]
    pub const fn presentation_index(self) -> u32 {
        self.record.presentation_index
    }

    /// 所选 presentation 的码流标识；码流未声明时为 `None`。
    #[must_use]
    pub const fn presentation_id(self) -> Option<u32> {
        self.record.presentation_id
    }

    /// 该 presentation substream 在 TOC index table 中的物理下标。
    #[must_use]
    pub const fn substream_index(self) -> u32 {
        self.record.substream_index
    }

    /// 从同一 AU 拓扑派生、且已经用于验证 payload 的完整解析上下文。
    #[must_use]
    pub const fn context(self) -> PresentationSubstreamContext {
        self.record.context
    }

    /// Session 自持的完整有界 presentation substream payload。
    #[must_use]
    pub const fn payload(self) -> &'a [u8] {
        self.payload
    }

    /// 已按 `TS103190-2:v1.3.1:6.2.2.3` 的 `ac4_presentation_substream()` 语法严格验证的
    /// payload 前缀。
    ///
    /// 通常与 [`payload`](Self::payload) 相同。已观测的 object/A-JOC 独立 DRC 帧会在规范
    /// 末尾之后额外携带一个 `0x00` 或 `0x80`；Scene 为回放互操作性只接受这两种精确形态，
    /// 并把该字节排除在本切片之外。
    #[must_use]
    pub fn syntax_payload(self) -> &'a [u8] {
        self.payload
            .get(..self.record.syntax_payload_len)
            .unwrap_or(&[])
    }

    /// 规范语法之后、为已知回放兼容形态保留的原始尾部。
    ///
    /// 当前返回值只可能为空、单字节 `[0x00]` 或单字节 `[0x80]`。该字节没有被赋予 AC-4
    /// processing 语义，调用方不得把它解释为 DRC、gain 或 loudness-correction 字段。
    #[must_use]
    pub fn compatibility_tail(self) -> &'a [u8] {
        self.payload
            .get(self.record.syntax_payload_len..)
            .unwrap_or(&[])
    }

    /// 当前 AU 成功提交后有效的 DRC 配置。
    ///
    /// 这与 [`parsed_substream`](Self::parsed_substream) 中“本帧是否传输配置”保持区分：
    /// dependent frame 可以不重新传配置，但仍使用前一有效配置解析当前 DRC data。
    #[must_use]
    pub const fn effective_drc_configuration(self) -> Option<PresentationDrcConfiguration> {
        self.record.effective_drc_configuration
    }

    /// 当前 AU 生效的逐 substream-group 六比特 gain 码值。
    ///
    /// 返回值仍是原始码值，不换算为 dB，也不应用到 PCM。当前帧的 absent/keep/new 传输形态
    /// 保留在 [`Ac4PresentationSubstream::substream_group_gain_update`] 中。
    #[must_use]
    pub const fn effective_group_gain_codes(self) -> PresentationSubstreamGroupGainCodes {
        self.record.effective_group_gain_codes
    }

    /// 从 Session 自持 payload 重建完整、借用的 presentation metadata 视图。
    ///
    /// Session 在发布本侧车前已经用相同上下文和 DRC 起始状态完成过一次严格验证；这里对
    /// [`syntax_payload`](Self::syntax_payload) 重放 parser，是为了在禁止自引用存储的安全 Rust
    /// 中恢复所有 opaque bit view。已知的非规范兼容尾部不参与语法解析。
    ///
    /// # Errors
    ///
    /// 若 Session 自持数据不再满足先前验证过的不变量，则返回底层解析错误；正常使用中不应
    /// 出现该情况。
    pub fn parsed_substream(
        self,
    ) -> Result<Ac4PresentationSubstream<'a>, PresentationSubstreamError> {
        let mut drc_state = self.record.replay_drc_state;
        Ac4PresentationSubstream::parse_with_drc_state(
            self.syntax_payload(),
            self.record.context,
            &mut drc_state,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PresentationSubstreamMetadataRecord {
    pub(crate) access_unit_index: u64,
    pub(crate) presentation_index: u32,
    pub(crate) presentation_id: Option<u32>,
    pub(crate) substream_index: u32,
    pub(crate) context: PresentationSubstreamContext,
    pub(crate) syntax_payload_len: usize,
    pub(crate) replay_drc_state: PresentationDrcState,
    pub(crate) effective_drc_configuration: Option<PresentationDrcConfiguration>,
    pub(crate) effective_group_gain_codes: PresentationSubstreamGroupGainCodes,
}

/// Session 拥有的 presentation payload 副本与当前可见记录。
#[derive(Debug, Default)]
pub(crate) struct PresentationSubstreamMetadataStorage {
    payload: Vec<u8>,
    record: Option<PresentationSubstreamMetadataRecord>,
}

impl PresentationSubstreamMetadataStorage {
    pub(crate) fn clear(&mut self) {
        self.payload.clear();
        self.record = None;
    }

    pub(crate) fn try_reserve_payload(&mut self, payload_len: usize) -> Result<(), ()> {
        let additional = payload_len.saturating_sub(self.payload.len());
        self.payload.try_reserve(additional).map_err(|_| ())
    }

    pub(crate) fn publish(&mut self, payload: &[u8], record: PresentationSubstreamMetadataRecord) {
        self.payload.clear();
        self.payload.extend_from_slice(payload);
        self.record = Some(record);
    }

    #[cfg_attr(
        not(feature = "audio-decode"),
        allow(dead_code, reason = "无 audio-decode 时公开入口不会返回成功的 AU 视图")
    )]
    pub(crate) fn view(&self) -> Option<PresentationSubstreamMetadata<'_>> {
        Some(PresentationSubstreamMetadata {
            payload: &self.payload,
            record: self.record?,
        })
    }
}

/// 一次 AU 解码得到的借用结果。
#[derive(Debug)]
pub struct DecodedAccessUnit<'a> {
    status: DecodeStatus,
    frames: &'a [SceneFrameStorage],
    presentation_metadata: Option<PresentationSubstreamMetadata<'a>>,
    #[cfg(feature = "audio-decode")]
    core_band_pcm: Option<&'a CoreBandPcmFrameStorage>,
}

impl<'a> DecodedAccessUnit<'a> {
    #[cfg_attr(
        not(feature = "audio-decode"),
        expect(
            dead_code,
            reason = "无 audio-decode 时公开入口只返回结构化 unsupported"
        )
    )]
    pub(crate) const fn new(
        status: DecodeStatus,
        frames: &'a [SceneFrameStorage],
        presentation_metadata: Option<PresentationSubstreamMetadata<'a>>,
        #[cfg(feature = "audio-decode")] core_band_pcm: Option<&'a CoreBandPcmFrameStorage>,
    ) -> Self {
        Self {
            status,
            frames,
            presentation_metadata,
            #[cfg(feature = "audio-decode")]
            core_band_pcm,
        }
    }

    #[must_use]
    pub const fn status(&self) -> DecodeStatus {
        self.status
    }

    #[must_use]
    pub fn frames(&self) -> SceneFrameIter<'a> {
        SceneFrameIter {
            remaining: self.frames,
        }
    }

    #[must_use]
    pub const fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// 所选 presentation 当前 AU 的只读 processing-metadata 侧车。
    ///
    /// 只有完整解析、DSP 与 Scene 组装全部成功后才返回 `Some`；所选 presentation 没有
    /// presentation substream 或当前仍在等待随机访问点时为 `None`。
    #[must_use]
    pub const fn presentation_metadata(&self) -> Option<PresentationSubstreamMetadata<'a>> {
        self.presentation_metadata
    }

    /// 当前 AU 的 pre-A-SPX normalized 核心带诊断侧车。
    ///
    /// 只有显式启用核心带诊断且成功解码的 A-JOC AU 返回该视图；默认配置或等待随机
    /// 访问点时为 `None`。语义边界见 [`CoreBandPcmFrame`]。
    #[cfg(feature = "audio-decode")]
    #[must_use]
    pub fn core_band_pcm(&self) -> Option<CoreBandPcmFrame<'a>> {
        Some(CoreBandPcmFrame {
            storage: self.core_band_pcm?,
        })
    }
}

/// `DecodedAccessUnit` 中的场景帧迭代器。
#[derive(Debug, Clone)]
pub struct SceneFrameIter<'a> {
    remaining: &'a [SceneFrameStorage],
}

impl<'a> Iterator for SceneFrameIter<'a> {
    type Item = Ac4SceneFrame<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (first, tail) = self.remaining.split_first()?;
        self.remaining = tail;
        Some(Ac4SceneFrame { storage: first })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.remaining.len();
        (len, Some(len))
    }
}

impl ExactSizeIterator for SceneFrameIter<'_> {}
impl FusedIterator for SceneFrameIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    use PresentationSelectionMetadataIdentity as MetadataIdentity;

    fn scene_presentation(id: Option<u32>, identity_occurrences: u32) -> ScenePresentation {
        ScenePresentation {
            index: 3,
            id,
            identity_occurrences,
            version: 1,
            md_compat: Some(4),
            group_indices: alloc::vec![0],
            substream_indices: alloc::vec![1],
            path: ScenePath::Ajoc,
            mode: DecodeMode::Full,
        }
    }

    #[test]
    fn decoder_config_defaults_to_full_without_diagnostics_and_can_opt_in() {
        let default = Ac4DecoderConfig::default();
        assert_eq!(default.presentation(), PresentationSelection::AutoUnique);
        assert_eq!(default.decode_mode(), DecodeMode::Full);
        assert!(!default.core_band_diagnostics());

        let core = Ac4DecoderConfig::new(PresentationSelection::Index(2))
            .with_decode_mode(DecodeMode::Core)
            .with_core_band_diagnostics(true);
        assert_eq!(core.presentation(), PresentationSelection::Index(2));
        assert_eq!(core.decode_mode(), DecodeMode::Core);
        assert!(core.core_band_diagnostics());
    }

    #[test]
    fn presentation_metadata_matches_unique_effective_id_without_using_array_order() {
        let first_body = [0x20, 0x21];
        let selected_body = [0x40, 0x41, 0x42];
        let metadata = [
            PresentationSelectionMetadata::new(
                9,
                MetadataIdentity::EffectiveId(8),
                1,
                2,
                first_body.as_slice(),
            ),
            PresentationSelectionMetadata::new(
                2,
                MetadataIdentity::EffectiveId(4),
                2,
                3,
                selected_body.as_slice(),
            ),
        ];

        let matched = scene_presentation(Some(4), 1).match_selection_metadata(&metadata);

        let PresentationSelectionMetadataMatch::Matched { metadata, basis } = matched else {
            panic!("effective ID 应唯一关联 metadata");
        };
        assert_eq!(
            basis,
            PresentationSelectionMetadataMatchBasis::EffectivePresentationId
        );
        assert_eq!(metadata.source_index(), 2);
        assert_eq!(metadata.version(), 2);
        assert_eq!(metadata.declared_bytes(), 3);
        assert_eq!(*metadata.value(), selected_body.as_slice());
    }

    #[test]
    fn presentation_metadata_uses_only_a_unique_without_id_fallback() {
        let identified = [0x01];
        let anonymous = [0x02];
        let metadata = [
            PresentationSelectionMetadata::new(
                0,
                MetadataIdentity::EffectiveId(5),
                1,
                1,
                identified.as_slice(),
            ),
            PresentationSelectionMetadata::new(
                1,
                MetadataIdentity::WithoutId,
                7,
                1,
                anonymous.as_slice(),
            ),
        ];

        let matched = scene_presentation(None, 1).match_selection_metadata(&metadata);

        let PresentationSelectionMetadataMatch::Matched { metadata, basis } = matched else {
            panic!("双方唯一无 ID presentation 应允许回退");
        };
        assert_eq!(
            basis,
            PresentationSelectionMetadataMatchBasis::SingleWithoutId
        );
        assert_eq!(metadata.source_index(), 1);
        assert_eq!(metadata.effective_presentation_id(), None);
        assert_eq!(metadata.into_value(), anonymous.as_slice());
    }

    #[test]
    fn presentation_metadata_refuses_duplicate_ids_on_either_side() {
        let metadata = [
            PresentationSelectionMetadata::new(0, MetadataIdentity::EffectiveId(4), 1, 0, ()),
            PresentationSelectionMetadata::new(1, MetadataIdentity::EffectiveId(4), 1, 0, ()),
        ];

        assert_eq!(
            scene_presentation(Some(4), 1).match_selection_metadata(&metadata),
            PresentationSelectionMetadataMatch::Ambiguous {
                effective_presentation_id: Some(4),
                scene_candidates: 1,
                metadata_candidates: 2,
            }
        );
        assert_eq!(
            scene_presentation(Some(4), 2).match_selection_metadata(&metadata[..1]),
            PresentationSelectionMetadataMatch::Ambiguous {
                effective_presentation_id: Some(4),
                scene_candidates: 2,
                metadata_candidates: 1,
            }
        );
    }

    #[test]
    fn presentation_metadata_refuses_multiple_without_id_and_reports_missing() {
        let anonymous = [
            PresentationSelectionMetadata::new(0, MetadataIdentity::WithoutId, 1, 0, ()),
            PresentationSelectionMetadata::new(1, MetadataIdentity::WithoutId, 1, 0, ()),
        ];
        assert_eq!(
            scene_presentation(None, 2).match_selection_metadata(&anonymous),
            PresentationSelectionMetadataMatch::Ambiguous {
                effective_presentation_id: None,
                scene_candidates: 2,
                metadata_candidates: 2,
            }
        );

        let other = [PresentationSelectionMetadata::new(
            0,
            MetadataIdentity::EffectiveId(9),
            1,
            0,
            (),
        )];
        assert_eq!(
            scene_presentation(Some(4), 1).match_selection_metadata(&other),
            PresentationSelectionMetadataMatch::Missing {
                effective_presentation_id: Some(4),
            }
        );
    }

    #[test]
    fn opaque_presentation_identity_makes_association_indeterminate() {
        let opaque_body = [0x80, 0x00];
        let metadata = [PresentationSelectionMetadata::new(
            0,
            MetadataIdentity::Unavailable,
            2,
            2,
            opaque_body.as_slice(),
        )];

        assert_eq!(
            scene_presentation(None, 1).match_selection_metadata(&metadata),
            PresentationSelectionMetadataMatch::Indeterminate {
                effective_presentation_id: None,
                known_metadata_candidates: 0,
                unavailable_metadata: 1,
            }
        );
        assert_eq!(metadata[0].identity(), MetadataIdentity::Unavailable);
        assert_eq!(metadata[0].effective_presentation_id(), None);
        assert_eq!(metadata[0].declared_bytes(), 2);
        assert_eq!(*metadata[0].value(), opaque_body.as_slice());
    }

    #[test]
    fn opaque_presentation_identity_blocks_otherwise_unique_matches() {
        let metadata = [
            PresentationSelectionMetadata::new(0, MetadataIdentity::WithoutId, 1, 0, ()),
            PresentationSelectionMetadata::new(1, MetadataIdentity::Unavailable, 2, 0, ()),
        ];

        assert_eq!(
            scene_presentation(None, 1).match_selection_metadata(&metadata),
            PresentationSelectionMetadataMatch::Indeterminate {
                effective_presentation_id: None,
                known_metadata_candidates: 1,
                unavailable_metadata: 1,
            }
        );

        let metadata = [
            PresentationSelectionMetadata::new(0, MetadataIdentity::EffectiveId(4), 1, 0, ()),
            PresentationSelectionMetadata::new(1, MetadataIdentity::Unavailable, 2, 0, ()),
        ];
        assert_eq!(
            scene_presentation(Some(4), 1).match_selection_metadata(&metadata),
            PresentationSelectionMetadataMatch::Indeterminate {
                effective_presentation_id: Some(4),
                known_metadata_candidates: 1,
                unavailable_metadata: 1,
            }
        );
    }

    #[test]
    fn access_unit_context_keeps_external_timelines_separate() {
        let context = AccessUnitContext::new(7)
            .with_source_sample_start(-2_048)
            .with_presentation_sample_start(0)
            .with_priming_samples(2_048)
            .with_random_access_hint(true)
            .with_discontinuity(true);

        assert_eq!(context.index(), 7);
        assert_eq!(context.source_sample_start(), Some(-2_048));
        assert_eq!(context.presentation_sample_start(), Some(0));
        assert_eq!(context.priming_samples(), Some(2_048));
        assert_eq!(context.random_access_hint(), Some(true));
        assert!(context.discontinuity());
    }

    #[test]
    fn metadata_field_set_combines_without_an_external_dependency() {
        let fields = MetadataFields::ACTIVE
            .union(MetadataFields::POSITION)
            .union(MetadataFields::HEADPHONE);
        assert!(fields.contains(MetadataFields::ACTIVE));
        assert!(fields.contains(MetadataFields::POSITION));
        assert!(fields.contains(MetadataFields::HEADPHONE));
        assert!(!fields.contains(MetadataFields::GAIN));
    }

    #[test]
    fn planar_pcm_declares_normalized_f32_contract() {
        let planes = [PcmPlane {
            samples: alloc::vec![0.0, 1.25],
        }];
        let pcm = PlanarPcm {
            planes: &planes,
            samples_per_plane: 2,
        };
        assert_eq!(pcm.sample_format(), PcmSampleFormat::F32);
        assert_eq!(pcm.layout(), PcmLayout::Planar);
        assert_eq!(pcm.nominal_full_scale(), 1.0);
        assert_eq!(pcm.samples_per_plane(), 2);
        let plane = pcm.planes().first().expect("one PCM plane");
        assert_eq!(plane.samples(), [0.0, 1.25]);
        assert_eq!(plane.stride(), 1);
    }

    #[test]
    fn scene_pcm_uses_fixed_scale_without_clipping_overrange() {
        let mut plane = PcmPlane::with_capacity(4);
        let has_signal = plane
            .copy_normalized_from(&[0.0, 32_768.0, -65_536.0, -0.0])
            .expect("有限的内部标量应可归一化");

        assert!(has_signal);
        assert_eq!(plane.samples(), [0.0, 1.0, -2.0, -0.0]);
        assert!(
            plane
                .samples()
                .get(2)
                .is_some_and(|sample| sample.abs() > 1.0),
            "overrange 不得被削波"
        );
    }

    #[test]
    fn non_finite_scene_pcm_leaves_the_previous_plane_unchanged() {
        let mut plane = PcmPlane::with_capacity(2);
        plane
            .copy_normalized_from(&[16_384.0, -16_384.0])
            .expect("初始有限 PCM 应成功");
        let before = plane.samples().to_vec();

        let error = plane
            .copy_normalized_from(&[0.0, f32::NAN])
            .expect_err("非有限样本必须 fail-closed");

        assert_eq!(error.sample_index, 1);
        assert_eq!(plane.samples(), before);
    }

    #[test]
    fn scene_ids_are_opaque_but_serializable_by_value() {
        let id = SceneElementId(42);
        assert_eq!(id.get(), 42);
    }

    #[test]
    fn raw_common_state_preserves_group_values_and_refresh_provenance() {
        let common = OamdCommonData {
            default_screen_size_ratio: false,
            master_screen_size_ratio_code: Some(17),
            bed_object_chan_distribute: true,
            add_data_bytes: None,
            trim: Default::default(),
            bed_render_info: Default::default(),
            headphone: Default::default(),
        };
        let state = RawOamdCommonState {
            group_index: 3,
            effective: Some(common),
            updated_in_source_access_unit: true,
        };

        assert_eq!(state.group_index(), 3);
        assert_eq!(state.effective(), Some(common));
        assert!(state.updated_in_source_access_unit());
    }

    #[test]
    fn raw_update_preserves_effective_timing_and_encoding_provenance() {
        let timing = RawOamdTiming::new(
            SampleOffsetSource::Explicit,
            16,
            2,
            ObjInfoBlockTiming {
                block_offset_factor: 7,
                ramp_duration_code: 3,
                ramp_duration_encoding:
                    macindecode_ac4_bitstream::oamd::RampDurationEncoding::Explicit { value: 512 },
                ramp_duration: 512,
            },
            false,
        );
        let update = RawOamdUpdate::new(OamdMetadataBlock::default(), timing, 23);

        assert_eq!(update.timing(), timing);
        assert_eq!(update.control_source_access_unit_index(), 23);
        assert_eq!(update.timing().sample_offset(), 16);
        assert_eq!(update.timing().block().offset_samples(), 224);
        assert!(!update.timing().updated_in_source_access_unit());
    }
}
