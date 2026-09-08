//! AC-4 Full A-JOC scene decoding. Transport framing stays separate from DSP.
use crate::{FrameData, ObjectChannelDecl, ObjectEvent, Pipeline};
use macindecode_ac4_bitstream::{SyncFrameError, SyncFrameIter};
use macindecode_ac4_scene::{
    Ac4DecoderConfig, Ac4DecoderSession, Ac4SceneFrame, AccessUnit, AccessUnitContext,
    DecodeStatus, ObjectExtent, PresentationSelection, SceneObjectState, SpeakerLabel,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Default)]
struct GainRamp {
    value: f32,
    target: f32,
    remaining: u32,
}
impl GainRamp {
    fn set(&mut self, target: f32, duration: u32) {
        self.target = target;
        self.remaining = duration;
        if duration == 0 {
            self.value = target;
        }
    }
    fn next(&mut self) -> f32 {
        let value = self.value;
        if self.remaining > 0 {
            self.value += (self.target - self.value) / self.remaining as f32;
            self.remaining -= 1;
        }
        value
    }
}

pub struct Ac4Pipeline {
    session: Ac4DecoderSession,
    pending: Vec<u8>,
    au_index: u64,
    ids: BTreeMap<u64, u32>,
    initialized: BTreeSet<u64>,
    lfe_gains: BTreeMap<u64, GainRamp>,
    generation: Option<u32>,
    expected_sample: Option<u64>,
    failed: bool,
}

impl Ac4Pipeline {
    pub fn new() -> Self {
        Self {
            session: Ac4DecoderSession::new(Ac4DecoderConfig::new(
                PresentationSelection::AutoUnique,
            ).with_opaque_presentation_tail(true)),
            pending: Vec::new(),
            au_index: 0,
            ids: BTreeMap::new(),
            initialized: BTreeSet::new(),
            lfe_gains: BTreeMap::new(),
            generation: None,
            expected_sample: None,
            failed: false,
        }
    }

    fn decode(&mut self, raw: &[u8], out: &mut VecDeque<FrameData>) -> Result<(), String> {
        let decoded = self
            .session
            .decode_access_unit(AccessUnit::new(raw, AccessUnitContext::new(self.au_index)))
            .map_err(|error| format!("AC-4 Full A-JOC decode failed: {error}"))?;
        self.au_index += 1;
        match decoded.status() {
            DecodeStatus::WaitingForRandomAccess { .. } => return Ok(()),
            DecodeStatus::Decoded => {}
            _ => return Err("AC-4 decoder returned an unsupported status".into()),
        }
        for scene in decoded.frames() {
            let generation = scene.timeline().configuration_generation();
            if self.generation != Some(generation) {
                self.initialized.clear();
                self.lfe_gains.clear();
                self.generation = Some(generation);
            }
            let frame = scene_frame(
                &scene,
                &mut self.ids,
                &mut self.initialized,
                &mut self.lfe_gains,
            )?;
            if self
                .expected_sample
                .is_some_and(|sample| sample != frame.sample_pos)
            {
                return Err("AC-4 scene timeline is discontinuous".into());
            }
            self.expected_sample = Some(frame.sample_pos + frame.channels[0].len() as u64);
            out.push_back(frame);
        }
        Ok(())
    }
}

impl Pipeline for Ac4Pipeline {
    fn codec_name(&self) -> &'static str {
        "ac4"
    }
    fn reset(&mut self) {
        *self = Self::new();
    }
    fn flush(&mut self, _out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        if !self.failed && !self.pending.is_empty() {
            errors.push("AC-4: truncated sync frame at end of input".into());
            self.failed = true;
        }
    }
    fn push(&mut self, data: &[u8], out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        if self.failed {
            return;
        }
        self.pending.extend_from_slice(data);
        let mut consumed = 0;
        while consumed < self.pending.len() {
            let source = &self.pending[consumed..];
            let Some(result) = SyncFrameIter::new(source).next() else {
                break;
            };
            let frame = match result {
                Ok(frame) => frame,
                Err(SyncFrameError::Truncated { .. }) => break,
                Err(error) => {
                    errors.push(format!("AC-4 framing failed: {error}"));
                    self.failed = true;
                    break;
                }
            };
            if frame.verify_crc(source) == Some(false) {
                errors.push("AC-4 sync frame CRC mismatch".into());
                self.failed = true;
                break;
            }
            consumed += frame.total_size;
            let raw = frame.raw_frame.to_vec();
            if let Err(error) = self.decode(&raw, out) {
                errors.push(error);
                self.failed = true;
                break;
            }
        }
        self.pending.drain(..consumed);
        if self.pending.len() > 0x1000008 {
            errors.push("AC-4 sync frame exceeds its 24-bit size limit".into());
            self.failed = true;
        }
        if self.failed {
            self.pending.clear();
        }
    }
}

fn gain(state: Option<SceneObjectState>) -> f32 {
    state
        .filter(|state| state.metadata_active() && state.semantic_complete())
        .map(|state| state.linear_gain().unwrap_or(1.0))
        .unwrap_or(0.0)
}

fn event(
    id: u32,
    sample_pos: u64,
    state: Option<SceneObjectState>,
    ramp_duration: u32,
) -> ObjectEvent {
    let position = state.and_then(|state| state.position());
    let amplitude = gain(state);
    let size = match state.and_then(|state| state.extent()) {
        Some(ObjectExtent::Uniform(size)) => [f64::from(size); 3],
        Some(ObjectExtent::Cartesian { x, y, z }) => [f64::from(x), f64::from(y), f64::from(z)],
        _ => [0.0; 3],
    };
    ObjectEvent {
        id,
        sample_pos,
        has_pos: position.is_some(),
        pos: position
            .map(|p| [f64::from(p.x()), f64::from(p.y()), f64::from(p.z())])
            .unwrap_or([0.0; 3]),
        gain_db: if amplitude > 0.0 && position.is_some() {
            20.0 * f64::from(amplitude).log10()
        } else {
            -200.0
        },
        size,
        anchor: "room".into(),
        distance_m: None,
        distance_infinite: false,
        screen_factor: state.and_then(|state| state.screen_factor()).map(f64::from),
        depth_factor: state.and_then(|state| state.depth_factor()).map(f64::from),
        ramp_duration,
    }
}

fn scene_frame(
    scene: &Ac4SceneFrame,
    ids: &mut BTreeMap<u64, u32>,
    initialized: &mut BTreeSet<u64>,
    lfe_gains: &mut BTreeMap<u64, GainRamp>,
) -> Result<FrameData, String> {
    let timeline = scene.timeline();
    let sample_pos = u64::try_from(timeline.codec_sample_start())
        .map_err(|_| "AC-4 has a negative codec timeline")?;
    let length = timeline.duration_samples() as usize;
    let mut channels = Vec::new();
    let mut labels = Vec::new();
    let mut events = Vec::new();
    let mut object_channels = Vec::new();
    for bed in scene.beds() {
        let key = bed.element_id().get();
        let envelope = lfe_gains.entry(key).or_insert_with(|| {
            let value = gain(bed.initial_state());
            GainRamp {
                value,
                target: value,
                remaining: 0,
            }
        });
        if bed.components().len() != 1 {
            return Err("AC-4 supports one mono LFE per bed".into());
        }
        for component in bed.components() {
            if component.speaker() != SpeakerLabel::Lfe {
                return Err("AC-4 returned an unsupported bed speaker".into());
            }
            let mut pcm = component.plane().samples().to_vec();
            let mut updates = scene
                .metadata_updates()
                .iter()
                .filter(|update| update.element_id().get() == key)
                .peekable();
            for (offset, sample) in pcm.iter_mut().enumerate() {
                while updates
                    .peek()
                    .is_some_and(|update| update.offset_samples() as usize == offset)
                {
                    let update = updates.next().unwrap();
                    envelope.set(gain(Some(update.state())), update.ramp_duration_samples());
                }
                *sample *= envelope.next();
            }
            channels.push(pcm);
            labels.push("LFE".into());
        }
    }
    for object in scene.objects() {
        let key = object.element_id().get();
        let next_id = u32::try_from(ids.len()).map_err(|_| "AC-4 object ID overflow")?;
        let id = *ids.entry(key).or_insert(next_id);
        let pcm = object.pcm();
        if pcm.planes().len() != 1 {
            return Err("AC-4 object PCM must be mono".into());
        }
        object_channels.push(ObjectChannelDecl {
            id,
            channel: channels.len() as u32,
        });
        channels.push(pcm.planes()[0].samples().to_vec());
        labels.push(format!("Obj_{id}"));
        if initialized.insert(key) {
            events.push(event(id, sample_pos, object.initial_state(), 0));
        }
        for update in scene
            .metadata_updates()
            .iter()
            .filter(|update| update.element_id().get() == key)
        {
            if update.offset_samples() >= timeline.duration_samples() {
                return Err("AC-4 update lies outside its PCM frame".into());
            }
            events.push(event(
                id,
                sample_pos + u64::from(update.offset_samples()),
                Some(update.state()),
                update.ramp_duration_samples(),
            ));
        }
    }
    if length == 0
        || timeline.sample_rate() == 0
        || channels.is_empty()
        || channels.len() > 128
        || channels.iter().any(|channel| {
            channel.len() != length || channel.iter().any(|sample| !sample.is_finite())
        })
    {
        return Err("AC-4 returned invalid PCM dimensions or samples".into());
    }
    events.sort_by_key(|event| (event.sample_pos, event.id));
    initialized.retain(|key| {
        scene
            .objects()
            .iter()
            .any(|object| object.element_id().get() == *key)
    });
    lfe_gains.retain(|key, _| {
        scene
            .beds()
            .iter()
            .any(|bed| bed.element_id().get() == *key)
    });
    let raw_bed_labels = if scene.beds().is_empty() {
        vec![]
    } else {
        vec!["LFE".into()]
    };
    Ok(FrameData {
        codec: "ac4",
        sample_rate: timeline.sample_rate(),
        sample_pos,
        channels,
        labels,
        raw_bed_labels,
        events,
        object_channels,
        program_loudness: None,
        ramp_duration: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_header_waits_and_flush_reports_truncation() {
        let mut pipeline = Ac4Pipeline::new();
        let mut out = VecDeque::new();
        let mut errors = Vec::new();
        for byte in [0xac, 0x40, 0x00, 0x05, 0x01] {
            pipeline.push(&[byte], &mut out, &mut errors);
        }
        assert!(errors.is_empty());
        assert!(out.is_empty());
        pipeline.flush(&mut out, &mut errors);
        assert!(errors[0].contains("truncated"));
        pipeline.reset();
        assert!(pipeline.pending.is_empty());
        assert!(!pipeline.failed);
    }
    #[test]
    fn gain_ramps_continue_across_pcm_frames() {
        let mut ramp = GainRamp::default();
        ramp.set(1.0, 4);
        assert_eq!([ramp.next(), ramp.next()], [0.0, 0.25]);
        assert_eq!([ramp.next(), ramp.next(), ramp.next()], [0.5, 0.75, 1.0]);
    }
}
