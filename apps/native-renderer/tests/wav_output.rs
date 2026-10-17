use std::sync::{Arc, atomic::Ordering};
use sda_native_renderer::{AudioOutput, RuntimeTelemetry, WavDumpOutput, render_command::RenderCommandQueue, stereo_fifo::StereoFifo};

#[test]
fn wav_dump_preserves_every_stereo_frame_and_channel() {
    let frames = 2051;
    let samples: Vec<f32> = (0..frames)
        .flat_map(|i| { let left = i as f32 / 8192.0; [left, -left - 0.125] })
        .collect();
    let fifo = Arc::new(StereoFifo::new(4096));
    assert_eq!(fifo.push(&samples), frames);
    let telemetry = Arc::new(RuntimeTelemetry::default());
    let path = std::env::temp_dir().join(format!("sda-wav-frames-{}.wav", std::process::id()));
    Arc::new(WavDumpOutput::new(&path, 48000)).run(
        fifo, telemetry.clone(), Arc::new(RenderCommandQueue::new(1)),
    );
    let wav = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(wav.len(), 44 + frames * 2 * size_of::<i16>());
    assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize, frames * 4);
    for (i, bytes) in wav[44..].chunks_exact(2).enumerate() {
        assert_eq!(i16::from_le_bytes([bytes[0], bytes[1]]), (samples[i] * 32767.0) as i16, "sample {i}");
    }
    assert_eq!(telemetry.callback_consumed_sample_pos.load(Ordering::Acquire), frames as u64);
}
