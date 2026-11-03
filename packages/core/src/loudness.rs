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
    history_length: usize,
    history_peak: Vec<f32>,
    write: usize,
    phases: Vec<Vec<(usize, f64)>>,
    peak_bound: f64,
    peak: f64,
}
impl LoudnessMeter {
    pub fn new(rate: u32) -> Self {
        Self::with_weights(rate, vec![1.0, 1.0])
    }
    /// CICP19: exclude LFE, weight horizontal surround channels by 1.41.
    pub fn speakers_7_1_4() -> Self {Self::speakers_7_1_4_at_rate(48000)}
    pub(crate) fn speakers_7_1_4_at_rate(sample_rate:u32) -> Self {
        Self::with_weights(
            sample_rate,
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
            history: (0..weights.len()).map(|_| vec![0.0; delay * 2]).collect(),
            history_length: delay,
            history_peak: vec![0.0; weights.len()],
            weights,
            write: 0,
            peak_bound: phases.iter().map(|phase|phase.iter().map(|(_,c)|c.abs()).sum::<f64>()).fold(0.,f64::max)*(1.+1e-12),
            phases,
            peak: 0.0,
        }
    }
    pub fn push(&mut self, channels: &[Vec<f32>]) {
        let mut offset = 0;
        while offset < channels[0].len() {
            let count = (channels[0].len() - offset).min(self.block[0].len() - self.fill);
            // Process a contiguous span per channel. Filter/FIR states are
            // independent; energy blocks and their addition order stay intact.
            for ch in 0..self.weights.len() {
                let channel = &channels[ch];
                let history = &mut self.history[ch];
                let filters = &mut self.filters[ch];
                let output = &mut self.block[ch][self.fill..self.fill + count];
                let mut write = self.write;
                let mut window_peak = self.history_peak[ch];
                let mut peak = self.peak;
                for (&x, out) in channel[offset..offset + count].iter().zip(output) {
                    let outgoing = history[write].abs();
                    history[write] = x;
                    history[write + self.history_length] = x;
                    let incoming = x.abs();
                    // Maintain the exact FIR-window maximum. Only rescan when
                    // its previous maximum leaves; no peak is approximated.
                    window_peak = if incoming >= window_peak { incoming }
                        else if outgoing >= window_peak {
                            history[write+1..write+self.history_length+1]
                                .iter().map(|x| x.abs()).fold(0.0_f32, f32::max)
                        } else { window_peak };
                    // The FIR L1 norm bounds every possible inter-sample peak
                    // in this window, so a smaller bound cannot change peak.
                    if f64::from(window_peak) * self.peak_bound > peak {
                        for phase in &self.phases {
                            let y: f64 = phase.iter().map(|(delay, c)|
                                f64::from(history[write+self.history_length-delay])*c).sum();
                            peak = peak.max(y.abs());
                        }
                    }
                    let filtered = filters[0].process(x);
                    *out = filters[1].process(filtered);
                    write += 1;
                    if write == self.history_length { write = 0; }
                }
                self.history_peak[ch] = window_peak;
                self.peak = peak;
            }
            self.write = (self.write + count) % self.history_length;
            self.fill += count;
            offset += count;
            if self.fill == self.block[0].len() {
                let n = self.fill;
                self.energy.push(
                    self.block.iter().zip(&self.weights).map(|(ch, weight)| {
                        weight * ch.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / n as f64
                    }).sum(),
                );
                for block in &mut self.block { block.copy_within(self.hop..n, 0); }
                self.fill = n - self.hop;
            }
        }
    }
    pub fn integrated(&self) -> ReferenceLoudness {
        // Energy-domain gates avoid two log/allocate passes over programme
        // history for every decoded frame. Preserve the original comparison at
        // floating-point gate boundaries instead of changing the gate policy.
        let absolute_gate=10f64.powf(-69.309/10.);
        let absolute=|e:f64| e>0. && if (e/absolute_gate-1.).abs()<1e-10 {10.*e.log10()>-69.309}else{e>absolute_gate};
        let mut sum=0.;let mut blocks=0usize;
        for &e in &self.energy {if absolute(e){sum+=e;blocks+=1;}}
        let peak=if self.peak>0.{Some(20.*self.peak.log10())}else{None};
        if blocks==0{return ReferenceLoudness{true_peak_dbtp:peak,..Default::default()};}
        let mean=sum/blocks as f64;let gate_energy=mean*0.1;
        let gate=-0.691+10.*mean.log10()-10.;let mut gated=0.;let mut count=0usize;
        for &e in &self.energy {if absolute(e) && if (e/gate_energy-1.).abs()<1e-10 {-0.691+10.*e.log10()>gate}else{e>gate_energy}{gated+=e;count+=1;}}
        ReferenceLoudness{integrated_lufs:Some(-0.691+10.*(gated/count as f64).log10()),blocks,true_peak_dbtp:peak}
    }
}

/// A decoded speaker bed is an explicit programme reference, even if listening
/// uses its separately reconstructed objects. Role weights follow labels, not
/// the number/order of object channels.
#[derive(Default)]
pub struct SpeakerReference {
    labels: Vec<String>,
    rate: u32,
    meter: Option<LoudnessMeter>,
}
impl SpeakerReference {
    pub fn push(
        &mut self,
        pcm: &[Vec<f32>],
        labels: &[String],
        rate: u32,
    ) -> Option<()> {
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
                    | "TopRearLeft" | "TopRearRight" | "Tfl" | "Tfr" | "Tbl" | "Tbr" | "Tsl" | "Tsr" => 1.,
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
        Some(())
    }
    pub fn measurement(&self) -> Option<ReferenceLoudness> {self.meter.as_ref().map(LoudnessMeter::integrated)}
    pub fn measure(&mut self,pcm:&[Vec<f32>],labels:&[String],rate:u32)->Option<ReferenceLoudness>{
        self.push(pcm,labels,rate)?;
        self.measurement()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn deferred_programme_gate_matches_every_frame_measurement_bit_for_bit() {
        let labels:Vec<String>=["L","R","C","LFE","Ls","Rs"].into_iter().map(String::from).collect();
        let mut deferred=super::SpeakerReference::default();let mut incremental=super::SpeakerReference::default();let mut expected=None;
        for block in 0..300 {
            let pcm:Vec<Vec<f32>>=(0..labels.len()).map(|channel|(0..317).map(|i|{
                let t=(block*317+i)as f32/44100.;
                (t*(370.+channel as f32*73.)*std::f32::consts::TAU).sin()*if block<40{0.002}else{0.24}
            }).collect()).collect();
            deferred.push(&pcm,&labels,44100).unwrap();expected=incremental.measure(&pcm,&labels,44100);
        }
        let actual=deferred.measurement().unwrap();let expected=expected.unwrap();
        assert_eq!(actual.integrated_lufs.map(f64::to_bits),expected.integrated_lufs.map(f64::to_bits));
        assert_eq!(actual.true_peak_dbtp.map(f64::to_bits),expected.true_peak_dbtp.map(f64::to_bits));
        assert_eq!(actual.blocks,expected.blocks);
    }

    use super::*;

    #[test]
    fn sliding_peak_bound_matches_unconditional_fir_at_all_oversampling_rates() {
        for rate in [8000, 44100, 48000, 96000, 192000] {
            let mut meter = LoudnessMeter::with_weights(rate, vec![1.0, 0.0, 1.41]);
            let pcm: Vec<Vec<f32>> = (0..3).map(|channel| (0..24000).map(|i| {
                let phase = (i * (17 + channel * 7)) as f32 * 0.031;
                let level = match i / 3000 { 0 => 0.0, 1 => 0.01, 2 => 0.8, 3 => 0.07, 4 => 0.0, 5 => 0.4, _ => 0.002 };
                if i % 1013 == channel { 0.95 } else { phase.sin() * level }
            }).collect()).collect();
            let mut history = vec![vec![0.0_f32; meter.history_length * 2]; pcm.len()];
            let mut write = 0;
            let mut expected_peak = 0.0_f64;
            for i in 0..pcm[0].len() {
                for ch in 0..pcm.len() {
                    history[ch][write] = pcm[ch][i];
                    history[ch][write + meter.history_length] = pcm[ch][i];
                    for phase in &meter.phases {
                        let y: f64 = phase.iter().map(|(offset, c)| f64::from(history[ch][write + meter.history_length - offset]) * c).sum();
                        expected_peak = expected_peak.max(y.abs());
                    }
                }
                write = (write + 1) % meter.history_length;
            }
            let mut at = 0;
            for count in [1, 3, 13, 7, 1024, 17].into_iter().cycle() {
                if at == pcm[0].len() { break; }
                let end = (at + count).min(pcm[0].len());
                let chunk: Vec<Vec<f32>> = pcm.iter().map(|ch| ch[at..end].to_vec()).collect();
                meter.push(&chunk);
                for ch in 0..pcm.len() {
                    let expected = meter.history[ch][..meter.history_length].iter().map(|x| x.abs()).fold(0.0_f32, f32::max);
                    assert_eq!(meter.history_peak[ch].to_bits(), expected.to_bits(), "rate={rate} channel={ch} at={end}");
                }
                at = end;
            }
            assert_eq!(meter.peak.to_bits(), expected_peak.to_bits(), "rate={rate}");
            assert_eq!(meter.integrated().true_peak_dbtp.unwrap().to_bits(), (20.0 * expected_peak.log10()).to_bits());
        }
    }

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
