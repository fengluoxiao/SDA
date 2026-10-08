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
    if let Ok(db) = std::env::var("SDA_ATMOS_REPLAY_MASTER_DB") {
        e.set_master_preamp_db(db.parse().unwrap()).unwrap();
    }
    if let Ok(db) = std::env::var("SDA_ATMOS_REPLAY_LAYER_DB") {
        e.set_spatial_layer_gain_db(db.parse().unwrap()).unwrap();
    }
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
    // Opt-in offline gain ledger; never used by production playback.
    let gain_ledger_enabled = std::env::var("SDA_ATMOS_REPLAY_LEDGER").as_deref() == Ok("1");
    let mut gain_ledger = Vec::new();
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
        if gain_ledger_enabled {
            let sources: std::collections::BTreeMap<_, _> = e.sources.iter().map(|(id, source)| {
                (id, serde_json::json!({"spatialLayerGain":source.spatial_layer_gain,"spatialLayerTarget":source.spatial_layer_target, "gain":source.gain, "targetGain":source.target_gain,
                    "availability":source.availability, "distanceGain":Engine::distance_gain(source),
                    "position":source.position, "distanceM":source.distance_m,
                    "nearTarget":source.near_target, "continuousMix":source.continuous_mix}))
            }).collect();
            gain_ledger.push(serde_json::json!({"samplePos":e.sample_pos,
                "masterPreampDb":e.master_preamp.target_db(), "masterPreampGain":e.master_preamp.gain(), "outputGain":e.output_gain, "programEnabled":e.program_enabled,
                "programGain":e.program_gain, "comparisonGain":e.comparison_gain,
                "monitorGain":e.cinema.monitor.master_gain(),
                "guardGain":e.peak_guard.diagnostic_gain(),
                "frontCommonEnabled":e.front_common.enabled,
                "spatialBalanceEnabled":e.spatial_balance.enabled,
                "spatialCuesActive":e.active_hrtf_set.as_ref().is_some_and(|set|set.spatial_cues_active()),
                "sources":sources}));
        }
        frames += 1;
    }
    output.flush().unwrap();
    if gain_ledger_enabled {
        std::fs::write(dir.join(format!("{tag}-gain-ledger.json")),
            serde_json::to_vec_pretty(&gain_ledger).unwrap()).unwrap();
    }
    assert_eq!(e.sources.len(), 16);
    assert_eq!(e.sources.values().filter(|s| s.kind == SourceKind::Object).count(), 15);
    assert_eq!(e.sources["bed:0"].bed_label.as_deref(), Some("LFE"));
    assert_eq!(e.sources["bed:0"].lfe_gain, 1.0);
    let report = serde_json::json!({"scope":"offline actual decoded Atmos PCM/metadata replay; not Android output", "hrtf":manifest,
        "wetWeight":wet,"direct":direct,"frames":frames,"samples":e.sample_pos,"events":event_count,"frontCommonGain":e.front_common.gain,"frontCommonActive":e.front_common.center.is_some(),"minimumGuard":minimum_guard,
        "spatialBalanceEnabled":e.spatial_balance.enabled,"spatialBalanceGain":e.spatial_balance.gain,
        "masterPreampDb":e.master_preamp.target_db(),"masterPreampGain":e.master_preamp.gain(),
        "spatialLayerGainDb":e.spatial_layer_gain_db(),
        "sources":e.sources.len(),"nearField":e.near_field.enabled,"room":e.cinema.enabled});
    std::fs::write(dir.join(format!("{tag}.json")), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
/// Generic, content-free probe: engine convolution vs its prepared filter and
/// equivalent one-object/ten-object decomposition at the same physical direction.
#[test]
#[ignore = "requires SDA_TRANSFER_MANIFEST and SDA_TRANSFER_OUT; diagnostic only"]
fn static_direction_transfer_and_object_split() {
    use std::io::Write;
    let manifest = std::path::PathBuf::from(std::env::var("SDA_TRANSFER_MANIFEST").unwrap());
    let out = std::path::PathBuf::from(std::env::var("SDA_TRANSFER_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let data: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    let mut directions: Vec<(f64,f64)> = data["positions"].as_array().unwrap().iter()
        .map(|p|(p["azimuth"].as_f64().unwrap(),p["elevation"].as_f64().unwrap())).collect();
    for elevation in [-45.0,-15.0,15.0,30.0,60.0] {
        for azimuth in [-135.0,-95.0,-55.0,-15.0,15.0,55.0,95.0,135.0] {
            if !directions.contains(&(azimuth,elevation)) { directions.push((azimuth,elevation)); }
        }
    }
    let set = hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap();
    let mut rows = Vec::new();
    const IMPULSE: usize = 12288;
    const FRAMES: usize = 24576;
    const AMPLITUDE: f32 = 0.01;
    for (index,(azimuth,elevation)) in directions.iter().copied().enumerate() {
        let az = azimuth.to_radians(); let el = elevation.to_radians();
        let position = [(-az.sin()*el.cos()) as f32,(az.cos()*el.cos()) as f32,el.sin() as f32];
        let render = |count: usize| {
            let mut e = Engine::new(48000,2);
            e.replace_hrtf(set.clone(),0.0).unwrap();
            e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);
            e.set_program_codec("eac3".into());e.direct_mix=1.0;
            e.output_active=true;e.paused=false;
            for i in 0..count {
                let id=format!("generic-probe-source-{i}");
                let mut source=Source {kind:SourceKind::Object,position,gain:1.0,target_gain:1.0,
                    availability:1.0,availability_target:1.0,..Default::default()};
                let mut pcm=vec![0.0;FRAMES];pcm[IMPULSE]=AMPLITUDE/count as f32;
                source.samples.write(0,0,&pcm);e.sources.insert(id.clone(),source);
                e.route_source_now(&id,0).unwrap();
            }
            let mut pcm=vec![0.0;FRAMES*2];
            e.render_into(&mut pcm[..IMPULSE*2],2);
            let route=e.sources["generic-probe-source-0"].continuous.as_ref().unwrap().effective_route().unwrap();
            let expected=e.active_hrtf_set.as_ref().unwrap().directional_dry_compact(route.0,route.1,route.2,route.3).unwrap();
            e.render_into(&mut pcm[IMPULSE*2..],2);
            assert!(pcm.iter().all(|v|v.is_finite()));
            assert_eq!(e.peak_guard.diagnostic_gain(),1.0);
            (pcm,expected)
        };
        let (single,expected)=render(1);
        let (split,_)=render(10);
        let split_error=single.iter().zip(&split).map(|(a,b)|(*a-*b).abs()).fold(0.0f32,f32::max);
        assert!(split_error<1e-7,"object decomposition changes gain at {azimuth}/{elevation}: {split_error}");
        let maximum = |xs:&[f32]| xs.iter().enumerate().max_by(|a,b|a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
        let left:Vec<f32>=single.chunks_exact(2).map(|f|f[0]/AMPLITUDE).collect();
        let right:Vec<f32>=single.chunks_exact(2).map(|f|f[1]/AMPLITUDE).collect();
        let delay=maximum(&left) as isize-maximum(&expected.0) as isize-IMPULSE as isize;
        assert!(delay>=0 && delay<4096);
        let mut error=0.0f32;let mut energy=0.0f64;let mut reference_energy=0.0f64;
        for (actual,reference) in [(&left,&expected.0),(&right,&expected.1)] {
            for (n,sample) in actual.iter().enumerate() {
                let offset=n as isize-IMPULSE as isize-delay;
                let target=if offset>=0 {reference.get(offset as usize).copied().unwrap_or(0.0)} else {0.0};
                error=error.max((*sample-target).abs());
                energy+=f64::from(*sample).powi(2);reference_energy+=f64::from(target).powi(2);
            }
        }
        assert!(error<1e-4,"engine/filter transfer mismatch at {azimuth}/{elevation}: {error}");
        let file=out.join(format!("direction-{index:03}.f32"));
        let mut writer=std::io::BufWriter::new(std::fs::File::create(&file).unwrap());
        for sample in &single {writer.write_all(&sample.to_le_bytes()).unwrap();}
        writer.flush().unwrap();
        rows.push(serde_json::json!({"index":index,"azimuth":azimuth,"elevation":elevation,
            "position":position,"impulseSample":IMPULSE,"impulseAmplitude":AMPLITUDE,
            "engineDelaySamples":delay,"splitMaxAbsError":split_error,
            "engineVsPreparedFilterMaxAbsError":error,"engineEnergy":energy,
            "preparedFilterEnergy":reference_energy,"file":file}));
    }
    let report=serde_json::json!({"scope":"Synthetic impulses, fixed measured and non-grid directions; cues-disabled asset manifest; one source vs ten co-located copies. No song-specific data. Prepared filter is implementation reference, not independent acoustic truth.","directions":rows});
    std::fs::write(out.join("report.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
/// Measure moving-filter handoffs against instantaneous prepared-kernel tone
/// transfer. The reference is an implementation model, not measured moving sound.
#[test]
#[ignore = "requires SDA_TRANSFER_MANIFEST and SDA_TRANSFER_OUT; diagnostic only"]
fn moving_direction_tone_transfer() {
    use std::io::Write;
    let manifest=std::path::PathBuf::from(std::env::var("SDA_TRANSFER_MANIFEST").unwrap());
    let out=std::path::PathBuf::from(std::env::var("SDA_TRANSFER_OUT").unwrap()).join("motion");
    std::fs::create_dir_all(&out).unwrap();
    let set=hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap();
    let partition=convolution::DEFAULT_PARTITION;
    // Derive whole-engine latency from the preceding impulse probe, not a
    // guessed convolver-only delay (the output chain has additional latency).
    let static_report:serde_json::Value=serde_json::from_slice(&std::fs::read(out.parent().unwrap().join("report.json")).unwrap()).unwrap();
    let delay=static_report["directions"][0]["engineDelaySamples"].as_u64().unwrap() as usize;
    assert!(static_report["directions"].as_array().unwrap().iter().all(|row|row["engineDelaySamples"].as_u64()==Some(delay as u64)));
    let frames=48000*10;let amplitude=0.01f32;
    let mut report=Vec::new();
    for frequency in [500.0f64,1500.0,3000.0,6000.0] {
        let omega=2.0*std::f64::consts::PI*frequency/48000.0;
        let mut e=Engine::new(48000,2);e.replace_hrtf(set.clone(),0.0).unwrap();
        e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);e.set_program_codec("eac3".into());
        e.direct_mix=1.0;e.output_active=true;e.paused=false;
        let mut source=Source{kind:SourceKind::Object,position:[-1.0,1.0,0.0],gain:1.0,target_gain:1.0,
            availability:1.0,availability_target:1.0,..Default::default()};
        let input:Vec<f32>=(0..frames+delay).map(|n|(omega*n as f64).sin() as f32*amplitude).collect();
        source.samples.write(0,0,&input);e.sources.insert("synthetic-moving-source".into(),source);
        e.route_source_now("synthetic-moving-source",0).unwrap();
        let mut actual=vec![0.0f32;(frames+delay)*2];
        let mut expected=vec![0.0f32;actual.len()];let mut rows=Vec::new();
        for start in (0..frames).step_by(partition) {
            let time=start as f64/48000.0;
            let phase=if time<2.0 {0.0} else {(time-2.0)*2.0*std::f64::consts::PI/2.0};
            let azimuth=75.0-30.0*phase.cos();let elevation=22.5*(1.0-phase.cos());
            let (az,el)=(azimuth.to_radians(),elevation.to_radians());
            let position=[(-az.sin()*el.cos()) as f32,(az.cos()*el.cos()) as f32,el.sin() as f32];
            e.sources.get_mut("synthetic-moving-source").unwrap().position=position;
            e.route_source_now("synthetic-moving-source",0).unwrap();
            let source=&e.sources["synthetic-moving-source"];
            let direction=directional::Direction {diffuse:0.0,horizontal_only:false,position,
                head:None,width:0.0,height:0.0,depth:0.0};
            let kernel=e.active_hrtf_set.as_ref().unwrap().directional_dry_compact(
                direction,e.layout,source.bus_gains,[0.0;vbap::MAX_BUS_COUNT]).unwrap();
            let mut magnitudes=[0.0f64;2];
            for (ear,ir) in [&kernel.0,&kernel.1].into_iter().enumerate() {
                let real=ir.iter().enumerate().map(|(k,h)|f64::from(*h)*(omega*k as f64).cos()).sum::<f64>();
                let imaginary=-ir.iter().enumerate().map(|(k,h)|f64::from(*h)*(omega*k as f64).sin()).sum::<f64>();
                magnitudes[ear]=real.hypot(imaginary);
                for n in start..(start+partition).min(frames) {
                    let phase=omega*n as f64;
                    expected[(n+delay)*2+ear]=(f64::from(amplitude)*(real*phase.sin()+imaginary*phase.cos())) as f32;
                }
            }
            let n=(frames-start).min(partition);
            e.render_into(&mut actual[start*2..(start+n)*2],2);
            assert_eq!(e.peak_guard.diagnostic_gain(),1.0);
            rows.push(serde_json::json!({"samplePos":start,"azimuth":azimuth,"elevation":elevation,"position":position,"preparedMagnitude":magnitudes}));
        }
        e.render_into(&mut actual[frames*2..],2);
        for (tag,pcm) in [("actual",&actual),("instantaneous",&expected)] {
            let path=out.join(format!("{frequency:.0}-{tag}.f32"));
            let mut writer=std::io::BufWriter::new(std::fs::File::create(path).unwrap());
            for value in pcm {assert!(value.is_finite());writer.write_all(&value.to_le_bytes()).unwrap();}
            writer.flush().unwrap();
        }
        report.push(serde_json::json!({"frequency":frequency,"amplitude":amplitude,"latencySamples":delay,"frames":frames,"routeSamples":rows}));
    }
    std::fs::write(out.join("report.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"Synthetic same-level single tones; 2s static, then 2s-period front-to-side/upper sweeps. No authored PCM/metadata altered. Instantaneous kernel reference ignores physical Doppler and is not acoustic truth.","frequencies":report})).unwrap()).unwrap();
}
