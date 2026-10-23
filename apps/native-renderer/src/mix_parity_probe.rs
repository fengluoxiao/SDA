//! Offline reproduction using decoded real-song PCM, not synthetic single tones.
use super::*;
use std::io::BufRead;

#[derive(serde::Deserialize)]
struct Frame {
    start: u64,
    labels: Vec<String>,
    channels: Vec<Vec<f32>>,
    events: Vec<NativeObjectEvent>,
}

#[test]
#[ignore = "requires SDA_MIX_FRAMES and SDA_PROBE_ROOM; real-song all-enabled summation audit"]
fn real_song_all_enabled_mix_equals_independent_channels() {
    let frames: Vec<Frame> = std::io::BufReader::new(std::fs::File::open(std::env::var("SDA_MIX_FRAMES").unwrap()).unwrap())
        .lines().take(160).map(|l| serde_json::from_str(&l.unwrap()).unwrap()).collect();
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/public/hrtf-dense/hrtf-set.json");
    let set = hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap();
    let room = Arc::new(cinema::RoomProfile::load(&std::env::var("SDA_PROBE_ROOM").unwrap()).unwrap());
    let render = |only: Option<usize>, slow: bool| {
        let mut e = Engine::new(48000,2);
        e.replace_hrtf(set.clone(),0.04).unwrap();
        e.configure_room(cinema::Settings { enabled:true,..Default::default() },Some(room.clone())).unwrap();
        e.configure_near_field(near_field::Settings { enabled:true,metres_per_unit:1.0 }).unwrap();
        e.set_direct_objects(true).unwrap(); e.set_directional_hrtf(true); e.set_program_codec("eac3".into());
        e.output_active=true; e.paused=false; e.disable_fast_objects=slow;
        e.sample_pos=frames[0].start;
        let mut ids=Vec::new();
        for (ch,label) in frames[0].labels.iter().enumerate() {
            let object_id=label.strip_prefix("Obj_").and_then(|s|s.parse::<u32>().ok());
            let id=object_id.map_or_else(||format!("bed:{ch}"),|id|format!("obj:{id}"));
            if only.is_none_or(|c|c==ch) {
                e.sources.insert(id.clone(), Source { kind:if object_id.is_some(){SourceKind::Object}else{SourceKind::Bed},
                    object_id,bed_label:object_id.is_none().then(||label.clone()),gain:1.0,target_gain:1.0,
                    availability:1.0,availability_target:1.0,..Default::default() });
                e.route_source_now(&id,0).unwrap();
            }
            ids.push(id);
        }
        let fifo=stereo_fifo::StereoFifo::new(4096);
        let telemetry=RuntimeTelemetry::default();
        let mut pcm=Vec::new(); let mut shaded=0;
        for frame in &frames {
            let entries=frame.channels.iter().enumerate().filter(|(ch,_)|only.is_none_or(|c|c==*ch))
                .map(|(ch,a)|(ids[ch].clone(),a.clone())).collect();
            let events=frame.events.iter().filter(|event|e.sources.contains_key(&format!("obj:{}",event.id)))
                .cloned().collect();
            assert!(protocol::apply_render_command(&mut e,render_command::RenderCommand::PcmFrame {start:frame.start,entries,events},&fifo,&telemetry));
            let mut block=vec![0.;frame.channels[0].len()*2]; e.render_into(&mut block,2); pcm.extend(block);
            shaded+=e.sources.values().filter(|s|s.occlusion_targets.iter().any(|a|*a<0.999)).count();
        }
        println!("only={only:?} slow={slow} fast_blocks={} shaded={shaded}",e.fast_object_blocks);
        pcm
    };
    let all=render(None,false); let slow=render(None,true);
    let mut sum=vec![0.;all.len()];
    for ch in 0..frames[0].labels.len() { for (a,b) in sum.iter_mut().zip(render(Some(ch),false)) {*a+=b;} }
    let compare=|name:&str,b:&[f32]| {
        let max=all.iter().zip(b).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);
        let rms=(all.iter().zip(b).map(|(a,b)|f64::from(a-b).powi(2)).sum::<f64>()/all.len() as f64).sqrt();
        println!("{name}: max={max} rms={rms}"); max
    };
    let slow_error=compare("parallel vs general",&slow);
    compare("all vs separate channels before master protection",&sum);
    // Solo outputs already contain the guard's 240-frame delay, but none
    // reaches its ceiling. Reconstruct the sum before that delay and apply
    // the shared master guard once, exactly as the full mix does.
    assert!(sum.iter().all(|v|v.is_finite()));
    let delay=240*2;
    let mut guard=dsp::StereoPeakGuard::new(48000);
    let mut protected=Vec::with_capacity(sum.len());
    for frame in sum[delay..].chunks_exact(2) { protected.extend(guard.process(frame[0],frame[1])); }
    let guard_error=all[..protected.len()].iter().zip(&protected)
        .map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);
    println!("all vs separately rendered sum through master guard: max={guard_error}");
    if let Ok(directory)=std::env::var("SDA_MIX_OUTPUT") {
        std::fs::create_dir_all(&directory).unwrap();
        for (name,pcm) in [("all",&all),("sum",&sum),("general",&slow)] {
            std::fs::write(std::path::Path::new(&directory).join(format!("{name}.f32")),
                pcm.iter().flat_map(|v|v.to_le_bytes()).collect::<Vec<_>>()).unwrap();
            println!("{name} peak={}",pcm.iter().map(|v|v.abs()).fold(0f32,f32::max));
        }
    }
    assert!(slow_error<5e-6,"parallel mixer changes output");
    assert!(guard_error<5e-6,"mixed signal differs beyond the shared peak guard");
}
