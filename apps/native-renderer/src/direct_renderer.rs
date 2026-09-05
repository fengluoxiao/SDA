//! Per-object convolution; shares measured assets with the speaker renderer.
use crate::{convolution::{StereoPartitionedConvolver, DEFAULT_PARTITION}, hrtf::NativeHrtfSet, vbap};

pub(super) struct DirectSource {
    background: Option<Box<DirectSource>>,
    background_filter: crate::focus::BackgroundFilter,
    convolver: StereoPartitionedConvolver,
    route: Option<(vbap::LayoutId, [f32; vbap::MAX_BUS_COUNT], u32)>,
    pub input: [f32; DEFAULT_PARTITION],
    pub left: [f32; DEFAULT_PARTITION],
    pub right: [f32; DEFAULT_PARTITION],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus_renderer;

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
        let (_, _, left, right) = set.mixed_nearest(0.0, 0.0, wet)?;
        Ok(Self {
            background: None,
            background_filter: crate::focus::BackgroundFilter::default(),
            convolver: StereoPartitionedConvolver::new(&left, &right, DEFAULT_PARTITION)?,
            route: None,
            input: [0.0; DEFAULT_PARTITION], left: [0.0; DEFAULT_PARTITION], right: [0.0; DEFAULT_PARTITION],
        })
    }

    pub fn update(&mut self, set: &mut NativeHrtfSet, solver: &vbap::VbapSolver, wet: f32, gains: [f32; vbap::MAX_BUS_COUNT]) -> Result<(), String> {
        let route = (solver.layout(), gains, wet.to_bits());
        if self.route == Some(route) { return Ok(()); }
        let mut combined = None;
        let mut total = 0.0;
        // Sum speaker filters with the actual VBAP amplitudes, preserving the
        // room layout while retaining this object's own convolution history.
        for (bus, &gain) in gains.iter().take(solver.bus_count()).enumerate() {
            if gain <= 0.0 { continue; }
            let (azimuth, elevation) = solver.speaker_direction(bus);
            let filter = set.prepared_direction(azimuth as f64, elevation as f64, wet)?;
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
                set.prepared_direction(azimuth as f64, elevation as f64, wet)?
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

    pub fn update_focus(&mut self, set: &mut NativeHrtfSet, solver: &vbap::VbapSolver, wet: f32,
        gains: [f32; vbap::MAX_BUS_COUNT], amounts: [f32; vbap::MAX_BUS_COUNT]) -> Result<(), String> {
        if self.background.is_none() && amounts.iter().any(|value| *value > 0.0) {
            self.background = Some(Box::new(Self::new(set, wet)?));
        }
        if let Some(background) = &mut self.background {
            background.update(set, solver, wet, std::array::from_fn(|i| gains[i] * amounts[i]))?;
        }
        self.update(set, solver, wet, std::array::from_fn(|i| gains[i] * (1.0 - amounts[i])))
    }

    pub fn finish_block(&mut self) {
        for (i, sample) in self.input.iter().enumerate() {
            let filtered = self.background_filter.process(*sample);
            if let Some(background) = &mut self.background { background.input[i] = filtered; }
        }
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.convolver.process_block(&self.input, &mut self.left, &mut self.right).expect("fixed block dimensions");
        if let Some(background) = &mut self.background {
            background.finish_block();
            for i in 0..DEFAULT_PARTITION {
                self.left[i] += background.left[i];
                self.right[i] += background.right[i];
            }
        }
        self.input.fill(0.0);
    }
}
