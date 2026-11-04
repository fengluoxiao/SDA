//! Host regression for Android's feed/seek contract, matching the working iOS
//! policy: user pause gates decoding, renderer startup pause does not.
//! Only test PCM is held transiently; no track render/cache is written.
use sda_native::{EngineConfig, MobileEngine};
use sda_native_renderer::{AudioOutput, RuntimeTelemetry, render_command::RenderCommandQueue, stereo_fifo::StereoFifo};
use std::{path::PathBuf, sync::{Arc, OnceLock, atomic::Ordering}, time::{Duration, Instant}};

#[derive(Default)]
struct Output { state: OnceLock<(Arc<StereoFifo>, Arc<RuntimeTelemetry>)> }
impl AudioOutput for Output {
    fn run(self: Arc<Self>, fifo: Arc<StereoFifo>, telemetry: Arc<RuntimeTelemetry>, _: Arc<RenderCommandQueue>) {
        self.state.set((fifo, telemetry.clone())).ok();
        while !telemetry.shutdown_requested.load(Ordering::Acquire) { std::thread::sleep(Duration::from_millis(1)); }
    }
}
impl Output {
    fn callback(&self) -> (u64, usize, bool) {
        let (fifo, t) = self.state.get().unwrap();
        fifo.apply_flush_from_consumer();
        let before = t.callback_consumed_sample_pos.load(Ordering::Acquire);
        let enabled = t.callback_output_enabled.load(Ordering::Acquire)
            && !t.output_device_recovering.load(Ordering::Acquire);
        let mut pcm = [0f32; 1024];
        let frames = if enabled { fifo.pop_into_f32(&mut pcm, 2) } else { 0 };
        sda_native_renderer::record_callback(t, Instant::now(), 512, frames, enabled);
        (before, frames, pcm[..frames * 2].iter().any(|s| s.abs() > 1e-6))
    }
}
fn wait_paused(engine: &MobileEngine, output: &Output) {
    let until = Instant::now() + Duration::from_secs(3);
    while !engine.playback_status().paused {
        output.callback();
        assert!(Instant::now() < until, "SeekReset never reached the renderer");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn open(kind: &str, path: &str) -> (MobileEngine, Arc<Output>) {
    let mut e = MobileEngine::new(EngineConfig { direct_object_hrtf: true, directional_hrtf: true, ..Default::default() }, None).unwrap();
    e.load_hrtf(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json")).unwrap();
    let output = Arc::new(Output::default()); e.start(output.clone()).unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while output.state.get().is_none() { assert!(Instant::now() < until); std::thread::yield_now(); }
    match kind { "alac" => { e.open_alac(path).unwrap(); }, "mp3" => { e.open_mp3(path).unwrap(); }, _ => { e.open_packet_file(path, kind).unwrap(); } }
    e.set_volume_balance(false).unwrap();
    (e, output)
}
fn audible(e: &mut MobileEngine, output: &Output, kind: &str, upmix: bool, target: u64, required: u64) {
    let until = Instant::now() + Duration::from_secs(5);
    let mut eof = false; let mut total = 0u64; let mut sound = false;
    while Instant::now() < until {
        let status = e.playback_status();
        // This is iOS/Android host intent, deliberately independent of the
        // renderer's paused=true telemetry during SeekReset/startup.
        if !eof && status.decoded_sample_pos < status.consumed_sample_pos + 16384 {
            let (_, end) = match kind { "alac" => e.pull_alac(4096, upmix), "mp3" => e.pull_mp3(4096), _ => e.pull_packet_file(4096) }.unwrap();
            eof = end;
            if eof { e.finish().unwrap(); }
        }
        let (before, n, nonzero) = output.callback();
        if n > 0 { assert!(before >= target, "preroll or old PCM reached the output"); total += n as u64; sound |= nonzero; }
        if total >= required && sound {
            assert!(!e.playback_status().paused, "StartAt must publish resumed output");
            assert!(e.playback_status().consumed_sample_pos >= target + required);
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("{kind} seek at {target}: no advancing audible output; total={total}, sound={sound}, paused={}", e.playback_status().paused);
}
fn exercise(kind: &str, path: &str, upmix: bool, targets: [u64; 3]) {
    let (mut e, output) = open(kind, path);
    for ms in targets {
        e.set_paused(true).unwrap(); e.seek(ms).unwrap();
        wait_paused(&e, &output);
        let clock = e.playback_status().consumed_sample_pos;
        for _ in 0..10 { assert_eq!(output.callback().1, 0); }
        assert_eq!(clock, ms * 48);
        e.set_paused(false).unwrap();
        // This is the deadlock prerequisite: resume before startup PCM does
        // not clear renderer pause. A host that waits on it can never feed.
        assert!(e.playback_status().paused);
        audible(&mut e, &output, kind, upmix, ms * 48, 512);
    }
    // Latest seek replaces pending render commands without a user pause/resume.
    for ms in [targets[0], targets[1], targets[0]] { e.seek(ms).unwrap(); }
    audible(&mut e, &output, kind, upmix, targets[0] * 48, 512);
    e.stop();
}
#[test]
fn alac_stereo_and_upmix_seek_resume_while_renderer_is_paused() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/alac/tone-48000-2ch.m4a");
    for upmix in [false, true] { exercise("alac", path.to_str().unwrap(), upmix, [20, 60, 120]); }
}
#[test]
fn mp3_seek_and_near_eof_seek_resume_while_renderer_is_paused() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/alac/tone-44100-2ch.mp3");
    exercise("mp3", path.to_str().unwrap(), false, [20, 60, 120]);
}
#[test]
fn local_immersive_sources_seek_with_the_same_host_pause_policy() {
    for (key, kind) in [("SDA_ATMOS_SEEK_FILE", "eac3"), ("SDA_AV3A_SEEK_FILE", "av3a"), ("SDA_MHAS_SEEK_FILE", "mpegh")] {
        match std::env::var(key) {
            Ok(path) => { exercise(kind, &path, false, [20017, 100013, 150233]); eprintln!("{kind} real-source seek, paused resume and repeated seeks PASS"); },
            Err(_) => eprintln!("{key} unset: private {kind} local-only coverage not run"),
        }
    }
}
