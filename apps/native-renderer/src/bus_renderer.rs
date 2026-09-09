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
    hardware: crate::hardware::Chain,
    background_filter: crate::focus::BackgroundFilter,
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
            let (left, right) = set.mixed_speaker(vbap::speakers(solver.layout())[index].name,
                solver.layout().as_str(), azimuth as f64, elevation as f64, wet_weight)?;
            buses.push(Bus {
                hardware: crate::hardware::Chain::new(&set.cinema.monitor.hardware),
                background_filter: crate::focus::BackgroundFilter::default(),
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

    pub(super) fn shape_background(&mut self, frame: usize, amounts: &[f32; vbap::MAX_BUS_COUNT]) {
        for (bus, amount) in self.buses.iter_mut().zip(amounts) {
            let input = bus.input[frame];
            let filtered = bus.background_filter.process(input);
            bus.input[frame] = input + (filtered - input) * amount;
        }
    }

    pub(super) fn finish_block(&mut self) -> Result<(), String> {
        for bus in &mut self.buses {
            bus.left.fill(0.0);
            bus.right.fill(0.0);
            for sample in &mut bus.input { *sample = bus.hardware.process(*sample); }
            bus.convolver
                .process_block(&bus.input, &mut bus.left, &mut bus.right)?;
        }
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        for bus in &mut self.buses {
            bus.convolver.reset();
            bus.hardware.reset();
            bus.background_filter = crate::focus::BackgroundFilter::default();
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

pub(super) fn route_diffuse(
    solver: &vbap::VbapSolver,
    position: [f32; 3],
    head_pose: Option<[f32; 4]>,
    spread: f32,
    diffuse: f32,
    horizontal_only: bool,
) -> [f32; vbap::MAX_BUS_COUNT] {
    let mut gains = if horizontal_only {
        solver.pan_horizontal(spatial::head_relative_adm(position, head_pose), spread)
    } else { route(solver, position, head_pose, spread) };
    let diffuse = diffuse.clamp(0.0, 1.0);
    if diffuse > 0.0 {
        let allowed: Vec<usize> = (0..solver.bus_count()).filter(|i| !horizontal_only || solver.speaker_direction(*i).1.abs() < 1e-3).collect();
        let count = allowed.len();
        for index in allowed {
            gains[index] = ((1.0 - diffuse) * gains[index] * gains[index] + diffuse / count as f32).sqrt();
        }
    }
    gains
}

pub(super) fn route_zoned(
    solver: &vbap::VbapSolver, position: [f32; 3], head_pose: Option<[f32; 4]>,
    spread: f32, diffuse: f32, horizontal_only: bool, zones: &[crate::adm_zone::Zone],
) -> [f32; vbap::MAX_BUS_COUNT] {
    let mut gains = route_diffuse(solver, position, head_pose, spread, diffuse, horizontal_only);
    crate::adm_zone::apply(&mut gains, solver, zones);
    gains
}

#[cfg(test)]
mod adm_tests {
    use super::*;
    #[test]
    fn diffuse_energy_and_horizontal_exclusions() {
        let solver = vbap::VbapSolver::new();
        for horizontal in [false, true] {
            let gains = route_diffuse(&solver, [0.0, 0.0, 1.0], None, 0.0, 1.0, horizontal);
            let energy: f32 = gains.iter().map(|gain| gain * gain).sum();
            assert!((energy - 1.0).abs() < 1e-5);
            for (index, gain) in gains.iter().enumerate().take(solver.bus_count()) {
                if horizontal && solver.speaker_direction(index).1 != 0.0 { assert_eq!(*gain, 0.0); }
                else { assert!(*gain > 0.0); }
            }
        }
    }
}
