//! Windows packages/player/src/bs1770.ts and player.ts playback policy.
//! Measure the original stereo master / upstream MPEG-H reference, never objects
//! or the KU100 output. Gain uses the shared renderer's sample-clock envelope.
use sda_core::FrameData;
use sda_native_renderer::Command;
use serde::{Deserialize, Serialize};

pub const MIN_BLOCKS: usize = 57;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measurement {
    pub integrated_lufs: Option<f64>,
    pub blocks: usize,
    pub true_peak_dbtp: Option<f64>,
}
pub fn master_gain(lufs: f64, peak: Option<f64>) -> f64 {
    if !lufs.is_finite() {
        return 0.0;
    }
    let target = (-18.0 - lufs).clamp(-60.0, 60.0).min(0.0);
    peak.filter(|v| v.is_finite())
        .map_or(target, |v| target.min(-1.0 - v))
}
pub fn stereo_master(f: &FrameData) -> bool {
    f.codec != "adm"
        && f.channels.len() == 2
        && f.labels.len() == 2
        && f.object_channels.is_empty()
        && f.events.is_empty()
        && ["L", "Left", "FrontLeft"].contains(&f.labels[0].as_str())
        && ["R", "Right", "FrontRight"].contains(&f.labels[1].as_str())
}
#[derive(Clone)]
struct Biquad {
    c: [f64; 5],
    x: [f64; 2],
    y: [f64; 2],
}
impl Biquad {
    fn new(c: [f64; 5]) -> Self {
        Self {
            c,
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }
    fn process(&mut self, input: f32) -> f32 {
        let [b0, b1, b2, a1, a2] = self.c;
        let x = input as f64;
        let y = b0 * x + b1 * self.x[0] + b2 * self.x[1] - a1 * self.y[0] - a2 * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y as f32 // Windows writes each stage through Float32Array.
    }
}
fn filters(rate: u32) -> [Biquad; 2] {
    let k = (std::f64::consts::PI * 1681.974450955533 / rate as f64).tan();
    let q = 0.7071752369554196;
    let vh = 10_f64.powf(3.999843853973347 / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad::new([
        (vh + vb * k / q + k * k) / a0,
        2.0 * (k * k - vh) / a0,
        (vh - vb * k / q + k * k) / a0,
        2.0 * (k * k - 1.0) / a0,
        (1.0 - k / q + k * k) / a0,
    ]);
    let k = (std::f64::consts::PI * 38.13547087602444 / rate as f64).tan();
    let q = 0.5003270373238773;
    let a0 = 1.0 + k / q + k * k;
    [
        shelf,
        Biquad::new([
            1.0,
            -2.0,
            1.0,
            2.0 * (k * k - 1.0) / a0,
            (1.0 - k / q + k * k) / a0,
        ]),
    ]
}
pub struct LoudnessMeter {
    filters: Vec<[Biquad; 2]>,
    weights: Vec<f64>,
    block: Vec<Vec<f32>>,
    fill: usize,
    hop: usize,
    energy: Vec<f64>,
    history: Vec<Vec<f32>>,
    write: usize,
    phases: Vec<Vec<(usize, f64)>>,
    peak: f64,
}
impl LoudnessMeter {
    pub fn new(rate: u32) -> Self { Self::with_weights(rate, vec![1.0, 1.0]) }
    /// CICP19: exclude LFE, weight horizontal surround channels by 1.41.
    pub fn speakers_7_1_4() -> Self {
        Self::with_weights(48000, vec![1.0,1.0,1.0,0.0,1.41,1.41,1.41,1.41,1.0,1.0,1.0,1.0])
    }
    fn with_weights(rate: u32, weights: Vec<f64>) -> Self {
        let factor = if rate < 96000 {
            4
        } else if rate < 192000 {
            2
        } else {
            1
        };
        let delay = 49_usize.div_ceil(factor);
        let mut phases = vec![vec![]; factor];
        for tap in 0..49 {
            let offset = tap as f64 - 24.0;
            let t = offset * std::f64::consts::PI / factor as f64;
            let c = (if offset == 0.0 { 1.0 } else { t.sin() / t })
                * 0.5
                * (1.0 - (2.0 * std::f64::consts::PI * tap as f64 / 48.0).cos());
            if c.abs() > 1e-6 {
                phases[tap % factor].push((tap / factor, c));
            }
        }
        Self {
            filters: (0..weights.len()).map(|_| filters(rate)).collect(),
            block: (0..weights.len()).map(|_| vec![0.0; (rate as f64 * 0.4).round() as usize]).collect(),
            fill: 0,
            hop: (rate as f64 * 0.1).round() as usize,
            energy: vec![],
            history: (0..weights.len()).map(|_| vec![0.0; delay]).collect(),
            weights,
            write: 0,
            phases,
            peak: 0.0,
        }
    }
    pub fn push(&mut self, channels: &[Vec<f32>]) {
        for i in 0..channels[0].len() {
            for ch in 0..self.weights.len() {
                let x = channels[ch][i];
                self.history[ch][self.write] = x;
                for phase in &self.phases {
                    let y: f64 = phase
                        .iter()
                        .map(|(offset, c)| {
                            self.history[ch][(self.write + self.history[ch].len() - offset)
                                % self.history[ch].len()] as f64
                                * c
                        })
                        .sum();
                    self.peak = self.peak.max(y.abs());
                }
                let y = self.filters[ch][0].process(x);
                self.block[ch][self.fill] = self.filters[ch][1].process(y);
            }
            self.write = (self.write + 1) % self.history[0].len();
            self.fill += 1;
            if self.fill == self.block[0].len() {
                let n = self.fill;
                self.energy.push(
                    self.block
                        .iter()
                        .zip(&self.weights)
                        .map(|(ch, weight)| weight * ch.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / n as f64)
                        .sum(),
                );
                for block in &mut self.block {
                    block.copy_within(self.hop..n, 0);
                }
                self.fill = n - self.hop;
            }
        }
    }
    pub fn integrated(&self) -> Measurement {
        let absolute: Vec<_> = self
            .energy
            .iter()
            .copied()
            .filter(|e| *e > 0.0 && 10.0 * e.log10() > -69.309)
            .collect();
        let peak = if self.peak > 0.0 {
            Some(20.0 * self.peak.log10())
        } else {
            None
        };
        if absolute.is_empty() {
            return Measurement {
                true_peak_dbtp: peak,
                ..Default::default()
            };
        }
        let mean = absolute.iter().sum::<f64>() / absolute.len() as f64;
        let gate = -0.691 + 10.0 * mean.log10() - 10.0;
        let relative: Vec<_> = absolute
            .iter()
            .copied()
            .filter(|e| -0.691 + 10.0 * e.log10() > gate)
            .collect();
        Measurement {
            integrated_lufs: Some(
                -0.691 + 10.0 * (relative.iter().sum::<f64>() / relative.len() as f64).log10(),
            ),
            blocks: absolute.len(),
            true_peak_dbtp: peak,
        }
    }
}
pub struct PendingFrame {
    pub frame: FrameData,
    pub measurement: Option<Measurement>,
}
#[derive(Default)]
pub struct VolumeBalance {
    pub enabled: bool,
    pub eligible: bool,
    non_stereo_seen: bool,
    meter: Option<LoudnessMeter>,
    post_counter: usize,
    cached: Option<Measurement>,
    settled: bool,
    balanced_blocks: usize,
    pub gain_db: f64,
    completed: bool,
}
impl VolumeBalance {
    pub fn reset(&mut self) {
        let enabled = self.enabled;
        *self = Self {
            enabled,
            ..Default::default()
        };
    }
    pub fn set_cached(&mut self, cached: Measurement) {
        self.cached = if cached.blocks >= MIN_BLOCKS
            && cached.integrated_lufs.is_some_and(|v| v.is_finite())
        {
            Some(cached)
        } else {
            None
        };
        self.settled = false;
    }
    pub fn measure(&mut self, frame: FrameData, reference: Option<Vec<Vec<f32>>>) -> PendingFrame {
        let channels = if frame.codec == "mpegh" {
            reference.as_deref()
        } else if stereo_master(&frame) {
            Some(frame.channels.as_slice())
        } else {
            None
        };
        let mut measurement = None;
        if let Some(channels) = channels {
            self.meter
                .get_or_insert_with(|| LoudnessMeter::new(frame.sample_rate))
                .push(channels);
            self.post_counter += 1;
            if self.post_counter % 8 == 0 {
                measurement = self.meter.as_ref().map(LoudnessMeter::integrated);
            }
        }
        PendingFrame { frame, measurement }
    }
    pub fn route(&mut self, frame: &PendingFrame) -> Vec<Command> {
        let f = &frame.frame;
        let stereo = f.codec != "mpegh" && stereo_master(f);
        if f.codec != "mpegh" && !stereo {
            self.non_stereo_seen = true;
        }
        let eligible = !self.non_stereo_seen && (f.codec == "mpegh" || stereo);
        let mut commands = vec![];
        if eligible != self.eligible {
            self.eligible = eligible;
            commands.push(Command::SetProgramEnabled {
                enabled: self.enabled && eligible,
            });
        }
        if !eligible {
            return commands;
        }
        if let Some(cached) = &self.cached {
            if !self.settled {
                self.gain_db = master_gain(
                    cached.integrated_lufs.unwrap(),
                    if stereo { cached.true_peak_dbtp } else { None },
                );
                self.settled = true;
                commands.push(Command::SetProgramGain {
                    gain: 10_f64.powf(self.gain_db / 20.0) as f32,
                    at: Some(f.sample_pos),
                });
            }
        } else if let Some(m) = &frame.measurement {
            if m.blocks >= MIN_BLOCKS && (!self.settled || m.blocks >= self.balanced_blocks + 50) {
                if let Some(lufs) = m.integrated_lufs.filter(|v| v.is_finite()) {
                    self.settled = true;
                    self.balanced_blocks = m.blocks;
                    let gain = master_gain(lufs, if stereo { m.true_peak_dbtp } else { None });
                    let previous = self.gain_db;
                    self.gain_db = gain;
                    if (gain - previous).abs() >= 0.05 {
                        let steps = ((gain - previous).abs() / 0.75).ceil() as u64;
                        let samples = (0.25_f64.min(4.0 / steps as f64) * f.sample_rate as f64)
                            .round() as u64;
                        for i in 1..=steps {
                            let db = if i == steps {
                                gain
                            } else {
                                previous + (gain - previous) * i as f64 / steps as f64
                            };
                            commands.push(Command::SetProgramGain {
                                gain: 10_f64.powf(db / 20.0) as f32,
                                at: Some(f.sample_pos + i * samples),
                            });
                        }
                    }
                }
            }
        }
        commands
    }
    pub fn finish(&mut self) {
        self.completed = true;
    }
    pub fn complete_measurement(&self) -> Option<Measurement> {
        let m = self.meter.as_ref()?.integrated();
        (self.completed && self.eligible && m.blocks >= MIN_BLOCKS && m.integrated_lufs.is_some())
            .then_some(m)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_meter_excludes_lfe_and_retains_stereo_calibration() {
        let tone: Vec<f32> = (0..48000).map(|i| 0.5*(i as f32*std::f32::consts::TAU*1000.0/48000.0).sin()).collect();
        let mut lfe_only = vec![vec![0.0;48000];12]; lfe_only[3] = tone.clone();
        let mut meter = LoudnessMeter::speakers_7_1_4(); meter.push(&lfe_only);
        assert!(meter.integrated().integrated_lufs.is_none());
        let mut speakers = vec![vec![0.0;48000];12]; speakers[0] = tone.clone(); speakers[1] = tone.clone();
        let mut multichannel = LoudnessMeter::speakers_7_1_4(); multichannel.push(&speakers);
        let mut stereo = LoudnessMeter::new(48000); stereo.push(&[tone.clone(),tone]);
        assert_eq!(multichannel.integrated().integrated_lufs,stereo.integrated().integrated_lufs);
        assert_eq!(multichannel.integrated().true_peak_dbtp,stereo.integrated().true_peak_dbtp);
    }
    fn frame(codec: &'static str, at: u64) -> FrameData {
        FrameData {
            codec,
            sample_rate: 48000,
            sample_pos: at,
            channels: vec![vec![0.0; 1024]; 2],
            labels: vec!["L".into(), "R".into()],
            raw_bed_labels: vec!["L".into(), "R".into()],
            object_channels: vec![],
            events: vec![],
            program_loudness: None,
            ramp_duration: 0,
        }
    }
    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("balance-windows-fixtures.json")).unwrap()
    }
    #[test]
    fn windows_meter_vectors_match_at_all_interpolation_rates_and_chunk_sizes() {
        for wave in fixture()["waves"].as_array().unwrap() {
            let rate = wave["rate"].as_u64().unwrap() as u32;
            for chunk in [997, 4096] {
                let mut meter = LoudnessMeter::new(rate);
                let count = (rate as f64 * 6.4).round() as usize;
                for start in (0..count).step_by(chunk) {
                    let channels: Vec<Vec<f32>> = (0..2)
                        .map(|ch| {
                            (start..(start + chunk).min(count))
                                .map(|i| {
                                    let amp = if (i as f64) < rate as f64 * 1.1 {
                                        0.012
                                    } else {
                                        0.42
                                    };
                                    (amp * ((2.0 * std::f64::consts::PI * 997.0 * i as f64
                                        / rate as f64
                                        + ch as f64 * 0.3)
                                        .sin()
                                        + 0.13
                                            * (2.0 * std::f64::consts::PI * 17003.0 * i as f64
                                                / rate as f64)
                                                .sin())) as f32
                                })
                                .collect()
                        })
                        .collect();
                    meter.push(&channels);
                }
                let m = meter.integrated();
                assert_eq!(m.blocks, wave["blocks"].as_u64().unwrap() as usize);
                assert!(
                    (m.integrated_lufs.unwrap() - wave["integratedLufs"].as_f64().unwrap()).abs()
                        < 1e-6,
                    "rate={rate}"
                );
                assert!(
                    (m.true_peak_dbtp.unwrap() - wave["truePeakDbtp"].as_f64().unwrap()).abs()
                        < 1e-6,
                    "peak rate={rate}"
                );
            }
        }
        let mut silence = LoudnessMeter::new(48000);
        silence.push(&vec![vec![0.0; 48000]; 2]);
        assert_eq!(silence.integrated().integrated_lufs, None);
        assert_eq!(silence.integrated().blocks, 0);
        assert!(serde_json::to_string(&silence.integrated())
            .unwrap()
            .contains("null"));
    }
    #[test]
    fn windows_protective_gain_and_sample_clock_staircase_match() {
        for v in fixture()["gains"].as_array().unwrap() {
            assert_eq!(
                master_gain(v["lufs"].as_f64().unwrap(), v["peak"].as_f64()),
                v["gainDb"].as_f64().unwrap()
            );
        }
        assert_eq!(master_gain(f64::NAN, Some(-1.0)), 0.0);
        let mut balance = VolumeBalance {
            enabled: true,
            ..Default::default()
        };
        for step in fixture()["schedule"].as_array().unwrap() {
            let pending = PendingFrame {
                frame: frame("alac", step["at"].as_u64().unwrap()),
                measurement: Some(Measurement {
                    integrated_lufs: step["lufs"].as_f64(),
                    blocks: step["blocks"].as_u64().unwrap() as usize,
                    true_peak_dbtp: step["peak"].as_f64(),
                }),
            };
            let commands = balance.route(&pending);
            let gains: Vec<_> = commands
                .iter()
                .filter_map(|c| {
                    if let Command::SetProgramGain { gain, at } = c {
                        Some((*gain, *at))
                    } else {
                        None
                    }
                })
                .collect();
            let expected = step["calls"].as_array().unwrap();
            assert_eq!(gains.len(), expected.len());
            for ((gain, at), v) in gains.iter().zip(expected) {
                assert_eq!(*at, v["at"].as_u64());
                assert!(
                    (*gain as f64 - 10_f64.powf(v["gainDb"].as_f64().unwrap() / 20.0)).abs() < 1e-7
                );
            }
        }
    }
    #[test]
    fn cached_first_sample_sticky_eligibility_and_mpegh_never_uses_reference_peak_to_boost() {
        let mut b = VolumeBalance {
            enabled: true,
            ..Default::default()
        };
        b.set_cached(Measurement {
            integrated_lufs: Some(-23.0),
            blocks: 100,
            true_peak_dbtp: Some(2.0),
        });
        let p = PendingFrame {
            frame: frame("alac", 0),
            measurement: None,
        };
        let commands = b.route(&p);
        assert!(matches!(
            commands[0],
            Command::SetProgramEnabled { enabled: true }
        ));
        assert!(matches!(
            commands[1],
            Command::SetProgramGain { at: Some(0), .. }
        ));
        assert_eq!(b.gain_db, -3.0);
        assert!(b.route(&p).is_empty());
        let mut objects = frame("eac3", 1024);
        objects.labels[0] = "Obj_0".into();
        let commands = b.route(&PendingFrame {
            frame: objects,
            measurement: None,
        });
        assert!(matches!(
            commands[0],
            Command::SetProgramEnabled { enabled: false }
        ));
        assert!(b.route(&p).is_empty());
        assert!(!b.eligible);
        b.reset();
        assert!(b.enabled);
        assert!(!b.eligible);
        assert_eq!(b.gain_db, 0.0);
        b.set_cached(Measurement {
            integrated_lufs: Some(-23.0),
            blocks: 100,
            true_peak_dbtp: Some(2.0),
        });
        b.route(&PendingFrame {
            frame: frame("mpegh", 0),
            measurement: None,
        });
        assert_eq!(
            b.gain_db, 0.0,
            "reference peak is not SDA rendered true peak"
        );
        b.reset();
        b.set_cached(Measurement {
            integrated_lufs: Some(-10.0),
            blocks: 100,
            true_peak_dbtp: Some(2.0),
        });
        b.route(&PendingFrame {
            frame: frame("mpegh", 0),
            measurement: None,
        });
        assert_eq!(b.gain_db, -8.0);
    }
    #[test]
    fn only_complete_eligible_decodes_can_be_persisted_and_measurement_is_independent_of_toggle() {
        let mut b = VolumeBalance::default();
        let mut at = 0;
        for _ in 0..300 {
            let mut f = frame("alac", at);
            for ch in &mut f.channels {
                for (i, x) in ch.iter_mut().enumerate() {
                    *x = (0.5
                        * (2.0 * std::f64::consts::PI * 997.0 * (at + i as u64) as f64 / 48000.0)
                            .sin()) as f32;
                }
            }
            let p = b.measure(f, None);
            b.route(&p);
            at += 1024;
        }
        assert!(!b.enabled);
        assert!(b.gain_db < 0.0);
        assert!(
            b.complete_measurement().is_none(),
            "stopped/aborted intro must not cache"
        );
        b.finish();
        assert!(b.complete_measurement().is_some());
        b.reset();
        assert!(!b.enabled);
        assert!(b.complete_measurement().is_none());
    }
    #[test]
    fn source_classification_matches_windows_not_container_stereo_core() {
        let mut f = frame("alac", 0);
        assert!(stereo_master(&f));
        f.codec = "adm";
        assert!(!stereo_master(&f));
        f.codec = "eac3";
        f.labels[0] = "Obj_1".into();
        assert!(!stereo_master(&f));
        f.labels[0] = "C".into();
        assert!(!stereo_master(&f));
        f.labels = vec!["Left".into(), "Right".into()];
        assert!(stereo_master(&f));
        f.channels.push(vec![0.0; 1024]);
        assert!(!stereo_master(&f));
    }
}
