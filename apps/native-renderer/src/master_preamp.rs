//! User-selected linked master preamp, not object enhancement or loudness normalization.
//! Default is exactly unity; the existing final peak guard follows this stage.
#[derive(Clone, Debug)]
pub(crate) struct MasterPreamp {
    gain: f32,
    target: f32,
    target_db: f32,
    step: f32,
    remaining: u32,
}
impl Default for MasterPreamp {
    fn default() -> Self { Self { gain:1.0,target:1.0,target_db:0.0,step:0.0,remaining:0 } }
}
impl MasterPreamp {
    pub fn set(&mut self, db:f32, rate:u32, immediate:bool) -> Result<(),String> {
        if !db.is_finite() || !(-12.0..=12.0).contains(&db) {
            return Err("master preamp must be finite and between -12 and +12 dB".into());
        }
        self.target_db=db;self.target=10.0f32.powf(db/20.0);
        if immediate {self.settle();} else {
            self.remaining=(rate.max(1) as f32*0.02).round().max(1.0) as u32;
            self.step=(self.target-self.gain)/self.remaining as f32;
        }
        Ok(())
    }
    pub fn gain(&self)->f32 {self.gain}
    pub fn target_db(&self)->f32 {self.target_db}
    pub fn advance(&mut self) {
        if self.remaining>0 {
            self.gain+=self.step;self.remaining-=1;
            if self.remaining==0 {self.gain=self.target;self.step=0.0;}
        }
    }
    /// Transport resets preserve the listener's setting, not a stale half-ramp.
    pub fn settle(&mut self) {self.gain=self.target;self.remaining=0;self.step=0.0;}
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    #[test]
    fn default_unity_and_invalid_changes_are_atomic() {
        let mut p=MasterPreamp::default();assert_eq!(p.gain(),1.0);assert_eq!(p.target_db(),0.0);
        p.set(2.0,48000,true).unwrap();let old=p.clone();
        for db in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY,-12.01,12.01] {
            assert!(p.set(db,48000,false).is_err());assert_eq!(p.gain(),old.gain());assert_eq!(p.target_db(),old.target_db());
        }
    }
    #[test]
    fn bounded_ramp_retargets_and_reaches_exact_unity() {
        for rate in [44100,48000,96000] {
            let mut p=MasterPreamp::default();p.set(6.0,rate,false).unwrap();
            let n=(rate as f32*0.02).round() as usize;let mut last=1.0;
            for _ in 0..n {p.advance();assert!(p.gain()>=last && p.gain()<=10.0f32.powf(6.0/20.0)+1e-5);last=p.gain();}
            assert_eq!(p.gain(),10.0f32.powf(6.0/20.0));
            p.set(-6.0,rate,false).unwrap();p.advance();p.set(0.0,rate,false).unwrap();
            for _ in 0..n {p.advance();}assert_eq!(p.gain(),1.0);
            p.set(3.0,rate,false).unwrap();p.settle();assert_eq!(p.gain(),10.0f32.powf(3.0/20.0));
        }
    }
    fn render(db:f32, amplitude:f32) -> Vec<f32> {
        let mut e=Engine::new(48000,2);
        let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json");
        e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),0.0).unwrap();
        e.set_layout(vbap::LayoutId::Stereo2_0).unwrap();e.stereo_mode=StereoMode::Original;
        e.stereo_weights=[1.0,0.0,0.0];
        e.set_master_preamp_db(db).unwrap();e.paused=false;e.output_active=true;
        for (ear,label) in ["FrontLeft","FrontRight"].iter().enumerate() {
            let mut s=Source{kind:SourceKind::Bed,bed_label:Some((*label).into()),gain:1.0,target_gain:1.0,
                availability:1.0,availability_target:1.0,..Default::default()};
            Engine::set_source_route(&mut s,bed_route(label,&e.vbap),0);
            let pcm:Vec<_>=(0..8192).map(|i|amplitude*(if ear==0 {1.0} else {0.5})*(i as f32*0.173).sin()).collect();
            s.samples.write(0,0,&pcm);e.sources.insert((*label).into(),s);
        }
        let mut out=vec![0.0;16384];e.render_into(&mut out,2);out
    }
    #[test]
    fn engine_master_scales_both_ears_without_remixing() {
        let reference=render(0.0,0.001);
        assert!(reference.iter().any(|v|v.abs()>1e-5));
        for db in [-6.0,2.0,6.0] {
            let output=render(db,0.001);let gain=10.0f32.powf(db/20.0);
            for (a,b) in reference.iter().zip(output) {assert!((*a*gain-b).abs()<1e-7);}
        }
        let command:Command=serde_json::from_str(r#"{"type":"setMasterPreamp","gainDb":2}"#).unwrap();
        assert!(matches!(command,Command::SetMasterPreamp{gain_db} if gain_db==2.0));
    }
    #[test]
    fn final_linked_guard_contains_hot_master_and_preserves_ear_ratio() {
        let output=render(12.0,0.7);
        assert!(output.iter().all(|v|v.is_finite() && v.abs()<=1.0));
        assert!(output.iter().any(|v|v.abs()>0.5), "guard test must render audible nonzero audio");
        for frame in output.chunks_exact(2) {assert!((frame[0]*0.5-frame[1]).abs()<1e-6);}
    }
}
