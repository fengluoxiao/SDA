//! Re-render the common component of a stationary front object pair at center.
//! Mid/Side routing with optional common-direct presence; not voice extraction.
//! The opt-in presence control leaves original side and reflection sends unchanged.
//! PCM envelopes are applied by the regular mixers before this partition pass.
use crate::{Source, SourceKind, bus_renderer, directional, dsp, hrtf, near_field, vbap};
use std::collections::HashMap;

// Complementary, causal split: bass keeps its authored front routes; only the
// upper common component is re-centered for the accepted clarity presentation.
#[derive(Clone, Copy)]
struct CommonBandSplit { a: f32, state: f32 }
impl CommonBandSplit {
    fn new(rate: u32) -> Self {
        let g = (std::f32::consts::PI * 500. / rate.max(1) as f32).tan();
        Self { a: g / (1. + g), state: 0. }
    }
    fn reset(&mut self) { self.state = 0.; }
    fn high(&mut self, input: f32) -> f32 {
        let v = (input - self.state) * self.a;
        let low = v + self.state;
        self.state = low + v;
        if input == 0. && self.state.abs() < 1e-30 { self.state = 0.; }
        input - low
    }
}

fn reference_with_presence(center: (Vec<f32>, Vec<f32>), presence: Option<dsp::Biquad>, rate: u32) -> (Vec<f32>, Vec<f32>) {
    let Some(presence) = presence else { return center; };
    let apply = |ir: Vec<f32>| {
        let mut filter = presence;
        filter.reset();
        ir.into_iter().chain(std::iter::repeat_n(0., (rate / 100).max(1) as usize))
            .map(|x| filter.process(x)).collect()
    };
    (apply(center.0), apply(center.1))
}

pub(crate) struct FrontCommon {
    pub center: Option<Box<directional::ContinuousSource>>,
    pub enabled: bool,
    mix: f32,
    pair: Option<(Vec<String>, Vec<String>)>,
    /// Group co-located front sources for the accepted shared presentation.
    pub grouped: bool,
    reference: Option<(directional::Route, directional::Route, directional::Route)>,
    pub gain: f32,
    meter: Option<ReferenceMeter>,
    split: CommonBandSplit,
    presence: Option<dsp::Biquad>,
    presence_db: f32,
    presence_rate: u32,
}
impl Default for FrontCommon {
    fn default() -> Self {
        Self {
            center: None,
            // Correlation is not metadata: equal samples in two objects may
            // contain backing vocals or ambience as well as a centered lead.
            // Re-centering changes the authored common component's direction.
            // Engine enables the listener-approved grouped policy only for the
            // shared HRTF family with spatial enhancement enabled.
            enabled: false,
            mix: 0.0,
            pair: None,
            grouped: false,
            reference: None,
            gain: 1.0,
            meter: None,
            split: CommonBandSplit::new(48000),
            presence: None,
            presence_db: 0.0,
            presence_rate: 0,
        }
    }
}

// Only unambiguous stationary front-corner objects start a pair. Other objects,
// beds, diffuse/extended objects, authored distance and exclusion zones retain
// their original rendering. No decoder object IDs or song-specific gain.
fn front_corner(source: &Source, x: f32) -> bool {
    source.kind == SourceKind::Object
        && !source.muted
        && source.remove_at.is_none()
        && source.motion.is_none()
        && source.distance_m.is_none()
        && source.zone_exclusion.is_empty()
        && source.lfe_gain == 0.0
        && source.lfe_target == 0.0
        && source.continuous_active
        && source.continuous_mix == 1.0
        && source.direct.is_none()
        && source
            .position
            .iter()
            .zip([x, 1.0, 0.0])
            .all(|(a, b)| (*a - b).abs() < 0.001)
        && source
            .continuous
            .as_ref()
            .and_then(|c| c.effective_route())
            .is_some_and(|r| {
                let d = r.0;
                d.width == 0.0
                    && d.height == 0.0
                    && d.depth == 0.0
                    && d.diffuse == 0.0
                    && d.position
                        .iter()
                        .zip([x, 1.0, 0.0])
                        .all(|(a, b)| (*a - b).abs() < 0.001)
            })
}
// Retain an established pair through authored motion inside the front plane.
// Its differential component still follows each actual object direction.
fn retained_front(source: &Source) -> bool {
    source.kind == SourceKind::Object
        && !source.muted
        && source.remove_at.is_none()
        && source.distance_m.is_none()
        && source.zone_exclusion.is_empty()
        && source.lfe_gain == 0.0
        && source.lfe_target == 0.0
        && source.continuous_active
        && source.continuous_mix == 1.0
        && source.direct.is_none()
        && source.position[1] > 0.1
        && source.position[2].abs() < 0.05
        && source
            .continuous
            .as_ref()
            .and_then(|c| c.effective_route())
            .is_some_and(|r| {
                let d = r.0;
                d.width == 0.0
                    && d.height == 0.0
                    && d.depth == 0.0
                    && d.diffuse == 0.0
                    && d.position[1] > 0.1
                    && d.position[2].abs() < 0.05
            })
}

fn unique_pair(
    sources: &HashMap<String, Source>,
    grouped: bool,
) -> Option<(Vec<String>, Vec<String>)> {
    let group = |x| {
        let mut ids: Vec<_> = sources
            .iter()
            .filter(|(_, s)| front_corner(s, x))
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        if ids.is_empty() || (!grouped && ids.len() != 1) {
            return None;
        }
        let route = sources[&ids[0]].continuous.as_ref()?.effective_route()?;
        // Group only identical complete render routes. Arbitrarily close or
        // diffuse/moving sources must not silently become a stereo pair.
        if ids.iter().any(|id| {
            sources[id]
                .continuous
                .as_ref()
                .and_then(|c| c.effective_route())
                != Some(route)
        }) {
            return None;
        }
        Some(ids)
    };
    Some((group(-1.0)?, group(1.0)?))
}

fn group_input(
    sources: &HashMap<String, Source>,
    ids: &[String],
) -> Option<(
    directional::Route,
    [f32; crate::convolution::DEFAULT_PARTITION],
)> {
    let first = sources.get(ids.first()?)?.continuous.as_ref()?;
    let route = first.effective_route()?;
    let mut input = [0.; crate::convolution::DEFAULT_PARTITION];
    for id in ids {
        let source = sources.get(id)?.continuous.as_ref()?;
        if source.effective_route()? != route {
            return None;
        }
        for (out, frame) in input.iter_mut().zip(&source.frames) {
            *out += frame.input;
        }
    }
    Some((route, input))
}

/// Broadband reference normalization, derived from the actual HRTF kernels.
/// The pair receives M twice; center receives M once. No song-window fitting,
/// per-band correction, or signal-dependent automatic gain control is used.
fn reference_gain(
    left: &(Vec<f32>, Vec<f32>),
    right: &(Vec<f32>, Vec<f32>),
    center: &(Vec<f32>, Vec<f32>),
) -> Option<f32> {
    let mut pair_energy = 0.0_f64;
    let mut center_energy = 0.0_f64;
    for (a, b, c) in [
        (&left.0, &right.0, &center.0),
        (&left.1, &right.1, &center.1),
    ] {
        for i in 0..a.len().max(b.len()) {
            let sum = f64::from(a.get(i).copied().unwrap_or(0.0))
                + f64::from(b.get(i).copied().unwrap_or(0.0));
            pair_energy += sum * sum;
        }
        center_energy += c.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    }
    let gain = (pair_energy / center_energy).sqrt() as f32;
    (gain.is_finite() && (0.25..=4.0).contains(&gain)).then_some(gain)
}

/// Long-term common-input PSD weights the two reference transfer energies.
/// No band of the signal is altered: this estimates only a scalar group level.
/// Silence does not change the estimate; seek/new program clears the history.
struct ReferenceMeter {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    buffer: Vec<rustfft::num_complex::Complex<f32>>,
    scratch: Vec<rustfft::num_complex::Complex<f32>>,
    pair_power: Vec<f64>,
    center_power: Vec<f64>,
    pair_sum: f64,
    center_sum: f64,
}
impl ReferenceMeter {
    fn new(l: &(Vec<f32>, Vec<f32>), r: &(Vec<f32>, Vec<f32>), c: &(Vec<f32>, Vec<f32>)) -> Self {
        use rustfft::num_complex::Complex;
        let n = 4096
            .max(l.0.len())
            .max(r.0.len())
            .max(c.0.len())
            .next_power_of_two();
        let fft = rustfft::FftPlanner::new().plan_fft_forward(n);
        let power = |a: &(Vec<f32>, Vec<f32>), b: Option<&(Vec<f32>, Vec<f32>)>| {
            let mut power = vec![0.0_f64; n];
            for ear in 0..2 {
                let av = if ear == 0 { &a.0 } else { &a.1 };
                let bv = b.map(|v| if ear == 0 { &v.0 } else { &v.1 });
                let mut data = vec![Complex::new(0.0, 0.0); n];
                for (i, d) in data.iter_mut().enumerate() {
                    d.re = av.get(i).copied().unwrap_or(0.0)
                        + bv.and_then(|v| v.get(i)).copied().unwrap_or(0.0);
                }
                fft.process(&mut data);
                for (p, d) in power.iter_mut().zip(data) {
                    *p += f64::from(d.norm_sqr());
                }
            }
            power
        };
        let pair_power = power(l, Some(r));
        let center_power = power(c, None);
        let scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        Self {
            fft,
            buffer: vec![Complex::new(0.0, 0.0); n],
            scratch,
            pair_power,
            center_power,
            pair_sum: 0.0,
            center_sum: 0.0,
        }
    }
    fn observe(
        &mut self,
        inputs: &[(f32, f32); crate::convolution::DEFAULT_PARTITION],
    ) -> Option<f32> {
        self.buffer
            .fill(rustfft::num_complex::Complex::new(0.0, 0.0));
        for (i, (l, r)) in inputs.iter().enumerate() {
            let window = 0.5
                - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (inputs.len() - 1) as f32).cos();
            self.buffer[i].re = (l + r) * 0.5 * window;
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        for (i, z) in self.buffer.iter().enumerate() {
            let p = f64::from(z.norm_sqr());
            self.pair_sum += p * self.pair_power[i];
            self.center_sum += p * self.center_power[i];
        }
        let gain = (self.pair_sum / self.center_sum).sqrt() as f32;
        (gain.is_finite() && (0.25..=4.0).contains(&gain)).then_some(gain)
    }
}

impl FrontCommon {
    pub fn reset(&mut self) {
        let enabled = self.enabled;
        let grouped = self.grouped;
        let mut presence = self.presence;
        if let Some(filter) = &mut presence {
            filter.reset();
        }
        let mut split = self.split;
        split.reset();
        let presence_db = self.presence_db;
        let presence_rate = self.presence_rate;
        *self = Self {
            enabled,
            grouped,
            presence,
            split,
            presence_db,
            presence_rate,
            ..Default::default()
        };
    }

    /// Common-direct presentation tone, applied before the central direct HRTF.
    /// It is never applied to the whole output, side difference, or rear layer.
    pub(crate) fn set_presence(&mut self, rate: u32, db: f32) -> Result<(), String> {
        if !db.is_finite() || !(0.0..=3.0).contains(&db) {
            return Err("front-common presence must be finite and between 0 and +3 dB".into());
        }
        let filter = dsp::Biquad::peaking(rate, 3000., 0.9, db)?;
        if self.presence_db == db && self.presence_rate == rate {
            return Ok(());
        }
        if self.presence_rate != rate { self.split = CommonBandSplit::new(rate); }
        self.reference = None;
        self.meter = None;
        self.presence = (db != 0.).then_some(filter);
        self.presence_db = db;
        self.presence_rate = rate;
        Ok(())
    }
    pub(crate) fn presence_db(&self) -> f32 {
        self.presence_db
    }
    fn drain_presence(&mut self) {
        self.split.reset();
        if let Some(filter) = &mut self.presence {
            if let Some(center) = &mut self.center {
                for frame in &mut center.frames {
                    frame.input = filter.process(0.);
                }
            } else {
                filter.reset();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        sources: &mut HashMap<String, Source>,
        set: &hrtf::NativeHrtfSet,
        bus: &mut bus_renderer::BusRenderer,
        layout: vbap::LayoutId,
        solver: &vbap::VbapSolver,
        head: Option<[f32; 4]>,
        near: near_field::Settings,
        allowed: bool,
    ) {
        let selected = if self.enabled && allowed {
            if self.grouped {
                unique_pair(sources, true)
            } else {
                self.pair
                    .as_ref()
                    .filter(|(l, r)| {
                        l.iter()
                            .chain(r)
                            .all(|id| sources.get(id).is_some_and(retained_front))
                    })
                    .cloned()
                    .or_else(|| unique_pair(sources, false))
            }
        } else {
            None
        };
        // Retain the previous pair during the short bypass ramp. A changed
        // pair must first return to its authored paths before another is used.
        let active = selected.is_some() && (self.pair.is_none() || self.pair == selected);
        if self.mix == 0.0 {
            self.pair = selected.clone();
        }
        let Some((left_ids, right_ids)) = self.pair.as_ref() else {
            self.drain_presence();
            return;
        };
        let Some((lroute, linput)) = group_input(sources, left_ids) else {
            self.mix = 0.;
            self.pair = None;
            self.reference = None;
            self.meter = None;
            self.drain_presence();
            return;
        };
        let Some((rroute, rinput)) = group_input(sources, right_ids) else {
            self.mix = 0.;
            self.pair = None;
            self.reference = None;
            self.meter = None;
            self.drain_presence();
            return;
        };
        let center_direction = directional::Direction {
            position: [0.0, 1.0, 0.0],
            head,
            width: 0.0,
            height: 0.0,
            depth: 0.0,
            diffuse: 0.0,
            horizontal_only: false,
        };
        let center_gains = bus_renderer::route(solver, center_direction.position, head, 0.0);
        let center_route = (
            center_direction,
            layout,
            center_gains,
            [0.0; vbap::MAX_BUS_COUNT],
        );
        // Normalize in the neutral listener frame. Turning the head changes
        // the HRTF, not a per-block loudness AGC.
        let neutral = |mut route: directional::Route| {
            route.0.head = None;
            route.2 = bus_renderer::route(solver, route.0.position, None, 0.0);
            route.3 = [0.0; vbap::MAX_BUS_COUNT];
            route
        };
        let key = (neutral(lroute), neutral(rroute), neutral(center_route));
        if self.reference.is_none() {
            let kernel = |r: directional::Route| set.directional_dry_compact(r.0, r.1, r.2, r.3);
            let result = kernel(key.0)
                .and_then(|l| kernel(key.1).and_then(|r| kernel(key.2).map(|c| (l, r, c))));
            let Ok((l, r, c)) = result else {
                return;
            };
            // Normalize the transfer that is actually rendered, including the
            // presence filter, instead of compensating its extra energy globally.
            let c = reference_with_presence(c, self.presence, self.presence_rate);
            let Some(gain) = reference_gain(&l, &r, &c) else {
                return;
            };
            if self.mix == 0. { self.gain = gain; }
            self.meter = Some(ReferenceMeter::new(&l, &r, &c));
            self.reference = Some(key);
        }
        if self.center.is_none() {
            let Ok(mut center) = directional::ContinuousSource::new(set) else {
                return;
            };
            center.perf_id = "front-common-center".into();
            self.center = Some(Box::new(center));
        }
        let inputs = std::array::from_fn::<_, { crate::convolution::DEFAULT_PARTITION }, _>(|i| {
            let high = self.split.high((linput[i] + rinput[i]) * 0.5);
            (high, high)
        });
        let target_gain = self
            .meter
            .as_mut()
            .and_then(|m| m.observe(&inputs))
            .unwrap_or(self.gain);
        let left_ids = left_ids.clone();
        let right_ids = right_ids.clone();
        let center = self.center.as_mut().unwrap();
        center.schedule(
            center_route.0,
            center_route.1,
            center_route.2,
            center_route.3,
        );
        let near_targets = if near.enabled {
            near_field::gains([0.0, 1.0, 0.0], head, near)
        } else {
            [1.0; 2]
        };
        let target = if active { 1.0 } else { 0.0 };
        for (i, (l, r)) in inputs.into_iter().enumerate() {
            self.mix += (target - self.mix).clamp(-1.0 / 512.0, 1.0 / 512.0);
            // One broadband group gain, slew-limited to avoid audible pumping.
            // Long-term input-weighted HRTF energy replaces clip-specific gain.
            self.gain += (target_gain - self.gain) / 48000.0;
            let mid = (l + r) * 0.5 * self.mix;
            // Remove the common excitation once per physical front group.
            // Splitting one co-located object into several objects must not
            // change the group's transfer or multiply its center contribution.
            for ids in [&left_ids, &right_ids] {
                let contribution = mid / ids.len() as f32;
                for id in ids {
                    sources
                        .get_mut(id)
                        .unwrap()
                        .continuous
                        .as_mut()
                        .unwrap()
                        .frames[i]
                        .input -= contribution;
                }
            }
            let direct_mid = mid * self.gain;
            center.frames[i].input = self
                .presence
                .as_mut()
                .map_or(direct_mid, |filter| filter.process(direct_mid));
            center.frames[i].near = near_targets;
            // Replace rather than add another reverberant excitation. These
            // canonical front points all have unit proximity-room weighting.
            if mid != 0.0 {
                bus.add_reflections(-mid, &lroute.2, i);
                bus.add_reflections(-mid, &rroute.2, i);
                bus.add_reflections(mid * self.gain, &center_gains, i);
            }
        }
        if self.mix == 0.0 {
            self.pair = None;
            self.reference = None;
            self.meter = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_uses_coherent_pair_not_independent_energy() {
        let l = (vec![1.0, 0.0], vec![0.0, 1.0]);
        let r = (vec![0.0, 1.0], vec![1.0, 0.0]);
        let c = (vec![1.0, 1.0], vec![1.0, 1.0]);
        assert_eq!(reference_gain(&l, &r, &c), Some(1.0));
        assert_eq!(reference_gain(&c, &c, &c), Some(2.0));
        assert_eq!(reference_gain(&l, &r, &(vec![0.0], vec![0.0])), None);
    }
    #[test]
    fn mid_side_replacement_preserves_difference_and_rejects_double_gain() {
        for (l, r) in [(0.4_f32, 0.4_f32), (0.4, -0.4), (0.7, 0.1), (0.0, 0.2)] {
            let mid = (l + r) * 0.5;
            let side_l = l - mid;
            let side_r = r - mid;
            assert!((side_l + side_r).abs() < 1e-6_f32);
            assert!((side_l - side_r - (l - r)).abs() < 1e-6_f32);
            assert!((side_l + mid - l).abs() < 1e-6_f32);
            assert!((side_r + mid - r).abs() < 1e-6_f32);
        }
    }
    #[test]
    fn reset_clears_pair_gain_ramp_but_keeps_bypass_preference() {
        let mut s = FrontCommon {
            enabled: false,
            mix: 1.0,
            pair: Some((vec!["foo".into()], vec!["bar".into()])),
            gain: 3.0,
            ..Default::default()
        };
        s.reset();
        assert!(!s.enabled);
        assert_eq!(s.mix, 0.0);
        assert!(s.pair.is_none());
        assert!(s.center.is_none());
    }
    fn engine(enabled: bool, kind: &str, side: bool, extras: usize) -> crate::Engine {
        let mut e = crate::Engine::new(48000, 2);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf-dense/hrtf-set.json");
        e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(), 0.04)
            .unwrap();
        e.front_common.enabled = enabled;
        e.set_directional_hrtf(true);
        e.set_program_codec(kind.into());
        e.set_direct_objects(true).unwrap();
        e.output_active = true;
        e.paused = false;
        e.direct_mix = 1.0;
        for i in 0..2 + extras {
            let id = format!("arbitrary-name-{i}");
            let position = match i {
                0 => [-1.0, 1.0, 0.0],
                1 => [1.0, 1.0, 0.0],
                _ => [0.0, -1.0, 0.0],
            };
            let mut src = Source {
                kind: SourceKind::Object,
                position,
                gain: 1.0,
                target_gain: 1.0,
                availability: 1.0,
                availability_target: 1.0,
                ..Default::default()
            };
            let samples: Vec<_> = (0..12288)
                .map(|n| {
                    if i >= 2 {
                        0.0
                    } else {
                        let v = (n as f32 * 0.173).sin() * 0.01;
                        if side && i == 1 { -v } else { v }
                    }
                })
                .collect();
            src.samples.write(0, 0, &samples);
            e.sources.insert(id.clone(), src);
            e.route_source_now(&id, 0).unwrap();
        }
        e
    }
    fn rendered(e: &mut crate::Engine, chunk: usize) -> Vec<f32> {
        let mut out = vec![0.0; 12288 * 2];
        for block in out.chunks_mut(chunk * 2) {
            e.render_into(block, 2);
        }
        out
    }
    #[test]
    fn default_actual_direction_keeps_correlated_objects_on_authored_routes() {
        for manifest in [
            "../web/public/hrtf-dense/hrtf-set.json",
            "../mobile/assets/hrtf-mobile-direct/hrtf-set.json",
        ] {
            for extras in [0, 8] {
                let make = |bypass: bool| {
                    let mut e = engine(false, "eac3", false, extras);
                    // Exercise the shipped default, not a diagnostic opt-in.
                    e.front_common = FrontCommon::default();
                    if bypass {
                        e.front_common.enabled = false;
                    }
                    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(manifest);
                    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(), 0.0)
                        .unwrap();
                    e
                };
                let expected = rendered(&mut make(true), 1536);
                let mut actual = make(false);
                let output = rendered(&mut actual, 173);
                let error = output
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f32, f32::max);
                assert!(
                    error < 1e-7,
                    "actual-direction mode recentered correlated object PCM: {manifest}, extras={extras}, error={error}"
                );
                assert!(
                    actual.front_common.center.is_none(),
                    "must not invent a center object"
                );
                assert_eq!(actual.peak_guard.diagnostic_gain(), 1.0);
            }
        }
    }

    #[test]
    fn quiet_height_and_front_object_contributions_are_additive_by_default() {
        for manifest in [
            "../web/public/hrtf-dense/hrtf-set.json",
            "../mobile/assets/hrtf-mobile-direct/hrtf-set.json",
        ] {
            // Ten declarations also exercise the parallel object mixer.
            let make = |group: usize| {
                let mut e = engine(false, "eac3", false, 8);
                e.front_common = FrontCommon::default();
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(manifest);
                e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(), 0.0)
                    .unwrap();
                for i in 0..10 {
                    let id = format!("arbitrary-name-{i}");
                    let source = e.sources.get_mut(&id).unwrap();
                    if i == 2 || i == 3 {
                        source.position = [if i == 2 { -1.0 } else { 1.0 }, 1.0, 1.0];
                    }
                    let samples: Vec<_> = (0..12288)
                        .map(|n| {
                            let included = group == 0
                                || (group == 1 && i < 2)
                                || (group == 2 && i == 2)
                                || (group == 3 && i == 3);
                            if !included || i >= 4 {
                                0.0
                            } else if i < 2 {
                                (n as f32 * 0.173).sin() * 0.01
                            }
                            // Deliberately below the UI activity threshold. A quiet
                            // harmony must not be dropped from the audio mix.
                            else {
                                (n as f32 * 0.093 + i as f32).sin() * 0.0001
                            }
                        })
                        .collect();
                    source.samples.write(0, 0, &samples);
                    e.route_source_now(&id, 0).unwrap();
                }
                e
            };
            let outputs: Vec<_> = (0..4)
                .map(|group| rendered(&mut make(group), 173))
                .collect();
            let mut max_error = 0.0_f32;
            for n in 0..outputs[0].len() {
                max_error = max_error
                    .max((outputs[0][n] - outputs[1][n] - outputs[2][n] - outputs[3][n]).abs());
            }
            assert!(
                max_error < 1e-7,
                "object summation changed authored contributions: {manifest}, error={max_error}"
            );
            for quiet in &outputs[2..] {
                let energy: f64 = quiet.iter().map(|v| f64::from(*v).powi(2)).sum();
                assert!(energy > 1e-6, "quiet elevated object was gated: {manifest}");
            }
        }
    }

    #[test]
    fn pure_side_is_bit_identical_in_general_and_parallel_mixers() {
        for extras in [0, 8] {
            let a = rendered(&mut engine(false, "eac3", true, extras), 1536);
            let mut e = engine(true, "eac3", true, extras);
            let b = rendered(&mut e, 1536);
            assert_eq!(a, b, "pure Side must not move, extras={extras}");
            assert!(
                e.front_common.center.is_some(),
                "pair must be detected without object IDs"
            );
            if extras > 0 {
                assert!(e.fast_object_blocks > 0);
            }
        }
    }
    #[test]
    fn common_uses_center_and_callback_chunking_does_not_change_output() {
        let original = rendered(&mut engine(false, "eac3", false, 0), 1536);
        let mut e = engine(true, "eac3", false, 0);
        let center = rendered(&mut e, 1536);
        assert!(e.front_common.center.is_some());
        assert!(
            center
                .iter()
                .zip(&original)
                .any(|(a, b)| (a - b).abs() > 1e-5)
        );
        let partial = rendered(&mut engine(true, "eac3", false, 0), 173);
        let delta = center
            .iter()
            .zip(&partial)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            delta < 1e-7,
            "partition timing changed across callbacks: max delta={delta}"
        );
        assert!(center.iter().all(|v| v.is_finite() && v.abs() < 1.0));
        assert_eq!(e.peak_guard.diagnostic_gain(), 1.0);
        println!("dense front-common reference gain={}", e.front_common.gain);
    }
    #[test]
    fn non_dolby_and_ambiguous_front_pairs_are_unchanged() {
        let a = rendered(&mut engine(false, "mpegh", false, 0), 1536);
        let mut e = engine(true, "mpegh", false, 0);
        let b = rendered(&mut e, 1536);
        assert_eq!(a, b);
        assert!(e.front_common.center.is_none());
        let mut e = engine(true, "eac3", false, 1);
        e.sources.get_mut("arbitrary-name-2").unwrap().position = [-1.0, 1.0, 0.0];
        e.route_source_now("arbitrary-name-2", 0).unwrap();
        rendered(&mut e, 1536);
        assert!(e.front_common.center.is_none());
    }
    #[test]
    fn session_reset_discards_center_tail_without_discarding_bypass() {
        let mut e = engine(true, "eac3", false, 0);
        rendered(&mut e, 1536);
        assert!(e.front_common.center.is_some());
        e.reset_session(48000);
        assert!(e.front_common.center.is_none());
        assert!(e.front_common.pair.is_none());
        assert_eq!(e.block_offset, 0);
        assert_eq!(e.sample_pos, 48000);
    }

    fn grouped_engine(kind: &str, side: bool, split: bool, extras: usize) -> crate::Engine {
        let mut e = engine(true, kind, side, extras);
        e.front_common.grouped = true;
        if split {
            for (i, position) in [[-1., 1., 0.], [1., 1., 0.]].into_iter().enumerate() {
                let original = e.sources.get_mut(&format!("arbitrary-name-{i}")).unwrap();
                original.gain = 0.5;
                original.target_gain = 0.5;
                let id = format!("unrelated-split-name-{i}");
                let mut source = Source {
                    kind: SourceKind::Object,
                    position,
                    gain: 0.5,
                    target_gain: 0.5,
                    availability: 1.,
                    availability_target: 1.,
                    ..Default::default()
                };
                let samples: Vec<_> = (0..12288)
                    .map(|n| {
                        let v = (n as f32 * 0.173).sin() * 0.01;
                        if side && i == 1 { -v } else { v }
                    })
                    .collect();
                source.samples.write(0, 0, &samples);
                e.sources.insert(id.clone(), source);
                e.route_source_now(&id, 0).unwrap();
            }
        }
        e
    }
    #[test]
    fn grouped_front_is_invariant_to_co_located_object_splitting_in_both_mixers() {
        for extras in [0, 8] {
            let mut single = grouped_engine("eac3", false, false, extras);
            let mut split = grouped_engine("eac3", false, true, extras);
            let a = rendered(&mut single, 1536);
            let b = rendered(&mut split, 1536);
            assert!(single.front_common.center.is_some());
            assert!(split.front_common.center.is_some());
            let delta = a
                .iter()
                .zip(&b)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(
                delta < 1e-7,
                "split changed front transfer: extras={extras}, delta={delta}"
            );
            assert_eq!(split.peak_guard.diagnostic_gain(), 1.);
        }
    }
    #[test]
    fn grouped_front_preserves_pure_difference_and_callback_timing() {
        for extras in [0, 8] {
            let mut original = grouped_engine("eac3", true, true, extras);
            original.front_common.enabled = false;
            let a = rendered(&mut original, 1536);
            let mut actual = grouped_engine("eac3", true, true, extras);
            let b = rendered(&mut actual, 1536);
            // Independent engines use separately seeded source maps; a four-
            // source sum may round differently without changing its transfer.
            let delta = a
                .iter()
                .zip(&b)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(
                delta < 1e-7,
                "pure difference changed: extras={extras}, delta={delta}"
            );
            assert!(actual.front_common.center.is_some());
        }
        let a = rendered(&mut grouped_engine("eac3", false, true, 0), 1536);
        let b = rendered(&mut grouped_engine("eac3", false, true, 0), 173);
        assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-7));
    }
    #[test]
    fn grouped_front_is_opt_in_and_keeps_reset_preference() {
        assert!(!FrontCommon::default().grouped);
        let mut e = grouped_engine("eac3", false, true, 0);
        let fixed = rendered(&mut e, 1536);
        let mut original = grouped_engine("eac3", false, true, 0);
        original.front_common.enabled = false;
        let baseline = rendered(&mut original, 1536);
        assert!(
            fixed
                .iter()
                .zip(baseline)
                .any(|(a, b)| (a - b).abs() > 1e-5)
        );
        e.reset_session(48000);
        assert!(e.front_common.grouped && e.front_common.enabled);
        assert!(e.front_common.center.is_none());
    }

    #[test]
    fn common_presence_is_exact_bypass_at_zero_and_has_bounded_frequency_response() {
        let mut s = FrontCommon::default();
        s.set_presence(48000, 0.).unwrap();
        assert!(s.presence.is_none());
        for invalid in [-1., 3.1, f32::NAN, f32::INFINITY] {
            assert!(s.set_presence(48000, invalid).is_err());
        }
        let response = |rate: u32, frequency: f32, db: f32| {
            let mut s = FrontCommon::default();
            s.set_presence(rate, db).unwrap();
            let mut input = 0f64;
            let mut output = 0f64;
            for i in 0..rate / 2 {
                let x = (i as f32 * std::f32::consts::TAU * frequency / rate as f32).sin();
                let y = s.presence.as_mut().unwrap().process(x);
                if i >= rate / 4 {
                    input += f64::from(x).powi(2);
                    output += f64::from(y).powi(2);
                }
            }
            10. * (output / input).log10()
        };
        for rate in [32000, 48000, 96000] {
            for db in [1.5, 3.] {
                assert!((response(rate, 3000., db) - f64::from(db)).abs() < 0.01);
                assert!(response(rate, 100., db).abs() < 0.05);
                assert!(response(rate, 10000., db).abs() < 0.5);
            }
        }
    }
    #[test]
    fn common_presence_leaves_pure_difference_unchanged_in_both_mixers() {
        for extras in [0, 8] {
            let mut a = grouped_engine("eac3", true, true, extras);
            let mut b = grouped_engine("eac3", true, true, extras);
            b.front_common.set_presence(48000, 3.).unwrap();
            let a = rendered(&mut a, 1536);
            let b = rendered(&mut b, 1536);
            let error = a
                .iter()
                .zip(&b)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(error < 1e-7, "presence changed pure difference: {error}");
        }
    }
    #[test]
    fn repeating_presence_setting_does_not_restart_filter_history() {
        let mut s = FrontCommon::default();
        s.set_presence(48000, 3.).unwrap();
        s.presence.as_mut().unwrap().process(0.7);
        let mut expected = s.presence.unwrap();
        let output = expected.process(0.2);
        s.set_presence(48000, 3.).unwrap();
        assert_eq!(s.presence.as_mut().unwrap().process(0.2), output);
    }
    #[test]
    fn common_presence_reset_keeps_tone_configuration_but_discards_filter_history() {
        let mut s = FrontCommon::default();
        s.set_presence(48000, 3.).unwrap();
        s.presence.as_mut().unwrap().process(1.);
        s.reset();
        assert_eq!(s.presence_db(), 3.);
        assert_eq!(s.presence.as_mut().unwrap().process(0.), 0.);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        let set = hrtf::NativeHrtfSet::load_calibrated(&path).unwrap();
        s.center = Some(Box::new(directional::ContinuousSource::new(&set).unwrap()));
        s.presence.as_mut().unwrap().process(1.);
        s.drain_presence();
        assert!(
            s.center
                .as_ref()
                .unwrap()
                .frames
                .iter()
                .any(|f| f.input != 0.)
        );
        for _ in 0..100 {
            s.drain_presence();
        }
        assert!(
            s.center
                .as_ref()
                .unwrap()
                .frames
                .iter()
                .all(|f| f.input.abs() < 1e-20)
        );
    }

    #[test]
    fn broadband_meter_matches_reference_and_silence_cannot_pump_gain() {
        let l = (vec![1.0], vec![1.0]);
        let r = (vec![0.5], vec![0.5]);
        let c = (vec![1.0], vec![1.0]);
        let mut m = ReferenceMeter::new(&l, &r, &c);
        let zero = [(0.0, 0.0); crate::convolution::DEFAULT_PARTITION];
        assert!(m.observe(&zero).is_none());
        let tone = std::array::from_fn(|i| {
            let x = (i as f32 * 0.173).sin() * 0.1;
            (x, x)
        });
        assert!((m.observe(&tone).unwrap() - 1.5).abs() < 1e-6);
        assert!((m.observe(&zero).unwrap() - 1.5).abs() < 1e-6);
    }
    #[test]
    fn established_pair_survives_front_motion_and_bypass_preserves_clock() {
        let mut e = engine(true, "eac3", false, 0);
        rendered(&mut e, 1536);
        e.sources.get_mut("arbitrary-name-1").unwrap().position = [-1.0, 1.0, 0.0];
        e.route_source_now("arbitrary-name-1", 128).unwrap();
        for src in e.sources.values_mut() {
            src.samples.write(12288, 12288, &vec![0.01; 4096]);
        }
        let before = e.sample_pos;
        let mut out = vec![0.0; 2048 * 2];
        e.render_into(&mut out, 2);
        assert_eq!(
            e.front_common.mix, 1.0,
            "association must survive an authored front move"
        );
        e.front_common.enabled = false;
        e.render_into(&mut out, 2);
        assert_eq!(e.front_common.mix, 0.0);
        assert_eq!(e.sample_pos, before + 4096);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(!e.paused);
    }
}

#[cfg(test)]
mod bass_preservation_tests {
    use super::*;
    #[test]
    fn crossover_keeps_bass_authored_and_retains_the_clarity_band() {
        for rate in [44100, 48000, 96000] {
            for (hz, minimum, maximum) in [(60., 0., 0.13), (3000., 0.97, 1.01)] {
                let mut split = CommonBandSplit::new(rate);
                let mut input = 0f64; let mut output = 0f64;
                for i in 0..rate {
                    let x = (i as f32 * std::f32::consts::TAU * hz / rate as f32).sin();
                    let high = split.high(x);
                    let low = x - high;
                    assert!((low + high - x).abs() < 1e-7);
                    if i >= rate / 2 { input += f64::from(x).powi(2); output += f64::from(high).powi(2); }
                }
                let gain = (output/input).sqrt();
                assert!(gain >= minimum && gain <= maximum, "rate={rate} hz={hz} gain={gain}");
                split.reset(); assert_eq!(split.high(0.), 0.);
            }
        }
    }
    #[test]
    fn presence_filter_is_included_in_the_common_energy_reference() {
        let ir = (vec![1.0], vec![1.0]);
        let p = dsp::Biquad::peaking(48000, 3000., 0.9, 3.).unwrap();
        let center = reference_with_presence(ir.clone(), Some(p), 48000);
        let mut meter = ReferenceMeter::new(&ir, &ir, &center);
        let mut gain = None;
        for block in 0..100 {
            let input = std::array::from_fn(|i| {
                let x = ((block * crate::convolution::DEFAULT_PARTITION + i) as f32 * std::f32::consts::TAU * 3000. / 48000.).sin();
                (x,x)
            });
            gain = meter.observe(&input);
        }
        let expected = 2. / 10f32.powf(3./20.);
        assert!((gain.unwrap()-expected).abs()<0.03, "presence energy was ignored: {:?}", gain);
        assert_eq!(reference_with_presence(ir.clone(), None, 48000), ir);
    }
}
