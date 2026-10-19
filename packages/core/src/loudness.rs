//! Shared incremental BS.1770 meter. Measurements describe a speaker/master reference, never raw objects.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceLoudness {
    pub integrated_lufs: Option<f64>,
    pub blocks: usize,
    pub true_peak_dbtp: Option<f64>,
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
    pub fn new(rate: u32) -> Self {
        Self::with_weights(rate, vec![1.0, 1.0])
    }
    /// CICP19: exclude LFE, weight horizontal surround channels by 1.41.
    pub fn speakers_7_1_4() -> Self {
        Self::with_weights(
            48000,
            vec![
                1.0, 1.0, 1.0, 0.0, 1.41, 1.41, 1.41, 1.41, 1.0, 1.0, 1.0, 1.0,
            ],
        )
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
            block: (0..weights.len())
                .map(|_| vec![0.0; (rate as f64 * 0.4).round() as usize])
                .collect(),
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
                        .map(|(ch, weight)| {
                            weight * ch.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / n as f64
                        })
                        .sum(),
                );
                for block in &mut self.block {
                    block.copy_within(self.hop..n, 0);
                }
                self.fill = n - self.hop;
            }
        }
    }
    pub fn integrated(&self) -> ReferenceLoudness {
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
            return ReferenceLoudness {
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
        ReferenceLoudness {
            integrated_lufs: Some(
                -0.691 + 10.0 * (relative.iter().sum::<f64>() / relative.len() as f64).log10(),
            ),
            blocks: absolute.len(),
            true_peak_dbtp: peak,
        }
    }
}

/// A decoded speaker bed is an explicit programme reference, even if listening
/// uses its separately reconstructed objects. Role weights follow labels, not
/// the number/order of object channels.
#[derive(Default)]
pub(crate) struct SpeakerReference {
    labels: Vec<String>,
    rate: u32,
    meter: Option<LoudnessMeter>,
}
impl SpeakerReference {
    pub fn measure(
        &mut self,
        pcm: &[Vec<f32>],
        labels: &[String],
        rate: u32,
    ) -> Option<ReferenceLoudness> {
        let weights: Option<Vec<f64>> = labels
            .iter()
            .map(|label| {
                Some(match label.as_str() {
                    "L" | "R" | "C" | "Left" | "Right" | "FrontLeft" | "FrontRight" | "Center"
                    | "FrontCenter" | "WideLeft" | "WideRight" => 1.,
                    "LFE" | "LowFrequencyEffects" | "LowFrequencyEffects2" => 0.,
                    "Ls" | "Rs" | "Lb" | "Rb" | "SurroundLeft" | "SurroundRight" | "RearLeft"
                    | "RearRight" | "RearCenter" => 1.41,
                    "TopFrontLeft" | "TopFrontRight" | "TopSurroundLeft" | "TopSurroundRight"
                    | "TopRearLeft" | "TopRearRight" | "Tfl" | "Tfr" | "Tbl" | "Tbr" => 1.,
                    _ => return None,
                })
            })
            .collect();
        let weights = weights?;
        if pcm.is_empty()
            || pcm.len() != labels.len()
            || pcm.iter().any(|c| c.len() != pcm[0].len())
        {
            return None;
        }
        if self.labels != labels || self.rate != rate || self.meter.is_none() {
            self.meter = Some(LoudnessMeter::with_weights(rate, weights));
            self.labels = labels.to_vec();
            self.rate = rate;
        }
        let meter = self.meter.as_mut().unwrap();
        meter.push(pcm);
        Some(meter.integrated())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn programme_speaker_reference_uses_roles_not_channel_count_and_rejects_raw_objects() {
        let mut m = SpeakerReference::default();
        let n = 32768;
        let tone: Vec<f32> = (0..n).map(|i| 0.2 * (i as f32 * 0.07).sin()).collect();
        let a = m
            .measure(
                &[tone.clone(), vec![0.; n]],
                &["Center".into(), "LFE".into()],
                48000,
            )
            .unwrap();
        let b = SpeakerReference::default()
            .measure(
                &[tone.clone(), vec![0.9; n]],
                &["Center".into(), "LFE".into()],
                48000,
            )
            .unwrap();
        assert!((a.integrated_lufs.unwrap() - b.integrated_lufs.unwrap()).abs() < 1e-9);
        let side = SpeakerReference::default()
            .measure(&[tone.clone()], &["SurroundLeft".into()], 48000)
            .unwrap();
        assert!(
            (side.integrated_lufs.unwrap() - a.integrated_lufs.unwrap() - 10. * 1.41f64.log10())
                .abs()
                < 1e-5
        );
        assert!(SpeakerReference::default()
            .measure(&[tone], &["Obj_1".into()], 48000)
            .is_none());
    }
}
