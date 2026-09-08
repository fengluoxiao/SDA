//! Scene 解码错误及其稳定上下文。

use core::fmt;
use macindecode_ac4_bitstream::{
    PresentationSubstreamCapacity, PresentationSubstreamError,
    PresentationSubstreamGroupGainStateError,
    oamd::{OamdError, OamdStateError},
    reader::ReadError,
    topology::{Capacity, TopologyError, Unsupported as TopologyUnsupported},
};
#[cfg(feature = "audio-decode")]
use macindecode_ac4_decode::full_ajoc::FullAjocUnsupported;

/// 解码流水线中的结构化阶段。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeStage {
    Topology,
    AudioSyntax,
    CoreAudio,
    Aspx,
    Ajoc,
    Qmf,
    Oamd,
    SceneAssembly,
}

impl DecodeStage {
    /// 用于日志和诊断序列化的稳定名称。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Topology => "topology",
            Self::AudioSyntax => "audio_syntax",
            Self::CoreAudio => "core_audio",
            Self::Aspx => "aspx",
            Self::Ajoc => "ajoc",
            Self::Qmf => "qmf",
            Self::Oamd => "oamd",
            Self::SceneAssembly => "scene_assembly",
        }
    }
}

impl fmt::Display for DecodeStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// 截断输入还缺少的读取信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeedMoreData {
    required_bits: u64,
    available_bits: u64,
}

impl NeedMoreData {
    /// 完成当前 AU 解析至少需要的总比特数。
    #[must_use]
    pub const fn required_bits(self) -> u64 {
        self.required_bits
    }

    /// 当前 AU 实际提供的总比特数。
    #[must_use]
    pub const fn available_bits(self) -> u64 {
        self.available_bits
    }

    /// 为完成当前读取至少还需追加的比特数。
    #[must_use]
    pub const fn minimum_additional_bits(self) -> u64 {
        self.required_bits.saturating_sub(self.available_bits)
    }
}

impl fmt::Display for NeedMoreData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Access unit requires at least {} bits, but only {} bits are available",
            self.required_bits, self.available_bits
        )
    }
}

impl core::error::Error for NeedMoreData {}

/// presentation 选择失败。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationSelectionError {
    /// 没有携带音频 group 的 eligible presentation。
    NoEligiblePresentation { declared: u32 },
    /// `AutoUnique` 遇到多个 eligible presentation。
    Ambiguous { eligible: u32 },
    /// 零基下标超出码流声明范围。
    IndexOutOfRange { requested: u32, declared: u32 },
    /// `presentation_id` 在当前配置中不存在。
    IdNotFound { requested: u32 },
    /// 同一 `presentation_id` 在当前配置中出现多次。
    IdNotUnique { requested: u32, matches: u32 },
    /// 显式选择到了只携带数据或不引用音频 group 的 presentation。
    NotEligible { index: u32 },
}

impl fmt::Display for PresentationSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NoEligiblePresentation { declared } => write!(
                formatter,
                "Input declares {declared} presentations but has no eligible audio presentation"
            ),
            Self::Ambiguous { eligible } => write!(
                formatter,
                "Input has {eligible} eligible presentations; AutoUnique cannot make a unique selection"
            ),
            Self::IndexOutOfRange {
                requested,
                declared,
            } => write!(
                formatter,
                "Presentation index {requested} is out of range; input declares {declared} presentations"
            ),
            Self::IdNotFound { requested } => {
                write!(formatter, "presentation_id {requested} does not exist")
            }
            Self::IdNotUnique { requested, matches } => write!(
                formatter,
                "presentation_id {requested} occurs {matches} times and cannot be selected uniquely"
            ),
            Self::NotEligible { index } => {
                write!(
                    formatter,
                    "Presentation {index} has no selectable audio group"
                )
            }
        }
    }
}

impl core::error::Error for PresentationSelectionError {}

/// 当前 Scene 解码器明确拒绝的边界。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedReason {
    LegacyBitstreamVersion {
        bitstream_version: u32,
    },
    FutureBitstreamVersion {
        bitstream_version: u32,
    },
    ReservedChannelMode {
        channel_mode: u32,
    },
    TopologyCapacityExceeded {
        what: Capacity,
        declared: u32,
        limit: usize,
    },
    PresentationSubstreamCapacityExceeded {
        what: PresentationSubstreamCapacity,
        declared: u32,
        limit: usize,
    },
    ChannelBased,
    DirectObject,
    Mixed,
    EmptyScene,
    AjocSubstreamIndexAbsent,
    OamdSubstreamIndexAbsent,
    AjocSubstreamContextConflict,
    MultipleFullSubstreams {
        count: u32,
    },
    MultipleCoreSubstreams {
        count: u32,
    },
    MultiSubstreamFrameRate {
        frame_rate_factor: u32,
    },
    FragmentedFrameRate {
        frame_rate_fraction: u32,
    },
    StaticDownmix,
    SamplingFrequency {
        sampling_frequency_hz: u32,
    },
    FullbandDownmixSignalsExceeded {
        declared: u32,
        limit: u8,
    },
    AjocObjectsExceeded {
        declared: u32,
        limit: usize,
    },
    FullbandObjectAssignment {
        bed_signals: u32,
        isf_signals: u32,
    },
    CoreObjectAssignment {
        bed_signals: u32,
        isf_signals: u32,
    },
    CoreDialogueEnhancement {
        dialogue_objects: u8,
    },
    AudioMetadataBranch,
    AlternativeObjectMetadata,
    FullAjocBranch,
    #[cfg(feature = "audio-decode")]
    FullAjoc(FullAjocUnsupported),
    AudioDecodeFeatureDisabled,
}

impl fmt::Display for UnsupportedReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::LegacyBitstreamVersion { bitstream_version } => write!(
                formatter,
                "Legacy presentation syntax for bitstream_version {bitstream_version} is unsupported"
            ),
            Self::FutureBitstreamVersion { bitstream_version } => write!(
                formatter,
                "bitstream_version {bitstream_version} is outside the supported specification range"
            ),
            Self::ReservedChannelMode { channel_mode } => {
                write!(formatter, "Reserved channel_mode {channel_mode}")
            }
            Self::TopologyCapacityExceeded {
                what,
                declared,
                limit,
            } => write!(
                formatter,
                "{what} count {declared} exceeds the current scene implementation limit {limit}"
            ),
            Self::PresentationSubstreamCapacityExceeded {
                what,
                declared,
                limit,
            } => write!(
                formatter,
                "Presentation {what} count {declared} exceeds the current scene implementation limit {limit}"
            ),
            Self::ChannelBased => formatter.write_str("Channel-based scenes are not yet supported by this API"),
            Self::DirectObject => {
                formatter.write_str("Direct-object has no real-PCM validation and is explicitly rejected")
            }
            Self::Mixed => formatter.write_str("Mixed coding path is unsupported"),
            Self::EmptyScene => formatter.write_str("Selected presentation has no audio scene elements"),
            Self::AjocSubstreamIndexAbsent => {
                formatter.write_str("A-JOC substream has no explicit locatable index")
            }
            Self::OamdSubstreamIndexAbsent => {
                formatter.write_str("OAMD substream has no explicit locatable index")
            }
            Self::AjocSubstreamContextConflict => {
                formatter.write_str("Audio parse contexts conflict for the same physical A-JOC substream")
            }
            Self::MultipleFullSubstreams { count } => write!(
                formatter,
                "Selected presentation references {count} A-JOC Full substreams; only one is currently supported"
            ),
            Self::MultipleCoreSubstreams { count } => write!(
                formatter,
                "Selected presentation references {count} A-JOC Core substreams; only one is currently supported"
            ),
            Self::MultiSubstreamFrameRate { frame_rate_factor } => write!(
                formatter,
                "frame_rate_factor is {frame_rate_factor}, so one info block covers multiple consecutive substreams"
            ),
            Self::FragmentedFrameRate {
                frame_rate_fraction,
            } => write!(
                formatter,
                "frame_rate_fraction is {frame_rate_fraction}, so a codec frame spans multiple transport frames"
            ),
            Self::StaticDownmix => formatter
                .write_str("b_static_dmx requires a channel-based core downmix, unsupported by the current scene subset"),
            Self::SamplingFrequency {
                sampling_frequency_hz,
            } => write!(
                formatter,
                "A-JOC substream sample rate is {sampling_frequency_hz} Hz; only 44100/48000 Hz are supported"
            ),
            Self::FullbandDownmixSignalsExceeded { declared, limit } => write!(
                formatter,
                "A-JOC fullband downmix signal count {declared} exceeds the current limit {limit}"
            ),
            Self::AjocObjectsExceeded { declared, limit } => write!(
                formatter,
                "A-JOC object count {declared} exceeds the current OAMD/scene limit {limit}"
            ),
            Self::FullbandObjectAssignment {
                bed_signals,
                isf_signals,
            } => write!(
                formatter,
                "A-JOC Full upmix allocation contains {bed_signals} bed and {isf_signals} ISF signals; the current scene supports dynamic objects only"
            ),
            Self::CoreObjectAssignment {
                bed_signals,
                isf_signals,
            } => write!(
                formatter,
                "A-JOC Core downmix allocation contains {bed_signals} bed and {isf_signals} ISF signals; the current scene supports dynamic objects only"
            ),
            Self::CoreDialogueEnhancement { dialogue_objects } => write!(
                formatter,
                "A-JOC Core enables {dialogue_objects} dialogue-enhancement objects; the current scene does not apply their coefficients"
            ),
            Self::AudioMetadataBranch => formatter
                .write_str("ac4_substream metadata contains a branch unsupported by the current A-JOC scene subset"),
            Self::AlternativeObjectMetadata => {
                formatter.write_str(
                    "b_alternative object metadata is parsed but not yet applied to object output",
                )
            }
            Self::FullAjocBranch => {
                formatter.write_str("Full A-JOC engine returned an unclassified unsupported branch")
            }
            #[cfg(feature = "audio-decode")]
            Self::FullAjoc(reason) => write!(formatter, "{reason}"),
            Self::AudioDecodeFeatureDisabled => {
                formatter.write_str("audio-decode feature was not enabled at build time")
            }
        }
    }
}

impl core::error::Error for UnsupportedReason {}

/// 保留底层量化或结构错误的码流失败原因。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitstreamFailure {
    Topology(TopologyError),
    PresentationSubstream(PresentationSubstreamError),
    PresentationSubstreamGroupGain(PresentationSubstreamGroupGainStateError),
    Oamd(OamdError),
    OamdState(OamdStateError),
    OamdCommonConflict,
    OamdTimingConflict {
        expected: u8,
        actual: u8,
    },
    OamdUpdateOrder {
        object_index: u8,
        previous_sample: i64,
        current_sample: i64,
    },
    FrameLengthUnavailable {
        fs_index: u8,
        frame_rate_index: u8,
        frame_rate_factor: u32,
    },
}

impl fmt::Display for BitstreamFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Topology(error) => write!(formatter, "{error}"),
            Self::PresentationSubstream(error) => write!(formatter, "{error}"),
            Self::PresentationSubstreamGroupGain(error) => write!(formatter, "{error}"),
            Self::Oamd(error) => write!(formatter, "{error}"),
            Self::OamdState(error) => write!(formatter, "{error}"),
            Self::OamdCommonConflict => {
                formatter.write_str("Current OAMD common updates conflict within one group")
            }
            Self::OamdTimingConflict { expected, actual } => write!(
                formatter,
                "Selected Full A-JOC group has conflicting OAMD block counts: {expected} and {actual}"
            ),
            Self::OamdUpdateOrder {
                object_index,
                previous_sample,
                current_sample,
            } => write!(
                formatter,
                "OAMD update sample for object {object_index} regressed from {previous_sample} to {current_sample}"
            ),
            Self::FrameLengthUnavailable {
                fs_index,
                frame_rate_index,
                frame_rate_factor,
            } => write!(
                formatter,
                "The combination of fs_index {fs_index}, frame_rate_index {frame_rate_index}, and factor \
                 {frame_rate_factor} has no defined codec frame length"
            ),
        }
    }
}

impl core::error::Error for BitstreamFailure {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Topology(error) => Some(error),
            Self::PresentationSubstream(error) => Some(error),
            Self::PresentationSubstreamGroupGain(error) => Some(error),
            Self::Oamd(error) => Some(error),
            Self::OamdState(error) => Some(error),
            Self::OamdCommonConflict
            | Self::OamdTimingConflict { .. }
            | Self::OamdUpdateOrder { .. }
            | Self::FrameLengthUnavailable { .. } => None,
        }
    }
}

/// Scene API 的顶层错误类别。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeErrorKind {
    NeedMoreData(NeedMoreData),
    Selection(PresentationSelectionError),
    Unsupported(UnsupportedReason),
    InvalidBitstream(BitstreamFailure),
    DecodeFailure { stage: DecodeStage },
    InternalInvariant { stage: DecodeStage },
    ResetRequired,
}

impl fmt::Display for DecodeErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NeedMoreData(error) => write!(formatter, "Truncated input: {error}"),
            Self::Selection(error) => write!(formatter, "Presentation selection failed: {error}"),
            Self::Unsupported(reason) => write!(formatter, "Unsupported decode boundary: {reason}"),
            Self::InvalidBitstream(error) => write!(formatter, "Invalid bitstream: {error}"),
            Self::DecodeFailure { stage } => write!(formatter, "{stage} decode failed"),
            Self::InternalInvariant { stage } => {
                write!(formatter, "{stage} internal invariant failed")
            }
            Self::ResetRequired => {
                formatter.write_str("Session is invalid and must be explicitly reset")
            }
        }
    }
}

/// 错误发生位置；所有下标均使用码流中的零基值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeErrorContext {
    access_unit_index: u64,
    presentation_index: Option<u32>,
    presentation_id: Option<u32>,
    group_index: Option<u32>,
    substream_index: Option<u32>,
    syntax_path: Option<&'static str>,
    bit_offset: Option<u64>,
}

impl DecodeErrorContext {
    #[must_use]
    pub const fn access_unit_index(self) -> u64 {
        self.access_unit_index
    }

    #[must_use]
    pub const fn presentation_index(self) -> Option<u32> {
        self.presentation_index
    }

    #[must_use]
    pub const fn presentation_id(self) -> Option<u32> {
        self.presentation_id
    }

    #[must_use]
    pub const fn group_index(self) -> Option<u32> {
        self.group_index
    }

    #[must_use]
    pub const fn substream_index(self) -> Option<u32> {
        self.substream_index
    }

    #[must_use]
    pub const fn syntax_path(self) -> Option<&'static str> {
        self.syntax_path
    }

    #[must_use]
    pub const fn bit_offset(self) -> Option<u64> {
        self.bit_offset
    }

    pub(crate) const fn for_access_unit(access_unit_index: u64) -> Self {
        Self {
            access_unit_index,
            presentation_index: None,
            presentation_id: None,
            group_index: None,
            substream_index: None,
            syntax_path: None,
            bit_offset: None,
        }
    }

    pub(crate) const fn with_presentation(mut self, index: u32, id: Option<u32>) -> Self {
        self.presentation_index = Some(index);
        self.presentation_id = id;
        self
    }

    pub(crate) const fn with_group(mut self, index: u32) -> Self {
        self.group_index = Some(index);
        self
    }

    pub(crate) const fn with_substream(mut self, index: u32) -> Self {
        self.substream_index = Some(index);
        self
    }

    pub(crate) const fn with_syntax_path(mut self, path: &'static str) -> Self {
        self.syntax_path = Some(path);
        self
    }

    pub(crate) const fn with_bit_offset(mut self, bit_offset: u64) -> Self {
        self.bit_offset = Some(bit_offset);
        self
    }
}

/// 带 AU、presentation、group、substream 与语法位置的解码错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError {
    kind: DecodeErrorKind,
    context: DecodeErrorContext,
}

impl DecodeError {
    #[must_use]
    pub const fn kind(self) -> DecodeErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn context(self) -> DecodeErrorContext {
        self.context
    }

    pub(crate) const fn new(kind: DecodeErrorKind, context: DecodeErrorContext) -> Self {
        Self { kind, context }
    }

    pub(crate) fn from_topology(error: TopologyError, access_unit_index: u64) -> Self {
        let mut context = DecodeErrorContext::for_access_unit(access_unit_index)
            .with_syntax_path(topology_syntax_path(&error));
        if let Some(bit_offset) = topology_bit_offset(&error) {
            context = context.with_bit_offset(bit_offset);
        }
        context = match error {
            TopologyError::GroupIndexOutOfRange { group_index, .. } => {
                context.with_group(group_index)
            }
            TopologyError::SubstreamIndexOutOfRange { index, .. }
            | TopologyError::UnreferencedSubstream { index }
            | TopologyError::SubstreamPayloadOutOfFrame { index, .. } => {
                context.with_substream(index)
            }
            _ => context,
        };

        let kind = match error {
            TopologyError::Read(ReadError::OutOfBounds {
                requested_bits,
                bit_position,
                remaining_bits,
            })
            | TopologyError::OamdCommon(OamdError::Read(ReadError::OutOfBounds {
                requested_bits,
                bit_position,
                remaining_bits,
            })) => DecodeErrorKind::NeedMoreData(need_more_from_read(
                requested_bits,
                bit_position,
                remaining_bits,
            )),
            payload @ TopologyError::SubstreamPayloadOutOfFrame { .. } => {
                DecodeErrorKind::InvalidBitstream(BitstreamFailure::Topology(payload))
            }
            TopologyError::Unsupported { what, .. } => {
                DecodeErrorKind::Unsupported(topology_unsupported(what))
            }
            TopologyError::CapacityExceeded {
                what,
                declared,
                limit,
            } => DecodeErrorKind::Unsupported(UnsupportedReason::TopologyCapacityExceeded {
                what,
                declared,
                limit,
            }),
            other => DecodeErrorKind::InvalidBitstream(BitstreamFailure::Topology(other)),
        };
        Self::new(kind, context)
    }

    pub(crate) fn from_presentation_substream(
        error: PresentationSubstreamError,
        access_unit_index: u64,
        presentation_index: u32,
        presentation_id: Option<u32>,
        substream_index: u32,
    ) -> Self {
        let mut context = DecodeErrorContext::for_access_unit(access_unit_index)
            .with_presentation(presentation_index, presentation_id)
            .with_substream(substream_index)
            .with_syntax_path("raw_ac4_frame/ac4_presentation_substream");
        if let Some(bit_offset) = presentation_substream_bit_offset(error) {
            context = context.with_bit_offset(bit_offset);
        }
        let kind = match error {
            PresentationSubstreamError::CapacityExceeded {
                what,
                declared,
                limit,
            } => DecodeErrorKind::Unsupported(
                UnsupportedReason::PresentationSubstreamCapacityExceeded {
                    what,
                    declared,
                    limit,
                },
            ),
            other => {
                DecodeErrorKind::InvalidBitstream(BitstreamFailure::PresentationSubstream(other))
            }
        };
        Self::new(kind, context)
    }

    pub(crate) fn from_presentation_group_gain(
        error: PresentationSubstreamGroupGainStateError,
        access_unit_index: u64,
        presentation_index: u32,
        presentation_id: Option<u32>,
        substream_index: u32,
    ) -> Self {
        Self::new(
            DecodeErrorKind::InvalidBitstream(BitstreamFailure::PresentationSubstreamGroupGain(
                error,
            )),
            DecodeErrorContext::for_access_unit(access_unit_index)
                .with_presentation(presentation_index, presentation_id)
                .with_substream(substream_index)
                .with_syntax_path("raw_ac4_frame/ac4_presentation_substream/sg_gain"),
        )
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "AU {}: {}",
            self.context.access_unit_index, self.kind
        )?;
        if let Some(path) = self.context.syntax_path {
            write!(formatter, " ({path}")?;
            if let Some(bit_offset) = self.context.bit_offset {
                write!(formatter, ", bit {bit_offset}")?;
            }
            formatter.write_str(")")?;
        }
        Ok(())
    }
}

impl core::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match &self.kind {
            DecodeErrorKind::NeedMoreData(error) => Some(error),
            DecodeErrorKind::Selection(error) => Some(error),
            DecodeErrorKind::Unsupported(error) => Some(error),
            DecodeErrorKind::InvalidBitstream(error) => Some(error),
            DecodeErrorKind::DecodeFailure { .. }
            | DecodeErrorKind::InternalInvariant { .. }
            | DecodeErrorKind::ResetRequired => None,
        }
    }
}

const fn topology_unsupported(error: TopologyUnsupported) -> UnsupportedReason {
    match error {
        TopologyUnsupported::LegacyPresentationInfo { bitstream_version } => {
            UnsupportedReason::LegacyBitstreamVersion { bitstream_version }
        }
        TopologyUnsupported::FutureBitstreamVersion { bitstream_version } => {
            UnsupportedReason::FutureBitstreamVersion { bitstream_version }
        }
        TopologyUnsupported::ReservedChannelMode { ch_mode } => {
            UnsupportedReason::ReservedChannelMode {
                channel_mode: ch_mode,
            }
        }
    }
}

const fn topology_syntax_path(error: &TopologyError) -> &'static str {
    match error {
        TopologyError::OamdCommon(_) => {
            "raw_ac4_frame/ac4_toc/ac4_substream_group_info/oamd_common_data"
        }
        TopologyError::Unsupported {
            what:
                TopologyUnsupported::LegacyPresentationInfo { .. }
                | TopologyUnsupported::FutureBitstreamVersion { .. },
            ..
        }
        | TopologyError::PresentationVersionTooLong { .. } => {
            "raw_ac4_frame/ac4_toc/ac4_presentation_info"
        }
        TopologyError::Unsupported {
            what: TopologyUnsupported::ReservedChannelMode { .. },
            ..
        } => "raw_ac4_frame/ac4_toc/ac4_substream_group_info/channel_mode",
        TopologyError::SubstreamIndexOutOfRange { .. }
        | TopologyError::UnreferencedSubstream { .. }
        | TopologyError::SubstreamSizesAbsent
        | TopologyError::SubstreamPayloadOutOfFrame { .. } => {
            "raw_ac4_frame/ac4_toc/substream_index_table"
        }
        TopologyError::GroupIndexOutOfRange { .. } => {
            "raw_ac4_frame/ac4_toc/ac4_presentation_info/group_index"
        }
        TopologyError::Read(_) | TopologyError::CapacityExceeded { .. } => "raw_ac4_frame/ac4_toc",
    }
}

const fn topology_bit_offset(error: &TopologyError) -> Option<u64> {
    match *error {
        TopologyError::Read(read) | TopologyError::OamdCommon(OamdError::Read(read)) => {
            Some(read_bit_offset(read))
        }
        TopologyError::Unsupported { bit_position, .. }
        | TopologyError::PresentationVersionTooLong { bit_position } => Some(bit_position),
        TopologyError::SubstreamPayloadOutOfFrame { frame_len, .. } => {
            Some(frame_len.saturating_mul(8))
        }
        TopologyError::OamdCommon(_)
        | TopologyError::CapacityExceeded { .. }
        | TopologyError::GroupIndexOutOfRange { .. }
        | TopologyError::SubstreamIndexOutOfRange { .. }
        | TopologyError::UnreferencedSubstream { .. }
        | TopologyError::SubstreamSizesAbsent => None,
    }
}

const fn read_bit_offset(error: ReadError) -> u64 {
    match error {
        ReadError::OutOfBounds { bit_position, .. }
        | ReadError::WidthUnsupported { bit_position, .. }
        | ReadError::ValueOverflow { bit_position } => bit_position,
    }
}

const fn presentation_substream_bit_offset(error: PresentationSubstreamError) -> Option<u64> {
    match error {
        PresentationSubstreamError::Read(error) => Some(read_bit_offset(error)),
        PresentationSubstreamError::InvalidLoudnessExtensionSize { bit_position, .. }
        | PresentationSubstreamError::InvalidDrcMetadataSize { bit_position, .. }
        | PresentationSubstreamError::InvalidDrcGainSetSize { bit_position, .. }
        | PresentationSubstreamError::InvalidFixedDrcGainSetSize { bit_position, .. }
        | PresentationSubstreamError::MissingDrcConfiguration { bit_position }
        | PresentationSubstreamError::TrailingDrcFrameBits { bit_position, .. }
        | PresentationSubstreamError::ReservedAssociatedPan { bit_position, .. }
        | PresentationSubstreamError::UnusedCustomDownmixOutputChannelConfig {
            bit_position, ..
        }
        | PresentationSubstreamError::ReservedStereoSurroundMixGain { bit_position, .. } => {
            Some(bit_position)
        }
        PresentationSubstreamError::MissingAudioSubstreams
        | PresentationSubstreamError::MissingDrcRepeatProfile { .. }
        | PresentationSubstreamError::CyclicDrcRepeatProfile { .. }
        | PresentationSubstreamError::TrailingBits { .. }
        | PresentationSubstreamError::CapacityExceeded { .. } => None,
    }
}

const fn need_more_from_read(
    requested_bits: u32,
    bit_position: u64,
    remaining_bits: u64,
) -> NeedMoreData {
    NeedMoreData {
        required_bits: bit_position.saturating_add(requested_bits as u64),
        available_bits: bit_position.saturating_add(remaining_bits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macindecode_ac4_bitstream::{PresentationSubstreamCapacity, topology::Capacity};

    #[test]
    fn truncated_topology_maps_to_retryable_need_more_data() {
        let error = DecodeError::from_topology(
            TopologyError::Read(ReadError::OutOfBounds {
                requested_bits: 7,
                bit_position: 19,
                remaining_bits: 2,
            }),
            41,
        );

        let DecodeErrorKind::NeedMoreData(needed) = error.kind() else {
            panic!("截断输入必须是 NeedMoreData")
        };
        assert_eq!(needed.minimum_additional_bits(), 5);
        assert_eq!(needed.required_bits(), 26);
        assert_eq!(needed.available_bits(), 21);
        assert_eq!(error.context().access_unit_index(), 41);
        assert_eq!(error.context().bit_offset(), Some(19));
        assert_eq!(error.context().syntax_path(), Some("raw_ac4_frame/ac4_toc"));
    }

    #[test]
    fn payload_past_the_bounded_au_is_invalid_bitstream() {
        let topology = TopologyError::SubstreamPayloadOutOfFrame {
            index: 2,
            start: 80,
            end: 112,
            frame_len: 96,
        };
        let error = DecodeError::from_topology(topology, 5);

        assert_eq!(
            error.kind(),
            DecodeErrorKind::InvalidBitstream(BitstreamFailure::Topology(topology))
        );
        assert_eq!(error.context().bit_offset(), Some(96 * 8));
        assert_eq!(error.context().substream_index(), Some(2));
    }

    #[test]
    fn topology_capacity_is_unsupported_instead_of_invalid() {
        let topology = TopologyError::CapacityExceeded {
            what: Capacity::Presentations,
            declared: 9,
            limit: 8,
        };
        let error = DecodeError::from_topology(topology, 3);

        assert_eq!(
            error.kind(),
            DecodeErrorKind::Unsupported(UnsupportedReason::TopologyCapacityExceeded {
                what: Capacity::Presentations,
                declared: 9,
                limit: 8,
            })
        );
    }

    #[test]
    fn presentation_substream_capacity_is_unsupported_instead_of_invalid() {
        let error = DecodeError::from_presentation_substream(
            PresentationSubstreamError::CapacityExceeded {
                what: PresentationSubstreamCapacity::Targets,
                declared: 33,
                limit: 32,
            },
            7,
            1,
            Some(11),
            3,
        );

        assert_eq!(
            error.kind(),
            DecodeErrorKind::Unsupported(
                UnsupportedReason::PresentationSubstreamCapacityExceeded {
                    what: PresentationSubstreamCapacity::Targets,
                    declared: 33,
                    limit: 32,
                }
            )
        );
        assert_eq!(error.context().access_unit_index(), 7);
        assert_eq!(error.context().presentation_index(), Some(1));
        assert_eq!(error.context().presentation_id(), Some(11));
        assert_eq!(error.context().substream_index(), Some(3));
    }

    #[test]
    fn topology_reference_errors_keep_their_structured_indices() {
        let group = DecodeError::from_topology(
            TopologyError::GroupIndexOutOfRange {
                group_index: 7,
                total: 2,
            },
            3,
        );
        assert_eq!(group.context().group_index(), Some(7));

        let substream = DecodeError::from_topology(
            TopologyError::SubstreamIndexOutOfRange {
                index: 11,
                total: 4,
            },
            3,
        );
        assert_eq!(substream.context().substream_index(), Some(11));
    }

    #[test]
    fn topology_unsupported_keeps_structured_reason_and_bit_offset() {
        let error = DecodeError::from_topology(
            TopologyError::Unsupported {
                what: TopologyUnsupported::FutureBitstreamVersion {
                    bitstream_version: 5,
                },
                bit_position: 6,
            },
            9,
        );

        assert_eq!(
            error.kind(),
            DecodeErrorKind::Unsupported(UnsupportedReason::FutureBitstreamVersion {
                bitstream_version: 5,
            })
        );
        assert_eq!(error.context().bit_offset(), Some(6));
    }
}
