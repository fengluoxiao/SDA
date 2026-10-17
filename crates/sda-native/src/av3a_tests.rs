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
