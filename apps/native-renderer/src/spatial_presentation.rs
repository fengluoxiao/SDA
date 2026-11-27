//! Runtime spatial presentation: causal reflection lead, transient protection,
//! and a separate world-space late diffusion field. Fixed bounded DSP state;
//! no extraction, whole-track scan or persistent audio storage.
use crate::{
    source_extent::Diffuser,
    spatial,
    vbap::{self, VbapSolver},
};
pub const ALGORITHM: &str = "microturn-precedence-envelopment-v1";
const B: usize = vbap::MAX_BUS_COUNT;
#[derive(Clone, Copy, Default)]
struct Gate {
    fast: f32,
    slow: f32,
    gain: f32,
    hold: u32,
    cooldown: u32,
}
impl Gate {
    fn next(&mut self, x: f32, rate: u32) -> f32 {
        let a = x.abs();
        self.fast += (a - self.fast) / (rate as f32 * 0.001).max(1.0);
        self.slow += (a - self.slow) / (rate as f32 * 0.040).max(1.0);
        if self.cooldown == 0 && self.fast > self.slow * 2.2 + 1e-6 {
            self.gain = 0.55;
            self.hold = (rate as f32 * 0.0015) as u32;
            self.cooldown = rate / 40;
        }
        self.cooldown = self.cooldown.saturating_sub(1);
        if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.gain = (self.gain + (1.0 - 0.55) / (rate as f32 * 0.008)).min(1.0);
        }
        self.gain
    }
}
struct Delay {
    data: Vec<[f32; B]>,
    cursor: usize,
}
impl Delay {
    fn new(frames: usize) -> Self {
        Self {
            data: vec![[0.0; B]; frames.max(1)],
            cursor: 0,
        }
    }
    fn next(&mut self, x: [f32; B]) -> [f32; B] {
        let out = self.data[self.cursor];
        self.data[self.cursor] = x;
        self.cursor = (self.cursor + 1) % self.data.len();
        out
    }
    fn reset(&mut self) {
        self.data.fill([0.0; B]);
        self.cursor = 0;
    }
}
pub(crate) struct Field {
    rate: u32,
    strength: f32,
    target: bool,
    lead: Delay,
    late: Delay,
    diffuser: Diffuser,
    gates: [Gate; B],
    routes: Vec<Vec<(usize, f32)>>,
    head: Option<[f32; 4]>,
    pub minimum_gate: f32,
    late_filters: [crate::dsp::Biquad; B],
    clean_late_filter: crate::dsp::Biquad,
}
impl Field {
    pub fn new(rate: u32, solver: &VbapSolver) -> Self {
        let highpass =
            crate::dsp::Biquad::butterworth_highpass(rate, 180.0).expect("supported spatial rate");
        let mut field = Self {
            rate,
            strength: 0.0,
            target: false,
            lead: Delay::new((rate / 1000) as usize),
            late: Delay::new((rate as f32 * 0.080) as usize),
            diffuser: Diffuser::new(),
            gates: [Gate {
                gain: 1.0,
                ..Gate::default()
            }; B],
            routes: Vec::new(),
            head: None,
            minimum_gate: 1.0,
            late_filters: [highpass; B],
            clean_late_filter: highpass,
        };
        field.orient(solver, None);
        field
    }
    pub fn set_enabled(&mut self, enabled: bool, immediate: bool) {
        if enabled && !self.target && self.strength == 0.0 {
            self.reset();
        }
        let clear = immediate && !enabled && (self.target || self.strength > 0.0);
        self.target = enabled;
        if immediate {
            self.strength = if enabled { 1.0 } else { 0.0 };
        }
        if clear {
            self.reset();
        }
    }
    pub fn active(&self) -> bool {
        self.target || self.strength > 0.0
    }
    pub fn orient(&mut self, solver: &VbapSolver, head: Option<[f32; 4]>) {
        if self.head == head && self.routes.len() == solver.bus_count() {
            return;
        }
        self.head = head;
        self.routes.clear();
        for speaker in vbap::speakers(solver.layout()) {
            let p = crate::static_bed_position(speaker.azimuth, speaker.elevation);
            let p = spatial::head_relative_adm(p, head);
            self.routes.push(
                solver
                    .pan(p, 0.0)
                    .into_iter()
                    .enumerate()
                    .filter(|(_, g)| *g != 0.0)
                    .collect(),
            );
        }
    }
    pub fn process(
        &mut self,
        main: &mut [f32; B],
        reflections: &mut [f32; B],
        envelopment: [f32; B],
        levels: &[f32; B],
    ) -> bool {
        let goal = if self.target { 1.0 } else { 0.0 };
        self.strength += (goal - self.strength).clamp(
            -1.0 / (self.rate as f32 * 0.020),
            1.0 / (self.rate as f32 * 0.020),
        );
        if (goal - self.strength).abs() < 1e-6 {
            self.strength = goal;
        }
        let mut desired = [0.0; B];
        for i in 0..self.routes.len() {
            let sum = main[i] + reflections[i];
            let x = if sum.is_finite() { sum } else { 0.0 };
            let gate = self.gates[i].next(x, self.rate);
            self.minimum_gate = self.minimum_gate.min(gate);
            desired[i] = x * gate;
        }
        let early = self.lead.next(desired);
        let mut late_input = [0.0; B];
        for i in 0..self.routes.len() {
            let x = if envelopment[i].is_finite() {
                envelopment[i]
            } else {
                0.0
            };
            let y = self.late_filters[i].process(x);
            if y.is_finite() {
                late_input[i] = y;
            } else {
                self.late_filters[i] = self.clean_late_filter;
            }
        }
        let late = self.diffuser.process_inputs(self.late.next(late_input));
        let s = self.strength;
        if s == 0.0 {
            return false;
        }
        let mut active = false;
        for i in 0..self.routes.len() {
            // The main kernel already contains its residual. Subtract the original
            // bed residual excitation and replace it with the protected delayed send.
            let correction = (early[i] - main[i] - reflections[i]) * s;
            reflections[i] += correction;
            active |= reflections[i] != 0.0;
        }
        for (world, route) in self.routes.iter().enumerate() {
            let x = late[world] * 0.10 * s;
            for &(bus, gain) in route {
                main[bus] += x * gain * levels[bus];
            }
        }
        active
    }
    pub fn reset(&mut self) {
        self.lead.reset();
        self.late.reset();
        self.diffuser.reset();
        self.gates = [Gate {
            gain: 1.0,
            ..Gate::default()
        }; B];
        self.late_filters.fill(self.clean_late_filter);
        self.minimum_gate = 1.0;
    }
    #[cfg(test)]
    pub fn allocated_samples(&self) -> usize {
        (self.lead.data.len() + self.late.data.len()) * B + self.diffuser.allocated_samples()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reflection_send_has_a_real_lead_and_transient_attenuation() {
        let solver = VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        let mut f = Field::new(48000, &solver);
        f.set_enabled(true, true);
        let levels = [1.0; B];
        let mut values = Vec::new();
        for i in 0..200 {
            let mut main = [0.0; B];
            main[0] = if i == 0 { 1.0 } else { 0.0 };
            let mut wet = [0.0; B];
            f.process(&mut main, &mut wet, [0.0; B], &levels);
            values.push(wet[0] + if i == 0 { 1.0 } else { 0.0 });
        }
        assert!(values[..48].iter().all(|x| x.abs() < 1e-7));
        assert!((values[48] - 0.55).abs() < 0.01);
    }
    #[test]
    fn late_field_is_delayed_bounded_and_resettable() {
        let solver = VbapSolver::with_layout(vbap::LayoutId::Sony360Ra13);
        let mut f = Field::new(48000, &solver);
        f.set_enabled(true, true);
        let mut energy = 0.0;
        for i in 0..16000 {
            let (mut m, mut r) = ([0.0; B], [0.0; B]);
            let mut input = [0.0; B];
            if i == 0 {
                input[3] = 1.0;
            }
            f.process(&mut m, &mut r, input, &[1.0; B]);
            let e = m.iter().map(|x| x * x).sum::<f32>();
            if i < 3840 {
                assert_eq!(e, 0.0);
            }
            energy += e;
        }
        assert!(energy > 0.009 && energy <= 0.01001);
        assert!(f.allocated_samples() < 120_000);
        f.reset();
        for _ in 0..5000 {
            let (mut m, mut r) = ([0.0; B], [0.0; B]);
            f.process(&mut m, &mut r, [0.0; B], &[1.0; B]);
            assert!(m.iter().all(|x| *x == 0.0));
        }
    }
    #[test]
    fn disabled_field_is_sample_exact() {
        let solver = VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        let mut f = Field::new(48000, &solver);
        for i in 0..8000 {
            let mut m = [(i as f32 * 0.07).sin() * 0.1; B];
            let mut r = m.map(|x| -x * 0.3);
            let original = (m, r);
            f.process(&mut m, &mut r, [0.1; B], &[1.0; B]);
            assert_eq!((m, r), original);
        }
    }
    #[test]
    fn reenable_cannot_replay_an_abandoned_late_field() {
        let solver = VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        let mut f = Field::new(48000, &solver);
        f.set_enabled(true, true);
        let (mut m, mut r) = ([0.0; B], [0.0; B]);
        let mut input = [0.0; B];
        input[3] = 1.0;
        f.process(&mut m, &mut r, input, &[1.0; B]);
        f.set_enabled(false, true);
        f.set_enabled(true, true);
        for _ in 0..5000 {
            let (mut m, mut r) = ([0.0; B], [0.0; B]);
            f.process(&mut m, &mut r, [0.0; B], &[1.0; B]);
            assert!(m.iter().all(|x| *x == 0.0));
        }
    }
}
