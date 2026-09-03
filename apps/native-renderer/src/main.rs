//! SDA native stereo renderer sidecar.
//!
//! JSONL control is deliberately separated from the audio callback. The callback
//! owns no allocation, JSON parsing, or renderer IPC; it only advances one
//! codec-clock sample position and mixes independent source rings to stereo.
//! This first foundation is a reference mix path. Object HRTF partitioned
//! convolution plugs in after the per-source sample fetch, without changing the
//! IPC clock or the WASAPI output lifecycle.

use std::{
    collections::{BTreeMap, HashMap},
    io::{self, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use serde::{Deserialize, Serialize};

const PROTOCOL: u32 = 1;
const MAX_SOURCES: usize = 64;
const MAX_PENDING_SAMPLES: usize = 480_000; // 10 s @ 48 kHz per source.
const FRAME_JSON: u8 = b'J';
const FRAME_PCM: u8 = b'P';
/// Atomic collection of all mono source blocks for one decoded codec frame.
const FRAME_PCM_BATCH: u8 = b'B';
const NATIVE_RENDERER_MAX_JSON_BYTES: usize = 16 * 1024;
const DEFAULT_OBJECT_RAMP: u32 = 128;
const STEREO_FIFO_CAPACITY_FRAMES: usize = 32_768;
const STEREO_FIFO_TARGET_FRAMES: usize = 16_384;

mod bus_renderer;
mod convolution;
#[allow(dead_code)]
mod hrtf;
mod pcm_ring;
mod protocol;
mod render_command;
mod spatial;
mod stereo_fifo;
mod vbap;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Command {
    Hello {
        protocol: u32,
    },
    Configure {
        sample_rate: u32,
        channels: u16,
    },
    AddSource {
        id: String,
    },
    RemoveSource {
        id: String,
        at: u64,
    },
    /// Reference transport. Production Electron integration upgrades samples to
    /// framed binary blocks while preserving `start` and the source lifecycle.
    Feed {
        id: String,
        start: u64,
        samples: Vec<f32>,
    },
    SetGain {
        id: String,
        gain: f32,
        ramp: u32,
        at: Option<u64>,
    },
    SetMuted {
        id: String,
        muted: bool,
        at: Option<u64>,
    },
    /// Full codec object metadata. Native applies it before PCM at `sample_pos`.
    ObjectEvents {
        events: Vec<NativeObjectEvent>,
    },
    HeadPose {
        orientation: [f32; 4],
    },
    /// Load one bundled calibrated v4 asset family relative to SDA_HRTF_ROOT.
    SetHrtf {
        set: String,
        #[serde(rename = "wetWeight")]
        wet_weight: f32,
    },
    /// Explicit exclusive output ownership. Defaults to muted while transport
    /// and HRTF preparation are being validated beside Web Audio.
    SetOutputActive {
        active: bool,
    },
    /// Set the authoritative codec-clock origin and atomically begin native output.
    StartAt {
        origin: u64,
    },
    ClearHeadPose,
    Pause {
        paused: bool,
    },
    Reset {
        origin: u64,
    },
    Health,
    Shutdown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Event<'a> {
    Ready {
        protocol: u32,
        sample_rate: u32,
        output_channels: u16,
    },
    Ack {
        command: &'a str,
        accepted: bool,
        detail: Option<&'a str>,
    },
    BatchAck {
        start: u64,
        samples: u32,
        accepted: bool,
        detail: Option<&'a str>,
    },
    Health(Health),
    Error {
        detail: String,
    },
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Health {
    sample_pos: u64,
    active_sources: usize,
    underrun_samples: u64,
    callback_count: u64,
    callback_max_micros: u64,
    callback_fifo_underrun_frames: u64,
    fifo_frames_available: usize,
    render_block_count: u64,
    render_block_mean_micros: u64,
    render_block_max_micros: u64,
    output_sample_rate: u32,
    output_channels: u16,
    paused: bool,
    reference_mix: bool,
    output_active: bool,
    hrtf_ready: bool,
    route_update_count: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeObjectEvent {
    id: u32,
    sample_pos: u64,
    has_pos: bool,
    pos: [f32; 3],
    gain_db: f32,
    size: [f32; 3],
    ramp_duration: u32,
}

#[derive(Clone, Copy)]
struct GainEvent {
    gain: f32,
    ramp: u32,
}

#[derive(Clone, Copy)]
struct SpatialEvent {
    position: [f32; 3],
    spread: f32,
    ramp: u32,
}

struct Source {
    samples: pcm_ring::AbsolutePcmRing,
    gain_events: BTreeMap<u64, GainEvent>,
    gain: f32,
    target_gain: f32,
    ramp_remaining: u32,
    ramp_step: f32,
    remove_at: Option<u64>,
    position: [f32; 3],
    spread: f32,
    spatial_events: BTreeMap<u64, SpatialEvent>,
    bus_gains: [f32; vbap::BUS_COUNT],
    bus_targets: [f32; vbap::BUS_COUNT],
    bus_steps: [f32; vbap::BUS_COUNT],
    bus_ramp_remaining: u32,
    availability: f32,
    availability_target: f32,
    availability_step: f32,
    availability_ramp_remaining: u32,
    muted: bool,
    mute_events: BTreeMap<u64, bool>,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            samples: pcm_ring::AbsolutePcmRing::new(MAX_PENDING_SAMPLES),
            gain_events: BTreeMap::new(),
            gain: 0.0,
            target_gain: 0.0,
            ramp_remaining: 0,
            ramp_step: 0.0,
            remove_at: None,
            position: [0.0, 1.0, 0.0],
            spread: 0.0,
            spatial_events: BTreeMap::new(),
            bus_gains: [0.0; vbap::BUS_COUNT],
            bus_targets: [0.0; vbap::BUS_COUNT],
            bus_steps: [0.0; vbap::BUS_COUNT],
            bus_ramp_remaining: 0,
            availability: 0.0,
            availability_target: 0.0,
            availability_step: 0.0,
            availability_ramp_remaining: 0,
            muted: false,
            mute_events: BTreeMap::new(),
        }
    }
}

struct RuntimeTelemetry {
    callback_output_enabled: AtomicBool,
    callback_count: AtomicU64,
    callback_max_micros: AtomicU64,
    callback_fifo_underrun_frames: AtomicU64,
    render_block_count: AtomicU64,
    render_block_total_micros: AtomicU64,
    render_block_max_micros: AtomicU64,
}

impl RuntimeTelemetry {
    fn record_max(target: &AtomicU64, value: u64) {
        let mut current = target.load(Ordering::Relaxed);
        while value > current {
            match target.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }
}

struct Engine {
    sample_pos: u64,
    paused: bool,
    sources: HashMap<String, Source>,
    underrun_samples: u64,
    output_sample_rate: u32,
    output_channels: u16,
    head_pose: Option<[f32; 4]>,
    pending_object_events: HashMap<String, Vec<NativeObjectEvent>>,
    active_hrtf_set: Option<hrtf::NativeHrtfSet>,
    hrtf_wet_weight: f32,
    vbap: vbap::VbapSolver,
    bus_renderer: Option<bus_renderer::BusRenderer>,
    block_offset: usize,
    output_active: bool,
    render_epoch: u64,
    route_update_count: u64,
}

impl Engine {
    fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_pos: 0,
            paused: false,
            sources: HashMap::new(),
            underrun_samples: 0,
            output_sample_rate: sample_rate,
            output_channels: channels,
            head_pose: None,
            pending_object_events: HashMap::new(),
            active_hrtf_set: None,
            hrtf_wet_weight: 0.5,
            vbap: vbap::VbapSolver::new(),
            bus_renderer: None,
            block_offset: 0,
            output_active: false,
            render_epoch: 0,
            route_update_count: 0,
        }
    }

    fn hrtf_root() -> std::path::PathBuf {
        std::env::var_os("SDA_HRTF_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("hrtf-assets"))
    }

    fn rebuild_bus_renderer(&mut self) -> Result<(), String> {
        let set = self
            .active_hrtf_set
            .as_ref()
            .ok_or("native HRTF set is not configured")?;
        self.bus_renderer = Some(bus_renderer::BusRenderer::new(
            set,
            &self.vbap,
            self.hrtf_wet_weight,
        )?);
        Ok(())
    }

    fn set_source_route(source: &mut Source, gains: [f32; vbap::BUS_COUNT], ramp: u32) {
        let ramp = ramp.max(1);
        source.bus_targets = gains;
        source.bus_ramp_remaining = ramp;
        for index in 0..vbap::BUS_COUNT {
            source.bus_steps[index] = (gains[index] - source.bus_gains[index]) / ramp as f32;
        }
    }

    fn advance_source_envelopes(source: &mut Source, samples: u32) {
        let scalar = samples.min(source.ramp_remaining);
        if scalar > 0 {
            source.gain += source.ramp_step * scalar as f32;
            source.ramp_remaining -= scalar;
            if source.ramp_remaining == 0 {
                source.gain = source.target_gain;
            }
        }
        let route = samples.min(source.bus_ramp_remaining);
        if route > 0 {
            for bus in 0..vbap::BUS_COUNT {
                source.bus_gains[bus] += source.bus_steps[bus] * route as f32;
            }
            source.bus_ramp_remaining -= route;
            if source.bus_ramp_remaining == 0 {
                source.bus_gains = source.bus_targets;
            }
        }
    }

    fn route_source_now(&mut self, id: &str, ramp: u32) -> Result<(), String> {
        let (position, spread) = self
            .sources
            .get(id)
            .map(|source| (source.position, source.spread))
            .ok_or("unknown source")?;
        let gains = bus_renderer::route(&self.vbap, position, self.head_pose, spread);
        let source = self.sources.get_mut(id).expect("source was checked above");
        Self::set_source_route(source, gains, ramp);
        if ramp == 0 {
            source.bus_gains = gains;
            source.bus_targets = gains;
            source.bus_ramp_remaining = 0;
        }
        Ok(())
    }

    fn health(&self, fifo: &stereo_fifo::StereoFifo, telemetry: &RuntimeTelemetry) -> Health {
        let render_blocks = telemetry.render_block_count.load(Ordering::Relaxed);
        let render_total = telemetry.render_block_total_micros.load(Ordering::Relaxed);
        Health {
            sample_pos: self.sample_pos,
            active_sources: self.sources.len(),
            underrun_samples: self.underrun_samples,
            callback_count: telemetry.callback_count.load(Ordering::Relaxed),
            callback_max_micros: telemetry.callback_max_micros.load(Ordering::Relaxed),
            callback_fifo_underrun_frames: telemetry
                .callback_fifo_underrun_frames
                .load(Ordering::Relaxed),
            fifo_frames_available: fifo.available_read(),
            render_block_count: render_blocks,
            render_block_mean_micros: if render_blocks == 0 {
                0
            } else {
                render_total / render_blocks
            },
            render_block_max_micros: telemetry.render_block_max_micros.load(Ordering::Relaxed),
            output_sample_rate: self.output_sample_rate,
            output_channels: self.output_channels,
            paused: self.paused,
            reference_mix: self.active_hrtf_set.is_none(),
            output_active: self.output_active,
            hrtf_ready: self.active_hrtf_set.is_some(),
            route_update_count: self.route_update_count,
        }
    }

    /// True only when no active source has the current codec sample. This is a
    /// producer-starvation guard, not the old all-source barrier: one late object
    /// is allowed to fade locally while any bed or other object keeps transport
    /// advancing. Without it, the native clock can race through future PCM and
    /// make the player incorrectly conclude that the program ended.
    fn has_any_pcm_at(&self, clock: u64) -> bool {
        self.sources.values().any(|source| {
            !source.remove_at.is_some_and(|remove_at| clock >= remove_at)
                && source.samples.has_at(clock)
        })
    }

    /// Renders source PCM into fixed virtual-speaker buses. PCM availability is
    /// intentionally source-local: a late object fades itself out instead of
    /// stopping all beds and objects at the next 128-sample boundary.
    fn render_into(&mut self, output: &mut [f32], channels: usize) {
        output.fill(0.0);
        if self.paused || !self.output_active || self.bus_renderer.is_none() {
            return;
        }
        let mut underruns = 0_u64;
        let vbap = &self.vbap;
        let head_pose = self.head_pose;
        for frame in output.chunks_exact_mut(channels) {
            let at = self.sample_pos;
            let block_index = self.block_offset;
            if block_index == 0 {
                self.bus_renderer
                    .as_mut()
                    .expect("checked above")
                    .begin_block();
            }
            let mut direct = 0.0_f32;
            for (id, source) in &mut self.sources {
                if source.remove_at.is_some_and(|remove_at| at >= remove_at) {
                    continue;
                }
                if let Some(event) = source.spatial_events.remove(&at) {
                    source.position = event.position;
                    source.spread = event.spread;
                    Self::set_source_route(
                        source,
                        bus_renderer::route(vbap, event.position, head_pose, event.spread),
                        event.ramp,
                    );
                    self.route_update_count = self.route_update_count.saturating_add(1);
                }
                if let Some(muted) = source.mute_events.remove(&at) {
                    source.muted = muted;
                }
                if let Some(event) = source.gain_events.remove(&at) {
                    source.target_gain = event.gain;
                    source.ramp_remaining = event.ramp.max(1);
                    source.ramp_step = (event.gain - source.gain) / source.ramp_remaining as f32;
                }
                // Match master worklet timing: the event boundary emits the
                // current vector/scalar first, then advances its envelopes for
                // the following sample. Advancing here would make every moving
                // object start one step ahead of its scheduled codec sample.
                let raw = source.samples.take(at);
                let target = if raw.is_some() { 1.0 } else { 0.0 };
                if target != source.availability_target {
                    source.availability_target = target;
                    source.availability_ramp_remaining = 32;
                    source.availability_step = (target - source.availability) / 32.0;
                }
                if source.availability_ramp_remaining > 0 {
                    source.availability += source.availability_step;
                    source.availability_ramp_remaining -= 1;
                    if source.availability_ramp_remaining == 0 {
                        source.availability = source.availability_target;
                    }
                }
                let sample = raw.unwrap_or(0.0)
                    * source.availability
                    * source.gain
                    * if source.muted { 0.0 } else { 1.0 };
                if raw.is_none() && source.gain != 0.0 {
                    underruns += 1;
                }
                if id.starts_with("obj:") {
                    self.bus_renderer.as_mut().expect("checked above").add(
                        sample,
                        &source.bus_gains,
                        block_index,
                    );
                } else {
                    direct += sample;
                }
                Self::advance_source_envelopes(source, 1);
            }
            let binaural = self
                .bus_renderer
                .as_ref()
                .expect("checked above")
                .output_at(block_index);
            let left = direct + binaural[0];
            let right = direct + binaural[1];
            if channels >= 2 {
                frame[0] = left;
                frame[1] = right;
            } else if channels == 1 {
                frame[0] = 0.5 * (left + right);
            }
            if block_index + 1 == convolution::DEFAULT_PARTITION {
                let _ = self
                    .bus_renderer
                    .as_mut()
                    .expect("checked above")
                    .finish_block();
            }
            self.sample_pos += 1;
            self.block_offset = (self.block_offset + 1) % convolution::DEFAULT_PARTITION;
        }
        self.underrun_samples += underruns;
        self.sources.retain(|_, source| {
            !source
                .remove_at
                .is_some_and(|remove_at| self.sample_pos >= remove_at)
        });
    }

    #[cfg(test)]
    fn mix(&mut self, output: &mut [f32], channels: usize) {
        self.render_into(output, channels);
    }
}

fn write_event(event: &Event<'_>) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if serde_json::to_writer(&mut out, event).is_ok() {
        let _ = writeln!(out);
        let _ = out.flush();
    }
}

fn spawn_render_worker(
    mut engine: Engine,
    commands: Arc<render_command::RenderCommandQueue>,
    fifo: Arc<stereo_fifo::StereoFifo>,
    telemetry: Arc<RuntimeTelemetry>,
) {
    thread::Builder::new()
        .name("sda-native-render".into())
        .spawn(move || {
            let mut block = vec![0.0_f32; convolution::DEFAULT_PARTITION * 2];
            let mut observed_epoch = 0_u64;
            loop {
                for _ in 0..16 {
                    let Some(command) = commands.pop() else {
                        break;
                    };
                    if !protocol::apply_render_command(&mut engine, command, &fifo, &telemetry) {
                        return;
                    }
                }
                if engine.render_epoch != observed_epoch {
                    fifo.clear_from_producer();
                    telemetry
                        .callback_output_enabled
                        .store(false, Ordering::Release);
                    observed_epoch = engine.render_epoch;
                }
                if fifo.available_read() >= STEREO_FIFO_TARGET_FRAMES
                    || fifo.available_write() < convolution::DEFAULT_PARTITION
                    || !engine.output_active
                    || engine.paused
                    || !engine.has_any_pcm_at(engine.sample_pos)
                {
                    commands.wait(Duration::from_micros(500));
                    continue;
                }
                let started = Instant::now();
                engine.render_into(&mut block, 2);
                if fifo.push(&block) != convolution::DEFAULT_PARTITION {
                    continue;
                }
                telemetry.render_block_count.fetch_add(1, Ordering::Relaxed);
                if fifo.available_read() >= 2_048 {
                    telemetry
                        .callback_output_enabled
                        .store(true, Ordering::Release);
                }
                let elapsed = started.elapsed().as_micros() as u64;
                telemetry
                    .render_block_total_micros
                    .fetch_add(elapsed, Ordering::Relaxed);
                RuntimeTelemetry::record_max(&telemetry.render_block_max_micros, elapsed);
            }
        })
        .expect("cannot start native render worker");
}

fn record_callback(
    telemetry: &RuntimeTelemetry,
    started: Instant,
    requested: usize,
    popped: usize,
    output_enabled: bool,
) {
    telemetry.callback_count.fetch_add(1, Ordering::Relaxed);
    if output_enabled {
        telemetry
            .callback_fifo_underrun_frames
            .fetch_add((requested - popped) as u64, Ordering::Relaxed);
    }
    RuntimeTelemetry::record_max(
        &telemetry.callback_max_micros,
        started.elapsed().as_micros() as u64,
    );
}

fn build_stream(
    fifo: Arc<stereo_fifo::StereoFifo>,
    telemetry: Arc<RuntimeTelemetry>,
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
) -> Result<Stream, String> {
    let channels = config.channels as usize;
    let error = |err| eprintln!("[SDA native renderer] WASAPI stream error: {err}");
    match format {
        SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                let started = Instant::now();
                let requested = data.len() / channels;
                let output_enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
                let popped = if output_enabled {
                    fifo.pop_into_f32(data, channels)
                } else {
                    data.fill(0.0);
                    0
                };
                record_callback(&telemetry, started, requested, popped, output_enabled);
            },
            error,
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            config,
            move |data: &mut [i16], _| {
                let started = Instant::now();
                let requested = data.len() / channels;
                let output_enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
                let popped = if output_enabled {
                    fifo.pop_into_i16(data, channels)
                } else {
                    data.fill(0);
                    0
                };
                record_callback(&telemetry, started, requested, popped, output_enabled);
            },
            error,
            None,
        ),
        SampleFormat::U16 => device.build_output_stream(
            config,
            move |data: &mut [u16], _| {
                let started = Instant::now();
                let requested = data.len() / channels;
                let output_enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
                let popped = if output_enabled {
                    fifo.pop_into_u16(data, channels)
                } else {
                    data.fill(u16::MAX / 2);
                    0
                };
                record_callback(&telemetry, started, requested, popped, output_enabled);
            },
            error,
            None,
        ),
        other => return Err(format!("unsupported WASAPI sample format: {other:?}")),
    }
    .map_err(|error| error.to_string())
}

fn main() {
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(device) => device,
        None => {
            write_event(&Event::Error {
                detail: "No default WASAPI output device".into(),
            });
            return;
        }
    };
    // The decoder and measured HRTF assets are both 48 kHz. Never advance their
    // shared codec clock at an arbitrary default device rate: select an actual
    // 48 kHz WASAPI configuration or fail explicitly until resampling exists.
    let supported = match device.supported_output_configs() {
        Ok(configs) => configs
            .filter(|config| {
                config.channels() >= 2
                    && config.min_sample_rate().0 <= 48_000
                    && config.max_sample_rate().0 >= 48_000
            })
            .min_by_key(|config| match config.sample_format() {
                SampleFormat::F32 => 0,
                SampleFormat::I16 => 1,
                SampleFormat::U16 => 2,
                _ => 3,
            })
            .map(|config| config.with_sample_rate(cpal::SampleRate(48_000))),
        Err(error) => {
            write_event(&Event::Error {
                detail: format!("Cannot enumerate WASAPI output formats: {error}"),
            });
            return;
        }
    };
    let Some(supported) = supported else {
        write_event(&Event::Error {
            detail: "Default WASAPI device has no stereo 48 kHz output format".into(),
        });
        return;
    };
    let config: StreamConfig = supported.config();
    let engine = Engine::new(config.sample_rate.0, config.channels);
    let commands = Arc::new(render_command::RenderCommandQueue::new(256));
    let fifo = Arc::new(stereo_fifo::StereoFifo::new(STEREO_FIFO_CAPACITY_FRAMES));
    let telemetry = Arc::new(RuntimeTelemetry {
        callback_output_enabled: AtomicBool::new(false),
        callback_count: AtomicU64::new(0),
        callback_max_micros: AtomicU64::new(0),
        callback_fifo_underrun_frames: AtomicU64::new(0),
        render_block_count: AtomicU64::new(0),
        render_block_total_micros: AtomicU64::new(0),
        render_block_max_micros: AtomicU64::new(0),
    });
    spawn_render_worker(engine, commands.clone(), fifo.clone(), telemetry.clone());
    let stream = match build_stream(
        fifo.clone(),
        telemetry.clone(),
        &device,
        &config,
        supported.sample_format(),
    ) {
        Ok(stream) => stream,
        Err(error) => {
            write_event(&Event::Error {
                detail: format!("Cannot start WASAPI output: {error}"),
            });
            return;
        }
    };
    if let Err(error) = stream.play() {
        write_event(&Event::Error {
            detail: format!("Cannot play WASAPI stream: {error}"),
        });
        return;
    }
    write_event(&Event::Ready {
        protocol: PROTOCOL,
        sample_rate: config.sample_rate.0,
        output_channels: config.channels,
    });

    let stdin = io::stdin();
    if let Err(error) = protocol::read_frames(&mut stdin.lock(), &commands) {
        write_event(&Event::Error {
            detail: format!("Native renderer protocol error: {error}"),
        });
    }
    drop(stream);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calibrated_engine() -> Engine {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let mut engine = Engine::new(48_000, 2);
        engine.active_hrtf_set = Some(hrtf::NativeHrtfSet::load_calibrated(&root).unwrap());
        engine.rebuild_bus_renderer().unwrap();
        engine.output_active = true;
        engine
    }

    #[test]
    fn calibrated_dense_assets_select_nearest_measured_direction() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf-dense/hrtf-set.json");
        let set = hrtf::NativeHrtfSet::load_calibrated(&root).unwrap();
        assert_eq!(set.sample_rate, 48_000);
        assert_eq!(set.nearest(22.0, 0.0).unwrap().azimuth, 20.0);
    }

    #[test]
    fn legacy_ku100_subject_hybrid_is_rejected() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf-ku100-d2/hrtf-set.json");
        assert!(
            hrtf::NativeHrtfSet::load_calibrated(&root)
                .unwrap_err()
                .contains("complete-subject")
        );
    }

    #[test]
    fn fixed_bus_graph_is_independent_of_source_count() {
        let mut engine = calibrated_engine();
        assert_eq!(
            engine.bus_renderer.as_ref().unwrap().bus_count(),
            vbap::BUS_COUNT
        );
        for object in 0..MAX_SOURCES {
            engine.sources.insert(
                format!("obj:{object}"),
                Source {
                    gain: 1.0,
                    target_gain: 1.0,
                    ..Source::default()
                },
            );
        }
        assert_eq!(
            engine.bus_renderer.as_ref().unwrap().bus_count(),
            vbap::BUS_COUNT
        );
    }

    #[test]
    fn missing_object_pcm_does_not_stop_other_sources_or_clock() {
        let mut engine = calibrated_engine();
        let mut bed = Source {
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        bed.samples
            .write(0, 0, &[0.25; convolution::DEFAULT_PARTITION]);
        engine.sources.insert("bed:0".into(), bed);
        engine.sources.insert(
            "obj:late".into(),
            Source {
                gain: 1.0,
                target_gain: 1.0,
                ..Source::default()
            },
        );
        let mut output = vec![0.0; convolution::DEFAULT_PARTITION * 2];
        engine.mix(&mut output, 2);
        assert_eq!(engine.sample_pos, convolution::DEFAULT_PARTITION as u64);
        assert!(output.iter().any(|sample| *sample != 0.0));
        assert!(engine.underrun_samples >= convolution::DEFAULT_PARTITION as u64);
    }

    #[test]
    fn empty_all_source_window_does_not_advance_the_native_clock() {
        let mut engine = calibrated_engine();
        engine.sources.insert(
            "bed:0".into(),
            Source {
                gain: 1.0,
                target_gain: 1.0,
                ..Source::default()
            },
        );
        assert!(!engine.has_any_pcm_at(0));
        assert_eq!(engine.sample_pos, 0);
    }

    #[test]
    fn object_route_ramp_emits_event_sample_before_advancing() {
        let mut source = Source {
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        source.bus_gains[0] = 1.0;
        let target = [0.0; vbap::BUS_COUNT];
        Engine::set_source_route(&mut source, target, 4);
        assert_eq!(source.bus_gains[0], 1.0);
        Engine::advance_source_envelopes(&mut source, 1);
        assert!((source.bus_gains[0] - 0.75).abs() < 1e-6);
        Engine::advance_source_envelopes(&mut source, 3);
        assert_eq!(source.bus_gains, target);
    }

    #[test]
    fn native_output_is_silent_until_ownership_is_explicitly_enabled() {
        let mut engine = calibrated_engine();
        engine.output_active = false;
        let mut source = Source {
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        source.samples.write(0, 0, &[1.0]);
        engine.sources.insert("bed:0".into(), source);
        let mut output = [1.0; 2];
        engine.mix(&mut output, 2);
        assert_eq!(output, [0.0, 0.0]);
        assert_eq!(engine.sample_pos, 0);
    }

    #[test]
    fn remove_boundary_is_sample_accurate_for_direct_bed() {
        let mut engine = calibrated_engine();
        let mut source = Source {
            gain: 1.0,
            target_gain: 1.0,
            remove_at: Some(1),
            ..Source::default()
        };
        source.samples.write(0, 0, &[1.0, 1.0]);
        engine.sources.insert("bed:0".into(), source);
        let mut output = vec![0.0; 34 * 2];
        engine.mix(&mut output, 2);
        assert!(output[0] > 0.0);
        assert_eq!(output[2], 0.0);
        assert!(!engine.sources.contains_key("bed:0"));
    }
}
