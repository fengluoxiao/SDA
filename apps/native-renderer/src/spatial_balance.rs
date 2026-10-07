//! Bounded foreground masking compensation for the shared directional renderer.
//! This is a bounded remix, not a correction to authored metadata or an HRTF
//! calibration claim. The accepted scene-balance policy is enabled by default.
use crate::{Source, convolution::DEFAULT_PARTITION};
use std::collections::HashMap;

pub(crate) struct SpatialBalance {
    pub enabled: bool,
    pub gain: f32,
}
impl Default for SpatialBalance {
    fn default() -> Self {
        Self {
            enabled: true,
            gain: 1.0,
        }
    }
}

// World/scene coordinates: head turns must not pump the program level.
fn weights(position: [f32; 3]) -> (f64, f64) {
    let [x, y, z] = position.map(f64::from);
    let radius = (x * x + y * y + z * z).sqrt();
    if !radius.is_finite() || radius < 1e-9 {
        return (0.0, 0.0);
    }
    let height = (z / radius).max(0.0);
    let upper = ((height - 0.25) / 0.25).clamp(0.0, 1.0);
    let front =
        ((y / radius - 0.35) / 0.35).clamp(0.0, 1.0) * (1.0 - (height / 0.35).clamp(0.0, 1.0));
    (front, upper)
}
fn target(front: f64, upper: f64, allowed: bool) -> f32 {
    // No adjustment for a front-only scene, silence, or non-finite telemetry.
    if !allowed || !front.is_finite() || !upper.is_finite() || upper <= 1e-7 || front <= 0.0 {
        return 1.0;
    }
    let dominance_db = 10.0 * (front / upper).log10();
    // At most the 6 dB difference used in the diagnostic; never boost sources.
    let reduction_db = ((dominance_db - 6.0) * 0.5).clamp(0.0, 6.0);
    10.0_f64.powf(-reduction_db / 20.0) as f32
}
impl SpatialBalance {
    pub fn reset(&mut self) {
        self.gain = 1.0;
    }
    pub fn prepare(&mut self, sources: &mut HashMap<String, Source>, allowed: bool, rate: u32) {
        if !self.enabled && self.gain == 1.0 {
            return;
        }
        let eligible = |s: &Source| {
            s.continuous_active
                && s.continuous_mix == 1.0
                && s.direct.is_none()
                && s.zone_exclusion.is_empty()
                && s.lfe_gain == 0.0
                && s.lfe_target == 0.0
        };
        let (mut front, mut upper) = (0.0, 0.0);
        for source in sources.values().filter(|s| eligible(s)) {
            if let Some(c) = &source.continuous {
                let energy = c
                    .frames
                    .iter()
                    .map(|f| f64::from(f.input).powi(2))
                    .sum::<f64>()
                    / DEFAULT_PARTITION as f64;
                let (f, u) = weights(source.position);
                front += energy * f;
                upper += energy * u;
            }
        }
        let goal = target(front, upper, self.enabled && allowed);
        let tau = if goal < self.gain { 0.15 } else { 0.8 };
        let alpha = 1.0 - (-1.0 / (rate.max(1) as f32 * tau)).exp();
        let mut gains = [1.0; DEFAULT_PARTITION];
        for g in &mut gains {
            self.gain += (goal - self.gain) * alpha;
            *g = self.gain;
        }
        if goal == 1.0 && (1.0 - self.gain).abs() < 1e-5 {
            self.gain = 1.0;
        }
        for source in sources.values_mut().filter(|s| eligible(s)) {
            let (weight, _) = weights(source.position);
            if weight == 0.0 {
                continue;
            }
            if let Some(c) = &mut source.continuous {
                for (f, g) in c.frames.iter_mut().zip(gains) {
                    f.input *= 1.0 + (g - 1.0) * weight as f32;
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_enables_accepted_policy_without_initial_attenuation() {
        let s = SpatialBalance::default();
        assert!(s.enabled);
        assert_eq!(s.gain, 1.0);
    }
    #[test]
    fn cardinal_directions_have_finite_symmetric_weights() {
        assert_eq!(weights([0.0, 1.0, 0.0]), (1.0, 0.0));
        for p in [[0.0,-1.0,0.0],[-1.0,0.0,0.0],[1.0,0.0,0.0],[0.0,0.0,-1.0]] {
            assert_eq!(weights(p), (0.0, 0.0));
        }
        for el in -90..=90 {
            for az in -180..=180 {
                let e = (el as f32).to_radians();
                let a = (az as f32).to_radians();
                let p = [a.sin()*e.cos(), a.cos()*e.cos(), e.sin()];
                let w = weights(p);
                assert_eq!(w, weights([-p[0],p[1],p[2]]));
                assert!((0.0..=1.0).contains(&w.0) && (0.0..=1.0).contains(&w.1));
            }
        }
    }
    #[test]
    fn front_only_and_silence_are_untouched() {
        assert_eq!(target(1.0, 0.0, true), 1.0);
        assert_eq!(target(0.0, 0.0, true), 1.0);
        assert_eq!(target(1.0, 1.0, true), 1.0);
        assert_eq!(target(1.0, 0.01, false), 1.0);
        assert_eq!(target(f64::NAN, 1.0, true), 1.0);
    }
    #[test]
    fn attenuation_is_bounded_and_monotonic() {
        let mut previous = 1.0;
        for n in 0..100 {
            let gain = target(10f64.powf(n as f64 / 10.0), 1.0, true);
            assert!(gain <= previous);
            assert!(gain >= 0.50118);
            previous = gain;
        }
    }
    #[test]
    fn geometry_is_mirrored_and_does_not_attenuate_zenith_or_rear() {
        assert_eq!(weights([-1.0, 1.0, 0.0]), weights([1.0, 1.0, 0.0]));
        assert_eq!(weights([0.0, 0.0, 1.0]), (0.0, 1.0));
        assert_eq!(weights([0.0, -1.0, 0.0]).0, 0.0);
        assert_eq!(weights([0.0; 3]), (0.0, 0.0));
    }
    #[test]
    fn prepared_scene_changes_only_front_excitation_and_keeps_positions() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json");
        let set = crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap();
        let make = |position| Source {
            position,
            continuous_active: true,
            continuous_mix: 1.0,
            continuous: Some(Box::new(
                crate::directional::ContinuousSource::new(&set).unwrap(),
            )),
            ..Default::default()
        };
        let mut sources = HashMap::from([
            ("arbitrary-front".into(), make([1.0, 1.0, 0.0])),
            ("arbitrary-upper".into(), make([0.0, 0.0, 1.0])),
        ]);
        let mut balance = SpatialBalance {
            enabled: true,
            ..Default::default()
        };
        for _ in 0..100 {
            for (name, source) in &mut sources {
                for f in &mut source.continuous.as_mut().unwrap().frames {
                    f.input = if name == "arbitrary-front" {
                        0.1
                    } else {
                        0.005
                    };
                }
            }
            balance.prepare(&mut sources, true, 48000);
        }
        assert!(balance.gain >= 0.50118 && balance.gain < 0.502);
        assert_eq!(sources["arbitrary-front"].position, [1.0, 1.0, 0.0]);
        assert!(
            sources["arbitrary-upper"]
                .continuous
                .as_ref()
                .unwrap()
                .frames
                .iter()
                .all(|f| f.input == 0.005)
        );
        let front = &sources["arbitrary-front"]
            .continuous
            .as_ref()
            .unwrap()
            .frames;
        assert!(front.iter().all(|f| f.input >= 0.050118 && f.input < 0.051));
        balance.reset();
        assert_eq!(balance.gain, 1.0);
        assert!(balance.enabled);
    }
}
