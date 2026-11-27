//! Pure ADM spatial math shared by native object rendering and protocol tests.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spherical {
    pub azimuth: f32,
    pub elevation: f32,
    pub distance: f32,
}

pub fn adm_to_spherical(position: [f32; 3]) -> Spherical {
    let [x, y, z] = position;
    let radius = (x * x + y * y + z * z).sqrt();
    let distance = radius.min(4.0);
    if distance < 1e-6 {
        return Spherical {
            azimuth: 0.0,
            elevation: 0.0,
            distance: 0.0,
        };
    }
    Spherical {
        azimuth: -x.atan2(y).to_degrees(),
        // The display/range cap must not change the direction. Large ADM
        // coordinates still use their actual vector length for elevation.
        elevation: if radius.is_infinite() && position.iter().all(|v| v.is_finite()) {
            let norm = position
                .iter()
                .map(|v| f64::from(*v).powi(2))
                .sum::<f64>()
                .sqrt();
            (f64::from(z) / norm).clamp(-1., 1.).asin().to_degrees() as f32
        } else {
            (z / radius).clamp(-1.0, 1.0).asin().to_degrees()
        },
        distance,
    }
}

/// Offset an angular footprint in its local tangent plane. Unlike adding
/// azimuth/elevation and clamping, this crosses either pole without truncating
/// half the source or collapsing its horizontal width. Does not change radius
/// metadata: the returned value is only an HRTF/panner sample direction.
pub(crate) fn footprint_direction(position: [f32; 3], horizontal: f32, vertical: f32) -> [f32; 3] {
    let [x, y, z] = position.map(f64::from);
    let radius = (x * x + y * y + z * z).sqrt();
    let n = if radius.is_finite() && radius >= f64::from(1e-6_f32) {
        [x / radius, y / radius, z / radius]
    } else {
        [0., 1., 0.]
    };
    // Even a tiny transverse component retains the authored bearing at a
    // pole. Discarding it rotates anisotropic footprints for one frame.
    let az = (-n[0]).atan2(n[1]);
    let elevation = n[2].clamp(-1., 1.).asin();
    let u = [-az.cos(), -az.sin(), 0.];
    let v = [
        az.sin() * elevation.sin(),
        -az.cos() * elevation.sin(),
        elevation.cos(),
    ];
    let a = f64::from(horizontal).to_radians();
    let b = f64::from(vertical).to_radians();
    let angle = a.hypot(b);
    if angle < 1e-12 {
        return n.map(|v| v as f32);
    }
    std::array::from_fn(|i| {
        (n[i] * angle.cos() + (u[i] * a + v[i] * b) * angle.sin() / angle) as f32
    })
}

pub fn spread_from_size(size: [f32; 3]) -> f32 {
    let average = (size[0].abs() + size[1].abs() + size[2].abs()) / 3.0;
    average.clamp(0.0, 1.0)
}

/// Spherical interpolation between two head-to-world quaternions. Used to
/// smooth head-pose updates across the render clock so HRTF route updates run
/// at the block cadence instead of the arrival cadence of pose messages.
#[must_use]
pub fn slerp_quaternion(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let a = normalize_quaternion(a).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let b = normalize_quaternion(b).unwrap_or(a);
    if !t.is_finite() {
        return a;
    }
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    // Take the short arc: a and -a describe the same rotation.
    let (b, dot) = if dot < 0.0 {
        ([-b[0], -b[1], -b[2], -b[3]], -dot)
    } else {
        (b, dot)
    };
    if dot > 0.9995 {
        // Nearly identical: fall back to normalized lerp for stability.
        let w = [
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
            a[3] + t * (b[3] - a[3]),
        ];
        return normalize_quaternion(w).unwrap_or(b);
    }
    let theta = dot.clamp(0.0, 1.0).acos();
    let denominator = theta.sin();
    let wa = ((1.0 - t) * theta).sin() / denominator;
    let wb = (t * theta).sin() / denominator;
    normalize_quaternion(std::array::from_fn(|i| wa * a[i] + wb * b[i])).unwrap_or(b)
}

pub fn normalize_quaternion(q: [f32; 4]) -> Option<[f32; 4]> {
    if !q.iter().all(|value| value.is_finite()) {
        return None;
    }
    let length = q.iter().map(|value| value * value).sum::<f32>().sqrt();
    // Retain the normal fast path and its rounding. Finite but very large
    // components can overflow the f32 norm and otherwise normalize to zero.
    if !length.is_finite() {
        let length = q.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>().sqrt();
        return (length >= 1e-8).then(|| q.map(|v| (f64::from(v) / length) as f32));
    }
    (length >= 1e-8).then(|| [q[0] / length, q[1] / length, q[2] / length, q[3] / length])
}

fn invert(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}
fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// Turns a world ADM direction into head-local ADM with the inverse canonical
/// head-to-world quaternion. Invalid poses leave the source unrotated.
pub fn head_relative_adm(position: [f32; 3], head_to_world: Option<[f32; 4]>) -> [f32; 3] {
    let Some(head_to_world) = head_to_world.and_then(normalize_quaternion) else {
        return position;
    };
    let inverse = invert(head_to_world);
    let rotated = multiply(
        multiply(inverse, [position[0], position[1], position[2], 0.0]),
        head_to_world,
    );
    [rotated[0], rotated[1], rotated[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spherical_interpolation_keeps_unit_norm_and_endpoint_rotations() {
        for degrees in [30.0_f32, 90.0, 170.0] {
            let angle = degrees.to_radians();
            let a = [0.0, 0.0, 0.0, 1.0];
            let b = [0.0, 0.0, (angle / 2.0).sin(), (angle / 2.0).cos()];
            for t in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
                let q = slerp_quaternion(a, b, t);
                let expected = [0.0, 0.0, (angle * t / 2.0).sin(), (angle * t / 2.0).cos()];
                assert!(
                    (q.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 2e-6,
                    "non-unit interpolation at {degrees}deg / t={t}: {q:?}"
                );
                for i in 0..4 {
                    assert!((q[i] - expected[i]).abs() < 2e-6);
                }
            }
        }
    }

    #[test]
    fn quaternion_normalization_is_scale_invariant_for_finite_inputs() {
        for scale in [1e-6_f32, 1.0, 1e6, 1e20, f32::MAX / 2.0] {
            let normalized = normalize_quaternion([scale / 2.0; 4]).unwrap();
            for x in normalized {
                assert!((x - 0.5).abs() < 1e-6);
            }
        }
        assert!(normalize_quaternion([0.0; 4]).is_none());
        assert!(normalize_quaternion([f32::NAN; 4]).is_none());
        let identity = [0.0, 0.0, 0.0, 1.0];
        assert_eq!(slerp_quaternion(identity, [f32::NAN; 4], 0.5), identity);
        assert_eq!(slerp_quaternion(identity, identity, f32::NAN), identity);
    }

    #[test]
    fn left_right_and_front_back_directions_are_distinct_and_pose_rotates_world_front() {
        let left = adm_to_spherical([-1.0, 0.0, 0.0]);
        let right = adm_to_spherical([1.0, 0.0, 0.0]);
        let front = adm_to_spherical([0.0, 1.0, 0.0]);
        let back = adm_to_spherical([0.0, -1.0, 0.0]);
        assert!((left.azimuth - 90.0).abs() < 1e-5);
        assert!((right.azimuth + 90.0).abs() < 1e-5);
        assert!(front.azimuth.abs() < 1e-5);
        assert!((back.azimuth.abs() - 180.0).abs() < 1e-5);

        let half = std::f32::consts::FRAC_1_SQRT_2;
        let head_left = head_relative_adm([0.0, 1.0, 0.0], Some([0.0, 0.0, half, half]));
        assert!(head_left[0] > 0.99 && head_left[1].abs() < 1e-4,
            "turning head left makes a world-front source head-relative right: {head_left:?}");
    }

    #[test]
    fn head_rotation_preserves_height_and_world_locked_direction() {
        for degrees in [-90.0_f32, -45.0, 0.0, 45.0, 90.0] {
            let angle = degrees.to_radians();
            let q = [0.0, 0.0, (angle / 2.0).sin(), (angle / 2.0).cos()];
            let local = head_relative_adm([0.0, 1.0, 0.4], Some(q));
            assert!((local[0] - angle.sin()).abs() < 1e-5);
            assert!((local[1] - angle.cos()).abs() < 1e-5);
            assert!((local[2] - 0.4).abs() < 1e-5);
        }
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let local = head_relative_adm([0.0, 1.0, 0.0], Some([half, 0.0, 0.0, half]));
        assert!((local[2] + 1.0).abs() < 1e-5);
    }

    #[test]
    fn local_footprint_crosses_poles_and_keeps_angular_width() {
        for pole in [1.0f32, -1.0] {
            for horizontal in [-60.0f32, -15., 0., 15., 60.] {
                for vertical in [-60.0f32, -15., 0., 15., 60.] {
                    let p = footprint_direction([0., 0., pole], horizontal, vertical);
                    let norm = p.iter().map(|v| v * v).sum::<f32>().sqrt();
                    assert!((norm - 1.).abs() < 1e-6);
                    let angle = f64::from(horizontal)
                        .hypot(f64::from(vertical))
                        .to_radians();
                    assert!((f64::from(p[2] * pole) - angle.cos()).abs() < 1e-6);
                    let mirror = footprint_direction([0., 0., pole], -horizontal, -vertical);
                    assert!((p[0] + mirror[0]).abs() < 1e-6 && (p[1] + mirror[1]).abs() < 1e-6);
                }
            }
        }
        let through = footprint_direction([0., 0., 1.], 0., 15.);
        assert!(
            through[1] < -0.25 && through[2] > 0.96,
            "upper half was clipped: {through:?}"
        );
        for az in [-179.0f32, -45., 0., 45., 179.] {
            let a = az.to_radians();
            let p = [-a.sin(), a.cos(), 0.];
            let q = footprint_direction(p, 15., 0.);
            let b = (az + 15.).to_radians();
            assert!((q[0] + b.sin()).abs() < 1e-6 && (q[1] - b.cos()).abs() < 1e-6);
        }
    }

    #[test]
    fn tiny_position_extent_uses_the_same_front_fallback_as_its_center() {
        for position in [[0., 0., 0.], [1e-8, -3e-8, 4e-8], [0., 0., 1e-8]] {
            assert_eq!(adm_to_spherical(position).azimuth, 0.);
            assert_eq!(adm_to_spherical(position).elevation, 0.);
            for (width, height) in [(0., 0.), (15., 0.), (0., -15.), (30., 30.)] {
                assert_eq!(
                    footprint_direction(position, width, height),
                    footprint_direction([0., 1., 0.], width, height)
                );
            }
        }
    }

    #[test]
    fn coordinate_scale_and_display_distance_cap_do_not_change_height() {
        let expected = adm_to_spherical([0.3, 0.7, 0.6]);
        for scale in [0.001f32, 0.5, 1., 4., 10., 1000., 1e20] {
            let actual = adm_to_spherical([0.3 * scale, 0.7 * scale, 0.6 * scale]);
            assert!((expected.azimuth - actual.azimuth).abs() < 0.0001);
            assert!(
                (expected.elevation - actual.elevation).abs() < 0.0001,
                "scale={scale} expected={expected:?} actual={actual:?}"
            );
            assert!(actual.distance <= 4.);
        }
        assert!((adm_to_spherical([0., 8., 8.]).elevation - 45.).abs() < 1e-5);
    }

    #[test]
    fn adm_coordinates_match_renderer_convention() {
        let front = adm_to_spherical([0.0, 1.0, 0.0]);
        let left = adm_to_spherical([-1.0, 0.0, 0.0]);
        let right = adm_to_spherical([1.0, 0.0, 0.0]);
        assert!(front.azimuth.abs() < 1e-5);
        assert!((left.azimuth - 90.0).abs() < 1e-5);
        assert!((right.azimuth + 90.0).abs() < 1e-5);
    }

    #[test]
    fn inverse_head_pose_rotates_world_sources_into_head_space() {
        // Head rotated +90° around ADM up. A world-front source is at head-right.
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let local = head_relative_adm([0.0, 1.0, 0.0], Some([0.0, 0.0, half, half]));
        assert!(local[0] > 0.99);
        assert!(local[1].abs() < 1e-4);
    }
}
