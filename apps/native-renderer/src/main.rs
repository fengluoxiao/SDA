//! SDA native stereo renderer sidecar.
//!
//! JSONL control is deliberately separated from the audio callback. The callback
//! owns no allocation, JSON parsing, or renderer IPC; it only advances one
//! codec-clock sample position and mixes independent source rings to stereo.
//! The engine core lives in the library crate root (src/lib.rs); this binary
//! is the desktop sidecar shell: stdin/stdout JSONL protocol plus device
//! output orchestration.

use std::sync::Arc;

use sda_native_renderer::*;


fn main() {
    if performance_simulation::entry() {
        return;
    }
    let commands = Arc::new(render_command::RenderCommandQueue::new(256));
    let fifo = Arc::new(stereo_fifo::StereoFifo::new(STEREO_FIFO_CAPACITY_FRAMES));
    let telemetry = Arc::new(RuntimeTelemetry::default());
    if !remote_audio::receiver() {
        spawn_render_worker(
            Engine::new(48000, 2),
            commands.clone(),
            fifo.clone(),
            telemetry.clone(),
        );
    }
    output_manager::run(fifo, telemetry, commands);
}

