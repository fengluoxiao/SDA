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
    diffuse_input: Vec<[f32; vbap::MAX_BUS_COUNT]>,
    diffuser: crate::source_extent::Diffuser,
    stereo_bass: Option<FrontBass>,
    stereo_bass_enabled: bool,
}

#[derive(Clone)]
struct FrontBass {
    indices: Vec<usize>,
    normal: Vec<convolution::PreparedStereoFilter>,
    preserved: Vec<convolution::PreparedStereoFilter>,
}

struct Bus {
    perf_id: String,
    perf_stage: &'static str,
    hardware: crate::hardware::Chain,
    background_filter: crate::focus::BackgroundFilter,
    convolver: convolution::StereoPartitionedConvolver,
    input: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
}

fn finish_bus(bus: &mut Bus) -> Result<(), String> {
    let _perf = crate::performance::span(
        bus.perf_stage,
        &bus.perf_id,
        convolution::DEFAULT_PARTITION as u64,
    );
    bus.left.fill(0.0);
    bus.right.fill(0.0);
    for sample in &mut bus.input {
        *sample = bus.hardware.process(*sample);
    }
    bus.convolver
        .process_block(&bus.input, &mut bus.left, &mut bus.right)
}

impl BusRenderer {
    pub(super) fn new(
        set: &hrtf::NativeHrtfSet,
        solver: &vbap::VbapSolver,
        wet_weight: f32,
    ) -> Result<Self, String> {
        let bass = crate::stereo_bass::prepare(set, solver, wet_weight)?;
        let mut buses = Vec::with_capacity(solver.bus_count());
        let mut reflections = Vec::with_capacity(solver.bus_count());
        for index in 0..solver.bus_count() {
            let (azimuth, elevation) = solver.speaker_direction(index);
            let (mut left, mut right) = set.mixed_speaker(
                vbap::speakers(solver.layout())[index].name,
                solver.layout().as_str(),
                azimuth as f64,
                elevation as f64,
                wet_weight,
            )?;
            let (dry_left, dry_right) = set.mixed_speaker(
                vbap::speakers(solver.layout())[index].name,
                solver.layout().as_str(),
                azimuth as f64,
                elevation as f64,
                0.0,
            )?;
            if let Some(b) = &bass {
                if let Some(i) = b.indices.iter().position(|v| *v == index) {
                    left = b.normal[i].0.clone();
                    right = b.normal[i].1.clone();
                }
            }
            let residual_left: Vec<_> = left
                .iter()
                .zip(dry_left.into_iter().chain(std::iter::repeat(0.)))
                .map(|(a, b)| a - b)
                .collect();
            let residual_right: Vec<_> = right
                .iter()
                .zip(dry_right.into_iter().chain(std::iter::repeat(0.)))
                .map(|(a, b)| a - b)
                .collect();
            reflections.push(Bus {
                perf_id: vbap::speakers(solver.layout())[index].name.into(),
                perf_stage: "hrtf.reflection_bus",
                hardware: crate::hardware::Chain::new(&set.cinema.monitor.hardware),
                background_filter: Default::default(),
                convolver: convolution::StereoPartitionedConvolver::new(
                    &residual_left,
                    &residual_right,
                    convolution::DEFAULT_PARTITION,
                )?,
                input: vec![0.0; convolution::DEFAULT_PARTITION],
                left: vec![0.0; convolution::DEFAULT_PARTITION],
                right: vec![0.0; convolution::DEFAULT_PARTITION],
            });
            buses.push(Bus {
                perf_id: vbap::speakers(solver.layout())[index].name.into(),
                perf_stage: "hrtf.bed_bus",
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
        let stereo_bass = bass.map(|b| {
            let normal = b
                .indices
                .iter()
                .map(|&index| buses[index].convolver.prepared_filter())
                .collect();
            let preserved = b
                .indices
                .iter()
                .enumerate()
                .map(|(i, &index)| {
                    buses[index]
                        .convolver
                        .prepare_pair(&b.preserved[i].0, &b.preserved[i].1)
                })
                .collect();
            FrontBass {
                indices: b.indices,
                normal,
                preserved,
            }
        });
        Ok(Self {
            stereo_bass,
            stereo_bass_enabled: false,
            buses,
            reflections,
            reflections_active: false,
            diffuse_input: vec![[0.0; vbap::MAX_BUS_COUNT]; convolution::DEFAULT_PARTITION],
            diffuser: crate::source_extent::Diffuser::new(),
        })
    }

    pub(super) fn stereo_bass_preservation_active(&self) -> bool {
        self.stereo_bass_enabled
    }

    pub(super) fn set_stereo_bass_preservation(&mut self, enabled: bool) {
        let enabled = enabled && self.stereo_bass.is_some();
        if enabled == self.stereo_bass_enabled {
            return;
        }
        self.stereo_bass_enabled = enabled;
        if let Some(b) = &self.stereo_bass {
            for i in 0..b.indices.len() {
                let filter = if enabled {
                    b.preserved[i].clone()
                } else {
                    b.normal[i].clone()
                };
                self.buses[b.indices[i]]
                    .convolver
                    .transition_to(filter, convolution::DEFAULT_PARTITION);
            }
        }
    }

    pub(super) fn transition_filters_from(&mut self, prepared: &Self) {
        // Same layout and asset length: preserve PCM history, hardware and tails.
        for (index, (old, new)) in self.buses.iter_mut().zip(&prepared.buses).enumerate() {
            let filter = if self.stereo_bass_enabled {
                prepared
                    .stereo_bass
                    .as_ref()
                    .and_then(|b| {
                        b.indices
                            .iter()
                            .position(|i| *i == index)
                            .map(|i| b.preserved[i].clone())
                    })
                    .unwrap_or_else(|| new.convolver.prepared_filter())
            } else {
                new.convolver.prepared_filter()
            };
            old.convolver
                .transition_to(filter, convolution::DEFAULT_PARTITION);
        }
        for (old, new) in self.reflections.iter_mut().zip(&prepared.reflections) {
            old.convolver.transition_to(
                new.convolver.prepared_filter(),
                convolution::DEFAULT_PARTITION,
            );
        }
        self.stereo_bass = prepared.stereo_bass.clone();
        self.stereo_bass_enabled &= self.stereo_bass.is_some();
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

    pub(super) fn add_accumulated(&mut self, frame: usize, input: &crate::object_mixer::Frame) {
        for (i, bus) in self.buses.iter_mut().enumerate() {
            bus.input[frame] += input.main[i];
        }
        for (i, bus) in self.reflections.iter_mut().enumerate() {
            bus.input[frame] += input.reflections[i];
        }
        self.reflections_active |= input.reflections.iter().any(|x| *x != 0.0);
        for (out, value) in self.diffuse_input[frame].iter_mut().zip(input.diffuse) {
            *out += value;
        }
    }
    pub(super) fn add_diffuse(&mut self, bus: usize, sample: f32, frame: usize) {
        self.diffuse_input[frame][bus] += sample;
    }
    pub(super) fn add_reflections(
        &mut self,
        sample: f32,
        gains: &[f32; vbap::MAX_BUS_COUNT],
        frame: usize,
    ) {
        if sample != 0.0 {
            self.reflections_active = true;
        }
        for (bus, gain) in self.reflections.iter_mut().zip(gains) {
            if *gain != 0.0 {
                bus.input[frame] += sample * gain;
            }
        }
    }
    pub(super) fn finish_diffuse_frame(&mut self, frame: usize) {
        let values = self.diffuser.process_inputs(std::mem::replace(
            &mut self.diffuse_input[frame],
            [0.0; vbap::MAX_BUS_COUNT],
        ));
        for (i, bus) in self.buses.iter_mut().enumerate() {
            bus.input[frame] += values[i];
        }
    }

    pub(super) fn output_at(&self, frame: usize) -> [f32; 2] {
        self.buses
            .iter()
            .chain(self.reflections.iter().filter(|_| self.reflections_active))
            .fold([0.0, 0.0], |mut output, bus| {
                output[0] += bus.left[frame];
                output[1] += bus.right[frame];
                output
            })
    }

    pub(super) fn shape_background(&mut self, frame: usize, amounts: &[f32; vbap::MAX_BUS_COUNT]) {
        let active = self.reflections_active;
        for (bus, amount) in self
            .buses
            .iter_mut()
            .zip(amounts)
            .chain(self.reflections.iter_mut().zip(amounts).filter(|_| active))
        {
            let input = bus.input[frame];
            let filtered = bus.background_filter.process(input);
            bus.input[frame] = input + (filtered - input) * amount;
        }
    }

    pub(super) fn finish_block(&mut self) -> Result<(), String> {
        use rayon::prelude::*;
        let active = self.reflections_active;
        let count = self.buses.len() + usize::from(active) * self.reflections.len();
        if (cfg!(any(target_os = "ios", feature = "caller-participating-dsp"))
            || crate::realtime::android_audio_policy())
        {
            crate::audio_executor::run(&mut self.buses, finish_bus)?;
            if active {
                crate::audio_executor::run(&mut self.reflections, finish_bus)?;
            }
        } else {
            if let Some(pool) = crate::direct_renderer::workers().filter(|_| count >= 8) {
                if active {
                    pool.install(|| {
                        self.buses
                            .par_iter_mut()
                            .chain(self.reflections.par_iter_mut())
                            .try_for_each(finish_bus)
                    })?;
                } else {
                    pool.install(|| self.buses.par_iter_mut().try_for_each(finish_bus))?;
                }
            } else if active {
                self.buses
                    .iter_mut()
                    .chain(&mut self.reflections)
                    .try_for_each(finish_bus)?;
            } else {
                self.buses.iter_mut().try_for_each(finish_bus)?;
            }
        }
        if active
            && self
                .reflections
                .iter()
                .all(|b| b.convolver.tail_is_silent())
        {
            self.reflections_active = false;
        }
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        self.reflections_active = false;
        self.diffuser = crate::source_extent::Diffuser::new();
        self.diffuse_input.fill([0.0; vbap::MAX_BUS_COUNT]);
        for bus in self.buses.iter_mut().chain(&mut self.reflections) {
            bus.convolver.reset();
            bus.hardware.reset();
            bus.background_filter = crate::focus::BackgroundFilter::default();
            bus.input.fill(0.0);
            let _perf = crate::performance::span(
                bus.perf_stage,
                &bus.perf_id,
                convolution::DEFAULT_PARTITION as u64,
            );
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
    } else {
        route(solver, position, head_pose, spread)
    };
    let diffuse = diffuse.clamp(0.0, 1.0);
    if diffuse > 0.0 {
        let allowed: Vec<usize> = (0..solver.bus_count())
            .filter(|i| !horizontal_only || solver.speaker_direction(*i).1.abs() < 1e-3)
            .collect();
        let count = allowed.len();
        for index in allowed {
            gains[index] =
                ((1.0 - diffuse) * gains[index] * gains[index] + diffuse / count as f32).sqrt();
        }
    }
    gains
}

pub(super) fn route_zoned(
    solver: &vbap::VbapSolver,
    position: [f32; 3],
    head_pose: Option<[f32; 4]>,
    spread: f32,
    diffuse: f32,
    horizontal_only: bool,
    zones: &[crate::adm_zone::Zone],
) -> [f32; vbap::MAX_BUS_COUNT] {
    let mut gains = route_diffuse(
        solver,
        position,
        head_pose,
        spread,
        diffuse,
        horizontal_only,
    );
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
                if horizontal && solver.speaker_direction(index).1 != 0.0 {
                    assert_eq!(*gain, 0.0);
                } else {
                    assert!(*gain > 0.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod bass_preservation_tests {
    use super::*;
    #[test]
    fn stereo_bass_does_not_move_or_reprocess_any_rear_or_height_bus() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        let mut set = hrtf::NativeHrtfSet::load_calibrated(&path).unwrap();
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        set.configure_spatial_cues(solver.layout()).unwrap();
        let wet = set.effective_wet(0.);
        let mut a = BusRenderer::new(&set, &solver, wet).unwrap();
        let mut b = BusRenderer::new(&set, &solver, wet).unwrap();
        b.set_stereo_bass_preservation(true);
        assert!(b.stereo_bass_enabled);
        for (index, speaker) in vbap::speakers(solver.layout()).iter().enumerate() {
            if matches!(speaker.name, "FrontLeft" | "FrontRight" | "Center") {
                continue;
            }
            let mut input = [0f32; convolution::DEFAULT_PARTITION];
            input[0] = 0.1;
            for block in 0..160 {
                if block > 0 {
                    input.fill(0.);
                }
                let (mut al, mut ar, mut bl, mut br) = (
                    [0.; convolution::DEFAULT_PARTITION],
                    [0.; convolution::DEFAULT_PARTITION],
                    [0.; convolution::DEFAULT_PARTITION],
                    [0.; convolution::DEFAULT_PARTITION],
                );
                a.buses[index]
                    .convolver
                    .process_block(&input, &mut al, &mut ar)
                    .unwrap();
                b.buses[index]
                    .convolver
                    .process_block(&input, &mut bl, &mut br)
                    .unwrap();
                assert_eq!(al, bl, "rear/height left response changed: {index}");
                assert_eq!(ar, br, "rear/height right response changed: {index}");
            }
        }
    }
}

#[cfg(test)]
mod stereo_front_tests {
    use super::*;
    #[test]
    fn stereo_front_calibration_keeps_rear_height_and_reflection_pcm_identical() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        let mut set = hrtf::NativeHrtfSet::load_calibrated(&path).unwrap();
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        set.configure_spatial_cues(solver.layout()).unwrap();
        let mut normal = BusRenderer::new(&set, &solver, set.effective_wet(0.)).unwrap();
        let mut candidate = BusRenderer::new(&set, &solver, set.effective_wet(0.)).unwrap();
        candidate.set_stereo_bass_preservation(true);
        let n = convolution::DEFAULT_PARTITION;
        // Let the filter transition finish before delivering the fixture.
        for _ in 0..2 {
            normal.begin_block();
            candidate.begin_block();
            normal.finish_block().unwrap();
            candidate.finish_block().unwrap();
        }
        for block in 0..40 {
            normal.begin_block();
            candidate.begin_block();
            if block == 0 {
                for (i, s) in vbap::speakers(solver.layout()).iter().enumerate() {
                    if matches!(s.name, "FrontLeft" | "FrontRight" | "Center") {
                        continue;
                    }
                    for bank in [&mut normal, &mut candidate] {
                        bank.buses[i].input[0] = 0.01 * (i + 1) as f32;
                        bank.reflections[i].input[0] = -0.003 * (i + 1) as f32;
                        bank.reflections_active = true;
                    }
                }
            }
            normal.finish_block().unwrap();
            candidate.finish_block().unwrap();
            for i in 0..n {
                assert_eq!(
                    normal.output_at(i),
                    candidate.output_at(i),
                    "non-front content changed at block={block} sample={i}"
                );
            }
        }
    }
}
