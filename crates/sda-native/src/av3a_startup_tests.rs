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
    let frame = FrameData {
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
