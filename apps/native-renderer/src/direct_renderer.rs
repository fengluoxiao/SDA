//! Per-object convolution; shares measured assets with the speaker renderer.
use crate::{convolution::{StereoPartitionedConvolver, DEFAULT_PARTITION}, hrtf::NativeHrtfSet, vbap};

pub(super) struct DirectSource {
    convolver: StereoPartitionedConvolver,
    route: Option<(vbap::LayoutId, [f32; vbap::MAX_BUS_COUNT], [f32; vbap::MAX_BUS_COUNT], u32)>,
    pub input: [f32; DEFAULT_PARTITION],
    pub left: [f32; DEFAULT_PARTITION],
    pub right: [f32; DEFAULT_PARTITION],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus_renderer;

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
                if block>90 && direct.left[i].abs()>1e-4 {tail=true;}
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
        let mut combined = None;
        let mut total = 0.0;
        // Sum speaker filters with the actual VBAP amplitudes, preserving the
        // room layout while retaining this object's own convolution history.
        for (bus, &gain) in gains.iter().take(solver.bus_count()).enumerate() {
            if gain <= 0.0 { continue; }
            let (azimuth, elevation) = solver.speaker_direction(bus);
            let name = vbap::speakers(solver.layout())[bus].name;
            let amount = amounts[bus];
            let mut filter = set.prepared_focus_speaker(name,
                solver.layout().as_str(), azimuth as f64, elevation as f64, wet, amount == 1.0)?;
            if amount > 0.0 && amount < 1.0 {
                let background = set.prepared_focus_speaker(name,
                    solver.layout().as_str(), azimuth as f64, elevation as f64, wet, true)?;
                crate::convolution::PreparedStereoFilter::blend(&mut filter, &background, amount);
            }
            total += gain;
            if let Some(current) = &mut combined {
                crate::convolution::PreparedStereoFilter::blend(current, &filter, gain / total);
            } else {
                combined = Some(filter);
            }
        }
        let mut filter = match combined {
            Some(filter) => filter,
            None => {
                let (azimuth, elevation) = solver.speaker_direction(0);
                set.prepared_focus_speaker(vbap::speakers(solver.layout())[0].name,
                    solver.layout().as_str(), azimuth as f64, elevation as f64, wet, false)?
            }
        };
        filter.scale(total);
        if self.route.is_none() {
            self.convolver.set_prepared_filter(filter);
        } else {
            // The engine already interpolates codec-timed routes. Only bridge
            // adjacent render blocks here; do not impose another 32 ms motion.
            self.convolver.transition_to(filter, DEFAULT_PARTITION);
        }
        self.route = Some(route);
        Ok(())
    }

    pub fn finish_block(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.convolver.process_block(&self.input, &mut self.left, &mut self.right).expect("fixed block dimensions");
        self.input.fill(0.0);
    }
}
