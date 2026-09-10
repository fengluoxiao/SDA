//! Layout-specific virtual-speaker bus renderer.
//!
//! Sources are mixed into the currently selected room's physical virtual
//! speakers before HRTF filtering. The graph is owned by the render worker;
//! each layout therefore changes both physical geometry and convolution count.

use crate::{convolution, hrtf, spatial, vbap};

pub(super) struct BusRenderer {
    buses: Vec<Bus>,
    reflections: Vec<Bus>,
    reflections_active: bool,
    diffuse_input: Vec<[f32;vbap::MAX_BUS_COUNT]>,
    diffuser: crate::source_extent::Diffuser,
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
        let mut reflections=Vec::with_capacity(solver.bus_count());
        for index in 0..solver.bus_count() {
            let (azimuth, elevation) = solver.speaker_direction(index);
            let (left, right) = set.mixed_speaker(vbap::speakers(solver.layout())[index].name,
                solver.layout().as_str(), azimuth as f64, elevation as f64, wet_weight)?;
            let (dry_left,dry_right)=set.mixed_speaker(vbap::speakers(solver.layout())[index].name,
                solver.layout().as_str(),azimuth as f64,elevation as f64,0.0)?;
            let residual_left:Vec<_>=left.iter().zip(dry_left).map(|(a,b)|a-b).collect();
            let residual_right:Vec<_>=right.iter().zip(dry_right).map(|(a,b)|a-b).collect();
            reflections.push(Bus {
                hardware:crate::hardware::Chain::new(&set.cinema.monitor.hardware),background_filter:Default::default(),
                convolver:convolution::StereoPartitionedConvolver::new(&residual_left,&residual_right,convolution::DEFAULT_PARTITION)?,
                input:vec![0.0;convolution::DEFAULT_PARTITION],left:vec![0.0;convolution::DEFAULT_PARTITION],right:vec![0.0;convolution::DEFAULT_PARTITION]
            });
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
        Ok(Self { buses, reflections, reflections_active:false, diffuse_input:vec![[0.0;vbap::MAX_BUS_COUNT];convolution::DEFAULT_PARTITION], diffuser:crate::source_extent::Diffuser::new() })
    }

    pub(super) fn bus_count(&self) -> usize {
        self.buses.len()
    }

    pub(super) fn begin_block(&mut self) {
        for bus in self.buses.iter_mut().chain(&mut self.reflections) {
            bus.input.fill(0.0);
        }
    }

    pub(super) fn add(&mut self, sample: f32, gains: &[f32; vbap::MAX_BUS_COUNT], frame: usize) {
        for (bus, gain) in self.buses.iter_mut().zip(gains) {
            bus.input[frame] += sample * gain;
        }
    }

    pub(super) fn add_accumulated(&mut self,frame:usize,input:&crate::object_mixer::Frame) {
        for (i,bus) in self.buses.iter_mut().enumerate(){bus.input[frame]+=input.main[i];}
        for (i,bus) in self.reflections.iter_mut().enumerate(){bus.input[frame]+=input.reflections[i];}
        self.reflections_active|=input.reflections.iter().any(|x|*x!=0.0);
        for (out,value) in self.diffuse_input[frame].iter_mut().zip(input.diffuse){*out+=value;}
    }
    pub(super) fn add_diffuse(&mut self,bus:usize,sample:f32,frame:usize) { self.diffuse_input[frame][bus]+=sample; }
    pub(super) fn add_reflections(&mut self,sample:f32,gains:&[f32;vbap::MAX_BUS_COUNT],frame:usize){
        if sample!=0.0 {self.reflections_active=true;}
        for (bus,gain) in self.reflections.iter_mut().zip(gains){if *gain!=0.0{bus.input[frame]+=sample*gain;}}
    }
    pub(super) fn finish_diffuse_frame(&mut self,frame:usize) {
        let values=self.diffuser.process_inputs(std::mem::replace(&mut self.diffuse_input[frame],[0.0;vbap::MAX_BUS_COUNT]));
        for (i,bus) in self.buses.iter_mut().enumerate(){bus.input[frame]+=values[i];}
    }

    pub(super) fn output_at(&self, frame: usize) -> [f32; 2] {
        self.buses.iter().chain(self.reflections.iter().filter(|_|self.reflections_active)).fold([0.0, 0.0], |mut output, bus| {
            output[0] += bus.left[frame];
            output[1] += bus.right[frame];
            output
        })
    }

    pub(super) fn shape_background(&mut self, frame: usize, amounts: &[f32; vbap::MAX_BUS_COUNT]) {
        let active=self.reflections_active;
        for (bus, amount) in self.buses.iter_mut().zip(amounts).chain(self.reflections.iter_mut().zip(amounts).filter(|_|active)) {
            let input = bus.input[frame];
            let filtered = bus.background_filter.process(input);
            bus.input[frame] = input + (filtered - input) * amount;
        }
    }

    pub(super) fn finish_block(&mut self) -> Result<(), String> {
        let active=self.reflections_active;
        for bus in self.buses.iter_mut().chain(self.reflections.iter_mut().filter(|_|active)) {
            bus.left.fill(0.0);
            bus.right.fill(0.0);
            for sample in &mut bus.input { *sample = bus.hardware.process(*sample); }
            bus.convolver
                .process_block(&bus.input, &mut bus.left, &mut bus.right)?;
        }
        if active && self.reflections.iter().all(|b|b.convolver.tail_is_silent()){self.reflections_active=false;}
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        self.reflections_active=false;
        self.diffuser=crate::source_extent::Diffuser::new();
        self.diffuse_input.fill([0.0;vbap::MAX_BUS_COUNT]);
        for bus in self.buses.iter_mut().chain(&mut self.reflections) {
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
