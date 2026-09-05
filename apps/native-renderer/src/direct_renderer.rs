//! Per-object convolution; shares measured assets with the speaker renderer.
use crate::{convolution::{StereoPartitionedConvolver, DEFAULT_PARTITION}, hrtf::NativeHrtfSet, spatial};

pub(super) struct DirectSource {
    convolver: StereoPartitionedConvolver,
    direction: Vec<(f64, f64)>,
    spread: f32,
    pose: Option<([f32; 3], Option<[f32; 4]>, f32)>,
    pub input: [f32; DEFAULT_PARTITION],
    pub left: [f32; DEFAULT_PARTITION],
    pub right: [f32; DEFAULT_PARTITION],
}

impl DirectSource {
    pub fn new(set: &NativeHrtfSet, wet: f32) -> Result<Self, String> {
        let (_, _, left, right) = set.mixed_nearest(0.0, 0.0, wet)?;
        Ok(Self {
            convolver: StereoPartitionedConvolver::new(&left, &right, DEFAULT_PARTITION)?,
            direction: Vec::new(), spread: -1.0, pose: None,
            input: [0.0; DEFAULT_PARTITION], left: [0.0; DEFAULT_PARTITION], right: [0.0; DEFAULT_PARTITION],
        })
    }

    pub fn update(&mut self, set: &mut NativeHrtfSet, wet: f32, position: [f32; 3], head: Option<[f32; 4]>, spread: f32) -> Result<(), String> {
        let pose = (position, head, spread);
        if self.pose == Some(pose) { return Ok(()); }
        let angle = spatial::adm_to_spherical(spatial::head_relative_adm(position, head));
        let directions = set.nearest_directions(angle.azimuth as f64, angle.elevation as f64, if spread > 0.0 { 3 } else { 1 })?;
        if directions != self.direction || spread != self.spread {
            let mut filter = set.prepared_direction(directions[0].0, directions[0].1, wet)?;
            // Local spread blends measured filters with unity total weight.
            if directions.len() == 3 {
                let mut sides = set.prepared_direction(directions[1].0, directions[1].1, wet)?;
                sides.blend(&set.prepared_direction(directions[2].0, directions[2].1, wet)?, 0.5);
                filter.blend(&sides, spread.clamp(0.0, 1.0) * (2.0 / 3.0));
            }
            self.convolver.transition_to(filter, 1536);
            self.direction = directions;
            self.spread = spread;
        }
        self.pose = Some(pose);
        Ok(())
    }

    pub fn finish_block(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.convolver.process_block(&self.input, &mut self.left, &mut self.right).expect("fixed block dimensions");
        self.input.fill(0.0);
    }
}
