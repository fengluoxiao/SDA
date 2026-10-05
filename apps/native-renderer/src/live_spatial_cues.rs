//! Prepared off the render worker; apply without clearing source histories or FIFO.
use crate::{Engine, hrtf::NativeHrtfSet, vbap, bus_renderer::BusRenderer, direct_renderer};
pub struct PreparedCueUpdate {
    set: NativeHrtfSet,
    bus: BusRenderer,
    layout: vbap::LayoutId,
    wet: f32,
}
impl PreparedCueUpdate {
    pub fn load(path: &str, layout: &str, gain: f32) -> Result<Self, String> {
        let layout = vbap::LayoutId::parse(layout).ok_or("unknown layout")?;
        let mut set = NativeHrtfSet::load_calibrated(std::path::Path::new(path))?;
        set.set_spatial_cue_gain(gain)?;
        set.configure_spatial_cues(layout)?;
        let wet = set.effective_wet(0.0);
        let solver = vbap::VbapSolver::with_layout(layout);
        let bus = BusRenderer::new(&set, &solver, wet)?;
        direct_renderer::warm_banks(&mut set, &solver, wet)?;
        Ok(Self { set, bus, layout, wet })
    }
}
impl Engine {
    pub fn apply_spatial_cue_update(&mut self, mut update: PreparedCueUpdate) -> Result<(), String> {
        if self.layout != update.layout { return Err("spatial cue layout changed; retry".into()); }
        let old = self.active_hrtf_set.as_ref().ok_or("HRTF not loaded")?;
        if old.simulation_shape() != update.set.simulation_shape() || old.speaker_filter_len() != update.set.speaker_filter_len() { return Err("spatial cue asset shape changed".into()); }
        // No graph replacement, transport operation, PCM flush, or clock reset.
        for source in self.sources.values_mut() {
            if let Some(direct) = source.direct.as_mut() {
                direct.refresh_cue_filters(&mut update.set, &self.vbap, update.wet)?;
            }
        }
        if let Some(bus) = self.bus_renderer.as_mut() { bus.transition_filters_from(&update.bus); }
        self.active_hrtf_set = Some(update.set);
        self.hrtf_wet_weight = update.wet;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Source, convolution::DEFAULT_PARTITION as N};
    fn path() -> String { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json").to_str().unwrap().into() }
    #[test]
    fn live_cues_preserve_transport_sources_and_convolution_history() {
        for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
            let path = path();
            let mut initial = PreparedCueUpdate::load(&path, layout.as_str(), 0.5011872).unwrap();
            let mut target = PreparedCueUpdate::load(&path, layout.as_str(), 1.0).unwrap();
            let solver = vbap::VbapSolver::with_layout(layout);
            let gains = crate::bus_renderer::route(&solver, [0.6, 0.0, -0.8], None, 0.0);
            let mut direct = direct_renderer::DirectSource::new(&initial.set, initial.wet).unwrap();
            direct.update(&mut initial.set, &solver, initial.wet, gains).unwrap();
            let mut reference = direct_renderer::DirectSource::new(&target.set, target.wet).unwrap();
            reference.update(&mut target.set, &solver, target.wet, gains).unwrap();
            // Both convolvers receive identical history, with different cue filters.
            for block in 0..20 {
                for i in 0..N { let v = ((block*N+i) as f32 * 0.037).sin()*0.01; direct.input[i]=v; reference.input[i]=v; }
                direct.finish_block(); reference.finish_block();
            }
            direct.refresh_cue_filters(&mut target.set, &solver, target.wet).unwrap();
            for block in 20..24 {
                for i in 0..N { let v=((block*N+i) as f32 * 0.037).sin()*0.01; direct.input[i]=v; reference.input[i]=v; }
                direct.finish_block(); reference.finish_block();
                if block >= 22 { for (a,b) in direct.left.iter().chain(&direct.right).zip(reference.left.iter().chain(&reference.right)) { assert!((a-b).abs()<1e-6, "history lost: {a} {b}"); } }
            }
            // Beds and continuous-object residuals also keep their convolution tails.
            for block in 0..24 {
                if block == 20 { initial.bus.transition_filters_from(&target.bus); }
                initial.bus.begin_block(); target.bus.begin_block();
                for i in 0..N {
                    let v = ((block*N+i) as f32 * 0.043).sin()*0.01;
                    initial.bus.add(v, &gains, i); target.bus.add(v, &gains, i);
                    initial.bus.add_reflections(v, &gains, i); target.bus.add_reflections(v, &gains, i);
                }
                initial.bus.finish_block().unwrap(); target.bus.finish_block().unwrap();
                if block >= 22 { for i in 0..N {
                    let a=initial.bus.output_at(i); let b=target.bus.output_at(i);
                    assert!((a[0]-b[0]).abs()<1e-6 && (a[1]-b[1]).abs()<1e-6, "bus history lost");
                } }
            }
            let mut engine = Engine::new(48000, 2);
            engine.layout = layout; engine.vbap = solver;
            engine.active_hrtf_set = Some(initial.set); engine.bus_renderer = Some(initial.bus);
            engine.sample_pos=12345; engine.render_epoch=77; engine.paused=true; engine.output_gain=0.72;
            engine.sources.insert("obj".into(), Source::default());
            let source=engine.sources.get_mut("obj").unwrap();
            source.direct=Some(Box::new(direct)); source.samples.write(12345,12345,&[0.3,-0.2]);
            for (index, db) in [0.0f32,-3.0,-6.0,-9.0,-12.0,-6.0].into_iter().enumerate() {
                engine.paused = index % 2 == 0;
                engine.apply_spatial_cue_update(PreparedCueUpdate::load(&path,layout.as_str(),10.0f32.powf(db/20.0)).unwrap()).unwrap();
                assert_eq!(engine.sample_pos,12345); assert_eq!(engine.render_epoch,77); assert_eq!(engine.paused,index % 2 == 0);
                assert_eq!(engine.output_gain,0.72); assert!(engine.sources["obj"].direct.is_some());
                assert!(engine.sources["obj"].samples.has_at(12345));
            }
        }
    }
    #[test]
    fn live_cues_reject_invalid_gain() {
        for gain in [0.0,-1.0,2.0,f32::NAN,f32::INFINITY] { assert!(PreparedCueUpdate::load(&path(),"7.1.4",gain).is_err()); }
    }
}
