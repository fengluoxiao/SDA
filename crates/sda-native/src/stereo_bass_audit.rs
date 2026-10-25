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
                let frame = adapter
                    .live_sda_frame(chunk, upmix, (i * 4096) as u64)
                    .unwrap();
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

fn matched_programme_loudness(
    dir: Option<&std::path::Path>,
    balances: &[bool],
) -> serde_json::Value {
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source =
        std::fs::read(root.join("harletty-bridge/harletty/tests/fixtures/joc_atmos_1s.eac3"))
            .unwrap();
    let bytes: Vec<u8> = (0..6).flat_map(|_| source.iter().copied()).collect();
    let mut decoder = StreamingDecoder::new("eac3").unwrap();
    decoder.push(&bytes).unwrap();
    decoder.flush();
    let mut reference = Vec::<f32>::new();
    let mut object_frames = 0usize;
    let mut count = 0usize;
    while let Some(f) = decoder.next_frame() {
        count += 1;
        if !f.object_channels.is_empty() || !f.events.is_empty() {
            object_frames += 1;
        }
        let (channels, labels) = if let Some(b) = &f.compatibility_bed {
            (&b.channels, &b.labels)
        } else {
            (&f.channels, &f.labels)
        };
        assert!(
            labels.iter().all(|label| !label.starts_with("Obj_")),
            "cannot substitute object slots for a compatible master"
        );
        let n = channels[0].len();
        let mut stereo = vec![[0f32; 2]; n];
        for (channel, label) in channels.iter().zip(labels) {
            let gains = sda_native_renderer::plain_downmix::gains(label);
            for (dst, &sample) in stereo.iter_mut().zip(channel) {
                for ear in 0..2 {
                    dst[ear] += sample * gains[ear];
                }
            }
        }
        reference.extend(stereo.into_iter().flatten());
    }
    assert!(
        object_frames > 0 && !reference.is_empty(),
        "fixture was not decoded as real Atmos objects"
    );
    assert!(decoder.drain_errors().is_empty());
    let mut results = Vec::new();
    for &enabled in balances {
        for mode in ["atmos", "stereo", "upmix"] {
            let mut e = MobileEngine::new(
                EngineConfig {
                    direct_object_hrtf: true,
                    directional_hrtf: true,
                    hrtf_wet_weight: 0.,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
            e.load_hrtf(
                root.join("assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json")
                    .to_str()
                    .unwrap(),
            )
            .unwrap();
            if mode == "atmos" {
                e.select_compressed_codec("eac3").unwrap();
            }
            let sink = Arc::new(Capture::default());
            e.start(sink.clone()).unwrap();
            e.set_volume_balance(enabled).unwrap();
            let deadline = Instant::now() + Duration::from_secs(300);
            let wait = |e: &MobileEngine| {
                while e
                    .decoded_sample_pos()
                    .saturating_sub(e.playback_status().consumed_sample_pos)
                    > if e.startup.lock().unwrap().started {
                        96000
                    } else {
                        384000
                    }
                {
                    assert!(
                        Instant::now() < deadline,
                        "matched programme feeder stalled"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
            };
            if mode == "atmos" {
                for chunk in bytes.chunks(4096) {
                    wait(&e);
                    let status = e.feed(chunk).unwrap();
                    assert!(status.errors.is_empty(), "{:?}", status.errors);
                }
            } else {
                let mut adapter = alac_pcm::AlacPcm::default();
                for (i, chunk) in reference.chunks(8192).enumerate() {
                    wait(&e);
                    let frame = adapter
                        .live_sda_frame(chunk, mode == "upmix", (i * 4096) as u64)
                        .unwrap();
                    let original = vec![
                        chunk.iter().step_by(2).copied().collect(),
                        chunk.iter().skip(1).step_by(2).copied().collect(),
                    ];
                    e.feed_alac_pcm(frame, original).unwrap();
                }
            }
            e.finish().unwrap();
            let frames = e.decoded_sample_pos() as usize;
            while sink.samples.lock().unwrap().len() < frames * 2 {
                assert!(
                    Instant::now() < deadline,
                    "matched programme output stalled"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            sink.stop.store(true, Ordering::Release);
            let all = sink.samples.lock().unwrap();
            let samples = &all[..frames * 2];
            let mut meter = balance::LoudnessMeter::new(48000);
            meter.push(&[
                samples.iter().step_by(2).copied().collect(),
                samples.iter().skip(1).step_by(2).copied().collect(),
            ]);
            let measurement = meter.integrated();
            let mut bytes = Vec::with_capacity(samples.len() * 4);
            for v in samples {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            if let Some(dir) = dir {
                std::fs::write(
                    dir.join(format!("matched-{mode}-balance-{enabled}.f32")),
                    bytes,
                )
                .unwrap();
            }
            results.push(serde_json::json!({"mode":mode,"balance":enabled,"frames":frames,"output":measurement,"programGainDb":e.balance.gain_db,"renderedProgramGainDb":e.playback_status().rendered_program_gain_db,"decodedReference":e.balance.complete_measurement(),"actualObjects":mode=="atmos"}));
            println!("MATCHED_LOUDNESS {}", results.last().unwrap());
            e.stop();
        }
    }
    let report = serde_json::json!({"scope":"same decoded Atmos compatible master; real Atmos objects versus derived stereo dry/upmix, same HRTFHQ and output gain","decodedFrames":count,"objectFrames":object_frames,"compatibleStereoFrames":reference.len()/2,"results":results});
    if let Some(dir) = dir {
        std::fs::write(
            dir.join("matched-programme-results.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    report
}

#[test]
#[ignore = "local matched-programme loudness measurement"]
fn compare_atmos_stereo_and_upmix_final_loudness() {
    let dir = std::path::PathBuf::from(std::env::var_os("SDA_MATCHED_LOUDNESS_DIR").unwrap());
    matched_programme_loudness(Some(&dir), &[false, true]);
}

#[test]
fn native_upmix_output_loudness_matches_same_programme_atmos() {
    let report = matched_programme_loudness(None, &[true]);
    let rows = report["results"].as_array().unwrap();
    let lufs = |name| {
        rows.iter().find(|r| r["mode"] == name).unwrap()["output"]["integratedLufs"]
            .as_f64()
            .unwrap()
    };
    let reference = lufs("atmos");
    assert!(
        (lufs("stereo") - reference).abs() < 1.,
        "ordinary stereo and Atmos level diverged"
    );
    assert!(
        (lufs("upmix") - reference).abs() < 1.,
        "upmix is not level-aligned to Atmos: stereo={} upmix={} atmos={reference}",
        lufs("stereo"),
        lufs("upmix")
    );
}
