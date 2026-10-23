//! Measurement-only fixed 7.1.4 VBAP reference for decoded object scenes.
//! It is independent of HRTF/head pose/user enhancements and is never audible.
//! Objects are summed in speaker channels before K-weighting (correlation matters).
use crate::{
    loudness::{LoudnessMeter, ReferenceLoudness},
    vbap::VbapSolver,
    FrameData, ObjectEvent,
};
use std::collections::BTreeMap;
const LABELS: [&str; 12] = [
    "L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs", "Tfl", "Tfr", "Tbl", "Tbr",
];
#[derive(Default)]
struct Row {
    current: [f32; 12],
    target: [f32; 12],
    remaining: usize,
    initialized: bool,
}
impl Row {
    fn set(&mut self, target: [f32; 12], duration: u32) {
        self.target = target;
        if !self.initialized || duration <= 1 {
            self.current = target;
            self.remaining = 0;
        } else {
            self.remaining = duration as usize;
        }
        self.initialized = true;
    }
    fn mix(&mut self, input: &[f32], output: &mut [Vec<f32>], start: usize, end: usize) {
        let ramp = (end - start).min(self.remaining);
        for ch in 0..12 {
            let a = self.current[ch];
            let b = self.target[ch];
            if a == 0. && b == 0. {
                continue;
            }
            let step = if self.remaining > 0 {
                (b - a) / self.remaining as f32
            } else {
                0.
            };
            for i in 0..ramp {
                output[ch][start + i] += input[start + i] * (a + step * (i + 1) as f32);
            }
            let g = if ramp == self.remaining {
                b
            } else {
                a + step * ramp as f32
            };
            for i in start + ramp..end {
                output[ch][i] += input[i] * g;
            }
            self.current[ch] = g;
        }
        self.remaining -= ramp;
    }
}
pub struct SceneMeter {
    solver: VbapSolver,
    rows: BTreeMap<u32, Row>,
    declarations: BTreeMap<usize, u32>,
    labels: Vec<String>,
    states: BTreeMap<u32, ObjectEvent>,
    meter: LoudnessMeter,
    sample_rate: u32,
}
impl SceneMeter {
    pub fn new() -> Self {
        Self::with_sample_rate(48000)
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn with_sample_rate(sample_rate: u32) -> Self {
        let angles: [(f64, f64); 12] = [
            (30., 0.),
            (-30., 0.),
            (0., 0.),
            (0., 0.),
            (135., 0.),
            (-135., 0.),
            (90., 0.),
            (-90., 0.),
            (45., 45.),
            (-45., 45.),
            (135., 45.),
            (-135., 45.),
        ];
        let mut dirs = vec![];
        let mut azimuths = vec![];
        for (a, e) in angles {
            azimuths.push(a);
            let (a, e) = (a.to_radians(), e.to_radians());
            dirs.extend([-a.sin() * e.cos(), a.cos() * e.cos(), e.sin()]);
        }
        Self {
            solver: VbapSolver::from_parts(&dirs, &[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0], &azimuths)
                .expect("fixed reference layout"),
            rows: BTreeMap::new(),
            declarations: BTreeMap::new(),
            labels: vec![],
            states: BTreeMap::new(),
            meter: LoudnessMeter::speakers_7_1_4_at_rate(sample_rate),
            sample_rate,
        }
    }
    fn event(&mut self, event: &ObjectEvent) {
        let mut state = event.clone();
        if !event.has_pos {
            let Some(previous) = self.states.get(&event.id) else {
                return;
            };
            state = previous.clone();
            state.gain_db = event.gain_db;
            state.sample_pos = event.sample_pos;
            state.ramp_duration = event.ramp_duration;
        }
        self.states.insert(event.id, state.clone());
        let e = &state;
        let pos = if e.pos.iter().all(|v| v.abs() < 1e-12) {
            [0., 1., 0.]
        } else {
            e.pos
        };
        let spread = e
            .size
            .iter()
            .copied()
            .fold(e.diffuse, f64::max)
            .clamp(0., 1.);
        let p = self.solver.pan(pos, spread);
        let gain = 10f64.powf(e.gain_db / 20.);
        let mut row = [0.; 12];
        for i in 0..12 {
            row[i] = (p[i] * gain) as f32;
        }
        self.rows.entry(e.id).or_default().set(row, e.ramp_duration);
    }
    fn mix(&mut self, f: &FrameData, out: &mut [Vec<f32>], start: usize, end: usize) {
        for (channel, samples) in f.channels.iter().enumerate() {
            if let Some(id) = self.declarations.get(&channel) {
                if let Some(row) = self.rows.get_mut(id) {
                    row.mix(samples, out, start, end);
                }
            } else if let Some(speaker) = LABELS.iter().position(|s| *s == f.labels[channel]) {
                for i in start..end {
                    out[speaker][i] += samples[i];
                }
            }
        }
    }
    pub fn measure(&mut self, f: &FrameData) -> ReferenceLoudness {
        debug_assert_eq!(f.sample_rate, self.sample_rate);
        let n = f.channels[0].len();
        let mut out = vec![vec![0.; n]; 12];
        let mut offset = 0;
        if self.labels != f.labels {
            self.declarations.retain(|channel, _| {
                self.labels.get(*channel) == f.labels.get(*channel) && *channel < f.labels.len()
            });
            self.labels = f.labels.clone();
        }
        for declaration in &f.object_channels {
            let channel = declaration.channel as usize;
            self.declarations
                .retain(|old, id| *id != declaration.id || *old == channel);
            self.declarations.insert(channel, declaration.id);
        }
        for (channel, label) in f.labels.iter().enumerate() {
            if let Some(id) = label
                .strip_prefix("Obj_")
                .and_then(|id| id.parse::<u32>().ok())
            {
                self.declarations.entry(channel).or_insert(id);
            }
        }
        let active = &self.declarations;
        self.rows
            .retain(|id, _| active.values().any(|current| current == id));
        self.states
            .retain(|id, _| active.values().any(|current| current == id));
        for e in &f.events {
            let at = e.sample_pos.saturating_sub(f.sample_pos).min(n as u64) as usize;
            self.mix(f, &mut out, offset, at);
            self.event(e);
            offset = at;
        }
        self.mix(f, &mut out, offset, n);
        self.meter.push(&out);
        self.meter.integrated()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectChannelDecl;
    fn frame(at: u64, scale: f32, second: Option<f32>, gain: f64) -> FrameData {
        let tone: Vec<f32> = (0..1024)
            .map(|i| scale * (2. * std::f32::consts::PI * 997. * (at + i) as f32 / 48000.).sin())
            .collect();
        let mut f = FrameData {
            reference_loudness: None,
            compatibility_bed: None,
            codec: "av3a",
            sample_rate: 48000,
            sample_pos: at,
            channels: vec![tone.clone()],
            labels: vec!["Obj_1".into()],
            raw_bed_labels: vec![],
            object_channels: vec![ObjectChannelDecl { id: 1, channel: 0 }],
            events: vec![],
            program_loudness: None,
            ramp_duration: 0,
        };
        if let Some(s) = second {
            f.channels.push(tone.iter().map(|v| v * s).collect());
            f.labels.push("Obj_2".into());
            f.object_channels
                .push(ObjectChannelDecl { id: 2, channel: 1 });
        }
        if at == 0 {
            for o in &f.object_channels {
                f.events.push(ObjectEvent {
                    id: o.id,
                    sample_pos: 0,
                    has_pos: true,
                    pos: [0., 1., 0.],
                    gain_db: gain,
                    size: [0.; 3],
                    diffuse: 0.,
                    anchor: "room".into(),
                    distance_m: None,
                    distance_infinite: false,
                    screen_factor: None,
                    depth_factor: None,
                    ramp_duration: 1,
                });
            }
        }
        f
    }
    fn run(second: Option<f32>, gain: f64) -> ReferenceLoudness {
        let mut m = SceneMeter::new();
        let mut result = ReferenceLoudness::default();
        for i in 0..32 {
            result = m.measure(&frame(i * 1024, 0.2, second, gain));
        }
        result
    }
    #[test]
    fn av3a_object_reference_sums_correlated_objects_before_measuring() {
        let one = run(None, 0.).integrated_lufs.unwrap();
        let two = run(Some(1.), 0.).integrated_lufs.unwrap();
        assert!((two - one - 6.020599913).abs() < 1e-5);
        assert!(
            run(Some(-1.), 0.).integrated_lufs.is_none(),
            "opposite-phase colocated objects cancel in the reference"
        );
    }
    #[test]
    fn av3a_object_reference_applies_authored_gain_once_and_excludes_lfe_energy() {
        assert!(
            (run(None, -6.020599913).integrated_lufs.unwrap()
                - run(None, 0.).integrated_lufs.unwrap()
                + 6.020599913)
                .abs()
                < 1e-5
        );
        let mut meter = SceneMeter::new();
        let mut result = ReferenceLoudness::default();
        for i in 0..32 {
            let mut f = frame(i * 1024, 0.2, None, 0.);
            let original = f.channels.clone();
            f.channels.push(vec![0.9; 1024]);
            f.labels.push("LFE".into());
            result = meter.measure(&f);
            assert_eq!(f.channels[0], original[0]);
        }
        assert!(
            (result.integrated_lufs.unwrap() - run(None, 0.).integrated_lufs.unwrap()).abs() < 1e-9
        );
        assert!(result.true_peak_dbtp.unwrap() > run(None, 0.).true_peak_dbtp.unwrap());
    }
    #[test]
    fn av3a_object_reference_ramps_are_continuous_across_chunks_and_reset_is_fresh() {
        let mut a = Row::default();
        a.set([1.; 12], 1);
        a.set([0.5; 12], 64);
        let mut b = Row::default();
        b.set([1.; 12], 1);
        b.set([0.5; 12], 64);
        let mut x = vec![vec![0.; 128]; 12];
        let mut y = x.clone();
        let pcm = vec![1.; 128];
        a.mix(&pcm, &mut x, 0, 128);
        b.mix(&pcm, &mut y, 0, 17);
        b.mix(&pcm, &mut y, 17, 90);
        b.mix(&pcm, &mut y, 90, 128);
        for (x, y) in x.iter().flatten().zip(y.iter().flatten()) {
            assert!((x - y).abs() < 1e-6);
        }
        let f = frame(0, 0.2, None, 0.);
        assert_eq!(SceneMeter::new().measure(&f).blocks, 0);
    }
}

#[cfg(test)]
mod shared_tests {
    use super::*;
    #[test]
    fn scene_reference_is_codec_independent_and_preserves_source_pcm() {
        let mut measurements = Vec::new();
        for codec in ["av3a", "ac4", "future-object-format"] {
            let mut meter = SceneMeter::new();
            let mut result = ReferenceLoudness::default();
            for block in 0..310 {
                let at = block * 1024;
                let input: Vec<f32> = (0..1024)
                    .map(|i| 0.3 * (std::f32::consts::TAU * 997. * (at + i) as f32 / 48000.).sin())
                    .collect();
                let frame = FrameData {
                    reference_loudness: None,
                    compatibility_bed: None,
                    codec,
                    sample_rate: 48000,
                    sample_pos: at as u64,
                    channels: vec![input.clone()],
                    labels: vec!["Obj_42".into()],
                    raw_bed_labels: vec![],
                    object_channels: vec![crate::ObjectChannelDecl { id: 42, channel: 0 }],
                    events: if block == 0 {
                        vec![ObjectEvent {
                            id: 42,
                            sample_pos: 0,
                            has_pos: true,
                            pos: [0., 1., 0.],
                            gain_db: 0.,
                            size: [0.; 3],
                            diffuse: 0.,
                            anchor: "room".into(),
                            distance_m: None,
                            distance_infinite: false,
                            screen_factor: None,
                            depth_factor: None,
                            ramp_duration: 1,
                        }]
                    } else {
                        vec![]
                    },
                    program_loudness: None,
                    ramp_duration: 0,
                };
                result = meter.measure(&frame);
                assert_eq!(frame.channels[0], input);
            }
            assert!(result.blocks >= 57);
            assert!(result.integrated_lufs.unwrap() > -18.);
            measurements.push((result.integrated_lufs, result.true_peak_dbtp));
        }
        assert!(measurements.windows(2).all(|pair| pair[0] == pair[1]));
    }
}

#[cfg(test)]
mod sparse_tests {
    use super::*;
    #[test]
    fn sparse_declarations_and_gain_only_updates_preserve_scene_reference() {
        let mut sparse = SceneMeter::new();
        let mut complete = SceneMeter::new();
        for block in 0..150u64 {
            let at = block * 1024;
            let event = ObjectEvent {
                id: 42,
                sample_pos: at,
                has_pos: true,
                pos: [1., -1., 0.],
                gain_db: if block >= 50 { -6. } else { 0. },
                size: [0.; 3],
                anchor: "room".into(),
                diffuse: 0.,
                distance_m: None,
                distance_infinite: false,
                screen_factor: None,
                depth_factor: None,
                ramp_duration: 128,
            };
            let pcm: Vec<_> = (0..1024)
                .map(|i| 0.2 * (std::f32::consts::TAU * 997. * (at + i) as f32 / 48000.).sin())
                .collect();
            let mut frame = FrameData {
                reference_loudness: None,
                compatibility_bed: None,
                codec: "ac4",
                sample_rate: 48000,
                sample_pos: at,
                channels: vec![pcm],
                labels: vec!["logical-source".into()],
                raw_bed_labels: vec![],
                events: if block == 0 || block == 50 {
                    vec![event]
                } else {
                    vec![]
                },
                object_channels: vec![crate::ObjectChannelDecl { id: 42, channel: 0 }],
                program_loudness: None,
                ramp_duration: 0,
            };
            let a = complete.measure(&frame);
            if block > 0 {
                frame.object_channels.clear();
            }
            if block == 50 {
                frame.events[0].has_pos = false;
                frame.events[0].pos = [f64::NAN; 3];
            }
            let b = sparse.measure(&frame);
            assert_eq!(a.integrated_lufs, b.integrated_lufs);
            assert_eq!(a.true_peak_dbtp, b.true_peak_dbtp);
        }
    }
}
