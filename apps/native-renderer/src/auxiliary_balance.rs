//! Foreground-priority, source-domain auxiliary control. No programme LUFS target.
//! Geometry is a conservative fallback, not vocal/semantic classification.
//! Uncontrolled source references are observed once; the same source gain feeds
//! its direct, reflected, diffuse and bass paths before any binaural processing.
use crate::{Source, SourceKind, convolution::DEFAULT_PARTITION, vbap};

#[derive(Clone, Copy, Default)]
pub(crate) struct Reference {
    pub groups: [[f64; 4]; 2],
}
impl Reference {
    pub fn add(&mut self, other: &Self) {
        for group in 0..2 {
            for axis in 0..4 {
                self.groups[group][axis] += other.groups[group][axis];
            }
        }
    }
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Route {
    basis: [f64; 4],
    front: f64,
    auxiliary: f32,
    lfe: bool,
}
fn smooth(x: f64) -> f64 {
    let x = x.clamp(0., 1.);
    x * x * (3. - 2. * x)
}
impl Route {
    fn position(position: [f32; 3]) -> Self {
        let [x, y, z] = position.map(f64::from);
        let radius = (x * x + y * y + z * z).sqrt();
        if !radius.is_finite() || radius < 1e-8 {
            return Self {
                basis: [1., 0., 0., 0.],
                front: 1.,
                ..Self::default()
            };
        }
        let [x, y, z] = [x / radius, y / radius, z / radius];
        let rear = smooth(-y / std::f64::consts::FRAC_1_SQRT_2);
        // Protect every source in the forward hemisphere, including steep
        // front-height/bottom positions. Exact overhead is ambiguous without
        // content roles, so only the rear part of a height layer is controlled.
        let height = smooth((z.abs() - 0.35) / 0.5) * smooth(-y / 0.25);
        Self {
            basis: [
                std::f64::consts::FRAC_1_SQRT_2,
                x * std::f64::consts::FRAC_1_SQRT_2,
                y * std::f64::consts::FRAC_1_SQRT_2,
                z * std::f64::consts::FRAC_1_SQRT_2,
            ],
            front: smooth(y / 0.5),
            auxiliary: (1. - (1. - rear) * (1. - height)) as f32,
            lfe: false,
        }
    }
    fn bed(source: &Source, solver: &vbap::VbapSolver) -> Self {
        let route = crate::bed_route(source.bed_label.as_deref().unwrap_or(""), solver);
        if route.lfe > 0. {
            return Self {
                lfe: true,
                ..Self::default()
            };
        }
        let mut out = Self::default();
        let mut total = 0.;
        for (gain, speaker) in route.buses.iter().zip(vbap::speakers(solver.layout())) {
            let power = f64::from(*gain).powi(2);
            if power == 0. {
                continue;
            }
            let r = Self::position(crate::static_bed_position(
                speaker.azimuth,
                speaker.elevation,
            ));
            total += power;
            out.front += r.front * power;
            out.auxiliary += r.auxiliary * power as f32;
            for axis in 0..4 {
                out.basis[axis] += r.basis[axis] * power;
            }
        }
        if total <= 1e-12 {
            return Self::position([0., 0., 0.]);
        }
        out.front /= total;
        out.auxiliary /= total as f32;
        for axis in 0..4 {
            out.basis[axis] /= total;
        }
        out
    }
}
pub(crate) fn refresh_beds(
    sources: &mut std::collections::HashMap<String, Source>,
    solver: &vbap::VbapSolver,
) {
    for source in sources.values_mut().filter(|s| s.kind == SourceKind::Bed) {
        source.auxiliary_route = Route::bed(source, solver);
        source.auxiliary_route_at = 0;
    }
}
pub(crate) fn process_source(
    source: &mut Source,
    sample: f32,
    at: u64,
    control: f32,
    reference: &mut Reference,
    rate: u32,
    solver: &vbap::VbapSolver,
) -> f32 {
    if source.auxiliary_route_at == u64::MAX || (source.kind == SourceKind::Object && at % 128 == 0)
    {
        source.auxiliary_route = if source.kind == SourceKind::Object {
            Route::position(source.position)
        } else {
            Route::bed(source, solver)
        };
        source.auxiliary_route_at = at;
    }
    let route = source.auxiliary_route;
    if route.lfe {
        source.auxiliary_weight = 0.;
        return sample;
    }
    // Slew group membership as well as group gain for moving sources.
    source.auxiliary_weight += (route.auxiliary - source.auxiliary_weight).clamp(
        -1. / (rate.max(1) as f32 * 0.03),
        1. / (rate.max(1) as f32 * 0.03),
    );
    if (route.auxiliary - source.auxiliary_weight).abs() < 1e-6 {
        source.auxiliary_weight = route.auxiliary;
    }
    let x = if sample.is_finite() {
        f64::from(sample)
    } else {
        0.
    };
    for axis in 0..4 {
        reference.groups[0][axis] += x * route.basis[axis] * route.front;
        reference.groups[1][axis] += x * route.basis[axis] * f64::from(source.auxiliary_weight);
    }
    sample * (1. + (control.clamp(0., 1.) - 1.) * source.auxiliary_weight)
}

#[derive(Clone, Copy)]
pub(crate) struct Policy {
    pub maximum_cut_db: f64,
    pub allowed_dominance_db: f64,
    pub knee_db: f64,
    pub attack_ms: f64,
    pub release_ms: f64,
    pub reference_gate_db: f64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            maximum_cut_db: 6.,
            allowed_dominance_db: 3.,
            knee_db: 3.,
            attack_ms: 80.,
            release_ms: 1200.,
            reference_gate_db: -55.,
        }
    }
}

pub(crate) struct AuxiliaryBalance {
    rate: u32,
    pub policy: Policy,
    filters: [[KWeight; 4]; 2],
    energy: [f64; 2],
    energy_alpha: f64,
    pub gains: [f32; DEFAULT_PARTITION],
    enabled: bool,
    gain_db: f64,
    goal_db: f64,
    pub maximum_attenuation_db: f64,
    last_applied_gain: f32,
}
impl AuxiliaryBalance {
    pub fn new(rate: u32) -> Self {
        Self::with_policy(rate, Policy::default())
    }
    pub fn with_policy(rate: u32, policy: Policy) -> Self {
        Self {
            rate,
            policy,
            filters: [[KWeight::new(rate); 4]; 2],
            energy: [0.; 2],
            energy_alpha: 1. - (-1. / (f64::from(rate) * 0.2)).exp(),
            gains: [1.; DEFAULT_PARTITION],
            enabled: false,
            gain_db: 0.,
            goal_db: 0.,
            maximum_attenuation_db: 0.,
            last_applied_gain: 1.,
        }
    }
    pub fn reset(&mut self) {
        self.filters = [[KWeight::new(self.rate); 4]; 2];
        self.energy = [0.; 2];
        self.gains.fill(1.);
        self.gain_db = 0.;
        self.goal_db = 0.;
        self.maximum_attenuation_db = 0.;
        self.last_applied_gain = 1.;
    }
    pub fn set_enabled(&mut self, enabled: bool, immediate: bool, offset: usize) {
        if immediate && !enabled {
            self.enabled = false;
            self.gain_db = 0.;
            self.gains.fill(1.);
            return;
        }
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        let offset = offset.min(DEFAULT_PARTITION - 1);
        self.gain_db = 20. * f64::from(self.gains[offset]).max(1e-12).log10();
        self.fill_from(offset);
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn levels(&self) -> [f64; 4] {
        [
            20. * f64::from(self.last_applied_gain).max(1e-12).log10(),
            self.maximum_attenuation_db,
            10. * self.energy[0].max(1e-30).log10(),
            10. * self.energy[1].max(1e-30).log10(),
        ]
    }
    pub fn mark_applied(&mut self, index: usize) {
        self.last_applied_gain = self.gains[index];
        self.maximum_attenuation_db = self
            .maximum_attenuation_db
            .max(-20. * f64::from(self.last_applied_gain).max(1e-12).log10());
    }
    pub fn observe(&mut self, reference: Reference, programme_gain: f32) {
        let scale = f64::from(programme_gain).max(0.).powi(2);
        for group in 0..2 {
            let mut power = 0.;
            for axis in 0..4 {
                let x = reference.groups[group][axis];
                let y = self.filters[group][axis].process(if x.is_finite() { x } else { 0. });
                power += y * y;
            }
            self.energy[group] += (power * scale - self.energy[group]) * self.energy_alpha;
        }
        let front = 10. * self.energy[0].max(1e-30).log10();
        let auxiliary = 10. * self.energy[1].max(1e-30).log10();
        self.goal_db =
            if front < self.policy.reference_gate_db || auxiliary < self.policy.reference_gate_db {
                0.
            } else {
                let excess = auxiliary - front - self.policy.allowed_dominance_db;
                let knee = self.policy.knee_db;
                let reduction = if excess <= -knee / 2. {
                    0.
                } else if excess >= knee / 2. {
                    0.5 * excess
                } else {
                    (excess + knee / 2.).powi(2) / (4. * knee)
                };
                -reduction.clamp(0., self.policy.maximum_cut_db)
            };
    }
    fn fill_from(&mut self, offset: usize) {
        let target = if self.enabled { self.goal_db } else { 0. };
        for i in offset..DEFAULT_PARTITION {
            self.gains[i] = 10f64.powf(self.gain_db / 20.) as f32;
            if !self.enabled {
                self.gain_db = (self.gain_db
                    + self.policy.maximum_cut_db.max(1.) / (f64::from(self.rate) * 0.05))
                    .min(0.);
            } else {
                let ms = if target < self.gain_db {
                    self.policy.attack_ms
                } else {
                    self.policy.release_ms
                };
                self.gain_db =
                    target + (self.gain_db - target) * (-1000. / (f64::from(self.rate) * ms)).exp();
                if (self.gain_db - target).abs() < 1e-8 {
                    self.gain_db = target;
                }
            }
        }
    }
    pub fn finish_block(&mut self) {
        self.fill_from(0);
    }
}

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
}

#[derive(Clone, Copy)]
struct KWeight {
    shelf: Filter,
    high: Filter,
}
impl KWeight {
    fn new(rate: u32) -> Self {
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

        Self { shelf, high }
    }
    fn process(&mut self, x: f64) -> f64 {
        self.high.run(self.shelf.run(x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn world_groups_protect_front_and_lfe_without_semantic_labels() {
        for p in [[0., 1., 0.], [-1., 1., 0.], [1., 1., 0.], [0., 1., 1.]] {
            assert_eq!(Route::position(p).auxiliary, 0.);
        }
        assert_eq!(Route::position([0., -1., 0.]).auxiliary, 1.);
        assert_eq!(Route::position([0., 0., 1.]).auxiliary, 0.);
        for azimuth in -90..=90 {for elevation in -90..=90 {
            let a=(azimuth as f64).to_radians();let e=(elevation as f64).to_radians();
            let p=[(a.sin()*e.cos()) as f32,(a.cos()*e.cos()) as f32,e.sin() as f32];
            assert_eq!(Route::position(p).auxiliary,0.,"front-height {azimuth}/{elevation}");
        }}
        assert_eq!(Route::position([0., 0., 0.]).auxiliary, 0.);
        assert_eq!(Route::position([f32::NAN, 0., 0.]).auxiliary, 0.);
    }
    fn run(front: f64, back: f64, programme: f32) -> AuxiliaryBalance {
        let mut b = AuxiliaryBalance::new(48000);
        b.set_enabled(true, true, 0);
        for i in 0..48000 * 4 {
            b.mark_applied(i % DEFAULT_PARTITION);
            let mut r = Reference::default();
            r.groups[0][0] = (i as f64 * 0.13).sin() * front;
            r.groups[1][0] = (i as f64 * 0.17).sin() * back;
            b.observe(r, programme);
            if (i + 1) % DEFAULT_PARTITION == 0 {
                b.finish_block();
            }
        }
        b
    }
    #[test]
    fn only_auxiliary_dominance_can_request_bounded_reduction() {
        for (f, a) in [(0.03, 0.), (0., 0.3), (0.03, 0.03), (0.3, 0.03)] {
            assert_eq!(run(f, a, 1.).maximum_attenuation_db, 0.);
        }
        let b = run(0.03, 0.3, 1.);
        assert!(b.maximum_attenuation_db > 3. && b.maximum_attenuation_db <= 6.000001);
    }
    #[test]
    fn programme_gain_changes_gate_not_a_second_loudness_target() {
        let a = run(0.03, 0.3, 1.);
        let b = run(0.03, 0.3, 0.25);
        assert!((a.goal_db - b.goal_db).abs() < 1e-8);
        assert_eq!(run(0.03, 0.3, 0.).maximum_attenuation_db, 0.);
        assert_eq!(run(0.000001, 0.00001, 1.).maximum_attenuation_db, 0.);
    }
    #[test]
    fn disable_and_transport_reset_remove_old_auxiliary_gain() {
        let mut b = run(0.03, 0.3, 1.);
        assert!(b.gains[0] < 1.);
        b.set_enabled(false, false, 0);
        for _ in 0..5 {
            b.finish_block();
        }
        assert!(b.gains.iter().all(|g| *g == 1.));
        b.reset();
        assert_eq!(b.maximum_attenuation_db, 0.);
        assert!(b.gains.iter().all(|g| *g == 1.));
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;
    #[test]
    fn source_ids_splitting_and_motion_cannot_create_control_steps() {
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        let mut one = Source {
            kind: SourceKind::Object,
            position: [0., -1., 0.],
            ..Default::default()
        };
        let mut split: [Source; 4] = std::array::from_fn(|i| Source {
            kind: SourceKind::Object,
            object_id: Some(900 + i as u32),
            position: [0., -1., 0.],
            ..Default::default()
        });
        let mut last = 1.;
        for i in 0..12000 {
            let angle = i as f32 / 12000. * std::f32::consts::TAU;
            let position = [angle.sin(), angle.cos(), 0.];
            one.position = position;
            for source in &mut split {
                source.position = position;
            }
            let (mut a, mut b) = (Reference::default(), Reference::default());
            let x = process_source(&mut one, 1., i, 0.5, &mut a, 48000, &solver);
            let mut y = 0.;
            for source in &mut split {
                y += process_source(source, 0.25, i, 0.5, &mut b, 48000, &solver);
            }
            assert!((x - y).abs() < 1e-7);
            assert_eq!(a.groups, b.groups);
            assert!((x - last).abs() <= 1. / 1440. + 1e-6);
            last = x;
        }
    }
    #[test]
    fn linked_group_envelope_is_bounded_across_enable_disable_and_programme_updates() {
        let mut b = AuxiliaryBalance::new(48000);
        let mut last = 0f64;
        for i in 0..48000 * 5 {
            if i == 0 || i == 144000 {
                b.set_enabled(true, false, i % DEFAULT_PARTITION);
            }
            if i == 96000 {
                b.set_enabled(false, false, i % DEFAULT_PARTITION);
            }
            let gain = 20. * f64::from(b.gains[i % DEFAULT_PARTITION]).log10();
            assert!(
                (gain - last).abs() < 0.003,
                "gain step at {i}: {}",
                gain - last
            );
            last = gain;
            let mut r = Reference::default();
            r.groups[0][0] = (i as f64 * 0.13).sin() * 0.03;
            r.groups[1][0] = (i as f64 * 0.17).sin() * 0.3;
            b.mark_applied(i % DEFAULT_PARTITION);
            b.observe(r, if i < 48000 { 1. } else { 0.25 });
            if (i + 1) % DEFAULT_PARTITION == 0 {
                b.finish_block();
            }
        }
    }
}
