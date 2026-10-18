use super::*;
fn scene(off:bool,slow:bool,layout:vbap::LayoutId)->Engine {
    let mut e=Engine::new(48000,2);e.set_layout(layout).unwrap();
    let manifest=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap(),0.).unwrap();
    e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);e.set_program_codec(if layout==vbap::LayoutId::Sony360Ra13 {"mpegh"} else {"eac3"}.into());
    e.set_binaural_enabled(!off);e.disable_fast_objects=slow;e.direct_mix=1.;
    e.set_master_preamp_db(-12.).unwrap();e.set_output_volume(0.5,true);
    let mut entries=Vec::new();
    for i in 0..8 {
        let id=format!("obj:{}",i+1);let angle=i as f32*0.72;
        let mut source=Source{kind:SourceKind::Object,object_id:Some(i+1),position:[angle.sin(),angle.cos(),0.4],gain:1.,target_gain:1.,availability:1.,availability_target:1.,..Default::default()};
        let pcm=(0..32768).map(|n| (n as f32*(0.021+i as f32*0.003)).sin()*0.002).collect();
        source.gain=1.;e.sources.insert(id.clone(),source);e.route_source_now(&id,0).unwrap();entries.push((id,pcm));
    }
    let compatibility=[(0..32768).map(|n|(n as f32*0.13).sin()*0.03).collect(),(0..32768).map(|n|(n as f32*0.17).cos()*0.02).collect()];
    let fifo=stereo_fifo::StereoFifo::new(4096);let telemetry=RuntimeTelemetry::default();let (reply,rx)=std::sync::mpsc::channel();
    assert!(protocol::apply_render_command(&mut e,render_command::RenderCommand::PcmFrameWithCompatibility{start:0,entries,events:vec![],compatibility,reply:Some(reply)},&fifo,&telemetry));
    assert!(rx.recv().unwrap());e.output_active=true;e.paused=false;e
}
#[test]
fn off_to_fixed_uses_already_warm_objects_without_new_pcm_or_source_replacement() {
    for (layout,slow) in [(vbap::LayoutId::Dolby7_1_4,false),(vbap::LayoutId::Dolby7_1_4,true),(vbap::LayoutId::Sony360Ra13,false),(vbap::LayoutId::Sony360Ra13,true)] {
        let mut warm=scene(true,slow,layout);let mut reference=scene(false,slow,layout);
        let mut plain=vec![0.;8192*2];let mut ignored=vec![0.;8192*2];
        warm.render_into(&mut plain,2);reference.render_into(&mut ignored,2);
        let gain=10f32.powf(-12./20.)*0.25;
        // Plain path retains the existing partition delay and 5 ms linked-guard lookahead.
        let latency=convolution::DEFAULT_PARTITION+240;
        for n in latency+128..8192 {
            let input=n-latency;
            assert!((plain[n*2]-(input as f32*0.13).sin()*0.03*gain).abs()<1e-7);
            assert!((plain[n*2+1]-(input as f32*0.17).cos()*0.02*gain).abs()<1e-7,"n={n} actual={} expected={} mix={} master={} output={}",plain[n*2+1],(input as f32*0.17).cos()*0.02*gain,warm.binaural_mix,warm.master_preamp.gain(),warm.output_gain);
        }
        let identities:Vec<_>=warm.sources.iter().map(|(id,s)|(id.clone(),s.continuous.as_deref().map(|v|v as *const _ as usize).expect("objects must already be prepared while Off"))).collect();
        warm.set_binaural_enabled(true);
        for (id,address) in identities {assert_eq!(warm.sources[&id].continuous.as_deref().unwrap() as *const _ as usize,address);}
        assert_eq!(warm.sample_pos,8192);assert_eq!(warm.sources.len(),8);
        let mut actual=vec![0.;8192*2];let mut expected=vec![0.;8192*2];warm.render_into(&mut actual,2);reference.render_into(&mut expected,2);
        assert_eq!(warm.peak_guard.diagnostic_gain(),1.);assert_eq!(reference.peak_guard.diagnostic_gain(),1.);
        let error=actual[2048..].iter().zip(&expected[2048..]).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);
        assert!(error<3e-7,"cold/incorrect spatial transition slow={slow}: {error}");
        // Switching back also uses the already clocked reference, not a new bed graph.
        warm.set_binaural_enabled(false);let mut output=vec![0.;4096*2];warm.render_into(&mut output,2);
        for n in latency+1024..4096 {let input=16384+n-latency;assert!((output[n*2]-(input as f32*0.13).sin()*0.03*gain).abs()<1e-7);}
    }
}
#[test]
fn compatible_pcm_is_validated_atomically_and_cleared_by_transport_reset() {
    let mut e=Engine::new(48000,2);e.sources.insert("obj:1".into(),Source::default());
    let fifo=stereo_fifo::StereoFifo::new(4096);let telemetry=RuntimeTelemetry::default();
    let submit=|e:&mut Engine,start,compatibility| {
        let (reply,rx)=std::sync::mpsc::channel();
        protocol::apply_render_command(e,render_command::RenderCommand::PcmFrameWithCompatibility{start,entries:vec![("obj:1".into(),vec![0.1;32])],events:vec![],compatibility,reply:Some(reply)},&fifo,&telemetry);
        rx.recv().unwrap()
    };
    assert!(!submit(&mut e,0,[vec![0.;31],vec![0.;32]]));assert!(e.compatible_pcm.is_none());assert!(!e.sources["obj:1"].samples.has_at(0));
    assert!(!submit(&mut e,0,[vec![f32::NAN;32],vec![0.;32]]));assert!(e.compatible_pcm.is_none());
    assert!(submit(&mut e,0,[vec![0.01;32],vec![0.02;32]]));
    assert!(!submit(&mut e,MAX_PENDING_SAMPLES as u64+1,[vec![0.;32],vec![0.;32]]));
    assert_eq!(e.compatible_pcm.as_mut().unwrap().take(0),Some([0.01,0.02]));
    // A completed replay is idempotent while its source still exists.
    e.sample_pos=100;assert!(submit(&mut e,0,[vec![0.3;32],vec![0.4;32]]));
    assert_eq!(e.compatible_pcm.as_mut().unwrap().take(100),None);
    e.reset_session(100);assert_eq!(e.compatible_pcm.as_mut().unwrap().take(1),None);
    // Reset retired that graph: a pre-seek source/frame cannot be resurrected.
    assert!(!submit(&mut e,0,[vec![0.3;32],vec![0.4;32]]));
}

#[test]
fn listener_yaw_telemetry_uses_the_renderer_applied_pose() {
    let mut engine=Engine::new(48000,2);let telemetry=RuntimeTelemetry::default();
    telemetry.publish_rendering_state(&engine);assert_eq!(telemetry.listener_yaw_degrees(),0.);
    for degrees in [-40f32,0.,30.] {
        let half=degrees.to_radians()/2.;engine.head_pose=Some([0.,0.,half.sin(),half.cos()]);
        telemetry.publish_rendering_state(&engine);
        assert!((telemetry.listener_yaw_degrees()-degrees as f64).abs()<1e-4);
    }
    engine.head_pose=None;telemetry.publish_rendering_state(&engine);assert_eq!(telemetry.listener_yaw_degrees(),0.);
}
