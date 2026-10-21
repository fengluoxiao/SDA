//! Causal evidence for existing delayed/decaying spatial components.
//! Envelope history is control data, not an audio delay or an effect generator.
use crate::convolution::DEFAULT_PARTITION;
const WINDOW: usize = 32;
const LAGS: [usize; 28] = [
    3, 4, 5, 6, 8, 10, 12, 15, 18, 20, 22, 25, 30, 35, 40, 45, 50, 60, 70, 75, 80, 90, 100, 110,
    125, 150, 175, 200,
];
#[derive(Clone)]
pub(crate) struct History {
    values: [f32; 512],
    end: u64,
    count: usize,
    power: f64,
    samples: u32,
    hop: u32,
    pub epoch: u64,
    pub held_power: f64,
    release: f64,
}
impl History {
    fn new(rate: u32, epoch: u64) -> Self {
        Self {
            values: [-100.; 512],
            end: 0,
            count: 0,
            power: 0.,
            samples: 0,
            hop: (rate / 100).max(1),
            epoch,
            held_power: 0.,
            release: (-1. / f64::from(rate.max(1))).exp(),
        }
    }
    fn get(&self, at: u64) -> Option<f32> {
        (self.count > 0 && at <= self.end && self.end - at < self.count as u64)
            .then(|| self.values[at as usize % 512])
    }
    fn observe(&mut self, power: f64, at: u64) {
        let power = if power.is_finite() { power.max(0.) } else { 0. };
        self.held_power = power.max(self.held_power * self.release);
        self.power += power;
        self.samples += 1;
        if (at + 1) % u64::from(self.hop) != 0 {
            return;
        }
        if self.samples == self.hop {
            let time = (at + 1) / u64::from(self.hop);
            self.count = if self.count > 0 && time == self.end + 1 {
                (self.count + 1).min(512)
            } else {
                1
            };
            self.end = time;
            self.values[time as usize % 512] =
                (10. * (self.power / f64::from(self.samples)).max(1e-10).log10()) as f32;
        }
        self.samples = 0;
        self.power = 0.;
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Control<'a> {
    pub history: &'a History,
    pub gains: &'a [f32; DEFAULT_PARTITION],
    pub enabled: bool,
}
#[derive(Clone)]
pub(crate) struct SourceState {
    values: [f32; 64],
    end: u64,
    count: usize,
    energy: f64,
    samples: u32,
    epoch: u64,
    rate: u32,
    last_protected_at: u64,
    protected_seen: bool,
    last_eval: u64,
    last_lag: usize,
    stable_checks: u8,
    target: f32,
    weight: f32,
    fast: f32,
    slow: f32,
    fast_a: f32,
    slow_a: f32,
    up_a: f32,
    down_a: f32,
}
impl Default for SourceState {
    fn default() -> Self {
        Self {
            values: [-100.; 64],
            end: 0,
            count: 0,
            energy: 0.,
            samples: 0,
            epoch: 0,
            rate: 0,
            last_protected_at: 0,
            protected_seen: false,
            last_eval: 0,
            last_lag: 0,
            stable_checks: 0,
            target: 0.,
            weight: 0.,
            fast: 0.,
            slow: 0.,
            fast_a: 0.,
            slow_a: 0.,
            up_a: 0.,
            down_a: 0.,
        }
    }
}
impl SourceState {
    fn get(&self, at: u64) -> Option<f32> {
        (self.count > 0 && at <= self.end && self.end - at < self.count as u64)
            .then(|| self.values[at as usize % 64])
    }
    fn correlation(&self, h: &History, end: u64, lag: usize) -> Option<f32> {
        let (mut x, mut y, mut xx, mut yy, mut xy) = (0f64, 0f64, 0f64, 0f64, 0f64);
        for i in 0..WINDOW {
            let t = end.checked_sub(i as u64)?;
            let a = f64::from(self.get(t)?);
            let b = f64::from(h.get(t.checked_sub(lag as u64)?)?);
            x += a;
            y += b;
            xx += a * a;
            yy += b * b;
            xy += a * b;
        }
        let n = WINDOW as f64;
        let vx = (xx - x * x / n).max(0.);
        let vy = (yy - y * y / n).max(0.);
        // Steady tones, background hiss and silence are not temporal evidence.
        if vx / n < 1.44 || vy / n < 1.44 {
            return None;
        }
        Some(((xy - x * y / n) / (vx * vy).sqrt()).clamp(-1., 1.) as f32)
    }
    pub fn process(
        &mut self,
        sample: f32,
        at: u64,
        index: usize,
        rate: u32,
        h: &History,
        enabled: bool,
        eligible: bool,
        diffuse: f32,
    ) -> f32 {
        if self.epoch != h.epoch || self.rate != rate {
            *self = Self::default();
            self.epoch = h.epoch;
            self.rate = rate;
            self.fast_a = 1. - (-1. / (rate.max(1) as f32 * 0.005)).exp();
            self.slow_a = 1. - (-1. / (rate.max(1) as f32 * 0.080)).exp();
            self.up_a = 1. - (-1. / (rate.max(1) as f32 * 0.15)).exp();
            self.down_a = 1. - (-1. / (rate.max(1) as f32 * 0.02)).exp();
        }
        if !eligible {
            // Inactive object slots may temporarily publish a default front
            // position. Only audible foreground material establishes a move.
            if sample.is_finite() && sample.abs() > 1e-4 {
                self.last_protected_at = at;
                self.protected_seen = true;
            }
            self.target = 0.;
        }
        if (!enabled || !eligible) && self.weight < 1e-8 {
            self.target = 0.;
            return 0.;
        }
        let hop = (rate / 100).max(1);
        let moved_from_front = self.protected_seen
            && at.saturating_sub(self.last_protected_at)
                < (LAGS[LAGS.len() - 1] + WINDOW) as u64 * u64::from(hop);
        if index == 0 && self.count > 0 && at / u64::from(hop) > self.end + 2 {
            self.count = 0;
            self.last_eval = 0;
            self.last_lag = 0;
            self.stable_checks = 0;
            self.target = 0.;
            self.weight = 0.;
        }
        if index == 0 && enabled && eligible && !moved_from_front {
            let end = self.end.min(h.end);
            if end >= self.last_eval + 4 && self.count >= WINDOW {
                self.last_eval = end;
                let zero = self.correlation(h, end, 0).unwrap_or(0.);
                let mut best = (0f32, 0usize);
                for lag in LAGS {
                    if let Some(v) = self.correlation(h, end, lag) {
                        if v > best.0 {
                            best = (v, lag);
                        }
                    }
                }
                let level = self.get(end).unwrap_or(-100.);
                let threshold = 0.85 - 0.05 * diffuse.clamp(0., 1.);
                self.stable_checks = if self.last_lag > 0 && self.last_lag.abs_diff(best.1) <= 2 {
                    self.stable_checks.saturating_add(1)
                } else {
                    1
                };
                let stable = if self.stable_checks >= 3 { 1.0 } else { 0.0 };
                let confidence =
                    if level > -65. && zero < 0.85 && best.0 > threshold && best.0 - zero > 0.15 {
                        ((best.0 - threshold) / (1. - threshold)).clamp(0., 1.)
                            * ((best.0 - zero - 0.15) / 0.35).clamp(0., 1.)
                            * stable
                    } else {
                        0.
                    };
                self.last_lag = best.1;
                self.target = if zero >= 0.85 || level <= -65. {
                    0.
                } else {
                    confidence.max(self.target * 0.85)
                };
            }
        } else if !enabled || !eligible || moved_from_front {
            self.target = 0.;
        }
        let x = if sample.is_finite() { sample } else { 0. };
        let power = (f64::from(x) * f64::from(x)).min(1e10) as f32;
        self.fast += (power - self.fast) * self.fast_a;
        self.slow += (power - self.slow) * self.slow_a;
        let target = if !enabled || !eligible || moved_from_front || self.fast > self.slow * 3. {
            0.
        } else {
            self.target
        };
        let alpha = if target > self.weight {
            self.up_a
        } else {
            self.down_a
        };
        self.weight += (target - self.weight) * alpha;
        if self.weight.abs() < 1e-8 {
            self.weight = 0.;
        }
        self.energy += f64::from(power);
        self.samples += 1;
        if (at + 1) % u64::from(hop) == 0 {
            if self.samples == hop {
                let time = (at + 1) / u64::from(hop);
                self.count = if self.count > 0 && time == self.end + 1 {
                    (self.count + 1).min(64)
                } else {
                    1
                };
                self.end = time;
                self.values[time as usize % 64] =
                    (10. * (self.energy / f64::from(self.samples)).max(1e-10).log10()) as f32;
            }
            self.samples = 0;
            self.energy = 0.;
        }
        self.weight.clamp(0., 1.)
    }
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Observation {
    pub auxiliary_abs: f64,
    pub applied_abs: f64,
    pub candidate_power: f64,
    pub evidence_power: f64,
    pub maximum_extra_db: f64,
}
impl Observation {
    pub fn add(&mut self, x: Self) {
        self.auxiliary_abs += x.auxiliary_abs;
        self.applied_abs += x.applied_abs;
        self.candidate_power += x.candidate_power;
        self.evidence_power += x.evidence_power;
        self.maximum_extra_db = self.maximum_extra_db.max(x.maximum_extra_db);
    }
}
pub(crate) struct Budget {
    pub history: History,
    pub gains: [f32; DEFAULT_PARTITION],
    rate: u32,
    gain: f64,
    auxiliary_abs: f64,
    applied_abs: f64,
    candidate_power: f64,
    evidence_power: f64,
    alpha: f64,
    peak: f64,
    peak_release: f64,
    pub maximum_extra_db: f64,
}
impl Budget {
    pub fn new(rate: u32) -> Self {
        Self {
            history: History::new(rate, 1),
            gains: [1.; DEFAULT_PARTITION],
            rate,
            gain: 1.,
            auxiliary_abs: 0.,
            applied_abs: 0.,
            candidate_power: 0.,
            evidence_power: 0.,
            alpha: 1. - (-1. / (f64::from(rate.max(1)) * 0.08)).exp(),
            peak: 0.,
            peak_release: (-1. / (f64::from(rate.max(1)) * 0.30)).exp(),
            maximum_extra_db: 0.,
        }
    }
    pub fn reset(&mut self) {
        let epoch = self.history.epoch.wrapping_add(1).max(1);
        *self = Self::new(self.rate);
        self.history.epoch = epoch;
    }
    pub fn observe(&mut self, x: Observation, front: f64, at: u64) {
        self.history.observe(front, at);
        self.maximum_extra_db = self.maximum_extra_db.max(x.maximum_extra_db);
        for (a, b) in [
            (&mut self.auxiliary_abs, x.auxiliary_abs),
            (&mut self.applied_abs, x.applied_abs),
            (&mut self.candidate_power, x.candidate_power),
            (&mut self.evidence_power, x.evidence_power),
        ] {
            *a += (b - *a) * self.alpha;
        }
    }
    pub fn observe_output(&mut self, x: [f64; 2]) {
        self.peak = x[0]
            .abs()
            .max(x[1].abs())
            .max(self.peak * self.peak_release);
    }
    pub fn retarget_at(&mut self, index: usize, base: f32) {
        self.gain = if base > 0. {
            f64::from(self.gains[index.min(DEFAULT_PARTITION - 1)] / base)
        } else {
            1.
        };
    }
    pub fn disable_immediately(&mut self) {
        self.gain = 1.;
        self.gains.fill(1.);
    }
    pub fn levels(&self) -> [f64; 4] {
        [
            20. * self.gain.max(1e-12).log10(),
            self.maximum_extra_db,
            self.candidate_power,
            self.evidence_power,
        ]
    }
    pub fn fill(
        &mut self,
        base: &[f32; DEFAULT_PARTITION],
        front: f64,
        auxiliary: f64,
        programme: f64,
        room: Option<f64>,
        enabled: bool,
        offset: usize,
    ) {
        let peak_room =
            crate::true_peak_guard::CEILING_DB - 20. * self.peak.max(1e-12).log10() - 1.;
        let available = room
            .filter(|v| v.is_finite())
            .map(|v| v.min(peak_room) - 0.2);
        let actual = if self.auxiliary_abs > 1e-12 {
            Some(self.applied_abs / self.auxiliary_abs)
        } else {
            None
        };
        let budget = available.zip(actual).and_then(|(h, g)| {
            if h <= 0. || g <= 0. || auxiliary <= 1e-12 {
                return None;
            }
            Some(g + (10f64.powf(h / 20.) - 1.) * (front + auxiliary * g) / auxiliary)
        });
        let quiet = self.candidate_power * programme * programme > 10f64.powf(-65. / 10.);
        let contrast = ((self.history.held_power - 2. * self.candidate_power)
            / (self.history.held_power + 2. * self.candidate_power).max(1e-30))
        .clamp(0., 1.);
        for i in offset..DEFAULT_PARTITION {
            let base = f64::from(base[i]);
            let target = if enabled && quiet && base >= 1. {
                budget.map_or(1., |limit| (1. + contrast).sqrt().min(limit / base).max(1.))
            } else {
                1.
            };
            self.gains[i] = (base * self.gain) as f32;
            let seconds = if target > self.gain { 0.12 } else { 0.05 };
            self.gain = target
                + (self.gain - target) * (-1. / (f64::from(self.rate.max(1)) * seconds)).exp();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn levels(seed: u64, n: usize) -> Vec<f32> {
        let mut s = seed;
        let mut out = vec![];
        let mut level = 0.;
        for i in 0..n {
            if i % 4 == 0 {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                level = 0.03 + 0.25 * ((s >> 32) as u32 as f32 / u32::MAX as f32);
            }
            out.push(level);
        }
        out
    }
    fn evidence(rate: u32, delay: usize, independent: bool, protected: bool) -> (f32, f32) {
        let hop = (rate / 100).max(1);
        let n = 700;
        let fore = levels(13, n);
        let other = levels(291, n);
        let mut h = History::new(rate, 1);
        let mut s = SourceState::default();
        let (mut sum, mut maximum, mut count) = (0., 0f32, 0);
        for at in 0..u64::from(hop) * n as u64 {
            let t = (at / u64::from(hop)) as usize;
            let carrier = (at as f32 * std::f32::consts::TAU * 1500. / rate as f32).sin();
            let front = carrier * fore[t];
            let sample = if independent {
                carrier * other[t] * 0.15
            } else if t >= delay {
                carrier * fore[t - delay] * 0.15
            } else {
                0.
            };
            let w = s.process(
                sample,
                at,
                at as usize % DEFAULT_PARTITION,
                rate,
                &h,
                true,
                !protected,
                0.,
            );
            h.observe(f64::from(front).powi(2), at);
            if t > 300 {
                sum += w;
                maximum = maximum.max(w);
                count += 1;
            }
        }
        (sum / count as f32, maximum)
    }
    #[test]
    fn delayed_material_has_evidence_but_direct_copy_and_foreground_do_not() {
        for rate in [44100, 48000, 96000] {
            let echo = evidence(rate, 25, false, false);
            let dry = evidence(rate, 0, false, false);
            let front = evidence(rate, 25, false, true);
            eprintln!("rate={rate} echo={echo:?} dry={dry:?} front={front:?}");
            assert!(echo.0 > 0.15 && echo.1 > 0.3);
            assert_eq!(dry, (0., 0.));
            assert_eq!(front, (0., 0.));
        }
    }
    #[test]
    fn independent_modulation_is_not_treated_as_a_reliable_delay() {
        let value = evidence(48000, 0, true, false);
        eprintln!("independent={value:?}");
        assert!(value.0 < 0.06 && value.1 < 0.25);
    }
    #[test]
    fn silence_steady_tone_and_missing_history_have_no_evidence() {
        let mut h = History::new(48000, 1);
        let mut a = SourceState::default();
        let mut b = SourceState::default();
        for at in 0..48000 * 4u64 {
            let x = (at as f32 * 0.1).sin() * 0.02;
            assert_eq!(
                a.process(
                    0.,
                    at,
                    at as usize % DEFAULT_PARTITION,
                    48000,
                    &h,
                    true,
                    true,
                    1.
                ),
                0.
            );
            assert_eq!(
                b.process(
                    x,
                    at,
                    at as usize % DEFAULT_PARTITION,
                    48000,
                    &h,
                    true,
                    true,
                    0.
                ),
                0.
            );
            h.observe(f64::from(x).powi(2), at);
        }
    }
    #[test]
    fn shared_budget_needs_evidence_and_reserves_whole_mix_headroom() {
        let base = [1.; DEFAULT_PARTITION];
        let mut b = Budget::new(48000);
        b.history.held_power = 1.;
        b.auxiliary_abs = 0.1;
        b.applied_abs = 0.1;
        b.candidate_power = 0.0001;
        for _ in 0..2000 {
            b.observe_output([0.1, -0.07]);
            b.fill(&base, 1., 0.1, 1., Some(0.6), true, 0);
        }
        let g = f64::from(b.gains[127]);
        let mix = 20. * ((1. + 0.1 * g) / 1.1).log10();
        assert!(
            g > 1.1 && mix <= 0.40001,
            "shared budget exceeded: {g} / {mix}"
        );
        b.fill(&base, 1., 0.1, 1., None, true, 0);
        let before = b.gain;
        for _ in 0..3000 {
            b.fill(&base, 1., 0.1, 1., None, true, 0);
        }
        assert!(b.gain < before && b.gain < 1.00001);
        b.candidate_power = 0.;
        for _ in 0..1000 {
            b.fill(&base, 1., 0.1, 1., Some(20.), true, 0);
        }
        assert!(b.gain < 1.00001);
    }
    #[test]
    fn hot_mix_and_auxiliary_reduction_cannot_invite_detail_gain() {
        let mut b = Budget::new(48000);
        b.history.held_power = 1.;
        b.auxiliary_abs = 1.;
        b.applied_abs = 1.;
        b.candidate_power = 0.001;
        for _ in 0..1000 {
            b.observe_output([1., 0.]);
            b.fill(&[1.; DEFAULT_PARTITION], 1., 1., 1., Some(20.), true, 0);
        }
        assert_eq!(b.gains, [1.; DEFAULT_PARTITION]);
        for _ in 0..1000 {
            b.peak = 0.01;
            b.fill(&[0.5; DEFAULT_PARTITION], 1., 1., 1., Some(20.), true, 0);
        }
        assert!(b.gains.iter().all(|g| *g <= 1.));
    }
    #[test]
    fn reset_epoch_discards_old_delay_evidence() {
        let mut b = Budget::new(48000);
        let mut s = SourceState::default();
        s.epoch = b.history.epoch;
        s.rate = 48000;
        s.target = 1.;
        s.weight = 1.;
        b.reset();
        assert_eq!(s.process(0., 0, 0, 48000, &b.history, true, true, 0.), 0.);
        assert_eq!(s.count, 0);
    }
    #[test]
    fn moving_foreground_is_not_mistaken_for_its_own_echo() {
        let rate = 48000;
        let hop = 480;
        let pattern = levels(13, 100);
        let mut h = History::new(rate, 1);
        let mut s = SourceState::default();
        for at in 0..48000 * 4u64 {
            let t = (at / 480) as usize;
            let x = (at as f32 * 0.1).sin() * pattern[t % 100];
            let front = t < 100;
            let w = s.process(x, at, at as usize % 128, rate, &h, true, !front, 0.);
            h.observe(if front { f64::from(x).powi(2) } else { 0. }, at);
            assert_eq!(w, 0.);
        }
    }
    #[test]
    fn silent_default_front_slots_do_not_hide_later_spatial_echoes() {
        let rate = 48000;
        let mut h = History::new(rate, 1);
        let mut s = SourceState::default();
        let pattern = levels(13, 300);
        let mut maximum = 0f32;
        for at in 0..rate as u64 * 3 {
            let t = (at / 480) as usize;
            let carrier = (at as f32 * std::f32::consts::TAU * 1500. / rate as f32).sin();
            let front = carrier * pattern[t];
            let rear = t >= 100;
            let sample = if rear {
                carrier * pattern[t - 25] * 0.15
            } else {
                0.
            };
            let weight = s.process(sample, at, at as usize % 128, rate, &h, true, rear, 0.);
            h.observe(f64::from(front).powi(2), at);
            if t < 220 {
                maximum = maximum.max(weight);
            }
        }
        assert!(
            maximum > 0.3,
            "silent slot was treated as a moving foreground source: {maximum}"
        );
    }
}
