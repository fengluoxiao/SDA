//! Per-object convolution; shares measured assets with the speaker renderer.
use crate::{convolution::{StereoPartitionedConvolver, DEFAULT_PARTITION}, hrtf::NativeHrtfSet, vbap};
use std::sync::{Arc, OnceLock};
use rayon::prelude::*;

type Route = (vbap::LayoutId, [f32; vbap::MAX_BUS_COUNT], [f32; vbap::MAX_BUS_COUNT], u32);
type FilterBank = Vec<[Arc<crate::convolution::PreparedStereoFilter>; 2]>;

pub(super) fn workers() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let count = std::thread::available_parallelism().map_or(1, usize::from).saturating_sub(2).clamp(1, 4);
        if count == 1 { return None; }
        rayon::ThreadPoolBuilder::new().num_threads(count)
            .thread_name(|id| format!("sda-object-hrtf-{id}"))
            .build().ok()
    }).as_ref()
}

fn prepare_bank(set: &mut NativeHrtfSet, solver: &vbap::VbapSolver, wet: f32) -> Result<FilterBank, String> {
    vbap::speakers(solver.layout()).iter().map(|speaker| {
        Ok([
            set.prepared_focus_speaker(speaker.name, solver.layout().as_str(), speaker.azimuth as f64, speaker.elevation as f64, wet, false)?,
            set.prepared_focus_speaker(speaker.name, solver.layout().as_str(), speaker.azimuth as f64, speaker.elevation as f64, wet, true)?,
        ])
    }).collect()
}

pub(super) fn finish_sources<'a>(sources: impl Iterator<Item = &'a mut DirectSource>,
    set: &mut NativeHrtfSet, solver: &vbap::VbapSolver, wet: f32) -> Result<(), String> {
    let mut sources: Vec<_> = sources.collect();
    let bank = if sources.iter().any(|source| source.needs_processing()
        && (source.pending_route.is_some() || source.idle_route.is_some())) {
        prepare_bank(set, solver, wet).map(Some)
    } else { Ok(None) };
    let finish = |source: &mut &mut DirectSource| {
        if !source.needs_processing() {
            if let Some(route) = source.pending_route.take() { source.idle_route = Some(route); }
        } else if let Ok(Some(bank)) = &bank {
            if let Some(route) = source.idle_route.take() {
                // No history remains, but the last silent block's direction
                // still defines the starting filter of an audible crossfade.
                source.route = None;
                source.update_from_bank(bank, route);
            }
            if let Some(route) = source.pending_route.take() { source.update_from_bank(bank, route); }
        }
        source.finish_block();
    };
    if let Some(pool) = workers().filter(|_| sources.len() >= 16) {
        // Independent histories stay with their sources; summation order in
        // the engine is unchanged. Join before publishing the next PCM block.
        pool.install(|| sources.par_iter_mut().with_min_len(4).for_each(finish));
    } else {
        sources.iter_mut().for_each(finish);
    }
    bank.map(|_| ())
}

pub(super) struct DirectSource {
    convolver: StereoPartitionedConvolver,
    route: Option<Route>,
    pending_route: Option<Route>,
    idle_route: Option<Route>,
    pub input: [f32; DEFAULT_PARTITION],
    pub left: [f32; DEFAULT_PARTITION],
    pub right: [f32; DEFAULT_PARTITION],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus_renderer;

    #[test]
    #[ignore = "offline performance measurement"]
    fn benchmark_adm_direct_objects() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let mut set = NativeHrtfSet::load_calibrated(&path).unwrap();
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        let mut sources: Vec<_> = (0..108).map(|_| DirectSource::new(&set, 0.04).unwrap()).collect();
        for moving in [false, true] {
            let mut routing = std::time::Duration::ZERO;
            let mut convolution = std::time::Duration::ZERO;
            let mut checksum = 0.0_f64;
            for block in 0..220 {
                let start = std::time::Instant::now();
                for (id, source) in sources.iter_mut().enumerate() {
                    let phase = id as f32 * 0.17 + if moving { block as f32 * 0.01 } else { 0.0 };
                    let gains = bus_renderer::route(&solver, [phase.cos() * 0.7, phase.sin() * 0.7, 0.4], None, 0.3);
                    source.schedule_focus(solver.layout(), 0.04, gains, [0.0; vbap::MAX_BUS_COUNT]);
                    source.input = std::array::from_fn(|i| ((block * DEFAULT_PARTITION + i + id) as f32 * 0.13).sin() * 0.01);
                }
                if block >= 20 { routing += start.elapsed(); }
                let start = std::time::Instant::now();
                finish_sources(sources.iter_mut(), &mut set, &solver, 0.04).unwrap();
                if block >= 20 { convolution += start.elapsed(); }
                for source in &sources { checksum += source.left.iter().chain(&source.right).map(|v| *v as f64).sum::<f64>(); }
            }
            eprintln!("108 objects moving={moving} schedule_us={:.1} filters_and_convolution_us={:.1} checksum={checksum:.9}",
                routing.as_secs_f64() * 1e6 / 200.0, convolution.as_secs_f64() * 1e6 / 200.0);
        }
    }

    #[test]
    fn batched_motion_focus_and_silent_tails_match_serial_sources() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let mut set = NativeHrtfSet::load_calibrated(&path).unwrap();
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        // Exercise both sides of the parallel threshold with the same inputs.
        for count in [3, 24] {
            let mut actual: Vec<_> = (0..count).map(|_| DirectSource::new(&set, 0.04).unwrap()).collect();
            let mut expected: Vec<_> = (0..count).map(|_| DirectSource::new(&set, 0.04).unwrap()).collect();
            for block in 0..130 {
                for (id, (a, b)) in actual.iter_mut().zip(&mut expected).enumerate() {
                    let phase = (block + id) as f32 * 0.03;
                    let gains = bus_renderer::route(&solver, [phase.cos() * 0.7, phase.sin() * 0.7, 0.4], None, 0.3);
                    let amounts = std::array::from_fn(|bus| if bus == id % solver.bus_count() { 0.0 } else { (block as f32 / 30.0).min(1.0) });
                    a.schedule_focus(solver.layout(), 0.04, gains, amounts);
                    b.update_focus(&mut set, &solver, 0.04, gains, amounts).unwrap();
                    let input = std::array::from_fn(|i| if block < 20 || block == 110 { ((block * DEFAULT_PARTITION + i + id) as f32 * 0.17).sin() * 0.01 } else { 0.0 });
                    a.input = input; b.input = input;
                    b.finish_block();
                }
                finish_sources(actual.iter_mut(), &mut set, &solver, 0.04).unwrap();
                for (a, b) in actual.iter().zip(&expected) {
                    assert_eq!(a.left, b.left, "left count={count} block={block}");
                    assert_eq!(a.right, b.right, "right count={count} block={block}");
                }
            }
        }
    }

    #[test]
    fn focus_single_convolver_matches_split_paths_during_motion_and_toggle() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let mut set = NativeHrtfSet::load_calibrated(&path).unwrap();
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        let mut actual = DirectSource::new(&set, 0.04).unwrap();
        let mut foreground = DirectSource::new(&set, 0.04).unwrap();
        let mut background = DirectSource::new(&set, 0.04).unwrap();
        let mut lowpass = crate::focus::BackgroundFilter::default();
        let mut max_error = 0.0_f32;
        for block in 0..160 {
            let gains = bus_renderer::route(&solver, [0.6, (block as f32 * 0.04).sin() * 0.7, 0.6], None, 0.4);
            let amounts = std::array::from_fn(|i| if i == 7 { 0.0 } else if (20..120).contains(&block) { 1.0 } else { 0.0 });
            actual.update_focus(&mut set, &solver, 0.04, gains, amounts).unwrap();
            foreground.update(&mut set, &solver, 0.04, std::array::from_fn(|i| gains[i] * (1.0 - amounts[i]))).unwrap();
            background.update(&mut set, &solver, 0.04, std::array::from_fn(|i| gains[i] * amounts[i])).unwrap();
            for i in 0..DEFAULT_PARTITION {
                let sample = if block < 130 { ((block * DEFAULT_PARTITION + i) as f32 * 0.19).sin() * 0.1 } else { 0.0 };
                actual.input[i] = sample;
                foreground.input[i] = sample;
                background.input[i] = lowpass.process(sample);
            }
            actual.finish_block(); foreground.finish_block(); background.finish_block();
            for i in 0..DEFAULT_PARTITION {
                max_error = max_error.max((actual.left[i] - foreground.left[i] - background.left[i]).abs());
                max_error = max_error.max((actual.right[i] - foreground.right[i] - background.right[i]).abs());
            }
        }
        assert!(max_error < 2e-6, "focus split-path error {max_error}");
    }

    #[test]
    fn cinema_measured_filters_match_independent_and_bus_paths_with_calibration() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/public/hrtf/hrtf-set.json");
        let mut set = NativeHrtfSet::load_calibrated(&path).unwrap();
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Stereo2_0);
        let speakers = vbap::speakers(solver.layout()).iter().enumerate().map(|(index,s)| {
            let mut left=vec![0.0;16384];let mut right=left.clone();
            left[128+index*4]=0.5;right[135+index*4]=0.3;
            let mut room_left=left.clone();let mut room_right=right.clone();
            room_left[12000]=0.1;room_right[12007]=0.05;
            crate::cinema::RoomSpeaker {name:s.name.into(),azimuth:s.azimuth,elevation:s.elevation,onset_sample:128,
                direct_left:left,direct_right:right,room_left,room_right}
        }).collect();
        let profile=crate::cinema::RoomProfile {version:1,name:"Synthetic regression fixture".into(),source:"Test".into(),license:"Test".into(),
            measurement:"dummy-head".into(),sample_rate:48000,layout:"2.0".into(),speakers,simulation:None};
        profile.validate().unwrap();
        let mut settings=crate::cinema::Settings {enabled:true,late_db:-3.0,..Default::default()};
        settings.speakers.insert("FrontLeft".into(),crate::cinema::SpeakerCalibration {delay_ms:2.0,gain_db:-3.0,..Default::default()});
          settings.speakers.insert("FrontRight".into(),crate::cinema::SpeakerCalibration {delay_ms:7.0,high_db:-2.0,..Default::default()});
          settings.monitor.enabled = true;
          settings.monitor.outputs.insert("FrontLeft".into(),crate::monitor::Output {trim_db:-4.0,delay_ms:3.0,invert:true,muted:false});
        set.configure_cinema(settings,Some(std::sync::Arc::new(profile)));
        let mut bus=bus_renderer::BusRenderer::new(&set,&solver,0.04).unwrap();
        let mut direct=DirectSource::new(&set,0.04).unwrap();
        let mut gains=[0.0;vbap::MAX_BUS_COUNT];gains[0]=0.6;gains[1]=0.8;
        direct.update(&mut set,&solver,0.04,gains).unwrap();
        let mut tail=false;
        for block in 0..150 {
            bus.begin_block();
            for i in 0..DEFAULT_PARTITION {
                let sample=if block==0&&i==0 {0.1}else{0.0};
                bus.add(sample,&gains,i);direct.input[i]=sample;
            }
            bus.finish_block().unwrap();direct.finish_block();
            for i in 0..DEFAULT_PARTITION {
                let expected=bus.output_at(i);
                assert!((expected[0]-direct.left[i]).abs()<1e-6);
                assert!((expected[1]-direct.right[i]).abs()<1e-6);
                if block*DEFAULT_PARTITION+i>11520 && direct.left[i].abs()>1e-4 {tail=true;}
            }
        }
        assert!(tail,"imported late tail was truncated");
        let fallback=set.mixed_speaker("FrontLeft","7.1.4",30.0,0.0,0.04).unwrap();
        assert_ne!(fallback.0[128],0.5,"mismatched room must not replace another layout");
    }

    #[test]
    fn independent_object_matches_speaker_sum_and_changes_with_layout() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let mut set = NativeHrtfSet::load_calibrated(&path).unwrap();
        let mut direct = DirectSource::new(&set, 0.04).unwrap();
        let mut outputs = Vec::new();
        for layout in [vbap::LayoutId::Dolby5_1_2, vbap::LayoutId::Dolby9_1_6] {
            let solver = vbap::VbapSolver::with_layout(layout);
            let position = [0.7, -0.4, 0.8];
            let gains = bus_renderer::route(&solver, position, None, 0.3);
            let mut buses = bus_renderer::BusRenderer::new(&set, &solver, 0.04).unwrap();
            direct.update(&mut set, &solver, 0.04, gains).unwrap();
            // Drain history and finish any layout-transition crossfade.
            for _ in 0..80 { direct.finish_block(); }
            let mut output = Vec::new();
            for block in 0..32 {
                buses.begin_block();
                for sample in 0..DEFAULT_PARTITION {
                    let x = if block == 0 && sample == 0 { 0.1 } else { 0.0 };
                    direct.input[sample] = x;
                    buses.add(x, &gains, sample);
                }
                direct.finish_block();
                buses.finish_block().unwrap();
                for sample in 0..DEFAULT_PARTITION {
                    let expected = buses.output_at(sample);
                    let actual = [direct.left[sample], direct.right[sample]];
                    for ear in 0..2 {
                        assert!((actual[ear] - expected[ear]).abs() < 1e-6,
                            "layout {layout:?} sample {sample} ear {ear}");
                    }
                    output.extend(actual);
                }
            }
            outputs.push(output);
        }
        let difference: f32 = outputs[0].iter().zip(&outputs[1]).map(|(a,b)| (a-b).abs()).sum();
        assert!(difference > 1e-4, "layout selection must change independent-object audio");
    }
}

impl DirectSource {
    pub fn new(set: &NativeHrtfSet, wet: f32) -> Result<Self, String> {
        let (_, _, mut left, mut right) = set.mixed_nearest(0.0, 0.0, wet)?;
        left.resize(set.speaker_filter_len() + DEFAULT_PARTITION, 0.0);
        right.resize(set.speaker_filter_len() + DEFAULT_PARTITION, 0.0);
        Ok(Self {
            convolver: StereoPartitionedConvolver::new(&left, &right, DEFAULT_PARTITION)?,
            route: None,
            pending_route: None,
            idle_route: None,
            input: [0.0; DEFAULT_PARTITION], left: [0.0; DEFAULT_PARTITION], right: [0.0; DEFAULT_PARTITION],
        })
    }

    pub fn update(&mut self, set: &mut NativeHrtfSet, solver: &vbap::VbapSolver, wet: f32, gains: [f32; vbap::MAX_BUS_COUNT]) -> Result<(), String> {
        self.update_focus(set, solver, wet, gains, [0.0; vbap::MAX_BUS_COUNT])
    }

    pub fn update_focus(&mut self, set: &mut NativeHrtfSet, solver: &vbap::VbapSolver, wet: f32,
        gains: [f32; vbap::MAX_BUS_COUNT], amounts: [f32; vbap::MAX_BUS_COUNT]) -> Result<(), String> {
        let route = (solver.layout(), gains, amounts, wet.to_bits());
        if self.route == Some(route) { return Ok(()); }
        let bank = prepare_bank(set, solver, wet)?;
        self.update_from_bank(&bank, route);
        Ok(())
    }

    pub fn schedule_focus(&mut self, layout: vbap::LayoutId, wet: f32,
        gains: [f32; vbap::MAX_BUS_COUNT], amounts: [f32; vbap::MAX_BUS_COUNT]) {
        let route = (layout, gains, amounts, wet.to_bits());
        self.pending_route = (self.idle_route.or(self.route) != Some(route)).then_some(route);
    }

    fn needs_processing(&self) -> bool {
        !self.convolver.tail_is_silent() || self.input.iter().any(|sample| *sample != 0.0)
    }

    fn update_from_bank(&mut self, bank: &FilterBank, route: Route) {
        let (_, gains, amounts, _) = route;
        let mut combined = self.convolver.take_spare_filter();
        if let Some(filter) = &mut combined { filter.clear(); }
        // Sum speaker filters with the actual VBAP amplitudes, preserving the
        // room layout while retaining this object's own convolution history.
        for (bus, &gain) in gains.iter().take(bank.len()).enumerate() {
            if gain <= 0.0 { continue; }
            let amount = amounts[bus];
            for (background, weight) in [(false, gain * (1.0 - amount)), (true, gain * amount)] {
                if weight == 0.0 { continue; }
                let filter = &bank[bus][usize::from(background)];
                if let Some(current) = &mut combined {
                    current.add_scaled(filter, weight);
                } else {
                    let mut first = (**filter).clone();
                    first.scale(weight);
                    combined = Some(first);
                }
            }
        }
        let filter = match combined {
            Some(filter) => filter,
            None => {
                let mut silent = (*bank[0][0]).clone();
                silent.clear();
                silent
            }
        };
        if self.route.is_none() {
            self.convolver.set_prepared_filter(filter);
        } else {
            // The engine already interpolates codec-timed routes. Only bridge
            // adjacent render blocks here; do not impose another 32 ms motion.
            self.convolver.transition_to(filter, DEFAULT_PARTITION);
        }
        self.route = Some(route);
    }

    pub fn finish_block(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.convolver.process_block(&self.input, &mut self.left, &mut self.right).expect("fixed block dimensions");
        self.input.fill(0.0);
    }
}
