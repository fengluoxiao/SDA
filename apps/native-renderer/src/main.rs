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
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, Stream, StreamConfig,
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
const STEREO_FIFO_CAPACITY_FRAMES: usize = 32_768;
const STEREO_FIFO_TARGET_FRAMES: usize = 16_384;

mod convolution;
#[allow(dead_code)]
mod hrtf;
mod pcm_ring;
mod protocol;
mod spatial;
mod stereo_fifo;

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
    hrtf_direction: Option<(f64, f64)>,
    spatial_events: BTreeMap<u64, SpatialEvent>,
    convolver: Option<convolution::StereoPartitionedConvolver>,
    input_block: Vec<f32>,
    output_left: Vec<f32>,
    output_right: Vec<f32>,
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
            hrtf_direction: None,
            spatial_events: BTreeMap::new(),
            convolver: None,
            input_block: vec![0.0; convolution::DEFAULT_PARTITION],
            output_left: vec![0.0; convolution::DEFAULT_PARTITION],
            output_right: vec![0.0; convolution::DEFAULT_PARTITION],
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
            match target.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
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
    block_offset: usize,
    output_active: bool,
    render_epoch: u64,
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
            block_offset: 0,
            output_active: false,
            render_epoch: 0,
        }
    }

    fn hrtf_root() -> std::path::PathBuf {
        std::env::var_os("SDA_HRTF_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("hrtf-assets"))
    }

    fn prepare_hrtf_convolver(&self, position: [f32; 3]) -> Result<convolution::StereoPartitionedConvolver, String> {
        let set = self.active_hrtf_set.clone().ok_or("native HRTF set is not configured")?;
        let local = spatial::head_relative_adm(position, self.head_pose);
        let direction = spatial::adm_to_spherical(local);
        let (_, _, left, right) = set.mixed_nearest(
            direction.azimuth as f64,
            direction.elevation as f64,
            self.hrtf_wet_weight,
        )?;
        convolution::StereoPartitionedConvolver::new(&left, &right, convolution::DEFAULT_PARTITION)
    }

    fn refresh_source_hrtf(&mut self, id: &str) -> Result<(), String> {
        let mut set = self.active_hrtf_set.take().ok_or("native HRTF set is not configured")?;
        let result = (|| {
            let source = self.sources.get_mut(id).ok_or("unknown source")?;
            let local = spatial::head_relative_adm(source.position, self.head_pose);
            let direction = spatial::adm_to_spherical(local);
            let selected_direction = set.nearest_direction(direction.azimuth as f64, direction.elevation as f64)?;
            if source.convolver.is_some() && source.hrtf_direction == Some(selected_direction) { return Ok(()); }
            let filter = set.prepared_direction(selected_direction.0, selected_direction.1, self.hrtf_wet_weight)?;
            if let Some(convolver) = &mut source.convolver {
                convolver.set_prepared_filter(filter);
            } else {
                let (left, right) = set.mixed_direction(selected_direction.0, selected_direction.1, self.hrtf_wet_weight)?;
                source.convolver = Some(convolution::StereoPartitionedConvolver::new(
                    &left, &right, convolution::DEFAULT_PARTITION,
                )?);
                source.convolver.as_mut().unwrap().set_prepared_filter(filter);
            }
            source.hrtf_direction = Some(selected_direction);
            source.input_block.fill(0.0);
            source.output_left.fill(0.0);
            source.output_right.fill(0.0);
            Ok(())
        })();
        self.active_hrtf_set = Some(set);
        result
    }

    fn refresh_all_hrtf(&mut self) -> Result<(), String> {
        let ids: Vec<String> = self
            .sources
            .keys()
            .filter(|id| id.starts_with("obj:"))
            .cloned()
            .collect();
        for id in ids {
            self.refresh_source_hrtf(&id)?;
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
            callback_fifo_underrun_frames: telemetry.callback_fifo_underrun_frames.load(Ordering::Relaxed),
            fifo_frames_available: fifo.available_read(),
            render_block_count: render_blocks,
            render_block_mean_micros: if render_blocks == 0 { 0 } else { render_total / render_blocks },
            render_block_max_micros: telemetry.render_block_max_micros.load(Ordering::Relaxed),
            output_sample_rate: self.output_sample_rate,
            output_channels: self.output_channels,
            paused: self.paused,
            reference_mix: self.active_hrtf_set.is_none(),
            output_active: self.output_active,
            hrtf_ready: self.active_hrtf_set.is_some(),
        }
    }

    /// Returns true only when every active source has PCM for a full render
    /// quantum. Rendering missing future samples into the output FIFO would turn
    /// a transient IPC delay into irreversible audible silence.
    fn can_render_quantum(&self, frames: usize) -> bool {
        (0..frames).all(|offset| {
            let at = self.sample_pos + offset as u64;
            self.sources.values().all(|source| {
                source.remove_at.is_some_and(|remove_at| at >= remove_at)
                    || source.gain == 0.0
                    || source.samples.has_at(at)
            })
        })
    }

    /// Render one preallocated stereo block on the dedicated render worker.
    /// This method never executes from the WASAPI callback.
    fn render_into(&mut self, output: &mut [f32], channels: usize) {
        output.fill(0.0);
        if self.paused || !self.output_active {
            return;
        }
        let mut underruns = 0_u64;
        for frame in output.chunks_exact_mut(channels) {
            let at = self.sample_pos;
            let block_index = self.block_offset;
            let mut left = 0.0_f32;
            let mut right = 0.0_f32;
            for source in self.sources.values_mut() {
                // Removal is sample-accurate even though HashMap cleanup is
                // deferred until the end of this callback.
                if source.remove_at.is_some_and(|remove_at| at >= remove_at) {
                    continue;
                }
                if let Some(event) = source.spatial_events.remove(&at) {
                    source.position = event.position;
                    source.spread = event.spread;
                }
                if let Some(event) = source.gain_events.remove(&at) {
                    source.target_gain = event.gain;
                    source.ramp_remaining = event.ramp.max(1);
                    source.ramp_step = (event.gain - source.gain) / source.ramp_remaining as f32;
                }
                if source.ramp_remaining > 0 {
                    source.gain += source.ramp_step;
                    source.ramp_remaining -= 1;
                    if source.ramp_remaining == 0 {
                        source.gain = source.target_gain;
                    }
                }
                let sample = match source.samples.take(at) {
                    Some(sample) => sample * source.gain,
                    None => {
                        if source.gain != 0.0 { underruns += 1; }
                        0.0
                    }
                };
                if source.convolver.is_some() {
                    left += source.output_left[block_index];
                    right += source.output_right[block_index];
                    source.input_block[block_index] = sample;
                    if block_index + 1 == convolution::DEFAULT_PARTITION {
                        source.output_left.fill(0.0);
                        source.output_right.fill(0.0);
                        let _ = source.convolver.as_mut().unwrap().process_block(
                            &source.input_block,
                            &mut source.output_left,
                            &mut source.output_right,
                        );
                    }
                } else {
                    // Bed/LFE routing remains direct until its native crossover
                    // stage is added; dynamic objects always use convolution.
                    left += sample;
                    right += sample;
                }
            }
            if channels >= 2 {
                frame[0] = left;
                frame[1] = right;
            } else if channels == 1 {
                frame[0] = 0.5 * (left + right);
            }
            self.sample_pos += 1;
            self.block_offset = (self.block_offset + 1) % convolution::DEFAULT_PARTITION;
        }
        self.underrun_samples += underruns;
        self.sources.retain(|_, source| {
            !source.remove_at.is_some_and(|remove_at| self.sample_pos >= remove_at)
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
    engine: Arc<Mutex<Engine>>,
    fifo: Arc<stereo_fifo::StereoFifo>,
    telemetry: Arc<RuntimeTelemetry>,
) {
    thread::Builder::new()
        .name("sda-native-render".into())
        .spawn(move || {
            let mut block = vec![0.0_f32; convolution::DEFAULT_PARTITION * 2];
            let mut observed_epoch = 0_u64;
            loop {
                if fifo.available_read() >= STEREO_FIFO_TARGET_FRAMES
                    || fifo.available_write() < convolution::DEFAULT_PARTITION
                {
                    thread::sleep(Duration::from_micros(500));
                    continue;
                }
                let mut state = match engine.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                if state.render_epoch != observed_epoch {
                    fifo.clear_from_producer();
                    telemetry.callback_output_enabled.store(false, Ordering::Release);
                    observed_epoch = state.render_epoch;
                }
                if !state.output_active || state.paused
                    || !state.can_render_quantum(convolution::DEFAULT_PARTITION)
                {
                    drop(state);
                    thread::sleep(Duration::from_micros(500));
                    continue;
                }
                let started = Instant::now();
                state.render_into(&mut block, 2);
                drop(state);
                if fifo.push(&block) != convolution::DEFAULT_PARTITION {
                    // A consumer-side race can only reduce free room after the
                    // capacity check. Discard this already-rendered quantum rather
                    // than block the worker or violate FIFO ownership.
                    continue;
                }
                telemetry.render_block_count.fetch_add(1, Ordering::Relaxed);
                if fifo.available_read() >= 2_048 {
                    telemetry.callback_output_enabled.store(true, Ordering::Release);
                }
                let elapsed = started.elapsed().as_micros() as u64;
                telemetry.render_block_total_micros.fetch_add(elapsed, Ordering::Relaxed);
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
        telemetry.callback_fifo_underrun_frames.fetch_add((requested - popped) as u64, Ordering::Relaxed);
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
                let popped = if output_enabled { fifo.pop_into_f32(data, channels) } else { data.fill(0.0); 0 };
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
                let popped = if output_enabled { fifo.pop_into_i16(data, channels) } else { data.fill(0); 0 };
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
                let popped = if output_enabled { fifo.pop_into_u16(data, channels) } else { data.fill(u16::MAX / 2); 0 };
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
    let engine = Arc::new(Mutex::new(Engine::new(
        config.sample_rate.0,
        config.channels,
    )));
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
    spawn_render_worker(engine.clone(), fifo.clone(), telemetry.clone());
    let stream = match build_stream(fifo.clone(), telemetry.clone(), &device, &config, supported.sample_format()) {
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
    if let Err(error) = protocol::read_frames(&mut stdin.lock(), &engine, &fifo, &telemetry) {
        write_event(&Event::Error {
            detail: format!("Native renderer protocol error: {error}"),
        });
    }
    drop(stream);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibrated_dense_assets_select_nearest_measured_direction() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf-dense/hrtf-set.json");
        let set = hrtf::NativeHrtfSet::load_calibrated(&root).unwrap();
        assert_eq!(set.sample_rate, 48_000);
        let ir = set.nearest(22.0, 0.0).unwrap();
        assert_eq!((ir.azimuth, ir.elevation), (20.0, 0.0));
        assert_eq!(ir.dry.len() % 2, 0);
        assert_eq!(ir.wet.len() % 2, 0);
    }

    #[test]
    fn legacy_ku100_subject_hybrid_is_rejected() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf-ku100-d2/hrtf-set.json");
        let error = hrtf::NativeHrtfSet::load_calibrated(&root).unwrap_err();
        assert!(error.contains("complete-subject"));
    }

    #[test]
    fn prepared_directions_reuse_state_when_grid_direction_is_unchanged() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let mut engine = Engine::new(48_000, 2);
        engine.active_hrtf_set = Some(hrtf::NativeHrtfSet::load_calibrated(&root).unwrap());
        engine.sources.insert("obj:1".into(), Source::default());
        engine.refresh_source_hrtf("obj:1").unwrap();
        let first = engine.sources["obj:1"].hrtf_direction;
        engine.head_pose = Some([0.0, 0.0, 0.05, 1.0]);
        engine.refresh_source_hrtf("obj:1").unwrap();
        assert_eq!(engine.sources["obj:1"].hrtf_direction, first);
        engine.sources.get_mut("obj:1").unwrap().position = [-1.0, 0.0, 0.0];
        engine.refresh_source_hrtf("obj:1").unwrap();
        assert_ne!(engine.sources["obj:1"].hrtf_direction, first);
    }

    #[test]
    fn calibrated_hrtf_mode_builds_the_dry_plus_residual_filter() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let set = hrtf::NativeHrtfSet::load_calibrated(&root).unwrap();
        let (_, _, left, right) = set.mixed_nearest(0.0, 0.0, 0.0).unwrap();
        let dry = set.nearest(0.0, 0.0).unwrap();
        let dry_len = dry.dry.len() / 2;
        assert!(left[..dry_len]
            .iter()
            .zip(&dry.dry[..dry_len])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-7));
        assert!(right[..dry_len]
            .iter()
            .zip(&dry.dry[dry_len..])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-7));
        let (_, _, wet_left, wet_right) = set.mixed_nearest(0.0, 0.0, 1.0).unwrap();
        let wet_len = dry.wet.len() / 2;
        assert!(wet_left[..wet_len]
            .iter()
            .zip(&dry.wet[..wet_len])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-7));
        assert!(wet_right[..wet_len]
            .iter()
            .zip(&dry.wet[wet_len..])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-7));
    }

    #[test]
    fn native_output_is_silent_until_ownership_is_explicitly_enabled() {
        let mut engine = Engine::new(48_000, 2);
        let mut source = Source {
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        source.samples.write(0, 0, &[1.0]);
        engine.sources.insert("obj:mute".into(), source);
        let mut out = [1.0; 2];
        engine.mix(&mut out, 2);
        assert_eq!(out, [0.0, 0.0]);
        assert_eq!(
            engine.sample_pos, 0,
            "muted ownership must not consume the codec clock"
        );
    }

    #[test]
    fn sources_mix_on_the_absolute_codec_clock() {
        let mut engine = Engine::new(48_000, 2);
        engine.output_active = true;
        let mut source = Source {
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        source.samples.write(10, 10, &[0.25, 0.5]);
        engine.sources.insert("obj:1".into(), source);
        engine.sample_pos = 10;
        let mut out = [0.0; 4];
        engine.mix(&mut out, 2);
        assert_eq!(out, [0.25, 0.25, 0.5, 0.5]);
        assert_eq!(engine.sample_pos, 12);
    }

    #[test]
    fn future_gain_event_does_not_retime_source_removal() {
        let mut engine = Engine::new(48_000, 2);
        engine.output_active = true;
        let mut source = Source {
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        source.samples.write(3, 3, &[1.0, 1.0]);
        source
            .gain_events
            .insert(4, GainEvent { gain: 0.0, ramp: 1 });
        engine.sources.insert("obj:7".into(), source);
        engine.sample_pos = 3;
        let mut out = [0.0; 4];
        engine.mix(&mut out, 2);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[2], 0.0);
        assert!(engine.sources.contains_key("obj:7"));
    }

    #[test]
    fn remove_boundary_is_sample_accurate() {
        let mut engine = Engine::new(48_000, 2);
        engine.output_active = true;
        let mut source = Source {
            gain: 1.0,
            target_gain: 1.0,
            remove_at: Some(9),
            ..Source::default()
        };
        source.samples.write(8, 8, &[1.0, 1.0]);
        engine.sources.insert("obj:9".into(), source);
        engine.sample_pos = 8;
        let mut out = [0.0; 4];
        engine.mix(&mut out, 2);
        assert_eq!(out, [1.0, 1.0, 0.0, 0.0]);
        assert!(!engine.sources.contains_key("obj:9"));
    }
}
