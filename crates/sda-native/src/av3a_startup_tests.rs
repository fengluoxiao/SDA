use super::*;
fn setup() -> MobileEngine {
    let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
    engine.pipeline = Some(RenderPipeline {
        commands: Arc::new(render_command::RenderCommandQueue::new(4096)),
        fifo: Arc::new(stereo_fifo::StereoFifo::new(4096)),
        telemetry: Arc::new(RuntimeTelemetry::default()),
    });
    engine.balance.enabled = true;
    engine
}
fn feed(engine: &mut MobileEngine, start: usize, count: usize, silent: bool) {
    feed_program(engine, start, count, silent, "av3a");
}
fn feed_program(engine: &mut MobileEngine, start: usize, count: usize, silent: bool, codec: &'static str) {
    let labels = [
        "L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs", "Tfl", "Tfr", "Tbl", "Tbr",
    ]
    .map(String::from)
    .to_vec();
    let channels = (0..12)
        .map(|ch| {
            (start..start + count)
                .map(|i| {
                    if silent || ch == 3 {
                        0.0
                    } else {
                        0.12 * (i as f32 * (0.04 + ch as f32 * 0.007)).sin()
                    }
                })
                .collect()
        })
        .collect();
    let frame = FrameData { downmix_trims: vec![], reference_loudness: None, compatibility_bed: None,
        codec: "av3a",
        sample_rate: 48000,
        sample_pos: start as u64,
        channels,
        raw_bed_labels: labels.clone(),
        labels,
        events: vec![],
        object_channels: vec![],
        program_loudness: None,
        ramp_duration: 0,
    };
    let mut frame = frame;
    if codec != "av3a" {
        frame.codec = codec;
        frame.channels.truncate(2);
        for channel in &mut frame.channels {for sample in channel {*sample *= 5.0;}}
        frame.labels = vec!["L".into(), "R".into()];
        frame.raw_bed_labels = frame.labels.clone();
    }
    let frame = engine.balance.measure(frame, None);
    engine.pending.lock().unwrap().push_back(frame);
    engine.drain_pending_into_pipeline().unwrap();
}
#[test]
fn av3a_startup_balances_before_start_at_and_does_not_wait_for_the_song() {
    let mut engine = setup();
    for start in (0..48000 * 5).step_by(4800) {
        feed(&mut engine, start, 4800, false);
    }
    assert!(engine.playback_status().preparing_audio);
    assert!(!engine.startup.lock().unwrap().started);
    assert!(engine.pipeline.as_ref().unwrap().commands.pending_len_for_debug() == 0);
    for start in (48000 * 5..48000 * 7).step_by(4800) {
        feed(&mut engine, start, 4800, false);
        if engine.startup.lock().unwrap().started {
            break;
        }
    }
    assert!(engine.startup.lock().unwrap().started);
    assert!(!engine.playback_status().preparing_audio);
    assert!(engine.balance.gain_db < -3.0);
    assert!(engine.pipeline.as_ref().unwrap().commands.pending_len_for_debug() > 10);
    assert!(
        engine.complete_loudness_json() == "null",
        "preroll must not become a full-track cache"
    );
}
#[test]
fn av3a_startup_silent_intro_has_bounded_memory_and_releases() {
    let mut engine = setup();
    for start in (0..48000 * 7).step_by(4800) {
        feed(&mut engine, start, 4800, true);
    }
    assert!(engine.startup.lock().unwrap().started);
    assert!(engine.pending.lock().unwrap().is_empty());
    assert_eq!(engine.balance.gain_db, 0.0);
}
#[test]
fn av3a_startup_short_eof_uses_available_measurement_and_does_not_stall() {
    let mut engine = setup();
    feed(&mut engine, 0, 48000, false);
    assert!(engine.playback_status().preparing_audio);
    engine.finish().unwrap();
    assert!(engine.startup.lock().unwrap().started);
    assert!(engine.pending.lock().unwrap().is_empty());
    assert!(engine.balance.gain_db < -3.0);
}
#[test]
fn av3a_startup_switch_off_releases_existing_preroll() {
    let mut engine = setup();
    feed(&mut engine, 0, 48000, false);
    assert!(engine.playback_status().preparing_audio);
    engine.set_volume_balance(false).unwrap();
    assert!(engine.startup.lock().unwrap().started);
    assert!(engine.pending.lock().unwrap().is_empty());
    assert_eq!(engine.balance.gain_db, 0.0);
}

#[test]
fn stereo_startup_initial_gain_precedes_pcm_and_start_for_uncached_masters() {
    for codec in ["alac", "aac", "mp3", "flac", "pcm"] {
        let mut engine=setup();
        feed_program(&mut engine,0,48000,false,codec);
        assert!(engine.playback_status().preparing_audio,"{codec}");
        assert_eq!(engine.pipeline.as_ref().unwrap().commands.pending_len_for_debug(),0);
        for start in (48000..48000*7).step_by(4800) {
            feed_program(&mut engine,start,4800,false,codec);
            if engine.startup.lock().unwrap().started {break;}
        }
        assert!(engine.startup.lock().unwrap().started,"{codec}");
        assert!(engine.balance.gain_db < -1.0,"{codec}: {}",engine.balance.gain_db);
        assert!(!engine.playback_status().preparing_audio);
        assert_eq!(engine.complete_loudness_json(),"null");
        assert!(engine.pipeline.as_ref().unwrap().commands.pending_len_for_debug()>10);
    }
}
#[test]
fn stereo_startup_short_eof_silence_disable_and_cached_measurement() {
    let mut engine=setup();feed_program(&mut engine,0,48000,false,"alac");
    engine.finish().unwrap();assert!(engine.startup.lock().unwrap().started);assert!(engine.balance.gain_db < -1.0);
    let mut engine=setup();
    for start in (0..48000*7).step_by(4800) {feed_program(&mut engine,start,4800,true,"flac");}
    assert!(engine.startup.lock().unwrap().started);assert_eq!(engine.balance.gain_db,0.0);
    let mut engine=setup();feed_program(&mut engine,0,48000,false,"mp3");engine.set_volume_balance(false).unwrap();
    assert!(engine.startup.lock().unwrap().started);assert_eq!(engine.balance.gain_db,0.0);
    let mut engine=setup();
    engine.balance.set_cached(balance::Measurement{integrated_lufs:Some(-10.0),blocks:100,true_peak_dbtp:Some(-3.0)});
    feed_program(&mut engine,0,24000,false,"aac");
    assert!(engine.startup.lock().unwrap().started,"cached gain must skip preroll");assert_eq!(engine.balance.gain_db,-8.0);
}
