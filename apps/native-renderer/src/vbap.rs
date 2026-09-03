//! Fixed 7.1.4 virtual-speaker VBAP for the native binaural renderer.
//!
//! This is intentionally evaluated only when object metadata or head pose
//! changes. The render hot path only interpolates the prepared bus gains.

const EPSILON: f32 = 1e-4;

#[derive(Clone, Copy)]
struct Speaker {
    azimuth: f32,
    elevation: f32,
}

// Master `LAYOUT_7_1_4`, excluding LFE which never receives object panning.
const SPEAKERS: [Speaker; 11] = [
    Speaker {
        azimuth: 30.0,
        elevation: 0.0,
    },
    Speaker {
        azimuth: -30.0,
        elevation: 0.0,
    },
    Speaker {
        azimuth: 0.0,
        elevation: 0.0,
    },
    Speaker {
        azimuth: 100.0,
        elevation: 0.0,
    },
    Speaker {
        azimuth: -100.0,
        elevation: 0.0,
    },
    Speaker {
        azimuth: 140.0,
        elevation: 0.0,
    },
    Speaker {
        azimuth: -140.0,
        elevation: 0.0,
    },
    Speaker {
        azimuth: 45.0,
        elevation: 45.0,
    },
    Speaker {
        azimuth: -45.0,
        elevation: 45.0,
    },
    Speaker {
        azimuth: 135.0,
        elevation: 45.0,
    },
    Speaker {
        azimuth: -135.0,
        elevation: 45.0,
    },
];

pub const BUS_COUNT: usize = SPEAKERS.len();

#[derive(Clone)]
pub struct VbapSolver {
    dirs: [[f32; 3]; BUS_COUNT],
    faces: Vec<Face>,
}

#[derive(Clone)]
struct Face {
    speakers: [usize; 3],
    inverse: [[f32; 3]; 3],
}

impl VbapSolver {
    pub fn new() -> Self {
        let dirs = SPEAKERS.map(|speaker| unit(speaker.azimuth, speaker.elevation));
        let mut faces = Vec::new();
        for a in 0..BUS_COUNT {
            for b in a + 1..BUS_COUNT {
                for c in b + 1..BUS_COUNT {
                    let basis = [dirs[a], dirs[b], dirs[c]];
                    let Some(inverse) = inverse3(basis) else {
                        continue;
                    };
                    let normal = cross(sub(dirs[b], dirs[a]), sub(dirs[c], dirs[a]));
                    let mut positive = false;
                    let mut negative = false;
                    for (index, direction) in dirs.iter().enumerate() {
                        if index == a || index == b || index == c {
                            continue;
                        }
                        let side = dot(normal, sub(*direction, dirs[a]));
                        positive |= side > EPSILON;
                        negative |= side < -EPSILON;
                    }
                    if !(positive && negative) {
                        faces.push(Face {
                            speakers: [a, b, c],
                            inverse,
                        });
                    }
                }
            }
        }
        Self { dirs, faces }
    }

    pub fn pan(&self, position: [f32; 3], spread: f32) -> [f32; BUS_COUNT] {
        let direction = normalize(position).unwrap_or(self.dirs[2]);
        let mut gains = [0.0; BUS_COUNT];
        let mut best: Option<([f32; 3], &[usize; 3], f32)> = None;
        for face in &self.faces {
            let gain = multiply(face.inverse, direction);
            let minimum = gain[0].min(gain[1]).min(gain[2]);
            if minimum >= -EPSILON
                && best
                    .as_ref()
                    .is_none_or(|(_, _, previous)| minimum > *previous)
            {
                best = Some((gain, &face.speakers, minimum));
            }
        }
        if let Some((gain, speakers, _)) = best {
            for index in 0..3 {
                gains[speakers[index]] = gain[index].max(0.0);
            }
        } else {
            let nearest = self
                .dirs
                .iter()
                .enumerate()
                .max_by(|(_, left), (_, right)| {
                    dot(**left, direction).total_cmp(&dot(**right, direction))
                })
                .map(|(index, _)| index)
                .unwrap_or(2);
            gains[nearest] = 1.0;
        }
        normalize_power(&mut gains);
        let spread = spread.clamp(0.0, 1.0);
        if spread > 0.0 {
            let mut nearest = [(f32::NEG_INFINITY, 0_usize); 4];
            for (index, speaker) in self.dirs.iter().enumerate() {
                let value = dot(*speaker, direction);
                for rank in 0..nearest.len() {
                    if value <= nearest[rank].0 {
                        continue;
                    }
                    for move_index in (rank + 1..nearest.len()).rev() {
                        nearest[move_index] = nearest[move_index - 1];
                    }
                    nearest[rank] = (value, index);
                    break;
                }
            }
            let diffuse = 1.0 / (nearest.len() as f32).sqrt();
            for (_, index) in nearest {
                gains[index] = (1.0 - spread) * gains[index] + spread * diffuse;
            }
            normalize_power(&mut gains);
        }
        gains
    }

    pub fn speaker_direction(&self, bus: usize) -> (f32, f32) {
        let speaker = SPEAKERS[bus];
        (speaker.azimuth, speaker.elevation)
    }
}

fn unit(azimuth: f32, elevation: f32) -> [f32; 3] {
    let azimuth = azimuth.to_radians();
    let elevation = elevation.to_radians();
    [
        elevation.cos() * azimuth.sin(),
        elevation.cos() * azimuth.cos(),
        elevation.sin(),
    ]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(value: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(value, value).sqrt();
    (length > EPSILON).then(|| [value[0] / length, value[1] / length, value[2] / length])
}
fn normalize_power(gains: &mut [f32]) {
    let power = gains.iter().map(|gain| gain * gain).sum::<f32>();
    if power > 0.0 {
        let scale = power.sqrt().recip();
        for gain in gains {
            *gain *= scale;
        }
    }
}
fn multiply(matrix: [[f32; 3]; 3], vector: [f32; 3]) -> [f32; 3] {
    [
        dot(matrix[0], vector),
        dot(matrix[1], vector),
        dot(matrix[2], vector),
    ]
}
fn inverse3(rows: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let determinant = rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
        - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
        + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0]);
    if determinant.abs() < EPSILON {
        return None;
    }
    let inverse = determinant.recip();
    Some([
        [
            (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1]) * inverse,
            (rows[0][2] * rows[2][1] - rows[0][1] * rows[2][2]) * inverse,
            (rows[0][1] * rows[1][2] - rows[0][2] * rows[1][1]) * inverse,
        ],
        [
            (rows[1][2] * rows[2][0] - rows[1][0] * rows[2][2]) * inverse,
            (rows[0][0] * rows[2][2] - rows[0][2] * rows[2][0]) * inverse,
            (rows[0][2] * rows[1][0] - rows[0][0] * rows[1][2]) * inverse,
        ],
        [
            (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0]) * inverse,
            (rows[0][1] * rows[2][0] - rows[0][0] * rows[2][1]) * inverse,
            (rows[0][0] * rows[1][1] - rows[0][1] * rows[1][0]) * inverse,
        ],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn front_object_is_power_normalized_and_local() {
        let solver = VbapSolver::new();
        let gains = solver.pan([0.0, 1.0, 0.0], 0.0);
        assert!((gains.iter().map(|gain| gain * gain).sum::<f32>() - 1.0).abs() < 1e-4);
        assert!(gains.iter().any(|gain| *gain > 0.0));
    }
    #[test]
    fn spread_remains_power_normalized() {
        let gains = VbapSolver::new().pan([1.0, 1.0, 0.5], 1.0);
        assert!((gains.iter().map(|gain| gain * gain).sum::<f32>() - 1.0).abs() < 1e-4);
        assert!(gains.iter().filter(|gain| **gain > 0.0).count() >= 4);
    }
}
