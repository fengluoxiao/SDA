//! Pure ADM spatial math shared by native object rendering and protocol tests.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spherical {
    pub azimuth: f32,
    pub elevation: f32,
    pub distance: f32,
}

pub fn adm_to_spherical(position: [f32; 3]) -> Spherical {
    let [x, y, z] = position;
    let distance = (x * x + y * y + z * z).sqrt().min(4.0);
    if distance < 1e-6 {
        return Spherical {
            azimuth: 0.0,
            elevation: 0.0,
            distance: 0.0,
        };
    }
    Spherical {
        azimuth: -x.atan2(y).to_degrees(),
        elevation: (z / distance).clamp(-1.0, 1.0).asin().to_degrees(),
        distance,
    }
}

pub fn spread_from_size(size: [f32; 3]) -> f32 {
    let average = (size[0].abs() + size[1].abs() + size[2].abs()) / 3.0;
    average.clamp(0.0, 1.0)
}

pub fn normalize_quaternion(q: [f32; 4]) -> Option<[f32; 4]> {
    if !q.iter().all(|value| value.is_finite()) {
        return None;
    }
    let length = q.iter().map(|value| value * value).sum::<f32>().sqrt();
    (length >= 1e-8).then(|| [q[0] / length, q[1] / length, q[2] / length, q[3] / length])
}

fn invert(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}
fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[1],
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
