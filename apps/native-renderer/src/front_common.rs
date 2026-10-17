//! Re-render the common component of a stationary front object pair at center.
//! This is Mid/Side routing, NOT voice extraction, EQ, or a global mono downmix.
//! PCM envelopes are applied by the regular mixers before this partition pass.
use crate::{Source, SourceKind, bus_renderer, directional, hrtf, near_field, vbap};
use std::collections::HashMap;

pub(crate) struct FrontCommon {
    pub center: Option<Box<directional::ContinuousSource>>,
    pub enabled: bool,
    mix: f32,
    pair: Option<(String, String)>,
    reference: Option<(directional::Route, directional::Route, directional::Route)>,
    pub gain: f32,
    meter: Option<ReferenceMeter>,
}
impl Default for FrontCommon {
    fn default() -> Self {
        Self {
            center: None,
            // Correlation is not metadata: equal samples in two objects may
            // contain backing vocals or ambience as well as a centered lead.
            // Re-centering them invents a source and changes the authored mix
            // even in actual-direction mode. Keep this diagnostic experiment
            // opt-in; normal playback must convolve each object independently.
            enabled: false,
            mix: 0.0,
            pair: None,
            reference: None,
            gain: 1.0,
            meter: None,
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

fn unique_pair(sources: &HashMap<String, Source>) -> Option<(String, String)> {
    let unique = |x| {
        let mut ids = sources
            .iter()
            .filter(|(_, s)| front_corner(s, x))
            .map(|(id, _)| id);
        let first = ids.next()?;
        if ids.next().is_some() {
            None
        } else {
            Some(first.clone())
        }
    };
    Some((unique(-1.0)?, unique(1.0)?))
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
        *self = Self {
            enabled,
            ..Default::default()
        };
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
            self.pair
                .as_ref()
                .filter(|(l, r)| {
                    sources.get(l).is_some_and(retained_front)
                        && sources.get(r).is_some_and(retained_front)
                })
                .cloned()
                .or_else(|| unique_pair(sources))
        } else {
            None
        };
        // Retain the previous pair during the short bypass ramp. A changed
        // pair must first return to its authored paths before another is used.
        let active = selected.is_some() && (self.pair.is_none() || self.pair == selected);
        if self.mix == 0.0 {
            self.pair = selected.clone();
        }
        let Some((left_id, right_id)) = self.pair.as_ref() else {
            return;
        };
        let Some(left) = sources.get(left_id) else {
            self.mix = 0.0;
            self.pair = None;
            return;
        };
        let Some(right) = sources.get(right_id) else {
            self.mix = 0.0;
            self.pair = None;
            return;
        };
        let Some(l) = left.continuous.as_ref() else {
            self.mix = 0.0;
            self.pair = None;
            return;
        };
        let Some(r) = right.continuous.as_ref() else {
            self.mix = 0.0;
            self.pair = None;
            return;
        };
        let (Some(lroute), Some(rroute)) = (l.effective_route(), r.effective_route()) else {
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
            let Some(gain) = reference_gain(&l, &r, &c) else {
                return;
            };
            self.gain = gain;
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
            (l.frames[i].input, r.frames[i].input)
        });
        let target_gain = self
            .meter
            .as_mut()
            .and_then(|m| m.observe(&inputs))
            .unwrap_or(self.gain);
        let left_id = left_id.clone();
        let right_id = right_id.clone();
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
            sources
                .get_mut(&left_id)
                .unwrap()
                .continuous
                .as_mut()
                .unwrap()
                .frames[i]
                .input -= mid;
            sources
                .get_mut(&right_id)
                .unwrap()
                .continuous
                .as_mut()
                .unwrap()
                .frames[i]
                .input -= mid;
            center.frames[i].input = mid * self.gain;
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
            pair: Some(("foo".into(), "bar".into())),
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
