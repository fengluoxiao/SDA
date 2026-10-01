//! Offline moving-object regression probe; requires externally decoded PCM.
use super::*;
use std::io::{BufRead, Read, Write};
#[test]
#[ignore = "requires SDA_MOTION_PROBE_DIR and calibrated HRTF assets"]
fn ku100_motion_engine_probe() {
    let dir=std::path::PathBuf::from(std::env::var("SDA_MOTION_PROBE_DIR").unwrap());
    let manifest=std::env::var("SDA_SONG_PROBE_HRTF").unwrap();
    let tag=std::env::var("SDA_SONG_PROBE_TAG").unwrap();
    let only=std::env::var("SDA_PROBE_ONLY").unwrap_or_default();
    let selected:Vec<&str>=only.split(',').filter(|v|!v.is_empty()).collect();
    let mut e=Engine::new(48000,2);
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(std::path::Path::new(&manifest)).unwrap(),0.04).unwrap();
    e.near_field=near_field::Settings {enabled:std::env::var("SDA_PROBE_NEAR").as_deref()==Ok("1"),metres_per_unit:1.0};
    e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);e.set_program_codec("eac3".into());
    e.output_active=true;e.paused=false;
    let fifo=stereo_fifo::StereoFifo::new(4096);let telemetry=RuntimeTelemetry::default();
    let mut input=std::io::BufReader::new(std::fs::File::open(dir.join("sources.pcm")).unwrap());
    let mut out=std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{tag}-engine.f32"))).unwrap());
    let mut meter=std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{tag}-meter.csv"))).unwrap());
    writeln!(meter,"sample,id,x,y,z,gain,distance_gain,near_l,near_r,guard_gain").unwrap();
    let scale=std::env::var("SDA_SONG_PROBE_SCALE").ok().map(|v|v.parse::<f32>().unwrap()).unwrap_or(0.1);
    let mut blocks=0usize;
    for line in std::io::BufReader::new(std::fs::File::open(dir.join("frames.jsonl")).unwrap()).lines() {
        let f:serde_json::Value=serde_json::from_str(&line.unwrap()).unwrap();
        let start=f["start"].as_u64().unwrap();let n=f["samples"].as_u64().unwrap() as usize;
        let mut entries=Vec::new();
        for label in f["labels"].as_array().unwrap() {
            let label=label.as_str().unwrap();
            let object_id=label.strip_prefix("Obj_").map(|v|v.parse::<u32>().unwrap());
            let id=object_id.map_or_else(||format!("bed:{label}"),|v|format!("obj:{v}"));
            if !e.sources.contains_key(&id) {
                e.sources.insert(id.clone(),Source {kind:if object_id.is_some(){SourceKind::Object}else{SourceKind::Bed},object_id,bed_label:object_id.is_none().then(||label.to_string()),gain:1.0,target_gain:1.0,availability:1.0,availability_target:1.0,..Default::default()});
                e.route_source_now(&id,0).unwrap();
            }
            let active=selected.is_empty()||selected.contains(&label);
            let mut bytes=vec![0u8;n*4];input.read_exact(&mut bytes).unwrap();
            let pcm=bytes.chunks_exact(4).map(|b|if active{f32::from_le_bytes(b.try_into().unwrap())*scale}else{0.0}).collect();
            entries.push((id,pcm));
        }
        let events=serde_json::from_value::<Vec<NativeObjectEvent>>(f["events"].clone()).unwrap();
        assert!(protocol::apply_render_command(&mut e,render_command::RenderCommand::PcmFrame {start,entries,events},&fifo,&telemetry));
        let mut block=vec![0.0;n*2];e.render_into(&mut block,2);
        assert!(block.iter().all(|s|s.is_finite()));
        for v in block{out.write_all(&v.to_le_bytes()).unwrap();}
        for id in [12u32,14] {
            let s=e.sources.get(&format!("obj:{id}")).unwrap();
            writeln!(meter,"{},{},{},{},{},{},{},{},{},{}",e.sample_pos,id,s.position[0],s.position[1],s.position[2],s.gain,s.distance_gain,s.near_target[0],s.near_target[1],e.peak_guard.diagnostic_gain()).unwrap();
        }
        blocks+=1;
    }
    out.flush().unwrap();meter.flush().unwrap();
    eprintln!("{tag}: {blocks} blocks, {} samples",e.sample_pos);
}
