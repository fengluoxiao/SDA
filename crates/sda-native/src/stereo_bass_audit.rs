use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Default)]
struct Capture {
    samples: Mutex<Vec<f32>>,
    stop: AtomicBool,
}
impl AudioOutput for Capture {
    fn run(
        self: Arc<Self>,
        fifo: Arc<stereo_fifo::StereoFifo>,
        telemetry: Arc<RuntimeTelemetry>,
        _: Arc<render_command::RenderCommandQueue>,
    ) {
        let mut buffer = [0f32; 2048];
        while !self.stop.load(Ordering::Acquire)
            && !telemetry.shutdown_requested.load(Ordering::Acquire)
        {
            fifo.apply_flush_from_consumer();
            let n = fifo.pop_into_f32(&mut buffer, 2);
            if n > 0 {
                self.samples
                    .lock()
                    .unwrap()
                    .extend_from_slice(&buffer[..n * 2]);
                telemetry
                    .callback_consumed_sample_pos
                    .fetch_add(n as u64, Ordering::Release);
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}
#[test]
#[ignore = "local supplied PCM audit; requires SDA_STEREO_AUDIT_DIR"]
fn actual_mobile_alac_bass_output() {
    let dir = std::path::PathBuf::from(std::env::var_os("SDA_STEREO_AUDIT_DIR").unwrap());
    let tag = std::env::var("SDA_STEREO_AUDIT_TAG").expect("explicit capture tag");
    let bytes = std::fs::read(dir.join("fixture-start-48k.f32")).unwrap();
    let pcm: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
    for enabled in [false, true] {
        for upmix in [false, true] {
            let mut e = MobileEngine::new(
                EngineConfig {
                    direct_object_hrtf: true,
                    directional_hrtf: true,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
            e.load_hrtf(manifest.to_str().unwrap()).unwrap();
            let sink = Arc::new(Capture::default());
            e.start(sink.clone()).unwrap();
            e.set_volume_balance(enabled).unwrap();
            let mut adapter = alac_pcm::AlacPcm::default();
            let deadline = Instant::now() + Duration::from_secs(180);
            for (i, chunk) in pcm.chunks(8192).enumerate() {
                while e
                    .decoded_sample_pos()
                    .saturating_sub(e.playback_status().consumed_sample_pos)
                    > 96000
                {
                    assert!(Instant::now() < deadline, "feed stalled");
                    std::thread::sleep(Duration::from_millis(1));
                }
                let frame = adapter.live_frame(chunk, upmix, (i * 4096) as u64).unwrap();
                let reference = vec![
                    chunk.iter().step_by(2).copied().collect(),
                    chunk.iter().skip(1).step_by(2).copied().collect(),
                ];
                e.feed_alac_pcm(frame, reference).unwrap();
            }
            e.finish().unwrap();
            while sink.samples.lock().unwrap().len() < pcm.len() {
                assert!(Instant::now() < deadline, "drain stalled");
                std::thread::sleep(Duration::from_millis(1));
            }
            sink.stop.store(true, Ordering::Release);
            let samples = sink.samples.lock().unwrap();
            let mut bytes = Vec::with_capacity(pcm.len() * 4);
            for v in samples.iter().take(pcm.len()) {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            std::fs::write(
                dir.join(format!("mobile-{tag}-balance-{enabled}-upmix-{upmix}.f32")),
                bytes,
            )
            .unwrap();
            println!(
                "MOBILE_BASS enabled={enabled} upmix={upmix} balanceGain={} frames={} status={}",
                e.balance.gain_db,
                pcm.len() / 2,
                serde_json::to_string(&e.playback_status()).unwrap()
            );
            e.stop();
        }
    }
}
