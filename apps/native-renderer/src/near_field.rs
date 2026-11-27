//! Low-frequency rigid-sphere distance variation, not a measured personal HRTF.
//! Duda & Martens (1998), doi:10.1121/1.423886; Brungart & Rabinowitz
//! (1999), doi:10.1121/1.427180. See docs/near-field-research.md for the
//! approximation, independent reference fixtures, and its frequency limits.
//! ADM positions are bearings unless physical metres or a manual scale exist.
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
pub(crate) fn distance(position: [f32; 3], metres: Option<f32>, settings: Settings) -> Option<f32> {
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
pub(crate) fn reflection_gain(position: [f32; 3], metres: Option<f32>, settings: Settings) -> f32 {
    distance(position, metres, settings).map_or(1.0, |r| 0.25 + 0.75 * r.clamp(0.0, 1.0))
}

/// Zero-frequency limit of the rigid-sphere point-source solution:
/// sum_n ((2n+1)/(n+1)) q^n P_n(mu), q = head_radius / source_distance.
/// The generating function integrates to this closed form. Evaluating in f64
/// and rationalizing the logarithm avoids cancellation near the ipsilateral ear.
fn sphere_dc(q: f64, mu: f64) -> f64 {
    let mu = mu.clamp(-1.0, 1.0);
    let path = (1.0 - 2.0 * q * mu + q * q).sqrt();
    let integral = if mu >= 1.0 - 1e-12 {
        -(-q).ln_1p() / q
    } else if mu <= -1.0 + 1e-12 {
        q.ln_1p() / q
    } else if mu > 0.0 {
        ((1.0 + mu) / (path + mu - q)).ln() / q
    } else {
        ((path + q - mu) / (1.0 - mu)).ln() / q
    };
    2.0 / path - integral
}

/// Relative low-frequency DVF at the ears of an 8.75 cm rigid sphere, referenced
/// to 1 m. Preserve the model's ILD, normalize the pair's mean-square gain, and
/// leave overall distance loudness to the existing explicit-metre gain path.
/// No common bass boost and no second direction/ITD/pinna model are added.
pub fn gains(position: [f32; 3], head: Option<[f32; 4]>, settings: Settings) -> [f32; 2] {
    if !settings.enabled || !settings.valid() || !position.iter().all(|x| x.is_finite()) {
        return [1.0; 2];
    }
    let p = crate::spatial::head_relative_adm(position, head);
    let norm = p.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    if !norm.is_finite() || norm < 1e-6 {
        return [1.0; 2];
    }
    // 12 cm is the nearest head-centre distance in Brungart's measurements.
    // Never extrapolate toward the sphere surface, where the point model diverges.
    let r = (norm * f64::from(settings.metres_per_unit)).max(0.12);
    if r >= 1.0 {
        return [1.0; 2];
    }
    let lateral = (f64::from(p[0]) / norm).clamp(-1.0, 1.0);
    // A symmetric bearing has no extra ILD. Its proximity remains represented
    // by physical intensity and direct/reverberant balance, not invented warmth.
    if lateral == 0.0 {
        return [1.0; 2];
    }
    let g = [-lateral, lateral].map(|mu| sphere_dc(0.0875 / r, mu) / sphere_dc(0.0875, mu));
    let scale = (2.0 / (g[0] * g[0] + g[1] * g[1])).sqrt();
    g.map(|ear| (ear * scale) as f32)
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
            assert!(
                a.iter()
                    .all(|x| x.is_finite() && *x > 0.0 && *x <= std::f32::consts::SQRT_2)
            );
            assert!(((a[0] * a[0] + a[1] * a[1]) * 0.5 - 1.0).abs() < 2e-6);
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
    fn symmetric_bearings_do_not_invent_a_common_bass_boost() {
        let s = Settings {
            enabled: true,
            ..Default::default()
        };
        let frontal = gains([0.0, 0.25, 0.0], None, s);
        let rear = gains([0.0, -0.25, 0.0], None, s);
        assert_eq!(frontal, [1.0; 2]);
        assert_eq!(rear, [1.0; 2]);
        assert_eq!(gains([0.0, 0.0, 0.25], None, s), [1.0; 2]);
        assert_eq!(gains([0.0, 0.0, -0.25], None, s), [1.0; 2]);
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
        let close = gains([-0.18, 0.24, 0.0], None, settings);
        let farther = gains([-0.48, 0.64, 0.0], None, settings);
        assert!(close[0] > farther[0] && close[1] < farther[1]);
        assert_eq!(close, gains([-0.18, -0.24, 0.0], None, settings));
        assert_eq!(farther, gains([-0.48, -0.64, 0.0], None, settings));
    }

    #[test]
    fn sphere_limit_matches_independent_finite_frequency_wave_solution() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../assets/near-field/sphere-reference.json"))
                .unwrap();
        let settings = Settings {
            enabled: true,
            ..Default::default()
        };
        let mut worst_db = 0.0_f64;
        for row in fixture["rows"].as_array().unwrap() {
            let r = row["distanceM"].as_f64().unwrap() as f32;
            let x = row["lateral"].as_f64().unwrap() as f32;
            let p = [r * x, r * (1.0 - x * x).sqrt(), 0.0];
            let actual = gains(p, None, settings);
            for ear in 0..2 {
                let expected = row["gains"][ear].as_f64().unwrap();
                let error_db = (20.0 * (f64::from(actual[ear]) / expected).log10()).abs();
                worst_db = worst_db.max(error_db);
                assert!(
                    error_db < 0.025,
                    "r={r} lateral={x} ear={ear} error={error_db}dB"
                );
            }
        }
        println!("NEAR_SPHERE_REFERENCE pairs=72 worstErrorDb={worst_db:.8}");
    }

    #[test]
    fn physical_metres_and_room_ratio_are_independent_of_manual_scale() {
        for scale in [0.25, 0.5, 1.0, 2.0, 4.0] {
            let settings = Settings {
                enabled: true,
                metres_per_unit: scale,
            };
            assert_eq!(distance([2.0, 0.0, 0.0], Some(0.3), settings), Some(0.3));
            assert!((reflection_gain([2.0, 0.0, 0.0], Some(0.3), settings) - 0.475).abs() < 1e-6);
            assert_eq!(distance([0.5, 0.0, 0.0], None, settings), Some(0.5 * scale));
            assert_eq!(reflection_gain([0.0; 3], None, settings), 1.0);
        }
        let off = Settings::default();
        assert_eq!(distance([0.1, 0.0, 0.0], None, off), None);
        for invalid in [f32::NAN, f32::INFINITY, -0.1, 0.0] {
            assert_eq!(distance([0.1, 0.0, 0.0], Some(invalid), off), None);
            assert_eq!(reflection_gain([0.1, 0.0, 0.0], Some(invalid), off), 1.0);
        }
    }

    #[test]
    fn supported_close_distances_are_distinct_and_reference_boundary_is_continuous() {
        let s = Settings {
            enabled: true,
            ..Default::default()
        };
        assert!(gains([0.12, 0.0, 0.0], None, s)[0] < gains([0.2, 0.0, 0.0], None, s)[0]);
        assert_eq!(
            gains([0.01, 0.0, 0.0], None, s),
            gains([0.12, 0.0, 0.0], None, s)
        );
        let mut previous = gains([0.99, 0.0, 0.0], None, s);
        for i in 1..=2000 {
            let current = gains([0.99 + i as f32 * 0.00001, 0.0, 0.0], None, s);
            for ear in 0..2 {
                assert!((current[ear] - previous[ear]).abs() < 0.0001);
            }
            previous = current;
        }
        assert_eq!(previous, [1.0; 2]);
    }

    #[test]
    fn low_band_correction_preserves_high_band_and_has_no_common_energy_boost() {
        let s = Settings {
            enabled: true,
            ..Default::default()
        };
        let target = gains([-0.12, 0.0, 0.0], None, s);
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
            let pair_ratio = output_energy.iter().sum::<f64>() / (2.0 * input_energy);
            assert!(
                pair_ratio <= 1.0001,
                "pair energy grew at {hz}Hz: {pair_ratio}"
            );
            if hz >= 8000.0 {
                for power in output_energy {
                    assert!(
                        (10.0 * (power / input_energy).log10()).abs() < 0.15,
                        "pinna band coloured at {hz}Hz"
                    );
                }
            }
            if hz == 125.0 {
                let ild = 10.0 * (output_energy[0] / output_energy[1]).log10();
                assert!(
                    ild > 10.0,
                    "near-side low-frequency ILD did not reach PCM: {ild}"
                );
            }
        }
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
