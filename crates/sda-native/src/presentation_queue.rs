//! Format-independent joining of a scene and ordered compatible PCM chunks.
//! The reference may be packetized differently (e.g. decoder preroll/latency
//! removal). It is never substituted for scene PCM or its object metadata.
use crate::EngineResult;
use sda_core::FrameData;
use std::collections::VecDeque;
use std::mem::size_of;
pub(crate) struct ReadyFrame {
    pub frame: FrameData,
    pub compatibility: Option<[Vec<f32>; 2]>,
}
#[derive(Default)]
pub(crate) struct PresentationQueue {
    scenes: VecDeque<FrameData>,
    reference: [VecDeque<f32>; 2],
    scene_bytes: usize,
    reference_seen: bool,
}
impl PresentationQueue {
    pub fn push(&mut self, mut frame: FrameData) -> EngineResult<Vec<ReadyFrame>> {
        let bed = frame.compatibility_bed.take();
        if bed.is_none() && !self.reference_seen {
            return Ok(vec![ReadyFrame {
                frame,
                compatibility: None,
            }]);
        }
        let samples = bed
            .as_ref()
            .and_then(|b| b.channels.first())
            .map_or(0, Vec::len);
        if let Some(bed) = &bed {
            if bed.channels.is_empty()
                || bed.channels.len() != bed.labels.len()
                || bed
                    .channels
                    .iter()
                    .any(|pcm| pcm.len() != samples || pcm.iter().any(|v| !v.is_finite()))
            {
                return Err("invalid compatible bed channels/samples".into());
            }
        }
        let scene_bytes = frame.channels.iter().map(Vec::len).sum::<usize>() * size_of::<f32>();
        let buffered = self.scene_bytes
            + scene_bytes
            + (self.reference[0].len() + samples) * 2 * size_of::<f32>();
        if buffered > sda_native_renderer::render_command::MAX_QUEUED_PCM_BYTES {
            return Err("compatible presentation alignment buffer is full".into());
        }
        let mut pcm = [vec![0.; samples], vec![0.; samples]];
        if let Some(bed) = &bed {
            for (channel, label) in bed.channels.iter().zip(&bed.labels) {
                let gains = sda_native_renderer::plain_downmix::gains(label);
                for (ear, gain) in pcm.iter_mut().zip(gains) {
                    for (dst, src) in ear.iter_mut().zip(channel) {
                        *dst += *src * gain;
                    }
                }
            }
        }
        if pcm.iter().flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite compatible downmix".into());
        }
        for (queue, ear) in self.reference.iter_mut().zip(pcm) {
            queue.extend(ear);
        }
        self.reference_seen = true;
        self.scene_bytes += scene_bytes;
        self.scenes.push_back(frame);
        self.drain(false)
    }
    pub fn finish(&mut self) -> EngineResult<Vec<ReadyFrame>> {
        let ready = self.drain(true)?;
        // The scene owns the programme clock. A reference renderer may return
        // an additional tail; it must not create extra objects/programme time.
        for ear in &mut self.reference {
            ear.clear();
        }
        self.reference_seen = false;
        Ok(ready)
    }
    fn drain(&mut self, ended: bool) -> EngineResult<Vec<ReadyFrame>> {
        let mut ready = Vec::new();
        while let Some(frame) = self.scenes.front() {
            let samples = frame.channels.first().map_or(0, Vec::len);
            if !ended && self.reference[0].len() < samples {
                break;
            }
            let frame = self.scenes.pop_front().expect("front checked");
            self.scene_bytes -=
                frame.channels.iter().map(Vec::len).sum::<usize>() * size_of::<f32>();
            let available = samples.min(self.reference[0].len());
            let mut pcm = std::array::from_fn(|ear| {
                self.reference[ear].drain(..available).collect::<Vec<_>>()
            });
            // Only true EOS can end a reference early: keep the primary scene
            // duration and use silence for the exhausted reference tail.
            for ear in &mut pcm {
                ear.resize(samples, 0.);
            }
            ready.push(ReadyFrame {
                frame,
                compatibility: Some(pcm),
            });
        }
        Ok(ready)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frame(at: u64, reference: Vec<f32>) -> FrameData {
        FrameData {
            compatibility_bed: Some(sda_core::CompatibilityBed {
                channels: vec![reference.clone(), reference],
                labels: vec!["L".into(), "R".into()],
            }),
            codec: "future-object-format",
            sample_rate: 48000,
            sample_pos: at,
            channels: vec![vec![0.25; 8]],
            labels: vec!["Obj_7".into()],
            raw_bed_labels: vec![],
            events: vec![sda_core::ObjectEvent {
                id: 7,
                sample_pos: at,
                has_pos: true,
                pos: [0.3, 0.7, 0.2],
                gain_db: -3.,
                size: [0.; 3],
                diffuse: 0.,
                anchor: "room".into(),
                distance_m: None,
                distance_infinite: false,
                screen_factor: None,
                depth_factor: None,
                ramp_duration: 128,
            }],
            object_channels: vec![sda_core::ObjectChannelDecl { id: 7, channel: 0 }],
            program_loudness: None,
            ramp_duration: 0,
        }
    }
    #[test]
    fn unequal_packet_sizes_join_without_changing_scene_or_inserting_midstream_silence() {
        let mut q = PresentationQueue::default();
        assert!(q
            .push(frame(0, vec![1., 2., 3., 4., 5.]))
            .unwrap()
            .is_empty());
        let ready = q
            .push(frame(8, vec![6., 7., 8., 9., 10., 11., 12., 13.]))
            .unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].frame.sample_pos, 0);
        assert_eq!(ready[0].frame.channels[0], vec![0.25; 8]);
        assert_eq!(ready[0].frame.object_channels[0].id, 7);
        assert_eq!(ready[0].frame.events[0].sample_pos, 0);
        assert_eq!(ready[0].frame.events[0].gain_db, -3.);
        assert_eq!(
            ready[0].compatibility.as_ref().unwrap()[0],
            vec![1., 2., 3., 4., 5., 6., 7., 8.]
        );
        let end = q.finish().unwrap();
        assert_eq!(end.len(), 1);
        assert_eq!(end[0].frame.sample_pos, 8);
        assert_eq!(end[0].frame.events[0].sample_pos, 8);
        assert_eq!(
            end[0].compatibility.as_ref().unwrap()[0],
            vec![9., 10., 11., 12., 13., 0., 0., 0.]
        );
    }
    #[test]
    fn later_reference_delivery_is_not_misinterpreted_as_a_codec_or_output_mode_change() {
        let mut q = PresentationQueue::default();
        assert!(q.push(frame(0, vec![1.; 5])).unwrap().is_empty());
        let mut delayed = frame(8, vec![]);
        delayed.compatibility_bed = None;
        assert!(q.push(delayed).unwrap().is_empty());
        let ready = q.push(frame(16, vec![2.; 19])).unwrap();
        assert_eq!(ready.len(), 3);
        assert_eq!(
            ready.iter().map(|f| f.frame.sample_pos).collect::<Vec<_>>(),
            vec![0, 8, 16]
        );
        assert!(ready.iter().all(|f| f.compatibility.as_ref().unwrap()[0]
            .iter()
            .all(|s| *s != 0.)));
    }
    #[test]
    fn an_initially_delayed_reference_keeps_the_first_scene_pending() {
        let mut q = PresentationQueue::default();
        assert!(q.push(frame(0, vec![])).unwrap().is_empty());
        let ready = q.push(frame(8, vec![0.5; 16])).unwrap();
        assert_eq!(ready.len(), 2);
        assert_eq!(ready[0].frame.sample_pos, 0);
        assert_eq!(ready[0].frame.events[0].sample_pos, 0);
        assert!(ready
            .iter()
            .all(|f| f.compatibility.as_ref().unwrap()[0] == vec![0.5; 8]));
        assert!(q.finish().unwrap().is_empty());
    }

    #[test]
    fn a_reference_chunk_can_release_multiple_previously_buffered_scene_frames() {
        let mut q = PresentationQueue::default();
        assert!(q.push(frame(0, vec![1.; 4])).unwrap().is_empty());
        let ready = q.push(frame(8, vec![2.; 12])).unwrap();
        assert_eq!(ready.len(), 2);
        assert_eq!(ready[0].frame.sample_pos, 0);
        assert_eq!(ready[1].frame.sample_pos, 8);
        assert!(q.finish().unwrap().is_empty());
    }
}
