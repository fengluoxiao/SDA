//! The selected production controller must consume measured room, preserve
//! protected foreground and ear cues, and reset safely across transport changes.
use super::*;
const RATE:u32=48000;
fn make(lift:bool,fast:bool,volume:bool,layout:vbap::LayoutId,head:Option<[f32;4]>)->Engine{
    let mut e=Engine::new(RATE,2);e.set_layout(layout).unwrap();
    let manifest=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap(),0.).unwrap();
    e.set_spatial_enhancement(true).unwrap();
    assert!(e.auxiliary_balance.temporal_control().directional_strength.is_some(),"test must use the production controller");
    e.auxiliary_balance.set_measured_lift_for_diagnostics(lift);
    e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);
    e.set_program_codec(if layout==vbap::LayoutId::Sony360Ra13{"mpegh"}else{"eac3"}.into());
    e.set_output_volume(1.,true);e.set_master_preamp_db(0.).unwrap();
    e.program_enabled=volume;e.set_program_target(if volume{0.6}else{1.},true);
    e.disable_fast_objects=!fast;e.head_pose=head;e.direct_mix=1.;e.output_active=true;e.paused=false;e
}
fn source(position:[f32;3])->Source{Source{kind:SourceKind::Object,position,gain:1.,target_gain:1.,availability:1.,availability_target:1.,..Default::default()}}
fn amplitude(pcm:&[f32],hz:f64,ear:usize)->f64{
    let(mut re,mut im)=(0.,0.);for(i,f)in pcm.chunks_exact(2).enumerate(){let p=std::f64::consts::TAU*hz*i as f64/f64::from(RATE);re+=f64::from(f[ear])*p.cos();im+=f64::from(f[ear])*p.sin();}(re*re+im*im).sqrt()
}
struct Render{pcm:Vec<f32>,gain_db:f64,common_db:f64,guard_gain:f32,source_count:usize}
fn render(position:[f32;3],lift:bool,fast:bool,volume:bool,layout:vbap::LayoutId,head:Option<[f32;4]>,chunk:usize,split:usize)->Render{
    let mut e=make(lift,fast,volume,layout,head);let frames=RATE as usize*5;
    e.sources.insert("front".into(),source([0.,1.,0.]));
    let front:Vec<_>=(0..frames).map(|i|0.02*(std::f32::consts::TAU*997.*i as f32/RATE as f32).sin()).collect();
    e.sources.get_mut("front").unwrap().samples.write(0,0,&front);e.route_source_now("front",0).unwrap();
    for id in 0..split {
        let name=format!("auxiliary-{id}");let mut s=source(position);s.object_id=Some(id as u32+1);
        let samples:Vec<_>=(0..frames).map(|i|{let p=std::f32::consts::TAU*i as f32/RATE as f32;0.001/split as f32*((p*251.).sin()+0.8*(p*1003.).sin()+0.2*(p*6007.).sin())}).collect();
        s.samples.write(0,0,&samples);e.sources.insert(name.clone(),s);e.route_source_now(&name,0).unwrap();
    }
    let mut pcm=vec![0.;frames*2];for output in pcm.chunks_mut(chunk*2){e.render_into(output,2);}
    Render{pcm,gain_db:e.auxiliary_balance.levels()[0],common_db:e.output_dynamics.gain_db(),guard_gain:e.peak_guard.diagnostic_gain(),source_count:e.sources.len()}
}
#[test]
fn measured_room_lifts_rear_overhead_and_front_height_without_ducking_foreground(){
    let mut rows=vec![];
    for (name,p)in [("rear",[0.,-1.,0.]),("front-top",[0.,0.5,0.8660254]),("zenith",[0.,0.,1.]),("rear-top",[-0.5,-0.5,0.70710677]),("rear-bottom",[0.,-0.8660254,-0.5])]{
        for volume in [false,true]{
            let layout=if volume{vbap::LayoutId::Sony360Ra13}else{vbap::LayoutId::Dolby7_1_4};
            let a=render(p,false,true,volume,layout,None,173,1);let b=render(p,true,true,volume,layout,None,173,1);
            assert_eq!(a.common_db,0.);assert_eq!(b.common_db,0.);assert_eq!(b.guard_gain,1.);assert!(b.gain_db>0.9,"no native measured lift: {name}/{volume}: {}",b.gain_db);
            assert_eq!(b.source_count,2);assert!(b.pcm.iter().all(|v|v.is_finite()));
            let n=RATE as usize*8;let bands:Vec<_>=[251.,1003.,6007.].into_iter().map(|hz|{
                let delta:[f64;2]=std::array::from_fn(|ear|20.*(amplitude(&b.pcm[n..],hz,ear)/amplitude(&a.pcm[n..],hz,ear)).log10());
                for db in delta{assert!(db>0.7&&db<1.2,"unexpected native gain {name}/{volume}/{hz}: {delta:?}");}
                assert!((delta[0]-delta[1]).abs()<0.02,"interaural cue changed: {name}/{hz}: {delta:?}");
                serde_json::json!({"hz":hz,"gainDb":delta})
            }).collect();
            let front:[f64;2]=std::array::from_fn(|ear|20.*(amplitude(&b.pcm[n..],997.,ear)/amplitude(&a.pcm[n..],997.,ear)).log10());
            assert!(front.iter().all(|v|v.abs()<0.02),"protected foreground changed: {name}: {front:?}");
            rows.push(serde_json::json!({"position":name,"volumeBalance":volume,"layout":format!("{layout:?}"),"groupGainDb":b.gain_db,"frontDeltaDb":front,"bands":bands}));
        }
    }
    if let Some(out)=std::env::var_os("SDA_HEADROOM_FIX_DIR") {std::fs::write(std::path::PathBuf::from(out).join("native-gain-matrix.json"),serde_json::to_vec_pretty(&rows).unwrap()).unwrap();}
}
#[test]
fn measured_gain_is_shared_by_fast_regular_partitioned_and_split_source_paths(){
    let p=[-0.5,-0.5,0.70710677];let head=Some([0.,0.,0.258819,0.965926]);
    let baseline=render(p,true,true,true,vbap::LayoutId::Sony360Ra13,head,1024,1);
    for (fast,chunk,split)in [(true,1,1),(false,173,1),(true,257,3),(false,4093,3)]{
        let b=render(p,true,fast,true,vbap::LayoutId::Sony360Ra13,head,chunk,split);
        let max=baseline.pcm.iter().zip(&b.pcm).map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
        assert!(max<2e-6,"gain or geometry depends on mixer/callback/source count: fast={fast} chunk={chunk} split={split} max={max}");
        assert!((baseline.gain_db-b.gain_db).abs()<1e-4);
    }
}
#[test]
fn limiter_veto_and_seek_reset_cannot_leave_auxiliary_gain_or_common_ducking(){
    let mut e=make(true,true,true,vbap::LayoutId::Dolby7_1_4,None);
    // Use the actual shared controller, then force the real linked guard to
    // limit a final-mix burst. The handoff must reject its attenuation as room.
    for i in 0..RATE*4{
        let mut r=auxiliary_balance::Reference::default();r.groups[0][0]=(i as f64*0.13).sin()*0.02;r.groups[1][0]=(i as f64*0.17).sin()*0.001;
        e.auxiliary_balance.observe_at(r,1.,i as u64);let x=0.01*(i as f32*0.13).sin();e.process_final_output([x,-x*0.8]);
        if i as usize%convolution::DEFAULT_PARTITION+1==convolution::DEFAULT_PARTITION {e.auxiliary_balance.set_output_headroom(e.output_dynamics.available_headroom_db());e.auxiliary_balance.finish_block();}
    }
    assert!(e.auxiliary_balance.gains[0]>1.1);
    for _ in 0..1024{e.process_final_output([3.,-2.]);}
    assert!(e.peak_guard.is_limiting());assert_eq!(e.output_dynamics.gain_db(),0.);
    // Exercise the production render loop's end-of-block budget handoff.
    e.render_into(&mut vec![0.;convolution::DEFAULT_PARTITION*2],2);
    assert_eq!(e.auxiliary_balance.headroom_db(),None);
    e.reset_seek(0,48000*12);assert!(e.auxiliary_balance.gains.iter().all(|g|*g==1.));
    assert_eq!(e.auxiliary_balance.headroom_db(),None);assert_eq!(e.output_dynamics.gain_db(),0.);
    assert_eq!(e.peak_guard.diagnostic_gain(),1.);
}
