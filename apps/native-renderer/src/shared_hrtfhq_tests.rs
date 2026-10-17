//! Shared synthetic output contract; no copyrighted fixture or Apple runtime.
use super::*;
fn manifest() -> std::path::PathBuf {
    std::env::var_os("SDA_SHARED_PROFILE_MANIFEST").map(std::path::PathBuf::from)
        .unwrap_or_else(||std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json"))
}
fn render(layout:vbap::LayoutId,chunk:usize,mode:&str) -> Vec<f32> {
    render_recipe(layout,chunk,mode,false)
}
fn render_recipe(layout:vbap::LayoutId,chunk:usize,mode:&str,explicit:bool) -> Vec<f32> {
    let is_bed=mode!="objects";
    let count=if mode=="stereo" {2} else {8};
    let labels=["FrontLeft","FrontRight","Center","SurroundLeft","SurroundRight","TopFrontLeft","TopRearRight","LFE"];
    const FRAMES:usize=98304;
    let mut e=Engine::new(48000,2);e.set_layout(layout).unwrap();
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap(),0.0).unwrap();
    if explicit {e.set_spatial_enhancement(true).unwrap();}
    e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);e.direct_mix=1.0;
    assert_eq!(e.spatial_layer_gain_db(),1.5);
    e.set_spatial_balance(false);e.front_common.enabled=false;
    e.set_master_preamp_db(-12.0).unwrap();
    e.set_program_codec(if mode=="stereo" {"alac"} else if layout==vbap::LayoutId::Dolby7_1_4 {"eac3"} else {"mpegh"}.into());
    e.output_active=true;e.paused=false;
    for (index,position) in [[0.,1.,0.],[-1.,0.,1.],[1.,0.,1.],[0.,-1.,0.],[-1.,1.,0.],
        [1.,1.,0.],[-0.7,-1.,0.6],[0.4,0.7,-0.5]].into_iter().take(count).enumerate() {
        let id=format!("{}:{}",if is_bed {"bed"} else {"obj"},index+1);let mut state=0x12345678u32.wrapping_add(index as u32);
        let pcm:Vec<f32>=(0..FRAMES).map(|i| {state=state.wrapping_mul(1664525).wrapping_add(1013904223);
            if i<4096 {0.0} else {((state>>16) as i32-32768) as f32*(0.02/32768.0)}}).collect();
        let mut source=Source{kind:if is_bed {SourceKind::Bed} else {SourceKind::Object},
            object_id:(!is_bed).then_some(index as u32+1),bed_label:is_bed.then(||labels[index].to_string()),position,
            gain:1.,target_gain:1.,availability:1.,availability_target:1.,..Default::default()};
        if is_bed {Engine::set_source_route(&mut source,bed_route(labels[index],&e.vbap),0);}
        source.samples.write(0,0,&pcm);e.sources.insert(id.clone(),source);e.route_source_now(&id,0).unwrap();
    }
    let mut output=vec![0.;FRAMES*2];
    for start in (0..FRAMES).step_by(chunk) {
        if start==32768 || start==65536 {
            let q=std::f32::consts::FRAC_1_SQRT_2;
            e.head_pose=Some(if start==32768 {[0.,0.,q,q]} else {[q,0.,0.,q]});
            e.pose_route_base=e.head_pose;
            if start==65536 && !is_bed {e.sources.get_mut("obj:8").unwrap().position=[-0.8,-0.5,0.8];}
            let ids:Vec<_>=e.sources.keys().cloned().collect();for id in ids {e.route_source_now(&id,256).unwrap();}
        }
        e.render_into(&mut output[start*2..(start+chunk).min(FRAMES)*2],2);
    }
    assert_eq!(e.peak_guard.diagnostic_gain(),1.0);
    assert!(output.iter().all(|v|v.is_finite()));assert!(output.iter().any(|v|v.abs()>1e-5));
    assert_eq!(e.active_hrtf_set.as_ref().unwrap().spatial_cues_active(), layout!=vbap::LayoutId::Stereo2_0);
    assert_eq!(e.sources.len(),count);
    output
}
#[test]
fn shared_hrtfhq_synthetic_output_is_callback_partition_invariant() {
    for (name,layout,mode) in [("atmos",vbap::LayoutId::Dolby7_1_4,"objects"),("360ra",vbap::LayoutId::Sony360Ra13,"objects"),
        ("bed714",vbap::LayoutId::Dolby7_1_4,"beds"),("stereo",vbap::LayoutId::Stereo2_0,"stereo")] {
        let a=render(layout,128,mode);let b=render(layout,512,mode);
        let error=a.iter().zip(&b).map(|(a,b)|(a-b).abs()).fold(0.0f32,f32::max);
        assert!(error<2e-6,"callback partition changes shared {name} output: {error}");
        if let Some(root)=std::env::var_os("SDA_SHARED_PROFILE_PCM_DIR") {
            let root=std::path::PathBuf::from(root);std::fs::create_dir_all(&root).unwrap();
            let data:Vec<u8>=a.iter().flat_map(|v|v.to_le_bytes()).collect();
            std::fs::write(root.join(format!("{name}.f32")),data).unwrap();
        }
    }
}

#[test]
fn shared_hrtfhq_default_presentation_is_host_independent_and_reload_is_idempotent() {
    let mut e=Engine::new(48000,2);
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap(),0.0).unwrap();
    assert_eq!(e.master_preamp_db(),0.0);assert_eq!(e.spatial_layer_gain_db(),1.5);
    assert_eq!(e.spatial_layer.main_restore_db,0.0);
    e.reset_session(0);assert_eq!(e.spatial_layer_gain_db(),1.5,"track reset lost presentation defaults");
    e.set_spatial_enhancement(false).unwrap();
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap(),0.0).unwrap();
    assert_eq!(e.spatial_layer_gain_db(),0.0,"same-profile reload must retain an explicit override");
    let mut live=Engine::new(48000,2);
    live.configure_hrtf_preset(hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap(),0.0,true,true).unwrap();
    assert_eq!(live.spatial_layer_gain_db(),1.5,"live preset path must apply the same recipe");
}

#[test]
fn shared_hrtfhq_default_matches_the_auditioned_presentation() {
    let automatic=render_recipe(vbap::LayoutId::Dolby7_1_4,128,"objects",false);
    let explicit=render_recipe(vbap::LayoutId::Dolby7_1_4,128,"objects",true);
    let error=automatic.iter().zip(&explicit).map(|(a,b)|(a-b).abs()).fold(0.0f32,f32::max);
    assert!(error<2e-6,"host defaults differ from the approved explicit recipe: {error}");
}

#[test]
fn shared_hrtfhq_depth_preserves_direct_and_respects_layout_and_user_room() {
    let original=hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    for layout in [vbap::LayoutId::Dolby7_1_4,vbap::LayoutId::Sony360Ra13] {
        let mut enabled=original.clone();enabled.configure_spatial_cues(layout).unwrap();
        let mut pure=original.diagnostic_without_shared_depth();pure.configure_spatial_cues(layout).unwrap();
        assert!(enabled.spatial_cues_active());assert!(!pure.spatial_cues_active());
        for s in vbap::speakers(layout) {
            let base=pure.mixed_speaker(s.name,layout.as_str(),s.azimuth as f64,s.elevation as f64,0.0).unwrap();
            let direct=enabled.mixed_speaker(s.name,layout.as_str(),s.azimuth as f64,s.elevation as f64,0.0).unwrap();
            let full=enabled.mixed_speaker(s.name,layout.as_str(),s.azimuth as f64,s.elevation as f64,enabled.effective_wet(0.0)).unwrap();
            for (a,b) in [(&base.0,&direct.0),(&base.1,&direct.1)] {
                assert_eq!(a.as_slice(),&b[..a.len()],"direct response changed: {}",s.name);
                assert!(b[a.len()..].iter().all(|x|*x==0.0));
            }
            assert!(full.0.iter().chain(&full.1).all(|x|x.is_finite()));
            assert!(full.0.iter().chain(&full.1).zip(direct.0.iter().chain(&direct.1)).any(|(a,b)|(a-b).abs()>1e-6));
        }
        enabled.configure_spatial_cues(vbap::LayoutId::Stereo2_0).unwrap();assert!(!enabled.spatial_cues_active());
        enabled.configure_spatial_cues(layout).unwrap();assert!(enabled.spatial_cues_active());
        enabled.configure_cinema(cinema::Settings{enabled:true,..Default::default()},None);
        enabled.configure_spatial_cues(layout).unwrap();assert!(!enabled.spatial_cues_active(),"user room must not acquire bundled residual");
    }
}
