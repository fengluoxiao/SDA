//! Master-aligned virtual-speaker VBAP for the native binaural renderer.
//!
//! Layout changes rebuild this solver only on the render worker. The audio hot
//! path interpolates already prepared gains and never parses layouts or builds
//! faces in the WASAPI callback.

const DET_EPSILON: f32 = 1e-9;
const HULL_EPSILON: f32 = 1e-7;
const GAIN_EPSILON: f32 = 1e-4;

/// 9.1.6 has fifteen non-LFE physical virtual speakers.
pub const MAX_BUS_COUNT: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutId {
    Stereo2_0,
    Stereo2_1,
    Dolby5_1,
    Dolby5_1_2,
    Dolby5_1_4,
    Dolby7_1_2,
    Dolby7_1_4,
    Dolby9_1_2,
    Dolby9_1_4,
    Dolby9_1_6,
}

impl LayoutId {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "2.0" => Self::Stereo2_0,
            "2.1" => Self::Stereo2_1,
            "5.1" => Self::Dolby5_1,
            "5.1.2" => Self::Dolby5_1_2,
            "5.1.4" => Self::Dolby5_1_4,
            "7.1.2" => Self::Dolby7_1_2,
            "7.1.4" => Self::Dolby7_1_4,
            "9.1.2" => Self::Dolby9_1_2,
            "9.1.4" => Self::Dolby9_1_4,
            "9.1.6" => Self::Dolby9_1_6,
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stereo2_0 => "2.0",
            Self::Stereo2_1 => "2.1",
            Self::Dolby5_1 => "5.1",
            Self::Dolby5_1_2 => "5.1.2",
            Self::Dolby5_1_4 => "5.1.4",
            Self::Dolby7_1_2 => "7.1.2",
            Self::Dolby7_1_4 => "7.1.4",
            Self::Dolby9_1_2 => "9.1.2",
            Self::Dolby9_1_4 => "9.1.4",
            Self::Dolby9_1_6 => "9.1.6",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Speaker {
    pub name: &'static str,
    pub azimuth: f32,
    pub elevation: f32,
}

const FRONT: [Speaker; 3] = [
    Speaker { name: "FrontLeft", azimuth: 30.0, elevation: 0.0 },
    Speaker { name: "FrontRight", azimuth: -30.0, elevation: 0.0 },
    Speaker { name: "Center", azimuth: 0.0, elevation: 0.0 },
];
const SURROUND_5: [Speaker; 2] = [
    Speaker { name: "SurroundLeft", azimuth: 110.0, elevation: 0.0 },
    Speaker { name: "SurroundRight", azimuth: -110.0, elevation: 0.0 },
];
const SURROUND_7: [Speaker; 4] = [
    Speaker { name: "SurroundLeft", azimuth: 100.0, elevation: 0.0 },
    Speaker { name: "SurroundRight", azimuth: -100.0, elevation: 0.0 },
    Speaker { name: "RearLeft", azimuth: 140.0, elevation: 0.0 },
    Speaker { name: "RearRight", azimuth: -140.0, elevation: 0.0 },
];
const WIDE: [Speaker; 2] = [
    Speaker { name: "WideLeft", azimuth: 60.0, elevation: 0.0 },
    Speaker { name: "WideRight", azimuth: -60.0, elevation: 0.0 },
];
const TOP_FRONT: [Speaker; 2] = [
    Speaker { name: "TopFrontLeft", azimuth: 45.0, elevation: 45.0 },
    Speaker { name: "TopFrontRight", azimuth: -45.0, elevation: 45.0 },
];
const TOP_MIDDLE: [Speaker; 2] = [
    Speaker { name: "TopMiddleLeft", azimuth: 90.0, elevation: 45.0 },
    Speaker { name: "TopMiddleRight", azimuth: -90.0, elevation: 45.0 },
];
const TOP_REAR: [Speaker; 2] = [
    Speaker { name: "TopRearLeft", azimuth: 135.0, elevation: 45.0 },
    Speaker { name: "TopRearRight", azimuth: -135.0, elevation: 45.0 },
];

const LAYOUT_2_0: [Speaker; 2] = [FRONT[0], FRONT[1]];
const LAYOUT_5_1: [Speaker; 5] = [FRONT[0], FRONT[1], FRONT[2], SURROUND_5[0], SURROUND_5[1]];
const LAYOUT_5_1_2: [Speaker; 7] = [
    FRONT[0], FRONT[1], FRONT[2], SURROUND_5[0], SURROUND_5[1], TOP_MIDDLE[0], TOP_MIDDLE[1],
];
const LAYOUT_5_1_4: [Speaker; 9] = [
    FRONT[0], FRONT[1], FRONT[2], SURROUND_5[0], SURROUND_5[1], TOP_FRONT[0], TOP_FRONT[1], TOP_REAR[0], TOP_REAR[1],
];
const LAYOUT_7_1_2: [Speaker; 9] = [
    FRONT[0], FRONT[1], FRONT[2], SURROUND_7[0], SURROUND_7[1], SURROUND_7[2], SURROUND_7[3], TOP_MIDDLE[0], TOP_MIDDLE[1],
];
const LAYOUT_7_1_4: [Speaker; 11] = [
    FRONT[0], FRONT[1], FRONT[2], SURROUND_7[0], SURROUND_7[1], SURROUND_7[2], SURROUND_7[3], TOP_FRONT[0], TOP_FRONT[1], TOP_REAR[0], TOP_REAR[1],
];
const LAYOUT_9_1_2: [Speaker; 11] = [
    FRONT[0], FRONT[1], FRONT[2], WIDE[0], WIDE[1], SURROUND_7[0], SURROUND_7[1], SURROUND_7[2], SURROUND_7[3], TOP_MIDDLE[0], TOP_MIDDLE[1],
];
const LAYOUT_9_1_4: [Speaker; 13] = [
    FRONT[0], FRONT[1], FRONT[2], WIDE[0], WIDE[1], SURROUND_7[0], SURROUND_7[1], SURROUND_7[2], SURROUND_7[3], TOP_FRONT[0], TOP_FRONT[1], TOP_REAR[0], TOP_REAR[1],
];
const LAYOUT_9_1_6: [Speaker; 15] = [
    FRONT[0], FRONT[1], FRONT[2], WIDE[0], WIDE[1], SURROUND_7[0], SURROUND_7[1], SURROUND_7[2], SURROUND_7[3], TOP_FRONT[0], TOP_FRONT[1], TOP_MIDDLE[0], TOP_MIDDLE[1], TOP_REAR[0], TOP_REAR[1],
];

pub fn speakers(layout: LayoutId) -> &'static [Speaker] {
    match layout {
        LayoutId::Stereo2_0 | LayoutId::Stereo2_1 => &LAYOUT_2_0,
        LayoutId::Dolby5_1 => &LAYOUT_5_1,
        LayoutId::Dolby5_1_2 => &LAYOUT_5_1_2,
        LayoutId::Dolby5_1_4 => &LAYOUT_5_1_4,
        LayoutId::Dolby7_1_2 => &LAYOUT_7_1_2,
        LayoutId::Dolby7_1_4 => &LAYOUT_7_1_4,
        LayoutId::Dolby9_1_2 => &LAYOUT_9_1_2,
        LayoutId::Dolby9_1_4 => &LAYOUT_9_1_4,
        LayoutId::Dolby9_1_6 => &LAYOUT_9_1_6,
    }
}

#[derive(Clone)]
pub struct VbapSolver {
    layout: LayoutId,
    speakers: &'static [Speaker],
    dirs: Vec<[f32; 3]>,
    faces: Vec<Face>,
    pairs: Vec<Pair>,
}

#[derive(Clone)]
struct Face {
    speakers: [usize; 3],
    inverse: [[f32; 3]; 3],
}

#[derive(Clone)]
struct Pair {
    speakers: [usize; 2],
    inverse: [f32; 4],
}

impl VbapSolver {
    pub fn new() -> Self {
        Self::with_layout(LayoutId::Dolby7_1_4)
    }

    pub fn with_layout(layout: LayoutId) -> Self {
        let speakers = speakers(layout);
        let dirs: Vec<_> = speakers
            .iter()
            .map(|speaker| unit(speaker.azimuth, speaker.elevation))
            .collect();
        let mut faces = Vec::new();
        let mut pairs = Vec::new();
        let coplanar = dirs.iter().all(|direction| direction[2].abs() < DET_EPSILON);
        if coplanar {
            let mut order: Vec<_> = speakers
                .iter()
                .enumerate()
                .map(|(index, speaker)| (speaker.azimuth, index))
                .collect();
            order.sort_by(|left, right| left.0.total_cmp(&right.0));
            for index in 0..order.len() {
                let a = order[index].1;
                let b = order[(index + 1) % order.len()].1;
                let determinant = dirs[a][0] * dirs[b][1] - dirs[b][0] * dirs[a][1];
                if determinant.abs() >= DET_EPSILON {
                    pairs.push(Pair {
                        speakers: [a, b],
                        inverse: [
                            dirs[b][1] / determinant,
                            -dirs[b][0] / determinant,
                            -dirs[a][1] / determinant,
                            dirs[a][0] / determinant,
                        ],
                    });
                }
            }
        } else {
            for a in 0..dirs.len() {
                for b in a + 1..dirs.len() {
                    for c in b + 1..dirs.len() {
                        let basis = [
                            [dirs[a][0], dirs[b][0], dirs[c][0]],
                            [dirs[a][1], dirs[b][1], dirs[c][1]],
                            [dirs[a][2], dirs[b][2], dirs[c][2]],
                        ];
                        let Some(inverse) = inverse3(basis) else {
                            continue;
                        };
                        let normal = cross(sub(dirs[b], dirs[a]), sub(dirs[c], dirs[a]));
                        let plane = dot(normal, dirs[a]);
                        if plane.abs() < DET_EPSILON {
                            continue;
                        }
                        let mut positive = false;
                        let mut negative = false;
                        for (index, direction) in dirs.iter().enumerate() {
                            if index == a || index == b || index == c {
                                continue;
                            }
                            let side = dot(normal, *direction) - plane;
                            positive |= side > HULL_EPSILON;
                            negative |= side < -HULL_EPSILON;
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
        }
        Self { layout, speakers, dirs, faces, pairs }
    }

    pub fn layout(&self) -> LayoutId {
        self.layout
    }

    pub fn bus_count(&self) -> usize {
        self.speakers.len()
    }

    pub fn speaker_index(&self, name: &str) -> Option<usize> {
        self.speakers.iter().position(|speaker| speaker.name == name)
    }

    pub fn pan(&self, position: [f32; 3], spread: f32) -> [f32; MAX_BUS_COUNT] {
        let fallback = self.dirs.get(2).copied().unwrap_or([0.0, 1.0, 0.0]);
        let direction = normalize(position).unwrap_or(fallback);
        let mut gains = [0.0; MAX_BUS_COUNT];
        if !self.pairs.is_empty() {
            let horizontal = (direction[0] * direction[0] + direction[1] * direction[1]).sqrt();
            let horizontal = if horizontal > DET_EPSILON { horizontal } else { 1.0 };
            let x = direction[0] / horizontal;
            let y = direction[1] / horizontal;
            let mut best: Option<([f32; 2], &[usize; 2], f32)> = None;
            for pair in &self.pairs {
                let gain = [
                    pair.inverse[0] * x + pair.inverse[1] * y,
                    pair.inverse[2] * x + pair.inverse[3] * y,
                ];
                let minimum = gain[0].min(gain[1]);
                if minimum >= -GAIN_EPSILON
                    && best.as_ref().is_none_or(|(_, _, previous)| minimum > *previous)
                {
                    best = Some((gain, &pair.speakers, minimum));
                }
            }
            if let Some((gain, speakers, _)) = best {
                gains[speakers[0]] = gain[0].max(0.0);
                gains[speakers[1]] = gain[1].max(0.0);
            } else {
                self.nearest_gain([x, y, 0.0], &mut gains);
            }
        } else {
            let mut best: Option<([f32; 3], &[usize; 3], f32)> = None;
            for face in &self.faces {
                let gain = multiply(face.inverse, direction);
                let minimum = gain[0].min(gain[1]).min(gain[2]);
                if minimum >= -GAIN_EPSILON
                    && best.as_ref().is_none_or(|(_, _, previous)| minimum > *previous)
                {
                    best = Some((gain, &face.speakers, minimum));
                }
            }
            if let Some((gain, speakers, _)) = best {
                for index in 0..3 {
                    gains[speakers[index]] = gain[index].max(0.0);
                }
            } else {
                self.nearest_gain(direction, &mut gains);
            }
        }
        normalize_power(&mut gains[..self.bus_count()]);
        let spread = spread.clamp(0.0, 1.0);
        if spread > 0.0 {
            let count = self.bus_count().min(4);
            let mut nearest = [(f32::NEG_INFINITY, 0_usize); 4];
            for (index, speaker) in self.dirs.iter().enumerate() {
                let value = dot(*speaker, direction);
                for rank in 0..count {
                    if value <= nearest[rank].0 {
                        continue;
                    }
                    for move_index in (rank + 1..count).rev() {
                        nearest[move_index] = nearest[move_index - 1];
                    }
                    nearest[rank] = (value, index);
                    break;
                }
            }
            let diffuse = 1.0 / (count as f32).sqrt();
            for (_, index) in nearest.into_iter().take(count) {
                gains[index] = (1.0 - spread) * gains[index] + spread * diffuse;
            }
            normalize_power(&mut gains[..self.bus_count()]);
        }
        gains
    }

    fn nearest_gain(&self, direction: [f32; 3], gains: &mut [f32; MAX_BUS_COUNT]) {
        let nearest = self
            .dirs
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| dot(**left, direction).total_cmp(&dot(**right, direction)))
            .map(|(index, _)| index)
            .unwrap_or(0);
        gains[nearest] = 1.0;
    }

    pub fn speaker_direction(&self, bus: usize) -> (f32, f32) {
        let speaker = self.speakers[bus];
        (speaker.azimuth, speaker.elevation)
    }
}

fn unit(azimuth: f32, elevation: f32) -> [f32; 3] {
    let azimuth = azimuth.to_radians();
    let elevation = elevation.to_radians();
    [
        -elevation.cos() * azimuth.sin(),
        elevation.cos() * azimuth.cos(),
        elevation.sin(),
    ]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(value: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(value, value).sqrt();
    (length > DET_EPSILON).then(|| [value[0] / length, value[1] / length, value[2] / length])
}
fn normalize_power(gains: &mut [f32]) {
    let power = gains.iter().map(|gain| gain * gain).sum::<f32>();
    if power > 0.0 {
        let scale = power.sqrt().recip();
        for gain in gains { *gain *= scale; }
    }
}
fn multiply(matrix: [[f32; 3]; 3], vector: [f32; 3]) -> [f32; 3] {
    [dot(matrix[0], vector), dot(matrix[1], vector), dot(matrix[2], vector)]
}
fn inverse3(rows: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let determinant = rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
        - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
        + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0]);
    if determinant.abs() < DET_EPSILON { return None; }
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
    fn master_layout_bus_counts_are_preserved() {
        assert_eq!(VbapSolver::with_layout(LayoutId::Stereo2_0).bus_count(), 2);
        assert_eq!(VbapSolver::with_layout(LayoutId::Dolby5_1).bus_count(), 5);
        assert_eq!(VbapSolver::with_layout(LayoutId::Dolby5_1_4).bus_count(), 9);
        assert_eq!(VbapSolver::with_layout(LayoutId::Dolby7_1_4).bus_count(), 11);
        assert_eq!(VbapSolver::with_layout(LayoutId::Dolby9_1_6).bus_count(), 15);
    }

    #[test]
    fn horizontal_5_1_uses_continuous_pair_panning() {
        let solver = VbapSolver::with_layout(LayoutId::Dolby5_1);
        let mut previous = solver.pan([0.0, 1.0, 0.0], 0.0);
        for degrees in 1..=360 {
            let radians = (degrees as f32).to_radians();
            let next = solver.pan([radians.sin(), radians.cos(), 0.0], 0.0);
            let delta = previous
                .iter()
                .zip(next)
                .map(|(left, right)| (left - right).abs())
                .sum::<f32>();
            assert!(delta < 0.2, "5.1 orbit discontinuity at {degrees}: {delta}");
            previous = next;
        }
    }

    #[test]
    fn layouts_change_object_distribution() {
        let position = [0.8, 0.6, 0.35];
        let five = VbapSolver::with_layout(LayoutId::Dolby5_1).pan(position, 0.0);
        let nine = VbapSolver::with_layout(LayoutId::Dolby9_1_6).pan(position, 0.0);
        assert_ne!(&five[..5], &nine[..5]);
        assert!(nine[9..15].iter().any(|gain| *gain > 0.0));
    }

    #[test]
    fn every_layout_is_power_normalized() {
        for layout in [
            LayoutId::Dolby5_1,
            LayoutId::Dolby5_1_2,
            LayoutId::Dolby5_1_4,
            LayoutId::Dolby7_1_2,
            LayoutId::Dolby7_1_4,
            LayoutId::Dolby9_1_2,
            LayoutId::Dolby9_1_4,
            LayoutId::Dolby9_1_6,
        ] {
            let solver = VbapSolver::with_layout(layout);
            let gains = solver.pan([0.25, 0.75, 0.35], 0.4);
            let power = gains[..solver.bus_count()].iter().map(|gain| gain * gain).sum::<f32>();
            assert!((power - 1.0).abs() < 1e-4, "{} is not normalized", layout.as_str());
        }
    }
}
