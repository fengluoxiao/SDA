//! Scene 解码会话的控制面与 presentation 解析。

#[cfg(any(feature = "audio-decode", test))]
use alloc::vec::Vec;
use macindecode_ac4_bitstream::{
    Ac4PresentationSubstream, Ac4PresentationV1Info, PresentationDrcState,
    PresentationSubstreamGroupGainState,
    substream::{ObjectAssignment, SubstreamInfo, SubstreamInfoAjoc},
    topology::{
        Ac4Topology, DecoderAction, MAX_PRESENTATIONS, MAX_SUBSTREAMS, ResetReason, TopologyError,
        TopologyStateMachine, TopologyTransition, validate_group_references,
        validate_substream_references,
    },
};
use macindecode_ac4_bitstream::{PresentationSubstreamContext, PresentationSubstreamError};

#[cfg(feature = "audio-decode")]
use crate::DecodeStatus;
#[cfg(feature = "audio-decode")]
use crate::model::{CoreBandPcmChannelStorage, CoreBandPcmFrameStorage};
use crate::{
    Ac4DecoderConfig, AccessUnit, AccessUnitContext, DecodeError, DecodeErrorContext,
    DecodeErrorKind, DecodeMode, DecodeStage, DecodedAccessUnit, PresentationSelection,
    PresentationSelectionError, ResetKind, UnsupportedReason,
    group_oamd::{ErrorScope, GroupOamdDecoder, PreparedGroupOamd},
    model::{PresentationSubstreamMetadataRecord, PresentationSubstreamMetadataStorage},
};
#[cfg(feature = "audio-decode")]
use crate::{
    assembly::{FullPcmAssemblyInput, SceneAssembler},
    full_engine::{decode_full_ajoc_frame, prepare_full_ajoc_input},
    model::SceneFrameStorage,
};
#[cfg(feature = "audio-decode")]
use macindecode_ac4_decode::full_ajoc::{
    DecodedFullAjocAsfFrame, DecodedFullAjocAudioFrame, FullAjocAudioFrameInput, FullAjocDecoder,
};

const PRESENTATION_SYNTAX: &str = "raw_ac4_frame/ac4_toc/ac4_presentation_info";
const GROUP_SYNTAX: &str = "raw_ac4_frame/ac4_toc/ac4_presentation_info/ac4_substream_group_info";
const PRESENTATION_SUBSTREAM_SYNTAX: &str = "raw_ac4_frame/ac4_presentation_substream";

fn parse_presentation_with_tail<'a>(payload: &'a [u8], context: PresentationSubstreamContext,
    state: &mut PresentationDrcState, allow_opaque_tail: bool)
    -> Result<(Ac4PresentationSubstream<'a>, usize), PresentationSubstreamError> {
    match Ac4PresentationSubstream::parse_with_drc_state_compat(payload, context, state) {
        Err(error @ PresentationSubstreamError::TrailingBits { remaining_bits: 8 })
            if allow_opaque_tail && context.presentation_is_independent() && context.pres_ch_mode_undefined() => {
            let Some((_, syntax)) = payload.split_last() else { return Err(error); };
            let mut candidate = *state;
            let parsed = Ac4PresentationSubstream::parse_with_drc_state(syntax, context, &mut candidate)?;
            if !parsed.drc_present || parsed.drc_configuration.is_none() { return Err(error); }
            *state = candidate;
            Ok((parsed, syntax.len()))
        },
        result => result,
    }
}
/// object/A-JOC 生产链在 independent DRC 帧规范语法之后写入的已观测尾字节。
#[cfg(test)]
const INDEPENDENT_OBJECT_DRC_COMPATIBILITY_BYTES: [u8; 2] = [0x00, 0x80];

/// 所选 presentation 的解析历史与当前 AU 可见存储。
#[derive(Debug, Default)]
struct PresentationMetadataState {
    drc: PresentationDrcState,
    group_gain: PresentationSubstreamGroupGainState,
    storage: PresentationSubstreamMetadataStorage,
}

impl PresentationMetadataState {
    fn clear_view(&mut self) {
        self.storage.clear();
    }

    fn reset(&mut self) {
        self.drc.reset();
        self.group_gain.reset();
        self.storage.clear();
    }
}

/// 一个连续 AC-4 时间线的 Scene 解码会话。
///
/// Session 拥有全部跨帧状态、A-JOC engine 及输出缓冲。
/// `mark_discontinuity` 清除可继承历史并等待完整随机访问点；`reset` 还会清除
/// 配置状态。`decode_access_unit` 返回的借用帧有效至下一次 Session 可变调用。
#[derive(Debug)]
pub struct Ac4DecoderSession {
    config: Ac4DecoderConfig,
    topology: TopologyStateMachine,
    group_oamd: GroupOamdDecoder,
    presentation_metadata: PresentationMetadataState,
    #[cfg(feature = "audio-decode")]
    full_decoder: FullAjocDecoder,
    #[cfg(feature = "audio-decode")]
    scene_assembler: SceneAssembler,
    #[cfg(feature = "audio-decode")]
    output_frames: Vec<SceneFrameStorage>,
    #[cfg(feature = "audio-decode")]
    core_band_pcm: CoreBandPcmFrameStorage,
    output_frame_count: usize,
    reset_required: bool,
}

impl Ac4DecoderSession {
    /// 创建一个尚未观察到配置的解码会话。
    #[must_use]
    pub fn new(config: Ac4DecoderConfig) -> Self {
        Self {
            config,
            topology: TopologyStateMachine::new(),
            group_oamd: GroupOamdDecoder::new(),
            presentation_metadata: PresentationMetadataState::default(),
            #[cfg(feature = "audio-decode")]
            full_decoder: FullAjocDecoder::new(),
            #[cfg(feature = "audio-decode")]
            scene_assembler: SceneAssembler::new(),
            #[cfg(feature = "audio-decode")]
            output_frames: Vec::new(),
            #[cfg(feature = "audio-decode")]
            core_band_pcm: CoreBandPcmFrameStorage::default(),
            output_frame_count: 0,
            reset_required: false,
        }
    }

    /// 创建 Session 时固定的配置。
    #[must_use]
    pub const fn config(&self) -> Ac4DecoderConfig {
        self.config
    }

    /// 解码一个已经由调用方定界、且不含 sync wrapper 的 `raw_ac4_frame`。
    ///
    /// 一个 AU 可以产生零个或多个借用的 [`crate::Ac4SceneFrame`]。返回值借用
    /// Session 自持的 PCM、元素和元数据存储；Rust 生命周期会阻止调用方在仍持有
    /// 结果时再次可变调用本 Session。等待完整随机访问点不是错误，而是返回
    /// [`crate::DecodeStatus::WaitingForRandomAccess`] 与空帧序列。
    ///
    /// # Errors
    ///
    /// TOC 或内联 control 解析在边界验证前耗尽输入时，返回可重试的
    /// [`DecodeErrorKind::NeedMoreData`]；完整 TOC 已证明声明 payload 越过 bounded AU
    /// 时返回 [`DecodeErrorKind::InvalidBitstream`]。不支持的拓扑、坏帧、DSP 失败或内部
    /// 不变量分别返回对应的结构化错误。未启用 `audio-decode` feature 时，本入口返回
    /// [`UnsupportedReason::AudioDecodeFeatureDisabled`]。
    pub fn decode_access_unit<'session>(
        &'session mut self,
        access_unit: AccessUnit<'_>,
    ) -> Result<DecodedAccessUnit<'session>, DecodeError> {
        #[cfg(not(feature = "audio-decode"))]
        {
            let error = DecodeError::new(
                DecodeErrorKind::Unsupported(UnsupportedReason::AudioDecodeFeatureDisabled),
                DecodeErrorContext::for_access_unit(access_unit.context().index())
                    .with_syntax_path("Ac4DecoderSession/decode_access_unit"),
            );
            self.apply_error_policy(error);
            Err(error)
        }

        #[cfg(feature = "audio-decode")]
        {
            let access_unit_index = access_unit.context().index();
            match self.prepare_access_unit(access_unit)? {
                AccessUnitPreflight::WaitingForRandomAccess { reason } => {
                    if self.output_frame_count > self.output_frames.len() {
                        let error = output_storage_error(access_unit_index);
                        self.apply_error_policy(error);
                        return Err(error);
                    }
                    let frames = self
                        .output_frames
                        .get(..self.output_frame_count)
                        .ok_or_else(|| output_storage_error(access_unit_index))?;
                    Ok(DecodedAccessUnit::new(
                        DecodeStatus::WaitingForRandomAccess { reason },
                        frames,
                        None,
                        #[cfg(feature = "audio-decode")]
                        None,
                    ))
                }
                AccessUnitPreflight::Ready(prepared) => {
                    self.decode_and_assemble_engine_frame(&prepared)?;
                    if self.output_frame_count > self.output_frames.len() {
                        let error = prepared_output_storage_error(&prepared);
                        self.apply_error_policy(error);
                        return Err(error);
                    }
                    self.commit_prepared(prepared);
                    let frames = self
                        .output_frames
                        .get(..self.output_frame_count)
                        .ok_or_else(|| output_storage_error(access_unit_index))?;
                    let core_band_pcm = self
                        .config
                        .core_band_diagnostics()
                        .then_some(&self.core_band_pcm);
                    Ok(DecodedAccessUnit::new(
                        DecodeStatus::Decoded,
                        frames,
                        self.presentation_metadata.storage.view(),
                        #[cfg(feature = "audio-decode")]
                        core_band_pcm,
                    ))
                }
            }
        }
    }

    /// 报告调用方时间线中的 seek、splice 或来源不连续。
    ///
    /// 已排队输出与全部可继承解码历史都会失效；后续解码必须等待完整随机访问点。
    pub fn mark_discontinuity(&mut self) {
        self.output_frame_count = 0;
        self.group_oamd.reset();
        self.presentation_metadata.reset();
        #[cfg(feature = "audio-decode")]
        {
            self.full_decoder.reset();
            self.scene_assembler.mark_discontinuity();
            self.core_band_pcm.clear_samples();
        }
        self.topology
            .mark_discontinuity(ResetReason::ExternalDiscontinuity);
    }

    /// 显式恢复一个全新的解码状态，同时保留已经分配的缓冲容量。
    pub fn reset(&mut self) {
        self.output_frame_count = 0;
        self.group_oamd.reset();
        self.presentation_metadata.reset();
        #[cfg(feature = "audio-decode")]
        {
            self.full_decoder.reset();
            self.scene_assembler.reset();
            self.core_band_pcm.reset();
        }
        self.topology = TopologyStateMachine::new();
        self.reset_required = false;
    }

    /// 在任何跨帧 DSP 状态改写前，事务性解析并验证 AU 的控制面。
    ///
    /// `Ready` 只携带候选拓扑状态；调用方必须等音频语法、DSP 与场景组装全部
    /// 成功后再调用 `commit_prepared`。边界验证前的读取耗尽不改变 Session，因而同一
    /// AU 可在补齐后重试；已验证 bounded AU 的声明 payload 越界则按无效码流处理。
    /// 等待随机访问点不需要运行 DSP，可以立即提交门禁状态。
    #[cfg_attr(
        not(any(test, feature = "audio-decode")),
        expect(dead_code, reason = "无 audio-decode 时公开入口不进入控制面预检")
    )]
    fn prepare_access_unit<'frame>(
        &mut self,
        access_unit: AccessUnit<'frame>,
    ) -> Result<AccessUnitPreflight<'frame>, DecodeError> {
        self.presentation_metadata.clear_view();
        let context = access_unit.context();
        if self.reset_required {
            return Err(DecodeError::new(
                DecodeErrorKind::ResetRequired,
                DecodeErrorContext::for_access_unit(context.index()),
            ));
        }

        let topology = match Ac4Topology::parse(access_unit.raw_frame()) {
            Ok(topology) => topology,
            Err(error) => {
                let error = DecodeError::from_topology(error, context.index());
                self.apply_error_policy(error);
                return Err(error);
            }
        };

        // 尺寸按 substream 下标累积；末项能够落入有界 AU，才说明整段载荷区完整。
        // 这项是 raw AU 的先决条件，不应被 presentation 选择或支持门禁遮蔽。
        let terminal_substream_index = topology.index_table.n_substreams.saturating_sub(1);
        if let Err(error) =
            topology.substream_payload(access_unit.raw_frame(), terminal_substream_index)
        {
            let error = DecodeError::from_topology(error, context.index());
            self.apply_error_policy(error);
            return Err(error);
        }

        let mut next_topology = self.topology;
        if context.discontinuity() {
            next_topology.mark_discontinuity(ResetReason::ExternalDiscontinuity);
        }
        let transition = next_topology.observe(&topology);

        let presentation = match resolve_presentation(
            &topology,
            self.config.presentation(),
            self.config.decode_mode(),
            context.index(),
        ) {
            Ok(presentation) => presentation,
            Err(error) => {
                self.apply_error_policy(error);
                return Err(error);
            }
        };

        match transition.action {
            DecoderAction::WaitForRandomAccess { reason } => {
                self.output_frame_count = 0;
                self.group_oamd.reset();
                self.presentation_metadata.reset();
                #[cfg(feature = "audio-decode")]
                {
                    self.full_decoder.reset();
                    self.scene_assembler.mark_discontinuity();
                    self.core_band_pcm.clear_samples();
                }
                self.topology = next_topology;
                Ok(AccessUnitPreflight::WaitingForRandomAccess {
                    reason: reset_kind(reason),
                })
            }
            DecoderAction::Continue | DecoderAction::Reset { .. } => {
                let reset_history = matches!(transition.action, DecoderAction::Reset { .. });
                let presentation_metadata = match self.prepare_presentation_metadata(
                    access_unit.raw_frame(),
                    &topology,
                    presentation,
                    context,
                    reset_history,
                ) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.apply_error_policy(error);
                        return Err(error);
                    }
                };
                let group_oamd = match self.group_oamd.prepare(
                    access_unit.raw_frame(),
                    &topology,
                    presentation.group_mask,
                    reset_history,
                    ErrorScope {
                        access_unit_index: context.index(),
                        presentation_index: presentation.index,
                        presentation_id: presentation.id,
                    },
                ) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.apply_error_policy(error);
                        return Err(error);
                    }
                };
                #[cfg(feature = "audio-decode")]
                let engine_input = match prepare_full_ajoc_input(
                    access_unit.raw_frame(),
                    context,
                    &topology,
                    presentation,
                    self.config.decode_mode(),
                    &group_oamd,
                ) {
                    Ok(input) => input,
                    Err(error) => {
                        self.apply_error_policy(error);
                        return Err(error);
                    }
                };
                Ok(AccessUnitPreflight::Ready(PreparedAccessUnit {
                    raw_frame: access_unit.raw_frame(),
                    context,
                    topology,
                    presentation,
                    transition,
                    group_oamd,
                    presentation_metadata,
                    #[cfg(feature = "audio-decode")]
                    engine_input,
                    next_topology,
                }))
            }
        }
    }

    /// 验证所选 presentation substream，并形成尚未发布的状态候选。
    ///
    /// payload 继续借用当前输入 AU；Session 只在这里预留复制容量，等音频 DSP 与 Scene
    /// 组装成功后才由 `commit_prepared` 复制、提交状态并发布借用侧车。
    fn prepare_presentation_metadata<'frame>(
        &mut self,
        raw_frame: &'frame [u8],
        topology: &Ac4Topology,
        presentation: ResolvedPresentation,
        access_unit_context: AccessUnitContext,
        reset_history: bool,
    ) -> Result<Option<PreparedPresentationMetadata<'frame>>, DecodeError> {
        let presentation_position = usize::try_from(presentation.index).unwrap_or(usize::MAX);
        let selected = topology
            .presentations()
            .get(presentation_position)
            .ok_or_else(|| {
                presentation_metadata_internal_error(
                    access_unit_context.index(),
                    presentation,
                    None,
                    DecodeStage::Topology,
                )
            })?;
        let Some(substream) = selected.substream else {
            return Ok(None);
        };
        let parse_context = topology
            .presentation_substream_context(presentation_position)
            .ok_or_else(|| {
                presentation_metadata_internal_error(
                    access_unit_context.index(),
                    presentation,
                    Some(substream.substream_index),
                    DecodeStage::Topology,
                )
            })?;
        let payload = topology
            .substream_payload(raw_frame, substream.substream_index)
            .map_err(|error| {
                scoped_presentation_topology_error(
                    error,
                    access_unit_context.index(),
                    presentation,
                    substream.substream_index,
                )
            })?;

        let replay_drc_state = if reset_history {
            PresentationDrcState::new()
        } else {
            self.presentation_metadata.drc
        };
        let mut next_drc_state = replay_drc_state;
        let (parsed, syntax_payload_len) = parse_presentation_with_tail(
            payload,
            parse_context,
            &mut next_drc_state,
            self.config.opaque_presentation_tail,
        )
        .map_err(|error| {
            DecodeError::from_presentation_substream(
                error,
                access_unit_context.index(),
                presentation.index,
                presentation.id,
                substream.substream_index,
            )
        })?;

        let mut next_group_gain_state = if reset_history {
            PresentationSubstreamGroupGainState::new()
        } else {
            self.presentation_metadata.group_gain
        };
        let effective_group_gain_codes = next_group_gain_state
            .apply(parsed.substream_group_gain_update, parse_context)
            .map_err(|error| {
                DecodeError::from_presentation_group_gain(
                    error,
                    access_unit_context.index(),
                    presentation.index,
                    presentation.id,
                    substream.substream_index,
                )
            })?;

        self.presentation_metadata
            .storage
            .try_reserve_payload(payload.len())
            .map_err(|()| {
                DecodeError::new(
                    DecodeErrorKind::DecodeFailure {
                        stage: DecodeStage::AudioSyntax,
                    },
                    DecodeErrorContext::for_access_unit(access_unit_context.index())
                        .with_presentation(presentation.index, presentation.id)
                        .with_substream(substream.substream_index)
                        .with_syntax_path("DecodedAccessUnit/presentation_metadata"),
                )
            })?;

        Ok(Some(PreparedPresentationMetadata {
            payload,
            record: PresentationSubstreamMetadataRecord {
                access_unit_index: access_unit_context.index(),
                presentation_index: presentation.index,
                presentation_id: presentation.id,
                substream_index: substream.substream_index,
                context: parse_context,
                syntax_payload_len,
                replay_drc_state,
                effective_drc_configuration: next_drc_state.configuration(),
                effective_group_gain_codes,
            },
            next_drc_state,
            next_group_gain_state,
        }))
    }

    /// 驱动当前 AU 候选对应的唯一 A-JOC engine。
    ///
    /// 成功会提交 engine 自身的 DSP/FIFO 状态，但不会提交 topology 与 group OAMD；
    /// 调用方必须先把借用输出完整组装进 Session 存储，再释放返回值并调用
    /// `commit_prepared`。若组装失败，必须走 `apply_error_policy` 使两侧历史同时
    /// 失效，不能让下一 AU 跨过未发布的半事务继续。
    #[cfg(feature = "audio-decode")]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "下一提交的 SceneFrame 组装会消费借用的 engine 输出"
        )
    )]
    fn decode_engine_frame<'decoder>(
        &'decoder mut self,
        prepared: &PreparedAccessUnit<'_>,
    ) -> Result<DecodedFullAjocAudioFrame<'decoder>, DecodeError> {
        let Self {
            topology,
            group_oamd,
            presentation_metadata,
            full_decoder,
            scene_assembler,
            output_frame_count,
            reset_required,
            ..
        } = self;
        *output_frame_count = 0;
        if matches!(prepared.transition.action, DecoderAction::Reset { .. }) {
            full_decoder.reset();
        }
        match decode_full_ajoc_frame(
            full_decoder,
            prepared.engine_input,
            prepared.context,
            prepared.presentation,
        ) {
            Ok(decoded) => Ok(decoded),
            Err(error) => {
                // `decode_audio_frame` 已原子清空当前物理 substream 的语法、DSP 与
                // FIFO；这里只处理与它互不借用的 Session 控制状态。
                apply_control_error_policy(
                    error,
                    topology,
                    group_oamd,
                    presentation_metadata,
                    output_frame_count,
                    reset_required,
                );
                scene_assembler.mark_discontinuity();
                Err(error)
            }
        }
    }

    /// 驱动 A-JOC engine，并在同一 Session 可变调用内复制成 normalized Scene PCM。
    ///
    /// engine 成功后若 Scene 形状或数值不变量失败，会同步清空 DSP、控制与输出
    /// 可见性；调用方不能观察到半帧。topology/group OAMD 仍由 `commit_prepared`
    /// 在本方法成功返回后统一提交。
    #[cfg(feature = "audio-decode")]
    fn decode_and_assemble_engine_frame(
        &mut self,
        prepared: &PreparedAccessUnit<'_>,
    ) -> Result<(), DecodeError> {
        let Self {
            config,
            topology,
            group_oamd,
            presentation_metadata,
            full_decoder,
            scene_assembler,
            output_frames,
            core_band_pcm,
            output_frame_count,
            reset_required,
            ..
        } = self;
        *output_frame_count = 0;
        if matches!(prepared.transition.action, DecoderAction::Reset { .. }) {
            full_decoder.reset();
        }
        let context = prepared.engine_input.syntax.context.params.context;
        let assembled = match decode_full_ajoc_frame(
            full_decoder,
            prepared.engine_input,
            prepared.context,
            prepared.presentation,
        ) {
            Ok(decoded) => {
                let sidecar = if config.core_band_diagnostics() {
                    copy_core_band_pcm(core_band_pcm, decoded.frontend().asf(), prepared)
                } else {
                    Ok(())
                };
                match sidecar {
                    Ok(()) => scene_assembler.assemble_pcm(
                        output_frames,
                        FullPcmAssemblyInput {
                            topology: &prepared.topology,
                            context: prepared.context,
                            presentation: prepared.presentation,
                            mode: config.decode_mode(),
                            transition: prepared.transition,
                            frame_length: context.frame_len_base,
                            sampling_frequency_hz: context.sampling_frequency_hz,
                        },
                        decoded.output(),
                    ),
                    Err(error) => Err(error),
                }
            }
            Err(error) => {
                core_band_pcm.reset();
                apply_control_error_policy(
                    error,
                    topology,
                    group_oamd,
                    presentation_metadata,
                    output_frame_count,
                    reset_required,
                );
                scene_assembler.mark_discontinuity();
                return Err(error);
            }
        };
        match assembled {
            Ok(frame_count) => {
                *output_frame_count = frame_count;
                Ok(())
            }
            Err(error) => {
                full_decoder.reset();
                core_band_pcm.reset();
                apply_control_error_policy(
                    error,
                    topology,
                    group_oamd,
                    presentation_metadata,
                    output_frame_count,
                    reset_required,
                );
                scene_assembler.mark_discontinuity();
                Err(error)
            }
        }
    }

    /// 提交一个已经完成全部可能失败工作的 AU 候选。
    #[cfg_attr(
        not(any(test, feature = "audio-decode")),
        expect(dead_code, reason = "无 audio-decode 时公开入口没有可提交的 DSP 候选")
    )]
    fn commit_prepared(&mut self, prepared: PreparedAccessUnit<'_>) {
        self.group_oamd.commit(&prepared.group_oamd);
        if let Some(metadata) = prepared.presentation_metadata {
            self.presentation_metadata.drc = metadata.next_drc_state;
            self.presentation_metadata.group_gain = metadata.next_group_gain_state;
            self.presentation_metadata
                .storage
                .publish(metadata.payload, metadata.record);
        } else {
            self.presentation_metadata.reset();
        }
        self.topology = prepared.next_topology;
    }

    fn apply_error_policy(&mut self, error: DecodeError) {
        let policy = error_policy(error);
        if let Some(reason) = policy.invalidation {
            self.invalidate_history(reason);
        }
        if policy.reset_required {
            self.reset_required = true;
        }
    }

    fn invalidate_history(&mut self, reason: ResetReason) {
        self.output_frame_count = 0;
        self.group_oamd.reset();
        self.presentation_metadata.reset();
        #[cfg(feature = "audio-decode")]
        {
            self.full_decoder.reset();
            self.scene_assembler.mark_discontinuity();
            self.core_band_pcm.clear_samples();
        }
        self.topology.mark_discontinuity(reason);
    }
}

fn presentation_metadata_internal_error(
    access_unit_index: u64,
    presentation: ResolvedPresentation,
    substream_index: Option<u32>,
    stage: DecodeStage,
) -> DecodeError {
    let mut context = DecodeErrorContext::for_access_unit(access_unit_index)
        .with_presentation(presentation.index, presentation.id)
        .with_syntax_path(PRESENTATION_SUBSTREAM_SYNTAX);
    if let Some(substream_index) = substream_index {
        context = context.with_substream(substream_index);
    }
    DecodeError::new(DecodeErrorKind::InternalInvariant { stage }, context)
}

fn scoped_presentation_topology_error(
    error: TopologyError,
    access_unit_index: u64,
    presentation: ResolvedPresentation,
    substream_index: u32,
) -> DecodeError {
    let mapped = DecodeError::from_topology(error, access_unit_index);
    let mapped_context = mapped.context();
    let mut context = DecodeErrorContext::for_access_unit(access_unit_index)
        .with_presentation(presentation.index, presentation.id)
        .with_substream(substream_index)
        .with_syntax_path(
            mapped_context
                .syntax_path()
                .unwrap_or(PRESENTATION_SUBSTREAM_SYNTAX),
        );
    if let Some(bit_offset) = mapped_context.bit_offset() {
        context = context.with_bit_offset(bit_offset);
    }
    DecodeError::new(mapped.kind(), context)
}

#[cfg(feature = "audio-decode")]
fn output_storage_error(access_unit_index: u64) -> DecodeError {
    DecodeError::new(
        DecodeErrorKind::InternalInvariant {
            stage: DecodeStage::SceneAssembly,
        },
        DecodeErrorContext::for_access_unit(access_unit_index)
            .with_syntax_path("Ac4DecoderSession/output_frames"),
    )
}

#[cfg(feature = "audio-decode")]
fn prepared_output_storage_error(prepared: &PreparedAccessUnit<'_>) -> DecodeError {
    let mut context = DecodeErrorContext::for_access_unit(prepared.context.index())
        .with_presentation(prepared.presentation.index, prepared.presentation.id)
        .with_substream(prepared.presentation.substream_index)
        .with_syntax_path("Ac4DecoderSession/output_frames");
    if prepared.presentation.group_mask != 0 {
        context = context.with_group(prepared.presentation.group_mask.trailing_zeros());
    }
    DecodeError::new(
        DecodeErrorKind::InternalInvariant {
            stage: DecodeStage::SceneAssembly,
        },
        context,
    )
}

#[cfg(feature = "audio-decode")]
fn core_band_storage_error(prepared: &PreparedAccessUnit<'_>) -> DecodeError {
    let mut context = DecodeErrorContext::for_access_unit(prepared.context.index())
        .with_presentation(prepared.presentation.index, prepared.presentation.id)
        .with_substream(prepared.presentation.substream_index)
        .with_syntax_path("DecodedAccessUnit/core_band_pcm");
    if prepared.presentation.group_mask != 0 {
        context = context.with_group(prepared.presentation.group_mask.trailing_zeros());
    }
    DecodeError::new(
        DecodeErrorKind::InternalInvariant {
            stage: DecodeStage::SceneAssembly,
        },
        context,
    )
}

#[cfg(feature = "audio-decode")]
fn copy_core_band_pcm(
    target: &mut CoreBandPcmFrameStorage,
    source: &DecodedFullAjocAsfFrame<'_>,
    prepared: &PreparedAccessUnit<'_>,
) -> Result<(), DecodeError> {
    let context = prepared.engine_input.syntax.context.params.context;
    let expected_samples = usize::from(context.frame_len_base);
    let channels = source.channels();
    let generation = prepared.transition.generation;
    let configure = target.generation != Some(generation);

    if channels == 0
        || (!configure
            && (target.sample_rate != context.sampling_frequency_hz
                || target.substream_index != prepared.presentation.substream_index
                || target.channels.len() != channels))
    {
        return Err(core_band_storage_error(prepared));
    }

    for index in 0..channels {
        let channel = source
            .channel(index)
            .ok_or_else(|| core_band_storage_error(prepared))?;
        let observation = channel.observation();
        if channel.samples().len() != expected_samples
            || channel.samples().iter().any(|sample| !sample.is_finite())
        {
            return Err(core_band_storage_error(prepared));
        }
        if !configure {
            let existing = target
                .channels
                .get(index)
                .ok_or_else(|| core_band_storage_error(prepared))?;
            if existing.element_index != observation.element_index()
                || existing.channel_index != observation.channel_index()
            {
                return Err(core_band_storage_error(prepared));
            }
        }
    }

    target
        .channels
        .try_reserve(channels.saturating_sub(target.channels.len()))
        .map_err(|_| core_band_storage_error(prepared))?;
    while target.channels.len() < channels {
        target.channels.push(CoreBandPcmChannelStorage::default());
    }
    if configure {
        target.channels.truncate(channels);
    }
    for channel in &mut target.channels {
        channel
            .samples
            .try_reserve(expected_samples.saturating_sub(channel.samples.len()))
            .map_err(|_| core_band_storage_error(prepared))?;
    }

    for index in 0..channels {
        let source = source
            .channel(index)
            .ok_or_else(|| core_band_storage_error(prepared))?;
        let observation = source.observation();
        let target = target
            .channels
            .get_mut(index)
            .ok_or_else(|| core_band_storage_error(prepared))?;
        if configure {
            target.element_index = observation.element_index();
            target.channel_index = observation.channel_index();
        }
        target.samples.clear();
        const INTERNAL_TO_NORMALIZED: f32 = f32::from_bits(0x3800_0000);
        target.samples.extend(
            source
                .samples()
                .iter()
                .map(|sample| *sample * INTERNAL_TO_NORMALIZED),
        );
    }

    target.generation = Some(generation);
    target.sample_rate = context.sampling_frequency_hz;
    target.substream_index = prepared.presentation.substream_index;
    target.samples_per_channel = expected_samples;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ErrorPolicy {
    invalidation: Option<ResetReason>,
    reset_required: bool,
}

const fn error_policy(error: DecodeError) -> ErrorPolicy {
    match error.kind() {
        DecodeErrorKind::NeedMoreData(_) => ErrorPolicy {
            invalidation: None,
            reset_required: false,
        },
        DecodeErrorKind::Selection(_) | DecodeErrorKind::Unsupported(_) => ErrorPolicy {
            invalidation: Some(ResetReason::ConfigurationChange),
            reset_required: false,
        },
        DecodeErrorKind::InvalidBitstream(_) | DecodeErrorKind::DecodeFailure { .. } => {
            ErrorPolicy {
                invalidation: Some(ResetReason::ParseFailure),
                reset_required: false,
            }
        }
        DecodeErrorKind::InternalInvariant { .. } => ErrorPolicy {
            invalidation: Some(ResetReason::ParseFailure),
            reset_required: true,
        },
        DecodeErrorKind::ResetRequired => ErrorPolicy {
            invalidation: None,
            reset_required: true,
        },
    }
}

#[cfg(feature = "audio-decode")]
fn apply_control_error_policy(
    error: DecodeError,
    topology: &mut TopologyStateMachine,
    group_oamd: &mut GroupOamdDecoder,
    presentation_metadata: &mut PresentationMetadataState,
    output_frame_count: &mut usize,
    reset_required: &mut bool,
) {
    let policy = error_policy(error);
    if let Some(reason) = policy.invalidation {
        *output_frame_count = 0;
        group_oamd.reset();
        presentation_metadata.reset();
        topology.mark_discontinuity(reason);
    }
    if policy.reset_required {
        *reset_required = true;
    }
}

impl Default for Ac4DecoderSession {
    fn default() -> Self {
        Self::new(Ac4DecoderConfig::default())
    }
}

/// 已通过控制面预检、但尚未提交任何跨帧 DSP 状态的 AU。
#[cfg_attr(
    not(any(test, feature = "audio-decode")),
    expect(dead_code, reason = "无 audio-decode 时不会形成 DSP 候选")
)]
#[derive(Debug)]
struct PreparedAccessUnit<'frame> {
    #[cfg_attr(
        all(not(test), feature = "audio-decode"),
        expect(dead_code, reason = "仅测试核对候选仍借用原始 AU")
    )]
    raw_frame: &'frame [u8],
    context: AccessUnitContext,
    topology: Ac4Topology,
    presentation: ResolvedPresentation,
    transition: TopologyTransition,
    group_oamd: PreparedGroupOamd,
    presentation_metadata: Option<PreparedPresentationMetadata<'frame>>,
    #[cfg(feature = "audio-decode")]
    engine_input: FullAjocAudioFrameInput<'frame>,
    next_topology: TopologyStateMachine,
}

/// 一个已经完整验证、但尚未复制到 Session 存储或提交跨帧状态的 presentation payload。
#[derive(Debug, Clone, Copy)]
struct PreparedPresentationMetadata<'frame> {
    payload: &'frame [u8],
    record: PresentationSubstreamMetadataRecord,
    next_drc_state: PresentationDrcState,
    next_group_gain_state: PresentationSubstreamGroupGainState,
}

/// AU 控制面预检的两种非错误结果。
#[cfg_attr(
    not(any(test, feature = "audio-decode")),
    expect(dead_code, reason = "无 audio-decode 时公开入口不执行预检")
)]
#[expect(
    clippy::large_enum_variant,
    reason = "Ready 直接拥有固定容量拓扑候选；Box 会引入禁止的逐 AU 堆分配"
)]
#[derive(Debug)]
enum AccessUnitPreflight<'frame> {
    WaitingForRandomAccess { reason: ResetKind },
    Ready(PreparedAccessUnit<'frame>),
}

const fn reset_kind(reason: ResetReason) -> ResetKind {
    match reason {
        ResetReason::Initial => ResetKind::Initial,
        ResetReason::SourceChange => ResetKind::SourceChange,
        ResetReason::ConfigurationChange => ResetKind::ConfigurationChange,
        ResetReason::ParseFailure => ResetKind::ParseFailure,
        ResetReason::ExternalDiscontinuity => ResetKind::ExternalDiscontinuity,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PresentationCandidate {
    index: u32,
    id: Option<u32>,
    eligible: bool,
}

impl PresentationCandidate {
    const EMPTY: Self = Self {
        index: 0,
        id: None,
        eligible: false,
    };
}

/// 当前配置中已唯一确定的 Full A-JOC 来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedPresentation {
    pub(crate) index: u32,
    pub(crate) id: Option<u32>,
    pub(crate) identity_occurrences: u32,
    pub(crate) group_mask: u8,
    pub(crate) substream_index: u32,
    pub(crate) ajoc_info: SubstreamInfoAjoc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SupportFailure {
    reason: UnsupportedReason,
    group_index: Option<u32>,
    substream_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AjocSourceFailure {
    Unsupported(SupportFailure),
    InternalInvariant {
        group_index: Option<u32>,
        substream_index: Option<u32>,
    },
}

impl From<SupportFailure> for AjocSourceFailure {
    fn from(failure: SupportFailure) -> Self {
        Self::Unsupported(failure)
    }
}

impl SupportFailure {
    const fn new(reason: UnsupportedReason) -> Self {
        Self {
            reason,
            group_index: None,
            substream_index: None,
        }
    }

    const fn with_group(mut self, index: u32) -> Self {
        self.group_index = Some(index);
        self
    }

    const fn with_substream(mut self, index: u32) -> Self {
        self.substream_index = Some(index);
        self
    }
}

/// 解析成功后按当前配置重新选择 presentation，并收敛到唯一 A-JOC substream。
///
/// 该函数不改变 Session；调用方可以先完成全部解析/DSP 候选工作，再统一提交状态。
pub(crate) fn resolve_presentation(
    topology: &Ac4Topology,
    selection: PresentationSelection,
    decode_mode: DecodeMode,
    access_unit_index: u64,
) -> Result<ResolvedPresentation, DecodeError> {
    validate_group_references(topology)
        .and_then(|()| validate_substream_references(topology))
        .map_err(|error| DecodeError::from_topology(error, access_unit_index))?;

    let mut candidates = [PresentationCandidate::EMPTY; MAX_PRESENTATIONS];
    let mut written = 0usize;
    for (index, presentation) in topology.presentations().iter().enumerate() {
        let Some(slot) = candidates.get_mut(written) else {
            return Err(DecodeError::new(
                DecodeErrorKind::InternalInvariant {
                    stage: DecodeStage::Topology,
                },
                DecodeErrorContext::for_access_unit(access_unit_index)
                    .with_syntax_path(PRESENTATION_SYNTAX),
            ));
        };
        *slot = PresentationCandidate {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            id: presentation.presentation_id,
            eligible: presentation_is_eligible(presentation),
        };
        written = written.saturating_add(1);
    }

    let available = candidates.get(..written).unwrap_or(&[]);
    let selected = select_candidate(available, selection).map_err(|error| {
        DecodeError::new(
            DecodeErrorKind::Selection(error),
            DecodeErrorContext::for_access_unit(access_unit_index)
                .with_syntax_path(PRESENTATION_SYNTAX),
        )
    })?;

    let presentation_position = usize::try_from(selected.index).unwrap_or(usize::MAX);
    let Some(presentation) = topology.presentations().get(presentation_position) else {
        return Err(DecodeError::new(
            DecodeErrorKind::InternalInvariant {
                stage: DecodeStage::Topology,
            },
            DecodeErrorContext::for_access_unit(access_unit_index)
                .with_presentation(selected.index, selected.id)
                .with_syntax_path(PRESENTATION_SYNTAX),
        ));
    };
    if let Some(reason) = unsupported_frame_rate(
        presentation.frame_rate_factor,
        presentation.frame_rate_fraction,
    ) {
        return Err(DecodeError::new(
            DecodeErrorKind::Unsupported(reason),
            DecodeErrorContext::for_access_unit(access_unit_index)
                .with_presentation(selected.index, selected.id)
                .with_syntax_path(PRESENTATION_SYNTAX),
        ));
    }

    let identity_occurrences = presentation_identity_occurrences(available, selected.id);
    match resolve_ajoc_source(
        topology,
        selected,
        identity_occurrences,
        presentation,
        decode_mode,
    ) {
        Ok(resolved) => Ok(resolved),
        Err(AjocSourceFailure::Unsupported(failure)) => {
            let mut context = DecodeErrorContext::for_access_unit(access_unit_index)
                .with_presentation(selected.index, selected.id)
                .with_syntax_path(GROUP_SYNTAX);
            if let Some(index) = failure.group_index {
                context = context.with_group(index);
            }
            if let Some(index) = failure.substream_index {
                context = context.with_substream(index);
            }
            Err(DecodeError::new(
                DecodeErrorKind::Unsupported(failure.reason),
                context,
            ))
        }
        Err(AjocSourceFailure::InternalInvariant {
            group_index,
            substream_index,
        }) => {
            let mut context = DecodeErrorContext::for_access_unit(access_unit_index)
                .with_presentation(selected.index, selected.id)
                .with_syntax_path(GROUP_SYNTAX);
            if let Some(index) = group_index {
                context = context.with_group(index);
            }
            if let Some(index) = substream_index {
                context = context.with_substream(index);
            }
            Err(DecodeError::new(
                DecodeErrorKind::InternalInvariant {
                    stage: DecodeStage::Topology,
                },
                context,
            ))
        }
    }
}

fn presentation_is_eligible(presentation: &Ac4PresentationV1Info) -> bool {
    presentation.presentation_config != Some(6) && !presentation.group_indices().is_empty()
}

fn presentation_identity_occurrences(
    candidates: &[PresentationCandidate],
    presentation_id: Option<u32>,
) -> u32 {
    u32::try_from(
        candidates
            .iter()
            .filter(|candidate| candidate.id == presentation_id)
            .count(),
    )
    .unwrap_or(u32::MAX)
}

fn select_candidate(
    candidates: &[PresentationCandidate],
    selection: PresentationSelection,
) -> Result<PresentationCandidate, PresentationSelectionError> {
    let declared = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
    let selected = match selection {
        PresentationSelection::AutoUnique => {
            let mut selected = None;
            let mut eligible = 0u32;
            for candidate in candidates.iter().copied().filter(|item| item.eligible) {
                eligible = eligible.saturating_add(1);
                selected = Some(candidate);
            }
            match (eligible, selected) {
                (0, _) => {
                    return Err(PresentationSelectionError::NoEligiblePresentation { declared });
                }
                (1, Some(candidate)) => candidate,
                (count, _) => {
                    return Err(PresentationSelectionError::Ambiguous { eligible: count });
                }
            }
        }
        PresentationSelection::Index(requested) => {
            let index = usize::try_from(requested).unwrap_or(usize::MAX);
            candidates
                .get(index)
                .copied()
                .ok_or(PresentationSelectionError::IndexOutOfRange {
                    requested,
                    declared,
                })?
        }
        PresentationSelection::Id(requested) => {
            let mut selected = None;
            let mut matches = 0u32;
            for candidate in candidates
                .iter()
                .copied()
                .filter(|item| item.id == Some(requested))
            {
                matches = matches.saturating_add(1);
                selected = Some(candidate);
            }
            match (matches, selected) {
                (0, _) => return Err(PresentationSelectionError::IdNotFound { requested }),
                (1, Some(candidate)) => candidate,
                (count, _) => {
                    return Err(PresentationSelectionError::IdNotUnique {
                        requested,
                        matches: count,
                    });
                }
            }
        }
    };

    if !selected.eligible {
        return Err(PresentationSelectionError::NotEligible {
            index: selected.index,
        });
    }
    Ok(selected)
}

fn resolve_ajoc_source(
    topology: &Ac4Topology,
    selected: PresentationCandidate,
    identity_occurrences: u32,
    presentation: &Ac4PresentationV1Info,
    decode_mode: DecodeMode,
) -> Result<ResolvedPresentation, AjocSourceFailure> {
    let mut has_channel = false;
    let mut has_ajoc = false;
    let mut has_direct = false;
    let mut group_mask = 0u8;
    let mut ajoc_substreams = 0u32;
    let mut ajoc_contexts = [None; MAX_SUBSTREAMS];
    let mut first_group = None;
    let mut first_substream = None;

    for &group_index in presentation.group_indices() {
        first_group = first_group.or(Some(group_index));
        let group_position = usize::try_from(group_index).unwrap_or(usize::MAX);
        let Some(group) = topology.groups().get(group_position) else {
            return Err(AjocSourceFailure::InternalInvariant {
                group_index: Some(group_index),
                substream_index: None,
            });
        };
        if group.frame_rate_factor != 1 {
            return Err(
                SupportFailure::new(UnsupportedReason::MultiSubstreamFrameRate {
                    frame_rate_factor: group.frame_rate_factor,
                })
                .with_group(group_index)
                .into(),
            );
        }
        let Some(group_bit) = 1u8.checked_shl(group_index) else {
            return Err(AjocSourceFailure::InternalInvariant {
                group_index: Some(group_index),
                substream_index: None,
            });
        };
        group_mask |= group_bit;

        for substream in group.substreams() {
            first_substream = first_substream.or(substream.substream_index());
            match *substream {
                SubstreamInfo::Chan(_) => has_channel = true,
                SubstreamInfo::Obj(_) => has_direct = true,
                SubstreamInfo::Ajoc(ref info) => {
                    has_ajoc = true;
                    let Some(index) = info.substream_index() else {
                        return Err(SupportFailure::new(
                            UnsupportedReason::AjocSubstreamIndexAbsent,
                        )
                        .with_group(group_index)
                        .into());
                    };
                    if let Some(reason) = unsupported_object_assignment(*info, decode_mode) {
                        return Err(SupportFailure::new(reason)
                            .with_group(group_index)
                            .with_substream(index)
                            .into());
                    }
                    let Some(bit) = 1u32.checked_shl(index) else {
                        return Err(AjocSourceFailure::InternalInvariant {
                            group_index: Some(group_index),
                            substream_index: Some(index),
                        });
                    };
                    register_ajoc_context(&mut ajoc_contexts, *info, group_index)?;
                    ajoc_substreams |= bit;
                }
            }
        }
    }

    let path_failure = unsupported_scene_path(has_channel, has_ajoc, has_direct);
    if let Some(reason) = path_failure {
        let mut failure = SupportFailure::new(reason);
        if let Some(index) = first_group {
            failure = failure.with_group(index);
        }
        if let Some(index) = first_substream {
            failure = failure.with_substream(index);
        }
        return Err(failure.into());
    }

    let substream_index =
        single_ajoc_substream(ajoc_substreams, decode_mode).map_err(|reason| {
            let mut failure = SupportFailure::new(reason);
            if let Some(index) = first_group {
                failure = failure.with_group(index);
            }
            AjocSourceFailure::Unsupported(failure)
        })?;
    let context_index = usize::try_from(substream_index).unwrap_or(usize::MAX);
    let Some(ajoc_info) = ajoc_contexts.get(context_index).copied().flatten() else {
        return Err(AjocSourceFailure::InternalInvariant {
            group_index: first_group,
            substream_index: Some(substream_index),
        });
    };

    Ok(ResolvedPresentation {
        index: selected.index,
        id: selected.id,
        identity_occurrences,
        group_mask,
        substream_index,
        ajoc_info,
    })
}

const fn unsupported_dynamic_assignment(
    assignment: ObjectAssignment,
    decode_mode: DecodeMode,
) -> Option<UnsupportedReason> {
    if assignment.n_bed == 0 && assignment.n_isf == 0 {
        None
    } else {
        Some(match decode_mode {
            DecodeMode::Core => UnsupportedReason::CoreObjectAssignment {
                bed_signals: assignment.n_bed,
                isf_signals: assignment.n_isf,
            },
            DecodeMode::Full => UnsupportedReason::FullbandObjectAssignment {
                bed_signals: assignment.n_bed,
                isf_signals: assignment.n_isf,
            },
        })
    }
}

const fn unsupported_object_assignment(
    info: SubstreamInfoAjoc,
    decode_mode: DecodeMode,
) -> Option<UnsupportedReason> {
    match decode_mode {
        DecodeMode::Core => match info.dmx_assignment {
            Some(assignment) => unsupported_dynamic_assignment(assignment, decode_mode),
            None => Some(UnsupportedReason::StaticDownmix),
        },
        DecodeMode::Full => unsupported_dynamic_assignment(info.upmix_assignment, decode_mode),
    }
}

fn register_ajoc_context(
    contexts: &mut [Option<SubstreamInfoAjoc>; MAX_SUBSTREAMS],
    candidate: SubstreamInfoAjoc,
    group_index: u32,
) -> Result<(), AjocSourceFailure> {
    let Some(substream_index) = candidate.substream_index() else {
        return Err(
            SupportFailure::new(UnsupportedReason::AjocSubstreamIndexAbsent)
                .with_group(group_index)
                .into(),
        );
    };
    let position = usize::try_from(substream_index).unwrap_or(usize::MAX);
    let Some(slot) = contexts.get_mut(position) else {
        return Err(AjocSourceFailure::InternalInvariant {
            group_index: Some(group_index),
            substream_index: Some(substream_index),
        });
    };
    if let Some(current) = slot {
        if !current.has_same_audio_context(&candidate) {
            return Err(
                SupportFailure::new(UnsupportedReason::AjocSubstreamContextConflict)
                    .with_group(group_index)
                    .with_substream(substream_index)
                    .into(),
            );
        }
    } else {
        *slot = Some(candidate);
    }
    Ok(())
}

const fn unsupported_frame_rate(
    frame_rate_factor: u32,
    frame_rate_fraction: u32,
) -> Option<UnsupportedReason> {
    if frame_rate_factor != 1 {
        return Some(UnsupportedReason::MultiSubstreamFrameRate { frame_rate_factor });
    }
    if frame_rate_fraction != 1 {
        return Some(UnsupportedReason::FragmentedFrameRate {
            frame_rate_fraction,
        });
    }
    None
}

const fn unsupported_scene_path(
    has_channel: bool,
    has_ajoc: bool,
    has_direct: bool,
) -> Option<UnsupportedReason> {
    match (has_channel, has_ajoc, has_direct) {
        (false, true, false) => None,
        (true, false, false) => Some(UnsupportedReason::ChannelBased),
        (false, false, true) => Some(UnsupportedReason::DirectObject),
        (false, false, false) => Some(UnsupportedReason::EmptyScene),
        _ => Some(UnsupportedReason::Mixed),
    }
}

fn single_ajoc_substream(mask: u32, decode_mode: DecodeMode) -> Result<u32, UnsupportedReason> {
    let count = mask.count_ones();
    if count != 1 {
        return Err(match decode_mode {
            DecodeMode::Core => UnsupportedReason::MultipleCoreSubstreams { count },
            DecodeMode::Full => UnsupportedReason::MultipleFullSubstreams { count },
        });
    }
    Ok(mask.trailing_zeros())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec};

    fn candidate(index: u32, id: Option<u32>, eligible: bool) -> PresentationCandidate {
        PresentationCandidate {
            index,
            id,
            eligible,
        }
    }

    #[test]
    fn auto_unique_counts_only_eligible_presentations() {
        let candidates = [candidate(0, None, false), candidate(1, Some(7), true)];
        assert_eq!(
            select_candidate(&candidates, PresentationSelection::AutoUnique),
            Ok(candidates[1])
        );
    }

    #[test]
    fn auto_unique_reports_zero_and_ambiguous_sets() {
        assert_eq!(
            select_candidate(
                &[candidate(0, None, false)],
                PresentationSelection::AutoUnique
            ),
            Err(PresentationSelectionError::NoEligiblePresentation { declared: 1 })
        );
        assert_eq!(
            select_candidate(
                &[candidate(0, None, true), candidate(1, None, true)],
                PresentationSelection::AutoUnique
            ),
            Err(PresentationSelectionError::Ambiguous { eligible: 2 })
        );
    }

    #[test]
    fn index_selection_is_zero_based_and_rejects_ineligible_entries() {
        let candidates = [candidate(0, None, true), candidate(1, None, false)];
        assert_eq!(
            select_candidate(&candidates, PresentationSelection::Index(0)),
            Ok(candidates[0])
        );
        assert_eq!(
            select_candidate(&candidates, PresentationSelection::Index(1)),
            Err(PresentationSelectionError::NotEligible { index: 1 })
        );
        assert_eq!(
            select_candidate(&candidates, PresentationSelection::Index(2)),
            Err(PresentationSelectionError::IndexOutOfRange {
                requested: 2,
                declared: 2,
            })
        );
    }

    #[test]
    fn id_selection_rejects_missing_and_duplicate_ids() {
        let candidates = [
            candidate(0, None, true),
            candidate(1, Some(9), true),
            candidate(2, Some(9), true),
        ];
        assert_eq!(
            select_candidate(&candidates, PresentationSelection::Id(7)),
            Err(PresentationSelectionError::IdNotFound { requested: 7 })
        );
        assert_eq!(
            select_candidate(&candidates, PresentationSelection::Id(9)),
            Err(PresentationSelectionError::IdNotUnique {
                requested: 9,
                matches: 2,
            })
        );
        assert_eq!(
            select_candidate(&[candidate(0, Some(4), true)], PresentationSelection::Id(4)),
            Ok(candidate(0, Some(4), true))
        );
        assert_eq!(presentation_identity_occurrences(&candidates, Some(9)), 2);
        assert_eq!(presentation_identity_occurrences(&candidates, None), 1);
    }

    #[test]
    fn only_a_pure_ajoc_path_reaches_the_full_engine() {
        assert_eq!(unsupported_scene_path(false, true, false), None);
        assert_eq!(
            unsupported_scene_path(true, false, false),
            Some(UnsupportedReason::ChannelBased)
        );
        assert_eq!(
            unsupported_scene_path(false, false, true),
            Some(UnsupportedReason::DirectObject)
        );
        assert_eq!(
            unsupported_scene_path(false, false, false),
            Some(UnsupportedReason::EmptyScene)
        );
        for path in [
            (true, true, false),
            (false, true, true),
            (true, false, true),
        ] {
            assert_eq!(
                unsupported_scene_path(path.0, path.1, path.2),
                Some(UnsupportedReason::Mixed)
            );
        }
    }

    #[test]
    fn full_mode_requires_exactly_one_physical_ajoc_substream() {
        assert_eq!(
            single_ajoc_substream(0, DecodeMode::Full),
            Err(UnsupportedReason::MultipleFullSubstreams { count: 0 })
        );
        assert_eq!(single_ajoc_substream(1 << 7, DecodeMode::Full), Ok(7));
        assert_eq!(single_ajoc_substream(1 << 7, DecodeMode::Core), Ok(7));
        assert_eq!(
            single_ajoc_substream((1 << 2) | (1 << 9), DecodeMode::Full),
            Err(UnsupportedReason::MultipleFullSubstreams { count: 2 })
        );
        assert_eq!(
            single_ajoc_substream(0, DecodeMode::Core),
            Err(UnsupportedReason::MultipleCoreSubstreams { count: 0 })
        );
    }

    #[test]
    fn full_mode_rejects_multiplied_and_fragmented_frame_rates() {
        assert_eq!(unsupported_frame_rate(1, 1), None);
        assert_eq!(
            unsupported_frame_rate(2, 1),
            Some(UnsupportedReason::MultiSubstreamFrameRate {
                frame_rate_factor: 2,
            })
        );
        assert_eq!(
            unsupported_frame_rate(1, 4),
            Some(UnsupportedReason::FragmentedFrameRate {
                frame_rate_fraction: 4,
            })
        );
    }

    #[test]
    fn full_mode_rejects_fullband_bed_and_isf_assignments() {
        let cases = [
            (
                ObjectAssignment {
                    n_signals: 4,
                    n_bed: 2,
                    ..ObjectAssignment::default()
                },
                UnsupportedReason::FullbandObjectAssignment {
                    bed_signals: 2,
                    isf_signals: 0,
                },
            ),
            (
                ObjectAssignment {
                    n_signals: 4,
                    n_isf: 4,
                    ..ObjectAssignment::default()
                },
                UnsupportedReason::FullbandObjectAssignment {
                    bed_signals: 0,
                    isf_signals: 4,
                },
            ),
        ];

        assert_eq!(
            unsupported_dynamic_assignment(
                ObjectAssignment {
                    n_signals: 1,
                    dynamic_only: true,
                    ..ObjectAssignment::default()
                },
                DecodeMode::Full,
            ),
            None
        );
        for (assignment, expected) in cases {
            assert_eq!(
                unsupported_dynamic_assignment(assignment, DecodeMode::Full),
                Some(expected)
            );
        }
    }

    #[test]
    fn core_mode_requires_dynamic_downmix_objects() {
        assert_eq!(
            unsupported_object_assignment(SubstreamInfoAjoc::default(), DecodeMode::Core),
            Some(UnsupportedReason::StaticDownmix)
        );

        let mut dynamic = SubstreamInfoAjoc::default();
        dynamic.dmx_assignment = Some(ObjectAssignment {
            n_signals: 1,
            dynamic_only: true,
            ..ObjectAssignment::default()
        });
        assert_eq!(
            unsupported_object_assignment(dynamic, DecodeMode::Core),
            None
        );

        let mut core_bed = SubstreamInfoAjoc::default();
        core_bed.dmx_assignment = Some(ObjectAssignment {
            n_signals: 2,
            n_bed: 2,
            ..ObjectAssignment::default()
        });
        assert_eq!(
            unsupported_object_assignment(core_bed, DecodeMode::Core),
            Some(UnsupportedReason::CoreObjectAssignment {
                bed_signals: 2,
                isf_signals: 0,
            })
        );
    }

    #[test]
    fn discontinuity_and_reset_clear_the_control_state() {
        let mut session = Ac4DecoderSession::default();
        assert!(!session.topology.is_waiting_for_random_access());
        session.mark_discontinuity();
        assert!(session.topology.is_waiting_for_random_access());
        session.reset();
        assert!(!session.topology.is_waiting_for_random_access());
        assert_eq!(session.config, Ac4DecoderConfig::default());
    }

    /// TOC 前置字段：version 2、48 kHz、frame_rate_index 13、I-frame、单 presentation。
    const TOC_PREFIX: &str = "10 0000000000 0 1 1101 1 1 0 0";
    /// 与 Full engine 单元夹具一致的 48 kHz、1920 样本 I-frame TOC。
    #[cfg(feature = "audio-decode")]
    const FULL_AUDIO_TOC_PREFIX: &str = "10 0000000000 0 1 0001 1 1 0 0";
    #[cfg(feature = "audio-decode")]
    const FULL_AUDIO_TOC_PREFIX_SEQUENCE_1: &str = "10 0000000001 0 1 0001 1 1 0 0";
    #[cfg(feature = "audio-decode")]
    const FULL_AUDIO_TOC_PREFIX_SEQUENCE_1_NOT_IFRAME: &str = "10 0000000001 0 1 0001 0 1 0 0";
    #[cfg(feature = "audio-decode")]
    const FULL_AUDIO_TOC_PREFIX_SEQUENCE_2: &str = "10 0000000010 0 1 0001 1 1 0 0";
    /// 与 [`TOC_PREFIX`] 相同，但声明两个 presentation。
    #[cfg(feature = "audio-decode")]
    const TOC_PREFIX_TWO_PRESENTATIONS: &str = "10 0000000000 0 1 1101 1 0 1 00 0 0 0";
    const TOC_PREFIX_SEQUENCE_1: &str = "10 0000000001 0 1 1101 1 1 0 0";
    const TOC_PREFIX_SEQUENCE_2: &str = "10 0000000010 0 1 1101 1 1 0 0";
    const TOC_PREFIX_SEQUENCE_1_NOT_IFRAME: &str = "10 0000000001 0 1 1101 0 1 0 0";
    const TOC_PREFIX_NOT_IFRAME: &str = "10 0000000000 0 1 1101 0 1 0 0";
    /// 单 group presentation；presentation substream 使用 index 0。
    const PRESENTATION: &str = "1 0 000 0 00 000 0 00 00 0 000 0 0 0 0 00";
    /// 与 `PRESENTATION` 相同，但 presentation substream 不依赖前帧。
    const PRESENTATION_NDOT: &str = "1 0 000 0 00 000 0 00 00 0 000 0 0 0 1 00";
    /// frame_rate_index 1 额外携带零值 multiply bit，其余与 `PRESENTATION_NDOT` 相同。
    #[cfg(feature = "audio-decode")]
    const FULL_AUDIO_PRESENTATION: &str = "1 0 000 0 0 00 000 0 00 00 0 000 0 0 0 1 00";
    /// 两 group presentation，按码流读取顺序引用 group 1、group 0。
    #[cfg(feature = "audio-decode")]
    const FULL_AUDIO_PRESENTATION_REVERSED_GROUPS: &str =
        "0 000 0 000 0 0 00 000 0 00 00 0 0 001 000 0 0 0 1 00";
    #[cfg(feature = "audio-decode")]
    const FULL_AUDIO_PRESENTATION_MD_COMPAT_1: &str = "1 0 001 0 0 00 000 0 00 00 0 000 0 0 0 1 00";
    /// 与 [`PRESENTATION_NDOT`] 相同，但引用 group 1。
    #[cfg(feature = "audio-decode")]
    const PRESENTATION_NDOT_GROUP_1: &str = "1 0 000 0 00 000 0 00 00 0 001 0 0 0 1 00";
    /// presentation 级 EMDF payload 使用 index 2，其余字段与 `PRESENTATION_NDOT` 相同。
    const PRESENTATION_NDOT_WITH_EMDF_2: &str = "1 0 000 0 00 000 1 10 00 00 0 000 0 0 0 1 00";
    /// 单一 A-JOC substream index 1，且音频本身不依赖前帧。
    const FULL_AJOC_GROUP: &str = "1 0 1 0 0 1 1 0 1000 1 0 0011 1 0 0 1 01 0";
    /// 与 [`FULL_AJOC_GROUP`] 相同，但 full 上混侧含两个 BED 信号。
    const FULL_AJOC_GROUP_WITH_FULLBAND_BED: &str =
        "1 0 1 0 0 1 1 0 1000 1 0 0011 0 0 1 000 0 0 1 01 0";
    /// 与 [`FULL_AJOC_GROUP`] 相同，但 full 上混侧是四信号 ISF。
    const FULL_AJOC_GROUP_WITH_ISF: &str = "1 0 1 0 0 1 1 0 1000 1 0 0011 0 1 000 0 0 1 01 0";
    /// 单动态下混信号、单动态空间对象、无 LFE 的 Full engine 最小夹具拓扑。
    #[cfg(feature = "audio-decode")]
    const FULL_AUDIO_GROUP: &str = "1 0 1 0 0 1 0 0 0000 1 0 0000 1 0 0 1 01 0";
    /// 与 [`FULL_AUDIO_GROUP`] 相同，但在 A-JOC info 内嵌屏幕比例码 17 的 common。
    #[cfg(feature = "audio-decode")]
    const FULL_AUDIO_GROUP_WITH_INLINE_COMMON: &str =
        "1 0 1 0 0 1 0 0 0000 1 1 0 10001 1 0 0000 1 0 0 1 01 0";
    /// 与 [`FULL_AJOC_GROUP`] 相同，但音频使用 substream index 2。
    #[cfg(feature = "audio-decode")]
    const FULL_AJOC_GROUP_SUBSTREAM_2: &str = "1 0 1 0 0 1 1 0 1000 1 0 0011 1 0 0 1 10 0";
    /// presentation substream 0、A-JOC audio 1、OAMD 2。
    const FULL_AJOC_GROUP_WITH_OAMD: &str = "1 0 1 0 1 1 10 1 1 0 1000 1 0 0011 1 0 0 1 01 0";
    /// index 0 是三字节 presentation metadata，index 1 为空。
    const TWO_SUBSTREAMS_WITH_PRESENTATION: &str = "10 0 0000000011 0 0000000000";
    /// index 0 是三字节 presentation metadata，index 1 为空，index 2 声明一个字节但不追加。
    const THREE_SUBSTREAMS_WITH_TRUNCATED_LAST: &str = "11 0 0000000011 0 0000000000 0 0000000001";
    /// 普通 object/A-JOC presentation：无 additional data、dialnorm 0x55、DRC absent、
    /// associated audio absent、object loudness correction absent。
    const MINIMAL_PRESENTATION_PAYLOAD: [u8; 3] = [0x55, 0x04, 0x00];
    /// independent object/A-JOC presentation：单一 default DRC mode，随后是已知兼容尾部。
    const INDEPENDENT_DRC_PRESENTATION_PAYLOAD: [u8; 5] = [0x00, 0x3d, 0x01, 0x00, 0x80];
    /// bitstream crate 的最小有效 Full A-JOC I-frame：单 ASF/A-SPX 输入、单对象。
    #[cfg(feature = "audio-decode")]
    const MINIMAL_FULL_AUDIO_PAYLOAD: [u8; 24] = [
        0x00, 0x28, 0x40, 0x85, 0x88, 0x40, 0x10, 0x00, 0x00, 0x0f, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x0e, 0xfe, 0x44, 0x02, 0x00, 0xc3, 0x00, 0x00, 0x20,
    ];
    /// 保持外层 substream 完整，但把内部 `audio_size` 声明改为 32767 字节。
    #[cfg(feature = "audio-decode")]
    const INVALID_FULL_AUDIO_PAYLOAD: [u8; 24] = [
        0xff, 0xfe, 0x40, 0x85, 0x88, 0x40, 0x10, 0x00, 0x00, 0x0f, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x0e, 0xfe, 0x44, 0x02, 0x00, 0xc3, 0x00, 0x00, 0x20,
    ];

    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "测试位串按已计数长度写入新分配的字节数组"
    )]
    fn frame_bytes(parts: &[&str]) -> Vec<u8> {
        let bits = parts
            .iter()
            .flat_map(|part| part.chars())
            .filter(|ch| *ch == '0' || *ch == '1')
            .collect::<Vec<_>>();
        let mut bytes = vec![0u8; bits.len().div_ceil(8)];
        for (index, bit) in bits.into_iter().enumerate() {
            if bit == '1' {
                bytes[index / 8] |= 1 << (7 - index % 8);
            }
        }
        bytes
    }

    fn parse_frame(parts: &[&str]) -> Ac4Topology {
        Ac4Topology::parse(&frame_bytes(parts)).expect("构造拓扑必须合法")
    }

    fn frame_with_minimal_presentation(parts: &[&str]) -> Vec<u8> {
        let mut frame = frame_bytes(parts);
        frame.extend_from_slice(&MINIMAL_PRESENTATION_PAYLOAD);
        frame
    }

    fn frame_with_oamd(toc: &str, payload: u8) -> Vec<u8> {
        frame_with_oamd_payload(toc, &[payload])
    }

    fn frame_with_oamd_payload(toc: &str, payload: &[u8]) -> Vec<u8> {
        let size = format!("{size:010b}", size = payload.len());
        let table = ["11 0 0000000011 0 0000000000 0 ", &size].concat();
        let mut frame = frame_bytes(&[toc, PRESENTATION_NDOT, FULL_AJOC_GROUP_WITH_OAMD, &table]);
        frame.extend_from_slice(&MINIMAL_PRESENTATION_PAYLOAD);
        frame.extend_from_slice(payload);
        frame
    }

    #[cfg(feature = "audio-decode")]
    fn frame_with_full_payload(payload: &[u8]) -> Vec<u8> {
        let size = format!("{size:010b}", size = payload.len());
        let table = ["10 0 0000000011 0 ", &size].concat();
        let mut frame = frame_bytes(&[TOC_PREFIX, PRESENTATION_NDOT, FULL_AJOC_GROUP, &table]);
        frame.extend_from_slice(&MINIMAL_PRESENTATION_PAYLOAD);
        frame.extend_from_slice(payload);
        frame
    }

    #[cfg(feature = "audio-decode")]
    fn frame_with_minimal_full_topology(payload: &[u8]) -> Vec<u8> {
        frame_with_minimal_full_topology_toc(FULL_AUDIO_TOC_PREFIX, payload)
    }

    #[cfg(feature = "audio-decode")]
    fn frame_with_minimal_full_topology_toc(toc: &str, payload: &[u8]) -> Vec<u8> {
        frame_with_minimal_full_topology_parts(toc, FULL_AUDIO_PRESENTATION, payload)
    }

    #[cfg(feature = "audio-decode")]
    fn frame_with_minimal_full_topology_parts(
        toc: &str,
        presentation: &str,
        payload: &[u8],
    ) -> Vec<u8> {
        frame_with_minimal_full_topology_group(toc, presentation, FULL_AUDIO_GROUP, payload)
    }

    #[cfg(feature = "audio-decode")]
    fn frame_with_minimal_full_topology_group(
        toc: &str,
        presentation: &str,
        group: &str,
        payload: &[u8],
    ) -> Vec<u8> {
        frame_with_presentation_and_audio_payload(
            toc,
            presentation,
            group,
            &MINIMAL_PRESENTATION_PAYLOAD,
            payload,
        )
    }

    #[cfg(feature = "audio-decode")]
    fn frame_with_presentation_and_audio_payload(
        toc: &str,
        presentation: &str,
        group: &str,
        presentation_payload: &[u8],
        audio_payload: &[u8],
    ) -> Vec<u8> {
        let presentation_size = format!("{size:010b}", size = presentation_payload.len());
        let audio_size = format!("{size:010b}", size = audio_payload.len());
        let table = ["10 0 ", &presentation_size, " 0 ", &audio_size].concat();
        let mut frame = frame_bytes(&[toc, presentation, group, &table]);
        frame.extend_from_slice(presentation_payload);
        frame.extend_from_slice(audio_payload);
        frame
    }

    #[cfg(feature = "audio-decode")]
    fn frame_with_reversed_full_groups(toc: &str, payload: &[u8]) -> Vec<u8> {
        let size = format!("{size:010b}", size = payload.len());
        let table = ["10 0 0000000011 0 ", &size].concat();
        let mut frame = frame_bytes(&[
            toc,
            FULL_AUDIO_PRESENTATION_REVERSED_GROUPS,
            FULL_AUDIO_GROUP_WITH_INLINE_COMMON,
            FULL_AUDIO_GROUP,
            &table,
        ]);
        frame.extend_from_slice(&MINIMAL_PRESENTATION_PAYLOAD);
        frame.extend_from_slice(payload);
        frame
    }

    fn first_ajoc_info(topology: &Ac4Topology) -> SubstreamInfoAjoc {
        let Some(SubstreamInfo::Ajoc(info)) = topology
            .groups()
            .first()
            .and_then(|group| group.substreams().first())
            .copied()
        else {
            panic!("测试 topology 的首个 group 应携带 A-JOC substream");
        };
        info
    }

    fn expect_ready(result: AccessUnitPreflight<'_>) -> PreparedAccessUnit<'_> {
        match result {
            AccessUnitPreflight::Ready(prepared) => prepared,
            AccessUnitPreflight::WaitingForRandomAccess { reason } => {
                panic!("预期 AU 可解码，实际仍在等待 {reason:?}")
            }
        }
    }

    #[cfg(not(feature = "audio-decode"))]
    #[test]
    fn public_decode_reports_the_disabled_audio_feature() {
        let mut session = Ac4DecoderSession::default();
        let error = session
            .decode_access_unit(AccessUnit::new(&[], AccessUnitContext::new(17)))
            .expect_err("未启用 audio-decode 时不得伪造场景输出");

        assert_eq!(
            error.kind(),
            DecodeErrorKind::Unsupported(UnsupportedReason::AudioDecodeFeatureDisabled)
        );
        assert_eq!(error.context().access_unit_index(), 17);
        assert_eq!(
            error.context().syntax_path(),
            Some("Ac4DecoderSession/decode_access_unit")
        );
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn public_decode_returns_borrowed_scene_frames_and_reuses_storage() {
        let first_frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let second_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session = Ac4DecoderSession::default();

        let (element_id, object_address, plane_address, pcm_address, metadata_address) = {
            let decoded = session
                .decode_access_unit(AccessUnit::new(
                    &first_frame,
                    AccessUnitContext::new(100)
                        .with_source_sample_start(9_600)
                        .with_presentation_sample_start(0)
                        .with_priming_samples(1_920)
                        .with_random_access_hint(true),
                ))
                .expect("首个 Full AU 应经公开入口产生预热场景帧");
            assert_eq!(decoded.status(), DecodeStatus::Decoded);
            assert_eq!(decoded.frame_count(), 1);

            let mut frames = decoded.frames();
            assert_eq!(frames.len(), 1);
            let frame = frames.next().expect("应有一份借用场景帧");
            assert!(frames.next().is_none());
            let object = frame.objects().first().expect("最小夹具应有一个对象");
            let pcm = object.pcm();
            let plane = pcm.planes().first().expect("对象应有 mono plane");

            assert_eq!(frame.timeline().access_unit_index(), 100);
            assert_eq!(frame.timeline().source_sample_start(), Some(9_600));
            assert_eq!(frame.timeline().presentation_sample_start(), Some(0));
            assert_eq!(frame.timeline().control_source_access_unit_index(), None);
            assert_eq!(frame.presentation().identity_occurrences(), 1);
            assert!(frame.diagnostics().warmup());
            assert!(!frame.diagnostics().state_complete());
            assert!(frame.metadata_updates().is_empty());
            assert_eq!(pcm.samples_per_plane(), 1_920);
            assert_eq!(plane.samples().len(), 1_920);

            (
                object.element_id(),
                frame.objects().as_ptr(),
                pcm.planes().as_ptr(),
                plane.samples().as_ptr(),
                frame.metadata_updates().as_ptr(),
            )
        };

        let decoded = session
            .decode_access_unit(AccessUnit::new(
                &second_frame,
                AccessUnitContext::new(101)
                    .with_source_sample_start(11_520)
                    .with_presentation_sample_start(1_920),
            ))
            .expect("连续 Full AU 应经公开入口复用场景存储");
        assert_eq!(decoded.status(), DecodeStatus::Decoded);
        let frame = decoded.frames().next().expect("连续 AU 应产生场景帧");
        let object = frame.objects().first().expect("对象拓扑应保持稳定");
        let pcm = object.pcm();
        let plane = pcm.planes().first().expect("对象 plane 应保持稳定");
        let metadata = frame
            .metadata_updates()
            .first()
            .expect("首份控制到期后应公开对象更新");

        assert_eq!(object.element_id(), element_id);
        assert_eq!(frame.objects().as_ptr(), object_address);
        assert_eq!(pcm.planes().as_ptr(), plane_address);
        assert_eq!(plane.samples().as_ptr(), pcm_address);
        assert_eq!(frame.metadata_updates().as_ptr(), metadata_address);
        assert_eq!(frame.timeline().access_unit_index(), 101);
        assert_eq!(
            frame.timeline().control_source_access_unit_index(),
            Some(100)
        );
        assert!(!frame.diagnostics().warmup());
        assert!(frame.diagnostics().state_complete());
        assert_eq!(metadata.element_id(), element_id);
        assert_eq!(metadata.control_source_access_unit_index(), 100);
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn public_decode_exposes_owned_presentation_metadata_and_reuses_payload_storage() {
        let first_frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let second_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let topology = Ac4Topology::parse(&first_frame).expect("首帧拓扑应有效");
        let source_payload_address = topology
            .substream_payload(&first_frame, 0)
            .expect("presentation payload 应可定位")
            .as_ptr();
        let mut session = Ac4DecoderSession::default();

        let payload_address = {
            let decoded = session
                .decode_access_unit(AccessUnit::new(
                    &first_frame,
                    AccessUnitContext::new(130).with_random_access_hint(true),
                ))
                .expect("完整 AU 应发布 presentation metadata");
            let metadata = decoded
                .presentation_metadata()
                .expect("所选 presentation 应有 metadata sidecar");

            assert_eq!(metadata.access_unit_index(), 130);
            assert_eq!(metadata.presentation_index(), 0);
            assert_eq!(metadata.presentation_id(), None);
            assert_eq!(metadata.substream_index(), 0);
            assert!(metadata.context().presentation_is_independent());
            assert_eq!(metadata.context().n_substream_groups(), 1);
            assert_eq!(metadata.payload(), MINIMAL_PRESENTATION_PAYLOAD);
            assert_eq!(metadata.syntax_payload(), MINIMAL_PRESENTATION_PAYLOAD);
            assert!(metadata.compatibility_tail().is_empty());
            assert_ne!(metadata.payload().as_ptr(), source_payload_address);
            assert_eq!(metadata.effective_drc_configuration(), None);
            assert_eq!(metadata.effective_group_gain_codes().as_slice(), &[0]);

            let parsed = metadata
                .parsed_substream()
                .expect("Session 自持 payload 应可重建完整解析视图");
            assert_eq!(parsed.dialnorm_bits, 0x55);
            assert!(!parsed.drc_present);
            assert_eq!(
                parsed.substream_group_gain_update,
                macindecode_ac4_bitstream::PresentationSubstreamGroupGainUpdate::NotSignaled
            );
            assert!(parsed.associated_audio.is_none());
            metadata.payload().as_ptr()
        };

        let decoded = session
            .decode_access_unit(AccessUnit::new(&second_frame, AccessUnitContext::new(131)))
            .expect("连续 AU 应复用 presentation payload 存储");
        let metadata = decoded
            .presentation_metadata()
            .expect("连续 AU 应继续发布 metadata sidecar");
        assert_eq!(metadata.access_unit_index(), 131);
        assert_eq!(metadata.payload().as_ptr(), payload_address);
    }

    #[test]
    fn scene_compatibility_tail_is_narrow_and_transactional() {
        let context = PresentationSubstreamContext::new(
            false,
            true,
            1,
            1,
            macindecode_ac4_bitstream::PresentationChannelContext::UNDEFINED,
        );
        for compatibility_byte in INDEPENDENT_OBJECT_DRC_COMPATIBILITY_BYTES {
            let mut payload = INDEPENDENT_DRC_PRESENTATION_PAYLOAD;
            payload[4] = compatibility_byte;
            let mut state = PresentationDrcState::new();
            let (parsed, syntax_payload_len) =
                Ac4PresentationSubstream::parse_with_drc_state_compat(
                    &payload, context, &mut state,
                )
                .expect("已验证的 independent object DRC 尾部应进入窄兼容路径");
            assert_eq!(syntax_payload_len, 4);
            assert!(parsed.drc_present);
            assert!(parsed.drc_configuration.is_some());
            assert!(state.configuration().is_some());
        }

        let mut wrong_byte = INDEPENDENT_DRC_PRESENTATION_PAYLOAD;
        wrong_byte[4] = 0x81;
        let mut rejected_state = PresentationDrcState::new();
        assert_eq!(
            Ac4PresentationSubstream::parse_with_drc_state_compat(
                &wrong_byte,
                context,
                &mut rejected_state,
            )
            .unwrap_err(),
            PresentationSubstreamError::TrailingBits { remaining_bits: 8 }
        );
        assert_eq!(rejected_state, PresentationDrcState::new());

        for compatibility_byte in INDEPENDENT_OBJECT_DRC_COMPATIBILITY_BYTES {
            let absent_drc_with_tail = [0x55, 0x04, 0x00, compatibility_byte];
            assert_eq!(
                Ac4PresentationSubstream::parse_with_drc_state_compat(
                    &absent_drc_with_tail,
                    context,
                    &mut rejected_state,
                )
                .unwrap_err(),
                PresentationSubstreamError::TrailingBits { remaining_bits: 8 }
            );
            assert_eq!(rejected_state, PresentationDrcState::new());
        }

        let channel_context = PresentationSubstreamContext::new(
            false,
            true,
            1,
            1,
            macindecode_ac4_bitstream::PresentationChannelContext::new(
                Some(0),
                None,
                false,
                0,
                false,
            ),
        );
        for compatibility_byte in INDEPENDENT_OBJECT_DRC_COMPATIBILITY_BYTES {
            let mut payload = INDEPENDENT_DRC_PRESENTATION_PAYLOAD;
            payload[4] = compatibility_byte;
            assert_eq!(
                Ac4PresentationSubstream::parse_with_drc_state_compat(
                    &payload,
                    channel_context,
                    &mut rejected_state,
                )
                .unwrap_err(),
                PresentationSubstreamError::TrailingBits { remaining_bits: 8 }
            );
            assert_eq!(rejected_state, PresentationDrcState::new());
        }
    }

    #[test]
    fn sda_opaque_tail_requires_complete_independent_drc_and_opt_in() {
        let context = PresentationSubstreamContext::new(false, true, 1, 1,
            macindecode_ac4_bitstream::PresentationChannelContext::UNDEFINED);
        let mut payload = INDEPENDENT_DRC_PRESENTATION_PAYLOAD;
        payload[4] = 0x8c;
        let mut state = PresentationDrcState::new();
        assert!(parse_presentation_with_tail(&payload, context, &mut state, false).is_err());
        assert_eq!(state, PresentationDrcState::new());
        let (parsed, length) = parse_presentation_with_tail(&payload, context, &mut state, true).unwrap();
        assert_eq!(length, 4);
        assert!(parsed.drc_configuration.is_some());
        for invalid in [&[0x55, 0x04, 0x00, 0x8c][..], &[0xff][..], &[0x55, 0x04, 0x00, 0x8c, 0x8c][..]] {
            let mut state = PresentationDrcState::new();
            assert!(parse_presentation_with_tail(invalid, context, &mut state, true).is_err());
            assert_eq!(state, PresentationDrcState::new());
        }
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn public_decode_preserves_known_presentation_compatibility_tails() {
        for compatibility_byte in INDEPENDENT_OBJECT_DRC_COMPATIBILITY_BYTES {
            let mut presentation_payload = INDEPENDENT_DRC_PRESENTATION_PAYLOAD;
            presentation_payload[4] = compatibility_byte;
            let frame = frame_with_presentation_and_audio_payload(
                FULL_AUDIO_TOC_PREFIX,
                FULL_AUDIO_PRESENTATION,
                FULL_AUDIO_GROUP,
                &presentation_payload,
                &MINIMAL_FULL_AUDIO_PAYLOAD,
            );
            let mut session = Ac4DecoderSession::default();
            let decoded = session
                .decode_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(131)))
                .expect("已知独立帧兼容尾部不得阻断 Scene 回放入口");
            let metadata = decoded
                .presentation_metadata()
                .expect("成功 AU 应发布 presentation metadata");

            assert_eq!(metadata.payload(), presentation_payload);
            assert_eq!(metadata.syntax_payload(), &presentation_payload[..4]);
            assert_eq!(metadata.compatibility_tail(), &[compatibility_byte]);
            assert!(metadata.effective_drc_configuration().is_some());
            let parsed = metadata
                .parsed_substream()
                .expect("Session 自持规范前缀应可重建借用视图");
            assert!(parsed.drc_present);
            assert!(parsed.drc_configuration.is_some());
        }
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn invalid_presentation_metadata_is_structured_and_never_published() {
        let frame = frame_with_presentation_and_audio_payload(
            FULL_AUDIO_TOC_PREFIX,
            FULL_AUDIO_PRESENTATION,
            FULL_AUDIO_GROUP,
            &[0],
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session = Ac4DecoderSession::default();

        let error = session
            .decode_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(132)))
            .expect_err("有界 presentation payload 内部截断必须失败关闭");

        assert_eq!(
            error.kind(),
            DecodeErrorKind::InvalidBitstream(crate::BitstreamFailure::PresentationSubstream(
                macindecode_ac4_bitstream::PresentationSubstreamError::Read(
                    macindecode_ac4_bitstream::reader::ReadError::OutOfBounds {
                        requested_bits: 1,
                        bit_position: 8,
                        remaining_bits: 0,
                    }
                )
            ))
        );
        assert_eq!(error.context().access_unit_index(), 132);
        assert_eq!(error.context().presentation_index(), Some(0));
        assert_eq!(error.context().substream_index(), Some(0));
        assert_eq!(error.context().bit_offset(), Some(8));
        assert_eq!(
            error.context().syntax_path(),
            Some(PRESENTATION_SUBSTREAM_SYNTAX)
        );
        assert!(session.presentation_metadata.storage.view().is_none());
        assert!(
            session
                .presentation_metadata
                .group_gain
                .effective_codes()
                .is_none()
        );
        assert!(session.topology.is_waiting_for_random_access());
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn public_decode_returns_core_scene_pcm_and_downmix_metadata() {
        let first_frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let second_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session =
            Ac4DecoderSession::new(Ac4DecoderConfig::default().with_decode_mode(DecodeMode::Core));

        let (element_id, object_address, pcm_address, metadata_address) = {
            let decoded = session
                .decode_access_unit(AccessUnit::new(
                    &first_frame,
                    AccessUnitContext::new(200).with_random_access_hint(true),
                ))
                .expect("首个 Core AU 应产生 A-SPX 预热场景帧");
            assert!(
                decoded.core_band_pcm().is_none(),
                "默认 renderer 配置不得生成核心带诊断侧车"
            );
            let frame = decoded.frames().next().expect("Core AU 应有一帧输出");
            let object = frame.objects().first().expect("应有一个 Core 对象");

            assert_eq!(frame.presentation().mode(), DecodeMode::Core);
            assert_eq!(object.kind(), crate::ObjectKind::AjocCoreObject);
            assert_eq!(
                object.source(),
                crate::SceneElementSource::AjocCoreObject {
                    substream_index: 1,
                    object_index: 0,
                    input_index: 0,
                }
            );
            assert!(frame.beds().is_empty());
            assert!(frame.diagnostics().warmup());
            assert!(!frame.diagnostics().state_complete());
            let plane = object
                .pcm()
                .planes()
                .first()
                .expect("Core 对象应有 mono PCM");
            assert_eq!(plane.samples().len(), 1_920);
            assert!(plane.samples().iter().all(|sample| sample.is_finite()));
            (
                object.element_id(),
                frame.objects().as_ptr(),
                plane.samples().as_ptr(),
                frame.metadata_updates().as_ptr(),
            )
        };

        let decoded = session
            .decode_access_unit(AccessUnit::new(&second_frame, AccessUnitContext::new(201)))
            .expect("连续 Core AU 应发布到期 downmix 控制");
        let frame = decoded.frames().next().expect("连续 Core AU 应有输出");
        let object = frame.objects().first().expect("Core 对象应保持存在");
        let metadata = frame
            .metadata_updates()
            .first()
            .expect("Core/downmix OAMD 应随 PCM 一起到期");

        assert_eq!(object.element_id(), element_id);
        assert_eq!(frame.objects().as_ptr(), object_address);
        assert_eq!(
            object
                .pcm()
                .planes()
                .first()
                .expect("Core 对象应复用 mono PCM")
                .samples()
                .as_ptr(),
            pcm_address
        );
        assert_eq!(frame.metadata_updates().as_ptr(), metadata_address);
        assert_eq!(frame.presentation().mode(), DecodeMode::Core);
        assert!(!frame.diagnostics().warmup());
        assert!(frame.diagnostics().state_complete());
        assert_eq!(metadata.element_id(), element_id);
        assert_eq!(metadata.raw().block().object_index, 0);
        assert_eq!(metadata.control_source_access_unit_index(), 200);
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn core_band_sidecar_is_normalized_and_reuses_storage() {
        let first_frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let second_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session = Ac4DecoderSession::new(
            Ac4DecoderConfig::default()
                .with_decode_mode(DecodeMode::Core)
                .with_core_band_diagnostics(true),
        );

        let samples_address = {
            let decoded = session
                .decode_access_unit(AccessUnit::new(
                    &first_frame,
                    AccessUnitContext::new(202).with_random_access_hint(true),
                ))
                .expect("首个 Core AU 应保留核心带侧车");
            let core = decoded
                .core_band_pcm()
                .expect("成功 AU 应返回 pre-A-SPX 核心带侧车");
            assert_eq!(core.sample_rate(), 48_000);
            assert_eq!(core.substream_index(), 1);
            assert_eq!(core.sample_format(), crate::PcmSampleFormat::F32);
            assert_eq!(core.layout(), crate::PcmLayout::Planar);
            assert_eq!(core.nominal_full_scale(), 1.0);
            assert_eq!(core.channel_count(), 1);
            assert_eq!(core.samples_per_channel(), 1_920);
            let channel = core.channel(0).expect("最小夹具应有一路核心带 PCM");
            assert_eq!(channel.element_index(), 0);
            assert_eq!(channel.channel_index(), 0);
            assert_eq!(channel.stride(), 1);
            assert_eq!(channel.samples().len(), 1_920);
            assert!(channel.samples().iter().all(|sample| sample.is_finite()));
            assert_eq!(decoded.frame_count(), 1, "核心带侧车不得取代 Core Scene 帧");
            channel.samples().as_ptr()
        };

        let decoded = session
            .decode_access_unit(AccessUnit::new(&second_frame, AccessUnitContext::new(203)))
            .expect("连续 Core AU 应复用核心带侧车");
        let core = decoded
            .core_band_pcm()
            .expect("连续 AU 应继续返回核心带侧车");
        let channel = core.channel(0).expect("核心带声道映射应保持稳定");
        assert_eq!((channel.element_index(), channel.channel_index()), (0, 0));
        assert_eq!(channel.samples().as_ptr(), samples_address);
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn public_decode_waits_after_discontinuity_and_recovers_at_random_access() {
        let first_frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let dependent_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1_NOT_IFRAME,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let recovery_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_2,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session = Ac4DecoderSession::default();

        {
            let decoded = session
                .decode_access_unit(AccessUnit::new(&first_frame, AccessUnitContext::new(110)))
                .expect("首帧应建立可继承历史");
            assert_eq!(decoded.status(), DecodeStatus::Decoded);
        }
        session.mark_discontinuity();

        {
            let waiting = session
                .decode_access_unit(AccessUnit::new(
                    &dependent_frame,
                    AccessUnitContext::new(111),
                ))
                .expect("依赖帧应返回等待状态而不是错误");
            assert_eq!(
                waiting.status(),
                DecodeStatus::WaitingForRandomAccess {
                    reason: ResetKind::ExternalDiscontinuity
                }
            );
            assert_eq!(waiting.frame_count(), 0);
            assert!(waiting.frames().next().is_none());
            assert!(waiting.presentation_metadata().is_none());
            assert!(waiting.core_band_pcm().is_none());
        }

        let recovered = session
            .decode_access_unit(AccessUnit::new(
                &recovery_frame,
                AccessUnitContext::new(112),
            ))
            .expect("完整随机访问点应恢复公开输出");
        assert_eq!(recovered.status(), DecodeStatus::Decoded);
        assert!(recovered.core_band_pcm().is_none());
        let frame = recovered.frames().next().expect("恢复 AU 应产生场景帧");
        assert_eq!(
            frame.diagnostics().reset(),
            Some(ResetKind::ExternalDiscontinuity)
        );
        assert!(frame.diagnostics().discontinuity());
        assert!(frame.diagnostics().warmup());
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn public_decode_keeps_truncated_input_retryable() {
        let frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let truncated = frame.get(..1).expect("完整帧至少一个字节");
        let mut session = Ac4DecoderSession::default();

        let error = session
            .decode_access_unit(AccessUnit::new(
                truncated,
                AccessUnitContext::new(120).with_discontinuity(true),
            ))
            .expect_err("截断 TOC 必须可重试地失败");
        assert!(matches!(error.kind(), DecodeErrorKind::NeedMoreData(_)));
        assert_eq!(error.context().access_unit_index(), 120);

        let decoded = session
            .decode_access_unit(AccessUnit::new(
                &frame,
                AccessUnitContext::new(120).with_random_access_hint(true),
            ))
            .expect("补齐同一 AU 后应直接重试成功");
        assert!(decoded.core_band_pcm().is_none());
        let scene = decoded.frames().next().expect("重试应产生预热场景帧");
        assert_eq!(scene.diagnostics().reset(), Some(ResetKind::Initial));
        assert!(!scene.diagnostics().discontinuity());
    }

    #[test]
    fn truncated_preflight_does_not_commit_discontinuity_or_topology() {
        let frame = frame_with_minimal_presentation(&[
            TOC_PREFIX,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let mut session = Ac4DecoderSession::default();
        let state_before = session.topology;
        let truncated = frame.get(..1).expect("完整帧至少一个字节");

        let error = session
            .prepare_access_unit(AccessUnit::new(
                truncated,
                AccessUnitContext::new(23).with_discontinuity(true),
            ))
            .expect_err("截断 TOC 必须可重试地失败");
        assert!(matches!(error.kind(), DecodeErrorKind::NeedMoreData(_)));
        assert_eq!(error.context().access_unit_index(), 23);
        assert_eq!(session.topology, state_before);
        assert!(!session.reset_required);

        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(23)))
                .expect("补齐同一 AU 后应能重试"),
        );
        assert_eq!(prepared.context.index(), 23);
        assert_eq!(prepared.raw_frame, frame);
        assert_eq!(
            prepared.topology.random_access(),
            macindecode_ac4_bitstream::topology::RandomAccess::Full
        );
        assert_eq!(prepared.presentation.substream_index, 1);
        assert_eq!(
            prepared.transition.action,
            DecoderAction::Reset {
                reason: ResetReason::Initial
            }
        );
        assert_eq!(session.topology, state_before, "Ready 候选仍未提交");
        session.commit_prepared(prepared);
        assert_eq!(session.topology.generation(), 1);
    }

    #[test]
    fn presentation_metadata_state_and_visibility_commit_transactionally() {
        let frame = frame_with_minimal_presentation(&[
            TOC_PREFIX,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let mut session = Ac4DecoderSession::default();

        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(24)))
                .expect("有效 presentation metadata 应形成候选"),
        );
        assert!(
            session
                .presentation_metadata
                .group_gain
                .effective_codes()
                .is_none()
        );
        assert!(session.presentation_metadata.storage.view().is_none());
        let candidate = prepared
            .presentation_metadata
            .expect("候选应保留 presentation payload 和有效状态");
        assert_eq!(candidate.payload, MINIMAL_PRESENTATION_PAYLOAD);
        assert_eq!(candidate.record.effective_group_gain_codes.as_slice(), &[0]);

        session.commit_prepared(prepared);
        assert_eq!(
            session
                .presentation_metadata
                .group_gain
                .effective_codes()
                .expect("提交后应有有效 group gain")
                .as_slice(),
            &[0]
        );
        assert_eq!(
            session
                .presentation_metadata
                .storage
                .view()
                .expect("提交后 sidecar 才可见")
                .payload(),
            MINIMAL_PRESENTATION_PAYLOAD
        );

        let error = session
            .prepare_access_unit(AccessUnit::new(&[0], AccessUnitContext::new(25)))
            .expect_err("下一份截断 TOC 应保持可重试");
        assert!(matches!(error.kind(), DecodeErrorKind::NeedMoreData(_)));
        assert!(session.presentation_metadata.storage.view().is_none());
        assert_eq!(
            session
                .presentation_metadata
                .group_gain
                .effective_codes()
                .expect("可重试读取耗尽不得清空已提交历史")
                .as_slice(),
            &[0]
        );
    }

    #[test]
    fn selected_full_presentation_rejects_fullband_bed_and_isf_before_dsp() {
        let cases = [
            (
                FULL_AJOC_GROUP_WITH_FULLBAND_BED,
                UnsupportedReason::FullbandObjectAssignment {
                    bed_signals: 2,
                    isf_signals: 0,
                },
            ),
            (
                FULL_AJOC_GROUP_WITH_ISF,
                UnsupportedReason::FullbandObjectAssignment {
                    bed_signals: 0,
                    isf_signals: 4,
                },
            ),
        ];

        for (group, expected) in cases {
            let topology = parse_frame(&[
                TOC_PREFIX,
                PRESENTATION_NDOT,
                group,
                TWO_SUBSTREAMS_WITH_PRESENTATION,
            ]);
            let error = resolve_presentation(
                &topology,
                PresentationSelection::AutoUnique,
                DecodeMode::Full,
                33,
            )
            .expect_err("当前 Scene 子集必须在 Full DSP 前拒绝 BED/ISF");

            assert_eq!(error.kind(), DecodeErrorKind::Unsupported(expected));
            assert_eq!(error.context().access_unit_index(), 33);
            assert_eq!(error.context().group_index(), Some(0));
            assert_eq!(error.context().substream_index(), Some(1));
        }
    }

    #[test]
    fn core_mode_uses_downmix_assignment_instead_of_full_assignment() {
        let topology = parse_frame(&[
            TOC_PREFIX,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP_WITH_FULLBAND_BED,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);

        let resolved = resolve_presentation(
            &topology,
            PresentationSelection::AutoUnique,
            DecodeMode::Core,
            34,
        )
        .expect("动态 Core 下混不应被 Full BED 分配阻断");

        assert_eq!(resolved.substream_index, 1);
        assert_eq!(resolved.ajoc_info.n_dmx_signals, 9);
        assert_eq!(resolved.ajoc_info.upmix_assignment.n_bed, 2);
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn preflight_freezes_full_engine_input_from_the_same_candidate() {
        let payload = [0xa5, 0x5a];
        let frame = frame_with_full_payload(&payload);
        let context = AccessUnitContext::new(41)
            .with_source_sample_start(-2048)
            .with_presentation_sample_start(0)
            .with_priming_samples(2048)
            .with_random_access_hint(true)
            .with_discontinuity(true);
        let mut session = Ac4DecoderSession::default();

        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&frame, context))
                .expect("完整随机访问 AU 应形成 Full engine 输入候选"),
        );
        let input = prepared.engine_input;
        let expected_payload = prepared
            .topology
            .substream_payload(&frame, prepared.presentation.substream_index)
            .expect("同一 topology 应能重新定位所选载荷");

        assert_eq!(input.syntax.payload, payload);
        assert_eq!(input.syntax.payload, expected_payload);
        assert_eq!(input.syntax.substream_index, 1);
        assert_eq!(input.syntax.physical_substreams, 1);
        assert_eq!(prepared.topology.index_table.n_substreams, 2);
        assert_eq!(input.syntax.context.params.context.frame_len_base, 2048);
        assert_eq!(
            input.syntax.context.params.context.sampling_frequency_hz,
            48_000
        );
        assert_eq!(input.syntax.context.params.group_num_obj_info_blocks, None);
        assert!(!input.syntax.context.metadata.alternative);
        assert_eq!(input.lfe_position, Some(0));
        assert_eq!(
            input.mode,
            macindecode_ac4_decode::full_ajoc::FullAjocDecodeMode::RequireFull
        );
        assert_eq!(input.provenance.access_unit_index(), 41);
        assert_eq!(input.provenance.source_sample_start(), Some(-2048));
        assert_eq!(input.provenance.presentation_sample_start(), Some(0));
        assert_eq!(input.provenance.priming_samples(), Some(2048));
        assert_eq!(input.provenance.random_access_hint(), Some(true));
        assert!(input.provenance.discontinuity());

        assert_eq!(session.topology.generation(), 0, "engine 输入仍只是候选");
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn core_preflight_selects_required_core_engine_mode() {
        let frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let mut session =
            Ac4DecoderSession::new(Ac4DecoderConfig::default().with_decode_mode(DecodeMode::Core));

        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(42)))
                .expect("动态 Core AU 应形成 A-SPX-only engine 候选"),
        );

        assert_eq!(
            prepared.engine_input.mode,
            macindecode_ac4_decode::full_ajoc::FullAjocDecodeMode::RequireCore
        );
        assert_eq!(prepared.presentation.ajoc_info.n_dmx_signals, 1);
        assert_eq!(session.topology.generation(), 0, "engine 输入仍只是候选");
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn session_drives_one_owned_full_engine_before_control_commit() {
        let frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let mut session = Ac4DecoderSession::default();
        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(
                    &frame,
                    AccessUnitContext::new(50)
                        .with_source_sample_start(9_600)
                        .with_random_access_hint(true),
                ))
                .expect("最小 Full A-JOC AU 应形成可驱动候选"),
        );

        assert_eq!(
            prepared
                .engine_input
                .syntax
                .context
                .params
                .context
                .frame_len_base,
            1_920
        );
        {
            let decoded = session
                .decode_engine_frame(&prepared)
                .expect("Session 自持 engine 应完成首帧事务");
            assert_eq!(decoded.frontend().syntax().elements().len(), 1);
            assert_eq!(decoded.frontend().asf().channels(), 1);
            assert!(decoded.output().observation().warmup());
            assert!(decoded.output().aligned_side_information().is_none());
            assert_eq!(decoded.output().diagnostic_channels(), 1);
            assert_eq!(decoded.output().reconstructed_channels(), 1);
            for channel in [
                decoded
                    .output()
                    .diagnostic_channel(0)
                    .expect("预热期应有一路诊断 PCM"),
                decoded
                    .output()
                    .reconstructed_channel(0)
                    .expect("预热期应有一路对象 PCM"),
            ] {
                assert_eq!(channel.samples().len(), 1_920);
                assert!(channel.samples().iter().all(|sample| sample.is_finite()));
            }
        }

        assert_eq!(
            session.topology.generation(),
            0,
            "engine 成功后仍不得提前提交控制面候选"
        );
        session.commit_prepared(prepared);
        assert_eq!(session.topology.generation(), 1);
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn session_assembles_normalized_scene_pcm_and_reuses_stable_buffers() {
        let first_frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let second_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session = Ac4DecoderSession::default();
        let first = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(
                    &first_frame,
                    AccessUnitContext::new(50)
                        .with_source_sample_start(9_600)
                        .with_presentation_sample_start(0)
                        .with_priming_samples(1_920)
                        .with_random_access_hint(true),
                ))
                .expect("首个 Full AU 应通过预检"),
        );
        session
            .decode_and_assemble_engine_frame(&first)
            .expect("首个 Full AU 应组装预热 Scene PCM");

        assert_eq!(session.output_frame_count, 1);
        let first_storage = session
            .output_frames
            .first()
            .expect("应有一份可见 SceneFrame 存储");
        let first_object = first_storage
            .objects
            .first()
            .expect("最小 Full 夹具应有一个空间对象组");
        let first_plane = first_object
            .planes
            .first()
            .expect("空间对象组应有一个 mono plane");
        let first_id = first_object.element_id();
        let frame_storage_address = session.output_frames.as_ptr();
        let frame_storage_capacity = session.output_frames.capacity();
        let object_storage_address = first_storage.objects.as_ptr();
        let object_storage_capacity = first_storage.objects.capacity();
        let metadata_storage_address = first_storage.metadata_updates.as_ptr();
        let metadata_storage_capacity = first_storage.metadata_updates.capacity();
        let plane_storage_address = first_object.planes.as_ptr();
        let plane_storage_capacity = first_object.planes.capacity();
        let first_pcm_address = first_plane.samples.as_ptr();
        let first_pcm_capacity = first_plane.samples.capacity();

        assert!(first_storage.beds.is_empty());
        assert_eq!(first_storage.timeline.sample_rate(), 48_000);
        assert_eq!(first_storage.timeline.codec_sample_start(), 0);
        assert_eq!(first_storage.timeline.source_sample_start(), Some(9_600));
        assert_eq!(first_storage.timeline.presentation_sample_start(), Some(0));
        assert_eq!(first_storage.timeline.duration_samples(), 1_920);
        assert_eq!(first_storage.timeline.access_unit_index(), 50);
        assert_eq!(
            first_storage.timeline.control_source_access_unit_index(),
            None
        );
        assert_eq!(first_storage.timeline.configuration_generation(), 1);
        assert_eq!(first_storage.timeline.priming_samples(), Some(1_920));
        assert_eq!(first_storage.timeline.pcm_alignment_delay_samples(), 288);
        assert_eq!(first_storage.timeline.control_alignment_delay_frames(), 1);
        assert!(first_storage.diagnostics.warmup());
        assert!(!first_storage.diagnostics.state_complete());
        assert!(!first_storage.diagnostics.semantic_metadata_complete());
        assert_eq!(first_object.initial_state(), None);
        assert!(first_storage.metadata_updates.is_empty());
        assert!(metadata_storage_capacity > 0);
        assert_eq!(first_object.pcm().samples_per_plane(), 1_920);
        assert_eq!(first_plane.samples().len(), 1_920);
        assert!(
            first_plane
                .samples()
                .iter()
                .all(|sample| sample.is_finite())
        );

        session.commit_prepared(first);
        let second = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(
                    &second_frame,
                    AccessUnitContext::new(51)
                        .with_source_sample_start(11_520)
                        .with_presentation_sample_start(1_920),
                ))
                .expect("连续 Full AU 应通过预检"),
        );
        session
            .decode_and_assemble_engine_frame(&second)
            .expect("连续 Full AU 应复用 Scene PCM 存储");

        let second_storage = session
            .output_frames
            .first()
            .expect("应继续复用同一 SceneFrame 存储");
        let second_object = second_storage.objects.first().expect("对象拓扑应保持稳定");
        let second_plane = second_object.planes.first().expect("对象 plane 应保持稳定");
        assert_eq!(second_object.element_id(), first_id);
        assert_eq!(session.output_frames.as_ptr(), frame_storage_address);
        assert_eq!(session.output_frames.capacity(), frame_storage_capacity);
        assert_eq!(second_storage.objects.as_ptr(), object_storage_address);
        assert_eq!(second_storage.objects.capacity(), object_storage_capacity);
        assert_eq!(
            second_storage.metadata_updates.as_ptr(),
            metadata_storage_address
        );
        assert_eq!(
            second_storage.metadata_updates.capacity(),
            metadata_storage_capacity
        );
        assert_eq!(second_object.planes.as_ptr(), plane_storage_address);
        assert_eq!(second_object.planes.capacity(), plane_storage_capacity);
        assert_eq!(second_plane.samples.as_ptr(), first_pcm_address);
        assert_eq!(second_plane.samples.capacity(), first_pcm_capacity);
        assert_eq!(second_storage.timeline.codec_sample_start(), 1_920);
        assert_eq!(second_storage.timeline.access_unit_index(), 51);
        assert_eq!(
            second_storage.timeline.control_source_access_unit_index(),
            Some(50)
        );
        assert!(!second_storage.diagnostics.warmup());
        assert!(second_storage.diagnostics.state_complete());
        assert!(second_storage.diagnostics.semantic_metadata_complete());
        let initial_state = second_object
            .initial_state()
            .expect("控制到期后应公开帧起点 OAMD 状态");
        assert!(!initial_state.metadata_active());
        let metadata_update = second_storage
            .metadata_updates
            .first()
            .expect("首份到期 Full OAMD 应形成帧内更新");
        assert_eq!(second_storage.metadata_updates.len(), 1);
        assert_eq!(metadata_update.element_id(), first_id);
        assert_eq!(metadata_update.state(), initial_state);
        assert_eq!(metadata_update.control_source_access_unit_index(), 50);
        assert_ne!(
            metadata_update.changed_fields(),
            crate::MetadataFields::empty()
        );
        assert_eq!(metadata_update.raw().block().object_index, 0);
        assert_eq!(metadata_update.raw().block().block_index, 0);
        let raw_timing = metadata_update.raw().timing();
        assert_eq!(raw_timing.num_obj_info_blocks(), 1);
        assert!(raw_timing.updated_in_source_access_unit());
        assert_eq!(
            metadata_update.offset_samples(),
            u32::from(raw_timing.sample_offset())
                .checked_add(raw_timing.block().offset_samples())
                .expect("测试 timing 偏移应在 u32 内")
        );
        assert_eq!(
            metadata_update.ramp_duration_samples(),
            u32::from(raw_timing.block().ramp_duration)
        );
        assert!(!second_object.has_signal());
        assert_eq!(session.topology.generation(), 1, "第二份候选仍未提交");
        session.commit_prepared(second);
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn recovery_frame_reports_a_deferred_external_discontinuity() {
        let first_frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let dependent_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1_NOT_IFRAME,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let recovery_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_2,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session = Ac4DecoderSession::default();
        let first = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&first_frame, AccessUnitContext::new(70)))
                .expect("首帧应通过预检"),
        );
        session
            .decode_and_assemble_engine_frame(&first)
            .expect("首帧应完成 Scene PCM 组装");
        session.commit_prepared(first);

        let waiting = session
            .prepare_access_unit(AccessUnit::new(
                &dependent_frame,
                AccessUnitContext::new(71).with_discontinuity(true),
            ))
            .expect("外部不连续后的依赖帧应等待随机访问点");
        assert!(matches!(
            waiting,
            AccessUnitPreflight::WaitingForRandomAccess {
                reason: ResetKind::ExternalDiscontinuity
            }
        ));

        let recovered = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&recovery_frame, AccessUnitContext::new(72)))
                .expect("完整随机访问帧应恢复解码"),
        );
        assert_eq!(
            recovered.transition.action,
            DecoderAction::Reset {
                reason: ResetReason::ExternalDiscontinuity
            }
        );
        session
            .decode_and_assemble_engine_frame(&recovered)
            .expect("恢复帧应完成 Scene PCM 组装");

        let diagnostics = session
            .output_frames
            .first()
            .expect("恢复帧应可见")
            .diagnostics;
        assert_eq!(diagnostics.reset(), Some(ResetKind::ExternalDiscontinuity));
        assert!(diagnostics.discontinuity());
        assert!(!diagnostics.configuration_changed());
        session.commit_prepared(recovered);
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn recovery_frame_reports_a_deferred_configuration_change() {
        let first_frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let changed_dependent_frame = frame_with_minimal_full_topology_parts(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1_NOT_IFRAME,
            FULL_AUDIO_PRESENTATION_MD_COMPAT_1,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let changed_recovery_frame = frame_with_minimal_full_topology_parts(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_2,
            FULL_AUDIO_PRESENTATION_MD_COMPAT_1,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session = Ac4DecoderSession::default();
        let first = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&first_frame, AccessUnitContext::new(80)))
                .expect("首帧应通过预检"),
        );
        session
            .decode_and_assemble_engine_frame(&first)
            .expect("首帧应完成 Scene PCM 组装");
        session.commit_prepared(first);

        let waiting = session
            .prepare_access_unit(AccessUnit::new(
                &changed_dependent_frame,
                AccessUnitContext::new(81),
            ))
            .expect("依赖帧上的配置变化应等待随机访问点");
        assert!(matches!(
            waiting,
            AccessUnitPreflight::WaitingForRandomAccess {
                reason: ResetKind::ConfigurationChange
            }
        ));

        let recovered = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(
                    &changed_recovery_frame,
                    AccessUnitContext::new(82),
                ))
                .expect("新配置的完整随机访问帧应恢复解码"),
        );
        assert!(!recovered.transition.config_changed);
        assert_eq!(
            recovered.transition.action,
            DecoderAction::Reset {
                reason: ResetReason::ConfigurationChange
            }
        );
        session
            .decode_and_assemble_engine_frame(&recovered)
            .expect("新配置恢复帧应完成 Scene PCM 组装");

        let diagnostics = session
            .output_frames
            .first()
            .expect("恢复帧应可见")
            .diagnostics;
        assert_eq!(diagnostics.reset(), Some(ResetKind::ConfigurationChange));
        assert!(diagnostics.configuration_changed());
        assert!(!diagnostics.discontinuity());
        session.commit_prepared(recovered);
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn group_common_follows_the_due_control_and_reuses_scene_storage() {
        let first_frame = frame_with_minimal_full_topology_group(
            FULL_AUDIO_TOC_PREFIX,
            FULL_AUDIO_PRESENTATION,
            FULL_AUDIO_GROUP_WITH_INLINE_COMMON,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let second_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let third_frame = frame_with_minimal_full_topology_toc(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_2,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session = Ac4DecoderSession::default();

        let first = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&first_frame, AccessUnitContext::new(90)))
                .expect("内嵌 common 的首帧应通过预检"),
        );
        session
            .decode_and_assemble_engine_frame(&first)
            .expect("首帧应产生 warm-up PCM");
        let warmup = session.output_frames.first().expect("warm-up 帧应有存储");
        assert!(warmup.oamd_common_states.is_empty());
        assert!(warmup.oamd_common_states.capacity() >= 1);
        let common_storage = warmup.oamd_common_states.as_ptr();
        let common_capacity = warmup.oamd_common_states.capacity();
        session.commit_prepared(first);

        let second = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&second_frame, AccessUnitContext::new(91)))
                .expect("未刷新 common 的连续帧应继承历史"),
        );
        session
            .decode_and_assemble_engine_frame(&second)
            .expect("第二帧应让首帧控制与 PCM 一起到期");
        let due_first = session.output_frames.first().expect("第二帧应有场景存储");
        let first_common = due_first
            .oamd_common_states
            .first()
            .expect("首个 AU 的 common 应在控制到期时出现");
        assert_eq!(
            due_first.timeline.control_source_access_unit_index(),
            Some(90)
        );
        assert_eq!(first_common.group_index(), 0);
        assert!(first_common.updated_in_source_access_unit());
        let effective = first_common.effective().expect("common 码值应完整保留");
        assert!(!effective.default_screen_size_ratio);
        assert_eq!(effective.master_screen_size_ratio_code, Some(17));
        assert!(effective.bed_object_chan_distribute);
        assert_eq!(due_first.oamd_common_states.as_ptr(), common_storage);
        assert_eq!(due_first.oamd_common_states.capacity(), common_capacity);
        session.commit_prepared(second);

        let third = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&third_frame, AccessUnitContext::new(92)))
                .expect("第三帧应继续继承 common"),
        );
        session
            .decode_and_assemble_engine_frame(&third)
            .expect("第三帧应让第二份控制到期");
        let due_second = session.output_frames.first().expect("第三帧应有场景存储");
        let inherited = due_second
            .oamd_common_states
            .first()
            .expect("继承后的 common 仍应完整出现");
        assert_eq!(
            due_second.timeline.control_source_access_unit_index(),
            Some(91)
        );
        assert_eq!(inherited.effective(), Some(effective));
        assert!(!inherited.updated_in_source_access_unit());
        assert_eq!(due_second.oamd_common_states.as_ptr(), common_storage);
        assert_eq!(due_second.oamd_common_states.capacity(), common_capacity);
        session.commit_prepared(third);

        session.reset();
        let reset_first_frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let reset_first = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(
                    &reset_first_frame,
                    AccessUnitContext::new(93),
                ))
                .expect("reset 后无 common 的完整帧应重新起解"),
        );
        session
            .decode_and_assemble_engine_frame(&reset_first)
            .expect("reset 后首帧应重新进入 warm-up");
        assert!(
            session
                .output_frames
                .first()
                .is_some_and(|frame| frame.oamd_common_states.is_empty()),
            "reset 后 warm-up 不得泄漏旧 common"
        );
        session.commit_prepared(reset_first);

        let reset_second = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&second_frame, AccessUnitContext::new(94)))
                .expect("reset 后第二帧应形成无 common 的连续候选"),
        );
        session
            .decode_and_assemble_engine_frame(&reset_second)
            .expect("reset 后首份控制应正常到期");
        let reset_common = session
            .output_frames
            .first()
            .and_then(|frame| frame.oamd_common_states.first())
            .expect("被引用 group 仍应报告显式的空有效状态");
        assert_eq!(reset_common.effective(), None);
        assert!(!reset_common.updated_in_source_access_unit());
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn group_common_is_sorted_independently_of_presentation_read_order() {
        let first_frame =
            frame_with_reversed_full_groups(FULL_AUDIO_TOC_PREFIX, &MINIMAL_FULL_AUDIO_PAYLOAD);
        let second_frame = frame_with_reversed_full_groups(
            FULL_AUDIO_TOC_PREFIX_SEQUENCE_1,
            &MINIMAL_FULL_AUDIO_PAYLOAD,
        );
        let mut session = Ac4DecoderSession::default();

        let first = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&first_frame, AccessUnitContext::new(100)))
                .expect("逆序 group 的首帧应通过预检"),
        );
        assert_eq!(
            first
                .topology
                .presentations()
                .first()
                .expect("fixture 应包含 presentation")
                .group_indices(),
            &[1, 0],
            "fixture 必须保留 presentation 的码流读取顺序"
        );
        session
            .decode_and_assemble_engine_frame(&first)
            .expect("逆序 group 的首帧应进入 warm-up");
        session.commit_prepared(first);

        let second = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&second_frame, AccessUnitContext::new(101)))
                .expect("逆序 group 的连续帧应通过预检"),
        );
        session
            .decode_and_assemble_engine_frame(&second)
            .expect("到期 common 不应误按 presentation 读取顺序校验");
        let due = session.output_frames.first().expect("第二帧应有场景存储");
        assert_eq!(due.presentation.group_indices(), &[1, 0]);
        assert_eq!(
            due.oamd_common_states
                .iter()
                .map(|state| state.group_index())
                .collect::<Vec<_>>(),
            vec![0, 1],
            "场景 common 契约应保持 group_index 升序"
        );
        assert!(
            due.oamd_common_states
                .first()
                .expect("group 0 common 应存在")
                .effective()
                .is_some_and(|common| common.master_screen_size_ratio_code == Some(17))
        );
        assert_eq!(
            due.oamd_common_states
                .get(1)
                .expect("group 1 common 应存在")
                .effective(),
            None
        );
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn explicit_reset_rotates_scene_ids_without_releasing_pcm_capacity() {
        let frame = frame_with_minimal_full_topology(&MINIMAL_FULL_AUDIO_PAYLOAD);
        let mut session = Ac4DecoderSession::default();
        let first = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(60)))
                .expect("首帧应通过预检"),
        );
        session
            .decode_and_assemble_engine_frame(&first)
            .expect("首帧应完成 Scene PCM 组装");
        let (first_id, pcm_address, pcm_capacity) = {
            let object = session
                .output_frames
                .first()
                .and_then(|storage| storage.objects.first())
                .expect("首帧应有对象");
            let plane = object.planes.first().expect("对象应有 plane");
            (
                object.element_id(),
                plane.samples.as_ptr(),
                plane.samples.capacity(),
            )
        };
        session.commit_prepared(first);

        session.reset();
        assert_eq!(session.output_frame_count, 0, "reset 后旧帧不得可见");
        let next = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(61)))
                .expect("reset 后完整随机访问帧应重新起解"),
        );
        session
            .decode_and_assemble_engine_frame(&next)
            .expect("reset 后应重建 Scene identity");
        let object = session
            .output_frames
            .first()
            .and_then(|storage| storage.objects.first())
            .expect("reset 后应重新发布对象");
        let plane = object.planes.first().expect("对象应有 plane");

        assert!(object.element_id().get() > first_id.get());
        assert_eq!(plane.samples.as_ptr(), pcm_address);
        assert_eq!(plane.samples.capacity(), pcm_capacity);
        assert_eq!(session.output_frames.len(), 1, "复用单个帧存储槽");
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn bounded_engine_syntax_failure_invalidates_without_half_commit() {
        let frame = frame_with_minimal_full_topology(&INVALID_FULL_AUDIO_PAYLOAD);
        let mut session = Ac4DecoderSession::default();

        let error = session
            .decode_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(51)))
            .expect_err("有界 substream 内部坏语法不得产生半帧");
        assert_eq!(
            error.kind(),
            DecodeErrorKind::DecodeFailure {
                stage: DecodeStage::AudioSyntax
            }
        );
        assert_eq!(error.context().access_unit_index(), 51);
        assert_eq!(error.context().presentation_index(), Some(0));
        assert_eq!(error.context().group_index(), Some(0));
        assert_eq!(error.context().substream_index(), Some(1));
        assert_eq!(
            error.context().syntax_path(),
            Some("raw_ac4_frame/ac4_substream")
        );
        assert_eq!(session.topology.generation(), 0, "失败候选不得提交");
        assert!(session.topology.is_waiting_for_random_access());
        assert!(!session.reset_required);
        assert!(session.output_frames.is_empty());
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn preflight_scopes_physical_substreams_to_the_selected_presentation() {
        let table = "11 0 0000000011 0 0000000000 0 0000000000";
        let frame = frame_with_minimal_presentation(&[
            TOC_PREFIX_TWO_PRESENTATIONS,
            PRESENTATION_NDOT,
            PRESENTATION_NDOT_GROUP_1,
            FULL_AJOC_GROUP,
            FULL_AJOC_GROUP_SUBSTREAM_2,
            table,
        ]);
        let mut session =
            Ac4DecoderSession::new(Ac4DecoderConfig::new(PresentationSelection::Index(0)));

        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(44)))
                .expect("未选 presentation 的独立 A-JOC 不应污染当前输出范围"),
        );
        let frame_ajoc_substreams = prepared
            .topology
            .groups()
            .iter()
            .flat_map(|group| group.substreams())
            .filter_map(|info| match *info {
                SubstreamInfo::Ajoc(ref ajoc) => ajoc.substream_index(),
                SubstreamInfo::Chan(_) | SubstreamInfo::Obj(_) => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(frame_ajoc_substreams, vec![1, 2]);
        assert_eq!(prepared.presentation.index, 0);
        assert_eq!(prepared.presentation.substream_index, 1);
        assert_eq!(prepared.engine_input.syntax.physical_substreams, 1);
        assert_eq!(session.topology.generation(), 0, "候选仍不得提前提交");
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn preflight_propagates_effective_group_oamd_timing_to_audio_context() {
        // outer common=0、timing=1；implicit offset、一个零偏移/零 ramp block。
        let timing_payload = frame_bytes(&["0 1 0 001 000000 00"]);
        let frame = frame_with_oamd_payload(TOC_PREFIX, &timing_payload);
        let mut session = Ac4DecoderSession::default();

        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(42)))
                .expect("group timing 应与音频上下文形成同一候选"),
        );

        assert_eq!(prepared.group_oamd.group_num_obj_info_blocks(), Some(1));
        assert_eq!(
            prepared
                .engine_input
                .syntax
                .context
                .params
                .group_num_obj_info_blocks,
            Some(1)
        );
        assert!(!prepared.engine_input.syntax.context.metadata.alternative);
        assert_eq!(session.topology.generation(), 0, "timing 候选不得提前提交");
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn full_engine_context_failure_is_structured_and_invalidates_history() {
        // A-JOC info 的 b_static_dmx=1，因而缺少构造 core 对象所需的 dmx_assignment。
        let static_group = "1 0 1 0 0 1 0 1 0 0011 1 0 0 1 01 0";
        let frame = frame_with_minimal_presentation(&[
            TOC_PREFIX,
            PRESENTATION_NDOT,
            static_group,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let mut session = Ac4DecoderSession::default();

        let error = session
            .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(43)))
            .expect_err("static downmix 不得进入 Full engine");

        assert_eq!(
            error.kind(),
            DecodeErrorKind::Unsupported(UnsupportedReason::StaticDownmix)
        );
        assert_eq!(error.context().presentation_index(), Some(0));
        assert_eq!(error.context().group_index(), Some(0));
        assert_eq!(error.context().substream_index(), Some(1));
        assert_eq!(
            error.context().syntax_path(),
            Some("raw_ac4_frame/ac4_toc/ac4_substream_group_info/ac4_substream_info_ajoc")
        );
        assert!(session.topology.is_waiting_for_random_access());
        assert_eq!(session.topology.generation(), 0);
    }

    #[test]
    fn preflight_rejects_a_truncated_substream_after_the_selected_full_payload() {
        let frame = frame_with_minimal_presentation(&[
            TOC_PREFIX,
            PRESENTATION_NDOT_WITH_EMDF_2,
            FULL_AJOC_GROUP,
            THREE_SUBSTREAMS_WITH_TRUNCATED_LAST,
        ]);
        let topology = Ac4Topology::parse(&frame).expect("控制面本身必须完整");
        assert!(
            topology.substream_payload(&frame, 1).is_ok(),
            "所选 Full A-JOC payload 本身为空且边界合法"
        );
        assert!(
            topology.substream_payload(&frame, 2).is_err(),
            "后续 EMDF payload 声明不得越过 AU"
        );

        let mut session = Ac4DecoderSession::default();
        let error = session
            .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(24)))
            .expect_err("预检必须覆盖所选 Full substream 之后的载荷边界");
        assert!(matches!(error.kind(), DecodeErrorKind::InvalidBitstream(_)));
        assert_eq!(error.context().substream_index(), Some(2));
        assert_eq!(session.topology.generation(), 0, "不得提交候选配置代次");
        assert!(session.topology.is_waiting_for_random_access());
    }

    #[test]
    fn payload_boundary_error_precedes_unsupported_presentation() {
        let direct_group = "1 0 1 0 0 0 010 1 0 0 0 1 01 0";
        let frame = frame_with_minimal_presentation(&[
            TOC_PREFIX,
            PRESENTATION_NDOT,
            direct_group,
            THREE_SUBSTREAMS_WITH_TRUNCATED_LAST,
        ]);
        let topology = Ac4Topology::parse(&frame).expect("控制面本身必须完整");
        assert!(topology.substream_payload(&frame, 2).is_err());

        let mut session = Ac4DecoderSession::default();
        let error = session
            .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(25)))
            .expect_err("完整 AU 边界必须先于不受支持的场景路径失败");

        assert!(matches!(error.kind(), DecodeErrorKind::InvalidBitstream(_)));
        assert_eq!(error.context().substream_index(), Some(2));
        assert!(session.topology.is_waiting_for_random_access());
    }

    #[test]
    fn preflight_commits_waiting_but_keeps_ready_reset_transactional() {
        let dependent = frame_with_minimal_presentation(&[
            TOC_PREFIX_NOT_IFRAME,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let random_access = frame_with_minimal_presentation(&[
            TOC_PREFIX_SEQUENCE_1,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let mut session = Ac4DecoderSession::default();

        let waiting = session
            .prepare_access_unit(AccessUnit::new(&dependent, AccessUnitContext::new(0)))
            .expect("依赖帧应进入等待状态而不是报错");
        assert!(matches!(
            waiting,
            AccessUnitPreflight::WaitingForRandomAccess {
                reason: ResetKind::Initial
            }
        ));
        assert_eq!(session.topology.generation(), 1);
        assert!(session.topology.is_waiting_for_random_access());

        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&random_access, AccessUnitContext::new(1)))
                .expect("完整随机访问点应形成可提交候选"),
        );
        assert_eq!(prepared.transition.generation, 1);
        assert_eq!(
            prepared.transition.action,
            DecoderAction::Reset {
                reason: ResetReason::Initial
            }
        );
        assert!(
            session.topology.is_waiting_for_random_access(),
            "DSP 成功前不得提前解除门禁"
        );
        session.commit_prepared(prepared);
        assert!(!session.topology.is_waiting_for_random_access());
    }

    #[test]
    fn preflight_commits_group_oamd_only_with_the_ready_candidate() {
        let common = frame_with_oamd(TOC_PREFIX, 0b1100_0000);
        let inherited = frame_with_oamd(TOC_PREFIX_SEQUENCE_1_NOT_IFRAME, 0);
        let recovered = frame_with_oamd(TOC_PREFIX_SEQUENCE_2, 0);
        let mut session = Ac4DecoderSession::default();

        let first = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&common, AccessUnitContext::new(0)))
                .expect("首帧 common 应形成事务候选"),
        );
        let common_state = first
            .group_oamd
            .groups()
            .first()
            .and_then(|group| group.effective_common)
            .expect("候选应携带当前 common");
        session.commit_prepared(first);

        let second = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&inherited, AccessUnitContext::new(1)))
                .expect("提交后依赖帧应继承 common"),
        );
        assert_eq!(
            second
                .group_oamd
                .groups()
                .first()
                .and_then(|group| group.effective_common),
            Some(common_state)
        );
        session.commit_prepared(second);

        session.mark_discontinuity();
        let reset = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&recovered, AccessUnitContext::new(2)))
                .expect("随机访问恢复帧应从空 OAMD 历史开始"),
        );
        assert_eq!(
            reset
                .group_oamd
                .groups()
                .first()
                .and_then(|group| group.effective_common),
            None
        );
    }

    #[test]
    fn access_unit_discontinuity_waits_for_a_full_random_access_point() {
        let first = frame_with_minimal_presentation(&[
            TOC_PREFIX,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let dependent = frame_with_minimal_presentation(&[
            TOC_PREFIX_SEQUENCE_1_NOT_IFRAME,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let recovered = frame_with_minimal_presentation(&[
            TOC_PREFIX_SEQUENCE_2,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let mut session = Ac4DecoderSession::default();
        let initial = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&first, AccessUnitContext::new(0)))
                .expect("首个随机访问点应可解码"),
        );
        session.commit_prepared(initial);

        let waiting = session
            .prepare_access_unit(AccessUnit::new(
                &dependent,
                AccessUnitContext::new(1).with_discontinuity(true),
            ))
            .expect("外部不连续后的依赖帧应等待");
        assert!(matches!(
            waiting,
            AccessUnitPreflight::WaitingForRandomAccess {
                reason: ResetKind::ExternalDiscontinuity
            }
        ));

        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&recovered, AccessUnitContext::new(2)))
                .expect("后续随机访问点应恢复"),
        );
        assert_eq!(
            prepared.transition.action,
            DecoderAction::Reset {
                reason: ResetReason::ExternalDiscontinuity
            }
        );
        session.commit_prepared(prepared);
        assert!(!session.topology.is_waiting_for_random_access());
    }

    #[test]
    fn unsupported_path_invalidates_history_without_committing_its_generation() {
        let first = frame_with_minimal_presentation(&[
            TOC_PREFIX,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let direct_group = "1 0 1 0 0 0 010 1 0 0 0 1 01 0";
        let direct = frame_with_minimal_presentation(&[
            TOC_PREFIX_SEQUENCE_1,
            PRESENTATION_NDOT,
            direct_group,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let recovered = frame_with_minimal_presentation(&[
            TOC_PREFIX_SEQUENCE_2,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let mut session = Ac4DecoderSession::default();
        let initial = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&first, AccessUnitContext::new(0)))
                .expect("首帧应可解码"),
        );
        session.commit_prepared(initial);

        let error = session
            .prepare_access_unit(AccessUnit::new(&direct, AccessUnitContext::new(1)))
            .expect_err("direct-object 必须明确拒绝");
        assert_eq!(
            error.kind(),
            DecodeErrorKind::Unsupported(UnsupportedReason::DirectObject)
        );
        assert!(session.topology.is_waiting_for_random_access());
        assert_eq!(session.topology.generation(), 1);

        let prepared = expect_ready(
            session
                .prepare_access_unit(AccessUnit::new(&recovered, AccessUnitContext::new(2)))
                .expect("受支持的完整随机访问点应恢复"),
        );
        assert_eq!(prepared.transition.generation, 1);
        assert_eq!(
            prepared.transition.action,
            DecoderAction::Reset {
                reason: ResetReason::ConfigurationChange
            }
        );
    }

    #[test]
    fn internal_invariant_requires_explicit_reset() {
        let frame = frame_with_minimal_presentation(&[
            TOC_PREFIX,
            PRESENTATION_NDOT,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let mut session = Ac4DecoderSession::default();
        session.apply_error_policy(DecodeError::new(
            DecodeErrorKind::InternalInvariant {
                stage: DecodeStage::SceneAssembly,
            },
            DecodeErrorContext::for_access_unit(4),
        ));
        session.mark_discontinuity();

        let error = session
            .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(5)))
            .expect_err("mark_discontinuity 不能解除内部不变量失效");
        assert_eq!(error.kind(), DecodeErrorKind::ResetRequired);
        assert_eq!(error.context().access_unit_index(), 5);

        session.reset();
        assert!(matches!(
            session
                .prepare_access_unit(AccessUnit::new(&frame, AccessUnitContext::new(5)))
                .expect("显式 reset 后应恢复"),
            AccessUnitPreflight::Ready(_)
        ));
    }

    #[test]
    fn selected_full_presentation_resolves_one_physical_ajoc_substream() {
        // A-JOC group：显式 substream index 1；index 0 属于 presentation substream。
        let group = "1 0 1 0 0 1 1 0 1000 1 0 0011 1 0 0 0 01 0";
        let table = "10 0 0000000000 0 0000000000";
        let topology = parse_frame(&[TOC_PREFIX, PRESENTATION, group, table]);

        let resolved = resolve_presentation(
            &topology,
            PresentationSelection::AutoUnique,
            DecodeMode::Full,
            12,
        )
        .expect("唯一 A-JOC presentation 应可解析");
        assert_eq!(resolved.index, 0);
        assert_eq!(resolved.identity_occurrences, 1);
        assert_eq!(resolved.group_mask, 1);
        assert_eq!(resolved.substream_index, 1);
        let expected_info = first_ajoc_info(&topology);
        assert_eq!(resolved.ajoc_info, expected_info);
    }

    #[test]
    fn duplicate_ajoc_substream_rejects_conflicting_audio_contexts() {
        let topology = parse_frame(&[
            TOC_PREFIX,
            PRESENTATION,
            FULL_AJOC_GROUP,
            TWO_SUBSTREAMS_WITH_PRESENTATION,
        ]);
        let info = first_ajoc_info(&topology);
        let mut contexts = [None; MAX_SUBSTREAMS];
        register_ajoc_context(&mut contexts, info, 0).expect("首次引用应建立上下文");
        register_ajoc_context(&mut contexts, info, 1).expect("相同上下文可由多个 group 共享");

        let mut conflicting = info;
        conflicting.n_upmix_signals = conflicting.n_upmix_signals.saturating_add(1);
        let error = register_ajoc_context(&mut contexts, conflicting, 2)
            .expect_err("同一物理 substream 的音频上下文冲突必须拒绝");
        let AjocSourceFailure::Unsupported(failure) = error else {
            panic!("上下文冲突应是结构化不支持边界，实际为 {error:?}");
        };
        assert_eq!(
            failure.reason,
            UnsupportedReason::AjocSubstreamContextConflict
        );
        assert_eq!(failure.group_index, Some(2));
        assert_eq!(failure.substream_index, info.substream_index());
    }

    #[test]
    fn multiplied_ajoc_info_does_not_resolve_as_one_physical_substream() {
        let presentation = "1 0 000 0 10 00 000 0 00 00 0 000 0 0 0 0 00";
        // frame_rate_factor=2，因此 tail 携带两个 b_audio_ndot，index 1 覆盖 1、2。
        let group = "1 0 1 0 0 1 1 0 1000 1 0 0011 1 0 0 0 0 01 0";
        let table = "11 0 0000000000 0 0000000000 0 0000000000";
        let topology = parse_frame(&["10 0000000000 0 1 0011 1 1 0 0", presentation, group, table]);

        let error = resolve_presentation(
            &topology,
            PresentationSelection::AutoUnique,
            DecodeMode::Full,
            18,
        )
        .expect_err("倍帧率 A-JOC 不得退化为单一物理 substream");
        assert_eq!(
            error.kind(),
            DecodeErrorKind::Unsupported(UnsupportedReason::MultiSubstreamFrameRate {
                frame_rate_factor: 2,
            })
        );
        assert_eq!(error.context().presentation_index(), Some(0));
        assert_eq!(error.context().syntax_path(), Some(PRESENTATION_SYNTAX));
    }

    #[test]
    fn selected_direct_object_presentation_fails_closed_with_context() {
        let group = "1 0 1 0 0 0 010 1 0 0 0 0 01 0";
        let table = "10 0 0000000000 0 0000000000";
        let topology = parse_frame(&[TOC_PREFIX, PRESENTATION, group, table]);

        let error = resolve_presentation(
            &topology,
            PresentationSelection::AutoUnique,
            DecodeMode::Full,
            15,
        )
        .expect_err("direct-object 必须拒绝");
        assert_eq!(
            error.kind(),
            DecodeErrorKind::Unsupported(UnsupportedReason::DirectObject)
        );
        assert_eq!(error.context().presentation_index(), Some(0));
        assert_eq!(error.context().group_index(), Some(0));
        assert_eq!(error.context().substream_index(), Some(1));
    }
}
