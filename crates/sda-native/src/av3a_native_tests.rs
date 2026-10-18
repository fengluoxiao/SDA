use super::*;

// Exercise the ordinary HRTF -> stereo FIFO used when system spatial is off.
fn render(enabled: bool, toggle: bool) -> (Vec<f32>, f64) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let active: Value = serde_json::from_slice(
        &std::fs::read(root.join("apps/mobile/ios-experiments/active.json")).unwrap(),
    )
    .unwrap();
    let asset = root
        .join(active["assetFamily"].as_str().unwrap())
        .join("hrtf-dense/hrtf-set.json");
    let path = CString::new(asset.to_str().unwrap()).unwrap();
    let config = CString::new(r#"{"sampleRate":48000,"outputChannels":2,"layout":"7.1.4","directObjectHrtf":true,"directionalHrtf":true,"hrtfWetWeight":0.0}"#).unwrap();
    let mut error = std::ptr::null_mut();
    let h = unsafe { sda_ios_create(config.as_ptr(), path.as_ptr(), &mut error) };
    assert!(!h.is_null(), "native create failed");
    struct Close(*mut std::ffi::c_void);
    impl Drop for Close {
        fn drop(&mut self) {
            unsafe {
                sda_ios_close(self.0);
            }
        }
    }
    let _close = Close(h);
    let host = unsafe { &*h.cast::<Host>() };
    host.engine
        .lock()
        .unwrap()
        .set_volume_balance(enabled)
        .unwrap();
    let frames = 48000 * if toggle { 20 } else { 15 };
    // A real decoded excerpt can be supplied for diagnostic regression.
    let pcm = (!toggle)
        .then(|| std::env::var("SDA_AV3A_BALANCE_PCM").ok())
        .flatten()
        .map(|p| {
            std::fs::read(p)
                .unwrap()
                .chunks_exact(4)
                .map(|s| f32::from_le_bytes(s.try_into().unwrap()))
                .collect::<Vec<_>>()
        });
    std::thread::scope(|scope| {
        let sink = host.output.clone();
        let (requests, receiver) = std::sync::mpsc::channel::<bool>();
        let (ack, acknowledgements) = std::sync::mpsc::channel();
        let consumer = scope.spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(120);
            let mut output = Vec::new();
            let mut toggles = 0;
            while output.len() < frames * 2 {
                let mut l = [0.0; 512];
                let mut r = [0.0; 512];
                let n = pull(&sink, &mut l, &mut r);
                for i in 0..n {
                    output.extend_from_slice(&[l[i], r[i]]);
                }
                if toggle && toggles < 2 && output.len() >= (12 + toggles * 4) * 48000 * 2 {
                    requests.send(toggles == 1).unwrap();
                    acknowledgements
                        .recv_timeout(Duration::from_secs(30))
                        .unwrap();
                    toggles += 1;
                }
                assert!(
                    Instant::now() < deadline,
                    "AV3A native output stalled at {}",
                    output.len() / 2
                );
                if n == 0 {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            output.truncate(frames * 2);
            output
        });
        let service = || {
            while let Ok(enabled) = receiver.try_recv() {
                host.engine
                    .lock()
                    .unwrap()
                    .set_volume_balance(enabled)
                    .unwrap();
                ack.send(()).unwrap();
            }
        };
        let gain = {
            if let Some(path) = (!toggle)
                .then(|| std::env::var("SDA_AV3A_BALANCE_BITSTREAM").ok())
                .flatten()
            {
                let bytes = std::fs::read(path).unwrap();
                for chunk in bytes.chunks(2415 * 8) {
                    service();
                    let status = host.engine.lock().unwrap().feed(chunk).unwrap();
                    assert!(status.errors.is_empty(), "{:?}", status.errors);
                }
                host.engine.lock().unwrap().finish().unwrap();
            } else {
                for start in (0..frames).step_by(4096) {
                    service();
                    let count = 4096.min(frames - start);
                    let channels = (0..12)
                        .map(|ch| {
                            (start..start + count)
                                .map(|i| {
                                    pcm.as_ref().map_or_else(
                                        || {
                                            if ch == 3 {
                                                0.0
                                            } else {
                                                0.12 * (i as f32 * (0.04 + ch as f32 * 0.007)).sin()
                                            }
                                        },
                                        |p| p[(i % (p.len() / 12)) * 12 + ch],
                                    )
                                })
                                .collect()
                        })
                        .collect();
                    let frame = sda_core::FrameData {
                        codec: "av3a".into(),
                        sample_rate: 48000,
                        sample_pos: start as u64,
                        channels,
                        labels: [
                            "L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs", "Tfl", "Tfr", "Tbl",
                            "Tbr",
                        ]
                        .map(String::from)
                        .into(),
                        raw_bed_labels: vec![],
                        events: vec![],
                        object_channels: vec![],
                        program_loudness: None,
                        ramp_duration: 0,
                    };
                    let mut engine = host.engine.lock().unwrap();
                    let pending = engine.balance.measure(frame, None);
                    engine.pending.lock().unwrap().push_back(pending);
                    engine.drain_pending_into_pipeline().unwrap();
                }
            }
            let mut engine = host.engine.lock().unwrap();
            engine.startup.lock().unwrap().ended = true;
            engine.start_playback_if_ready().unwrap();
            engine.balance.gain_db
        };
        while let Ok(enabled) = receiver.recv() {
            host.engine
                .lock()
                .unwrap()
                .set_volume_balance(enabled)
                .unwrap();
            ack.send(()).unwrap();
        }
        let output = consumer.join().unwrap();
        let status = host.engine.lock().unwrap().playback_status();
        assert_eq!(
            status.hrtf_directions as u64,
            active["directionCount"].as_u64().unwrap()
        );
        assert!(status.volume_balance_eligible);
        assert_eq!(status.volume_balance_enabled, enabled);
        assert_eq!(status.renderer_program_enabled, enabled);
        if enabled {
            assert!(status.rendered_program_gain_db < -3.0);
        } else {
            assert_eq!(status.rendered_program_gain_db, 0.0);
        }
        (output, gain)
    })
}
#[test]
fn av3a_native_balance_reaches_hrtf_output_without_system_spatial() {
    let (off, off_gain) = render(false, false);
    let (on, on_gain) = render(true, false);
    assert!(
        (off_gain - on_gain).abs() < 0.1,
        "streaming targets diverged: {off_gain} / {on_gain}"
    );
    let energy = |p: &[f32]| p.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
    let first = 48000 * 2..48000 * 3 * 2;
    let startup_db = 10.0 * (energy(&on[first.clone()]) / energy(&off[first])).log10();
    eprintln!("AV3A first audible segment: {startup_db:.3} dB");
    assert!(startup_db < -3.0, "startup escaped balance: {startup_db}");
    let range = 48000 * 12 * 2..48000 * 14 * 2;
    let ratio = energy(&on[range.clone()]) / energy(&off[range]);
    let actual_db = 10.0 * ratio.log10();
    eprintln!("AV3A native HRTF balance: target={on_gain:.3} dB, output={actual_db:.3} dB");
    assert!(on_gain < -3.0);
    if std::env::var_os("SDA_AV3A_BALANCE_PCM").is_some()
        || std::env::var_os("SDA_AV3A_BALANCE_BITSTREAM").is_some()
    {
        // On a hot real mix the bypass can engage the peak guard. It cannot be
        // used as a linear unity reference; the output must still attenuate.
        assert!(
            actual_db < -3.0,
            "real AV3A PCM was not attenuated: {actual_db}"
        );
    } else {
        assert!(
            (actual_db - on_gain).abs() < 0.05,
            "ordinary HRTF gain missing: {actual_db} versus {on_gain}"
        );
    }
}

#[test]
fn av3a_native_live_balance_switch_changes_the_same_playback_output() {
    let (pcm, gain) = render(true, true);
    let energy = |start: usize, end: usize| {
        let p = &pcm[start * 48000 * 2..end * 48000 * 2];
        p.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / p.len() as f64
    };
    let before = energy(10, 11);
    let disabled = energy(14, 15);
    let restored = energy(18, 19);
    let off_delta = 10.0 * (disabled / before).log10();
    let on_delta = 10.0 * (restored / disabled).log10();
    eprintln!("AV3A live switch: target={gain:.3} dB, off delta={off_delta:.3} dB, on delta={on_delta:.3} dB");
    assert!(
        (off_delta + gain).abs() < 0.2,
        "live switch off ineffective: {off_delta}"
    );
    assert!(
        (on_delta - gain).abs() < 0.2,
        "live switch on ineffective: {on_delta}"
    );
}
