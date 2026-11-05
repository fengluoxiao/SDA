use std::sync::Arc;
use sda_native_renderer::*;
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if sda_native::desktop::reference_entry(&args) {return;}
    if args.get(1).is_some_and(|arg| arg == "--sda-file") { sda_format::cli::main(&args[2..]); return; }
    if bed_export::entry() || performance_simulation::entry() { return; }
    let commands = Arc::new(render_command::RenderCommandQueue::new(4 * sda_core::MAX_DECODED_SOURCE_CHANNELS + 32));
    let fifo = Arc::new(stereo_fifo::StereoFifo::new(STEREO_FIFO_CAPACITY_FRAMES));
    let telemetry = Arc::new(RuntimeTelemetry::default());
    sda_native::desktop::install(commands.clone(), fifo.clone(), telemetry.clone());
    if !remote_audio::receiver() { spawn_render_worker(Engine::new(48000, 2), commands.clone(), fifo.clone(), telemetry.clone()); }
    output_manager::run(fifo, telemetry, commands);
}
