//! Layout-specific virtual-speaker bus renderer.
//!
//! Sources are mixed into the currently selected room's physical virtual
//! speakers before HRTF filtering. The graph is owned by the render worker;
//! each layout therefore changes both physical geometry and convolution count.

use crate::{convolution, hrtf, spatial, vbap};

pub(super) struct BusRenderer {
    buses: Vec<Bus>,
}

struct Bus {
    convolver: convolution::StereoPartitionedConvolver,
    input: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl BusRenderer {
    pub(super) fn new(
        set: &hrtf::NativeHrtfSet,
        solver: &vbap::VbapSolver,
        wet_weight: f32,
    ) -> Result<Self, String> {
        let mut buses = Vec::with_capacity(solver.bus_count());
        for index in 0..solver.bus_count() {
            let (azimuth, elevation) = solver.speaker_direction(index);
            let (_, _, left, right) =
                set.mixed_nearest(azimuth as f64, elevation as f64, wet_weight)?;
            buses.push(Bus {
                convolver: convolution::StereoPartitionedConvolver::new(
                    &left,
                    &right,
                    convolution::DEFAULT_PARTITION,
                )?,
                input: vec![0.0; convolution::DEFAULT_PARTITION],
                left: vec![0.0; convolution::DEFAULT_PARTITION],
                right: vec![0.0; convolution::DEFAULT_PARTITION],
            });
        }
        Ok(Self { buses })
    }

    pub(super) fn bus_count(&self) -> usize {
        self.buses.len()
    }

    pub(super) fn begin_block(&mut self) {
        for bus in &mut self.buses {
            bus.input.fill(0.0);
        }
    }

    pub(super) fn add(&mut self, sample: f32, gains: &[f32; vbap::MAX_BUS_COUNT], frame: usize) {
        for (bus, gain) in self.buses.iter_mut().zip(gains) {
            bus.input[frame] += sample * gain;
        }
    }

    pub(super) fn output_at(&self, frame: usize) -> [f32; 2] {
        self.buses.iter().fold([0.0, 0.0], |mut output, bus| {
            output[0] += bus.left[frame];
            output[1] += bus.right[frame];
            output
        })
    }

    pub(super) fn finish_block(&mut self) -> Result<(), String> {
        for bus in &mut self.buses {
            bus.left.fill(0.0);
            bus.right.fill(0.0);
            bus.convolver
                .process_block(&bus.input, &mut bus.left, &mut bus.right)?;
        }
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        for bus in &mut self.buses {
            bus.convolver.reset();
            bus.input.fill(0.0);
            bus.left.fill(0.0);
            bus.right.fill(0.0);
        }
    }
}

pub(super) fn route(
    solver: &vbap::VbapSolver,
    position: [f32; 3],
    head_pose: Option<[f32; 4]>,
    spread: f32,
) -> [f32; vbap::MAX_BUS_COUNT] {
    solver.pan(spatial::head_relative_adm(position, head_pose), spread)
}
