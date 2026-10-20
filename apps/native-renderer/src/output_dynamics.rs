//! Linked, format-independent final-output attenuation. Applied after programme,
//! manual and spatial gains, before the existing true-peak guard. Source-group
//! control, not this common gain, is responsible for foreground/auxiliary balance.
//! -20 LUFS detector target provides engineering margin for the app's requested
//! <= -18 LUFS integrated output ceiling; whole-track output is verified separately.
//! This is an application policy, not a Dolby loudness-reference renderer.
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
// Fixed-size 0.1 dB energy histogram: controller approximation to the
// BS.1770 absolute (-70 LUFS) and relative (-10 LU) gates. Exact energy sums
// are retained per bin; only a relative-gate boundary inside a bin is approximate.
// Final compliance measurements use an independent complete-output meter.
struct IntegratedReference {
    energy: Box<[f64]>,
    count: Box<[u64]>,
}
impl IntegratedReference {
    fn new() -> Self {
        Self {
            energy: vec![0.; 1024].into_boxed_slice(),
            count: vec![0; 1024].into_boxed_slice(),
        }
    }
    fn clear(&mut self) {
        self.energy.fill(0.);
        self.count.fill(0);
    }
    fn push(&mut self, power: f64) {
        let level = -0.691 + 10. * power.max(1e-30).log10();
        if !power.is_finite() || level <= -70. {
            return;
        }
        let i = (((level + 70.) * 10.).floor() as usize).min(self.energy.len() - 1);
        self.energy[i] += power;
        self.count[i] += 1;
    }
    fn loudness(&self) -> Option<f64> {
        let n: u64 = self.count.iter().sum();
        if n == 0 {
            return None;
        }
        let threshold = self.energy.iter().sum::<f64>() / n as f64 * 0.1;
        let (mut energy, mut count) = (0., 0u64);
        for (&e, &n) in self.energy.iter().zip(self.count.iter()) {
            if n > 0 && e / n as f64 > threshold {
                energy += e;
                count += n;
            }
        }
        (count > 0).then(|| -0.691 + 10. * (energy / count as f64).max(1e-30).log10())
    }
}
pub(super) struct FinalMixAttenuator {
    rate: f64,
    hop: usize,
    policy: Policy,
    enabled: bool,
    requested_attenuation_db: f64,
    reference_gain: f64,
    output_makeup_db: f64,
    output_makeup_gain: f64,
    available_headroom_db: Option<f64>,
    filters: [[Filter; 2]; 2],
    offset: usize,
    energy: f64,
    history: [(f64, usize); 300],
    history_write: usize,
    history_count: usize,
    hops_seen: u64,
    integrated: IntegratedReference,
    integrated_lufs: Option<f64>,
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
            enabled: true,
            requested_attenuation_db: 0.,
            reference_gain: 1.,
            output_makeup_db: 0.,
            output_makeup_gain: 1.,
            available_headroom_db: None,
            filters: [[shelf, high]; 2],
            offset: 0,
            energy: 0.,
            history: [(0., 0); 300],
            history_write: 0,
            history_count: 0,
            hops_seen: 0,
            integrated: IntegratedReference::new(),
            integrated_lufs: None,
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
        self.hops_seen = 0;
        self.integrated.clear();
        self.integrated_lufs = None;
        self.gain_db = 0.;
        self.gain = 1.;
        self.step = 1.;
        self.maximum_attenuation_db = 0.;
        self.requested_attenuation_db = 0.;
        self.reference_gain = 1.;
        self.output_makeup_db = 0.;
        self.output_makeup_gain = 1.;
        self.available_headroom_db = None;
    }
    pub fn set_enabled(&mut self, enabled: bool, immediate: bool) {
        if self.enabled != enabled {
            // Plain bypass / another spatial state is not a reference for the
            // newly enabled output. Reset detectors without stepping audio gain.
            let gain = self.gain;
            let maximum = self.maximum_attenuation_db;
            self.reset();
            self.enabled = enabled;
            self.gain = gain;
            self.gain_db = 20. * gain.max(1e-30).log10();
            self.maximum_attenuation_db = maximum;
        }
        if !enabled && immediate {
            self.gain_db = 0.;
            self.gain = 1.;
            self.step = 1.;
        }
    }
    pub fn requested_reduction_db(&self) -> f64 {
        if self.enabled {
            self.requested_attenuation_db
        } else {
            0.
        }
    }
    pub fn available_headroom_db(&self) -> Option<f64> {
        if self.enabled {
            self.available_headroom_db
        } else {
            None
        }
    }
    #[cfg(test)]
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    #[cfg(test)]
    pub fn gain_db(&self) -> f64 {
        20. * self.gain.max(1e-30).log10()
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
        self.hops_seen = self.hops_seen.saturating_add(1);
        if self.history_count >= 40 && self.hops_seen % 10 == 0 {
            self.integrated.push(momentary / mn as f64);
            self.integrated_lufs = self.integrated.loudness();
        }
        let reference_db = 20. * self.reference_gain.max(1e-30).log10();
        let short = -0.691 + 10. * (short / sn as f64).max(1e-30).log10() + reference_db;
        let momentary = -0.691 + 10. * (momentary / mn as f64).max(1e-30).log10() + reference_db;
        let p = self.policy;
        // A requested final makeup belongs after the nominal loudness target.
        // Let the detector see it too, retaining at least 1 LU of margin to
        // the app's -18 LUFS acceptance ceiling. Zero makeup keeps the old policy.
        let target_lufs = self.detector_target_lufs();
        // Preserve musical dynamics: long-term control follows the accumulated
        // gated programme rather than continually normalising every 3 seconds.
        let integrated = self.integrated_lufs.map_or(short, |v| v + reference_db);
        let mut target = 0f64
            .min(target_lufs - integrated)
            .min(target_lufs + p.momentary_margin_db - momentary);
        self.available_headroom_db = if self.history_count >= 40
            && self.reference_gain > 0.
            && short >= p.silence_lufs
            && short.is_finite()
            && momentary.is_finite()
        {
            // The audio already carries the linked attenuation envelope.
            // Testing the pre-attenuation mix here double-counts its excess
            // and prevents quiet auxiliary content from using real headroom.
            let applied_db = 20. * self.gain.min(1.).max(1e-30).log10();
            Some((target_lufs - short - applied_db)
                .min(target_lufs + p.momentary_margin_db - momentary - applied_db))
        } else {
            None
        };
        self.requested_attenuation_db = if short < p.silence_lufs { 0. } else { -target };
        if short < p.silence_lufs {
            target = self.gain_db;
        }
        if !self.enabled {
            target = 0.;
        }
        let tau = if !self.enabled {
            0.05
        } else if target < self.gain_db {
            if self.hops_seen < 40 {
                p.attack_seconds.min(0.005)
            } else {
                p.attack_seconds
            }
        } else {
            p.release_seconds
        };
        let next = target + (-(self.hop as f64) / self.rate / tau).exp() * (self.gain_db - target);
        let next = if !self.enabled && next.abs() < 1e-7 {
            0.
        } else {
            next
        };
        self.gain = 10f64.powf(self.gain_db / 20.);
        self.step = 10f64.powf((next - self.gain_db) / (self.hop as f64 * 20.));
        self.gain_db = next;
        self.maximum_attenuation_db = self.maximum_attenuation_db.max(-next);
        self.energy = 0.;
    }
    #[cfg(test)]
    pub fn process(&mut self, frame: [f64; 2]) -> [f64; 2] {
        self.process_scaled(frame, 1.)
    }
    pub fn detector_target_lufs(&self) -> f64 {
        if self.output_makeup_db > 0. {
            (self.policy.target_lufs + self.output_makeup_db).min(-19.0)
        } else { self.policy.target_lufs }
    }
    pub fn process_scaled(&mut self, frame: [f64; 2], reference_gain: f64) -> [f64; 2] {
        self.process_scaled_with_makeup(frame, reference_gain, 1.)
    }
    pub fn process_scaled_with_makeup(&mut self, frame: [f64; 2], reference_gain: f64, makeup_gain: f64) -> [f64; 2] {
        const MAX_MAKEUP_GAIN: f64 = 1.1885022274370185; // +1.5 dB
        let makeup_gain = if makeup_gain.is_finite() { makeup_gain.clamp(1., MAX_MAKEUP_GAIN) } else { 1. };
        if makeup_gain != self.output_makeup_gain {
            self.output_makeup_gain = makeup_gain;
            self.output_makeup_db = 20. * makeup_gain.log10();
        }
        let reference_gain = reference_gain * makeup_gain;
        self.reference_gain = if reference_gain.is_finite() {
            reference_gain.max(0.)
        } else {
            0.
        };
        let frame = frame.map(|v| if v.is_finite() { v } else { 0. });
        let out = frame.map(|v| v * self.reference_gain * self.gain.min(1.));
        self.gain *= self.step;
        if self.enabled {
            for (ear, x) in frame.into_iter().enumerate() {
                let y = self.filters[ear][0].run(x);
                let y = self.filters[ear][1].run(y);
                self.energy += y * y;
            }
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

#[cfg(test)]
mod integrated_tests {
    use super::*;
    fn power(lufs: f64) -> f64 {
        10f64.powf((lufs + 0.691) / 10.)
    }
    #[test]
    fn histogram_gates_silence_and_does_not_flatten_programme_sections() {
        let mut m = IntegratedReference::new();
        for _ in 0..100 {
            m.push(power(-90.));
        }
        assert_eq!(m.loudness(), None);
        for _ in 0..100 {
            m.push(power(-25.));
        }
        for _ in 0..100 {
            m.push(power(-15.));
        }
        let expected = -0.691 + 10. * ((power(-25.) + power(-15.)) / 2.).log10();
        assert!((m.loudness().unwrap() - expected).abs() < 1e-10);
        // The integrated result retains both sections; it is not the latest block.
        assert!(m.loudness().unwrap() < -17. && m.loudness().unwrap() > -18.);
        for _ in 0..100 {
            m.push(power(-60.));
        }
        assert!((m.loudness().unwrap() - expected).abs() < 1e-10);
        m.clear();
        assert_eq!(m.loudness(), None);
    }
    #[test]
    fn current_programme_gain_rescales_the_reference_without_old_gain_history() {
        let mut a = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        for i in 0..48000 {
            let x = (i as f64 * 0.13).sin() * 0.3;
            a.process_scaled([x, -x], 1.);
        }
        assert!(a.requested_reduction_db() > 0.);
        for i in 0..480 {
            let x = (i as f64 * 0.13).sin() * 0.3;
            a.process_scaled([x, -x], 0.01);
        }
        assert_eq!(a.requested_reduction_db(), 0.);
        assert!(a.available_headroom_db().unwrap() > 10.);
    }
}

#[cfg(test)]
mod mode_transition_tests {
    use super::*;
    #[test]
    fn bypass_history_cannot_duck_a_new_quiet_spatial_output() {
        let mut n = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        n.set_enabled(false, true);
        for i in 0..48000 * 8 {
            let x = (i as f64 * 0.13).sin() * 0.8;
            assert_eq!(n.process([x, -x]), [x, -x]);
        }
        n.set_enabled(true, false);
        assert_eq!(n.history_count, 0);
        assert_eq!(n.integrated_lufs, None);
        assert_eq!(n.available_headroom_db(), None);
        for i in 0..48000 {
            let x = (i as f64 * 0.13).sin() * 0.001;
            assert_eq!(n.process([x, -x]), [x, -x]);
        }
        assert_eq!(n.maximum_attenuation_db, 0.);
    }
    #[test]
    fn cold_activation_is_identical_and_mode_changes_keep_gain_continuous() {
        let mut a = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        let mut b = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        b.set_enabled(false, true);
        b.set_enabled(true, false);
        for i in 0..48000 * 4 {
            let x = (i as f64 * 0.13).sin() * 0.3;
            assert_eq!(a.process([x, -x * 0.7]), b.process([x, -x * 0.7]));
        }
        let gain = a.gain;
        a.set_enabled(false, false);
        assert!((a.process([0.1, -0.07])[0] - 0.1 * gain).abs() < 1e-12);
        assert_eq!(a.available_headroom_db(), None);
    }
}

#[cfg(test)]
mod controlled_headroom_tests {
    use super::*;
    #[test]
    fn measured_headroom_includes_the_already_applied_output_attenuation() {
        let mut n = FinalMixAttenuator::new(48000, Policy::default()).unwrap();
        for i in 0..48000 * 5 {
            let x = (i as f64 * std::f64::consts::TAU * 1000. / 48000.).sin() * 0.7;
            n.process([x, -x * 0.7]);
        }
        assert!(n.gain_db() < -10.);
        let room = n.available_headroom_db().unwrap();
        assert!(room.abs() < 1., "controlled output near target must not report raw input excess: {room}");
    }
}
