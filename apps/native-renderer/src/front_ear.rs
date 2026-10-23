//! Geometry-only generalization of the accepted front near-ear audition.
//! No source identity, codec, PCM level, or song metadata participates.
fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
fn gains(azimuth: f64, elevation: f64, strength: f32) -> [f32; 2] {
    if !azimuth.is_finite() || !elevation.is_finite() || !strength.is_finite() {
        return [1.0; 2];
    }
    let az = (azimuth + 180.0).rem_euclid(360.0) - 180.0;
    // Full isolation at the approved +/-45deg and neighbouring front/side buses.
    // Center, rear, and overhead retain both ears, with smooth transitions.
    let amount = smooth(az.abs() / 30.0)
        * (1.0 - smooth((az.abs() - 105.0) / 30.0))
        * (1.0 - smooth((elevation.abs() - 15.0) / 30.0))
        * f64::from(strength.clamp(0.0, 1.0));
    if az > 0.0 {
        [1.0, (1.0 - amount) as f32]
    } else {
        [(1.0 - amount) as f32, 1.0]
    }
}
pub(crate) fn apply(left: &mut [f32], right: &mut [f32], az: f64, el: f64, strength: f32) {
    for (ear, gain) in [left, right].into_iter().zip(gains(az, el, strength)) {
        if gain != 1.0 {
            for sample in ear {
                *sample *= gain;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approved_pair_preserves_near_waveform_and_removes_entire_far_tail() {
        for az in [30., 45., 90., 105.] {
            let a = vec![0.2, -0.8, 0.1, 0., 0.03];
            let b = vec![-0.4, 0.3, 0.01];
            let (mut l, mut r) = (a.clone(), b.clone());
            apply(&mut l, &mut r, az, 0., 1.);
            assert_eq!(l, a);
            assert!(r.iter().all(|x| *x == 0.));
            let (mut l, mut r) = (a.clone(), b.clone());
            apply(&mut l, &mut r, -az, 0., 1.);
            assert_eq!(r, b);
            assert!(l.iter().all(|x| *x == 0.));
        }
    }
    #[test]
    fn center_rear_height_and_bypass_are_exact_identity() {
        for (az, el, s) in [
            (0., 0., 1.),
            (135., 0., 1.),
            (-180., 0., 1.),
            (45., 45., 1.),
            (-45., -90., 1.),
            (45., 0., 0.),
        ] {
            assert_eq!(gains(az, el, s), [1.; 2]);
        }
    }
    #[test]
    fn moving_source_is_bounded_continuous_and_mirrored() {
        let mut last = gains(-180., 0., 1.);
        for step in -18000..=18000 {
            let a = f64::from(step) / 100.;
            let g = gains(a, 0., 1.);
            let mirror = gains(-a, 0., 1.);
            assert_eq!(g, [mirror[1], mirror[0]]);
            for i in 0..2 {
                assert!((0.0..=1.0).contains(&g[i]));
                assert!((g[i] - last[i]).abs() < 0.001);
            }
            last = g;
        }
    }
    #[test]
    fn head_turn_recomputes_listener_side() {
        let q = std::f32::consts::FRAC_1_SQRT_2;
        let d = crate::spatial::adm_to_spherical(crate::spatial::head_relative_adm(
            [0., 1., 0.],
            Some([0., 0., q, q]),
        ));
        assert!(d.azimuth < 0.);
        assert_eq!(gains(d.azimuth.into(), d.elevation.into(), 1.), [0., 1.]);
    }
}
