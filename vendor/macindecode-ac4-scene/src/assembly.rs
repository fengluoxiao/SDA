//! Bitstream-owned OAMD 量化状态到 Scene-owned 语义状态的映射。

use macindecode_ac4_bitstream::{
    math,
    oamd::{
        AdditionalObjectMetadata, ObjectGainState, ObjectMetadataState, ObjectPriorityState,
        PositionCoding, QuantizedPosition, WidthUpdate, ZoneUpdate,
    },
};

#[cfg(feature = "audio-decode")]
use alloc::{vec, vec::Vec};
#[cfg(feature = "audio-decode")]
use macindecode_ac4_bitstream::{
    oamd::{MAX_OAMD_METADATA_BLOCKS, MAX_OAMD_OBJECTS},
    toc::DecodingDelay,
    topology::{Ac4Topology, DecoderAction, RandomAccess, ResetReason, TopologyTransition},
};
#[cfg(feature = "audio-decode")]
use macindecode_ac4_decode::{
    frame_alignment::frame_alignment,
    full_ajoc::{
        DecodedFullAjocFrame, FullAjocAlignedSideInformation, FullAjocPcmChannel, FullAjocPcmSource,
    },
};

#[cfg(feature = "audio-decode")]
use crate::model::SceneFrameStorage;
use crate::model::{
    CartesianPosition, HeadphoneMode, HeadphoneState, MetadataFields, ObjectExtent, RawOamdState,
    SceneObjectState, SceneObjectStateParts, ZoneState,
};
#[cfg(feature = "audio-decode")]
use crate::{
    AccessUnitContext, BedKind, BitstreamFailure, CodecDelay, DecodeError, DecodeErrorContext,
    DecodeErrorKind, DecodeMode, DecodeStage, FrameDiagnostics, ObjectKind, PcmPlane,
    RawOamdCommonState, RawOamdTiming, RawOamdUpdate, ResetKind, SceneBed, SceneBedComponent,
    SceneElementId, SceneElementSource, SceneMetadataUpdate, SceneObject, ScenePath,
    ScenePresentation, SceneTimeline, SpeakerLabel, session::ResolvedPresentation,
};

/// OAMD 更新起点最大为 `31 + 63 * 32 = 2_047` 个样本；表 188 支持的最短
/// codec frame 为 384 个样本，因此同一输出帧最多汇合六批 raw AU 更新。
#[cfg(feature = "audio-decode")]
const MAX_METADATA_FRAME_SPAN: usize = 6;
#[cfg(feature = "audio-decode")]
const MAX_SCENE_METADATA_UPDATES: usize =
    MAX_OAMD_METADATA_BLOCKS.saturating_mul(MAX_METADATA_FRAME_SPAN);
#[cfg(feature = "audio-decode")]
type OamdElementIds = [Option<SceneElementId>; MAX_OAMD_OBJECTS];
#[cfg(feature = "audio-decode")]
type OamdSceneStates = [Option<SceneObjectState>; MAX_OAMD_OBJECTS];
#[cfg(feature = "audio-decode")]
type OamdUpdateSamples = [Option<i64>; MAX_OAMD_OBJECTS];
#[cfg(feature = "audio-decode")]
type MetadataFrameStates = (OamdSceneStates, OamdSceneStates);

// 10^(dB/20) = 2^(dB * log2(10)/20)。常量按 f64 完成换算后才收窄到 Scene f32。
const LOG2_10_OVER_20: f64 = 0.166_096_404_744_368_13;

/// 把已经完成继承合并的 OAMD 码值映射为 renderer 友好的完整状态。
///
/// distance 与 divergence 尚无经验证的通用场景语义，只留在 `raw` 中。保留的
/// zone 码会留在 `raw` 与 `ZoneState` 供诊断；三者都通过
/// `semantic_complete = false` 明确标记，不能借用其他格式的近似概念填充。
pub(crate) fn map_oamd_object_state(
    effective: ObjectMetadataState,
    additional: AdditionalObjectMetadata,
) -> SceneObjectState {
    let basic = effective.basic;
    let render = effective.render;
    let raw = RawOamdState::new(effective, additional);
    let semantic_complete = render.is_none_or(|state| {
        let other = state.other_properties;
        // TS103190-2:v1.3.1:6.3.9.8.7 表 104 将 zone_mask 7 保留。
        state.zone.zone_mask != Some(7)
            && other.object_at_infinity.is_none()
            && other.distance_factor_code.is_none()
            && other.divergence_mode.is_none()
            && other.divergence_table.is_none()
            && other.divergence_code.is_none()
    });

    SceneObjectState::from_parts(SceneObjectStateParts {
        metadata_active: effective.active,
        position: render.map(|state| map_position(state.position, additional)),
        linear_gain: basic.map(|state| map_gain(state.gain)),
        importance: basic.map(|state| map_importance(state.priority)),
        extent: render.and_then(|state| map_extent(state.other_properties.width)),
        zone: render.map(|state| map_zone(state.zone)),
        screen_factor: render.and_then(|state| {
            state
                .other_properties
                .screen_factor_code
                .map(|code| f32::from(u16::from(code).saturating_add(1)) / 8.0)
        }),
        depth_factor: render.and_then(|state| {
            state
                .other_properties
                .depth_factor
                .and_then(map_depth_factor)
        }),
        trim_disabled: additional.trim_disabled,
        headphone: additional.headphone.map(|state| {
            HeadphoneState::new(
                match state.render_mode {
                    0 => HeadphoneMode::Off,
                    1 => HeadphoneMode::Near,
                    2 => HeadphoneMode::Far,
                    3 => HeadphoneMode::Mid,
                    value => HeadphoneMode::Reserved(value),
                },
                state.head_tracking_disabled,
            )
        }),
        semantic_complete,
        raw,
    })
}

/// 比较相邻两份完整有效状态，返回语义上实际变化的字段，而不是简单复述 raw
/// block 中出现过的组。这样 `REUSE`、默认值重建和附加字段复位共用同一判据。
#[cfg_attr(
    not(any(test, feature = "audio-decode")),
    expect(dead_code, reason = "无 audio-decode 时只保留 Scene 语义映射数据契约")
)]
pub(crate) fn changed_oamd_fields(previous: RawOamdState, current: RawOamdState) -> MetadataFields {
    let previous_effective = previous.effective();
    let current_effective = current.effective();
    let previous_additional = previous.additional();
    let current_additional = current.additional();
    let previous_state = map_oamd_object_state(previous_effective, previous_additional);
    let current_state = map_oamd_object_state(current_effective, current_additional);
    let previous_render = previous_effective.render;
    let current_render = current_effective.render;
    let previous_other = previous_render.map(|state| state.other_properties);
    let current_other = current_render.map(|state| state.other_properties);
    let mut changed = MetadataFields::empty();

    if previous_state.metadata_active() != current_state.metadata_active() {
        changed = changed.union(MetadataFields::ACTIVE);
    }
    if previous_state.linear_gain() != current_state.linear_gain() {
        changed = changed.union(MetadataFields::GAIN);
    }
    if previous_state.importance() != current_state.importance() {
        changed = changed.union(MetadataFields::IMPORTANCE);
    }
    if previous_state.position() != current_state.position() {
        changed = changed.union(MetadataFields::POSITION);
    }
    if previous_state.extent() != current_state.extent() {
        changed = changed.union(MetadataFields::EXTENT);
    }
    if previous_state.zone() != current_state.zone() {
        changed = changed.union(MetadataFields::ZONE);
    }
    if previous_state.screen_factor() != current_state.screen_factor() {
        changed = changed.union(MetadataFields::SCREEN_FACTOR);
    }
    if previous_state.depth_factor() != current_state.depth_factor() {
        changed = changed.union(MetadataFields::DEPTH_FACTOR);
    }
    if (
        previous_other.and_then(|state| state.object_at_infinity),
        previous_other.and_then(|state| state.distance_factor_code),
    ) != (
        current_other.and_then(|state| state.object_at_infinity),
        current_other.and_then(|state| state.distance_factor_code),
    ) {
        changed = changed.union(MetadataFields::DISTANCE);
    }
    if (
        previous_other.and_then(|state| state.divergence_mode),
        previous_other.and_then(|state| state.divergence_table),
        previous_other.and_then(|state| state.divergence_code),
    ) != (
        current_other.and_then(|state| state.divergence_mode),
        current_other.and_then(|state| state.divergence_table),
        current_other.and_then(|state| state.divergence_code),
    ) {
        changed = changed.union(MetadataFields::DIVERGENCE);
    }
    if previous_state.trim_disabled() != current_state.trim_disabled() {
        changed = changed.union(MetadataFields::TRIM);
    }
    if previous_state.headphone() != current_state.headphone() {
        changed = changed.union(MetadataFields::HEADPHONE);
    }

    changed
}

fn map_position(
    base: QuantizedPosition,
    additional: AdditionalObjectMetadata,
) -> CartesianPosition {
    let semantic_extension = |code: Option<u8>| match code {
        Some(0) => 1.0,
        Some(1) => 2.0,
        Some(2) => -1.0,
        Some(3) => -2.0,
        _ => 0.0,
    };
    let extended = additional.extended_position.unwrap_or_default();
    let x = f64::from(base.x) / 31.0 - 1.0 + semantic_extension(extended.x) / 155.0;
    let y = 1.0 - f64::from(base.y) / 31.0 - semantic_extension(extended.y) / 155.0;
    let z_extension = match base.coding {
        PositionCoding::AbsoluteNegative => -semantic_extension(extended.z),
        PositionCoding::AbsolutePositive | PositionCoding::Differential => {
            semantic_extension(extended.z)
        }
    };
    let z = f64::from(base.z) / 15.0 + z_extension / 75.0;
    CartesianPosition::new(
        x.clamp(-1.0, 1.0) as f32,
        y.clamp(-1.0, 1.0) as f32,
        z.clamp(-1.0, 1.0) as f32,
    )
}

fn map_zone(zone: ZoneUpdate) -> ZoneState {
    let flag = zone.group_zone_flag.unwrap_or(0);
    ZoneState::new(
        !zone.grouped_defaults && flag & 0b001 != 0,
        zone.grouped_defaults || flag & 0b010 == 0,
        zone.zone_mask.unwrap_or(0),
    )
}

fn map_gain(gain: ObjectGainState) -> f32 {
    let db = match gain {
        ObjectGainState::Default => return 1.0,
        ObjectGainState::NegativeInfinity => return 0.0,
        ObjectGainState::Quantized(code) if code <= 14 => f64::from(15u8.saturating_sub(code)),
        ObjectGainState::Quantized(code) => 14.0 - f64::from(code),
    };
    math::exp2(db * LOG2_10_OVER_20) as f32
}

const fn map_importance(priority: ObjectPriorityState) -> f32 {
    match priority {
        ObjectPriorityState::Default => 1.0,
        ObjectPriorityState::Minimum => 0.0,
        ObjectPriorityState::Quantized(code) => code as f32 / 31.0,
    }
}

fn map_extent(width: Option<WidthUpdate>) -> Option<ObjectExtent> {
    match width {
        Some(WidthUpdate::Uniform(code)) => Some(ObjectExtent::Uniform(f32::from(code) / 31.0)),
        Some(WidthUpdate::Cartesian { x, y, z }) => Some(ObjectExtent::Cartesian {
            x: f32::from(x) / 31.0,
            y: f32::from(y) / 31.0,
            z: f32::from(z) / 31.0,
        }),
        None => None,
    }
}

const fn map_depth_factor(code: u8) -> Option<f32> {
    match code {
        0 => Some(0.25),
        1 => Some(0.5),
        2 => Some(1.0),
        3 => Some(2.0),
        _ => None,
    }
}

/// A-JOC engine 借用输出组装时需要的不可变控制快照。
#[cfg(feature = "audio-decode")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct FullPcmAssemblyInput<'a> {
    pub(crate) topology: &'a Ac4Topology,
    pub(crate) context: AccessUnitContext,
    pub(crate) presentation: ResolvedPresentation,
    pub(crate) mode: DecodeMode,
    pub(crate) transition: TopologyTransition,
    pub(crate) frame_length: u16,
    pub(crate) sampling_frequency_hz: u32,
}

/// Session 自持的 A-JOC Core/Full PCM 场景组装状态。
///
/// 首个可解码配置建立对象、LFE、group common 与有界元数据队列；稳定配置只
/// 覆盖既有有效长度，不重建元素或释放容量。逐对象状态、timing 与 PCM 使用同一
/// 份 engine 到期快照，跨越帧尾的更新留在 Session 自持队列中继续推进。
#[cfg(feature = "audio-decode")]
#[derive(Debug)]
pub(crate) struct SceneAssembler {
    next_element_id: u64,
    configured_generation: Option<u32>,
    configured_substream: Option<u32>,
    codec_sample_cursor: i64,
    oamd_states: OamdSceneStates,
    last_metadata_update_samples: OamdUpdateSamples,
    next_metadata_order: u64,
    pending_metadata_updates: Vec<SceneMetadataUpdate>,
    next_pending_metadata_updates: Vec<SceneMetadataUpdate>,
}

#[cfg(feature = "audio-decode")]
impl SceneAssembler {
    pub(crate) const fn new() -> Self {
        Self {
            next_element_id: 1,
            configured_generation: None,
            configured_substream: None,
            codec_sample_cursor: 0,
            oamd_states: [None; MAX_OAMD_OBJECTS],
            last_metadata_update_samples: [None; MAX_OAMD_OBJECTS],
            next_metadata_order: 0,
            pending_metadata_updates: Vec::new(),
            next_pending_metadata_updates: Vec::new(),
        }
    }

    /// 不连续会切断采样时间线，但同一配置代次的元素 identity 保持稳定。
    pub(crate) fn mark_discontinuity(&mut self) {
        self.codec_sample_cursor = 0;
        self.clear_metadata_history();
    }

    /// 显式 reset 建立新的 identity 域，但绝不回退单调 ID 计数器。
    pub(crate) fn reset(&mut self) {
        self.configured_generation = None;
        self.configured_substream = None;
        self.codec_sample_cursor = 0;
        self.clear_metadata_history();
    }

    pub(crate) fn assemble_pcm(
        &mut self,
        frames: &mut Vec<SceneFrameStorage>,
        input: FullPcmAssemblyInput<'_>,
        output: &DecodedFullAjocFrame<'_>,
    ) -> Result<usize, DecodeError> {
        let frame_length = input.frame_length;
        let expected_samples = usize::from(frame_length);
        if frame_length == 0 {
            return Err(assembly_error(input, "Ac4SceneFrame/timeline"));
        }
        let Some(alignment) = frame_alignment(frame_length) else {
            return Err(assembly_error(input, "Ac4SceneFrame/timeline/table188"));
        };
        let channel_shape = validate_channel_shape(output, input, expected_samples)?;

        let reset = match input.transition.action {
            DecoderAction::Reset { reason } => Some(map_reset_reason(reason)),
            DecoderAction::Continue | DecoderAction::WaitForRandomAccess { .. } => None,
        };
        if reset.is_some() {
            self.clear_metadata_history();
        }
        let frame_start = if reset.is_some() {
            0
        } else {
            self.codec_sample_cursor
        };
        let duration_samples = u32::from(frame_length);
        let next_cursor = frame_start
            .checked_add(i64::from(duration_samples))
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/timeline"))?;
        let random_access = input.topology.random_access() == RandomAccess::Full;
        if frames.is_empty() {
            frames.push(empty_frame_storage(
                input,
                frame_start,
                duration_samples,
                alignment,
            ));
        }
        let Some(frame) = frames.first_mut() else {
            return Err(assembly_error(input, "Ac4SceneFrame/storage"));
        };
        update_frame_header(
            frame,
            input,
            frame_start,
            duration_samples,
            alignment,
            random_access,
            reset,
            output,
        )?;

        let needs_configuration = self.configured_generation != Some(input.transition.generation)
            || self.configured_substream != Some(input.presentation.substream_index);
        if needs_configuration {
            self.clear_metadata_history();
            self.configure_elements(frame, input, channel_shape, expected_samples, output)?;
        } else {
            validate_configured_elements(frame, input, channel_shape, output)?;
        }
        copy_element_pcm(frame, input, output, expected_samples)?;
        self.assemble_metadata(frame, input, output, frame_start, duration_samples)?;

        self.configured_generation = Some(input.transition.generation);
        self.configured_substream = Some(input.presentation.substream_index);
        self.codec_sample_cursor = next_cursor;
        Ok(1)
    }

    fn configure_elements(
        &mut self,
        frame: &mut SceneFrameStorage,
        input: FullPcmAssemblyInput<'_>,
        shape: ChannelShape,
        samples: usize,
        output: &DecodedFullAjocFrame<'_>,
    ) -> Result<(), DecodeError> {
        frame.objects.truncate(shape.objects);
        frame.beds.truncate(shape.lfe_components);
        reserve_total(&mut frame.objects, shape.objects);
        reserve_total(&mut frame.beds, shape.lfe_components);
        reserve_total(&mut frame.metadata_updates, MAX_SCENE_METADATA_UPDATES);
        reserve_total(
            &mut self.pending_metadata_updates,
            MAX_SCENE_METADATA_UPDATES,
        );
        reserve_total(
            &mut self.next_pending_metadata_updates,
            MAX_SCENE_METADATA_UPDATES,
        );

        let mut object_position = 0usize;
        let mut bed_position = 0usize;
        for output_index in 0..scene_channels(output, input.mode) {
            let channel = scene_channel(output, input.mode, output_index)
                .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/pcm"))?;
            let output_index_u32 = u32::try_from(output_index)
                .map_err(|_| assembly_error(input, "Ac4SceneFrame/pcm/source"))?;
            match (input.mode, channel.source()) {
                (DecodeMode::Full, FullAjocPcmSource::AjocObject(object)) => {
                    let object_index =
                        oamd_object_index(object, input.presentation.ajoc_info.b_lfe)
                            .and_then(|index| u8::try_from(index).ok())
                            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/objects/source"))?;
                    let source = SceneElementSource::AjocObject {
                        substream_index: input.presentation.substream_index,
                        object_index,
                        output_index: output_index_u32,
                    };
                    let element_id = self.allocate_id(input)?;
                    if let Some(item) = frame.objects.get_mut(object_position) {
                        item.element_id = element_id;
                        item.kind = ObjectKind::AjocSpatialObjectGroup;
                        item.source = source;
                        item.content_classifier = None;
                        item.initial_state = None;
                        item.has_signal = false;
                        item.planes.truncate(1);
                        if item.planes.is_empty() {
                            item.planes.push(PcmPlane::with_capacity(samples));
                        } else if let Some(plane) = item.planes.first_mut() {
                            reserve_samples(plane, samples);
                        }
                        item.samples_per_plane = samples;
                    } else {
                        let planes = vec![PcmPlane::with_capacity(samples)];
                        frame.objects.push(SceneObject {
                            element_id,
                            kind: ObjectKind::AjocSpatialObjectGroup,
                            source,
                            content_classifier: None,
                            initial_state: None,
                            has_signal: false,
                            planes,
                            samples_per_plane: samples,
                        });
                    }
                    object_position = object_position.saturating_add(1);
                }
                (DecodeMode::Core, FullAjocPcmSource::AjocInput) => {
                    let object_index =
                        oamd_object_index(object_position, input.presentation.ajoc_info.b_lfe)
                            .and_then(|index| u8::try_from(index).ok())
                            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/objects/source"))?;
                    let source = SceneElementSource::AjocCoreObject {
                        substream_index: input.presentation.substream_index,
                        object_index,
                        input_index: output_index_u32,
                    };
                    let element_id = self.allocate_id(input)?;
                    if let Some(item) = frame.objects.get_mut(object_position) {
                        item.element_id = element_id;
                        item.kind = ObjectKind::AjocCoreObject;
                        item.source = source;
                        item.content_classifier = None;
                        item.initial_state = None;
                        item.has_signal = false;
                        item.planes.truncate(1);
                        if item.planes.is_empty() {
                            item.planes.push(PcmPlane::with_capacity(samples));
                        } else if let Some(plane) = item.planes.first_mut() {
                            reserve_samples(plane, samples);
                        }
                        item.samples_per_plane = samples;
                    } else {
                        frame.objects.push(SceneObject {
                            element_id,
                            kind: ObjectKind::AjocCoreObject,
                            source,
                            content_classifier: None,
                            initial_state: None,
                            has_signal: false,
                            planes: vec![PcmPlane::with_capacity(samples)],
                            samples_per_plane: samples,
                        });
                    }
                    object_position = object_position.saturating_add(1);
                }
                (mode, FullAjocPcmSource::Lfe) => {
                    let (kind, source) = match mode {
                        DecodeMode::Core => (
                            BedKind::AjocCoreLfe,
                            SceneElementSource::AjocCoreLfe {
                                substream_index: input.presentation.substream_index,
                                object_index: 0,
                                output_index: output_index_u32,
                            },
                        ),
                        DecodeMode::Full => (
                            BedKind::AjocLfe,
                            SceneElementSource::AjocLfe {
                                substream_index: input.presentation.substream_index,
                                object_index: 0,
                                reinsertion_index: output_index_u32,
                            },
                        ),
                    };
                    let element_id = self.allocate_id(input)?;
                    if let Some(item) = frame.beds.get_mut(bed_position) {
                        item.element_id = element_id;
                        item.kind = kind;
                        item.source = source;
                        item.content_classifier = None;
                        item.initial_state = None;
                        item.components.truncate(1);
                        if item.components.is_empty() {
                            item.components.push(SceneBedComponent {
                                speaker: SpeakerLabel::Lfe,
                                has_signal: false,
                                plane: PcmPlane::with_capacity(samples),
                            });
                        } else if let Some(component) = item.components.first_mut() {
                            component.speaker = SpeakerLabel::Lfe;
                            component.has_signal = false;
                            reserve_samples(&mut component.plane, samples);
                        }
                    } else {
                        frame.beds.push(SceneBed {
                            element_id,
                            kind,
                            source,
                            content_classifier: None,
                            initial_state: None,
                            components: vec![SceneBedComponent {
                                speaker: SpeakerLabel::Lfe,
                                has_signal: false,
                                plane: PcmPlane::with_capacity(samples),
                            }],
                        });
                    }
                    bed_position = bed_position.saturating_add(1);
                }
                (DecodeMode::Core, FullAjocPcmSource::AjocObject(_))
                | (DecodeMode::Full, FullAjocPcmSource::AjocInput) => {
                    return Err(assembly_error(input, "Ac4SceneFrame/pcm/source"));
                }
            }
        }
        Ok(())
    }

    fn assemble_metadata(
        &mut self,
        frame: &mut SceneFrameStorage,
        input: FullPcmAssemblyInput<'_>,
        output: &DecodedFullAjocFrame<'_>,
        frame_start: i64,
        duration_samples: u32,
    ) -> Result<(), DecodeError> {
        let (element_ids, element_count) = oamd_element_ids(frame, input)?;
        let aligned = output.aligned_side_information();

        frame.metadata_updates.clear();
        self.next_pending_metadata_updates.clear();
        for update in &self.pending_metadata_updates {
            partition_metadata_update(
                &mut frame.metadata_updates,
                &mut self.next_pending_metadata_updates,
                *update,
                duration_samples,
            )
            .map_err(|()| assembly_error(input, "Ac4SceneFrame/metadata_updates/capacity"))?;
        }

        let mut next_order = self.next_metadata_order;
        let mut last_metadata_update_samples = self.last_metadata_update_samples;
        if let Some(side_information) = aligned {
            let mut schedule = MetadataUpdateSchedule {
                current: &mut frame.metadata_updates,
                future: &mut self.next_pending_metadata_updates,
                frame_start,
                duration_samples,
                next_order: &mut next_order,
                last_update_samples: &mut last_metadata_update_samples,
            };
            append_aligned_metadata_updates(&mut schedule, &element_ids, side_information, input)?;
        }
        frame
            .metadata_updates
            .sort_unstable_by_key(|update| (update.offset_samples(), update.stream_order()));

        let (initial_states, end_states) =
            metadata_frame_states(self.oamd_states, &element_ids, &frame.metadata_updates)
                .map_err(|path| assembly_error(input, path))?;
        set_initial_states(frame, input, &initial_states)?;
        let state_complete = element_count != 0
            && element_ids.iter().enumerate().all(|(index, element_id)| {
                element_id.is_none()
                    || initial_states
                        .get(index)
                        .copied()
                        .flatten()
                        .is_some_and(scene_state_is_complete)
            });

        frame.diagnostics.state_complete = state_complete;
        frame.diagnostics.semantic_metadata_complete = state_complete
            && frame.objects.iter().all(|object| {
                object
                    .initial_state
                    .is_some_and(|state| state.semantic_complete())
            })
            && frame.beds.iter().all(|bed| {
                bed.initial_state
                    .is_some_and(|state| state.semantic_complete())
            })
            && frame
                .metadata_updates
                .iter()
                .all(|update| update.state().semantic_complete());

        self.oamd_states = end_states;
        self.last_metadata_update_samples = last_metadata_update_samples;
        self.next_metadata_order = next_order;
        core::mem::swap(
            &mut self.pending_metadata_updates,
            &mut self.next_pending_metadata_updates,
        );
        self.next_pending_metadata_updates.clear();
        Ok(())
    }

    fn allocate_id(
        &mut self,
        input: FullPcmAssemblyInput<'_>,
    ) -> Result<SceneElementId, DecodeError> {
        let id = SceneElementId::new(self.next_element_id);
        self.next_element_id = self
            .next_element_id
            .checked_add(1)
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/element_id"))?;
        Ok(id)
    }

    fn clear_metadata_history(&mut self) {
        self.oamd_states.fill(None);
        self.last_metadata_update_samples.fill(None);
        self.next_metadata_order = 0;
        self.pending_metadata_updates.clear();
        self.next_pending_metadata_updates.clear();
    }
}

#[cfg(feature = "audio-decode")]
impl Default for SceneAssembler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "audio-decode")]
fn oamd_element_ids(
    frame: &SceneFrameStorage,
    input: FullPcmAssemblyInput<'_>,
) -> Result<(OamdElementIds, usize), DecodeError> {
    let mut element_ids = [None; MAX_OAMD_OBJECTS];
    for object in &frame.objects {
        register_oamd_element(&mut element_ids, object.source, object.element_id, input)?;
    }
    for bed in &frame.beds {
        register_oamd_element(&mut element_ids, bed.source, bed.element_id, input)?;
    }
    let element_count = frame.objects.len().saturating_add(frame.beds.len());
    if element_ids.iter().filter(|id| id.is_some()).count() != element_count {
        return Err(assembly_error(input, "Ac4SceneFrame/elements/oamd_index"));
    }
    Ok((element_ids, element_count))
}

#[cfg(feature = "audio-decode")]
fn register_oamd_element(
    element_ids: &mut OamdElementIds,
    source: SceneElementSource,
    element_id: SceneElementId,
    input: FullPcmAssemblyInput<'_>,
) -> Result<(), DecodeError> {
    let (substream_index, object_index) = match source {
        SceneElementSource::AjocCoreObject {
            substream_index,
            object_index,
            ..
        }
        | SceneElementSource::AjocCoreLfe {
            substream_index,
            object_index,
            ..
        }
        | SceneElementSource::AjocObject {
            substream_index,
            object_index,
            ..
        }
        | SceneElementSource::AjocLfe {
            substream_index,
            object_index,
            ..
        } => (substream_index, object_index),
    };
    if substream_index != input.presentation.substream_index {
        return Err(assembly_error(input, "Ac4SceneFrame/elements/substream"));
    }
    let slot = element_ids
        .get_mut(usize::from(object_index))
        .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/elements/oamd_index"))?;
    if slot.is_some() {
        return Err(assembly_error(input, "Ac4SceneFrame/elements/oamd_index"));
    }
    *slot = Some(element_id);
    Ok(())
}

#[cfg(feature = "audio-decode")]
fn metadata_frame_states(
    inherited: OamdSceneStates,
    element_ids: &OamdElementIds,
    updates: &[SceneMetadataUpdate],
) -> Result<MetadataFrameStates, &'static str> {
    let mut initial = inherited;
    let mut end = inherited;
    for update in updates {
        let object_index = usize::from(update.raw().block().object_index);
        let expected_id = element_ids
            .get(object_index)
            .copied()
            .flatten()
            .ok_or("Ac4SceneFrame/metadata_updates/element_id")?;
        if expected_id != update.element_id() {
            return Err("Ac4SceneFrame/metadata_updates/element_id");
        }
        if !scene_state_is_complete(update.state()) {
            return Err("Ac4SceneFrame/metadata_updates/state");
        }
        if update.offset_samples() == 0 {
            let initial_slot = initial
                .get_mut(object_index)
                .ok_or("Ac4SceneFrame/metadata_updates/object_index")?;
            *initial_slot = Some(update.state());
        }
        let end_slot = end
            .get_mut(object_index)
            .ok_or("Ac4SceneFrame/metadata_updates/object_index")?;
        *end_slot = Some(update.state());
    }
    Ok((initial, end))
}

#[cfg(feature = "audio-decode")]
fn set_initial_states(
    frame: &mut SceneFrameStorage,
    input: FullPcmAssemblyInput<'_>,
    states: &OamdSceneStates,
) -> Result<(), DecodeError> {
    for object in &mut frame.objects {
        let index = scene_source_oamd_index(object.source, input)?;
        object.initial_state = states
            .get(index)
            .copied()
            .flatten()
            .filter(|state| scene_state_is_complete(*state));
    }
    for bed in &mut frame.beds {
        let index = scene_source_oamd_index(bed.source, input)?;
        bed.initial_state = states
            .get(index)
            .copied()
            .flatten()
            .filter(|state| scene_state_is_complete(*state));
    }
    Ok(())
}

#[cfg(feature = "audio-decode")]
fn scene_source_oamd_index(
    source: SceneElementSource,
    input: FullPcmAssemblyInput<'_>,
) -> Result<usize, DecodeError> {
    let (substream_index, object_index) = match source {
        SceneElementSource::AjocCoreObject {
            substream_index,
            object_index,
            ..
        }
        | SceneElementSource::AjocCoreLfe {
            substream_index,
            object_index,
            ..
        }
        | SceneElementSource::AjocObject {
            substream_index,
            object_index,
            ..
        }
        | SceneElementSource::AjocLfe {
            substream_index,
            object_index,
            ..
        } => (substream_index, object_index),
    };
    if substream_index != input.presentation.substream_index {
        return Err(assembly_error(input, "Ac4SceneFrame/elements/substream"));
    }
    Ok(usize::from(object_index))
}

#[cfg(feature = "audio-decode")]
struct MetadataUpdateSchedule<'a> {
    current: &'a mut Vec<SceneMetadataUpdate>,
    future: &'a mut Vec<SceneMetadataUpdate>,
    frame_start: i64,
    duration_samples: u32,
    next_order: &'a mut u64,
    last_update_samples: &'a mut OamdUpdateSamples,
}

#[cfg(feature = "audio-decode")]
fn append_aligned_metadata_updates(
    schedule: &mut MetadataUpdateSchedule<'_>,
    element_ids: &OamdElementIds,
    aligned: FullAjocAlignedSideInformation<'_>,
    input: FullPcmAssemblyInput<'_>,
) -> Result<(), DecodeError> {
    let (snapshot, timing_state, num_obj_info_blocks) = match input.mode {
        DecodeMode::Core => (
            aligned.dmx_oamd(),
            aligned.dmx_effective_oamd_timing(),
            aligned.dmx_num_obj_info_blocks(),
        ),
        DecodeMode::Full => (
            aligned.umx_oamd(),
            aligned.umx_effective_oamd_timing(),
            aligned.umx_num_obj_info_blocks(),
        ),
    };
    let updates = snapshot.updates();
    if updates.is_empty() {
        return Ok(());
    }
    let timing = timing_state
        .effective()
        .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/metadata_updates/timing"))?;
    if timing.num_obj_info_blocks != num_obj_info_blocks {
        return Err(assembly_error(
            input,
            "Ac4SceneFrame/metadata_updates/timing/shape",
        ));
    }
    let control_source_access_unit_index = aligned.provenance().access_unit_index();

    let mut previous = [None; MAX_OAMD_OBJECTS];
    for (object_index, slot) in previous.iter_mut().enumerate() {
        *slot = snapshot
            .object_at_start(object_index)
            .map(|state| RawOamdState::new(state.metadata(), state.additional()));
    }
    for update in updates {
        let raw = update.raw();
        let object_index = usize::from(raw.object_index);
        let element_id = element_ids
            .get(object_index)
            .copied()
            .flatten()
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/metadata_updates/element_id"))?;
        let block = timing
            .blocks()
            .get(usize::from(raw.block_index))
            .copied()
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/metadata_updates/block_index"))?;
        let resolved = update.state();
        let raw_state = RawOamdState::new(resolved.metadata(), resolved.additional());
        let prior = previous
            .get(object_index)
            .copied()
            .flatten()
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/metadata_updates/state"))?;
        let state = map_oamd_object_state(resolved.metadata(), resolved.additional());
        if !scene_state_is_complete(state) {
            return Err(assembly_error(
                input,
                "Ac4SceneFrame/metadata_updates/state",
            ));
        }
        let offset_samples = u32::from(timing.sample_offset)
            .checked_add(block.offset_samples())
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/metadata_updates/offset"))?;
        match register_metadata_update_sample(
            schedule.last_update_samples,
            object_index,
            schedule.frame_start,
            offset_samples,
        ) {
            Ok(()) => {}
            Err(MetadataUpdateTimeError::ObjectIndex) => {
                return Err(assembly_error(
                    input,
                    "Ac4SceneFrame/metadata_updates/object_index",
                ));
            }
            Err(MetadataUpdateTimeError::Overflow) => {
                return Err(assembly_error(
                    input,
                    "Ac4SceneFrame/metadata_updates/offset",
                ));
            }
            Err(MetadataUpdateTimeError::Reordered { previous, current }) => {
                return Err(metadata_update_order_error(
                    input,
                    raw.object_index,
                    previous,
                    current,
                ));
            }
        }
        let following_order = (*schedule.next_order)
            .checked_add(1)
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/metadata_updates/order"))?;
        let raw_timing = RawOamdTiming::new(
            timing.offset_source,
            timing.sample_offset,
            timing.num_obj_info_blocks,
            block,
            timing_state.updated_in_source_access_unit(),
        );
        let scene_update = SceneMetadataUpdate::new(
            element_id,
            offset_samples,
            u32::from(block.ramp_duration),
            changed_oamd_fields(prior, raw_state),
            state,
            RawOamdUpdate::new(raw, raw_timing, control_source_access_unit_index),
            *schedule.next_order,
        );
        partition_metadata_update(
            schedule.current,
            schedule.future,
            scene_update,
            schedule.duration_samples,
        )
        .map_err(|()| assembly_error(input, "Ac4SceneFrame/metadata_updates/capacity"))?;
        let prior_slot = previous
            .get_mut(object_index)
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/metadata_updates/state"))?;
        *prior_slot = Some(raw_state);
        *schedule.next_order = following_order;
    }
    Ok(())
}

#[cfg(feature = "audio-decode")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataUpdateTimeError {
    ObjectIndex,
    Overflow,
    Reordered { previous: i64, current: i64 },
}

#[cfg(feature = "audio-decode")]
fn register_metadata_update_sample(
    last_update_samples: &mut OamdUpdateSamples,
    object_index: usize,
    frame_start: i64,
    offset_samples: u32,
) -> Result<(), MetadataUpdateTimeError> {
    let current = frame_start
        .checked_add(i64::from(offset_samples))
        .ok_or(MetadataUpdateTimeError::Overflow)?;
    let slot = last_update_samples
        .get_mut(object_index)
        .ok_or(MetadataUpdateTimeError::ObjectIndex)?;
    if let Some(previous) = *slot
        && current < previous
    {
        return Err(MetadataUpdateTimeError::Reordered { previous, current });
    }
    *slot = Some(current);
    Ok(())
}

#[cfg(feature = "audio-decode")]
fn partition_metadata_update(
    current: &mut Vec<SceneMetadataUpdate>,
    future: &mut Vec<SceneMetadataUpdate>,
    update: SceneMetadataUpdate,
    duration_samples: u32,
) -> Result<(), ()> {
    if update.offset_samples() < duration_samples {
        push_bounded_metadata_update(current, update)
    } else {
        let remaining = update.offset_samples().saturating_sub(duration_samples);
        push_bounded_metadata_update(future, update.with_offset_samples(remaining))
    }
}

#[cfg(feature = "audio-decode")]
fn push_bounded_metadata_update(
    updates: &mut Vec<SceneMetadataUpdate>,
    update: SceneMetadataUpdate,
) -> Result<(), ()> {
    if updates.len() >= MAX_SCENE_METADATA_UPDATES
        || updates.capacity() < MAX_SCENE_METADATA_UPDATES
    {
        return Err(());
    }
    updates.push(update);
    Ok(())
}

#[cfg(feature = "audio-decode")]
fn scene_state_is_complete(state: SceneObjectState) -> bool {
    let effective = state.raw().effective();
    effective.basic.is_some() && effective.render.is_some()
}

#[cfg(feature = "audio-decode")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChannelShape {
    objects: usize,
    lfe_components: usize,
}

#[cfg(feature = "audio-decode")]
const fn scene_channels(output: &DecodedFullAjocFrame<'_>, mode: DecodeMode) -> usize {
    match mode {
        DecodeMode::Core => output.diagnostic_channels(),
        DecodeMode::Full => output.reconstructed_channels(),
    }
}

#[cfg(feature = "audio-decode")]
fn scene_channel<'a>(
    output: &DecodedFullAjocFrame<'a>,
    mode: DecodeMode,
    index: usize,
) -> Option<FullAjocPcmChannel<'a>> {
    match mode {
        DecodeMode::Core => output.diagnostic_channel(index),
        DecodeMode::Full => output.reconstructed_channel(index),
    }
}

#[cfg(feature = "audio-decode")]
fn validate_channel_shape(
    output: &DecodedFullAjocFrame<'_>,
    input: FullPcmAssemblyInput<'_>,
    expected_samples: usize,
) -> Result<ChannelShape, DecodeError> {
    let mut objects = 0usize;
    let mut lfe_components = 0usize;
    for output_index in 0..scene_channels(output, input.mode) {
        let channel = scene_channel(output, input.mode, output_index)
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/pcm"))?;
        if channel.samples().len() != expected_samples {
            return Err(assembly_error(input, "Ac4SceneFrame/pcm/length"));
        }
        if channel.samples().iter().any(|sample| !sample.is_finite()) {
            return Err(assembly_error(input, "Ac4SceneFrame/pcm/non_finite"));
        }
        match (input.mode, channel.source()) {
            (DecodeMode::Core, FullAjocPcmSource::AjocInput)
                if output_index == objects && lfe_components == 0 =>
            {
                objects = objects.saturating_add(1);
            }
            (DecodeMode::Full, FullAjocPcmSource::AjocObject(object)) if object == objects => {
                objects = objects.saturating_add(1);
            }
            (_, FullAjocPcmSource::Lfe) if lfe_components == 0 => {
                lfe_components = 1;
            }
            _ => {
                return Err(assembly_error(input, "Ac4SceneFrame/pcm/source"));
            }
        }
    }
    let declared_objects = usize::try_from(match input.mode {
        DecodeMode::Core => input.presentation.ajoc_info.n_dmx_signals,
        DecodeMode::Full => input.presentation.ajoc_info.n_upmix_signals,
    })
    .map_err(|_| assembly_error(input, "Ac4SceneFrame/objects"))?;
    if objects != declared_objects
        || lfe_components != usize::from(input.presentation.ajoc_info.b_lfe)
        || scene_channels(output, input.mode) == 0
    {
        return Err(assembly_error(input, "Ac4SceneFrame/pcm/shape"));
    }
    Ok(ChannelShape {
        objects,
        lfe_components,
    })
}

#[cfg(feature = "audio-decode")]
fn validate_configured_elements(
    frame: &SceneFrameStorage,
    input: FullPcmAssemblyInput<'_>,
    shape: ChannelShape,
    output: &DecodedFullAjocFrame<'_>,
) -> Result<(), DecodeError> {
    if frame.objects.len() != shape.objects || frame.beds.len() != shape.lfe_components {
        return Err(assembly_error(input, "Ac4SceneFrame/elements/shape"));
    }
    let mut object_position = 0usize;
    let mut bed_position = 0usize;
    for output_index in 0..scene_channels(output, input.mode) {
        let source = scene_channel(output, input.mode, output_index)
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/pcm"))?
            .source();
        let expected = scene_source(input, source, output_index)?;
        let actual = match (input.mode, source) {
            (DecodeMode::Core, FullAjocPcmSource::AjocInput)
            | (DecodeMode::Full, FullAjocPcmSource::AjocObject(_)) => {
                let item = frame
                    .objects
                    .get(object_position)
                    .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/objects/shape"))?;
                object_position = object_position.saturating_add(1);
                item.source
            }
            (_, FullAjocPcmSource::Lfe) => {
                let item = frame
                    .beds
                    .get(bed_position)
                    .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/beds/shape"))?;
                bed_position = bed_position.saturating_add(1);
                item.source
            }
            (DecodeMode::Core, FullAjocPcmSource::AjocObject(_))
            | (DecodeMode::Full, FullAjocPcmSource::AjocInput) => {
                return Err(assembly_error(input, "Ac4SceneFrame/pcm/source"));
            }
        };
        if actual != expected {
            return Err(assembly_error(input, "Ac4SceneFrame/elements/source"));
        }
    }
    Ok(())
}

#[cfg(feature = "audio-decode")]
fn copy_element_pcm(
    frame: &mut SceneFrameStorage,
    input: FullPcmAssemblyInput<'_>,
    output: &DecodedFullAjocFrame<'_>,
    expected_samples: usize,
) -> Result<(), DecodeError> {
    let mut object_position = 0usize;
    let mut bed_position = 0usize;
    for output_index in 0..scene_channels(output, input.mode) {
        let channel = scene_channel(output, input.mode, output_index)
            .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/pcm"))?;
        match (input.mode, channel.source()) {
            (DecodeMode::Core, FullAjocPcmSource::AjocInput)
            | (DecodeMode::Full, FullAjocPcmSource::AjocObject(_)) => {
                let item = frame
                    .objects
                    .get_mut(object_position)
                    .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/objects/shape"))?;
                let plane = item
                    .planes
                    .first_mut()
                    .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/objects/pcm"))?;
                item.has_signal = plane
                    .copy_normalized_from(channel.samples())
                    .map_err(|_| assembly_error(input, "Ac4SceneFrame/pcm/non_finite"))?;
                item.samples_per_plane = expected_samples;
                object_position = object_position.saturating_add(1);
            }
            (_, FullAjocPcmSource::Lfe) => {
                let item = frame
                    .beds
                    .get_mut(bed_position)
                    .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/beds/shape"))?;
                let component = item
                    .components
                    .first_mut()
                    .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/beds/pcm"))?;
                component.has_signal = component
                    .plane
                    .copy_normalized_from(channel.samples())
                    .map_err(|_| assembly_error(input, "Ac4SceneFrame/pcm/non_finite"))?;
                bed_position = bed_position.saturating_add(1);
            }
            (DecodeMode::Core, FullAjocPcmSource::AjocObject(_))
            | (DecodeMode::Full, FullAjocPcmSource::AjocInput) => {
                return Err(assembly_error(input, "Ac4SceneFrame/pcm/source"));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "audio-decode")]
fn empty_frame_storage(
    input: FullPcmAssemblyInput<'_>,
    frame_start: i64,
    duration_samples: u32,
    alignment: macindecode_ac4_decode::frame_alignment::FrameAlignment,
) -> SceneFrameStorage {
    SceneFrameStorage {
        timeline: timeline(input, frame_start, duration_samples, alignment, None),
        presentation: ScenePresentation {
            index: input.presentation.index,
            id: input.presentation.id,
            identity_occurrences: input.presentation.identity_occurrences,
            version: 0,
            md_compat: None,
            group_indices: Vec::new(),
            substream_indices: Vec::new(),
            path: ScenePath::Ajoc,
            mode: input.mode,
        },
        oamd_common_states: Vec::new(),
        beds: Vec::new(),
        objects: Vec::new(),
        metadata_updates: Vec::new(),
        diagnostics: FrameDiagnostics {
            reset: None,
            random_access: false,
            random_access_hint_mismatch: false,
            configuration_changed: false,
            discontinuity: false,
            warmup: true,
            state_complete: false,
            concealed: false,
            semantic_metadata_complete: false,
        },
    }
}

#[cfg(feature = "audio-decode")]
#[expect(
    clippy::too_many_arguments,
    reason = "SceneFrame header 同时绑定当前 AU、配置转移、表 188 与 engine 观察"
)]
fn update_frame_header(
    frame: &mut SceneFrameStorage,
    input: FullPcmAssemblyInput<'_>,
    frame_start: i64,
    duration_samples: u32,
    alignment: macindecode_ac4_decode::frame_alignment::FrameAlignment,
    random_access: bool,
    reset: Option<ResetKind>,
    output: &DecodedFullAjocFrame<'_>,
) -> Result<(), DecodeError> {
    let presentation_position = usize::try_from(input.presentation.index)
        .map_err(|_| assembly_error(input, "Ac4SceneFrame/presentation"))?;
    let presentation = input
        .topology
        .presentations()
        .get(presentation_position)
        .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/presentation"))?;
    frame.timeline = timeline(
        input,
        frame_start,
        duration_samples,
        alignment,
        output
            .aligned_side_information()
            .map(|side| side.provenance().access_unit_index()),
    );
    frame.presentation.index = input.presentation.index;
    frame.presentation.id = input.presentation.id;
    frame.presentation.identity_occurrences = input.presentation.identity_occurrences;
    frame.presentation.version = presentation.presentation_version;
    frame.presentation.md_compat = presentation.md_compat;
    frame.presentation.group_indices.clear();
    frame
        .presentation
        .group_indices
        .extend_from_slice(presentation.group_indices());
    frame.presentation.substream_indices.clear();
    frame
        .presentation
        .substream_indices
        .push(input.presentation.substream_index);
    frame.presentation.path = ScenePath::Ajoc;
    frame.presentation.mode = input.mode;
    update_group_oamd_states(frame, input, input.presentation.group_mask, output)?;
    frame.metadata_updates.clear();
    frame.diagnostics = FrameDiagnostics {
        reset,
        random_access,
        random_access_hint_mismatch: input
            .context
            .random_access_hint()
            .is_some_and(|hint| hint != random_access),
        configuration_changed: input.transition.config_changed
            || matches!(
                reset,
                Some(ResetKind::Initial | ResetKind::ConfigurationChange)
            ),
        discontinuity: input.context.discontinuity()
            || reset == Some(ResetKind::ExternalDiscontinuity),
        // Scene 的 warm-up 指「还没有与 PCM 同时到期的控制快照」。AspxOnly
        // observation 不执行 Full 重建，不能用其 Full 专用 warmup 位替代该判据。
        warmup: output.aligned_side_information().is_none(),
        // 元数据组装在 header、元素与 PCM 全部验证后一次性覆盖这两个完整性字段。
        state_complete: false,
        concealed: false,
        semantic_metadata_complete: false,
    };
    Ok(())
}

#[cfg(feature = "audio-decode")]
fn update_group_oamd_states(
    frame: &mut SceneFrameStorage,
    input: FullPcmAssemblyInput<'_>,
    expected_group_mask: u8,
    output: &DecodedFullAjocFrame<'_>,
) -> Result<(), DecodeError> {
    let expected_group_count =
        usize::try_from(expected_group_mask.count_ones()).unwrap_or(usize::MAX);
    reserve_total(&mut frame.oamd_common_states, expected_group_count);
    frame.oamd_common_states.clear();
    let Some(aligned) = output.aligned_side_information() else {
        return Ok(());
    };
    let states = aligned.group_oamd_states();
    if states.len() != expected_group_count {
        return Err(assembly_error(
            input,
            "Ac4SceneFrame/oamd_common_states/shape",
        ));
    }
    let expected_group_indices = (0..u8::BITS).filter(|group_index| {
        expected_group_mask & 1u8.checked_shl(*group_index).unwrap_or(0) != 0
    });
    for (state, expected_group_index) in states.iter().zip(expected_group_indices) {
        if state.group_index() != expected_group_index {
            return Err(assembly_error(
                input,
                "Ac4SceneFrame/oamd_common_states/group_index",
            ));
        }
        frame.oamd_common_states.push(RawOamdCommonState::new(
            state.group_index(),
            state.effective_common(),
            state.common_updated_in_source_access_unit(),
        ));
    }
    Ok(())
}

#[cfg(feature = "audio-decode")]
fn timeline(
    input: FullPcmAssemblyInput<'_>,
    frame_start: i64,
    duration_samples: u32,
    alignment: macindecode_ac4_decode::frame_alignment::FrameAlignment,
    control_source_access_unit_index: Option<u64>,
) -> SceneTimeline {
    SceneTimeline {
        sample_rate: input.sampling_frequency_hz,
        codec_sample_start: frame_start,
        source_sample_start: input.context.source_sample_start(),
        presentation_sample_start: input.context.presentation_sample_start(),
        duration_samples,
        access_unit_index: input.context.index(),
        control_source_access_unit_index,
        random_access: input.topology.random_access() == RandomAccess::Full,
        configuration_generation: input.transition.generation,
        priming_samples: input.context.priming_samples(),
        codec_delay: input.topology.toc.decoding_delay().map(map_codec_delay),
        pcm_alignment_delay_samples: alignment.pcm_delay(),
        control_alignment_delay_frames: alignment.control_delay_frames(),
    }
}

#[cfg(feature = "audio-decode")]
fn scene_source(
    input: FullPcmAssemblyInput<'_>,
    source: FullAjocPcmSource,
    output_index: usize,
) -> Result<SceneElementSource, DecodeError> {
    let output_index = u32::try_from(output_index)
        .map_err(|_| assembly_error(input, "Ac4SceneFrame/pcm/source"))?;
    match (input.mode, source) {
        (DecodeMode::Core, FullAjocPcmSource::AjocInput) => {
            let object_index = usize::try_from(output_index)
                .ok()
                .and_then(|index| oamd_object_index(index, input.presentation.ajoc_info.b_lfe))
                .and_then(|index| u8::try_from(index).ok())
                .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/objects/source"))?;
            Ok(SceneElementSource::AjocCoreObject {
                substream_index: input.presentation.substream_index,
                object_index,
                input_index: output_index,
            })
        }
        (DecodeMode::Full, FullAjocPcmSource::AjocObject(object)) => {
            let object_index = oamd_object_index(object, input.presentation.ajoc_info.b_lfe)
                .and_then(|index| u8::try_from(index).ok())
                .ok_or_else(|| assembly_error(input, "Ac4SceneFrame/objects/source"))?;
            Ok(SceneElementSource::AjocObject {
                substream_index: input.presentation.substream_index,
                object_index,
                output_index,
            })
        }
        (DecodeMode::Core, FullAjocPcmSource::Lfe) => Ok(SceneElementSource::AjocCoreLfe {
            substream_index: input.presentation.substream_index,
            object_index: 0,
            output_index,
        }),
        (DecodeMode::Full, FullAjocPcmSource::Lfe) => Ok(SceneElementSource::AjocLfe {
            substream_index: input.presentation.substream_index,
            object_index: 0,
            reinsertion_index: output_index,
        }),
        (DecodeMode::Core, FullAjocPcmSource::AjocObject(_))
        | (DecodeMode::Full, FullAjocPcmSource::AjocInput) => {
            Err(assembly_error(input, "Ac4SceneFrame/pcm/source"))
        }
    }
}

#[cfg(feature = "audio-decode")]
const fn oamd_object_index(object: usize, has_lfe: bool) -> Option<usize> {
    object.checked_add(has_lfe as usize)
}

#[cfg(feature = "audio-decode")]
fn reserve_total<T>(items: &mut Vec<T>, total: usize) {
    if items.capacity() < total {
        items.reserve(total.saturating_sub(items.len()));
    }
}

#[cfg(feature = "audio-decode")]
fn reserve_samples(plane: &mut PcmPlane, total: usize) {
    if plane.samples.capacity() < total {
        plane
            .samples
            .reserve(total.saturating_sub(plane.samples.len()));
    }
}

#[cfg(feature = "audio-decode")]
const fn map_codec_delay(delay: DecodingDelay) -> CodecDelay {
    match delay {
        DecodingDelay::ConstantBitRate => CodecDelay::ConstantBitRate,
        DecodingDelay::Frames(frames) => CodecDelay::Frames(frames),
        DecodingDelay::VariableBitRate => CodecDelay::VariableBitRate,
    }
}

#[cfg(feature = "audio-decode")]
const fn map_reset_reason(reason: ResetReason) -> ResetKind {
    match reason {
        ResetReason::Initial => ResetKind::Initial,
        ResetReason::SourceChange => ResetKind::SourceChange,
        ResetReason::ConfigurationChange => ResetKind::ConfigurationChange,
        ResetReason::ParseFailure => ResetKind::ParseFailure,
        ResetReason::ExternalDiscontinuity => ResetKind::ExternalDiscontinuity,
    }
}

#[cfg(feature = "audio-decode")]
fn assembly_error_context(
    input: FullPcmAssemblyInput<'_>,
    path: &'static str,
) -> DecodeErrorContext {
    let mut context = DecodeErrorContext::for_access_unit(input.context.index())
        .with_presentation(input.presentation.index, input.presentation.id)
        .with_substream(input.presentation.substream_index)
        .with_syntax_path(path);
    if input.presentation.group_mask != 0 {
        context = context.with_group(input.presentation.group_mask.trailing_zeros());
    }
    context
}

#[cfg(feature = "audio-decode")]
fn assembly_error(input: FullPcmAssemblyInput<'_>, path: &'static str) -> DecodeError {
    DecodeError::new(
        DecodeErrorKind::InternalInvariant {
            stage: DecodeStage::SceneAssembly,
        },
        assembly_error_context(input, path),
    )
}

#[cfg(feature = "audio-decode")]
fn metadata_update_order_error(
    input: FullPcmAssemblyInput<'_>,
    object_index: u8,
    previous_sample: i64,
    current_sample: i64,
) -> DecodeError {
    DecodeError::new(
        DecodeErrorKind::InvalidBitstream(BitstreamFailure::OamdUpdateOrder {
            object_index,
            previous_sample,
            current_sample,
        }),
        assembly_error_context(input, "Ac4SceneFrame/metadata_updates/order"),
    )
}

#[cfg(test)]
mod tests {
    use macindecode_ac4_bitstream::oamd::{
        ExtendedPrecisionPosition, ObjectBasicState, ObjectHeadphone, ObjectRenderState,
        OtherPropertiesUpdate, ZoneUpdate,
    };
    #[cfg(feature = "audio-decode")]
    use macindecode_ac4_bitstream::oamd::{
        OamdMetadataBlock, ObjInfoBlockTiming, SampleOffsetSource,
    };

    use super::*;

    fn complete_state(
        gain: ObjectGainState,
        priority: ObjectPriorityState,
        other_properties: OtherPropertiesUpdate,
    ) -> ObjectMetadataState {
        ObjectMetadataState {
            active: true,
            basic: Some(ObjectBasicState { gain, priority }),
            render: Some(ObjectRenderState {
                position: QuantizedPosition {
                    x: 31,
                    y: 31,
                    z: -3,
                    coding: PositionCoding::AbsoluteNegative,
                },
                zone: ZoneUpdate {
                    grouped_defaults: false,
                    group_zone_flag: Some(0b011),
                    zone_mask: Some(4),
                },
                other_properties,
            }),
        }
    }

    #[cfg(feature = "audio-decode")]
    fn queued_update(offset_samples: u32, stream_order: u64) -> SceneMetadataUpdate {
        queued_update_from(offset_samples, stream_order, 41)
    }

    #[cfg(feature = "audio-decode")]
    fn queued_update_from(
        offset_samples: u32,
        stream_order: u64,
        control_source_access_unit_index: u64,
    ) -> SceneMetadataUpdate {
        let effective = complete_state(
            ObjectGainState::Default,
            ObjectPriorityState::Default,
            OtherPropertiesUpdate::default(),
        );
        let state = map_oamd_object_state(effective, AdditionalObjectMetadata::default());
        let block = ObjInfoBlockTiming {
            block_offset_factor: 1,
            ramp_duration_code: 2,
            ramp_duration_encoding:
                macindecode_ac4_bitstream::oamd::RampDurationEncoding::Fixed1536,
            ramp_duration: 1_536,
        };
        SceneMetadataUpdate::new(
            SceneElementId::new(7),
            offset_samples,
            u32::from(block.ramp_duration),
            MetadataFields::POSITION,
            state,
            RawOamdUpdate::new(
                OamdMetadataBlock::default(),
                RawOamdTiming::new(SampleOffsetSource::Implicit, 0, 1, block, false),
                control_source_access_unit_index,
            ),
            stream_order,
        )
    }

    #[test]
    fn maps_verified_scene_semantics_and_keeps_quantized_state() {
        let effective = complete_state(
            ObjectGainState::Quantized(42),
            ObjectPriorityState::Quantized(31),
            OtherPropertiesUpdate {
                grouped_defaults: false,
                group_other_mask: Some(0b0011),
                width: Some(WidthUpdate::Cartesian { x: 31, y: 15, z: 0 }),
                screen_factor_code: Some(7),
                depth_factor: Some(3),
                ..OtherPropertiesUpdate::default()
            },
        );
        let additional = AdditionalObjectMetadata {
            trim_disabled: true,
            extended_position: Some(ExtendedPrecisionPosition {
                presence: 0b111,
                x: Some(0),
                y: Some(0),
                z: Some(0),
            }),
            headphone: Some(ObjectHeadphone {
                render_mode: 3,
                head_tracking_disabled: true,
            }),
        };

        let state = map_oamd_object_state(effective, additional);
        let position = state.position().expect("完整 render 状态应产生位置");
        assert!((position.x() - (1.0_f32 / 155.0)).abs() < f32::EPSILON);
        assert!((position.y() + (1.0_f32 / 155.0)).abs() < f32::EPSILON);
        assert!((position.z() - (-3.0_f32 / 15.0 - 1.0_f32 / 75.0)).abs() < f32::EPSILON);
        assert!((state.linear_gain().expect("应有增益") - 0.039_810_72).abs() < 1.0e-8);
        assert_eq!(state.importance(), Some(1.0));
        assert_eq!(
            state.extent(),
            Some(ObjectExtent::Cartesian {
                x: 1.0,
                y: 15.0 / 31.0,
                z: 0.0,
            })
        );
        let zone = state.zone().expect("完整 render 状态应产生 zone");
        assert!(zone.snap());
        assert!(!zone.elevation());
        assert_eq!(zone.mask(), 4);
        assert_eq!(state.screen_factor(), Some(1.0));
        assert_eq!(state.depth_factor(), Some(2.0));
        assert!(state.trim_disabled());
        let headphone = state.headphone().expect("应映射耳机状态");
        assert_eq!(headphone.mode(), HeadphoneMode::Mid);
        assert!(headphone.head_tracking_disabled());
        assert!(state.semantic_complete());
        assert_eq!(state.raw().effective(), effective);
        assert_eq!(state.raw().additional(), additional);
    }

    #[test]
    fn leaves_distance_and_divergence_raw_without_inventing_semantics() {
        let other = OtherPropertiesUpdate {
            grouped_defaults: false,
            group_other_mask: Some(0b1100),
            object_at_infinity: Some(false),
            distance_factor_code: Some(7),
            divergence_mode: Some(2),
            divergence_code: Some(12),
            ..OtherPropertiesUpdate::default()
        };
        let effective = complete_state(
            ObjectGainState::Default,
            ObjectPriorityState::Default,
            other,
        );

        let state = map_oamd_object_state(effective, AdditionalObjectMetadata::default());

        assert!(!state.semantic_complete());
        assert_eq!(
            state
                .raw()
                .effective()
                .render
                .map(|render| render.other_properties),
            Some(other)
        );
    }

    #[test]
    fn gain_mapping_keeps_positive_overrange_and_silence() {
        let positive = map_gain(ObjectGainState::Quantized(0));
        assert!(positive > 1.0, "正增益不能在 Scene 边界削波");
        assert_eq!(map_gain(ObjectGainState::Default), 1.0);
        assert_eq!(map_gain(ObjectGainState::NegativeInfinity), 0.0);
    }

    #[test]
    fn changed_mask_compares_effective_fields_instead_of_raw_presence() {
        let previous_effective = complete_state(
            ObjectGainState::Default,
            ObjectPriorityState::Default,
            OtherPropertiesUpdate::default(),
        );
        let mut current_effective = previous_effective;
        current_effective
            .basic
            .as_mut()
            .expect("测试状态应有 basic")
            .gain = ObjectGainState::Quantized(15);
        let current_other = &mut current_effective
            .render
            .as_mut()
            .expect("测试状态应有 render")
            .other_properties;
        current_other.object_at_infinity = Some(false);
        current_other.distance_factor_code = Some(7);

        let previous = RawOamdState::new(previous_effective, AdditionalObjectMetadata::default());
        let current = RawOamdState::new(
            current_effective,
            AdditionalObjectMetadata {
                trim_disabled: true,
                extended_position: Some(ExtendedPrecisionPosition {
                    presence: 0b100,
                    x: Some(1),
                    y: None,
                    z: None,
                }),
                headphone: Some(ObjectHeadphone {
                    render_mode: 1,
                    head_tracking_disabled: false,
                }),
            },
        );

        let changed = changed_oamd_fields(previous, current);
        let expected = MetadataFields::GAIN
            .union(MetadataFields::POSITION)
            .union(MetadataFields::DISTANCE)
            .union(MetadataFields::TRIM)
            .union(MetadataFields::HEADPHONE);
        assert_eq!(changed, expected);
        assert!(!changed.contains(MetadataFields::ACTIVE));
        assert!(!changed.contains(MetadataFields::IMPORTANCE));
        assert!(!changed.contains(MetadataFields::DIVERGENCE));
    }

    #[test]
    fn equivalent_encodings_do_not_set_changed_fields() {
        let previous_effective = ObjectMetadataState {
            active: true,
            basic: Some(ObjectBasicState {
                gain: ObjectGainState::Default,
                priority: ObjectPriorityState::Default,
            }),
            render: Some(ObjectRenderState {
                position: QuantizedPosition {
                    x: 31,
                    y: 31,
                    z: 0,
                    coding: PositionCoding::AbsolutePositive,
                },
                zone: ZoneUpdate {
                    grouped_defaults: true,
                    ..ZoneUpdate::default()
                },
                other_properties: OtherPropertiesUpdate::default(),
            }),
        };
        let current_effective = ObjectMetadataState {
            basic: Some(ObjectBasicState {
                gain: ObjectGainState::Default,
                priority: ObjectPriorityState::Quantized(31),
            }),
            render: Some(ObjectRenderState {
                position: QuantizedPosition {
                    coding: PositionCoding::Differential,
                    ..previous_effective
                        .render
                        .expect("测试前态应有 render")
                        .position
                },
                zone: ZoneUpdate {
                    grouped_defaults: false,
                    group_zone_flag: Some(0),
                    zone_mask: None,
                },
                other_properties: OtherPropertiesUpdate::default(),
            }),
            ..previous_effective
        };
        let previous = RawOamdState::new(previous_effective, AdditionalObjectMetadata::default());
        let current = RawOamdState::new(
            current_effective,
            AdditionalObjectMetadata {
                extended_position: Some(ExtendedPrecisionPosition::default()),
                ..AdditionalObjectMetadata::default()
            },
        );

        assert_eq!(
            changed_oamd_fields(previous, current),
            MetadataFields::empty()
        );
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn metadata_queue_carries_offsets_and_ramps_across_multiple_frames() {
        let mut current = Vec::with_capacity(MAX_SCENE_METADATA_UPDATES);
        let mut pending = Vec::with_capacity(MAX_SCENE_METADATA_UPDATES);
        let update = queued_update(800, 3);

        partition_metadata_update(&mut current, &mut pending, update, 384)
            .expect("第一个短帧应把更新留到后续");
        assert!(current.is_empty());
        assert_eq!(
            pending.first().map(SceneMetadataUpdate::offset_samples),
            Some(416)
        );

        let first_pending = pending.pop().expect("应有第一段跨帧更新");
        partition_metadata_update(&mut current, &mut pending, first_pending, 384)
            .expect("第二个短帧仍应保留更新");
        assert!(current.is_empty());
        assert_eq!(
            pending.first().map(SceneMetadataUpdate::offset_samples),
            Some(32)
        );

        let second_pending = pending.pop().expect("应有第二段跨帧更新");
        partition_metadata_update(&mut current, &mut pending, second_pending, 384)
            .expect("第三个短帧应让更新到期");
        let due = current.first().expect("更新应落在第三个帧内");
        assert_eq!(due.offset_samples(), 32);
        assert_eq!(due.ramp_duration_samples(), 1_536);
        assert_eq!(due.raw(), update.raw());
        assert_eq!(due.control_source_access_unit_index(), 41);
        assert_eq!(due.stream_order(), 3);
        assert!(pending.is_empty());
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn frame_start_state_only_applies_updates_effective_at_offset_zero() {
        let mut element_ids = [None; MAX_OAMD_OBJECTS];
        element_ids[0] = Some(SceneElementId::new(7));
        let inherited = [None; MAX_OAMD_OBJECTS];

        let (positive_start, positive_end) =
            metadata_frame_states(inherited, &element_ids, &[queued_update(32, 0)])
                .expect("正偏移更新应形成有效帧状态");
        assert_eq!(positive_start[0], None, "尚未到期的状态不能提前发布");
        assert_eq!(positive_end[0], Some(queued_update(32, 0).state()));

        let (zero_start, zero_end) =
            metadata_frame_states(inherited, &element_ids, &[queued_update(0, 0)])
                .expect("帧起点更新应形成有效帧状态");
        assert_eq!(zero_start[0], Some(queued_update(0, 0).state()));
        assert_eq!(zero_end, zero_start);
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn metadata_schedule_rejects_a_later_au_overtaking_a_queued_update() {
        let mut samples = [None; MAX_OAMD_OBJECTS];
        register_metadata_update_sample(&mut samples, 0, 0, 2_000)
            .expect("首份跨帧更新应登记绝对时间");

        assert_eq!(
            register_metadata_update_sample(&mut samples, 0, 1_536, 0),
            Err(MetadataUpdateTimeError::Reordered {
                previous: 2_000,
                current: 1_536,
            })
        );
        assert_eq!(samples[0], Some(2_000), "失败更新不得污染单调时间状态");
        register_metadata_update_sample(&mut samples, 0, 1_536, 464)
            .expect("相同绝对样本按原码流顺序稳定排列");
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn metadata_sort_uses_offset_then_original_stream_order() {
        assert_eq!(
            queued_update(32, 1),
            queued_update(32, 99),
            "内部排序键不得改变公共更新值的相等语义"
        );
        assert_ne!(
            queued_update_from(32, 1, 41),
            queued_update_from(32, 1, 42),
            "control source AU 属于公共更新来源，不能从相等语义中丢失"
        );
        let mut updates = Vec::with_capacity(MAX_SCENE_METADATA_UPDATES);
        for update in [
            queued_update(32, 3),
            queued_update(16, 4),
            queued_update(32, 1),
            queued_update(32, 2),
        ] {
            push_bounded_metadata_update(&mut updates, update).expect("测试更新应在容量内");
        }

        updates.sort_unstable_by_key(|update| (update.offset_samples(), update.stream_order()));

        assert_eq!(
            updates
                .iter()
                .map(|update| (update.offset_samples(), update.stream_order()))
                .collect::<Vec<_>>(),
            vec![(16, 4), (32, 1), (32, 2), (32, 3)]
        );
    }

    #[cfg(feature = "audio-decode")]
    #[test]
    fn discontinuity_clears_metadata_history_without_releasing_queue_capacity() {
        let mut assembler = SceneAssembler::new();
        reserve_total(
            &mut assembler.pending_metadata_updates,
            MAX_SCENE_METADATA_UPDATES,
        );
        reserve_total(
            &mut assembler.next_pending_metadata_updates,
            MAX_SCENE_METADATA_UPDATES,
        );
        let pending_address = assembler.pending_metadata_updates.as_ptr();
        let pending_capacity = assembler.pending_metadata_updates.capacity();
        let state = queued_update(64, 9).state();
        let state_slot = assembler
            .oamd_states
            .get_mut(0)
            .expect("OAMD 固定容量应包含对象 0");
        *state_slot = Some(state);
        assembler.last_metadata_update_samples[0] = Some(64);
        assembler.next_metadata_order = 10;
        push_bounded_metadata_update(
            &mut assembler.pending_metadata_updates,
            queued_update(64, 9),
        )
        .expect("测试更新应进入预分配队列");

        assembler.mark_discontinuity();

        assert!(assembler.oamd_states.iter().all(Option::is_none));
        assert!(
            assembler
                .last_metadata_update_samples
                .iter()
                .all(Option::is_none)
        );
        assert_eq!(assembler.next_metadata_order, 0);
        assert!(assembler.pending_metadata_updates.is_empty());
        assert!(assembler.next_pending_metadata_updates.is_empty());
        assert_eq!(assembler.pending_metadata_updates.as_ptr(), pending_address);
        assert_eq!(
            assembler.pending_metadata_updates.capacity(),
            pending_capacity
        );
    }

    #[test]
    fn reserved_zone_mask_is_not_advertised_as_complete_semantics() {
        let mut effective = complete_state(
            ObjectGainState::Default,
            ObjectPriorityState::Default,
            OtherPropertiesUpdate::default(),
        );
        effective.render.as_mut().expect("测试状态应有 render").zone = ZoneUpdate {
            grouped_defaults: false,
            group_zone_flag: Some(0b100),
            zone_mask: Some(7),
        };

        let state = map_oamd_object_state(effective, AdditionalObjectMetadata::default());

        assert_eq!(state.zone().map(|zone| zone.mask()), Some(7));
        assert!(!state.semantic_complete());
        assert_eq!(
            state
                .raw()
                .effective()
                .render
                .map(|render| render.zone.zone_mask),
            Some(Some(7))
        );
    }
}
