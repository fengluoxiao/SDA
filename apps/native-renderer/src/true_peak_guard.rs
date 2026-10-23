//! Linked lookahead protection driven by inter-sample estimates, not only samples.
//! Only the detector interpolates; quiet audio is an exact delayed copy.
pub(super) const CEILING_DB: f64 = -1.2;
pub(super) const DETECTOR_TAPS: usize = 193;
const HISTORY: usize = 97;
pub(super) struct TruePeakGuard {
    buffer: Vec<[f32; 2]>,
    cursor: usize,
    lookahead: usize,
    history: [[f64; HISTORY * 2]; 2],
    history_write: usize,
    coefficient_phases: [[f64; 4]; HISTORY],
    #[cfg(test)]
    scalar_detector: bool,
    factor: usize,
    taps: usize,
    gain: f64,
    target: f64,
    step: f64,
    hold: usize,
    release: f64,
    ceiling: f64,
}
impl TruePeakGuard {
    pub fn new(rate: u32) -> Self {
        let factor = if rate < 96000 {
            4
        } else if rate < 192000 {
            2
        } else {
            1
        };
        let taps = if factor == 1 {
            1
        } else {
            (DETECTOR_TAPS + factor - 1) / factor
        };
        let delay = if factor == 1 {
            0
        } else {
            ((DETECTOR_TAPS - 1) / 2) / factor
        };
        let lookahead = ((rate as f64 * 0.005).round() as usize).max(1);
        let mut coefficients = [[0.; HISTORY]; 4];
        if factor == 1 {
            coefficients[0][0] = 1.;
        } else {
            for tap in 0..DETECTOR_TAPS {
                let x = (tap as f64 - ((DETECTOR_TAPS - 1) / 2) as f64) / factor as f64;
                let sinc = if x == 0. {
                    1.
                } else {
                    (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                };
                let window = 0.5
                    * (1.
                        - (2. * std::f64::consts::PI * tap as f64 / (DETECTOR_TAPS - 1) as f64)
                            .cos());
                coefficients[tap % factor][tap / factor] = sinc * window;
            }
        }
        let coefficient_phases = std::array::from_fn(|tap| std::array::from_fn(|phase| coefficients[phase][tap]));
        Self {
            buffer: vec![[0.; 2]; lookahead],
            cursor: 0,
            lookahead: lookahead.saturating_sub(delay).max(1),
            history: [[0.; HISTORY * 2]; 2],
            history_write: 0,
            coefficient_phases,
            #[cfg(test)]
            scalar_detector: false,
            factor,
            taps,
            gain: 1.,
            target: 1.,
            step: 0.,
            hold: 0,
            release: (-1. / (rate.max(1) as f64 * 0.08)).exp(),
            ceiling: 10f64.powf(CEILING_DB / 20.),
        }
    }
    pub fn latency_frames(&self) -> usize {
        self.buffer.len()
    }
    #[cfg(test)]
    pub fn diagnostic_gain(&self) -> f32 {
        self.gain as f32
    }
    pub fn reset(&mut self) {
        self.buffer.fill([0.; 2]);
        self.cursor = 0;
        self.history.fill([0.; HISTORY * 2]);
        self.history_write = 0;
        self.gain = 1.;
        self.target = 1.;
        self.step = 0.;
        self.hold = 0;
    }
    pub fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        let input = [left, right].map(|v| if v.is_finite() { v } else { 0. });
        let delayed = self.buffer[self.cursor];
        self.buffer[self.cursor] = input;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        let mut peak = 0f64;
        for ear in 0..2 {
            self.history[ear][self.history_write] = input[ear] as f64;
            self.history[ear][self.history_write + self.taps] = input[ear] as f64;
            let history = &self.history[ear][self.history_write+1..self.history_write+self.taps+1];
            let coefficients = &self.coefficient_phases[..self.taps];
            #[cfg(test)]
            let p = if self.scalar_detector {
                crate::true_peak_detector::scalar(history, coefficients, self.factor)
            } else { crate::true_peak_detector::peak(history, coefficients, self.factor) };
            #[cfg(not(test))]
            let p = crate::true_peak_detector::peak(history, coefficients, self.factor);
            peak = peak.max(p);
        }
        self.history_write = (self.history_write + 1) % self.taps;
        let requested = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.
        };
        if requested < self.target {
            let next = (requested - self.gain) / self.lookahead as f64;
            self.step = if self.gain > self.target {
                self.step.min(next)
            } else {
                next
            };
            self.target = requested;
        }
        if requested < 1. {
            self.hold = self.lookahead + self.taps;
        }
        if self.gain > self.target {
            self.gain = self.target.max(self.gain + self.step);
        } else if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.gain = 1. - (1. - self.gain) * self.release;
            self.target = self.gain;
        }
        // A shared emergency bound also covers an unexpectedly large native sample.
        let magnitude = delayed[0].abs().max(delayed[1].abs()) as f64;
        if magnitude * self.gain > self.ceiling {
            self.gain = self.ceiling / magnitude;
            self.target = self.target.min(self.gain);
            self.hold = self.lookahead + self.taps;
        }
        delayed.map(|v| (v as f64 * self.gain) as f32)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vector_detector_preserves_every_pcm_sample_and_limiter_envelope() {
        for rate in [44100,48000,96000,192000,384000] {
            let mut a=TruePeakGuard::new(rate);let mut b=TruePeakGuard::new(rate);b.scalar_detector=true;
            let mut seed=23u64;
            for i in 0..24000 {
                seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let x=if i%4000>3000 {0.} else {((seed>>32)as u32 as f32/u32::MAX as f32*2.-1.)*1.7};
                let right=if i%997==0{f32::NAN}else{-x*0.37};
                assert_eq!(a.process(x,right).map(f32::to_bits),b.process(x,right).map(f32::to_bits),"rate={rate} frame={i}");
                assert_eq!(a.gain.to_bits(),b.gain.to_bits());
                assert_eq!(a.target.to_bits(),b.target.to_bits());
            }
        }
    }
    #[test]
    fn quiet_audio_is_exact_delayed_audio_and_reset_clears_every_delay() {
        for sr in [44100, 48000, 96000, 192000, 384000] {
            let mut g = TruePeakGuard::new(sr);
            let d = g.latency_frames();
            for i in 0..sr as usize {
                let x = (i as f32 * 0.07).sin() * 0.05;
                let y = g.process(x, -x * 0.3);
                if i >= d {
                    let expected = ((i - d) as f32 * 0.07).sin() * 0.05;
                    assert_eq!(y, [expected, -expected * 0.3]);
                }
            }
            g.reset();
            for _ in 0..d * 2 {
                assert_eq!(g.process(0., 0.), [0., 0.]);
            }
        }
    }
    #[test]
    fn inter_sample_peak_triggers_when_native_samples_are_below_ceiling() {
        let mut g = TruePeakGuard::new(48000);
        let mut minimum = 1f32;
        for i in 0..48000 {
            let x =
                (std::f64::consts::FRAC_PI_2 * i as f64 + std::f64::consts::FRAC_PI_4).sin() * 1.1;
            assert!(x.abs() < g.ceiling);
            let y = g.process(x as f32, -0.3 * x as f32);
            minimum = minimum.min(g.diagnostic_gain());
            assert!((y[1] + y[0] * 0.3).abs() < 1e-7);
        }
        assert!(
            minimum < 0.85,
            "inter-sample protection did not react: {minimum}"
        );
    }
    #[test]
    fn broadband_transients_stay_below_true_peak_ceiling() {
        let mut guard = TruePeakGuard::new(48000);
        let mut seed = 615u64;
        let mut out = Vec::new();
        for _ in 0..24000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let x = (((seed >> 32) as u32 as f64) / u32::MAX as f64 * 2. - 1.) * 1.7;
            out.push(guard.process(x as f32, 0.)[0] as f64);
        }
        for _ in 0..512 {
            out.push(guard.process(0., 0.)[0] as f64);
        }
        let mut coeff = [[0.; 65]; 4];
        for phase in 0..4 {
            for tap in 0..65 {
                let k = tap as f64 - 32.;
                let x = phase as f64 / 4. - k;
                let sinc = if x == 0. {
                    1.
                } else {
                    (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                };
                let w = 0.42 - 0.5 * (2. * std::f64::consts::PI * tap as f64 / 64.).cos()
                    + 0.08 * (4. * std::f64::consts::PI * tap as f64 / 64.).cos();
                coeff[phase][tap] = sinc * w;
            }
        }
        let mut peak = 0f64;
        for center in 32..out.len() - 32 {
            for phase in 0..4 {
                let mut v = 0.;
                for tap in 0..65 {
                    v += out[center + tap - 32] * coeff[phase][tap];
                }
                peak = peak.max(v.abs());
            }
        }
        assert!(
            20. * peak.log10() <= -1.,
            "broadband reconstructed peak {} dBTP",
            20. * peak.log10()
        );
    }
    #[test]
    fn both_ears_share_peak_bound_and_nonfinite_input_is_silent() {
        let mut g = TruePeakGuard::new(48000);
        for i in 0..48000 {
            let x = if i % 997 == 0 {
                9.
            } else {
                (i as f32 * 0.71).sin() * 1.8
            };
            let y = g.process(x, -0.25 * x);
            assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 0.871));
            assert!((y[1] + y[0] * 0.25).abs() < 1e-7);
        }
        g.reset();
        for _ in 0..1000 {
            assert_eq!(g.process(f32::NAN, f32::INFINITY), [0., 0.]);
        }
    }
}
