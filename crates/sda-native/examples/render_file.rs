use std::{path::PathBuf, sync::{Arc, atomic::Ordering}, time::{Duration, Instant}};
use sda_native::{EngineConfig, MobileEngine};
use sda_native_renderer::WavDumpOutput;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: render_file <raw-codec-file> <hrtf-set.json> <output.wav>".into());
    }
    let mut engine = MobileEngine::new(EngineConfig::default(), None)?;
    engine.load_hrtf(&args[2])?;
    let path = PathBuf::from(&args[3]);
    if path.exists() { return Err("output already exists".into()); }
    let sink = Arc::new(WavDumpOutput::new(&path, 48000));
    let stop = sink.stop.clone();
    engine.start(sink)?;
    let input = std::fs::read(&args[1])?;
    let deadline = Instant::now() + Duration::from_secs(90);
    for chunk in input.chunks(4096) {
        loop {
            if Instant::now() > deadline { return Err("timed out feeding renderer".into()); }
            let status = engine.playback_status();
            if status.decoded_sample_pos.saturating_sub(status.consumed_sample_pos) <= 96000 { break; }
            std::thread::sleep(Duration::from_millis(1));
        }
        let status = engine.feed(chunk)?;
        if !status.errors.is_empty() { return Err(format!("decode errors: {:?}", status.errors).into()); }
    }
    engine.finish()?;
    let expected = engine.decoded_sample_pos();
    while engine.playback_status().consumed_sample_pos < expected {
        if Instant::now() > deadline { return Err("timed out draining renderer".into()); }
        std::thread::sleep(Duration::from_millis(2));
    }
    stop.store(true, Ordering::Release);
    loop {
        if Instant::now() > deadline { return Err("timed out saving WAV".into()); }
        if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() == 44 + expected * 4) { break; }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = engine.playback_status();
    assert_eq!(status.consumed_sample_pos, expected);
    println!("render_file decoded_frames={expected} consumed_frames={} wav_bytes={} duration_seconds={:.3}", status.consumed_sample_pos, std::fs::metadata(&path)?.len(), expected as f64 / 48000.0);
    engine.stop();
    Ok(())
}
