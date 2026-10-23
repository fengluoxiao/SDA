//! Audio Vivid enters the same mobile renderer/FIFO used on Android and iOS.
use super::*;
#[test]
fn av3a_mobile_bed_uses_48k_clock_and_preserves_all_twelve_sources() {
    let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
    let bytes =
        include_bytes!("../../../packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a");
    for chunk in bytes.chunks(777) {
        let status = engine.feed(chunk).unwrap();
        assert!(status.errors.is_empty(), "{:?}", status.errors);
    }
    let status = engine.finish().unwrap();
    assert_eq!(status.codec, "av3a");
    assert!(status.errors.is_empty());
    assert_eq!(
        engine.decoded_sample_pos(),
        (3 * 1024 * 48000 + 22050) / 44100
    );
    let frames = engine.take_pending_frames();
    assert!(!frames.is_empty());
    let mut router = crate::frame_router::FrameRouter::default();
    let mut at = 0;
    for frame in frames {
        assert_eq!(frame.sample_pos, at);
        assert_eq!(frame.sample_rate, 48000);
        assert_eq!(
            frame.labels,
            ["L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs", "Tfl", "Tfr", "Tbl", "Tbr"]
        );
        assert_eq!(frame.channels.len(), 12);
        assert!(frame.events.is_empty());
        at += frame.channels[0].len() as u64;
        let commands = router.route(frame).unwrap();
        let render_command::RenderCommand::PcmFrame {
            entries, events, ..
        } = commands.last().unwrap()
        else {
            panic!()
        };
        assert_eq!(entries.len(), 12);
        assert!(events.is_empty());
    }
    assert_eq!(at, engine.decoded_sample_pos());
}

/// Private real-file regression. No music or encoded user frames are committed.
#[test]
#[ignore = "requires SDA_AV3A_MIX_INPUT (raw, CRC-validated AV3A access units)"]
fn av3a_private_mix_file_preserves_scene_through_mobile_facade() {
    let path = std::env::var("SDA_AV3A_MIX_INPUT").expect("SDA_AV3A_MIX_INPUT");
    let bytes = std::fs::read(path).unwrap();
    let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
    let mut router = crate::frame_router::FrameRouter::default();
    let (mut at, mut count, mut events, mut routed, mut lfe_energy) =
        (0u64, 0usize, 0usize, 0usize, 0f64);
    let mut check = |frames: Vec<sda_core::FrameData>| {
        for frame in frames {
            assert_eq!(frame.codec, "av3a");
            assert_eq!(frame.sample_rate, 48000);
            assert_eq!(frame.sample_pos, at);
            assert_eq!(frame.raw_bed_labels, ["L", "R", "C", "LFE", "Ls", "Rs"]);
            assert_eq!(frame.channels.len(), 21);
            assert_eq!(frame.object_channels.len(), 15);
            let n = frame.channels[0].len();
            assert!(n > 0);
            for (i, channel) in frame.channels.iter().enumerate() {
                assert_eq!(channel.len(), n);
                assert!(channel.iter().all(|s| s.is_finite()));
                if i < 6 && i != 3 {
                    assert!(channel.iter().all(|s| *s == 0.));
                }
            }
            lfe_energy += frame.channels[3]
                .iter()
                .map(|s| f64::from(*s).powi(2))
                .sum::<f64>();
            for declaration in &frame.object_channels {
                assert!((6..21).contains(&declaration.channel));
                assert_eq!(
                    frame.labels[declaration.channel as usize],
                    format!("Obj_{}", declaration.id)
                );
            }
            for e in &frame.events {
                assert!(e.sample_pos >= at && e.sample_pos < at + n as u64);
                assert!(frame.object_channels.iter().any(|o| o.id == e.id));
                assert!(e.gain_db.is_finite());
            }
            events += frame.events.len();
            let commands = router.route(frame).unwrap();
            for command in commands {
                if let render_command::RenderCommand::PcmFrame { entries, .. } = command {
                    assert_eq!(entries.len(), 21);
                    assert_eq!(
                        entries
                            .iter()
                            .filter(|(id, _)| id.starts_with("obj:"))
                            .count(),
                        15
                    );
                    assert_eq!(entries[3].0, "bed:3");
                    routed += 1;
                }
            }
            at += n as u64;
            count += 1;
        }
    };
    for chunk in bytes.chunks(8191) {
        let status = engine.feed(chunk).unwrap();
        assert!(status.errors.is_empty(), "{:?}", status.errors);
        check(engine.take_pending_frames());
    }
    let status = engine.finish().unwrap();
    assert!(status.errors.is_empty(), "{:?}", status.errors);
    check(engine.take_pending_frames());
    assert_eq!(count, 9722);
    assert_eq!(at, 9_955_328);
    assert_eq!(at, engine.decoded_sample_pos());
    assert_eq!(routed, count);
    assert!(events > 15 && lfe_energy > 0.);
    assert!(router.finish().unwrap().is_empty());
    // A seek must clear decoder history/events and start the same scene cleanly.
    engine.seek(0).unwrap();
    assert_eq!(engine.decoded_sample_pos(), 0);
    let status = engine.feed(&bytes[..3 * 4096]).unwrap();
    assert!(status.errors.is_empty(), "{:?}", status.errors);
    let restarted = engine.take_pending_frames();
    assert_eq!(restarted[0].sample_pos, 0);
    assert_eq!(restarted[0].object_channels.len(), 15);
    assert_eq!(restarted[0].events.len(), 15);
    println!("AV3A private Mix: {count} frames, {at} samples, {events} events, {routed} routed PCM batches; seek/restart passed");
}
