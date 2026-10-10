//! Optional spatial-layer lift. This is an explicit remix, NOT vocal detection
//! or a claim that authored levels / measured HRTFs were wrong. Default is off.
use crate::{Source,SourceKind,vbap};
use std::collections::HashMap;
const MAX_DB:f32=6.0;
fn smooth(v:f32)->f32 {let t=v.clamp(0.0,1.0);t*t*(3.0-2.0*t)}
/// World coordinates: no PCM/loudness, object IDs or head pose enters this rule.
/// Ear-level front and sides through ±105deg belong to the protected main layer.
/// Rear 105..135deg and ±30deg height blend into the auxiliary layer. These are enhancement
/// design choices, not a normative Dolby/Apple coordinate mapping.
fn weight(position:[f32;3])->f32 {
    if !position.iter().all(|v|v.is_finite()) {return 0.0;}
    let [x,y,z]=position.map(f64::from);let h=(x*x+y*y).sqrt();let r=(h*h+z*z).sqrt();
    if r<1e-6 {return 0.0;}
    let front=if h>1e-6 {(y/h) as f32} else {1.0};
    // Horizontal side sources can carry lead material. Do not blindly lift
    // the whole lateral plane: auxiliary rear starts behind the side region.
    let rear_start=0.25881904f32; // sin(15deg), i.e. azimuth 105deg.
    let side=smooth((-front-rear_start)/(std::f32::consts::FRAC_1_SQRT_2-rear_start));
    let height=smooth((z.abs()/r/0.5) as f32);
    (1.0-(1.0-side)*(1.0-height)).clamp(0.0,1.0)
}
fn speaker_weight(azimuth:f32,elevation:f32)->f32 {
    let a=azimuth.to_radians();let e=elevation.to_radians();
    weight([-a.sin()*e.cos(),a.cos()*e.cos(),e.sin()])
}
pub(crate) struct Settings {pub db:f32,pub main_restore_db:f32,pub slew:f32}
impl Settings {
    pub fn new(rate:u32)->Self {Self {db:0.0,main_restore_db:0.0,slew:(10.0f32.powf(MAX_DB/20.0)-1.0)/(rate.max(1) as f32*0.03)}}
    pub fn set(&mut self,db:f32,rate:u32)->Result<(),String> {
        if !db.is_finite() || !(0.0..=MAX_DB).contains(&db) {return Err("spatial layer gain must be finite and between 0 and +6 dB".into());}
        self.db=db;self.slew=Self::new(rate).slew;Ok(())
    }
    /// Bed routes use canonical world layout, never the rotated listener route.
    pub fn refresh_beds(&self,sources:&mut HashMap<String,Source>,solver:&vbap::VbapSolver) {
        // The accepted preset has no auxiliary lift, but still restores the
        // horizontal main layer. Its bed weights must not stay zero/stale.
        if self.db==0.0 && self.main_restore_db==0.0 {return;}
        for s in sources.values_mut().filter(|s|s.kind==SourceKind::Bed) {
            let route=crate::bed_route(s.bed_label.as_deref().unwrap_or(""),solver);
            if route.lfe>0.0 {s.spatial_layer_bed_weight=0.0;continue;}
            let mut w=0.0f32;let mut total=0.0f32;
            for (gain,speaker) in route.buses.iter().zip(vbap::speakers(solver.layout())) {
                let p=gain*gain;total+=p;w+=p*speaker_weight(speaker.azimuth,speaker.elevation);
            }
            s.spatial_layer_bed_weight=if total>1e-12 {(w/total).clamp(0.0,1.0)} else {0.0};
        }
    }
}
pub(crate) fn source_gain(s:&mut Source,db:f32,main_restore_db:f32,slew:f32,at:u64)->f32 {
    if s.lfe_gain>0.0 || s.lfe_target>0.0 {s.spatial_layer_gain=1.0;s.spatial_layer_target=1.0;return 1.0;}
    if db==0.0 && main_restore_db==0.0 && s.spatial_layer_gain==1.0 {return 1.0;}
    if at.is_multiple_of(128) {
        let w=if s.kind==SourceKind::Object {weight(s.position)} else {s.spatial_layer_bed_weight};
        s.spatial_layer_target=10.0f32.powf((db*w-main_restore_db*(1.0-w))/20.0);
    }
    s.spatial_layer_gain+=(s.spatial_layer_target-s.spatial_layer_gain).clamp(-slew,slew);
    if (s.spatial_layer_target-s.spatial_layer_gain).abs()<1e-6 {s.spatial_layer_gain=s.spatial_layer_target;}
    s.spatial_layer_gain
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_candidate_motion_is_bounded_at_common_sample_rates() {
        for rate in [44100,48000,96000] {
            let settings=Settings{db:1.5,main_restore_db:2.0,..Settings::new(rate)};
            let mut source=Source{kind:SourceKind::Object,position:[0.0,1.0,0.0],..Default::default()};
            let low=10.0f32.powf(-2.0/20.0);let high=10.0f32.powf(1.5/20.0);
            let mut previous=1.0f32;
            for at in 0..u64::from(rate)*2 {
                let a=at as f64/f64::from(rate)*std::f64::consts::TAU;
                // Traverse front/side/rear and height transitions continuously.
                source.position=[a.sin() as f32,a.cos() as f32,(a*1.3).sin() as f32];
                let gain=source_gain(&mut source,settings.db,settings.main_restore_db,settings.slew,at);
                assert!(gain.is_finite() && gain>=low-1e-6 && gain<=high+1e-6);
                assert!((gain-previous).abs()<=settings.slew+1e-6);
                previous=gain;
            }
            // A metadata jump still obeys the same per-sample bound.
            source.position=[0.0,1.0,0.0];
            for at in u64::from(rate)*2..u64::from(rate)*2+4096 {
                let gain=source_gain(&mut source,settings.db,settings.main_restore_db,settings.slew,at);
                assert!((gain-previous).abs()<=settings.slew+1e-6);previous=gain;
            }
            assert!((previous-low).abs()<1e-6);
        }
    }

    #[test]
    fn accepted_candidate_pause_keeps_source_gain_and_resumes_same_output() {
        use crate::*;
        let make=|| {
            let mut e=Engine::new(48000,2);
            let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json");
            e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),0.0).unwrap();
            e.set_spatial_enhancement(true).unwrap();
            // Match the offline +1.5 candidate, retaining main compensation.
            e.spatial_layer.db=1.5;
            e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);
            e.set_program_codec("eac3".into());e.direct_mix=1.0;e.output_active=true;e.paused=false;
            let id="pause-probe".to_string();
            let mut source=Source{kind:SourceKind::Object,position:[0.0,-1.0,0.7],gain:1.0,target_gain:1.0,availability:1.0,availability_target:1.0,..Default::default()};
            let pcm:Vec<_>=(0..16384).map(|n|(n as f32*0.137).sin()*0.001).collect();
            source.samples.write(0,0,&pcm);e.sources.insert(id.clone(),source);e.route_source_now(&id,0).unwrap();e
        };
        let mut reference=make();let mut paused=make();
        let mut a=vec![0.0;8192];let mut b=a.clone();
        reference.render_into(&mut a,2);paused.render_into(&mut b,2);
        assert!(a.iter().zip(&b).all(|(a,b)|(*a-*b).abs()<1e-7));
        let clock=paused.sample_pos;let gain=paused.sources["pause-probe"].spatial_layer_gain;
        paused.paused=true;
        let mut silence=vec![1.0;4096];paused.render_into(&mut silence,2);
        assert!(silence.iter().all(|v|*v==0.0));
        assert_eq!(paused.sample_pos,clock);assert_eq!(paused.sources["pause-probe"].spatial_layer_gain,gain);
        paused.paused=false;
        reference.render_into(&mut a,2);paused.render_into(&mut b,2);
        assert!(a.iter().any(|v|v.abs()>1e-5));
        assert!(a.iter().zip(&b).all(|(a,b)|(*a-*b).abs()<1e-7));
        assert_eq!(paused.peak_guard.diagnostic_gain(),1.0);
    }
    #[test]
    fn world_geometry_is_continuous_mirrored_and_radius_independent() {
        assert!(weight([-1.0,1.0,0.0])<1e-6);assert_eq!(weight([0.0,1.0,0.0]),0.0);
        for p in [[-1.0,0.0,0.0],[1.0,0.0,0.0],[0.0,1.0,0.0]] {assert_eq!(weight(p),0.0);}
        for p in [[0.0,-1.0,0.0],[0.0,1.0,1.0],[0.0,1.0,-1.0]] {assert_eq!(weight(p),1.0);}
        for i in -180..=180 {let a=(i as f32).to_radians();let p=[a.sin(),a.cos(),0.3];let w=weight(p);assert!((0.0..=1.0).contains(&w));assert_eq!(w,weight([-p[0],p[1],p[2]]));assert!((w-weight(p.map(|v|v*3.0))).abs()<1e-5);}
        assert_eq!(weight([0.0;3]),0.0);assert_eq!(weight([f32::NAN,0.0,0.0]),0.0);
    }
    #[test]
    fn gain_never_attenuates_and_is_not_a_weak_source_selector() {
        let settings=Settings{db:6.0,..Settings::new(48000)};
        for level in [0.00001,0.1,1.0] {
            let mut s=Source{kind:SourceKind::Object,position:[0.0,-1.0,0.0],gain:level,target_gain:level,..Default::default()};
            for at in 0..4096 {let g=source_gain(&mut s,settings.db,0.0,settings.slew,at);assert!((1.0..=10.0f32.powf(6.0/20.0)+1e-6).contains(&g));}
            assert_eq!(s.spatial_layer_gain,10.0f32.powf(6.0/20.0));assert_eq!(s.gain,level);
        }
    }
    #[test]
    fn disabled_is_exact_unity_and_live_bypass_releases() {
        let mut s=Source{kind:SourceKind::Object,position:[0.0,-1.0,0.0],..Default::default()};let p=Settings::new(48000);
        for at in 0..256 {assert_eq!(source_gain(&mut s,0.0,0.0,p.slew,at),1.0);}
        for at in 256..4352 {source_gain(&mut s,6.0,0.0,p.slew,at);}
        for at in 4352..8448 {source_gain(&mut s,0.0,0.0,p.slew,at);}
        assert_eq!(s.spatial_layer_gain,1.0);
    }
    #[test]
    fn lfe_is_excluded_and_invalid_setting_is_atomic() {
        let mut p=Settings::new(48000);p.set(6.0,48000).unwrap();
        for v in [f32::NAN,f32::INFINITY,-0.1,6.1] {assert!(p.set(v,48000).is_err());assert_eq!(p.db,6.0);}
        let mut s=Source{kind:SourceKind::Bed,lfe_gain:1.0,spatial_layer_gain:1.9,..Default::default()};
        assert_eq!(source_gain(&mut s,6.0,0.0,p.slew,0),1.0);
    }
    #[test]
    fn beds_and_objects_both_use_layout_not_song_or_channel_ordinal() {
        let solver=vbap::VbapSolver::new();let settings=Settings{db:6.0,..Settings::new(48000)};
        let mut sources=HashMap::new();
        for label in ["FrontLeft","FrontRight","TopFrontLeft","SurroundLeft","LFE"] {
            sources.insert(label.into(),Source{kind:SourceKind::Bed,bed_label:Some(label.into()),..Default::default()});
        }
        settings.refresh_beds(&mut sources,&solver);
        assert!(sources["FrontLeft"].spatial_layer_bed_weight<1e-6);assert!(sources["FrontRight"].spatial_layer_bed_weight<1e-6);
        assert!(sources["TopFrontLeft"].spatial_layer_bed_weight>0.99);assert!(sources["SurroundLeft"].spatial_layer_bed_weight<1e-6);assert_eq!(sources["LFE"].spatial_layer_bed_weight,0.0);
    }

    #[test]
    fn zero_auxiliary_lift_still_classifies_beds_for_main_restore() {
        let solver=vbap::VbapSolver::new();
        let settings=Settings{db:0.0,main_restore_db:2.0,..Settings::new(48000)};
        let mut sources=HashMap::new();
        for label in ["FrontLeft","TopFrontLeft","LFE"] {
            sources.insert(label.into(),Source{kind:SourceKind::Bed,bed_label:Some(label.into()),..Default::default()});
        }
        settings.refresh_beds(&mut sources,&solver);
        assert_eq!(sources["FrontLeft"].spatial_layer_bed_weight,0.0);
        assert!(sources["TopFrontLeft"].spatial_layer_bed_weight>0.99);
        assert_eq!(sources["LFE"].spatial_layer_bed_weight,0.0);
        let master=10.0f32.powf(2.0/20.0);
        for (label,expected) in [("FrontLeft",1.0),("TopFrontLeft",master)] {
            let source=sources.get_mut(label).unwrap();
            for at in 0..4096 {source_gain(source,settings.db,settings.main_restore_db,settings.slew,at);}
            assert!((source.spatial_layer_gain*master-expected).abs()<1e-5);
        }
        // Previously cached weights must also be replaced by current labels.
        sources.get_mut("TopFrontLeft").unwrap().bed_label=Some("FrontLeft".into());
        settings.refresh_beds(&mut sources,&solver);
        assert_eq!(sources["TopFrontLeft"].spatial_layer_bed_weight,0.0);
    }
    #[test]
    fn parallel_and_general_object_mixers_apply_the_same_layer() {
        use crate::*;
        let render=|slow:bool,moving:bool| {
            let mut e=Engine::new(48000,2);
            let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json");
            e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),0.0).unwrap();
            e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);e.set_program_codec("eac3".into());
            e.set_spatial_layer_gain_db(6.0).unwrap();e.set_master_preamp_db(2.0).unwrap();
            e.disable_fast_objects=slow;e.direct_mix=1.0;e.output_active=true;e.paused=false;
            for i in 0..10 {
                let id=format!("arbitrary-layer-name-{i}");
                let position=match i%3 {0=>[-1.0,1.0,0.0],1=>[1.0,0.0,0.0],_=>[0.0,1.0,1.0]};
                let mut source=Source{kind:SourceKind::Object,position,gain:1.0,target_gain:1.0,availability:1.0,availability_target:1.0,..Default::default()};
                let pcm:Vec<_>=(0..16384).map(|n|((n+i*5) as f32*0.173).sin()*0.0002).collect();
                source.samples.write(0,0,&pcm);e.sources.insert(id.clone(),source);e.route_source_now(&id,0).unwrap();
            }
            let mut output=vec![0.0;32768];
            for block in 0..64 {
                if moving {
                    let angle=block as f32*0.08;
                    for i in 0..10 {
                        let id=format!("arbitrary-layer-name-{i}");
                        let p=angle+i as f32*0.4;
                        e.sources.get_mut(&id).unwrap().position=[p.sin(),p.cos(),(p*0.7).sin()*0.8];
                        e.route_source_now(&id,128).unwrap();
                    }
                }
                e.render_into(&mut output[block*512..(block+1)*512],2);
                assert_eq!(e.peak_guard.diagnostic_gain(),1.0);
            }
            output
        };
        for moving in [false,true] {
            let fast=render(false,moving);let slow=render(true,moving);
            assert!(fast.iter().any(|v|v.abs()>1e-5));
            assert!(fast.iter().zip(slow).all(|(a,b)|(*a-b).abs()<1e-6),"mixer parity failed: moving={moving}");
        }
    }

    #[test]
    fn moving_correlated_objects_sum_without_extra_loss_in_accepted_preset() {
        use crate::*;
        for slow in [false,true] {
            let render=|included:Option<usize>| {
                let mut e=Engine::new(48000,2);
                let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json");
                e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),0.0).unwrap();
                e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);
                e.set_program_codec("eac3".into());e.set_spatial_enhancement(true).unwrap();
                e.disable_fast_objects=slow;e.direct_mix=1.0;e.output_active=true;e.paused=false;
                // Keep identical source declarations in every run, so mixer
                // selection/availability do not confound linear superposition.
                for i in 0..10 {
                    let id=format!("sum-probe-{i}");
                    let mut source=Source{kind:SourceKind::Object,position:[0.0,1.0,0.0],gain:1.0,target_gain:1.0,availability:1.0,availability_target:1.0,..Default::default()};
                    let samples:Vec<_>=(0..8192).map(|n| {
                        if i>=3 || included.is_some_and(|only|only!=i) {0.0}
                        else {(n as f32*0.137).sin()*[0.002,0.0001,-0.0001][i]}
                    }).collect();
                    source.samples.write(0,0,&samples);e.sources.insert(id.clone(),source);
                    e.route_source_now(&id,0).unwrap();
                }
                let mut out=vec![0.0;16384];
                for block in 0..32 {
                    for i in 0..3 {
                        let a=block as f32*0.09+i as f32*0.6;
                        let id=format!("sum-probe-{i}");
                        e.sources.get_mut(&id).unwrap().position=[a.sin(),a.cos(),(a*0.8).sin()];
                        e.route_source_now(&id,128).unwrap();
                    }
                    e.render_into(&mut out[block*512..(block+1)*512],2);
                    assert_eq!(e.peak_guard.diagnostic_gain(),1.0);
                }
                out
            };
            let full=render(None);
            let solos:Vec<_>=(0..3).map(|i|render(Some(i))).collect();
            let error=full.iter().enumerate().map(|(n,v)|(*v-solos.iter().map(|s|s[n]).sum::<f32>()).abs()).fold(0.0f32,f32::max);
            assert!(error<1e-7,"unexpected nonlinear loss: slow={slow}, error={error}");
            for solo in &solos {assert!(solo.iter().map(|v|f64::from(*v).powi(2)).sum::<f64>()>1e-6);}
        }
    }

    #[test]
    fn accepted_preset_toggles_both_controls_without_touching_authored_gains() {
        let mut e=crate::Engine::new(48000,2);
        assert_eq!(e.master_preamp_db(),0.0);assert_eq!(e.spatial_layer_gain_db(),0.0);
        e.set_spatial_enhancement(true).unwrap();
        assert_eq!(e.master_preamp_db(),2.0);assert_eq!(e.spatial_layer_gain_db(),1.5);
        assert_eq!(e.final_output_trim.target_db(),-0.75);
        e.set_spatial_enhancement(false).unwrap();
        assert_eq!(e.master_preamp_db(),0.0);assert_eq!(e.spatial_layer_gain_db(),0.0);
        let command:crate::Command=serde_json::from_str(r#"{"type":"setSpatialEnhancement","enabled":true}"#).unwrap();
        assert!(matches!(command,crate::Command::SetSpatialEnhancement{enabled:true}));
    }

    #[test]
    fn protected_horizontal_main_restores_baseline_without_ids_or_vocal_detection() {
        let settings=Settings::new(48000);let master=10.0f32.powf(2.0/20.0);
        for position in [[-1.0,1.0,0.0],[1.0,1.0,0.0],[-1.0,0.0,0.0],[1.0,0.0,0.0]] {
            let mut s=Source{kind:SourceKind::Object,position,..Default::default()};
            for at in 0..4096 {source_gain(&mut s,6.0,2.0,settings.slew,at);}
            assert!((s.spatial_layer_gain*master-1.0).abs()<1e-6);
        }
        let mut upper=Source{kind:SourceKind::Object,position:[0.0,1.0,1.0],..Default::default()};
        for at in 0..4096 {source_gain(&mut upper,6.0,2.0,settings.slew,at);}
        assert!((upper.spatial_layer_gain*master-10.0f32.powf(8.0/20.0)).abs()<1e-6);
    }

}
