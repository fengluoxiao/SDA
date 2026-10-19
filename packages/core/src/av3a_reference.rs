//! Measurement-only fixed 7.1.4 VBAP reference for decoded AV3A scenes.
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
pub(crate) struct SceneMeter {
    solver: VbapSolver,
    rows: BTreeMap<u32, Row>,
    meter: LoudnessMeter,
}
impl SceneMeter {
    pub fn new() -> Self {
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
            meter: LoudnessMeter::speakers_7_1_4(),
        }
    }
    fn event(&mut self, e: &ObjectEvent) {
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
            if let Some(object) = f
                .object_channels
                .iter()
                .find(|o| o.channel as usize == channel)
            {
                if let Some(row) = self.rows.get_mut(&object.id) {
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
        debug_assert_eq!(f.sample_rate, 48000);
        let n = f.channels[0].len();
        let mut out = vec![vec![0.; n]; 12];
        let mut offset = 0;
        self.rows
            .retain(|id, _| f.object_channels.iter().any(|o| o.id == *id));
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
