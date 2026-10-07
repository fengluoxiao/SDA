//! Offline synthetic front-pair transfer audit; never changes production DSP.
use super::*;

#[test]
#[ignore = "requires SDA_FRONT_PAIR_DIR; exports synthetic impulse responses only"]
fn export_atmos_front_pair_transfer() {
    let out = std::path::PathBuf::from(std::env::var("SDA_FRONT_PAIR_DIR").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let mut report = Vec::new();
    for (set_name, direct, directional) in [
        ("hrtf", false, false), ("hrtf", true, false), ("hrtf", true, true),
        ("hrtf-dense", false, false), ("hrtf-dense", true, true),
        ("hrtf-dense-raw", true, true),
    ] {
        let wet = if set_name == "hrtf-dense-raw" { 0.0 } else { 0.04 };
        let tag = format!("{set_name}-d{}-c{}", direct as u8, directional as u8);
        let render = |xs: &[f32]| {
            let mut engine = Engine::new(48000, 2);
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../web/public").join(set_name).join("hrtf-set.json");
            engine.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(), wet).unwrap();
            engine.set_direct_objects(direct).unwrap();
            engine.set_directional_hrtf(directional);
            engine.set_program_codec("eac3".into());
            engine.output_active = true;
            engine.paused = false;
            engine.direct_mix = if direct { 1.0 } else { 0.0 };
            for (index, x) in xs.iter().enumerate() {
                let id = format!("obj:{index}");
                let mut source = Source { kind: SourceKind::Object, position: [*x, 1.0, 0.0],
                    gain: 1.0, target_gain: 1.0, availability: 1.0, availability_target: 1.0,
                    ..Default::default() };
                let mut pcm = vec![0.0; 144384];
                pcm[96000] = 0.01;
                source.samples.write(0, 0, &pcm);
                engine.sources.insert(id.clone(), source);
                engine.route_source_now(&id, 0).unwrap();
            }
            let mut pcm = vec![0.0; 144384 * 2];
            engine.render_into(&mut pcm, 2);
            assert!(pcm.iter().all(|v| v.is_finite()));
            assert!(pcm.iter().any(|v| v.abs() > 1e-6));
            let guard = engine.peak_guard.diagnostic_gain();
            assert!((guard - 1.0).abs() < 1e-6, "synthetic impulse must not activate guard");
            pcm
        };
        let left = render(&[-1.0]);
        let right = render(&[1.0]);
        let both = render(&[-1.0, 1.0]);
        let error = both.iter().zip(left.iter().zip(&right))
            .map(|(a,(b,c))| (a-b-c).abs()).fold(0.0_f32, f32::max);
        assert!(error < 1e-6, "front-pair mixing is nonlinear: {tag} {error}");
        let energy: f64 = left.iter().map(|v| f64::from(*v).powi(2)).sum();
        let mirror: f64 = left.chunks_exact(2).zip(right.chunks_exact(2))
            .map(|(a,b)| f64::from(a[0]-b[1]).powi(2)+f64::from(a[1]-b[0]).powi(2)).sum();
        if set_name != "hrtf-dense-raw" {
            assert!((mirror/energy).sqrt() < 1e-3,
                "calibrated front pair lost bilateral symmetry: {tag}");
        }
        for (name, samples) in [("left", &left), ("right", &right), ("both", &both)] {
            let mut file = std::io::BufWriter::new(std::fs::File::create(out.join(format!("{tag}-{name}.f32"))).unwrap());
            for value in samples { file.write_all(&value.to_le_bytes()).unwrap(); }
        }
        report.push(serde_json::json!({"tag":tag,"set":set_name,"direct":direct,
            "directional":directional,"hrtfWetWeight":wet,"inputPositionLeft":[-1.0,1.0,0.0],
            "inputPositionRight":[1.0,1.0,0.0],"impulseSample":96000,
            "impulseAmplitude":0.01,"frames":144384,"mixMaxError":error,
            "mirrorRelativeRms":(mirror/energy).sqrt()}));
    }
    std::fs::write(out.join("front-pair-transfer.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}

/// Replays the actual decoded Atmos object PCM and timestamped metadata.
/// Unlike the older MPEG-H song probe this preserves LFE, object ids and motion.
#[test]
#[ignore = "requires SDA_ATMOS_REPLAY_DIR/HRTF/TAG; local copyrighted fixture only"]
fn replay_atmos_object_stream() {
    use std::io::{BufRead, Read, Write};
    let dir = std::path::PathBuf::from(std::env::var("SDA_ATMOS_REPLAY_DIR").unwrap());
    let manifest = std::env::var("SDA_ATMOS_REPLAY_HRTF").unwrap();
    let tag = std::env::var("SDA_ATMOS_REPLAY_TAG").unwrap();
    let wet: f32 = std::env::var("SDA_ATMOS_REPLAY_WET").unwrap().parse().unwrap();
    assert!((0.0..=1.0).contains(&wet));
    let seconds: u64 = std::env::var("SDA_ATMOS_REPLAY_SECONDS").unwrap().parse().unwrap();
    let direct = std::env::var("SDA_ATMOS_REPLAY_DIRECT").as_deref() != Ok("0");
    let mut e = Engine::new(48000, 2);
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(std::path::Path::new(&manifest)).unwrap(), wet).unwrap();
    e.front_common.enabled = std::env::var("SDA_ATMOS_REPLAY_FRONT_COMMON").as_deref() != Ok("0");
    if let Ok(balance) = std::env::var("SDA_ATMOS_REPLAY_SPATIAL_BALANCE") {
        e.set_spatial_balance(balance == "1");
    }
    e.set_program_codec("eac3".into());
    e.set_direct_objects(direct).unwrap();
    e.set_directional_hrtf(direct);
    e.direct_mix = if direct { 1.0 } else { 0.0 };
    e.output_active = true;
    e.paused = false;
    let fifo = stereo_fifo::StereoFifo::new(4096);
    let telemetry = RuntimeTelemetry::default();
    let mut input = std::io::BufReader::new(std::fs::File::open(dir.join("sources.pcm")).unwrap());
    let mut output = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{tag}.f32"))).unwrap());
    let mut labels = None;
    let mut frames = 0;
    let mut event_count = 0;
    let mut targets = std::collections::BTreeMap::<u32, NativeObjectEvent>::new();
    let mut minimum_guard = 1.0_f32;
    for line in std::io::BufReader::new(std::fs::File::open(dir.join("frames.jsonl")).unwrap()).lines() {
        let f: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let start = f["start"].as_u64().unwrap();
        if start >= seconds * 48000 { break; }
        assert_eq!(start, e.sample_pos);
        let n = f["samples"].as_u64().unwrap() as usize;
        if let Some(previous) = &labels { assert_eq!(previous, &f["labels"], "fixture channel layout changed"); }
        else { labels = Some(f["labels"].clone()); }
        let mut entries = Vec::new();
        for (channel, label) in f["labels"].as_array().unwrap().iter().enumerate() {
            let label = label.as_str().unwrap();
            let declared = f["objectChannels"].as_array().unwrap().iter()
                .find(|o| o["channel"].as_u64() == Some(channel as u64));
            let object = declared.map(|o| o["id"].as_u64().unwrap() as u32)
                .or_else(|| label.strip_prefix("Obj_").and_then(|s| s.parse().ok()));
            let id = object.map_or_else(|| format!("bed:{channel}"), |o| format!("obj:{o}"));
            if frames == 0 {
                let mut source = Source { kind: if object.is_some() { SourceKind::Object } else { SourceKind::Bed },
                    bed_label: object.is_none().then(|| label.to_string()), object_id: object,
                    gain: 1.0, target_gain: 1.0, availability: 1.0, availability_target: 1.0,
                    ..Default::default() };
                if object.is_none() { Engine::set_source_route(&mut source, crate::bed_route(label, &e.vbap), 0); }
                e.sources.insert(id.clone(), source);
                e.route_source_now(&id, 0).unwrap();
            }
            let mut bytes = vec![0u8; n * 4];
            input.read_exact(&mut bytes).unwrap();
            let pcm = bytes.chunks_exact(4).map(|v| f32::from_le_bytes(v.try_into().unwrap())).collect();
            entries.push((id, pcm));
        }
        let raw_events: Vec<NativeObjectEvent> = serde_json::from_value(f["events"].clone()).unwrap();
        let mut events = Vec::new();
        for event in raw_events {
            let redundant = targets.get(&event.id).is_some_and(|previous| {
                let ramp = if previous.ramp_duration == 0 { 128 } else { previous.ramp_duration };
                let mut a = previous.clone();
                let mut b = event.clone();
                a.sample_pos = 0; b.sample_pos = 0;
                a.ramp_duration = 0; b.ramp_duration = 0;
                a == b && previous.sample_pos.saturating_add(u64::from(ramp)) <= event.sample_pos
            });
            if !redundant { targets.insert(event.id, event.clone()); events.push(event); }
        }
        event_count += events.len();
        let (reply, received) = std::sync::mpsc::channel();
        assert!(protocol::apply_render_command(&mut e, render_command::RenderCommand::PcmFrameWithAck { start, entries, events, reply }, &fifo, &telemetry));
        assert!(received.recv().unwrap(), "PCM transaction rejected at {start}");
        let mut pcm = vec![0.0; n * 2];
        e.render_into(&mut pcm, 2);
        assert!(pcm.iter().all(|v| v.is_finite()));
        for v in pcm { output.write_all(&v.to_le_bytes()).unwrap(); }
        minimum_guard = minimum_guard.min(e.peak_guard.diagnostic_gain());
        frames += 1;
    }
    output.flush().unwrap();
    assert_eq!(e.sources.len(), 16);
    assert_eq!(e.sources.values().filter(|s| s.kind == SourceKind::Object).count(), 15);
    assert_eq!(e.sources["bed:0"].bed_label.as_deref(), Some("LFE"));
    assert_eq!(e.sources["bed:0"].lfe_gain, 1.0);
    let report = serde_json::json!({"scope":"offline actual decoded Atmos PCM/metadata replay; not Android output", "hrtf":manifest,
        "wetWeight":wet,"direct":direct,"frames":frames,"samples":e.sample_pos,"events":event_count,"frontCommonGain":e.front_common.gain,"frontCommonActive":e.front_common.center.is_some(),"minimumGuard":minimum_guard,
        "spatialBalanceEnabled":e.spatial_balance.enabled,"spatialBalanceGain":e.spatial_balance.gain,
        "sources":e.sources.len(),"nearField":e.near_field.enabled,"room":e.cinema.enabled});
    std::fs::write(dir.join(format!("{tag}.json")), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
