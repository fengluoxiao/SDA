//! Session 控制候选到 Full A-JOC engine 的事务适配。

use macindecode_ac4_bitstream::{
    audio_substream::AudioSubstreamError, emdf::EmdfError, oamd::OamdError, reader::ReadError,
    topology::Ac4Topology,
};
use macindecode_ac4_decode::{
    ajoc::{AjocError, de::AjocDeError},
    asf::{AsfError, AsfSpectrumError},
    aspx::AspxError,
    audio_data::AudioDataError,
    channel::ChannelError,
    full_ajoc::{
        DecodedFullAjocAudioFrame, FullAjocAsfError, FullAjocAsfErrorKind, FullAjocAudioFrameError,
        FullAjocAudioFrameInput, FullAjocBlocker, FullAjocDecodeError, FullAjocDecodeErrorKind,
        FullAjocDecodeMode, FullAjocDecoder, FullAjocFrameProvenance, FullAjocGroupOamdState,
        FullAjocSyntaxError, FullAjocSyntaxFrameInput, FullAjocUnsupported,
    },
    huffman::HuffmanError,
    substream_audio::{AjocSubstreamContext, SubstreamAudioError},
    var_element::{MAX_FULLBAND_DMX_SIGNALS, VarElementError},
};

use crate::{
    AccessUnitContext, BitstreamFailure, DecodeError, DecodeErrorContext, DecodeErrorKind,
    DecodeMode, DecodeStage, UnsupportedReason, group_oamd::PreparedGroupOamd,
    session::ResolvedPresentation,
};

const AJOC_INFO_SYNTAX: &str =
    "raw_ac4_frame/ac4_toc/ac4_substream_group_info/ac4_substream_info_ajoc";
const AC4_SUBSTREAM_SYNTAX: &str = "raw_ac4_frame/ac4_substream";
const AJOC_PAYLOAD_SYNTAX: &str = "raw_ac4_frame/ac4_substream/audio_data_ajoc";
const ASF_SYNTAX: &str = "raw_ac4_frame/ac4_substream/audio_data_ajoc/var_channel_element/asf";
const ASPX_SYNTAX: &str = "raw_ac4_frame/ac4_substream/audio_data_ajoc/var_channel_element/aspx";
const FULL_SYNTAX: &str = "raw_ac4_frame/ac4_substream/audio_data_ajoc/ajoc";
const DMX_DE_SYNTAX: &str = "raw_ac4_frame/ac4_substream/audio_data_ajoc/ajoc_dmx_de_data";
const OAMD_SYNTAX: &str = "raw_ac4_frame/ac4_substream/audio_data_ajoc/oamd_dyndata_single";
const GROUP_OAMD_SYNTAX: &str = "raw_ac4_frame/ac4_toc/ac4_substream_group_info/oamd_common_data";

/// 把同一控制面候选中的所有权信息冻结为一次完整 A-JOC engine 调用。
///
/// `physical_substreams` 是所选 presentation 实际引用的物理 A-JOC substream 数；
/// Session 已在拓扑门禁收敛到恰好一条，不能误用 index table 中包含 presentation、
/// OAMD 或 EMDF 在内的总条目数。
pub(crate) fn prepare_full_ajoc_input<'frame>(
    raw_frame: &'frame [u8],
    access_unit: AccessUnitContext,
    topology: &Ac4Topology,
    presentation: ResolvedPresentation,
    decode_mode: DecodeMode,
    group_oamd: &PreparedGroupOamd,
) -> Result<FullAjocAudioFrameInput<'frame>, DecodeError> {
    let context = AjocSubstreamContext::derive(
        &topology.toc,
        &presentation.ajoc_info,
        1,
        1,
        group_oamd.alternative(),
        group_oamd.group_num_obj_info_blocks(),
    )
    .map_err(|error| context_error(error, access_unit, presentation))?;
    let payload = topology
        .substream_payload(raw_frame, presentation.substream_index)
        .map_err(|_| {
            DecodeError::new(
                DecodeErrorKind::InternalInvariant {
                    stage: DecodeStage::Topology,
                },
                error_context(access_unit, presentation).with_syntax_path(AJOC_PAYLOAD_SYNTAX),
            )
        })?;

    let provenance = provenance(access_unit, presentation, group_oamd)?;
    Ok(FullAjocAudioFrameInput {
        syntax: FullAjocSyntaxFrameInput {
            payload,
            context,
            substream_index: presentation.substream_index,
            physical_substreams: 1,
        },
        provenance,
        lfe_position: presentation.ajoc_info.lfe_reinsertion_position(),
        mode: match decode_mode {
            DecodeMode::Core => FullAjocDecodeMode::RequireCore,
            DecodeMode::Full => FullAjocDecodeMode::RequireFull,
        },
    })
}

/// 驱动 Session 自持的 Full engine，并把底层失败收敛为 Scene 稳定错误模型。
///
/// 调用前的 topology 已经验证完整 raw AU 与全部 substream 边界；因此这里的
/// `OutOfBounds` 是有界音频区段内部的坏语法，而不是还能通过追加 AU 字节恢复的
/// [`crate::NeedMoreData`]。底层 engine 自身保证失败不返回半帧，Session 再负责
/// 清空其它控制历史。
pub(crate) fn decode_full_ajoc_frame<'decoder>(
    decoder: &'decoder mut FullAjocDecoder,
    input: FullAjocAudioFrameInput<'_>,
    access_unit: AccessUnitContext,
    presentation: ResolvedPresentation,
) -> Result<DecodedFullAjocAudioFrame<'decoder>, DecodeError> {
    let mode = input.mode;
    decoder
        .decode_complete_audio_frame(input)
        .map_err(|error| engine_error(error, mode, access_unit, presentation))
}

fn engine_error(
    error: FullAjocAudioFrameError,
    mode: FullAjocDecodeMode,
    access_unit: AccessUnitContext,
    presentation: ResolvedPresentation,
) -> DecodeError {
    match error {
        FullAjocAudioFrameError::Syntax(error) => syntax_error(error, access_unit, presentation),
        FullAjocAudioFrameError::Asf(error) => asf_error(error, access_unit, presentation),
        FullAjocAudioFrameError::Decode(error) => {
            decode_error(error, mode, access_unit, presentation)
        }
        _ => DecodeError::new(
            DecodeErrorKind::InternalInvariant {
                stage: DecodeStage::AudioSyntax,
            },
            error_context(access_unit, presentation).with_syntax_path(AJOC_PAYLOAD_SYNTAX),
        ),
    }
}

fn syntax_error(
    error: FullAjocSyntaxError,
    access_unit: AccessUnitContext,
    presentation: ResolvedPresentation,
) -> DecodeError {
    match error {
        FullAjocSyntaxError::Decode {
            substream_index,
            error,
        } => substream_syntax_error(
            error,
            error_context(access_unit, presentation).with_substream(substream_index),
            presentation,
        ),
        FullAjocSyntaxError::SubstreamIndexOutOfRange { .. }
        | FullAjocSyntaxError::WorkspaceInvariant { .. } => DecodeError::new(
            DecodeErrorKind::InternalInvariant {
                stage: DecodeStage::AudioSyntax,
            },
            error_context(access_unit, presentation).with_syntax_path(AJOC_PAYLOAD_SYNTAX),
        ),
        _ => DecodeError::new(
            DecodeErrorKind::InternalInvariant {
                stage: DecodeStage::AudioSyntax,
            },
            error_context(access_unit, presentation).with_syntax_path(AJOC_PAYLOAD_SYNTAX),
        ),
    }
}

fn substream_syntax_error(
    error: SubstreamAudioError,
    context: DecodeErrorContext,
    presentation: ResolvedPresentation,
) -> DecodeError {
    match error {
        SubstreamAudioError::Substream(error) => {
            let kind = match error {
                AudioSubstreamError::Unsupported { .. } => {
                    DecodeErrorKind::Unsupported(UnsupportedReason::AudioMetadataBranch)
                }
                _ => DecodeErrorKind::DecodeFailure {
                    stage: DecodeStage::AudioSyntax,
                },
            };
            DecodeError::new(
                kind,
                with_bit_offset(
                    context.with_syntax_path(AC4_SUBSTREAM_SYNTAX),
                    audio_substream_bit_offset(error),
                ),
            )
        }
        SubstreamAudioError::AudioData(error) => {
            let (kind, path) = match error {
                AudioDataError::StaticDownmixUnsupported => (
                    DecodeErrorKind::Unsupported(UnsupportedReason::StaticDownmix),
                    AJOC_PAYLOAD_SYNTAX,
                ),
                AudioDataError::Oamd(error) => (
                    DecodeErrorKind::InvalidBitstream(BitstreamFailure::Oamd(error)),
                    OAMD_SYNTAX,
                ),
                _ => (
                    DecodeErrorKind::DecodeFailure {
                        stage: DecodeStage::AudioSyntax,
                    },
                    AJOC_PAYLOAD_SYNTAX,
                ),
            };
            DecodeError::new(
                kind,
                with_bit_offset(context.with_syntax_path(path), audio_data_bit_offset(error)),
            )
        }
        SubstreamAudioError::Oamd(error) => DecodeError::new(
            DecodeErrorKind::InvalidBitstream(BitstreamFailure::Oamd(error)),
            with_bit_offset(
                context.with_syntax_path(OAMD_SYNTAX),
                oamd_bit_offset(error),
            ),
        ),
        other => DecodeError::new(
            context_kind(other, presentation),
            context.with_syntax_path(AJOC_PAYLOAD_SYNTAX),
        ),
    }
}

fn asf_error(
    error: FullAjocAsfError,
    access_unit: AccessUnitContext,
    presentation: ResolvedPresentation,
) -> DecodeError {
    let stage = match error.kind() {
        FullAjocAsfErrorKind::SubstreamIndexOutOfRange { .. }
        | FullAjocAsfErrorKind::TooManyElements { .. }
        | FullAjocAsfErrorKind::UnsupportedChannelCount { .. }
        | FullAjocAsfErrorKind::MissingLayout
        | FullAjocAsfErrorKind::MissingSpectrum
        | FullAjocAsfErrorKind::FrameLengthChanged { .. }
        | FullAjocAsfErrorKind::NonFinite { .. }
        | FullAjocAsfErrorKind::WorkspaceInvariant { .. } => DecodeErrorKind::InternalInvariant {
            stage: DecodeStage::CoreAudio,
        },
        FullAjocAsfErrorKind::UnsupportedFrameLength { .. }
        | FullAjocAsfErrorKind::ScaleFactors(_)
        | FullAjocAsfErrorKind::ScaleSpectrum(_)
        | FullAjocAsfErrorKind::ChannelMatrix(_)
        | FullAjocAsfErrorKind::UngroupSpectrum(_)
        | FullAjocAsfErrorKind::Synthesis(_) => DecodeErrorKind::DecodeFailure {
            stage: DecodeStage::CoreAudio,
        },
        _ => DecodeErrorKind::InternalInvariant {
            stage: DecodeStage::CoreAudio,
        },
    };
    DecodeError::new(
        stage,
        error_context(access_unit, presentation)
            .with_substream(error.substream_index())
            .with_syntax_path(ASF_SYNTAX),
    )
}

fn decode_error(
    error: FullAjocDecodeError,
    mode: FullAjocDecodeMode,
    access_unit: AccessUnitContext,
    presentation: ResolvedPresentation,
) -> DecodeError {
    let error_kind = error.kind();
    let unsupported = error.unsupported_reason();
    let kind = match error_kind {
        FullAjocDecodeErrorKind::Unsupported => {
            DecodeErrorKind::Unsupported(map_unsupported_reason(mode, unsupported))
        }
        FullAjocDecodeErrorKind::Reconstruction => DecodeErrorKind::DecodeFailure {
            stage: DecodeStage::Ajoc,
        },
        FullAjocDecodeErrorKind::OamdState => DecodeErrorKind::DecodeFailure {
            stage: DecodeStage::Oamd,
        },
        FullAjocDecodeErrorKind::Other
        | FullAjocDecodeErrorKind::DecodeModeMismatch
        | FullAjocDecodeErrorKind::ObjectsNonFinite
        | FullAjocDecodeErrorKind::ObjectShapeMismatch => DecodeErrorKind::InternalInvariant {
            stage: DecodeStage::Ajoc,
        },
        _ => DecodeErrorKind::InternalInvariant {
            stage: DecodeStage::Ajoc,
        },
    };
    DecodeError::new(
        kind,
        error_context(access_unit, presentation).with_syntax_path(
            decode_error_syntax_path_for_mode(error_kind, unsupported, mode),
        ),
    )
}

const fn map_unsupported_reason(
    mode: FullAjocDecodeMode,
    unsupported: Option<FullAjocUnsupported>,
) -> UnsupportedReason {
    match (mode, unsupported) {
        (_, Some(FullAjocUnsupported::Full(FullAjocBlocker::AlternativeObjectMetadata))) => {
            UnsupportedReason::AlternativeObjectMetadata
        }
        (
            FullAjocDecodeMode::RequireCore,
            Some(FullAjocUnsupported::Full(FullAjocBlocker::ActiveDialogueEnhancement {
                dialogue_objects,
            })),
        ) => UnsupportedReason::CoreDialogueEnhancement { dialogue_objects },
        (_, Some(reason)) => UnsupportedReason::FullAjoc(reason),
        (_, None) => UnsupportedReason::FullAjocBranch,
    }
}

const fn decode_error_syntax_path_for_mode(
    kind: FullAjocDecodeErrorKind,
    unsupported: Option<FullAjocUnsupported>,
    mode: FullAjocDecodeMode,
) -> &'static str {
    if matches!(
        (mode, unsupported),
        (
            FullAjocDecodeMode::RequireCore,
            Some(FullAjocUnsupported::Full(
                FullAjocBlocker::ActiveDialogueEnhancement { .. }
            ))
        )
    ) {
        DMX_DE_SYNTAX
    } else {
        decode_error_syntax_path(kind, unsupported)
    }
}

const fn decode_error_syntax_path(
    kind: FullAjocDecodeErrorKind,
    unsupported: Option<FullAjocUnsupported>,
) -> &'static str {
    match (kind, unsupported) {
        (FullAjocDecodeErrorKind::OamdState, _) => OAMD_SYNTAX,
        (
            FullAjocDecodeErrorKind::Unsupported,
            Some(FullAjocUnsupported::Full(FullAjocBlocker::AlternativeObjectMetadata)),
        ) => OAMD_SYNTAX,
        (FullAjocDecodeErrorKind::Unsupported, Some(FullAjocUnsupported::Aspx(_))) => ASPX_SYNTAX,
        _ => FULL_SYNTAX,
    }
}

const fn audio_substream_bit_offset(error: AudioSubstreamError) -> Option<u64> {
    match error {
        AudioSubstreamError::Read(error) => Some(read_bit_offset(error)),
        AudioSubstreamError::Emdf(error) => emdf_bit_offset(error),
        AudioSubstreamError::Oamd(error) => oamd_bit_offset(error),
        AudioSubstreamError::InvalidExtensionSize { bit_position, .. }
        | AudioSubstreamError::InvalidToolsMetadataSize { bit_position, .. }
        | AudioSubstreamError::TrailingToolsMetadataBits { bit_position, .. }
        | AudioSubstreamError::InvalidDialogEnhancementChannelConfiguration {
            bit_position, ..
        }
        | AudioSubstreamError::Unsupported { bit_position, .. } => Some(bit_position),
        AudioSubstreamError::AudioSizeOutOfRange { .. }
        | AudioSubstreamError::TrailingBits { .. } => None,
    }
}

const fn emdf_bit_offset(error: EmdfError) -> Option<u64> {
    match error {
        EmdfError::Read(error) => Some(read_bit_offset(error)),
        EmdfError::MissingTerminator { bit_position, .. }
        | EmdfError::TooManyPayloads { bit_position, .. }
        | EmdfError::PayloadTooLarge { bit_position, .. } => Some(bit_position),
    }
}

const fn audio_data_bit_offset(error: AudioDataError) -> Option<u64> {
    match error {
        AudioDataError::Read(error) => Some(read_bit_offset(error)),
        AudioDataError::VarElement(error) => var_element_bit_offset(error),
        AudioDataError::Ajoc(error) => ajoc_bit_offset(error),
        AudioDataError::AjocDe(error) => ajoc_de_bit_offset(error),
        AudioDataError::Oamd(error) => oamd_bit_offset(error),
        _ => None,
    }
}

const fn var_element_bit_offset(error: VarElementError) -> Option<u64> {
    match error {
        VarElementError::Read(error) => Some(read_bit_offset(error)),
        VarElementError::Channel(error) => channel_bit_offset(error),
        VarElementError::Aspx(error) => aspx_bit_offset(error),
        _ => None,
    }
}

const fn channel_bit_offset(error: ChannelError) -> Option<u64> {
    match error {
        ChannelError::Read(error) => Some(read_bit_offset(error)),
        ChannelError::Huffman(error) => huffman_bit_offset(error),
        ChannelError::Framing(error) => asf_bit_offset(error),
        ChannelError::Spectrum(error) => asf_spectrum_bit_offset(error),
        _ => None,
    }
}

const fn asf_bit_offset(error: AsfError) -> Option<u64> {
    match error {
        AsfError::Read(error) => Some(read_bit_offset(error)),
        _ => None,
    }
}

const fn asf_spectrum_bit_offset(error: AsfSpectrumError) -> Option<u64> {
    match error {
        AsfSpectrumError::Read(error) => Some(read_bit_offset(error)),
        AsfSpectrumError::Huffman(error) => huffman_bit_offset(error),
        AsfSpectrumError::Framing(error) => asf_bit_offset(error),
        _ => None,
    }
}

const fn aspx_bit_offset(error: AspxError) -> Option<u64> {
    match error {
        AspxError::Read(error) => Some(read_bit_offset(error)),
        AspxError::Huffman(error) => huffman_bit_offset(error),
        _ => None,
    }
}

const fn ajoc_bit_offset(error: AjocError) -> Option<u64> {
    match error {
        AjocError::Read(error) => Some(read_bit_offset(error)),
        AjocError::Huffman(error) => huffman_bit_offset(error),
        _ => None,
    }
}

const fn ajoc_de_bit_offset(error: AjocDeError) -> Option<u64> {
    match error {
        AjocDeError::Read(error) => Some(read_bit_offset(error)),
        _ => None,
    }
}

const fn huffman_bit_offset(error: HuffmanError) -> Option<u64> {
    match error {
        HuffmanError::Read(error) => Some(read_bit_offset(error)),
        HuffmanError::MalformedTable => None,
    }
}

const fn oamd_bit_offset(error: OamdError) -> Option<u64> {
    match error {
        OamdError::Read(error) => Some(read_bit_offset(error)),
        _ => None,
    }
}

const fn read_bit_offset(error: ReadError) -> u64 {
    match error {
        ReadError::OutOfBounds { bit_position, .. }
        | ReadError::WidthUnsupported { bit_position, .. }
        | ReadError::ValueOverflow { bit_position } => bit_position,
    }
}

const fn with_bit_offset(
    context: DecodeErrorContext,
    bit_offset: Option<u64>,
) -> DecodeErrorContext {
    match bit_offset {
        Some(bit_offset) => context.with_bit_offset(bit_offset),
        None => context,
    }
}

fn provenance(
    context: AccessUnitContext,
    presentation: ResolvedPresentation,
    group_oamd: &PreparedGroupOamd,
) -> Result<FullAjocFrameProvenance, DecodeError> {
    let mut provenance = FullAjocFrameProvenance::new(context.index());
    if let Some(value) = context.source_sample_start() {
        provenance = provenance.with_source_sample_start(value);
    }
    if let Some(value) = context.presentation_sample_start() {
        provenance = provenance.with_presentation_sample_start(value);
    }
    if let Some(value) = context.priming_samples() {
        provenance = provenance.with_priming_samples(value);
    }
    if let Some(value) = context.random_access_hint() {
        provenance = provenance.with_random_access_hint(value);
    }
    provenance = provenance.with_discontinuity(context.discontinuity());
    for group in group_oamd.groups() {
        let state = FullAjocGroupOamdState::new(
            group.group_index,
            group.effective_common,
            group.common_updated_in_source_access_unit,
            group.effective_timing,
            group.timing_updated_in_source_access_unit,
        );
        provenance = provenance.try_with_group_oamd_state(state).ok_or_else(|| {
            DecodeError::new(
                DecodeErrorKind::InternalInvariant {
                    stage: DecodeStage::Oamd,
                },
                error_context(context, presentation)
                    .with_group(group.group_index)
                    .with_syntax_path(GROUP_OAMD_SYNTAX),
            )
        })?;
    }
    Ok(provenance)
}

fn context_error(
    error: SubstreamAudioError,
    access_unit: AccessUnitContext,
    presentation: ResolvedPresentation,
) -> DecodeError {
    let kind = context_kind(error, presentation);
    DecodeError::new(
        kind,
        error_context(access_unit, presentation).with_syntax_path(AJOC_INFO_SYNTAX),
    )
}

fn context_kind(error: SubstreamAudioError, presentation: ResolvedPresentation) -> DecodeErrorKind {
    match error {
        SubstreamAudioError::StaticDownmixUnsupported => {
            DecodeErrorKind::Unsupported(UnsupportedReason::StaticDownmix)
        }
        SubstreamAudioError::SamplingFrequencyUnsupported {
            sampling_frequency_hz,
        } => DecodeErrorKind::Unsupported(UnsupportedReason::SamplingFrequency {
            sampling_frequency_hz,
        }),
        SubstreamAudioError::DownmixSignalsOutOfRange { declared } => {
            DecodeErrorKind::Unsupported(UnsupportedReason::FullbandDownmixSignalsExceeded {
                declared,
                limit: MAX_FULLBAND_DMX_SIGNALS,
            })
        }
        SubstreamAudioError::Oamd(OamdError::TooManyObjects { limit }) => {
            let declared = presentation
                .ajoc_info
                .n_dmx_signals
                .max(presentation.ajoc_info.n_upmix_signals)
                .saturating_add(u32::from(presentation.ajoc_info.b_lfe));
            DecodeErrorKind::Unsupported(UnsupportedReason::AjocObjectsExceeded { declared, limit })
        }
        SubstreamAudioError::FrameLengthUnavailable {
            fs_index,
            frame_rate_index,
            frame_rate_factor,
        } => DecodeErrorKind::InvalidBitstream(BitstreamFailure::FrameLengthUnavailable {
            fs_index,
            frame_rate_index,
            frame_rate_factor,
        }),
        SubstreamAudioError::UnsupportedBitstreamVersion { bitstream_version }
            if bitstream_version < 2 =>
        {
            DecodeErrorKind::Unsupported(UnsupportedReason::LegacyBitstreamVersion {
                bitstream_version,
            })
        }
        SubstreamAudioError::UnsupportedBitstreamVersion { bitstream_version } => {
            DecodeErrorKind::Unsupported(UnsupportedReason::FutureBitstreamVersion {
                bitstream_version,
            })
        }
        SubstreamAudioError::MultiSubstreamFrameRateUnsupported { frame_rate_factor } => {
            DecodeErrorKind::Unsupported(UnsupportedReason::MultiSubstreamFrameRate {
                frame_rate_factor,
            })
        }
        SubstreamAudioError::FragmentedFrameRateUnsupported {
            frame_rate_fraction,
        } => DecodeErrorKind::Unsupported(UnsupportedReason::FragmentedFrameRate {
            frame_rate_fraction,
        }),
        SubstreamAudioError::Oamd(error) => {
            DecodeErrorKind::InvalidBitstream(BitstreamFailure::Oamd(error))
        }
        SubstreamAudioError::Substream(_) | SubstreamAudioError::AudioData(_) => {
            DecodeErrorKind::InternalInvariant {
                stage: DecodeStage::AudioSyntax,
            }
        }
    }
}

fn error_context(
    access_unit: AccessUnitContext,
    presentation: ResolvedPresentation,
) -> DecodeErrorContext {
    let mut context = DecodeErrorContext::for_access_unit(access_unit.index())
        .with_presentation(presentation.index, presentation.id)
        .with_substream(presentation.substream_index);
    if presentation.group_mask != 0 {
        context = context.with_group(presentation.group_mask.trailing_zeros());
    }
    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use macindecode_ac4_decode::full_ajoc::AspxBlocker;

    #[test]
    fn decode_error_path_follows_the_failing_engine_stage() {
        assert_eq!(
            decode_error_syntax_path(FullAjocDecodeErrorKind::OamdState, None),
            OAMD_SYNTAX
        );
        assert_eq!(
            decode_error_syntax_path(
                FullAjocDecodeErrorKind::Unsupported,
                Some(FullAjocUnsupported::Aspx(AspxBlocker::ActiveCompanding))
            ),
            ASPX_SYNTAX
        );
        assert_eq!(
            decode_error_syntax_path(FullAjocDecodeErrorKind::Reconstruction, None),
            FULL_SYNTAX
        );
    }

    #[test]
    fn core_dialogue_enhancement_keeps_a_scene_specific_reason_and_path() {
        let blocker = FullAjocUnsupported::Full(FullAjocBlocker::ActiveDialogueEnhancement {
            dialogue_objects: 2,
        });
        assert_eq!(
            map_unsupported_reason(FullAjocDecodeMode::RequireCore, Some(blocker)),
            UnsupportedReason::CoreDialogueEnhancement {
                dialogue_objects: 2,
            }
        );
        assert_eq!(
            decode_error_syntax_path_for_mode(
                FullAjocDecodeErrorKind::Unsupported,
                Some(blocker),
                FullAjocDecodeMode::RequireCore,
            ),
            DMX_DE_SYNTAX
        );
        assert_eq!(
            map_unsupported_reason(FullAjocDecodeMode::RequireFull, Some(blocker)),
            UnsupportedReason::FullAjoc(blocker)
        );
    }

    #[test]
    fn alternative_oamd_keeps_its_scene_reason_and_syntax_path() {
        let blocker = FullAjocUnsupported::Full(FullAjocBlocker::AlternativeObjectMetadata);
        for mode in [
            FullAjocDecodeMode::RequireCore,
            FullAjocDecodeMode::RequireFull,
        ] {
            assert_eq!(
                map_unsupported_reason(mode, Some(blocker)),
                UnsupportedReason::AlternativeObjectMetadata
            );
            assert_eq!(
                decode_error_syntax_path_for_mode(
                    FullAjocDecodeErrorKind::Unsupported,
                    Some(blocker),
                    mode,
                ),
                OAMD_SYNTAX
            );
        }
    }

    #[test]
    fn nested_audio_read_errors_keep_their_bit_offset() {
        let read = ReadError::OutOfBounds {
            requested_bits: 7,
            bit_position: 37,
            remaining_bits: 3,
        };
        let errors = [
            AudioDataError::Read(read),
            AudioDataError::VarElement(VarElementError::Read(read)),
            AudioDataError::VarElement(VarElementError::Channel(ChannelError::Read(read))),
            AudioDataError::VarElement(VarElementError::Channel(ChannelError::Huffman(
                HuffmanError::Read(read),
            ))),
            AudioDataError::VarElement(VarElementError::Channel(ChannelError::Framing(
                AsfError::Read(read),
            ))),
            AudioDataError::VarElement(VarElementError::Channel(ChannelError::Spectrum(
                AsfSpectrumError::Read(read),
            ))),
            AudioDataError::VarElement(VarElementError::Channel(ChannelError::Spectrum(
                AsfSpectrumError::Huffman(HuffmanError::Read(read)),
            ))),
            AudioDataError::VarElement(VarElementError::Channel(ChannelError::Spectrum(
                AsfSpectrumError::Framing(AsfError::Read(read)),
            ))),
            AudioDataError::VarElement(VarElementError::Aspx(AspxError::Read(read))),
            AudioDataError::VarElement(VarElementError::Aspx(AspxError::Huffman(
                HuffmanError::Read(read),
            ))),
            AudioDataError::Ajoc(AjocError::Read(read)),
            AudioDataError::Ajoc(AjocError::Huffman(HuffmanError::Read(read))),
            AudioDataError::AjocDe(AjocDeError::Read(read)),
            AudioDataError::Oamd(OamdError::Read(read)),
        ];

        for error in errors {
            assert_eq!(audio_data_bit_offset(error), Some(37), "{error:?}");
        }

        assert_eq!(
            audio_substream_bit_offset(AudioSubstreamError::Emdf(EmdfError::Read(read))),
            Some(37)
        );
        assert_eq!(
            audio_substream_bit_offset(AudioSubstreamError::Emdf(EmdfError::MissingTerminator {
                bit_position: 41,
                remaining_bits: 0,
            })),
            Some(41)
        );
    }
}
