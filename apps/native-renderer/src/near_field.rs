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

/// Authoritative physical distance wins over the optional manual ADM mapping.
/// A missing/invalid distance never turns direction-only music into metres.
pub(crate) fn distance(
    position: [f32; 3],
    metres: Option<f32>,
    infinite: bool,
    settings: Settings,
) -> Option<f32> {
    if infinite {
        return None;
    }
    if let Some(r) = metres.filter(|r| r.is_finite() && *r > 0.0) {
        return Some(r);
    }
    if !settings.enabled || !settings.valid() || !position.iter().all(|v| v.is_finite()) {
        return None;
    }
    let radius = position
        .iter()
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        .sqrt();
    (radius > 1e-6).then_some((radius * f64::from(settings.metres_per_unit)) as f32)
}

/// Room attenuation uses the same metres as the binaural correction, including
/// an explicit codec distance when a manual coordinate mapping is also enabled.
/// This is a bounded room policy, not a measured room's distance response.
pub(crate) fn reflection_gain(
    position: [f32; 3],
    metres: Option<f32>,
    infinite: bool,
    settings: Settings,
) -> f32 {
    distance(position, metres, infinite, settings).map_or(1.0, |r| 0.25 + 0.75 * r.clamp(0.0, 1.0))
}

/// Relative spherical-spreading ear gains referenced to the same direction at
/// one ADM unit. Unity mean-square gain avoids adding a global inverse-distance
/// volume law (the engine's distance gain owns loudness); this layer supplies
/// the timbral near-field cues — bass build-up and interaural shading — that
/// make "approaching" audible beyond raw level. The effect fades in below one
/// unit and stays transparent on the unit sphere and beyond.
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
    // Fade the correction in across 1.0 -> 0.6 units so a source sitting on
    // the ADM reference sphere stays untouched.
    if r >= 1.0 {
        return [1.0; 2];
    }
    let fade = (1.0 - r).max(0.0).min(0.4) / 0.4;
    let lateral = p[0] / norm;
    let ear_gain = |ear: f32| {
        let near = (r * r + ear * ear - 2.0 * r * ear * lateral).sqrt();
        let reference = (1.0 + ear * ear - 2.0 * ear * lateral).sqrt();
        r * reference / near
    };
    let g = [ear_gain(-0.0875), ear_gain(0.0875)];
    let scale = (2.0 / (g[0] * g[0] + g[1] * g[1])).sqrt();
    // Frontal/back bearings have near-equal ear paths, so the pairwise term
    // alone is silent exactly where ADM music places its voices. Add a
    // direction-independent proximity shelf: a source inside the reference
    // sphere also sounds warmer (bass build-up) and more present.
    let shelf = 1.0 + 0.8 * fade;
    g.map(|ear| 1.0 + ((ear * scale) * shelf - 1.0) * fade)
}

pub struct Filter {
    low: [f32; 2],
    previous: [f32; 2],
    gain: [f32; 2],
}
impl Default for Filter {
    fn default() -> Self {
        Self {
            low: [0.0; 2],
            previous: [0.0; 2],
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
        // Keep correction below the main pinna-cue region. No second
        // propagation/ITD delay is added; this is a minimum-phase shelf.
        std::array::from_fn(|ear| {
            // Bilinear one-pole at 700 Hz: an explicit zero at Nyquist removes
            // the old exponential pole's high-band feedthrough. Coefficients
            // are fixed (k=tan(pi*700/48000)); no runtime trigonometry needed.
            const B: f32 = 0.043_837_175;
            const A: f32 = 1.0 - 2.0 * B;
            self.low[ear] = B * (input[ear] + self.previous[ear]) + A * self.low[ear];
            self.previous[ear] = input[ear];
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
        distance_m: Option<f32>,
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
            distance_m,
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
        // Direction-only coordinates must not auto-enable correction. Far
        // objects, beds, and hardware mode stay identical across the toggle.
        for (p, bed, room, hardware) in [
            ([1.0, 0.0, 0.0], false, false, false),
            ([1.0, 0.0, 0.0], true, false, false),
            ([0.25, 0.0, 0.0], true, false, false),
            ([0.25, 0.0, 0.0], false, false, true),
        ] {
            assert_eq!(
                render(false, p, None, bed, room, hardware),
                render(true, p, None, bed, room, hardware)
            );
        }
        // Explicit physical distance auto-enables correction. The direction
        // vector remains a bearing and has no role in this decision.
        let near_object = render(false, [0.25, 0.0, 0.0], Some(0.25), false, false, false);
        let far_object = render(false, [0.25, 0.0, 0.0], None, false, false, false);
        let near_energy: f32 = near_object[24000..].iter().map(|x| x * x).sum();
        let delta: f32 = near_object[24000..]
            .iter()
            .zip(&far_object[24000..])
            .map(|(a, b)| (a - b) * (a - b))
            .sum();
        assert!(
            delta / near_energy > 0.005,
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
            // The proximity shelf raises both ears together, so the mean-square
            // unity bound only holds outside the near-field fade.
            assert!(a.iter().all(|x| x.is_finite() && *x > 0.0 && *x <= 3.5));
            if radius >= 1.0 {
                assert_eq!(a, [1.0; 2]);
            }
        }
        assert!(gains([-0.2, 0.0, 0.0], None, s)[0] > gains([-0.5, 0.0, 0.0], None, s)[0]);
        assert_eq!(gains([f32::NAN, 0.0, 0.0], None, s), [1.0; 2]);
        let turned = gains([-0.2, 0.0, 0.0], Some([0.0, 0.0, 1.0, 0.0]), s);
        assert_eq!(turned, gains([0.2, 0.0, 0.0], None, s));
    }
    #[test]
    fn frontal_and_rear_near_sources_keep_proximity_presence() {
        let s = Settings {
            enabled: true,
            ..Default::default()
        };
        let frontal = gains([0.0, 0.25, 0.0], None, s);
        let rear = gains([0.0, -0.25, 0.0], None, s);
        assert!(frontal.iter().all(|gain| *gain > 1.0));
        assert!(rear.iter().all(|gain| *gain > 1.0));
        assert_eq!(frontal, rear);
        assert_eq!(gains([0.0, 1.0, 0.0], None, s), [1.0; 2]);
        assert_eq!(gains([0.0, -1.0, 0.0], None, s), [1.0; 2]);
    }

    #[test]
    fn near_field_adds_radial_cues_but_cannot_replace_front_back_hrtf() {
        let settings = Settings {
            enabled: true,
            ..Default::default()
        };
        let close = gains([0.0, 0.3, 0.0], None, settings);
        let farther = gains([0.0, 0.8, 0.0], None, settings);
        assert!(close[0] > farther[0]);
        assert_eq!(close, gains([0.0, -0.3, 0.0], None, settings));
        assert_eq!(farther, gains([0.0, -0.8, 0.0], None, settings));
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

    #[test]
    fn physical_metres_and_room_ratio_are_independent_of_manual_scale() {
        for scale in [0.25, 0.5, 1.0, 2.0, 4.0] {
            let settings = Settings {
                enabled: true,
                metres_per_unit: scale,
            };
            assert_eq!(
                distance([2.0, 0.0, 0.0], Some(0.3), false, settings),
                Some(0.3)
            );
            assert!(
                (reflection_gain([2.0, 0.0, 0.0], Some(0.3), false, settings) - 0.475).abs() < 1e-6
            );
            assert_eq!(
                distance([0.5, 0.0, 0.0], None, false, settings),
                Some(0.5 * scale)
            );
            assert_eq!(reflection_gain([0.0; 3], None, false, settings), 1.0);
            assert_eq!(distance([0.1, 0.0, 0.0], None, true, settings), None);
            assert_eq!(distance([0.1, 0.0, 0.0], Some(0.3), true, settings), None);
            assert_eq!(reflection_gain([0.1, 0.0, 0.0], None, true, settings), 1.0);
        }
        let off = Settings::default();
        assert_eq!(distance([0.1, 0.0, 0.0], None, false, off), None);
        for invalid in [f32::NAN, f32::INFINITY, -0.1, 0.0] {
            assert_eq!(distance([0.1, 0.0, 0.0], Some(invalid), false, off), None);
            assert_eq!(
                reflection_gain([0.1, 0.0, 0.0], Some(invalid), false, off),
                1.0
            );
        }
    }

    #[test]
    fn near_field_shelf_does_not_leak_a_constant_high_frequency_gain() {
        let target = [1.3, 0.6];
        for hz in [125.0, 700.0, 4000.0, 8000.0, 16000.0] {
            let mut f = Filter::default();
            let mut input_energy = 0.0_f64;
            let mut output_energy = [0.0_f64; 2];
            for i in 0..48000 {
                let x = 0.1 * (std::f32::consts::TAU * hz * i as f32 / 48000.0).sin();
                let y = f.process([x; 2], target);
                if i >= 24000 {
                    input_energy += f64::from(x).powi(2);
                    for ear in 0..2 {
                        output_energy[ear] += f64::from(y[ear]).powi(2);
                    }
                }
            }
            if hz >= 8000.0 {
                for power in output_energy {
                    assert!(
                        (10.0 * (power / input_energy).log10()).abs() < 0.05,
                        "pinna band coloured at {hz}Hz"
                    );
                }
            }
        }
    }
}
