//! Foreground-priority, source-domain auxiliary control. No programme LUFS target.
//! Geometry is a conservative fallback, not vocal/semantic classification.
//! Uncontrolled source references are observed once; the same source gain feeds
//! its direct, reflected, diffuse and bass paths before any binaural processing.
use crate::{Source, SourceKind, convolution::DEFAULT_PARTITION, vbap};

#[derive(Clone, Copy, Default)]
pub(crate) struct Reference {
    pub groups: [[f64; 4]; 2],
    pub temporal: crate::temporal_detail::Observation,
}
impl Reference {
    pub fn add(&mut self, other: &Self) {
        self.temporal.add(other.temporal);
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
    protection: f32,
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
                protection: 1.,
                ..Self::default()
            };
        }
        let [x, y, z] = [x / radius, y / radius, z / radius];
        let auxiliary =
            crate::spatial_layer::weight_for(position, crate::spatial_layer::Geometry::Front45);
        // A steady forward-hemisphere source keeps its authored baseline,
        // including steep height/near-side positions. Membership itself slews.
        let protection = if y > 1e-8 { 1. } else { 0. };
        Self {
            basis: [
                std::f64::consts::FRAC_1_SQRT_2,
                x * std::f64::consts::FRAC_1_SQRT_2,
                y * std::f64::consts::FRAC_1_SQRT_2,
                z * std::f64::consts::FRAC_1_SQRT_2,
            ],
            front: smooth(y / 0.5),
            auxiliary,
            protection,
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
            out.protection += r.protection * power as f32;
            out.auxiliary += r.auxiliary * power as f32;
            for axis in 0..4 {
                out.basis[axis] += r.basis[axis] * power;
            }
        }
        if total <= 1e-12 {
            return Self::position([0., 0., 0.]);
        }
        out.front /= total;
        out.protection /= total as f32;
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
        let initial = source.auxiliary_route_at == u64::MAX;
        source.auxiliary_route = Route::bed(source, solver);
        if initial {
            source.auxiliary_protection = source.auxiliary_route.protection;
        }
        source.auxiliary_route_at = 0;
    }
}
#[cfg(test)]
pub(crate) fn process_source(source: &mut Source, sample: f32, at: u64, control: f32, reference: &mut Reference, rate: u32, solver: &vbap::VbapSolver) -> f32 {
    process_source_detail(source,sample,at,control,reference,rate,solver,None,0)
}
pub(crate) fn process_source_detail(
    source: &mut Source,
    sample: f32,
    at: u64,
    control: f32,
    reference: &mut Reference,
    rate: u32,
    solver: &vbap::VbapSolver,
    temporal: Option<crate::temporal_detail::Control<'_>>,
    index: usize,
) -> f32 {
    if source.auxiliary_route_at == u64::MAX || (source.kind == SourceKind::Object && at % 128 == 0)
    {
        let initial = source.auxiliary_route_at == u64::MAX;
        source.auxiliary_route = if source.kind == SourceKind::Object {
            Route::position(source.position)
        } else {
            Route::bed(source, solver)
        };
        if initial {
            source.auxiliary_protection = source.auxiliary_route.protection;
        }
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
    source.auxiliary_protection += (route.protection - source.auxiliary_protection).clamp(
        -1. / (rate.max(1) as f32 * 0.03),
        1. / (rate.max(1) as f32 * 0.03),
    );
    if (route.protection - source.auxiliary_protection).abs() < 1e-6 {
        source.auxiliary_protection = route.protection;
    }
    // Foreground height/bottom sources are protected programme material. Do
    // not count the same samples in the auxiliary reference as well: a loud
    // forward layer would otherwise hide a weak rear/overhead deficit.
    let auxiliary = source.auxiliary_weight * (1. - source.auxiliary_protection).clamp(0., 1.);
    for axis in 0..4 {
        reference.groups[0][axis] += x * route.basis[axis] * route.front;
        reference.groups[1][axis] += x * route.basis[axis] * f64::from(auxiliary);
    }
    if let Some(strength) = temporal.and_then(|detail| detail.directional_strength) {
        // New presentation redistributes the existing auxiliary spectrum. No
        // broadband group gain, cut, common-output feedback or cache is used.
        return source.directional_detail.process(sample, auxiliary, strength[index], rate);
    }
    let control = if control.is_finite() {
        control.max(0.)
    } else {
        1.
    };
    let gain = 1. + (control - 1.) * auxiliary;
    // In the foreground, remove enhancement only; do not reduce the authored
    // baseline to solve a side/height/rear excess. Membership slews with motion.
    let floor = source.auxiliary_protection / source.spatial_layer_gain.max(1.);
    let controlled=sample*gain.max(floor);
    let Some(detail)=temporal else{return controlled;};
    let eligible=route.protection==0. && auxiliary>0.01;
    let confidence=source.temporal_detail.process(sample,at,index,rate,detail.history,detail.enabled,eligible,source.diffuse);
    let weight=confidence*(1.-source.auxiliary_protection).clamp(0.,1.);
    let extra=if control>=1. { (detail.gains[index]-control).max(0.)*auxiliary*weight }else{0.};
    let output=if extra>0. {controlled+sample*extra}else{controlled};
    let a=f64::from(auxiliary);
    reference.temporal.auxiliary_abs+=x.abs()*a;
    reference.temporal.applied_abs+=if output.is_finite(){f64::from(output).abs()*a}else{0.};
    reference.temporal.candidate_power+=x*x*a*a*f64::from(weight);
    reference.temporal.evidence_power+=x*x*a*a*f64::from(confidence);
    if extra>0.&&gain.max(floor)>0.{reference.temporal.maximum_extra_db=reference.temporal.maximum_extra_db.max(20.*f64::from((gain.max(floor)+extra)/gain.max(floor)).log10());}
    output
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
            allowed_dominance_db: 1.5,
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
    reference_gain: f64,
    observed: u64,
    pub gains: [f32; DEFAULT_PARTITION],
    enabled: bool,
    gain_db: f64,
    goal_db: f64,
    goal_dirty: bool,
    headroom: Option<f64>,
    pub maximum_attenuation_db: f64,
    last_applied_gain: f32,
    temporal: crate::temporal_detail::Budget,
    temporal_detail_enabled: bool,
    directional: bool,
    directional_strength: [f32; DEFAULT_PARTITION],
    strength: f64,
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
            reference_gain: 1.,
            observed: 0,
            gains: [1.; DEFAULT_PARTITION],
            enabled: false,
            gain_db: 0.,
            goal_db: 0.,
            goal_dirty: false,
            headroom: None,
            maximum_attenuation_db: 0.,
            last_applied_gain: 1.,
            temporal: crate::temporal_detail::Budget::new(rate),
            temporal_detail_enabled: true,
            directional: false,
            directional_strength: [0.; DEFAULT_PARTITION],
            strength: 0.,
        }
    }
    pub fn directional_detail(rate: u32) -> Self {
        let mut controller = Self::new(rate);
        controller.directional = true;
        // Spectral redistribution does not raise the noise floor in bulk.
        // Keep genuine quiet ambience eligible below the old lift gate.
        controller.policy.reference_gate_db = -75.;
        controller
    }
    #[cfg(test)]
    pub fn directional_strength(&self) -> f32 { self.directional_strength[0] }
    pub fn reset(&mut self) {
        self.filters = [[KWeight::new(self.rate); 4]; 2];
        self.energy = [0.; 2];
        self.reference_gain = 1.;
        self.observed = 0;
        self.gains.fill(1.);
        self.gain_db = 0.;
        self.goal_db = 0.;
        self.goal_dirty = false;
        self.headroom = None;
        self.maximum_attenuation_db = 0.;
        self.last_applied_gain = 1.;
        self.temporal.reset();
        self.directional_strength.fill(0.);
        self.strength = 0.;
    }
    pub fn set_enabled(&mut self, enabled: bool, immediate: bool, offset: usize) {
        if immediate && !enabled {
            self.enabled = false;
            self.gain_db = 0.;
            self.gains.fill(1.);
            self.temporal.disable_immediately();
            self.directional_strength.fill(0.);
            self.strength = 0.;
            return;
        }
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        let offset = offset.min(DEFAULT_PARTITION - 1);
        self.temporal.retarget_at(offset,self.gains[offset]);
        self.gain_db = 20. * f64::from(self.gains[offset]).max(1e-12).log10();
        self.fill_from(offset);
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn levels(&self) -> [f64; 4] {
        let reference_db = 20. * self.reference_gain.max(1e-30).log10();
        [
            20. * f64::from(self.last_applied_gain).max(1e-12).log10(),
            self.maximum_attenuation_db,
            10. * self.energy[0].max(1e-30).log10() + reference_db,
            10. * self.energy[1].max(1e-30).log10() + reference_db,
        ]
    }
    pub fn mark_applied(&mut self, index: usize) {
        self.last_applied_gain = self.gains[index];
        if self.last_applied_gain < 1. || !self.last_applied_gain.is_finite() {
            self.maximum_attenuation_db = self.maximum_attenuation_db
                .max(-20. * f64::from(self.last_applied_gain).max(1e-12).log10());
        }
    }
    #[cfg(test)]
    pub fn observe(&mut self, reference: Reference, programme_gain: f32) {self.observe_at(reference,programme_gain,self.observed);}
    pub fn temporal_control(&self)->crate::temporal_detail::Control<'_>{crate::temporal_detail::Control{history:&self.temporal.history,gains:&self.temporal.gains,enabled:self.temporal_enabled(),directional_strength:self.directional.then_some(&self.directional_strength)}}
    pub fn temporal_enabled(&self)->bool{self.enabled&&!self.directional&&self.temporal_detail_enabled&&self.policy.maximum_cut_db>0.}
    #[cfg(test)]
    pub fn set_temporal_for_diagnostics(&mut self,enabled:bool){self.temporal_detail_enabled=enabled;if !enabled{self.temporal.disable_immediately();}}
    pub fn temporal_levels(&self)->[f64;4]{self.temporal.levels()}
    pub fn observe_detail_output(&mut self,x:[f64;2]){self.temporal.observe_output(x);}
    pub fn observe_at(&mut self, reference: Reference, programme_gain: f32, at:u64) {
        self.reference_gain = if programme_gain.is_finite() {
            f64::from(programme_gain).max(0.)
        } else {
            0.
        };
        let mut front_power=0.;
        for group in 0..2 {
            let mut power = 0.;
            for axis in 0..4 {
                let x = reference.groups[group][axis];
                let y = self.filters[group][axis].process(if x.is_finite() { x } else { 0. });
                power += y * y;
            }
            if group==0{front_power=power;}
            self.energy[group] += (power - self.energy[group]) * self.energy_alpha;
        }
        self.temporal.observe(reference.temporal,front_power,at);
        self.observed = self.observed.saturating_add(1);
        // Detector state still advances for every source sample. Its dB
        // target is only consumed at fill_from (block/toggle boundaries).
        self.goal_dirty = true;
    }
    fn refresh_goal(&mut self) {
        if !self.goal_dirty { return; }
        self.goal_dirty = false;
        let db = 20. * self.reference_gain.max(1e-30).log10();
        let front = 10. * self.energy[0].max(1e-30).log10() + db;
        let auxiliary = 10. * self.energy[1].max(1e-30).log10() + db;
        self.goal_db = if self.reference_gain <= 0.
            || front < self.policy.reference_gate_db
            || auxiliary < self.policy.reference_gate_db
        {
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
            if reduction > 0. {
                -reduction.min(self.policy.maximum_cut_db)
            } else if self.observed >= u64::from(self.rate) * 2 / 5 {
                // Positive gain is a fraction of a measured deficit, never a
                // stored +2/+6 recipe. Keep 3 dB foreground separation.
                ((front - auxiliary - 3.) * 0.5).max(0.)
            } else {
                0.
            }
        };
    }
    pub fn set_output_headroom(&mut self, available_db: Option<f64>) {
        self.headroom = available_db.filter(|v| v.is_finite())
            .map(|db| db.min(self.temporal.peak_headroom_db()));
    }
    #[cfg(test)]
    pub fn headroom_db(&self) -> Option<f64> {
        self.headroom
    }
    fn fill_from(&mut self, offset: usize) {
        self.refresh_goal();
        if self.directional {
            let deficit = self.goal_db.max(0.);
            let target = if self.enabled && self.policy.maximum_cut_db > 0. {
                deficit / (deficit + 6.)
            } else { 0. };
            let rate = f64::from(self.rate.max(1));
            let attack = (-1. / (rate * 0.15)).exp();
            let release = (-1. / (rate * 0.05)).exp();
            for i in offset..DEFAULT_PARTITION {
                self.gains[i] = 1.;
                self.directional_strength[i] = self.strength as f32;
                let coefficient = if target > self.strength { attack } else { release };
                self.strength = target + (self.strength-target)*coefficient;
            }
            self.gain_db = 0.;
            self.temporal.disable_immediately();
            return;
        }
        let target = if !self.enabled || self.policy.maximum_cut_db == 0. {
            0.
        } else if self.goal_db <= 0. {
            self.goal_db
        } else {
            // Incremental headroom feedback uses already rendered output; the
            // growth rate is slow relative to the detector. Missing confidence
            // withdraws enhancement instead of falling back to a positive gain.
            self.headroom.map_or(0., |h| {
                // h is a whole-mix dB budget, not an auxiliary-source gain.
                // Bound the incremental source lift using its current share
                // of the summed amplitudes. This is more conservative than
                // assuming uncorrelated powers and keeps a 0.2 dB mix reserve.
                let front = self.energy[0].max(0.).sqrt();
                let auxiliary = self.energy[1].max(0.).sqrt()
                    * 10f64.powf(self.gain_db / 20.);
                let share = auxiliary / (front + auxiliary).max(1e-30);
                if share <= 1e-12 { return 0.; }
                let mix_ratio = 10f64.powf((h - 0.2) / 20.);
                let source_ratio = (1. + (mix_ratio - 1.) / share).max(1e-12);
                let source_budget = 20. * source_ratio.log10();
                self.goal_db.min((self.gain_db.max(0.) + source_budget).max(0.))
            })
        };
        let initial_db = self.gain_db;
        for i in offset..DEFAULT_PARTITION {
            self.gains[i] = 10f64.powf(self.gain_db / 20.) as f32;
            if !self.enabled {
                let step = self.policy.maximum_cut_db.max(self.gain_db.abs()).max(1.)
                    / (f64::from(self.rate) * 0.05);
                self.gain_db += (-self.gain_db).clamp(-step, step);
            } else {
                let ms = if target < self.gain_db && self.gain_db > 0. {
                    // Withdraw added enhancement before the slower common
                    // protector reacts. Authored auxiliary cuts retain their
                    // own attack/release policy below unity.
                    5.
                } else if target < self.gain_db {
                    self.policy.attack_ms
                } else {
                    self.policy.release_ms
                };
                let next =
                    target + (self.gain_db - target) * (-1000. / (f64::from(self.rate) * ms)).exp();
                // Gain above authored unity grows at most 0.25 dB per second.
                // Recovery from attenuation keeps the established release.
                self.gain_db = if next > 0. && next > self.gain_db {
                    next.min(self.gain_db.max(0.) + 0.25 / f64::from(self.rate))
                } else {
                    next
                };
                if (self.gain_db - target).abs() < 1e-8 {
                    self.gain_db = target;
                }
            }
        }
        // Both enhancement stages share one budget. Reserve the mix growth
        // already scheduled above before the temporal-detail stage can spend it.
        let front = self.energy[0].max(0.).sqrt();
        let auxiliary = self.energy[1].max(0.).sqrt() * 10f64.powf(initial_db / 20.);
        let share = auxiliary / (front + auxiliary).max(1e-30);
        let growth = 20. * (1. + share * (10f64.powf((self.gain_db-initial_db).max(0.)/20.) - 1.)).log10();
        let remaining = self.headroom.map(|room| room-growth);
        self.temporal.fill(&self.gains,front,self.energy[1].max(0.).sqrt(),self.reference_gain,remaining,self.temporal_enabled(),offset);
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
    fn dynamic_groups_cover_the_same_side_and_height_lift() {
        for p in [[0., 1., 0.], [-1., 1., 0.], [1., 1., 0.]] {
            assert_eq!(Route::position(p).auxiliary, 0.);
        }
        for p in [
            [1., 0., 0.],
            [-1., 0., 0.],
            [0., -1., 0.],
            [0., 0., 1.],
            [0., 0., -1.],
        ] {
            assert_eq!(
                Route::position(p).auxiliary,
                1.,
                "uncontrolled boosted source {p:?}"
            );
        }
        for azimuth in -180..=180 {
            for elevation in -90..=90 {
                let a = (azimuth as f64).to_radians();
                let e = (elevation as f64).to_radians();
                let p = [
                    (a.sin() * e.cos()) as f32,
                    (a.cos() * e.cos()) as f32,
                    e.sin() as f32,
                ];
                assert_eq!(
                    Route::position(p).auxiliary,
                    crate::spatial_layer::weight_for(p, crate::spatial_layer::Geometry::Front45)
                );
            }
        }
        assert_eq!(Route::position([0., 0., 0.]).auxiliary, 0.);
        assert_eq!(Route::position([f32::NAN, 0., 0.]).auxiliary, 0.);
    }
    #[test]
    fn foreground_height_control_cannot_remove_the_unenhanced_baseline() {
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        for p in [
            [0., 1., 3.],
            [-2., 1., 3.],
            [2., 1., -3.],
            [-10., 1., 3.],
            [10., 1., -3.],
        ] {
            let mut source = Source {
                kind: SourceKind::Object,
                position: p,
                spatial_layer_gain: 10f32.powf(1. / 20.),
                ..Default::default()
            };
            for i in 0..4800 {
                let mut reference = Reference::default();
                let out = process_source(
                    &mut source,
                    10f32.powf(1. / 20.),
                    i,
                    0.25,
                    &mut reference,
                    48000,
                    &solver,
                );
                assert!(out >= 1. - 2e-6, "front baseline reduced at {p:?}: {out}");
            }
        }
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
        for (f, a) in [(0.03, 0.), (0., 0.3), (0.3, 0.03)] {
            assert_eq!(run(f, a, 1.).maximum_attenuation_db, 0.);
        }
        // Different test frequencies have slightly different K weighting.
        assert!(run(0.03, 0.03, 1.).maximum_attenuation_db < 0.1);
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

#[cfg(test)]
mod adaptive_lift_tests {
    use super::*;
    fn block(b: &mut AuxiliaryBalance, front: f64, aux: f64, block: u64) {
        for j in 0..DEFAULT_PARTITION {
            let i = block * DEFAULT_PARTITION as u64 + j as u64;
            let mut r = Reference::default();
            r.groups[0][0] = (i as f64 * 0.13).sin() * front;
            r.groups[1][0] = (i as f64 * 0.17).sin() * aux;
            b.mark_applied(j);
            b.observe(r, 1.);
        }
        b.finish_block();
    }
    #[test]
    fn missing_measurement_and_silence_never_fall_back_to_positive_gain() {
        let mut b = AuxiliaryBalance::new(48000);
        b.set_enabled(true, true, 0);
        for i in 0..400 {
            block(&mut b, 0.1, 0.01, i);
            assert!(b.gains.iter().all(|g| *g == 1.));
        }
        b.set_output_headroom(Some(30.));
        b.reset();
        for i in 0..100 {
            block(&mut b, 0., 0., i);
            assert!(b.gains.iter().all(|g| *g == 1.));
        }
        assert_eq!(b.headroom_db(), None);
    }
    #[test]
    fn enhancement_depends_on_measurement_and_withdraws_without_headroom() {
        let mut b = AuxiliaryBalance::new(48000);
        b.set_enabled(true, true, 0);
        b.set_output_headroom(Some(10.));
        let mut previous = 0f64;
        for i in 0..400 {
            block(&mut b, 0.1, 0.01, i);
            let db = 20. * f64::from(b.gains[DEFAULT_PARTITION - 1]).log10();
            assert!(db - previous <= 0.25 * DEFAULT_PARTITION as f64 / 48000. + 1e-5);
            previous = db;
        }
        assert!(
            previous > 1. && previous < 2.2,
            "unexpected enhancement {previous}"
        );
        b.set_output_headroom(None);
        for i in 400..500 {
            block(&mut b, 0.1, 0.01, i);
        }
        assert!(b.gains.iter().all(|g| (*g - 1.).abs() < 1e-6));
    }
    #[test]
    fn diagnostic_bypass_disables_measured_enhancement_too() {
        let mut b = AuxiliaryBalance::with_policy(
            48000,
            Policy {
                maximum_cut_db: 0.,
                ..Default::default()
            },
        );
        b.set_enabled(true, true, 0);
        b.set_output_headroom(Some(30.));
        for i in 0..100 {
            block(&mut b, 0.1, 0.01, i);
            assert!(b.gains.iter().all(|g| *g == 1.));
        }
    }
}


#[cfg(test)]
mod requested_direction_tests {
    use super::*;
    #[test]
    fn rear_overhead_diagonals_and_upper_rear_lift_only_with_measured_room() {
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        for position in [[0., -1., 0.], [0., 0., 1.], [-1., -1., 0.], [1., -1., 0.],
                         [-1., -1., 1.], [1., -1., 1.], [1., 0., 1.]] {
            for room in [None, Some(12.)] {
                let mut control = AuxiliaryBalance::new(48000);
                control.set_enabled(true, true, 0);
                control.set_output_headroom(room);
                let mut front = Source { kind: SourceKind::Object, position: [0., 1., 0.], ..Default::default() };
                let mut auxiliary = Source { kind: SourceKind::Object, position, ..Default::default() };
                for i in 0..48000 * 3 {
                    let j = i % DEFAULT_PARTITION;
                    let mut reference = Reference::default();
                    let f = (i as f32 * 0.13).sin() * 0.1;
                    let a = (i as f32 * 0.17).sin() * 0.01;
                    let gain = control.gains[j];
                    let out_front = process_source(&mut front, f, i as u64, gain, &mut reference, 48000, &solver);
                    let out_aux = process_source(&mut auxiliary, a, i as u64, gain, &mut reference, 48000, &solver);
                    assert_eq!(out_front, f, "front changed while lifting {position:?}");
                    assert!((out_aux - a * gain).abs() < 1e-7, "direction omitted: {position:?}");
                    control.mark_applied(j);
                    control.observe(reference, 1.);
                    if j + 1 == DEFAULT_PARTITION { control.finish_block(); }
                }
                let lift = 20. * f64::from(control.gains[0]).log10();
                if room.is_some() {
                    assert!(lift > 0.4 && lift <= 0.751, "{position:?}: {lift} dB");
                } else {
                    assert_eq!(lift, 0., "missing headroom must not invent enhancement");
                }
            }
        }
    }
}

#[cfg(test)]
mod mix_budget_tests {
    use super::*;
    #[test]
    fn subdecibel_mix_room_can_lift_a_measured_weak_auxiliary_group() {
        let mut b = AuxiliaryBalance::new(48000);
        b.set_enabled(true, true, 0);
        b.set_output_headroom(Some(0.8));
        for i in 0..48000 * 4 {
            let mut r = Reference::default();
            r.groups[0][0] = (i as f64 * 0.13).sin() * 0.1;
            r.groups[1][0] = (i as f64 * 0.17).sin() * 0.01;
            b.observe(r, 1.);
            b.mark_applied(i % DEFAULT_PARTITION);
            if (i + 1) % DEFAULT_PARTITION == 0 { b.finish_block(); }
        }
        let lift = 20. * f64::from(b.gains[0]).log10();
        assert!(lift > 0.7 && lift <= 1.001, "valid sub-dB mix budget was discarded: {lift}");
        assert_eq!(b.maximum_attenuation_db, 0.);
    }
}
#[cfg(test)]
mod exclusive_reference_tests {
    use super::*;
    #[test]
    fn protected_front_height_and_bottom_do_not_enter_the_auxiliary_reference() {
        let solver=vbap::VbapSolver::with_layout(vbap::LayoutId::Sony360Ra13);
        for position in [[-0.433,0.75,0.5],[0.433,0.75,0.5],[-0.433,0.75,-0.5],[0.433,0.75,-0.5]] {
            for gain in [0.25,1.,2.] {
                let mut s=Source{kind:SourceKind::Object,position,..Default::default()};
                for at in 0..2048 {
                    let mut r=Reference::default();
                    assert_eq!(process_source(&mut s,0.3,at,gain,&mut r,48000,&solver),0.3);
                    assert_eq!(r.groups[1],[0.;4]);
                    assert!(r.groups[0].iter().any(|v|v.abs()>0.));
                }
            }
        }
    }
    #[test]
    fn loud_protected_height_cannot_hide_a_weak_rear_deficit() {
        let solver=vbap::VbapSolver::with_layout(vbap::LayoutId::Sony360Ra13);
        let mut front=Source{kind:SourceKind::Object,position:[0.,1.,0.],..Default::default()};
        let mut height=Source{kind:SourceKind::Object,position:[0.,1.,1.],..Default::default()};
        let mut rear=Source{kind:SourceKind::Object,position:[0.,-1.,1.],..Default::default()};
        let mut b=AuxiliaryBalance::new(48000);b.set_enabled(true,true,0);b.set_output_headroom(Some(20.));
        for i in 0..48000*5 {
            let j=i%DEFAULT_PARTITION;let gain=b.gains[j];let mut r=Reference::default();
            let f=(i as f32*0.13).sin()*0.1;let h=(i as f32*0.17).sin()*0.6;let a=(i as f32*0.19).sin()*0.01;
            assert_eq!(process_source(&mut front,f,i as u64,gain,&mut r,48000,&solver),f);
            assert_eq!(process_source(&mut height,h,i as u64,gain,&mut r,48000,&solver),h);
            process_source(&mut rear,a,i as u64,gain,&mut r,48000,&solver);
            b.mark_applied(j);b.observe(r,1.);if j+1==DEFAULT_PARTITION{b.finish_block();}
        }
        assert!(20.*f64::from(b.gains[0]).log10()>1.,"protected height still masks the rear reference");
    }
}
#[cfg(test)]
mod lazy_control_tests {
    use super::*;
    #[test]
    fn lazy_targets_equal_eager_targets_at_blocks_and_mid_block_toggles() {
        for rate in [44100,48000,96000] {
            for directional in [false,true] {
                let make=||if directional {AuxiliaryBalance::directional_detail(rate)} else {AuxiliaryBalance::new(rate)};
                let (mut lazy,mut eager)=(make(),make());
                lazy.set_output_headroom(Some(8.));eager.set_output_headroom(Some(8.));
                let mut enabled=false;
                for at in 0..rate as u64*2 {
                    let index=at as usize%DEFAULT_PARTITION;
                    if [0,765,901,2203,43001,48001].contains(&at) {
                        enabled=!enabled;lazy.set_enabled(enabled,false,index);eager.set_enabled(enabled,false,index);
                        assert_eq!(lazy.gains,eager.gains);assert_eq!(lazy.directional_strength,eager.directional_strength);
                    }
                    let front=(at as f64*0.071).sin()*0.02;
                    let rear=(at as f64*0.031).sin()*if at%17000<10000{0.001}else{0.04};
                    let mut r=Reference::default();r.groups[0]=[front,front*0.2,front*0.8,0.];r.groups[1]=[rear,rear*0.3,-rear*0.7,rear*0.4];
                    let programme=if at<rate as u64{0.7}else{0.5};
                    lazy.mark_applied(index);eager.mark_applied(index);
                    lazy.observe_at(r,programme,at);eager.observe_at(r,programme,at);eager.refresh_goal();
                    if index+1==DEFAULT_PARTITION {
                        lazy.finish_block();eager.finish_block();
                        assert_eq!(lazy.goal_db.to_bits(),eager.goal_db.to_bits());
                        assert_eq!(lazy.gains,eager.gains);assert_eq!(lazy.directional_strength,eager.directional_strength);
                        assert_eq!(lazy.levels(),eager.levels());
                    }
                }
            }
        }
    }
}
