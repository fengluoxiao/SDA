//! Experimental low-frequency finite-distance correction, not a measured NF-HRTF.
//! ADM positions are normalized: metres_per_unit is an explicit user mapping.
#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    pub metres_per_unit: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            metres_per_unit: 1.0,
        }
    }
}
impl Settings {
    pub fn valid(self) -> bool {
        self.metres_per_unit.is_finite() && (0.25..=4.0).contains(&self.metres_per_unit)
    }
}

/// Relative spherical-spreading ear gains below one metre, referenced to the
/// same direction at one metre. Unity mean-square gain avoids adding a global
/// inverse-distance volume law. Ear x is negative on the listener's left.
pub fn gains(position: [f32; 3], head: Option<[f32; 4]>, settings: Settings) -> [f32; 2] {
    if !settings.enabled || !settings.valid() || !position.iter().all(|x| x.is_finite()) {
        return [1.0; 2];
    }
    let p = crate::spatial::head_relative_adm(position, head);
    let norm = p.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 1e-6 {
        return [1.0; 2];
    }
    let r = (norm * settings.metres_per_unit).max(0.20);
    if r >= 1.0 {
        return [1.0; 2];
    }
    let lateral = p[0] / norm;
    if lateral.abs() < 1e-7 {
        return [1.0; 2];
    }
    let ear_gain = |ear: f32| {
        let near = (r * r + ear * ear - 2.0 * r * ear * lateral).sqrt();
        let reference = (1.0 + ear * ear - 2.0 * ear * lateral).sqrt();
        r * reference / near
    };
    let g = [ear_gain(-0.0875), ear_gain(0.0875)];
    let scale = (2.0 / (g[0] * g[0] + g[1] * g[1])).sqrt();
    g.map(|x| x * scale)
}

pub struct Filter {
    low: [f32; 2],
    gain: [f32; 2],
}
impl Default for Filter {
    fn default() -> Self {
        Self {
            low: [0.0; 2],
            gain: [1.0; 2],
        }
    }
}
impl Filter {
    pub fn is_bypassed(&self) -> bool {
        self.gain == [1.0; 2]
    }
    pub fn process(&mut self, input: [f32; 2], target: [f32; 2]) -> [f32; 2] {
        // Fixed 48 kHz internal renderer. 700 Hz one-pole shelf, 20 ms smoothing.
        // Preserve the existing high-frequency pinna cues and ITD.
        std::array::from_fn(|ear| {
            self.low[ear] += 0.087557 * (input[ear] - self.low[ear]);
            // Silent object tails must reach zero instead of entering the
            // subnormal range, where scalar floating point can become costly.
            if self.low[ear].abs() < 1e-20 {
                self.low[ear] = 0.0;
            }
            self.gain[ear] += (target[ear] - self.gain[ear]) / 960.0;
            if (target[ear] - self.gain[ear]).abs() < 1e-4 {
                self.gain[ear] = target[ear];
            }
            input[ear] + (self.gain[ear] - 1.0) * self.low[ear]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silent_tail_settles_without_subnormal_state() {
        let mut filter = Filter::default();
        for _ in 0..1024 {
            filter.process([0.1, -0.1], [1.3, 0.6]);
        }
        for _ in 0..4096 {
            filter.process([0.0; 2], [1.3, 0.6]);
        }
        assert_eq!(filter.low, [0.0; 2]);
        assert_eq!(filter.process([0.0; 2], [1.3, 0.6]), [0.0; 2]);
        assert!(filter.process([0.1; 2], [1.3, 0.6])[0] > 0.1);
    }
    fn render(
        enabled: bool,
        position: [f32; 3],
        bed: bool,
        room: bool,
        hardware: bool,
    ) -> Vec<f32> {
        let mut engine = crate::Engine::new(48000, 2);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        engine
            .replace_hrtf(
                crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),
                0.0,
            )
            .unwrap();
        engine.near_field = Settings {
            enabled,
            ..Default::default()
        };
        engine.cinema.enabled = room;
        engine.cinema.monitor.hardware.enabled = hardware;
        engine.paused = false;
        engine.output_active = true;
        let mut source = crate::Source {
            kind: if bed {
                crate::SourceKind::Bed
            } else {
                crate::SourceKind::Object
            },
            bed_label: bed.then(|| "FrontLeft".into()),
            position,
            gain: 1.0,
            target_gain: 1.0,
            availability: 1.0,
            availability_target: 1.0,
            ..Default::default()
        };
        let samples: Vec<_> = (0..24000)
            .map(|i| (i as f32 * std::f32::consts::TAU * 200.0 / 48000.0).sin() * 0.01)
            .collect();
        source.samples.write(0, 0, &samples);
        engine.sources.insert("obj:1".into(), source);
        engine.route_source_now("obj:1", 0).unwrap();
        // Keep the original direct path in both runs to isolate the correction.
        engine.set_direct_objects(true).unwrap();
        engine.direct_mix = 1.0;
        let mut out = vec![0.0; 48000];
        engine.render_into(&mut out, 2);
        assert!(out.iter().all(|x| x.is_finite()));
        out
    }
    #[test]
    fn near_field_changes_object_pcm_but_preserves_far_beds_and_blocked_modes() {
        for (p, bed, room, hardware) in [
            ([1.0, 0.0, 0.0], false, false, false),
            ([0.25, 0.0, 0.0], true, false, false),
            ([0.25, 0.0, 0.0], false, false, true),
        ] {
            assert_eq!(
                render(false, p, bed, room, hardware),
                render(true, p, bed, room, hardware)
            );
        }
        let bypass = render(false, [0.25, 0.0, 0.0], false, false, false);
        let near = render(true, [0.25, 0.0, 0.0], false, false, false);
        let energy: f32 = bypass[24000..].iter().map(|x| x * x).sum();
        let delta: f32 = near[24000..]
            .iter()
            .zip(&bypass[24000..])
            .map(|(a, b)| (a - b) * (a - b))
            .sum();
        assert!(
            delta / energy > 0.005,
            "near-field correction did not reach PCM"
        );
    }
    #[test]
    fn geometry_is_symmetric_bounded_and_far_is_identity() {
        let s = Settings {
            enabled: true,
            ..Default::default()
        };
        for radius in [0.0, 0.001, 0.2, 0.5, 0.99, 1.0, 4.0] {
            let a = gains([-radius, 0.0, 0.0], None, s);
            let b = gains([radius, 0.0, 0.0], None, s);
            assert_eq!(a, [b[1], b[0]]);
            assert!(
                a.iter()
                    .all(|x| x.is_finite() && *x > 0.0 && *x <= 2.0_f32.sqrt())
            );
            assert!((a[0] * a[0] + a[1] * a[1] - 2.0).abs() < 1e-5);
            if radius >= 1.0 {
                assert_eq!(a, [1.0; 2]);
            }
        }
        assert!(gains([-0.2, 0.0, 0.0], None, s)[0] > gains([-0.5, 0.0, 0.0], None, s)[0]);
        assert_eq!(gains([0.0, 0.2, 0.0], None, s), [1.0; 2]);
        assert_eq!(gains([f32::NAN, 0.0, 0.0], None, s), [1.0; 2]);
        let turned = gains([-0.2, 0.0, 0.0], Some([0.0, 0.0, 1.0, 0.0]), s);
        assert_eq!(turned, gains([0.2, 0.0, 0.0], None, s));
    }
    #[test]
    fn bypass_is_exact_and_toggle_is_smooth() {
        let mut f = Filter::default();
        for i in 0..4800 {
            let x = (i as f32 * 0.17).sin();
            assert_eq!(f.process([x, x], [1.0; 2]), [x, x]);
        }
        let mut previous = f.process([0.1; 2], [1.0; 2]);
        for i in 0..24000 {
            let output = f.process([0.1; 2], if i < 4800 { [1.3, 0.55] } else { [1.0; 2] });
            assert!(output.iter().all(|x| x.is_finite()));
            if i > 100 {
                assert!((output[0] - previous[0]).abs() < 0.001);
            }
            previous = output;
        }
        assert_eq!(previous, [0.1; 2]);
    }
}
