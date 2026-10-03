//! Offline pre-device PCM probe: identical executable logic on Windows and Android.
use std::{sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}, io::Write};
use sda_native::{MobileEngine, EngineConfig};
use sda_native_renderer::{AudioOutput, RuntimeTelemetry, stereo_fifo::StereoFifo, render_command::RenderCommandQueue};

#[derive(Default)]
struct Capture { samples: Mutex<Vec<f32>>, done: AtomicBool }
impl AudioOutput for Capture {
    fn run(self: Arc<Self>, fifo: Arc<StereoFifo>, telemetry: Arc<RuntimeTelemetry>, _: Arc<RenderCommandQueue>) {
        let mut block = [0f32; 2048];
        while !self.done.load(Ordering::Acquire) {
            fifo.apply_flush_from_consumer();
            let n = fifo.pop_into_f32(&mut block, 2);
            if n > 0 {
                self.samples.lock().unwrap().extend_from_slice(&block[..n*2]);
                telemetry.callback_consumed_sample_pos.fetch_add(n as u64, Ordering::Release);
            } else { std::thread::sleep(Duration::from_millis(1)); }
        }
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(args.len() == 5 || args.len() == 6, "pcm_probe INPUT.ec3 HRTF.json OUTPUT.f32 SECONDS [desktop-feeder|android-startup]");
    let limit = args[4].parse::<u64>().unwrap() * 48000;
    if args.get(5).map(String::as_str) == Some("dump-frames") {
        let start = std::env::var("SDA_PROBE_START_SECONDS").ok().map(|v| v.parse::<u64>().unwrap() * 48000 / 1536 * 1536).unwrap_or(0);
        let mut decoder = sda_core::StreamingDecoder::new("auto").unwrap();
        let mut output = std::io::BufWriter::new(std::fs::File::create(&args[3]).unwrap());
        for chunk in std::fs::read(&args[1]).unwrap().chunks(24 * 1024) {
            decoder.push(chunk).unwrap();
            assert!(decoder.drain_errors().is_empty());
            while let Some(mut frame) = decoder.next_frame() {
                let end = frame.sample_pos + frame.channels[0].len() as u64;
                if frame.sample_pos < start { continue; }
                frame.sample_pos -= start;
                for event in &mut frame.events { event.sample_pos = event.sample_pos.saturating_sub(start); }
                serde_json::to_writer(&mut output, &serde_json::json!({"start":frame.sample_pos,"labels":frame.labels,"channels":frame.channels,"events":frame.events,"objectChannels":frame.object_channels})).unwrap();
                writeln!(output).unwrap();
                if end >= start + limit { eprintln!("window origin={start} frames={}", end-start); return; }
            }
        }
        panic!("input shorter than requested probe");
    }
    if args.get(5).map(String::as_str) == Some("desktop-feeder") {
        desktop_feeder(&args, limit);
        return;
    }
    let mut engine = MobileEngine::new(EngineConfig { direct_object_hrtf: true, directional_hrtf: true, ..EngineConfig::default() }, None).unwrap();
    engine.load_hrtf(&args[2]).unwrap();
    if std::env::var_os("SDA_PROBE_NEAR_FIELD").is_some() { engine.set_near_field(true, 1.0).unwrap(); }
    if let Ok(path) = std::env::var("SDA_PROBE_ROOM") { engine.set_room(&path).unwrap(); }
    let sink = Arc::new(Capture::default());
    engine.start(sink.clone()).unwrap();
    if args.get(5).map(String::as_str) == Some("android-startup") {
        engine.set_head_yaw_degrees(0.0).unwrap();
        engine.set_volume(1.0).unwrap();
    }
    let bytes = std::fs::read(&args[1]).unwrap();
    for chunk in bytes.chunks(24*1024) {
        let status = engine.feed(chunk).unwrap();
        engine.object_snapshot();
        assert!(status.errors.is_empty(), "decode errors: {:?}", status.errors);
        while engine.decoded_sample_pos().saturating_sub(engine.playback_status().consumed_sample_pos) > 48000 {
            std::thread::sleep(Duration::from_millis(2));
        }
        if engine.decoded_sample_pos() >= limit { break; }
    }
    engine.finish().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while engine.playback_status().consumed_sample_pos < engine.decoded_sample_pos() {
        assert!(Instant::now() < deadline, "render drain timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
    sink.done.store(true, Ordering::Release);
    let samples = sink.samples.lock().unwrap();
    let mut file = std::io::BufWriter::new(std::fs::File::create(&args[3]).unwrap());
    for sample in samples.iter().take(limit as usize * 2) { file.write_all(&sample.to_le_bytes()).unwrap(); }
    file.flush().unwrap();
    eprintln!("probe frames={} directions={}", samples.len()/2, engine.playback_status().hrtf_directions);
}

// Diagnostic reproduction of player.ts source mapping and decoder.worker.ts
// event compaction. Uses the actual native renderer, but does not run Electron,
// WASM decoding, transport batching, persisted settings, or a device output.
fn desktop_feeder(args: &[String], limit: u64) {
    use sda_native_renderer::{Engine, Command, NativeObjectEvent, render_command::RenderCommand};
    use std::collections::{HashMap, HashSet};
    let lead = std::env::var("SDA_PROBE_LEAD_FRAMES").ok().map(|s| s.parse::<u64>().unwrap()).unwrap_or(48000);
    let mut renderer = Engine::new(48000, 2);
    renderer.replace_hrtf(sda_native_renderer::hrtf::NativeHrtfSet::load_calibrated(std::path::Path::new(&args[2])).unwrap(), 0.04).unwrap();
    if std::env::var_os("SDA_PROBE_NEAR_FIELD").is_some() {
        renderer.configure_near_field(sda_native_renderer::near_field::Settings { enabled: true, metres_per_unit: 1.0 }).unwrap();
    }
    if let Ok(path) = std::env::var("SDA_PROBE_ROOM") {
        let room = sda_native_renderer::cinema::RoomProfile::load(&path).unwrap();
        renderer.configure_room(sda_native_renderer::cinema::Settings { enabled: true, ..Default::default() }, Some(Arc::new(room))).unwrap();
    }
    renderer.set_output_active(true);
    let commands = Arc::new(RenderCommandQueue::new(4096));
    let fifo = Arc::new(StereoFifo::new(sda_native_renderer::STEREO_FIFO_CAPACITY_FRAMES));
    let telemetry = Arc::new(RuntimeTelemetry::default());
    let sink = Arc::new(Capture::default());
    let push = |command| assert!(commands.push(command).is_ok(), "probe queue full");
    for command in [Command::Pause { paused: false }, Command::SetLayout { layout: "7.1.4".into() }, Command::SetDirectionalHrtf { enabled: true }, Command::SetObjectHrtf { enabled: true }, Command::SetProgramCodec { codec: "eac3".into() }] {
        push(RenderCommand::Command(command));
    }
    sda_native_renderer::spawn_render_worker(renderer, commands.clone(), fifo.clone(), telemetry.clone());
    let (output, output_telemetry, output_commands) = (sink.clone(), telemetry.clone(), commands.clone());
    std::thread::spawn(move || output.run(fifo, output_telemetry, output_commands));
    let mut decoder = sda_core::StreamingDecoder::new("auto").unwrap();
    let mut last: HashMap<u32, sda_core::ObjectEvent> = HashMap::new();
    let mut objects = HashSet::new();
    let mut acknowledged_sources = HashSet::new();
    let mut end = 0;
    let mut dropped = 0;
    let mut total = 0;
    let bytes = std::fs::read(&args[1]).unwrap();
    let replay = args[1].ends_with(".jsonl");
    let mut replay_frames = std::collections::VecDeque::new();
    if replay {
        for line in std::str::from_utf8(&bytes).unwrap().lines() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            // Decode ObjectEvent through explicit fields because core exposes
            // serialization only. Keep this adapter diagnostic-only.
            let events = value["events"].as_array().unwrap().iter().map(|e| sda_core::ObjectEvent {
                diffuse: e["diffuse"].as_f64().unwrap_or(0.0),
                id: e["id"].as_u64().unwrap() as u32, sample_pos: e["samplePos"].as_u64().unwrap(),
                has_pos: e["hasPos"].as_bool().unwrap(), pos: serde_json::from_value(e["pos"].clone()).unwrap(),
                gain_db: e["gainDb"].as_f64().unwrap(), size: serde_json::from_value(e["size"].clone()).unwrap(),
                anchor: e["anchor"].as_str().unwrap_or("room").into(), distance_m: e["distanceM"].as_f64(),
                distance_infinite: e["distanceInfinite"].as_bool().unwrap_or(false), screen_factor: e["screenFactor"].as_f64(),
                depth_factor: e["depthFactor"].as_f64(), ramp_duration: e["rampDuration"].as_u64().unwrap_or(128) as u32,
            }).collect();
            replay_frames.push_back(sda_core::FrameData {
                codec: "eac3", sample_rate: 48000, sample_pos: value["start"].as_u64().unwrap(),
                channels: serde_json::from_value(value["channels"].clone()).unwrap(),
                labels: serde_json::from_value(value["labels"].clone()).unwrap(),
                raw_bed_labels: Vec::new(), events,
                object_channels: value["objectChannels"].as_array().unwrap().iter().map(|d| sda_core::ObjectChannelDecl {id:d["id"].as_u64().unwrap() as u32,channel:d["channel"].as_u64().unwrap() as u32}).collect(),
                program_loudness: None, ramp_duration: 128,
            });
        }
    }
    let input = if replay { &[0u8][..] } else { &bytes };
    'input: for chunk in input.chunks(24 * 1024) {
        if !replay { decoder.push(chunk).unwrap(); }
        assert!(decoder.drain_errors().is_empty());
        while let Some(frame) = if replay { replay_frames.pop_front() } else { decoder.next_frame() } {
            assert_eq!(frame.sample_rate, 48000);
            let mut mapping: HashMap<usize, u32> = frame.object_channels.iter().map(|d| (d.channel as usize, d.id)).collect();
            if mapping.is_empty() {
                for (ch, label) in frame.labels.iter().enumerate() {
                    if let Some(id) = label.strip_prefix("Obj_") { mapping.insert(ch, id.parse().unwrap()); }
                }
            }
            let next: HashSet<u32> = mapping.values().copied().collect();
            for id in objects.difference(&next) {
                push(RenderCommand::Command(Command::RemoveSource { id: format!("obj:{id}"), at: frame.sample_pos }));
            }
            objects = next;
            last.retain(|id, _| objects.contains(id));
            let mut entries = Vec::new();
            for (ch, pcm) in frame.channels.into_iter().enumerate() {
                let id = mapping.get(&ch).map(|id| format!("obj:{id}")).unwrap_or_else(|| format!("bed:{ch}"));
                let bed_label = (!mapping.contains_key(&ch)).then(|| frame.labels[ch].clone());
                if acknowledged_sources.insert((id.clone(), bed_label.clone())) {
                    push(RenderCommand::Command(Command::AddSource { id: id.clone(), at: Some(frame.sample_pos), bed_label }));
                }
                entries.push((id, pcm));
            }
            let mut events = Vec::new();
            for event in frame.events {
                total += 1;
                let coalesce = last.get(&event.id).is_some_and(|old| {
                    let mut a = serde_json::to_value(old).unwrap();
                    let mut b = serde_json::to_value(&event).unwrap();
                    for key in ["samplePos", "rampDuration"] { a.as_object_mut().unwrap().remove(key); b.as_object_mut().unwrap().remove(key); }
                    a == b && old.sample_pos + u64::from(if old.ramp_duration == 0 {128} else {old.ramp_duration}) <= event.sample_pos
                });
                if coalesce { dropped += 1; continue; }
                events.push(serde_json::from_str::<NativeObjectEvent>(&serde_json::to_string(&event).unwrap()).unwrap());
                last.insert(event.id, event);
            }
            end = frame.sample_pos + entries[0].1.len() as u64;
            push(RenderCommand::PcmFrame { start: frame.sample_pos, entries, events });
            let deadline = Instant::now() + Duration::from_secs(60);
            while end.saturating_sub(telemetry.callback_consumed_sample_pos.load(Ordering::Acquire)) > lead {
                assert!(Instant::now() < deadline, "probe stalled");
                std::thread::sleep(Duration::from_millis(2));
            }
            if end >= limit { break 'input; }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    while telemetry.callback_consumed_sample_pos.load(Ordering::Acquire) < end {
        assert!(Instant::now() < deadline, "probe drain stalled");
        std::thread::sleep(Duration::from_millis(2));
    }
    sink.done.store(true, Ordering::Release);
    let samples = sink.samples.lock().unwrap();
    assert!(samples.len() >= limit as usize * 2);
    let mut file = std::fs::File::create(&args[3]).unwrap();
    for sample in samples.iter().take(limit as usize * 2) { file.write_all(&sample.to_le_bytes()).unwrap(); }
    eprintln!("desktop-feeder frames={} events={total} coalesced={dropped}", samples.len()/2);
}
