//! Causal, format-independent final-mix attenuation, before user/program gains.
//! Uses the audition's 3s/400ms detector and 80ms/3s smoothing policy. The
//! gain ramp follows completed 10ms measurements without buffering the audio,
//! preserving the renderer's presentation clock and EOF behavior.
#[derive(Clone, Copy, Default)]
struct Filter {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}
impl Filter {
    fn run(&mut self, x: f64) -> f64 {
        if x.abs() < 1e-30 && self.z.iter().all(|v| v.abs() < 1e-30) {
            self.z = [0.; 2];
            return 0.;
        }
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
    fn clear(&mut self) {
        self.z = [0.; 2]
    }
}
#[derive(Clone, Copy)]
pub(super) struct Policy {
    pub target_lufs: f64,
    pub momentary_margin_db: f64,
    pub attack_seconds: f64,
    pub release_seconds: f64,
    pub silence_lufs: f64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            target_lufs: -20.,
            momentary_margin_db: 4.,
            attack_seconds: 0.08,
            release_seconds: 3.,
            silence_lufs: -60.,
        }
    }
}
pub(super) struct FinalMixAttenuator {
    rate: f64,
    hop: usize,
    policy: Policy,
    filters: [[Filter; 2]; 2],
    offset: usize,
    energy: f64,
    history: [(f64, usize); 300],
    history_write: usize,
    history_count: usize,
    gain_db: f64,
    gain: f64,
    step: f64,
    pub maximum_attenuation_db: f64,
}
impl FinalMixAttenuator {
    pub fn new(rate: u32, policy: Policy) -> Result<Self, &'static str> {
        if rate < 8000 {
            return Err("unsupported sample rate");
        }
        if !policy.target_lufs.is_finite()
            || policy.target_lufs >= 0.
            || !policy.momentary_margin_db.is_finite()
            || !policy.silence_lufs.is_finite()
            || !policy.attack_seconds.is_finite()
            || policy.attack_seconds <= 0.
            || !policy.release_seconds.is_finite()
            || policy.release_seconds <= 0.
        {
            return Err("invalid policy");
        }
        let sr = rate as f64;
        let k = (std::f64::consts::PI * 1681.974450955533 / sr).tan();
        let vh = 10f64.powf(3.999843853973347 / 20.);
        let vb = vh.powf(0.4996667741545416);
        let q = 0.7071752369554196;
        let a = 1. + k / q + k * k;
        let shelf = Filter {
            b: [
                (vh + vb * k / q + k * k) / a,
                2. * (k * k - vh) / a,
                (vh - vb * k / q + k * k) / a,
            ],
            a: [2. * (k * k - 1.) / a, (1. - k / q + k * k) / a],
            ..Default::default()
        };
        let k = (std::f64::consts::PI * 38.13547087602444 / sr).tan();
        let q = 0.5003270373238773;
        let a = 1. + k / q + k * k;
        let high = Filter {
            b: [1., -2., 1.],
            a: [2. * (k * k - 1.) / a, (1. - k / q + k * k) / a],
            ..Default::default()
        };
        Ok(Self {
            rate: sr,
            hop: (sr * 0.01).round().max(1.) as usize,
            policy,
            filters: [[shelf, high]; 2],
            offset: 0,
            energy: 0.,
            history: [(0., 0); 300],
            history_write: 0,
            history_count: 0,
            gain_db: 0.,
            gain: 1.,
            step: 1.,
            maximum_attenuation_db: 0.,
        })
    }
    #[cfg(test)]
    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub fn latency_frames(&self) -> usize {
        0
    }
    pub fn reset(&mut self) {
        for ear in &mut self.filters {
            for f in ear {
                f.clear()
            }
        }
        self.offset = 0;
        self.energy = 0.;
        self.history.fill((0., 0));
        self.history_write = 0;
        self.history_count = 0;
        self.gain_db = 0.;
        self.gain = 1.;
        self.step = 1.;
        self.maximum_attenuation_db = 0.;
    }
    fn update(&mut self) {
        self.history[self.history_write] = (self.energy, self.hop);
        self.history_write = (self.history_write + 1) % 300;
        self.history_count = (self.history_count + 1).min(300);
        let (mut short, mut momentary, mut sn, mut mn) = (0., 0., 0, 0);
        for ago in 0..self.history_count {
            let i = (self.history_write + 299 - ago) % 300;
            let (e, n) = self.history[i];
            short += e;
            sn += n;
            if ago < 40 {
                momentary += e;
                mn += n
            }
        }
        let short = -0.691 + 10. * (short / sn as f64).max(1e-30).log10();
        let momentary = -0.691 + 10. * (momentary / mn as f64).max(1e-30).log10();
        let p = self.policy;
        let mut target = 0f64
            .min(p.target_lufs - short)
            .min(p.target_lufs + p.momentary_margin_db - momentary);
        if short < p.silence_lufs {
            target = self.gain_db
        }
        let tau = if target < self.gain_db {
            p.attack_seconds
        } else {
            p.release_seconds
        };
        let next = target + (-(self.hop as f64) / self.rate / tau).exp() * (self.gain_db - target);
        self.gain = 10f64.powf(self.gain_db / 20.);
        self.step = 10f64.powf((next - self.gain_db) / (self.hop as f64 * 20.));
        self.gain_db = next;
        self.maximum_attenuation_db = self.maximum_attenuation_db.max(-next);
        self.energy = 0.;
    }
    pub fn process(&mut self, frame: [f64; 2]) -> [f64; 2] {
        let frame = frame.map(|v| if v.is_finite() { v } else { 0. });
        let out = frame.map(|v| v * self.gain.min(1.));
        self.gain *= self.step;
        for (ear, x) in frame.into_iter().enumerate() {
            let y = self.filters[ear][0].run(x);
            let y = self.filters[ear][1].run(y);
            self.energy += y * y
        }
        self.offset += 1;
        if self.offset == self.hop {
            self.update();
            self.offset = 0
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quiet_exact_no_latency_and_all_rates_share_gain() {
        for rate in [44100, 48000, 96000, 192000, 384000] {
            let mut n = FinalMixAttenuator::new(rate, Policy::default()).unwrap();
            assert_eq!(n.latency_frames(), 0);
            for i in 0..rate {
                let x = (i as f64 * 0.07).sin() * 0.001;
                assert_eq!(n.process([x, -x]), [x, -x]);
            }
            for i in 0..rate * 3 {
                let x = (i as f64 * 0.13).sin() * 2.;
                let y = n.process([x, -0.7 * x]);
                assert!((y[1] + 0.7 * y[0]).abs() < 1e-12);
                assert!(y[0].abs() <= x.abs() + 1e-12);
            }
            assert!(n.maximum_attenuation_db > 5.);
            n.reset();
            assert_eq!(n.process([0.0001, -0.00005]), [0.0001, -0.00005]);
        }
    }
    #[test]
    fn chunk_boundaries_do_not_change_prefix_or_tail() {
        let mut a = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        let mut b = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        let input: Vec<_> = (0..43217)
            .map(|i| [(i as f64 * 0.33).sin() * 1.2, -0.31])
            .collect();
        let expected: Vec<_> = input.iter().map(|x| a.process(*x)).collect();
        let mut actual = Vec::new();
        for block in input.chunks(997) {
            actual.extend(block.iter().map(|x| b.process(*x)));
        }
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), input.len());
    }
    #[test]
    fn silence_flushes_iir_state_below_audible_precision() {
        let mut n = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        n.process([1., -1.]);
        for _ in 0..48000 {
            n.process([0., 0.]);
        }
        for ear in n.filters {
            for filter in ear {
                assert_eq!(filter.z, [0., 0.]);
            }
        }
    }
    #[test]
    fn reset_and_nonfinite_input() {
        let mut a = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        for _ in 0..48000 {
            a.process([2., -1.]);
        }
        a.reset();
        let mut fresh = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        for i in 0..4000 {
            let x = [(i as f64 * 0.12).sin() * 0.01, 0.];
            assert_eq!(a.process(x), fresh.process(x));
        }
        a.reset();
        assert_eq!(a.process([f64::NAN, f64::INFINITY]), [0., 0.]);
    }
}
