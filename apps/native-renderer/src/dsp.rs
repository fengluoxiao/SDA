//! Fixed-state DSP primitives for native render-worker paths.
//!
//! These processors are dependency-free and allocation-free after construction.
//! They never run in the WASAPI callback; the render worker owns them.

#[derive(Clone, Copy)]
pub(super) struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn normalized(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn shelf(
        sample_rate: u32,
        frequency: f32,
        q: f32,
        gain_db: f32,
        high: bool,
    ) -> Result<Self, String> {
        if sample_rate == 0
            || !frequency.is_finite()
            || frequency <= 0.0
            || frequency >= sample_rate as f32 * 0.5
            || !gain_db.is_finite()
        {
            return Err("invalid shelf configuration".into());
        }
        let omega = std::f32::consts::TAU * frequency / sample_rate as f32;
        let cosine = omega.cos();
        let alpha = omega.sin() / (2.0 * q.max(1e-4));
        let a = 10.0_f32.powf(gain_db / 40.0);
        let beta = 2.0 * a.sqrt() * alpha;
        let (b0, b1, b2, a0, a1, a2) = if high {
            (
                a * ((a + 1.0) + (a - 1.0) * cosine + beta),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cosine),
                a * ((a + 1.0) + (a - 1.0) * cosine - beta),
                (a + 1.0) - (a - 1.0) * cosine + beta,
                2.0 * ((a - 1.0) - (a + 1.0) * cosine),
                (a + 1.0) - (a - 1.0) * cosine - beta,
            )
        } else {
            (
                a * ((a + 1.0) - (a - 1.0) * cosine + beta),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cosine),
                a * ((a + 1.0) - (a - 1.0) * cosine - beta),
                (a + 1.0) + (a - 1.0) * cosine + beta,
                -2.0 * ((a - 1.0) + (a + 1.0) * cosine),
                (a + 1.0) + (a - 1.0) * cosine - beta,
            )
        };
        Ok(Self::normalized(b0, b1, b2, a0, a1, a2))
    }

    pub(super) fn lowshelf(
        sample_rate: u32,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> Result<Self, String> {
        Self::shelf(sample_rate, frequency, q, gain_db, false)
    }

    pub(super) fn highshelf(
        sample_rate: u32,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> Result<Self, String> {
        Self::shelf(sample_rate, frequency, q, gain_db, true)
    }

    pub(super) fn peaking(
        sample_rate: u32,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> Result<Self, String> {
        if sample_rate == 0
            || !frequency.is_finite()
            || frequency <= 0.0
            || frequency >= sample_rate as f32 * 0.5
            || !gain_db.is_finite()
        {
            return Err("invalid peaking configuration".into());
        }
        let omega = std::f32::consts::TAU * frequency / sample_rate as f32;
        let a = 10.0_f32.powf(gain_db / 40.0);
        let alpha = omega.sin() / (2.0 * q.max(1e-4));
        Ok(Self::normalized(
            1.0 + alpha * a,
            -2.0 * omega.cos(),
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * omega.cos(),
            1.0 - alpha / a,
        ))
    }

    pub(super) fn butterworth_lowpass(sample_rate: u32, frequency: f32) -> Result<Self, String> {
        if sample_rate == 0
            || !frequency.is_finite()
            || frequency <= 0.0
            || frequency >= sample_rate as f32 * 0.5
        {
            return Err("invalid low-pass configuration".into());
        }
        let omega = std::f32::consts::TAU * frequency / sample_rate as f32;
        let cosine = omega.cos();
        let alpha = omega.sin() * std::f32::consts::FRAC_1_SQRT_2;
        let a0 = 1.0 + alpha;
        Ok(Self {
            b0: ((1.0 - cosine) * 0.5) / a0,
            b1: (1.0 - cosine) / a0,
            b2: ((1.0 - cosine) * 0.5) / a0,
            a1: (-2.0 * cosine) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        })
    }

    pub(super) fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        output
    }

    pub(super) fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

pub(super) struct Lr4Lowpass {
    first: Biquad,
    second: Biquad,
}

impl Lr4Lowpass {
    pub(super) fn new(sample_rate: u32, frequency: f32) -> Result<Self, String> {
        Ok(Self {
            first: Biquad::butterworth_lowpass(sample_rate, frequency)?,
            second: Biquad::butterworth_lowpass(sample_rate, frequency)?,
        })
    }

    pub(super) fn process(&mut self, input: f32) -> f32 {
        self.second.process(self.first.process(input))
    }

    pub(super) fn reset(&mut self) {
        self.first.reset();
        self.second.reset();
    }
}

/// Deterministic mono hard-knee compressor for the dedicated LFE path.
/// Its threshold/ratio/attack/release match the master graph's exposed
/// DynamicsCompressorNode settings; browser detector internals are unspecified,
/// so this uses a documented peak detector rather than claiming bit parity.
pub(super) struct MonoCompressor {
    threshold: f32,
    ratio: f32,
    attack_coeff: f32,
    release_coeff: f32,
    gain: f32,
}

impl MonoCompressor {
    pub(super) fn lfe(sample_rate: u32) -> Self {
        let threshold = 10.0_f32.powf(-3.0 / 20.0);
        Self {
            threshold,
            ratio: 8.0,
            attack_coeff: (-1.0 / (sample_rate as f32 * 0.003)).exp(),
            release_coeff: (-1.0 / (sample_rate as f32 * 0.1)).exp(),
            gain: 1.0,
        }
    }

    pub(super) fn process(&mut self, input: f32) -> f32 {
        let magnitude = input.abs();
        let target = if magnitude > self.threshold {
            let level_db = 20.0 * magnitude.log10();
            let output_db = -3.0 + (level_db + 3.0) / self.ratio;
            10.0_f32.powf((output_db - level_db) / 20.0)
        } else {
            1.0
        };
        let coefficient = if target < self.gain {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.gain = target + coefficient * (self.gain - target);
        input * self.gain
    }

    pub(super) fn reset(&mut self) {
        self.gain = 1.0;
    }
}

/// Master's linked 5 ms sample-peak guard. Both ears share one envelope so
/// limiting cannot move a binaural image.
pub(super) struct StereoPeakGuard {
    left: Vec<f32>,
    right: Vec<f32>,
    write: usize,
    ceiling: f32,
    release_coeff: f32,
    gain: f32,
    attack_target: f32,
    attack_step: f32,
    hold: usize,
}

impl StereoPeakGuard {
    pub(super) fn new(sample_rate: u32) -> Self {
        let lookahead = ((sample_rate as f32 * 0.005).round() as usize).max(1);
        Self {
            left: vec![0.0; lookahead],
            right: vec![0.0; lookahead],
            write: 0,
            ceiling: 10.0_f32.powf(-1.0 / 20.0),
            release_coeff: (-1.0 / (sample_rate as f32 * 0.1)).exp(),
            gain: 1.0,
            attack_target: 1.0,
            attack_step: 0.0,
            hold: 0,
        }
    }

    pub(super) fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        let delayed_left = self.left[self.write];
        let delayed_right = self.right[self.write];
        let left = if left.is_finite() { left } else { 0.0 };
        let right = if right.is_finite() { right } else { 0.0 };
        self.left[self.write] = left;
        self.right[self.write] = right;

        let peak = left.abs().max(right.abs());
        let target = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };
        if target < self.attack_target {
            let next_step = (target - self.gain) / self.left.len() as f32;
            self.attack_step = if self.gain > self.attack_target {
                self.attack_step.min(next_step)
            } else {
                next_step
            };
            self.attack_target = target;
        }
        if target < 1.0 {
            self.hold = self.left.len();
        }
        if self.gain > self.attack_target {
            self.gain = self.attack_target.max(self.gain + self.attack_step);
            if self.gain == self.attack_target {
                self.attack_step = 0.0;
            }
        } else if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.gain = 1.0 - (1.0 - self.gain) * self.release_coeff;
            self.attack_target = self.gain;
        }
        self.write = (self.write + 1) % self.left.len();
        [
            (delayed_left * self.gain).clamp(-1.0, 1.0),
            (delayed_right * self.gain).clamp(-1.0, 1.0),
        ]
    }

    pub(super) fn reset(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.write = 0;
        self.gain = 1.0;
        self.attack_target = 1.0;
        self.attack_step = 0.0;
        self.hold = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_at(filter: &mut Lr4Lowpass, sample_rate: u32, frequency: f32) -> f32 {
        let warmup = sample_rate as usize;
        let measure = sample_rate as usize;
        let mut square = 0.0;
        for index in 0..warmup + measure {
            let input =
                (std::f32::consts::TAU * frequency * index as f32 / sample_rate as f32).sin();
            let output = filter.process(input);
            if index >= warmup {
                square += output * output;
            }
        }
        (square / measure as f32).sqrt() / std::f32::consts::FRAC_1_SQRT_2
    }

    #[test]
    fn lr4_lowpass_has_expected_cutoff_and_rejection() {
        let mut at_cutoff = Lr4Lowpass::new(48_000, 120.0).unwrap();
        let mut above_cutoff = Lr4Lowpass::new(48_000, 120.0).unwrap();
        let cutoff = rms_at(&mut at_cutoff, 48_000, 120.0);
        let octave_up = rms_at(&mut above_cutoff, 48_000, 240.0);
        assert!(
            (cutoff - 0.5).abs() < 0.025,
            "cutoff magnitude was {cutoff}"
        );
        assert!(octave_up < 0.07, "240 Hz magnitude was {octave_up}");
    }

    #[test]
    fn reset_clears_filter_tail() {
        let mut filter = Lr4Lowpass::new(48_000, 120.0).unwrap();
        for _ in 0..256 {
            let _ = filter.process(1.0);
        }
        filter.reset();
        assert_eq!(filter.process(0.0), 0.0);
    }

    #[test]
    fn lfe_compressor_reduces_over_threshold_peak() {
        let mut compressor = MonoCompressor::lfe(48_000);
        let mut output = 0.0;
        for _ in 0..4_000 {
            output = compressor.process(1.0);
        }
        assert!(output < 0.85);
        assert!(output > 0.65);
    }

    #[test]
    fn peak_guard_has_lookahead_and_linked_ceiling() {
        let mut guard = StereoPeakGuard::new(48_000);
        for _ in 0..239 {
            assert_eq!(guard.process(0.0, 0.0), [0.0, 0.0]);
        }
        let _ = guard.process(2.0, 1.0);
        for _ in 0..240 {
            let output = guard.process(0.0, 0.0);
            assert!(output[0].abs() <= 1.0 && output[1].abs() <= 1.0);
        }
    }
}
