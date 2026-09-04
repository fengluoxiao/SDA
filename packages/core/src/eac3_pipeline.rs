//! E-AC-3 (incl. JOC/Atmos) streaming pipeline — a WASM-facing port of
//! harletty-bridge's `bridge/src/eac3_pipeline.rs`, minus the Omniphony ABI.
//!
//! Strategy per access unit (same as the bridge): try the object decoder
//! first (JOC + OAMD), fall back to the plain core PCM decoder.

use std::collections::VecDeque;

use eac3::{
    inspect_access_unit, merge_core_with_dependent, BedChannel, CorePcmFrame, FrameType,
    OamdElementKind, OamdPayload, ObjectPcmPushResult, JOC_QMF_LATENCY_SAMPLES,
};

use crate::{FrameData, ObjectChannelDecl, ObjectEvent, Pipeline, ProgramLoudnessMetadata};

pub struct Eac3Pipeline {
    extractor: eac3::Extractor,
    object_decoder: eac3::ObjectPcmDecoder,
    pcm_decoder: eac3::PcmDecoder,
    dependent_pcm_decoder: eac3::PcmDecoder,
    pending_independent_core: Option<(CorePcmFrame, ProgramLoudnessMetadata)>,
    total_samples: u64,
    declared: Option<Vec<ObjectChannelDecl>>,
}

impl Eac3Pipeline {
    pub fn new() -> Self {
        Self {
            extractor: eac3::Extractor::default(),
            object_decoder: eac3::ObjectPcmDecoder::new(),
            pcm_decoder: eac3::PcmDecoder::new(),
            dependent_pcm_decoder: eac3::PcmDecoder::new(),
            pending_independent_core: None,
            total_samples: 0,
            declared: None,
        }
    }

    fn raw_bed_labels(core: &CorePcmFrame) -> Vec<String> {
        let mut labels: Vec<String> = core
            .fullband_channel_order
            .iter()
            .map(|label| format!("{label:?}"))
            .collect();
        if core.lfe_channel.is_some() {
            labels.push("LFE".to_string());
        }
        labels
    }

    fn loudness(info: &eac3::AccessUnitInfo) -> ProgramLoudnessMetadata {
        ProgramLoudnessMetadata::dolby("eac3-dialnorm", info.dialogue_normalization[0])
    }

    fn emit_bed_frame(
        &mut self,
        core: CorePcmFrame,
        loudness: ProgramLoudnessMetadata,
        out: &mut VecDeque<FrameData>,
    ) {
        let sample_pos = self.total_samples;
        self.total_samples += core.samples_per_channel() as u64;
        out.push_back(bed_frame(
            "eac3",
            core,
            Vec::new(),
            Vec::new(),
            Some(loudness),
            sample_pos,
            &[],
        ));
    }

    fn process_frame(
        &mut self,
        frame: &[u8],
        out: &mut VecDeque<FrameData>,
        errors: &mut Vec<String>,
    ) {
        let info = match inspect_access_unit(frame) {
            Ok(info) => info,
            Err(error) => {
                errors.push(format!("E-AC-3 frame rejected: {error}"));
                return;
            }
        };
        let is_dependent = info.frame_type == FrameType::Dependent;

        // An independent frame without a dependent partner is a complete core.
        // Flush it before decoding the next independent presentation.
        if !is_dependent {
            if let Some((core, loudness)) = self.pending_independent_core.take() {
                self.emit_bed_frame(core, loudness, out);
            }
        }

        if is_dependent {
            let Some((core, loudness)) = self.pending_independent_core.take() else {
                errors.push(
                    "E-AC-3 dependent substream arrived without an independent core".to_string(),
                );
                return;
            };
            if info.joc_payload_count() > 0 {
                match self
                    .object_decoder
                    .push_access_unit_with_core(frame, core.clone())
                {
                    Ok(Some(result)) => {
                        out.push_back(self.build_object_frame(result, Some(loudness)))
                    }
                    Ok(None) => {
                        errors.push("E-AC-3 JOC dependent substream yielded no object PCM; using independent core".to_string());
                        self.emit_bed_frame(core, loudness, out);
                    }
                    Err(error) => {
                        errors.push(format!(
                            "E-AC-3 dependent JOC frame rejected, using independent core: {error}"
                        ));
                        self.emit_bed_frame(core, loudness, out);
                    }
                }
            } else {
                let merged =
                    merge_core_with_dependent(&mut self.dependent_pcm_decoder, &core, frame)
                        .unwrap_or(core);
                self.emit_bed_frame(merged, loudness, out);
            }
            return;
        }

        match self.object_decoder.push_access_unit(frame) {
            Ok(Some(result)) => {
                out.push_back(self.build_object_frame(result, None));
            }
            Ok(None) => match self.pcm_decoder.push_access_unit(frame) {
                Ok(result) => {
                    let loudness = Self::loudness(&result.info);
                    self.pending_independent_core = Some((result.pcm, loudness));
                }
                Err(error) => errors.push(format!("E-AC-3 frame rejected: {error}")),
            },
            Err(error) => {
                errors.push(format!(
                    "E-AC-3 object frame rejected, using core PCM: {error}"
                ));
                match self.pcm_decoder.push_access_unit(frame) {
                    Ok(result) => {
                        let loudness = Self::loudness(&result.info);
                        self.pending_independent_core = Some((result.pcm, loudness));
                    }
                    Err(core_error) => {
                        errors.push(format!("E-AC-3 core fallback rejected: {core_error}"));
                        self.total_samples += u64::from(info.num_blocks) * 256;
                    }
                }
            }
        }
    }

    fn build_object_frame(
        &mut self,
        result: ObjectPcmPushResult,
        loudness_override: Option<ProgramLoudnessMetadata>,
    ) -> FrameData {
        let loudness = loudness_override.unwrap_or_else(|| Self::loudness(&result.info));
        let pcm = result.pcm;
        let sample_pos = self.total_samples;
        self.total_samples += pcm.samples_per_channel() as u64;

        let core = pcm.core;
        let raw_bed_labels = Self::raw_bed_labels(&core);
        let joc_slot_count = pcm.object_channels.len();
        let Some(slot_layout) = consistent_joc_slot_layout(
            &pcm.oamd_payloads,
            joc_slot_count,
            core.lfe_channel.is_some(),
        ) else {
            // Sparse JOC updates are legal. When the current frame does not carry
            // a complete topology map, keep the last declared object layout instead
            // of collapsing the whole programme back to bed channels.
            let declarations = self.declared.clone().unwrap_or_default();
            let object_channels = self.sparse_declare(declarations);
            let mut channels =
                Vec::with_capacity(joc_slot_count + usize::from(core.lfe_channel.is_some()));
            let mut labels = Vec::with_capacity(channels.capacity());
            if let Some(lfe) = &core.lfe_channel {
                channels.push(lfe.clone());
                labels.push("LFE".to_string());
            }
            for (index, pcm_channel) in pcm.object_channels.iter().enumerate() {
                channels.push(pcm_channel.clone());
                labels.push(format!("Obj_{}", 10 + index as u32));
            }
            return FrameData {
                codec: "eac3",
                sample_rate: core.sample_rate,
                sample_pos,
                channels,
                labels,
                raw_bed_labels,
                ramp_duration: 0,
                events: Vec::new(),
                object_channels,
                program_loudness: Some(loudness),
            };
        };

        // JOC reconstructs every OAMD essence except the ordinary LFE carried by
        // CorePcmFrame::lfe_channel. Non-LFE bed members (including LFE2) retain
        // fixed labels; only the dynamic suffix receives Obj_* routes/events.
        let mut channels =
            Vec::with_capacity(joc_slot_count + usize::from(core.lfe_channel.is_some()));
        let mut labels = Vec::with_capacity(channels.capacity());
        if let Some(lfe) = &core.lfe_channel {
            channels.push(lfe.clone());
            labels.push("LFE".to_string());
        }
        let joc_channel_base = channels.len();
        for (slot, pcm_channel) in slot_layout.slots.iter().zip(&pcm.object_channels) {
            channels.push(pcm_channel.clone());
            labels.push(match slot {
                JocSlot::Bed(label) => format!("{label:?}"),
                JocSlot::Dynamic { id } => format!("Obj_{id}"),
            });
        }

        let dynamic_count = slot_layout
            .slots
            .iter()
            .filter(|slot| matches!(slot, JocSlot::Dynamic { .. }))
            .count();
        let mut events = Vec::new();
        for (oamd, sample_offset) in &pcm.oamd_payloads {
            let event_sample_pos =
                sample_pos + sample_offset.unwrap_or(0) as u64 + JOC_QMF_LATENCY_SAMPLES as u64;
            events.extend(extract_events(oamd, event_sample_pos, dynamic_count));
        }
        events.sort_by_key(|event| event.sample_pos);

        let declarations = dynamic_object_declarations(&slot_layout.slots, joc_channel_base);
        let object_channels = self.sparse_declare(declarations);
        FrameData {
            codec: "eac3",
            sample_rate: core.sample_rate,
            sample_pos,
            channels,
            labels,
            raw_bed_labels,
            ramp_duration: events.first().map_or(0, |event| event.ramp_duration),
            events,
            object_channels,
            program_loudness: Some(loudness),
        }
    }

    /// Emit the object↔channel declaration only when it changed (bridge parity).
    fn sparse_declare(&mut self, current: Vec<ObjectChannelDecl>) -> Vec<ObjectChannelDecl> {
        if self.declared.as_ref() == Some(&current) {
            Vec::new()
        } else {
            self.declared = Some(current.clone());
            current
        }
    }
}

impl Pipeline for Eac3Pipeline {
    fn codec_name(&self) -> &'static str {
        "eac3"
    }

    fn push(&mut self, data: &[u8], out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        self.extractor.push_bytes(data);
        loop {
            match self.extractor.next_frame() {
                Ok(Some(frame)) => self.process_frame(frame.as_bytes(), out, errors),
                Ok(None) => break,
                Err(e) => {
                    errors.push(format!("E-AC-3 extract error: {e:?}"));
                    break;
                }
            }
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JocPresentationLayout {
    has_lfe: bool,
    slots: Vec<JocSlot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JocSlot {
    Bed(BedChannel),
    Dynamic { id: u32 },
}

/// Map each JOC output slot to the OAMD essence it reconstructs. The ordinary
/// LFE bed member is the sole exception: it bypasses JOC and comes from the
/// core's dedicated `lfe_channel`. LFE2 is independent essence and remains a
/// JOC slot. ISF needs a separate speaker/object projection that this renderer
/// does not expose yet, so reject it rather than guessing a moving-object map.
fn joc_slot_layout(oamd: &OamdPayload, joc_slot_count: usize) -> Option<JocPresentationLayout> {
    if oamd.isf_in_use
        || oamd.bed_or_isf_objects > oamd.object_count
        || oamd.dynamic_objects != oamd.object_count - oamd.bed_or_isf_objects
    {
        return None;
    }
    let bed_labels: Vec<BedChannel> = oamd.bed_assignment.iter().flatten().copied().collect();
    if bed_labels.len() != oamd.beds || oamd.beds != oamd.bed_or_isf_objects {
        return None;
    }

    let lfe_count = bed_labels
        .iter()
        .filter(|&&label| label == BedChannel::LowFrequencyEffects)
        .count();
    if lfe_count > 1 {
        return None;
    }
    let has_lfe = lfe_count == 1;
    let mut slots = Vec::with_capacity(joc_slot_count);
    for label in bed_labels {
        if label != BedChannel::LowFrequencyEffects {
            slots.push(JocSlot::Bed(label));
        }
    }
    for dynamic_index in 0..oamd.dynamic_objects {
        slots.push(JocSlot::Dynamic {
            id: (10 + dynamic_index) as u32,
        });
    }
    (slots.len() == joc_slot_count).then_some(JocPresentationLayout { has_lfe, slots })
}

fn consistent_joc_slot_layout(
    payloads: &[(OamdPayload, Option<u16>)],
    joc_slot_count: usize,
    core_has_lfe: bool,
) -> Option<JocPresentationLayout> {
    let (first, _) = payloads.first()?;
    let layout = joc_slot_layout(first, joc_slot_count)?;
    (layout.has_lfe == core_has_lfe
        && payloads
            .iter()
            .skip(1)
            .all(|(oamd, _)| joc_slot_layout(oamd, joc_slot_count).as_ref() == Some(&layout)))
    .then_some(layout)
}

fn dynamic_object_count(oamd: &OamdPayload, expected_count: usize) -> usize {
    (oamd.bed_or_isf_objects <= oamd.object_count
        && oamd.dynamic_objects == oamd.object_count - oamd.bed_or_isf_objects
        && oamd.dynamic_objects == expected_count)
        .then_some(expected_count)
        .unwrap_or(0)
}

fn dynamic_object_declarations(
    slot_layout: &[JocSlot],
    joc_channel_base: usize,
) -> Vec<ObjectChannelDecl> {
    slot_layout
        .iter()
        .enumerate()
        .filter_map(|(slot_index, slot)| match slot {
            JocSlot::Bed(_) => None,
            JocSlot::Dynamic { id } => Some(ObjectChannelDecl {
                id: *id,
                channel: (joc_channel_base + slot_index) as u32,
            }),
        })
        .collect()
}

fn object_gain_db(gain: Option<f32>, inactive: bool) -> i8 {
    if inactive {
        return i8::MIN;
    }
    match gain {
        Some(value) if value.is_finite() => value.round().clamp(-127.0, 127.0) as i8,
        Some(_) => i8::MIN,
        None => 0,
    }
}

/// Port of `bridge/src/metadata.rs::extract_eac3_events`.
fn extract_events(
    oamd: &OamdPayload,
    base_sample_pos: u64,
    object_channel_count: usize,
) -> Vec<ObjectEvent> {
    let dynamic_objects = dynamic_object_count(oamd, object_channel_count);
    let mut events = Vec::with_capacity(dynamic_objects);

    for element in &oamd.elements {
        let OamdElementKind::Object(ref obj_element) = element.kind else {
            continue;
        };

        for (obj_idx, blocks) in obj_element.object_blocks.iter().enumerate() {
            if obj_idx < oamd.bed_or_isf_objects {
                continue;
            }
            let dynamic_idx = obj_idx - oamd.bed_or_isf_objects;
            if dynamic_idx >= object_channel_count {
                continue;
            }
            let id = (10 + dynamic_idx) as u32;

            for (block_index, block) in blocks.iter().enumerate() {
                let update = obj_element.block_updates.get(block_index);
                let sample_offset = update.map_or(0, |value| value.offset as u64);
                let ramp_duration = update.map_or(0, |value| value.ramp_duration as u32);
                let has_pos = block.valid_position;
                let pos: [f64; 3] = if has_pos {
                    match block.position.as_ref() {
                        Some(p) if !block.differential_position => [
                            ((p.x as f64).clamp(0.0, 1.0) - 0.5) * 2.0,
                            (0.5 - (p.y as f64).clamp(0.0, 1.0)) * 2.0,
                            (p.z as f64).clamp(-1.0, 1.0),
                        ],
                        Some(p) => [
                            (p.x as f64).clamp(-1.0, 1.0),
                            (-(p.y as f64)).clamp(-1.0, 1.0),
                            (p.z as f64).clamp(-1.0, 1.0),
                        ],
                        None => [0.0; 3],
                    }
                } else {
                    [0.0; 3]
                };
                let size = block
                    .size
                    .map(|s| [s[0] as f64, s[1] as f64, s[2] as f64])
                    .unwrap_or([0.0; 3]);
                let (distance_m, distance_infinite) = match block.distance {
                    Some(distance) if distance.is_infinite() => (None, true),
                    Some(distance) => (Some(distance as f64), false),
                    None => (None, false),
                };
                let anchor = match block.anchor {
                    eac3::ObjectAnchor::Room => "room",
                    eac3::ObjectAnchor::Screen => "screen",
                    eac3::ObjectAnchor::Speaker => "speaker",
                }
                .to_string();

                events.push(ObjectEvent {
                    id,
                    sample_pos: base_sample_pos + sample_offset,
                    has_pos,
                    pos,
                    gain_db: object_gain_db(block.gain, block.inactive),
                    size,
                    anchor,
                    distance_m,
                    distance_infinite,
                    screen_factor: block.screen_factor.map(f64::from),
                    depth_factor: block.depth_factor.map(f64::from),
                    ramp_duration,
                });
            }
        }
    }
    events
}

/// Shared bed-frame construction (also used by the plain PCM fallback).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_gain_preserves_db_and_silence() {
        assert_eq!(object_gain_db(Some(-3.0), false), -3);
        assert_eq!(object_gain_db(Some(f32::NEG_INFINITY), false), i8::MIN);
        assert_eq!(object_gain_db(None, false), 0);
        assert_eq!(object_gain_db(Some(0.0), true), i8::MIN);
    }

    fn oamd(bed: Vec<BedChannel>, dynamic_objects: usize) -> OamdPayload {
        OamdPayload {
            version: 0,
            object_count: bed.len() + dynamic_objects,
            alternate_object_present: false,
            element_count: 0,
            beds: bed.len(),
            bed_instances: usize::from(!bed.is_empty()),
            bed_or_isf_objects: bed.len(),
            dynamic_objects,
            isf_in_use: false,
            isf_index: None,
            bed_assignment: (!bed.is_empty()).then_some(bed).into_iter().collect(),
            elements: Vec::new(),
        }
    }

    #[test]
    fn lfe_only_program_maps_every_joc_slot_to_dynamic_suffix() {
        let payload = oamd(vec![BedChannel::LowFrequencyEffects], 3);
        let slots = joc_slot_layout(&payload, 3).expect("slot layout");
        assert!(slots.has_lfe);
        assert_eq!(
            slots.slots,
            vec![
                JocSlot::Dynamic { id: 10 },
                JocSlot::Dynamic { id: 11 },
                JocSlot::Dynamic { id: 12 },
            ]
        );
        assert_eq!(
            dynamic_object_declarations(&slots.slots, 1),
            vec![
                ObjectChannelDecl { id: 10, channel: 1 },
                ObjectChannelDecl { id: 11, channel: 2 },
                ObjectChannelDecl { id: 12, channel: 3 },
            ]
        );
    }

    #[test]
    fn non_lfe_bed_occupies_leading_joc_slot_before_dynamic_objects() {
        let payload = oamd(
            vec![
                BedChannel::FrontLeft,
                BedChannel::LowFrequencyEffects,
                BedChannel::Center,
            ],
            2,
        );
        let slots = joc_slot_layout(&payload, 4).expect("slot layout");
        assert!(slots.has_lfe);
        assert_eq!(
            slots.slots,
            vec![
                JocSlot::Bed(BedChannel::FrontLeft),
                JocSlot::Bed(BedChannel::Center),
                JocSlot::Dynamic { id: 10 },
                JocSlot::Dynamic { id: 11 },
            ]
        );
        assert_eq!(
            dynamic_object_declarations(&slots.slots, 1),
            vec![
                ObjectChannelDecl { id: 10, channel: 3 },
                ObjectChannelDecl { id: 11, channel: 4 },
            ]
        );
    }

    #[test]
    fn lfe2_is_an_independent_joc_bed_slot() {
        let payload = oamd(
            vec![
                BedChannel::LowFrequencyEffects,
                BedChannel::LowFrequencyEffects2,
            ],
            1,
        );
        assert_eq!(
            joc_slot_layout(&payload, 2),
            Some(JocPresentationLayout {
                has_lfe: true,
                slots: vec![
                    JocSlot::Bed(BedChannel::LowFrequencyEffects2),
                    JocSlot::Dynamic { id: 10 },
                ],
            })
        );
    }

    #[test]
    fn multi_payload_layout_must_match_and_agree_with_core_lfe() {
        let lfe_dynamic = oamd(vec![BedChannel::LowFrequencyEffects], 2);
        let center_dynamic = oamd(vec![BedChannel::Center], 1);
        let payloads = vec![(lfe_dynamic.clone(), None), (lfe_dynamic, Some(64))];
        assert!(consistent_joc_slot_layout(&payloads, 2, true).is_some());
        assert!(consistent_joc_slot_layout(&payloads, 2, false).is_none());

        let conflicting = vec![
            (oamd(vec![BedChannel::LowFrequencyEffects], 2), None),
            (center_dynamic, Some(64)),
        ];
        assert!(consistent_joc_slot_layout(&conflicting, 2, true).is_none());
    }

    #[test]
    fn slot_layout_rejects_count_mismatch_and_isf() {
        let payload = oamd(vec![BedChannel::FrontLeft], 2);
        assert_eq!(joc_slot_layout(&payload, 2), None);

        let mut isf = oamd(Vec::new(), 2);
        isf.isf_in_use = true;
        isf.isf_index = Some(0);
        isf.bed_or_isf_objects = 2;
        isf.object_count = 4;
        assert_eq!(joc_slot_layout(&isf, 4), None);
    }
}

fn bed_frame(
    codec: &'static str,
    core: CorePcmFrame,
    events: Vec<ObjectEvent>,
    object_channels: Vec<ObjectChannelDecl>,
    program_loudness: Option<ProgramLoudnessMetadata>,
    sample_pos: u64,
    extra_labels: &[String],
) -> FrameData {
    let mut channels = core.fullband_channels;
    let raw_bed_labels: Vec<String> = core
        .fullband_channel_order
        .iter()
        .map(|b| format!("{b:?}"))
        .chain(core.lfe_channel.is_some().then(|| "LFE".to_string()))
        .collect();
    let mut labels: Vec<String> = core
        .fullband_channel_order
        .iter()
        .map(|b| format!("{b:?}"))
        .collect();
    labels.extend(extra_labels.iter().cloned());
    if let Some(lfe) = core.lfe_channel {
        channels.push(lfe);
        labels.push("LFE".to_string());
    }
    FrameData {
        codec,
        sample_rate: core.sample_rate,
        sample_pos,
        channels,
        labels,
        raw_bed_labels,
        ramp_duration: 0,
        events,
        object_channels,
        program_loudness,
    }
}
