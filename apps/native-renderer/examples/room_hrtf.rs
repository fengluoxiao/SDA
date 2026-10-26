//! Offline room-asset preparation using the same HRTF loader as playback.
use sda_native_renderer::{hrtf::NativeHrtfSet,cinema::{RoomProfile,Settings,ReflectionMode},vbap::LayoutId};
use std::{fs,path::Path,sync::Arc};
use serde_json::json;
fn write_pair(path:&Path,left:&[f32],right:&[f32]) {
    assert_eq!(left.len(),right.len());
    fs::write(path,left.iter().chain(right).flat_map(|x|x.to_le_bytes()).collect::<Vec<_>>()).unwrap();
}
fn main() {
    let args:Vec<_>=std::env::args().collect();
    assert!(args.len()>=4,"room_hrtf export|reference <live-hrtf-manifest> <output-directory> [room.json]");
    let manifest=Path::new(&args[2]);let out=Path::new(&args[3]);fs::create_dir_all(out).unwrap();
    let mut set=NativeHrtfSet::load_calibrated(manifest).unwrap();
    if args[1]=="export" {
        let source:serde_json::Value=serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
        assert_eq!(source["sharedRenderProfile"],"apple-hrtfhq-542-v1");
        let mut positions=Vec::new();
        for (i,p) in source["positions"].as_array().unwrap().iter().enumerate() {
            let az=p["azimuth"].as_f64().unwrap();let el=p["elevation"].as_f64().unwrap();
            let ir=set.nearest(az,el).unwrap();let n=ir.dry.len()/2;
            let file=format!("direction-{i:03}.f32");write_pair(&out.join(&file),&ir.dry[..n],&ir.dry[n..]);
            positions.push(json!({"azimuth":az,"elevation":el,"dry":file,"samplesPerEar":n}));
        }
        fs::write(out.join("hrtf-set.json"),serde_json::to_vec_pretty(&json!({
            "roomReceiverSchema":1,"sampleRate":48000,"profileId":"apple-hrtfhq-542-v1",
            "preparation":"native-shared-clarity-v1","nativeRevision":env!("SDA_RENDERER_REVISION"),
            "positions":positions,
        })).unwrap()).unwrap();
    } else {
        assert_eq!(args[1],"reference");
        let profile=Arc::new(RoomProfile::load(&args[4]).unwrap());
        let layout=LayoutId::parse(&profile.layout).unwrap();let mut entries=Vec::new();
        for (stage,mode) in [("direct",ReflectionMode::Direct),("early",ReflectionMode::Early),("room",ReflectionMode::Full)] {
            let settings=Settings{enabled:true,reflection_mode:mode,..Settings::default()};
            set.configure_cinema(settings,Some(profile.clone()));set.configure_spatial_cues(layout).unwrap();
            for (i,speaker) in profile.speakers.iter().enumerate() {
                let (left,right)=set.mixed_speaker(&speaker.name,&profile.layout,speaker.azimuth as f64,speaker.elevation as f64,0.04).unwrap();
                let file=format!("{stage}-{i}.f32");write_pair(&out.join(&file),&left,&right);
                entries.push(json!({"stage":stage,"speaker":speaker.name,"file":file,"samplesPerEar":left.len()}));
            }
        }
        fs::write(out.join("reference.json"),serde_json::to_vec(&entries).unwrap()).unwrap();
    }
}
