//! Whole-song diagnostic using exported MPEG-H source PCM and metadata.
use super::*;
use std::io::{BufRead, Read, Write};
#[test]
#[ignore = "requires SDA_SONG_PROBE_DIR; renders an externally supplied complete song"]
fn ku100_whole_song_engine_probe() {
    let dir=std::path::PathBuf::from(std::env::var("SDA_SONG_PROBE_DIR").unwrap());
    let manifest=std::env::var("SDA_SONG_PROBE_HRTF").unwrap();
    let tag=std::env::var("SDA_SONG_PROBE_TAG").unwrap();
    let set=hrtf::NativeHrtfSet::load_calibrated(std::path::Path::new(&manifest)).unwrap();
    let mut e=Engine::new(48000,2);
    e.replace_hrtf(set,0.04).unwrap();
    e.configure_near_field(near_field::Settings {enabled:std::env::var("SDA_PROBE_NEAR").as_deref()!=Ok("0"),metres_per_unit:1.0}).unwrap();
    e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);e.set_program_codec("mpegh".into());
    if std::env::var("SDA_PROBE_LAYOUT").as_deref()==Ok("360ra") { e.set_layout(vbap::LayoutId::Sony360Ra13).unwrap(); }
    if let Ok(room)=std::env::var("SDA_SONG_PROBE_ROOM") {
        let reflection_mode = match std::env::var("SDA_SONG_PROBE_REFLECTION").as_deref() {
            Ok("direct") => cinema::ReflectionMode::Direct,
            Ok("early") => cinema::ReflectionMode::Early,
            _ => cinema::ReflectionMode::Full,
        };
        let settings = if std::env::var_os("SDA_SONG_PROBE_LISTENING").is_some() {
            cinema::Settings::room_listening()
        } else {
            cinema::Settings::default()
        };
        e.configure_room(cinema::Settings {enabled:true,reflection_mode,..settings},
            Some(Arc::new(cinema::RoomProfile::load(&room).unwrap()))).unwrap();
    }
    e.output_active=true;e.paused=false;
    let fifo=stereo_fifo::StereoFifo::new(4096);let telemetry=RuntimeTelemetry::default();
    let mut source_pcm=std::io::BufReader::new(std::fs::File::open(dir.join("sources.pcm")).unwrap());
    let mut out=std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{tag}-engine.f32"))).unwrap());
    let scale=std::env::var("SDA_SONG_PROBE_SCALE").ok().map(|v|v.parse::<f32>().unwrap()).unwrap_or(1.0);
    let mut stats=std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{tag}-guard.csv"))).unwrap());
    writeln!(stats,"sample,guard_gain").unwrap();
    let mut blocks=0usize;let mut minimum_gain=1.0_f32;
    for line in std::io::BufReader::new(std::fs::File::open(dir.join("frames.jsonl")).unwrap()).lines() {
        let f:serde_json::Value=serde_json::from_str(&line.unwrap()).unwrap();
        let start=f["start"].as_u64().unwrap();let n=f["samples"].as_u64().unwrap() as usize;
        let mut entries=Vec::new();
        for (channel,label) in f["labels"].as_array().unwrap().iter().enumerate() {
            let label=label.as_str().unwrap();let id=format!("obj:{channel}");
            if blocks==0 {
                assert!(label.starts_with("Obj_"));
                e.sources.insert(id.clone(),Source {kind:SourceKind::Object,object_id:Some(channel as u32),gain:1.0,target_gain:1.0,availability:1.0,availability_target:1.0,..Default::default()});
                e.route_source_now(&id,0).unwrap();
            }
            let mut bytes=vec![0u8;n*4];source_pcm.read_exact(&mut bytes).unwrap();
            let pcm=bytes.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())*scale).collect();
            entries.push((id,pcm));
        }
        // This programme has fixed metadata: compact it exactly as the player.
        let events=if blocks==0 {serde_json::from_value::<Vec<NativeObjectEvent>>(f["events"].clone()).unwrap()} else {Vec::new()};
        assert!(protocol::apply_render_command(&mut e,render_command::RenderCommand::PcmFrame {start,entries,events},&fifo,&telemetry));
        let mut block=vec![0.0;n*2];e.render_into(&mut block,2);
        assert!(block.iter().all(|s|s.is_finite()));
        for v in block {out.write_all(&v.to_le_bytes()).unwrap();}
        writeln!(stats,"{},{}",e.sample_pos,e.peak_guard.diagnostic_gain()).unwrap();
        if blocks == 300 && std::env::var_os("SDA_SONG_PROBE_ROUTE_AUDIT").is_some() {
            let mut sources = Vec::new();
            for channel in 0..f["labels"].as_array().unwrap().len() {
                let id = format!("obj:{channel}");
                let source = e.sources.get(&id).unwrap();
                sources.push(serde_json::json!({
                    "id": id, "objectId": source.object_id, "position": source.position,
                    "gain": source.gain, "targetGain": source.target_gain,
                    "distanceGain": source.distance_gain, "distanceM": source.distance_m,
                    "availability": source.availability, "muted": source.muted,
                    "continuousActive": source.continuous_active,
                    "continuousMix": source.continuous_mix,
                    "nearTargets": source.near_target, "occlusionTargets": source.occlusion_targets,
                    "busGains": source.bus_gains.to_vec(), "lfeGain": source.lfe_gain
                }));
            }
            let snapshot = serde_json::json!({
                "sample": e.sample_pos, "directMix": e.direct_mix, "sources": sources
            });
            std::fs::write(dir.join(format!("{tag}-routes.json")),
                serde_json::to_vec_pretty(&snapshot).unwrap()).unwrap();
        }
        minimum_gain=minimum_gain.min(e.output_gain);blocks+=1;
        if blocks%2000==0 {eprintln!("{tag}: rendered {} seconds",e.sample_pos as f64/48000.0);}
    }
    out.flush().unwrap();stats.flush().unwrap();
    eprintln!("{tag}: {blocks} blocks, {} samples, output gain minimum={minimum_gain}",e.sample_pos);
}
