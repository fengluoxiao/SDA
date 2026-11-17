//! Prepared off the render worker; apply without clearing source histories or FIFO.
use crate::{Engine, hrtf::NativeHrtfSet, vbap, bus_renderer::BusRenderer, direct_renderer};
pub struct PreparedCueUpdate {
    set: NativeHrtfSet,
    bus: BusRenderer,
    layout: vbap::LayoutId,
    wet: f32,
    reference_update: bool,
}
impl PreparedCueUpdate {
    pub fn reference_monitor_id(&self) -> Option<&'static str> {
        self.reference_update.then(|| if self.set.room_profile.as_deref().is_some_and(crate::reference_room::is_profile) { crate::reference_room::ID } else { "" })
    }
    /// Prepare the complete optional listening room before the render-worker
    /// handoff. Both enabling and returning to standard use one transaction.
    pub fn load_reference(path: &str, layout: &str, id: &str, gain: f32) -> Result<Self, String> {
        if !id.is_empty() && id != crate::reference_room::ID { return Err("Unknown reference monitor".into()); }
        let layout = vbap::LayoutId::parse(layout).ok_or("unknown layout")?;
        let mut set = NativeHrtfSet::load_calibrated(std::path::Path::new(path))?;
        if !id.is_empty() && !set.uses_shared_measured_response() { return Err("Reference monitor requires the shared HRTF profile".into()); }
        set.set_spatial_cue_gain(gain)?;
        let profile = if id.is_empty() { None } else {
            Some(crate::reference_room::profile(layout, gain / 0.5011872)?)
        };
        let settings = crate::cinema::Settings { enabled: profile.is_some(), ..Default::default() };
        set.configure_cinema(settings, profile);
        set.reserve_speaker_taps(crate::reference_room::FILTER_CAPACITY);
        set.configure_spatial_cues(layout)?;
        let wet = if id.is_empty() { set.effective_wet(0.0) } else { 0.04 };
        let solver = vbap::VbapSolver::with_layout(layout);
        let bus = BusRenderer::new(&set, &solver, wet)?;
        direct_renderer::warm_banks(&mut set, &solver, wet)?;
        Ok(Self { set, bus, layout, wet, reference_update: true })
    }
    pub fn load(path: &str, layout: &str, gain: f32) -> Result<Self, String> {
        let layout = vbap::LayoutId::parse(layout).ok_or("unknown layout")?;
        let mut set = NativeHrtfSet::load_calibrated(std::path::Path::new(path))?;
        set.set_spatial_cue_gain(gain)?;
        if set.uses_shared_measured_response() { set.reserve_speaker_taps(crate::reference_room::FILTER_CAPACITY); }
        set.configure_spatial_cues(layout)?;
        let wet = set.effective_wet(0.0);
        let solver = vbap::VbapSolver::with_layout(layout);
        let bus = BusRenderer::new(&set, &solver, wet)?;
        direct_renderer::warm_banks(&mut set, &solver, wet)?;
        Ok(Self { set, bus, layout, wet, reference_update: false })
    }
}
impl Engine {
    pub fn apply_spatial_cue_update(&mut self, mut update: PreparedCueUpdate) -> Result<(), String> {
        if self.layout != update.layout { return Err("spatial cue layout changed; retry".into()); }
        let old = self.active_hrtf_set.as_ref().ok_or("HRTF not loaded")?;
        if old.simulation_shape() != update.set.simulation_shape() || (!update.reference_update && !update.set.uses_shared_measured_response() && old.speaker_filter_len() != update.set.speaker_filter_len()) { return Err("spatial cue asset shape changed".into()); }
        // No graph replacement, transport operation, PCM flush, or clock reset.
        for source in self.sources.values_mut() {
            if let Some(direct) = source.direct.as_mut() {
                direct.refresh_cue_filters(&mut update.set, &self.vbap, update.wet)?;
            }
        }
        if let Some(bus) = self.bus_renderer.as_mut() { bus.transition_filters_from(&update.bus); }
        if update.reference_update {
            self.cinema = update.set.cinema.clone();
            self.room_profile = update.set.room_profile.clone();
            self.near_field.enabled = false;
            self.bed_compatibility = false;
            self.direct_objects = true;
            self.set_directional_hrtf(true);
        }
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
    fn shared_path() -> String { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json").to_str().unwrap().into() }
    #[test]
    fn reference_roundtrip_keeps_pcm_clock_pose_and_direct_object_filters() {
        for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13, vbap::LayoutId::parse("2.0").unwrap()] {
            let mut initial=PreparedCueUpdate::load(&shared_path(),layout.as_str(),0.5011872).unwrap();
            let mut e=Engine::new(48000,2); e.layout=layout;e.vbap=vbap::VbapSolver::with_layout(layout);
            let direction=crate::directional::Direction { diffuse:0., horizontal_only:false, position:[0.4,0.6,0.7], head:None, width:0.,height:0.,depth:0. };
            let gains=crate::bus_renderer::route(&e.vbap,direction.position,None,0.);
            let amounts=[0.;vbap::MAX_BUS_COUNT];
            let dry=initial.set.directional_dry_compact(direction,layout,gains,amounts).unwrap();
            e.active_hrtf_set=Some(initial.set);e.bus_renderer=Some(initial.bus);
            e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);
            e.head_pose=Some([0.,0.258819,0.,0.9659258]);e.sample_pos=23456;e.render_epoch=81;
            e.output_gain=0.73;e.paused=false;
            let mut source=Source::default();source.samples.write(23456,23456,&[0.02,-0.03,0.04]);
            e.sources.insert("obj:1".into(),source);
            for id in [crate::reference_room::ID,"",crate::reference_room::ID,""] {
                let prepared=PreparedCueUpdate::load_reference(&shared_path(),layout.as_str(),id,0.5011872).unwrap();
                assert_eq!(prepared.reference_monitor_id(),Some(id));
                assert_eq!(dry,prepared.set.directional_dry_compact(direction,layout,gains,amounts).unwrap());
                e.apply_spatial_cue_update(prepared).unwrap();
                assert_eq!(e.sample_pos,23456);assert_eq!(e.render_epoch,81);assert!(!e.paused);
                assert_eq!(e.output_gain,0.73);assert_eq!(e.head_pose,Some([0.,0.258819,0.,0.9659258]));
                assert!(e.sources["obj:1"].samples.has_at(23456));assert!(e.direct_objects&&e.directional_hrtf);
                assert_eq!(e.room_profile.is_some(),!id.is_empty());assert_eq!(e.cinema.enabled,!id.is_empty());
            }
            assert!(PreparedCueUpdate::load_reference(&shared_path(),layout.as_str(),"not-a-room",0.5011872).is_err());
            let other=if layout==vbap::LayoutId::Dolby7_1_4 {"360RA-13"}else{"7.1.4"};
            let stale=PreparedCueUpdate::load_reference(&shared_path(),other,crate::reference_room::ID,0.5011872).unwrap();
            assert!(e.apply_spatial_cue_update(stale).is_err());assert!(e.room_profile.is_none());
            assert_eq!(e.sample_pos,23456);assert!(e.sources["obj:1"].samples.has_at(23456));
            e.bed_compatibility=true;e.direct_objects=false;e.set_directional_hrtf(false);
            e.apply_spatial_cue_update(PreparedCueUpdate::load_reference(&shared_path(),layout.as_str(),crate::reference_room::ID,0.5011872).unwrap()).unwrap();
            assert!(!e.bed_compatibility&&e.direct_objects&&e.directional_hrtf);
            for next in [vbap::LayoutId::parse(other).unwrap(),layout] {
                e.set_layout(next).unwrap();
                let profile=e.room_profile.as_ref().unwrap();assert_eq!(profile.layout,next.as_str());
                assert_eq!(profile.speakers.len(),vbap::speakers(next).len());
                assert_eq!(e.sample_pos,23456);assert_eq!(e.render_epoch,81);
                assert!(e.sources["obj:1"].samples.has_at(23456));
            }
        }
    }
    #[test]
    fn reference_bed_and_object_residual_filters_are_nonzero_and_stereo_calibration_survives() {
        let standard=PreparedCueUpdate::load_reference(&shared_path(),"7.1.4","",0.5011872).unwrap();
        let reference=PreparedCueUpdate::load_reference(&shared_path(),"7.1.4",crate::reference_room::ID,0.5011872).unwrap();
        let solver=vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        assert!(crate::stereo_bass::prepare(&reference.set,&solver,reference.wet).unwrap().is_some());
        for sp in vbap::speakers(solver.layout()) {
            let (dl,dr)=reference.set.mixed_speaker(sp.name,"7.1.4",sp.azimuth as f64,sp.elevation as f64,0.).unwrap();
            let (rl,rr)=reference.set.mixed_speaker(sp.name,"7.1.4",sp.azimuth as f64,sp.elevation as f64,reference.wet).unwrap();
            let (sl,sr)=standard.set.mixed_speaker(sp.name,"7.1.4",sp.azimuth as f64,sp.elevation as f64,0.).unwrap();
            assert_eq!(dl,sl);assert_eq!(dr,sr);
            let difference=rl.iter().chain(&rr).zip(dl.iter().chain(&dr)).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);
            assert!(difference>1e-7,"no reference contribution for {}",sp.name);
        }
    }
    #[test]
    fn reference_roundtrip_accepts_the_shared_seventeen_direction_speaker_bank() {
        let bank=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/hrtf/apple-hrtfhq-v1/hrtf/hrtf-set.json");
        let initial=PreparedCueUpdate::load_reference(bank.to_str().unwrap(),"7.1.4","",0.5011872).unwrap();
        assert_eq!(initial.set.simulation_shape().0,17);
        let mut engine=Engine::new(48000,2);engine.active_hrtf_set=Some(initial.set);engine.bus_renderer=Some(initial.bus);
        engine.sample_pos=9876;engine.render_epoch=42;
        for id in [crate::reference_room::ID,""] {
            engine.apply_spatial_cue_update(PreparedCueUpdate::load_reference(bank.to_str().unwrap(),"7.1.4",id,0.5011872).unwrap()).unwrap();
            assert_eq!(engine.active_hrtf_set.as_ref().unwrap().simulation_shape().0,17);
            assert_eq!(engine.sample_pos,9876);assert_eq!(engine.render_epoch,42);
            assert_eq!(engine.room_profile.is_some(),!id.is_empty());
        }
    }
    #[test]
    fn live_cues_reject_invalid_gain() {
        for gain in [0.0,-1.0,2.0,f32::NAN,f32::INFINITY] { assert!(PreparedCueUpdate::load(&path(),"7.1.4",gain).is_err()); }
    }
}
