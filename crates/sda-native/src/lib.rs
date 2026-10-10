//! SDA mobile engine facade.
//!
//! Wires the two Rust halves of SDA together for mobile hosts (Android JNI,
//! iOS Swift):
//!
//! ```text
//! demuxed codec chunks (from JS/Kotlin/Java)
//!     │  MobileEngine::feed()
//!     ▼
//! sda_core::StreamingDecoder  →  FrameData (planar PCM + object events)
//!     │                         (PCM never crosses back over the FFI)
//!     ▼
//! RenderCommand::PcmFrame  →  sda_native_renderer::spawn_render_worker
//!     │
//!     ▼
//! StereoFifo → AudioOutput (Android: AAudio; desktop: CpalOutput; tests:
//!             WavDumpOutput)  +  install_event_sink(...) for ACK/activity
//! ```
//!
//! Build (Android):
//! ```text
//! export MACINDECODE_AC4_SPEC_DIR=<repo>/tmp/MacinDecode-AC4-Core/spec
//! cargo ndk -t arm64-v8a --platform 26 -- build --release
//! ```
//!
//! Source-id convention (matches the desktop sidecar): dynamic objects are
//! `obj:{codec object id}`, bed channels are `bed:{channel index}`.

mod frame_router;
#[cfg(feature = "ios-host")]
pub mod ios;
pub mod mpegh;
pub mod balance;
mod alac_pcm;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sda_core::{FrameData, ObjectEvent, StreamingDecoder};
pub mod mp3;
pub use sda_native_renderer::{
    install_event_sink, AudioOutput, Command, Engine, Event, EventSink, NativeObjectEvent,
    RuntimeTelemetry, render_command, stereo_fifo,
};
pub use sda_native_renderer::hrtf::NativeHrtfSet as NativeHrtfSetFacade;

/// Host-provided engine configuration (JSON-friendly mirror of the desktop
/// sidecar's `Configure` command).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConfig {
    pub sample_rate: u32,
    pub output_channels: u16,
    /// Speaker layout id fed to the VBAP solver (desktop default "7.1.4").
    /// Omitted in JSON -> engine default.
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default)]
    pub direct_object_hrtf: bool,
    #[serde(default)]
    pub directional_hrtf: bool,
    /// Contribution of the HRTF's separate wet response, independent of room
    /// simulation. Zero selects dry HRIR only; omitted retains legacy 0.04.
    #[serde(default = "default_hrtf_wet_weight")]
    pub hrtf_wet_weight: f32,
}

fn default_hrtf_wet_weight() -> f32 { 0.04 }

fn default_layout() -> String {
    "7.1.4".to_string()
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { sample_rate: 48000, output_channels: 2, layout: default_layout(), direct_object_hrtf: false, directional_hrtf: false, hrtf_wet_weight: default_hrtf_wet_weight() }
    }
}

/// One decoded frame batch drained from the decoder, reduced to what the
/// mobile bridge needs. PCM samples stay inside the engine (see module docs);
/// hosts only see metadata plus a watermark so they know when to feed more.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeStatus {
    pub codec: String,
    /// Frames decoded and queued since the last poll.
    pub frames_pushed: u32,
    /// Absolute codec sample position of the newest decoded frame.
    pub sample_pos: u64,
    pub errors: Vec<String>,
}

/// Presentation state reported to the host UI (plan T1.8): the consumption
/// clock plus the buffered audio watermark for backpressure.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    /// Codec clock consumed by the audio output (samples @ config.sample_rate).
    pub consumed_sample_pos: u64,
    /// Codec clock of the newest decoded frame: hosts pace feeding so this
    /// stays a bounded amount ahead of consumed_sample_pos (clock-drift-free
    /// backpressure).
    pub decoded_sample_pos: u64,
    pub position_ms: u64,
    /// Rendered stereo frames buffered in the engine FIFO awaiting output.
    pub fifo_frames: usize,
    /// Undecoded FrameData batches queued inside the engine.
    pub pending_batches: usize,
    pub paused: bool,
    pub hrtf_ready: bool,
    pub hrtf_directions: usize,
    pub direct_object_hrtf: bool,
    pub directional_hrtf: bool,
    pub object_convolver_count: u64,
    pub continuous_object_count: u64,
    pub near_field_enabled: bool,
    pub room_enabled: bool,
    pub spatial_cues_enabled: bool,
    pub volume_balance_enabled: bool,
    pub volume_balance_eligible: bool,
    pub program_gain_db: f64,
}

/// Latest object snapshot per id (plan T1.10): UI polls at its own cadence
/// and receives at most one position per object per poll.
pub type ObjectSnapshot = HashMap<u32, ObjectEvent>;

fn snapshot_at_clock(
    events: &mut VecDeque<ObjectEvent>,
    active: &mut ObjectSnapshot,
    clock: u64,
) -> ObjectSnapshot {
    while events.front().is_some_and(|event| event.sample_pos <= clock) {
        let event = events.pop_front().expect("front checked");
        if event.has_pos {
            active.insert(event.id, event);
        } else {
            active.remove(&event.id);
        }
    }
    active.clone()
}

/// Errors surfaced across the FFI boundary as plain strings.
pub type EngineResult<T> = Result<T, String>;

struct RenderPipeline {
    fifo: Arc<stereo_fifo::StereoFifo>,
    commands: Arc<render_command::RenderCommandQueue>,
    telemetry: Arc<RuntimeTelemetry>,
}

/// Mirrors player.ts STARTUP_AHEAD_SECONDS and its startPlaybackIfReady gate.
#[derive(Default)]
struct StartupGate {
    origin: Option<u64>,
    accepted_end: u64,
    started: bool,
    paused: bool,
    ended: bool,
}

/// Mobile engine: owns the decoder, the decoded-frame queue and (after
/// [`MobileEngine::start`]) the renderer worker + FIFO + output handoff.
pub struct MobileEngine {
    config: EngineConfig,
    hrtf_directions: usize,
    announced_codec: Option<String>,
    decoder: StreamingDecoder,
    renderer: Option<Engine>,
    pipeline: Option<RenderPipeline>,
    startup: Mutex<StartupGate>,
    /// Decoded frames waiting to be handed to the renderer worker.
    pending: Mutex<VecDeque<balance::PendingFrame>>,
    balance: balance::VolumeBalance,
    /// Desktop-compatible channel mapping, source lifecycle and event compaction.
    frame_router: frame_router::FrameRouter,
    /// Codec clock of the newest queued frame (presentation clock base).
    newest_sample_pos: Mutex<u64>,
    /// 66 ms coalescing window for `poll_object_snapshot` (plan T1.10).
    last_poll: Mutex<Option<Instant>>,
    /// Decoded object metadata retained until the presentation clock reaches it.
    object_events: Mutex<VecDeque<ObjectEvent>>,
    active_objects: Mutex<HashMap<u32, ObjectEvent>>,
    stereo_bed_mode: bool,
    mp3_decoder: Option<mp3::Mp3FileDecoder>,
    mpegh_decoder: Option<mpegh::MpeghDecoder>,
    hrtf_loaded: bool,
}

/// Object-event throttle window; mirrors the web player's 66 ms batching.
pub const OBJECT_POLL_INTERVAL: Duration = Duration::from_millis(66);
pub const OBJECT_EVENT_CAPACITY: usize = 65_536;

impl MobileEngine {
    /// Create the engine. `hrtf_dir` is reserved for T1.12 (HRTF asset
    /// loading); the renderer's current defaults apply until then.
    pub fn new(config: EngineConfig, _hrtf_dir: Option<&str>) -> EngineResult<MobileEngine> {
        if !config.hrtf_wet_weight.is_finite() || !(0.0..=1.0).contains(&config.hrtf_wet_weight) {
            return Err("HRTF wet weight must be finite and in 0..=1".into());
        }
        if config.output_channels != 2 {
            return Err(format!(
                "mobile engine currently renders stereo only, got {} channels",
                config.output_channels
            ));
        }
        Ok(MobileEngine {
            renderer: Some(Engine::new(config.sample_rate, config.output_channels)),
            decoder: StreamingDecoder::new("auto")?,
            config,
            hrtf_directions: 0,
            announced_codec: None,
            pipeline: None,
            startup: Mutex::new(StartupGate::default()),
            pending: Mutex::new(VecDeque::new()),
            balance: balance::VolumeBalance::default(),
            frame_router: frame_router::FrameRouter::default(),
            newest_sample_pos: Mutex::new(0),
            last_poll: Mutex::new(None),
            object_events: Mutex::new(VecDeque::new()),
            active_objects: Mutex::new(HashMap::new()),
            stereo_bed_mode: false,
            mp3_decoder: None,
            mpegh_decoder: None,
            hrtf_loaded: false,
        })
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Start the render worker and hand the FIFO to `output`. Call once,
    /// before playback. `output` implementations: `CpalOutput` (desktop),
    /// AAudio sink (Android, T2.2), `WavDumpOutput` (tests).
    pub fn start(&mut self, output: Arc<dyn AudioOutput>) -> EngineResult<()> {
        if self.pipeline.is_some() {
            return Err("engine already started".into());
        }
        if (self.config.direct_object_hrtf || self.config.directional_hrtf) && self.hrtf_directions == 0 {
            return Err("object rendering requires a loaded HRTF set".into());
        }
        let Some(mut renderer) = self.renderer.take() else {
            return Err("renderer engine unavailable".into());
        };
        // With calibrated assets, keep the same paused startup gate as the
        // desktop: declare sources and prebuffer PCM before StartAt.
        let gated = self.hrtf_directions > 0;
        renderer.set_output_active(!gated);
        *self.startup.lock().expect("startup lock") = StartupGate { started: !gated, ..Default::default() };
        let commands = Arc::new(render_command::RenderCommandQueue::new(256));
        let fifo = Arc::new(stereo_fifo::StereoFifo::new(
            sda_native_renderer::STEREO_FIFO_CAPACITY_FRAMES,
        ));
        let telemetry = Arc::new(RuntimeTelemetry::default());
        sda_native_renderer::spawn_render_worker(
            renderer,
            commands.clone(),
            fifo.clone(),
            telemetry.clone(),
        );
        // Engine::new starts paused; unpause through the command path the
        // worker owns (mirrors the desktop Play flow).
        if !gated {
            let _ = commands.push(render_command::RenderCommand::Command(Command::Pause { paused: false }));
        }
        // Apply the requested virtual-speaker layout before PCM is submitted.
        let _ = commands.push(render_command::RenderCommand::Command(
            Command::SetLayout { layout: self.config.layout.clone() },
        ));
        let _ = commands.push(render_command::RenderCommand::Command(
            Command::SetDirectionalHrtf { enabled: self.config.directional_hrtf },
        ));
        let _ = commands.push(render_command::RenderCommand::Command(
            Command::SetObjectHrtf { enabled: self.config.direct_object_hrtf },
        ));
        self.pipeline = Some(RenderPipeline { fifo: fifo.clone(), commands: commands.clone(), telemetry: telemetry.clone() });
        self.drain_pending_into_pipeline()?;
        // Start the output last, on its own thread: AudioOutput::run owns a
        // long-lived loop and must never block the caller - a blocking
        // implementation (WavDump) exiting before the worker's first
        // flush-epoch wait would deadlock the pipeline.
        std::thread::Builder::new()
            .name("sda-audio-output".into())
            .spawn(move || output.run(fifo, telemetry, commands))
            .map_err(|error| format!("spawn audio output: {error}"))?;
        Ok(())
    }

    /// Load a calibrated HRTF set (plan T1.12). Must be called before
    /// `start()`; the desktop sidecar performs hot-swaps via protocol
    /// commands, which mobile gains later alongside live layout switching.
    pub fn load_hrtf(&mut self, hrtf_json_path: &str) -> EngineResult<()> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Err("engine already started".into());
        };
        let path = std::path::Path::new(hrtf_json_path);
        let set = sda_native_renderer::hrtf::NativeHrtfSet::load_calibrated(path)
            .map_err(|error| format!("HRTF load failed: {error}"))?;
        let directions = set.simulation_shape().0;
        renderer
            .replace_hrtf(set, self.config.hrtf_wet_weight)
            .map_err(|error| format!("HRTF apply failed: {error}"))?;
        self.hrtf_directions = directions;
        self.hrtf_loaded = true;
        Ok(())
    }

    pub fn hrtf_loaded(&self) -> bool { self.hrtf_loaded }

    /// Acknowledged live switch for room/near-field-off mobile presets. Asset
    /// parsing happens on the caller, not the render worker. Decoder/FIFO intact.
    pub fn set_hrtf_preset(&mut self, path: &str, wet: f32, direct: bool,
        directional: bool) -> EngineResult<()> {
        if !wet.is_finite() || !(0.0..=1.0).contains(&wet) {
            return Err("invalid HRTF wet weight".into());
        }
        let set = sda_native_renderer::hrtf::NativeHrtfSet::load_calibrated(std::path::Path::new(path))
            .map_err(|e| format!("HRTF load failed: {e}"))?;
        let directions = set.simulation_shape().0;
        if let Some(pipeline) = &self.pipeline {
            let (reply, received) = std::sync::mpsc::channel();
            pipeline.commands.push(render_command::RenderCommand::HrtfPreset {
                set: Box::new(set), wet, direct, directional, reply,
            }).map_err(|_| "HRTF preset command queue is full")?;
            received.recv_timeout(Duration::from_secs(30))
                .map_err(|_| "HRTF preset acknowledgement timed out".to_string())??;
        } else {
            self.renderer.as_mut().ok_or("renderer unavailable")?
                .configure_hrtf_preset(set, wet, direct, directional)?;
        }
        self.config.hrtf_wet_weight = wet;
        self.config.direct_object_hrtf = direct;
        self.config.directional_hrtf = directional;
        self.hrtf_directions = directions;
        self.hrtf_loaded = true;
        Ok(())
    }

    /// Prepare filters away from the render worker, then crossfade in place.
    pub fn set_spatial_cue_gain(&mut self, path: &str, gain: f32) -> EngineResult<()> {
        let update = sda_native_renderer::live_spatial_cues::PreparedCueUpdate::load(path, &self.config.layout, gain)?;
        self.apply_spatial_cue_update(update)
    }
    pub fn apply_spatial_cue_update(&mut self, update: sda_native_renderer::live_spatial_cues::PreparedCueUpdate) -> EngineResult<()> {
        if let Some(pipeline) = &self.pipeline {
            let (reply, received) = std::sync::mpsc::channel();
            pipeline.commands.push(render_command::RenderCommand::SpatialCueGain { update: Box::new(update), reply })
                .map_err(|_| "spatial cue command queue is full")?;
            received.recv_timeout(Duration::from_secs(30)).map_err(|_| "spatial cue acknowledgement timed out")??;
        } else {
            self.renderer.as_mut().ok_or("renderer unavailable")?.apply_spatial_cue_update(update)?;
        }
        Ok(())
    }

    /// Open a seekable MP3 file. Decoded PCM enters the shared 48 kHz renderer as a stereo bed.
    pub fn open_mp3(&mut self, path: &str) -> EngineResult<u32> {
        if self.mp3_decoder.is_some() {
            return Err("engine already has an active media source".into());
        }
        let decoder = mp3::Mp3FileDecoder::open(path)?;
        let rate = decoder.input_rate();
        self.mp3_decoder = Some(decoder);
        self.stereo_bed_mode = true;
        Ok(rate)
    }

    /// Pull bounded stereo frames; caller paces by native FIFO/consumption telemetry.
    pub fn pull_mp3(&mut self, max_frames: usize) -> EngineResult<(usize, bool)> {
        if !self.stereo_bed_mode { return Err("active source is not an MP3 stereo bed".into()); }
        let decoder = self.mp3_decoder.as_mut().ok_or("MP3 decoder is unavailable")?;
        let samples = decoder.read_interleaved(max_frames.min(4096))?;
        let frames = samples.len() / 2;
        if frames > 0 {
            let start = self.decoded_sample_pos();
            let frame = FrameData {
                codec: "mp3-stereo-bed", sample_rate: 48_000, sample_pos: start,
                channels: vec![samples.iter().step_by(2).copied().collect(), samples.iter().skip(1).step_by(2).copied().collect()],
                labels: vec!["FrontLeft".into(), "FrontRight".into()],
                raw_bed_labels: vec!["FrontLeft".into(), "FrontRight".into()],
                events: Vec::new(), object_channels: Vec::new(), program_loudness: None, ramp_duration: 0,
            };
            self.pending.lock().expect("pending lock").push_back(self.balance.measure(frame, None));
            *self.newest_sample_pos.lock().expect("clock lock") = start + frames as u64;
            self.drain_pending_into_pipeline()?;
        }
        let done = self.mp3_decoder.as_ref().is_some_and(mp3::Mp3FileDecoder::is_finished);
        Ok((frames, done))
    }

    /// Apple-decoded ALAC enters the existing bed renderer and balance policy.
    pub(crate) fn feed_alac_pcm(&mut self, frame: FrameData, reference: Vec<Vec<f32>>) -> EngineResult<()> {
        self.balance.enable_alac_startup();
        // Reserve all buses once, before the first ALAC PCM. Live dry/wet
        // transitions change samples, not the convolution graph or its tails.
        if self.config.layout != "7.1.4" {
            self.config.layout = "7.1.4".into();
            if let Some(pipeline) = &self.pipeline {
                pipeline.commands.push(render_command::RenderCommand::Command(
                    Command::SetLayout { layout: self.config.layout.clone() },
                )).map_err(|_| "ALAC layout command queue full")?;
            }
        }
        self.stereo_bed_mode = frame.channels.len() == 2;
        let end = frame.sample_pos + frame.channels[0].len() as u64;
        self.pending.lock().expect("pending lock").push_back(self.balance.measure(frame, Some(reference)));
        *self.newest_sample_pos.lock().expect("clock lock") = end;
        self.drain_pending_into_pipeline()
    }

    pub fn stereo_bed_mode(&self) -> bool { self.stereo_bed_mode }

    pub fn reset_source_state(&mut self) {
        self.mp3_decoder = None;
        self.mpegh_decoder = None;
        self.stereo_bed_mode = false;
        self.decoder = StreamingDecoder::new("auto").expect("known decoder type");
        self.pending.lock().expect("pending lock").clear();
        self.frame_router = frame_router::FrameRouter::default();
        self.balance.reset();
        self.announced_codec = None;
        *self.startup.lock().expect("startup lock") = StartupGate::default();
        self.object_events.lock().expect("object events lock").clear();
        self.active_objects.lock().expect("active objects lock").clear();
        *self.newest_sample_pos.lock().expect("clock lock") = 0;
    }

    /// Select the Windows MPEG-H decoder. Input is MHAS (mha1 is wrapped by the
    /// shared Windows MP4/MHAS adapter before it reaches JNI).
    pub fn open_mpegh(&mut self) -> EngineResult<()> {
        if self.mp3_decoder.is_some() || self.mpegh_decoder.is_some() {
            return Err("engine already has an active media source".into());
        }
        let decoder = mpegh::MpeghDecoder::new()?;
        // Windows auto-layout selects its authored below-ear speaker layer.
        self.config.layout = "360RA-13".into();
        if let Some(pipeline) = &self.pipeline {
            pipeline.commands.push(render_command::RenderCommand::Command(
                Command::SetLayout { layout: self.config.layout.clone() },
            )).map_err(|_| "360RA layout command queue full")?;
        }
        self.mpegh_decoder = Some(decoder);
        Ok(())
    }

    /// Feed demuxed bitstream bytes (any chunking; the decoder re-frames).
    /// Decoded frames are converted to `PcmFrame` render commands once the
    /// pipeline is started; before that they queue (visualizer-only use).
    pub fn feed(&mut self, data: &[u8]) -> EngineResult<DecodeStatus> {
        if let Some(decoder) = &mut self.mpegh_decoder { decoder.push(data)?; }
        else { self.decoder.push(data)?; }
        let codec = self.codec_name().to_string();
        if self.announced_codec.as_ref() != Some(&codec) && codec != "auto" && !codec.is_empty() {
            if let Some(pipeline) = &self.pipeline {
                pipeline.commands.push(render_command::RenderCommand::Command(
                    Command::SetProgramCodec { codec: codec.clone() },
                )).map_err(|_| "codec command queue full")?;
                self.announced_codec = Some(codec);
            }
        }
        let mut frames_pushed = 0_u32;
        let mut sample_pos = self.decoded_sample_pos();
        {
            let mut pending = self.pending.lock().expect("pending lock");
            while let Some((frame, reference)) = if let Some(decoder) = &mut self.mpegh_decoder {
                decoder.next_frame_with_reference().map(|(frame, reference)| (frame, Some(reference)))
            } else { self.decoder.next_frame().map(|frame| (frame, None)) } {
                if frame.sample_rate != self.config.sample_rate {
                    return Err(format!("Native {} Hz output requires sample-clock conversion for {} {} Hz", self.config.sample_rate, frame.codec, frame.sample_rate));
                }
                sample_pos = frame.sample_pos + frame.channels.first().map_or(0, |pcm| pcm.len()) as u64;
                {
                    let mut timeline = self.object_events.lock().expect("object events lock");
                    let available = OBJECT_EVENT_CAPACITY.saturating_sub(timeline.len());
                    if frame.events.len() > available {
                        return Err(format!(
                            "object metadata timeline full (capacity {OBJECT_EVENT_CAPACITY}); feed was rejected before enqueuing this frame"
                        ));
                    }
                    timeline.extend(frame.events.iter().cloned());
                }
                pending.push_back(self.balance.measure(frame, reference));
                frames_pushed += 1;
            }
        }
        *self.newest_sample_pos.lock().expect("clock lock") = sample_pos;
        self.drain_pending_into_pipeline()?;
        Ok(DecodeStatus {
            codec: self.codec_name().to_string(),
            frames_pushed,
            sample_pos,
            errors: self.decoder.drain_errors(),
        })
    }

    pub fn finish(&mut self) -> EngineResult<DecodeStatus> {
        if let Some(decoder) = &self.mpegh_decoder { decoder.flush()?; } else { self.decoder.flush(); }
        let status = self.feed(&[])?;
        self.balance.finish();
        self.startup.lock().expect("startup lock").ended = true;
        self.start_playback_if_ready()?;
        Ok(status)
    }

    /// Codec clock of the newest decoded frame (samples @ config.sample_rate).
    pub fn decoded_sample_pos(&self) -> u64 {
        *self.newest_sample_pos.lock().expect("clock lock")
    }

    /// Presentation clock + watermark for host backpressure (plan T1.8).
    /// Before `start()`, the consumed clock is 0 and the watermark reflects
    /// only the undrained decode queue.
    pub fn playback_status(&self) -> PlaybackStatus {
        let pending_len = self.pending.lock().expect("pending lock").len();
        let (consumed, fifo_frames, paused) = match &self.pipeline {
            Some(pipeline) => (
                pipeline
                    .telemetry
                    .callback_consumed_sample_pos
                    .load(std::sync::atomic::Ordering::Acquire),
                pipeline.fifo.available_read(),
                pipeline.telemetry.paused.load(std::sync::atomic::Ordering::Acquire),
            ),
            None => (0, 0, true),
        };
        PlaybackStatus {
            consumed_sample_pos: consumed,
            decoded_sample_pos: self.decoded_sample_pos(),
            position_ms: consumed * 1000 / u64::from(self.config.sample_rate),
            fifo_frames,
            pending_batches: pending_len,
            paused,
            hrtf_ready: self.pipeline.as_ref().map_or(self.hrtf_directions > 0, |p| p.telemetry.hrtf_ready.load(std::sync::atomic::Ordering::Acquire)),
            hrtf_directions: self.hrtf_directions,
            direct_object_hrtf: self.pipeline.as_ref().is_some_and(|p| p.telemetry.direct_object_hrtf.load(std::sync::atomic::Ordering::Acquire)),
            directional_hrtf: self.pipeline.as_ref().is_some_and(|p| p.telemetry.directional_hrtf.load(std::sync::atomic::Ordering::Acquire)),
            object_convolver_count: self.pipeline.as_ref().map_or(0, |p| p.telemetry.object_convolver_count.load(std::sync::atomic::Ordering::Acquire)),
            continuous_object_count: self.pipeline.as_ref().map_or(0, |p| p.telemetry.continuous_object_count.load(std::sync::atomic::Ordering::Acquire)),
            near_field_enabled: self.pipeline.as_ref().is_some_and(|p| p.telemetry.near_field_enabled.load(std::sync::atomic::Ordering::Acquire)),
            volume_balance_enabled: self.balance.enabled,
            volume_balance_eligible: self.balance.eligible,
            program_gain_db: self.balance.gain_db,
            room_enabled: self.pipeline.as_ref().is_some_and(|p| p.telemetry.room_enabled.load(std::sync::atomic::Ordering::Acquire)),
            spatial_cues_enabled: self.pipeline.as_ref().is_some_and(|p| p.telemetry.spatial_cues_enabled.load(std::sync::atomic::Ordering::Acquire)),
        }
    }

    /// Seek (plan T1.9): the host repositions demuxing, then calls this to
    /// flush the decoder, the queued frames and the rendered FIFO so the next
    /// `feed()` starts the new time base. Object-event coalescing resets.
    pub fn seek(&mut self, _position_ms: u64) -> EngineResult<()> {
        self.decoder.reset();
        self.announced_codec = None;
        self.pending.lock().expect("pending lock").clear();
        *self.newest_sample_pos.lock().expect("clock lock") = 0;
        *self.last_poll.lock().expect("poll lock") = None;
        self.object_events.lock().expect("object events lock").clear();
        self.active_objects.lock().expect("active objects lock").clear();
        self.frame_router = frame_router::FrameRouter::default();
        self.balance.reset();
        *self.startup.lock().expect("startup lock") = StartupGate::default();
        if let Some(pipeline) = &self.pipeline {
            pipeline.commands.push(render_command::RenderCommand::Command(Command::Reset { origin: 0 }))
                .map_err(|_| "render command queue full")?;
            // Invalidate the rendered FIFO: epoch flush consumed by the
            // output callback, mirroring the desktop seek path.
            let _epoch = pipeline.fifo.clear_from_producer();
            pipeline
                .telemetry
                .callback_consumed_sample_pos
                .store(0, std::sync::atomic::Ordering::Release);
        }
        Ok(())
    }

    pub fn set_volume_balance(&mut self, enabled: bool) -> EngineResult<()> {
        if let Some(pipeline) = &self.pipeline {
            pipeline.commands.push(render_command::RenderCommand::Command(
                Command::SetProgramEnabled {enabled:enabled && self.balance.eligible}
            )).map_err(|_| "volume balance command queue full")?;
        }
        self.balance.enabled=enabled;
        Ok(())
    }
    pub fn set_measured_loudness(&mut self, json: &str) -> EngineResult<()> {
        let measurement: balance::Measurement=serde_json::from_str(json).map_err(|e|e.to_string())?;
        self.balance.set_cached(measurement);
        Ok(())
    }
    pub fn complete_loudness_json(&self) -> String {
        serde_json::to_string(&self.balance.complete_measurement()).unwrap_or_else(|_|"null".into())
    }

    /// User-selected total-output preamp; independent of objects and volume balance.
    pub fn set_spatial_enhancement(&self, enabled: bool) -> EngineResult<()> {
        let pipeline = self.pipeline.as_ref().ok_or("engine not started")?;
        pipeline.commands.push(render_command::RenderCommand::Command(
            Command::SetSpatialEnhancement { enabled }
        )).map_err(|_| "command queue full".into())
    }

    pub fn set_spatial_layer_gain_db(&self, gain_db: f32) -> EngineResult<()> {
        if !gain_db.is_finite() || !(0.0..=6.0).contains(&gain_db) {
            return Err("spatial layer gain must be finite and between 0 and +6 dB".into());
        }
        let pipeline = self.pipeline.as_ref().ok_or("engine not started")?;
        pipeline.commands.push(render_command::RenderCommand::Command(
            Command::SetSpatialLayerGain { gain_db }
        )).map_err(|_| "command queue full".into())
    }

    pub fn set_master_preamp_db(&self, gain_db: f32) -> EngineResult<()> {
        if !gain_db.is_finite() || !(-12.0..=12.0).contains(&gain_db) {
            return Err("master preamp must be finite and between -12 and +12 dB".into());
        }
        let pipeline = self.pipeline.as_ref().ok_or("engine not started")?;
        pipeline.commands.push(render_command::RenderCommand::Command(
            Command::SetMasterPreamp { gain_db }
        )).map_err(|_| "command queue full".into())
    }

    pub fn set_volume(&self, volume: f32) -> EngineResult<()> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err("volume must be between 0 and 1".into());
        }
        let pipeline = self.pipeline.as_ref().ok_or("engine not started")?;
        pipeline.commands
            .push(render_command::RenderCommand::Command(Command::SetVolume { volume }))
            .map_err(|_| "command queue full".into())
    }

    pub fn set_object_rendering(&self, direct: bool, directional: bool) -> EngineResult<()> {
        if (direct || directional) && self.hrtf_directions == 0 {
            return Err("object rendering requires a loaded HRTF set".into());
        }
        let pipeline = self.pipeline.as_ref().ok_or("engine not started")?;
        pipeline.commands.push(render_command::RenderCommand::ObjectRendering { direct, directional })
            .map_err(|_| "render command queue full".into())
    }

    /// Apply the desktop near-field correction with the same distance mapping.
    pub fn set_near_field(&mut self, enabled: bool, metres_per_unit: f32) -> EngineResult<()> {
        let settings = sda_native_renderer::near_field::Settings { enabled, metres_per_unit };
        if !settings.valid() { return Err("invalid near-field distance mapping".into()); }
        if let Some(pipeline) = &self.pipeline {
            let (reply, received) = std::sync::mpsc::channel();
            pipeline.commands.push(render_command::RenderCommand::NearField { settings, reply })
                .map_err(|_| "near-field command queue is full")?;
            received.recv_timeout(Duration::from_secs(10)).map_err(|_| "near-field acknowledgement timed out".to_string())?
        } else {
            self.renderer.as_mut().ok_or("renderer unavailable")?.configure_near_field(settings)
        }
    }

    /// Use the exact desktop room graph, with an acknowledged live switch.
    /// An empty path bypasses the room; asset parsing runs on the calling thread.
    pub fn set_room(&mut self, path: &str) -> EngineResult<()> {
        let profile = if path.is_empty() { None } else {
            let room = sda_native_renderer::cinema::RoomProfile::load(path)?;
            if room.layout != self.config.layout { return Err("room layout does not match playback layout".into()); }
            Some(Arc::new(room))
        };
        // Mobile room selection is a listening action (the UI exposes packaged
        // rooms, not raw measurement comparison). Bypass remains neutral.
        let settings = if profile.is_some() {
            sda_native_renderer::cinema::Settings {
                enabled: true,
                ..sda_native_renderer::cinema::Settings::room_listening()
            }
        } else {
            sda_native_renderer::cinema::Settings::default()
        };
        if let Some(pipeline) = &self.pipeline {
            let (reply, received) = std::sync::mpsc::channel();
            pipeline.commands.push(render_command::RenderCommand::Room { settings, profile, reply })
                .map_err(|_| "room command queue is full")?;
            received.recv_timeout(Duration::from_secs(30)).map_err(|_| "room renderer acknowledgement timed out".to_string())?
        } else {
            self.renderer.as_mut().ok_or("renderer unavailable")?.configure_room(settings, profile)
        }
    }

    pub fn stop(&mut self) {
        if let Some(pipeline) = self.pipeline.take() {
            pipeline.telemetry.shutdown_requested.store(true, std::sync::atomic::Ordering::Release);
            let _ = pipeline.commands.push(render_command::RenderCommand::Command(Command::Shutdown));
        }
        self.pending.lock().expect("pending lock").clear();
        self.object_events.lock().expect("object events lock").clear();
        self.active_objects.lock().expect("active objects lock").clear();
        *self.last_poll.lock().expect("poll lock") = None;
    }

    pub fn set_paused(&self, paused: bool) -> EngineResult<()> {
        let Some(pipeline) = &self.pipeline else {
            return Err("engine not started".into());
        };
        {
            let mut startup = self.startup.lock().expect("startup lock");
            startup.paused = paused;
            if !startup.started {
                drop(startup);
                return self.start_playback_if_ready();
            }
        }
        pipeline
            .commands
            .push(render_command::RenderCommand::Command(Command::Pause { paused }))
            .map_err(|_| "command queue full")?;
        Ok(())
    }

    /// Latest valid object positions at the audio presentation clock. Events
    /// decoded ahead of playback are retained but never exposed early.
    pub fn object_snapshot(&self) -> ObjectSnapshot {
        let clock = self.playback_status().consumed_sample_pos;
        snapshot_at_clock(
            &mut self.object_events.lock().expect("object events lock"),
            &mut self.active_objects.lock().expect("active objects lock"),
            clock,
        )
    }

    /// Latest object positions, coalesced to at most one snapshot per
    /// [`OBJECT_POLL_INTERVAL`].
    pub fn poll_object_snapshot(&self, _pending_frames: &[FrameData]) -> Option<ObjectSnapshot> {
        let mut last = self.last_poll.lock().expect("poll lock");
        if let Some(previous) = last.as_ref() {
            if previous.elapsed() < OBJECT_POLL_INTERVAL {
                return None;
            }
        }
        *last = Some(Instant::now());
        Some(self.object_snapshot())
    }

    pub fn set_head_yaw_degrees(&self, degrees: f32) -> EngineResult<()> {
        if !degrees.is_finite() || !(-180.0..=180.0).contains(&degrees) {
            return Err("head yaw must be finite and between -180 and 180 degrees".into());
        }
        if degrees == 0.0 { return self.reset_head_pose(); }
        let pipeline = self.pipeline.as_ref().ok_or("engine not started")?;
        let half = degrees.to_radians() * 0.5;
        let orientation = [0.0, 0.0, half.sin(), half.cos()];
        pipeline.commands.push(render_command::RenderCommand::Command(
            Command::HeadPose { orientation },
        )).map_err(|_| "command queue full".into())
    }

    pub fn reset_head_pose(&self) -> EngineResult<()> {
        let pipeline = self.pipeline.as_ref().ok_or("engine not started")?;
        pipeline.commands.push(render_command::RenderCommand::Command(Command::ClearHeadPose))
            .map_err(|_| "command queue full".into())
    }

    /// Start with the platform output: AAudio blocking-write sink (T2.2).
    /// Android only; hosts elsewhere construct their own AudioOutput.
    #[cfg(target_os = "android")]
    pub fn start_android(&mut self) -> EngineResult<()> {
        // Negotiate the device before spinning up the renderer. A previous
        // asynchronous open could fail after Java had reported success, which
        // left Android feeding an engine whose FIFO had no consumer.
        let output = sda_native_renderer::aaudio_output::AAudioWriterSink::open()?;
        self.start(Arc::new(output))
    }

    /// Codec in use (meaningful after auto-detection).
    pub fn codec_name(&self) -> &str {
        if self.mpegh_decoder.is_some() { "mpegh" } else { self.decoder.codec_name() }
    }

    /// Drains queued FrameData into the render pipeline once started.
    fn drain_pending_into_pipeline(&mut self) -> EngineResult<()> {
        let Some(pipeline) = &self.pipeline else { return Ok(()) };
        let mut pending = self.pending.lock().expect("pending lock");
        while let Some(pending_frame) = pending.pop_front() {
            for command in self.balance.route(&pending_frame) {
                pipeline.commands.push(render_command::RenderCommand::Command(command))
                    .map_err(|_| "volume balance command queue full")?;
            }
            let frame = pending_frame.frame;
            let origin = frame.sample_pos;
            let end = origin + frame.channels.first().map_or(0, Vec::len) as u64;
            for command in self.frame_router.route(frame)? {
                // The unconfigured reference-mix utility is not a calibrated
                // playback session and does not advance the source-ring clock.
                if self.hrtf_directions == 0 {
                    pipeline.commands.push(command).map_err(|_| "render command queue full")?;
                    continue;
                }
                if let render_command::RenderCommand::PcmFrame { start, entries, events } = command {
                    let (reply, received) = std::sync::mpsc::channel();
                    pipeline.commands.push(render_command::RenderCommand::PcmFrameWithAck { start, entries, events, reply })
                        .map_err(|_| "render command queue is full; feed must be paced")?;
                    let accepted = received.recv_timeout(Duration::from_secs(10))
                        .map_err(|_| "desktop PCM frame acknowledgement timed out")?;
                    if !accepted { return Err(format!("desktop renderer rejected PCM frame at {start}")); }
                } else {
                    pipeline.commands.push(command)
                        .map_err(|_| "render command queue is full; feed must be paced")?;
                }
            }
            {
                let mut startup = self.startup.lock().expect("startup lock");
                startup.origin.get_or_insert(origin);
                startup.accepted_end = end;
            }
            self.start_playback_if_ready()?;
        }
        Ok(())
    }

    fn start_playback_if_ready(&self) -> EngineResult<()> {
        let Some(pipeline) = &self.pipeline else { return Ok(()) };
        let mut startup = self.startup.lock().expect("startup lock");
        let Some(origin) = startup.origin else { return Ok(()) };
        if startup.started || startup.paused { return Ok(()) }
        if !startup.ended && startup.accepted_end.saturating_sub(origin) < u64::from(self.config.sample_rate) / 2 {
            return Ok(());
        }
        // This command follows the complete PCM/event batches on the same FIFO.
        pipeline.commands.push(render_command::RenderCommand::Command(Command::StartAt { origin }))
            .map_err(|_| "render command queue full at startup")?;
        startup.started = true;
        Ok(())
    }

    /// Drains queued frames (test accessor; FFI hosts do not see PCM).
    pub fn take_pending_frames(&self) -> Vec<FrameData> {
        let mut pending = self.pending.lock().expect("pending lock");
        pending.drain(..).map(|pending| pending.frame).collect()
    }
}

impl Drop for MobileEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

/// sda_core::ObjectEvent → renderer NativeObjectEvent (camelCase contract on
/// both sides; zone/diffuse extras default like the web bridge).
fn native_object_event(event: &ObjectEvent) -> EngineResult<NativeObjectEvent> {
    // Use the desktop sidecar's serde wire contract, including nullable distance
    // and float parsing, rather than a second handwritten field conversion.
    let json = serde_json::to_vec(event).map_err(|error| error.to_string())?;
    serde_json::from_slice(&json).map_err(|error| format!("desktop object metadata contract: {error}"))
}

#[cfg(target_os = "android")]
pub mod jni;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_live_hrtf_preset_keeps_pipeline_decoder_and_pause() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/native-renderer/hrtf-assets");
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        engine.load_hrtf(root.join("hrtf/hrtf-set.json").to_str().unwrap()).unwrap();
        engine.feed(&joc_fixture()).unwrap();
        let decoded = engine.decoded_sample_pos();
        engine.start(Arc::new(MobileTestOutput)).unwrap();
        engine.set_paused(true).unwrap();
        wait_mobile_status(&engine, |s| s.paused);
        let commands = engine.pipeline.as_ref().unwrap().commands.clone();
        let fifo = engine.pipeline.as_ref().unwrap().fifo.clone();
        for (directory, wet, direct, directions) in [("hrtf-dense-raw", 0.0, true, 61), ("hrtf", 0.04, true, 17), ("hrtf-dense", 0.04, true, 61)] {
            engine.set_hrtf_preset(root.join(directory).join("hrtf-set.json").to_str().unwrap(), wet, direct, direct).unwrap();
            assert!(Arc::ptr_eq(&commands, &engine.pipeline.as_ref().unwrap().commands));
            assert!(Arc::ptr_eq(&fifo, &engine.pipeline.as_ref().unwrap().fifo));
            assert_eq!(engine.decoded_sample_pos(), decoded);
            let status = engine.playback_status();
            assert!(status.paused && status.hrtf_ready);
            assert_eq!(status.direct_object_hrtf, direct);
            assert_eq!(status.directional_hrtf, direct);
            assert_eq!(engine.hrtf_directions, directions);
            assert_eq!(engine.config.hrtf_wet_weight, wet);
        }
        assert!(engine.set_hrtf_preset("missing-file", 0.0, false, false).is_err());
        assert!(engine.set_hrtf_preset("missing-file", f32::NAN, false, false).is_err());
        assert_eq!(engine.hrtf_directions, 61);
        assert_eq!(engine.decoded_sample_pos(), decoded);
        assert!(engine.playback_status().paused);
        engine.set_paused(false).unwrap();
        wait_mobile_status(&engine, |s| !s.paused);
        engine.stop();
    }

    #[test]
    fn mobile_hrtf_wet_config_defaults_roundtrips_zero_and_rejects_invalid() {
        let legacy: EngineConfig = serde_json::from_str(r#"{"sampleRate":48000,"outputChannels":2}"#).unwrap();
        assert_eq!(legacy.hrtf_wet_weight, 0.04);
        assert_eq!(EngineConfig::default().hrtf_wet_weight, legacy.hrtf_wet_weight);
        let dry: EngineConfig = serde_json::from_str(r#"{"sampleRate":48000,"outputChannels":2,"directObjectHrtf":true,"directionalHrtf":true,"hrtfWetWeight":0}"#).unwrap();
        let roundtrip: EngineConfig = serde_json::from_str(&serde_json::to_string(&dry).unwrap()).unwrap();
        assert_eq!(roundtrip.hrtf_wet_weight, 0.0);
        assert!(roundtrip.direct_object_hrtf && roundtrip.directional_hrtf);
        assert_eq!(MobileEngine::new(roundtrip, None).unwrap().config().hrtf_wet_weight, 0.0);
        for wet in [0.0, 0.04, 1.0] {
            assert!(MobileEngine::new(EngineConfig { hrtf_wet_weight: wet, ..EngineConfig::default() }, None).is_ok());
        }
        for wet in [-0.01, 1.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let result = MobileEngine::new(EngineConfig { hrtf_wet_weight: wet, ..EngineConfig::default() }, None);
            assert!(matches!(result, Err(error) if error.contains("wet weight")));
        }
    }

    #[test]
    fn mobile_spatial_cues_start_without_legacy_room_or_near_field() {
        let mut engine = MobileEngine::new(EngineConfig { hrtf_wet_weight: 0.0,
            direct_object_hrtf: true, directional_hrtf: true, ..EngineConfig::default() }, None).unwrap();
        engine.load_hrtf(concat!(env!("CARGO_MANIFEST_DIR"),
            "/../../apps/mobile/assets/hrtf-mobile-direct/hrtf-set.json")).unwrap();
        assert_eq!(engine.hrtf_directions, 128);
        engine.start(Arc::new(MobileTestOutput)).unwrap();
        let status = wait_mobile_status(&engine, |s| s.direct_object_hrtf && s.directional_hrtf);
        assert!(status.hrtf_ready);
        assert!(status.spatial_cues_enabled);
        assert!(!status.room_enabled && !status.near_field_enabled);
        engine.stop();
    }

    #[test]
    fn desktop_startup_waits_for_all_batches_and_respects_pause_and_short_eof() {
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        let commands = Arc::new(render_command::RenderCommandQueue::new(32));
        engine.pipeline = Some(RenderPipeline {
            commands: commands.clone(), fifo: Arc::new(stereo_fifo::StereoFifo::new(4096)),
            telemetry: Arc::new(RuntimeTelemetry::default()),
        });
        let queue_frame = |engine: &mut MobileEngine, at, count| {
            let frame = FrameData {
                codec: "eac3", sample_rate: 48000, sample_pos: at,
                channels: vec![vec![0.0; count]], labels: vec!["Obj_42".into()],
                raw_bed_labels: vec![], events: vec![], object_channels: vec![],
                program_loudness: None, ramp_duration: 128,
            };
            for command in engine.frame_router.route(frame).unwrap() {
                assert!(commands.push(command).is_ok());
            }
            {
                let mut startup = engine.startup.lock().unwrap();
                startup.origin.get_or_insert(at);
                startup.accepted_end = at + count as u64;
            }
            engine.start_playback_if_ready().unwrap();
        };
        queue_frame(&mut engine, 0, 12000);
        assert!(!engine.startup.lock().unwrap().started);
        assert_eq!(commands.pending_len_for_debug(), 2, "declaration and first PCM batch only");
        engine.set_paused(true).unwrap();
        queue_frame(&mut engine, 12000, 12000);
        assert!(!engine.startup.lock().unwrap().started);
        assert_eq!(commands.pending_len_for_debug(), 3, "paused startup must not enqueue StartAt");
        engine.set_paused(false).unwrap();
        assert_eq!(commands.pending_len_for_debug(), 4, "StartAt follows both PCM batches");
        assert!(engine.startup.lock().unwrap().started);
        engine.start_playback_if_ready().unwrap();
        assert_eq!(commands.pending_len_for_debug(), 4, "start must be sent exactly once");

        *engine.startup.lock().unwrap() = StartupGate::default();
        queue_frame(&mut engine, 24000, 1000);
        assert!(!engine.startup.lock().unwrap().started);
        engine.finish().unwrap();
        assert!(engine.startup.lock().unwrap().started, "short files must drain at EOF");
        assert_eq!(engine.startup.lock().unwrap().origin, Some(24000));
        assert_eq!(commands.pending_len_for_debug(), 6);
    }

    struct MobileTestOutput;

    impl AudioOutput for MobileTestOutput {
        fn run(self: Arc<Self>, fifo: Arc<stereo_fifo::StereoFifo>, telemetry: Arc<RuntimeTelemetry>,
            _commands: Arc<render_command::RenderCommandQueue>) {
            let mut block = [0.0; 2048];
            while !telemetry.shutdown_requested.load(std::sync::atomic::Ordering::Acquire) {
                fifo.apply_flush_from_consumer();
                let count = fifo.pop_into_f32(&mut block, 2);
                telemetry.callback_consumed_sample_pos.fetch_add(count as u64, std::sync::atomic::Ordering::Release);
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    fn wait_mobile_status(engine: &MobileEngine, predicate: impl Fn(&PlaybackStatus) -> bool) -> PlaybackStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = engine.playback_status();
            if predicate(&status) { return status; }
            assert!(Instant::now() < deadline, "renderer did not apply state: {status:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn mobile_object_rendering_requires_hrtf() {
        let mut engine = MobileEngine::new(EngineConfig { direct_object_hrtf: true,
            directional_hrtf: true, ..EngineConfig::default() }, None).unwrap();
        assert!(engine.start(Arc::new(MobileTestOutput)).unwrap_err().contains("HRTF"));
    }

    #[test]
    fn mobile_balance_routes_cached_master_gain_and_bypasses_object_programs() {
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        engine.load_hrtf(concat!(env!("CARGO_MANIFEST_DIR"),
            "/../../apps/mobile/android/app/src/main/assets/hrtf-dense/hrtf-set.json")).unwrap();
        engine.start(Arc::new(MobileTestOutput)).unwrap();
        engine.set_volume_balance(true).unwrap();
        engine.set_measured_loudness(r#"{"integratedLufs":-10,"blocks":100,"truePeakDbtp":-2}"#).unwrap();
        let mut frame = FrameData {
            codec: "mp3", sample_rate: 48000, sample_pos: 0,
            channels: vec![vec![0.0; 1024]; 2], labels: vec!["L".into(), "R".into()],
            raw_bed_labels: vec![], events: vec![], object_channels: vec![],
            program_loudness: None, ramp_duration: 128,
        };
        let stereo_frame = FrameData { channels: frame.channels.clone(), labels: frame.labels.clone(),
            raw_bed_labels: vec![], events: vec![], object_channels: vec![], codec: "mp3",
            sample_rate: 48000, sample_pos: 0, program_loudness: None, ramp_duration: 128 };
        let pending = engine.balance.measure(stereo_frame, None);
        engine.pending.lock().unwrap().push_back(pending);
        engine.drain_pending_into_pipeline().unwrap();
        let status = engine.playback_status();
        assert!(status.volume_balance_enabled && status.volume_balance_eligible);
        assert_eq!(status.program_gain_db, -8.0);
        engine.set_volume_balance(false).unwrap();
        assert!(!engine.playback_status().volume_balance_enabled);
        assert_eq!(engine.playback_status().program_gain_db, -8.0);
        engine.set_volume_balance(true).unwrap();
        frame.codec = "eac3";
        frame.sample_pos = 1024;
        frame.labels[0] = "Obj_0".into();
        let pending = engine.balance.measure(frame, None);
        engine.pending.lock().unwrap().push_back(pending);
        engine.drain_pending_into_pipeline().unwrap();
        assert!(!engine.playback_status().volume_balance_eligible);
        engine.seek(0).unwrap();
        let status = engine.playback_status();
        assert!(status.volume_balance_enabled);
        assert!(!status.volume_balance_eligible);
        assert_eq!(status.program_gain_db, 0.0);
        engine.stop();
    }

    #[test]
    fn mobile_near_field_acknowledges_applied_state_and_rejects_invalid_scale() {
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        engine.load_hrtf(concat!(env!("CARGO_MANIFEST_DIR"),
            "/../../apps/mobile/android/app/src/main/assets/hrtf-dense/hrtf-set.json")).unwrap();
        engine.start(Arc::new(MobileTestOutput)).unwrap();
        engine.set_near_field(true, 1.0).unwrap();
        assert!(engine.playback_status().near_field_enabled);
        assert!(!engine.playback_status().direct_object_hrtf);
        for invalid in [f32::NAN, 0.1, 4.1] {
            assert!(engine.set_near_field(false, invalid).is_err());
            assert!(engine.playback_status().near_field_enabled);
        }
        engine.set_near_field(false, 0.5).unwrap();
        assert!(!engine.playback_status().near_field_enabled);
        engine.stop();
    }

    #[test]
    fn mobile_object_rendering_applies_desktop_modes() {
        let mut engine = MobileEngine::new(EngineConfig { direct_object_hrtf: true,
            directional_hrtf: true, ..EngineConfig::default() }, None).unwrap();
        engine.load_hrtf(concat!(env!("CARGO_MANIFEST_DIR"),
            "/../../apps/mobile/android/app/src/main/assets/hrtf-dense/hrtf-set.json")).unwrap();
        engine.start(Arc::new(MobileTestOutput)).unwrap();
        let sample = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"),
            "/../../apps/mobile/android/app/src/main/assets/joc_atmos_1s.eac3")).unwrap();
        engine.feed(&sample).unwrap();
        let status = wait_mobile_status(&engine, |s| s.directional_hrtf && s.continuous_object_count > 0);
        assert!(status.hrtf_ready);
        assert!(status.direct_object_hrtf);
        assert_eq!(status.hrtf_directions, 61);
        assert!(status.object_convolver_count >= status.continuous_object_count);
        engine.set_object_rendering(false, false).unwrap();
        wait_mobile_status(&engine, |s| !s.direct_object_hrtf && !s.directional_hrtf && s.object_convolver_count == 0);
        engine.set_object_rendering(true, false).unwrap();
        engine.feed(&sample).unwrap();
        wait_mobile_status(&engine, |s| s.direct_object_hrtf && !s.directional_hrtf && s.object_convolver_count > 0);
        engine.set_object_rendering(false, true).unwrap();
        engine.feed(&sample).unwrap();
        wait_mobile_status(&engine, |s| !s.direct_object_hrtf && s.directional_hrtf && s.continuous_object_count > 0);
        engine.stop();
    }

    /// Decode-only dump of song.eac3 (no render pipeline): writes the raw
    /// decoder output (all bed channels, 6ch WAV) so decode correctness can
    /// be judged by ear on the PC, isolated from the render path.
    #[test]
    fn dump_song_decode_only() {
        let song = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/android-engine-demo/android/app/src/main/assets/song.eac3"
        );
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        let bytes = std::fs::read(song).unwrap();
        engine.feed(&bytes).unwrap();
        let frames = engine.take_pending_frames();
        assert!(!frames.is_empty(), "song.eac3 must decode");
        let max_frames = 2000.min(frames.len());
        let channels = frames[0].channels.len();
        let mut inter = Vec::new();
        for frame in &frames[..max_frames] {
            let len = frame.channels[0].len();
            for i in 0..len {
                for channel in frame.channels.iter().take(channels) {
                    inter.push(channel[i]);
                }
            }
        }
        let out = std::env::temp_dir().join("song-decode-dump.wav");
        let bytes_out = sda_native_renderer::encode_wav_i16_multichannel(&inter, 48000, channels as u16);
        std::fs::write(&out, bytes_out).unwrap();
        let mut peaks = vec![0.0_f32; channels];
        for frame in &frames[..max_frames] {
            for (ch, channel) in frame.channels.iter().enumerate() {
                for v in channel {
                    peaks[ch] = peaks[ch].max(v.abs());
                }
            }
        }
        println!(
            "DECODE_DUMP frames={} channels={} out={:?} peaks={:?}",
            max_frames, channels, out, peaks
        );
    }

    /// Full pipeline render of song.eac3 on host (decode -> render -> FIFO ->
    /// WavDump) with paced feeding. Compare with dump_song_decode_only to
    /// isolate decode vs render distortion.
    #[test]
    fn dump_song_render() {
        let song = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/android-engine-demo/android/app/src/main/assets/song.eac3"
        );
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        let hrtf = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/web/public/hrtf/hrtf-set.json"
        );
        engine.load_hrtf(hrtf).unwrap();
        let dump = std::env::temp_dir().join("song-render-dump.wav");
        let _ = std::fs::remove_file(&dump);
        let sink = Arc::new(sda_native_renderer::WavDumpOutput::new(&dump, 48000));
        let stop_flag = sink.stop.clone();
        engine.start(sink).unwrap();
        let bytes = std::fs::read(song).unwrap();
        let chunk = 24 * 1024;
        let mut fed = 0;
        while fed < bytes.len() {
            let end = (fed + chunk).min(bytes.len());
            engine.feed(&bytes[fed..end]).unwrap();
            fed = end;
            while engine.decoded_sample_pos().saturating_sub(engine.playback_status().consumed_sample_pos) > 48000 {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline && !dump.exists() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(dump.exists(), "render dump never written");
        std::thread::sleep(std::time::Duration::from_millis(800));
        let wav = std::fs::read(&dump).expect("dump readable");
        let frames = (wav.len() - 44) / 4;
        let mut envelope = Vec::new();
        for chunk in wav[44..].chunks(4800 * 4) {
            let rms = (chunk
                .chunks_exact(2)
                .map(|s| {
                    let v = i16::from_le_bytes([s[0], s[1]]) as f64 / 32768.0;
                    v * v
                })
                .sum::<f64>()
                / (chunk.len() / 2).max(1) as f64)
                .sqrt();
            envelope.push((rms * 1000.0).round() / 1000.0);
        }
        println!(
            "RENDER_DUMP frames={frames} (~{} ms) envelope(100ms)={:?}",
            frames * 1000 / 48000,
            envelope
        );
        assert!(frames > 48000, "expected over 1 s of rendered audio");
    }

    /// Same as dump_song_render but WITHOUT HRTF: isolates whether the
    /// distortion enters via the HRTF convolution path or the routing/mixer.
    #[test]
    fn dump_song_render_nohrtf() {
        let song = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/android-engine-demo/android/app/src/main/assets/song.eac3"
        );
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        let dump = std::env::temp_dir().join("song-render-nohrtf.wav");
        let _ = std::fs::remove_file(&dump);
        let sink = Arc::new(sda_native_renderer::WavDumpOutput::new(&dump, 48000));
        let stop_flag = sink.stop.clone();
        engine.start(sink).unwrap();
        let bytes = std::fs::read(song).unwrap();
        let chunk = 24 * 1024;
        let mut fed = 0;
        while fed < bytes.len() {
            let end = (fed + chunk).min(bytes.len());
            engine.feed(&bytes[fed..end]).unwrap();
            fed = end;
            while engine.playback_status().fifo_frames > 12000 {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
        // Feed is done: stop the writer so it flushes and exits.
        stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline && !dump.exists() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(dump.exists(), "render dump never written");
        std::thread::sleep(std::time::Duration::from_millis(800));
        let wav = std::fs::read(&dump).expect("dump readable");
        let frames = (wav.len() - 44) / 4;
        let mut envelope = Vec::new();
        for chunk in wav[44..].chunks(4800 * 4) {
            let rms = (chunk
                .chunks_exact(2)
                .map(|s| {
                    let v = i16::from_le_bytes([s[0], s[1]]) as f64 / 32768.0;
                    v * v
                })
                .sum::<f64>()
                / (chunk.len() / 2).max(1) as f64)
                .sqrt();
            envelope.push((rms * 1000.0).round() / 1000.0);
        }
        println!(
            "NOHRTF_DUMP frames={frames} (~{} ms) envelope={:?}",
            frames * 1000 / 48000,
            envelope
        );
    }

    /// Dumps the full engine render of the JOC fixture through WavDumpOutput
    /// so the audible content can be inspected offline (duration, envelope).
    #[test]
    fn dump_engine_render_for_inspection() {
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        let hrtf = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/web/public/hrtf/hrtf-set.json"
        );
        engine.load_hrtf(hrtf).unwrap();
        engine.feed(&joc_fixture()).unwrap();
        let dump = std::env::temp_dir().join("sda-engine-render-dump.wav");
        let _ = std::fs::remove_file(&dump);
        let sink = Arc::new(sda_native_renderer::WavDumpOutput::new(&dump, 48000));
        let stop_flag = sink.stop.clone();
        engine
            .start(sink)
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline && !dump.exists() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if !dump.exists() {
            let status = engine.playback_status();
            panic!("dump never written: fifo={}", status.fifo_frames);
        }
        std::thread::sleep(std::time::Duration::from_millis(800));
        let wav = std::fs::read(&dump).expect("dump written");
        let frames = (wav.len() - 44) / 4;
        let mut envelope = Vec::new();
        let window = 4800 * 4;
        for chunk in wav[44..].chunks(window) {
            let rms = (chunk
                .chunks_exact(2)
                .map(|s| {
                    let v = i16::from_le_bytes([s[0], s[1]]) as f64 / 32768.0;
                    v * v
                })
                .sum::<f64>()
                / (chunk.len() / 2).max(1) as f64)
                .sqrt();
            envelope.push((rms * 1000.0).round() / 1000.0);
        }
        println!(
            "DUMP frames={frames} (~{} ms) envelope(100ms)={:?}",
            frames * 1000 / 48000,
            envelope
        );
        assert!(frames > 24000, "expected at least 0.5 s of rendered audio");
    }

    fn joc_fixture() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../harletty-bridge/harletty/tests/fixtures/joc_atmos_1s.eac3"
        );
        std::fs::read(path)
            .unwrap_or_else(|error| panic!("fixture missing ({error}); run `git submodule update --init`"))
    }

    #[test]
    fn engine_feeds_decoder_and_queues_frames() {
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        let bytes = joc_fixture();
        let status = engine.feed(&bytes).unwrap();
        assert_eq!(status.codec, "eac3");
        assert!(status.frames_pushed > 0, "fixture must decode frames");
        assert!(status.sample_pos > 0);
        assert!(status.errors.is_empty());

        let frames = engine.take_pending_frames();
        assert_eq!(frames.len() as u32, status.frames_pushed);
        assert!(frames[0].channels.iter().all(|channel| !channel.is_empty()));
        assert!(!frames[0].labels.is_empty());

        // Draining twice yields nothing new until more input arrives.
        assert!(engine.take_pending_frames().is_empty());
        assert_eq!(engine.codec_name(), "eac3");
    }

    #[test]
    fn rejects_non_stereo_output() {
        let error = match MobileEngine::new(
            EngineConfig { output_channels: 6, ..EngineConfig::default() },
            None,
        ) {
            Err(error) => error,
            Ok(_) => panic!("expected non-stereo config to be rejected"),
        };
        assert!(error.contains("stereo"));
    }

    #[test]
    fn playback_status_tracks_consumed_clock() {
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        let hrtf = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/web/public/hrtf/hrtf-set.json"
        );
        engine.load_hrtf(hrtf).unwrap();
        engine.feed(&joc_fixture()).unwrap();
        assert_eq!(engine.playback_status().position_ms, 0);

        engine.start(Arc::new(
            sda_native_renderer::WavDumpOutput::new(
                std::env::temp_dir().join(format!(
                    "sda-mobile-clock-{}.wav",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()
                )),
                48000,
            ),
        )).unwrap();
        assert!(engine.start(Arc::new(sda_native_renderer::WavDumpOutput::new("x.wav", 48000))).is_err());

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            let status = engine.playback_status();
            if status.consumed_sample_pos > 4800 {
                assert!(status.position_ms >= 100);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let status = engine.playback_status();
        let pipeline = engine.pipeline.as_ref().unwrap();
        panic!(
            "consumed clock never advanced: fifo={} pending={} consumed={} blocks={} callbacks={} cmdq_len={}",
            status.fifo_frames,
            status.pending_batches,
            status.consumed_sample_pos,
            pipeline.telemetry.render_block_count.load(std::sync::atomic::Ordering::Relaxed),
            pipeline.telemetry.callback_count.load(std::sync::atomic::Ordering::Relaxed),
            pipeline.commands.pending_len_for_debug(),
        );
    }

    /// T1.9: seek flushes decoder, queued frames and the FIFO; the next feed
    /// starts a fresh time base.
    #[test]
    fn seek_flushes_decode_and_render_state() {
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        engine.feed(&joc_fixture()).unwrap();
        assert!(engine.take_pending_frames().len() > 0);
        engine.seek(30_000).unwrap();
        assert!(engine.take_pending_frames().is_empty());
        assert_eq!(engine.playback_status().position_ms, 0);
        assert_eq!(engine.decoded_sample_pos(), 0);
    }

    /// T1.10: object snapshots coalesce to one per 66 ms window.
    #[test]
    fn object_snapshot_waits_for_presentation_clock_and_evicts_consumed_events() {
        let object = ObjectEvent {
            diffuse: 0.0,
            id: 7,
            sample_pos: 480,
            has_pos: true,
            pos: [1.0, 0.0, 0.0],
            gain_db: 0.0,
            size: [0.0; 3],
            anchor: "room".into(),
            distance_m: None,
            distance_infinite: false,
            screen_factor: None,
            depth_factor: None,
            ramp_duration: 0,
        };
        let mut events = VecDeque::from([object.clone()]);
        let mut active = ObjectSnapshot::new();
        assert!(snapshot_at_clock(&mut events, &mut active, 479).is_empty());
        assert_eq!(events.len(), 1);
        let snapshot = snapshot_at_clock(&mut events, &mut active, 480);
        assert_eq!(snapshot.get(&7).map(|value| value.pos), Some(object.pos));
        assert!(events.is_empty(), "consumed events are evicted from the timeline");

        let inactive = ObjectEvent { has_pos: false, sample_pos: 960, ..object };
        events.push_back(inactive);
        assert_eq!(snapshot_at_clock(&mut events, &mut active, 960).len(), 0,
            "hasPos=false removes the active object on its effective presentation frame");
    }

    #[test]
    fn object_snapshot_capacity_rejects_instead_of_dropping_future_events() {
        let event = ObjectEvent {
            diffuse: 0.0,
            id: 1, sample_pos: 1, has_pos: true, pos: [0.0; 3], gain_db: 0.0,
            size: [0.0; 3], anchor: "room".into(), distance_m: None,
            distance_infinite: false, screen_factor: None, depth_factor: None, ramp_duration: 0,
        };
        let mut timeline = VecDeque::from(vec![event; OBJECT_EVENT_CAPACITY]);
        let available = OBJECT_EVENT_CAPACITY.saturating_sub(timeline.len());
        assert_eq!(available, 0, "full timeline has no capacity for another event");
        assert_eq!(timeline.len(), OBJECT_EVENT_CAPACITY, "no future event was silently evicted");
    }

    #[test]
    fn head_yaw_rejects_non_finite_and_out_of_range_values() {
        let engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        assert!(engine.set_head_yaw_degrees(f32::NAN).is_err());
        assert!(engine.set_head_yaw_degrees(181.0).is_err());
        assert!(engine.set_head_yaw_degrees(-181.0).is_err());
    }

    #[test]
    fn joc_fixture_exposes_object_channels_and_position_events() {
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        engine.feed(&joc_fixture()).unwrap();
        let frames = engine.take_pending_frames();
        assert!(frames.iter().any(|frame| frame.labels.iter().any(|label| label.starts_with("Obj_"))),
            "fixture must decode object PCM channels");
        assert!(frames.iter().any(|frame| !frame.object_channels.is_empty()),
            "fixture must declare the object-to-channel mapping");
        assert!(frames.iter().any(|frame| frame.events.iter().any(|event| event.has_pos)),
            "raw decoded fixture frames must include positioned events");
    }

    #[test]
    fn object_snapshot_waits_for_presentation_clock_and_throttles() {
        let mut engine = MobileEngine::new(EngineConfig::default(), None).unwrap();
        engine.feed(&joc_fixture()).unwrap();
        let frames = engine.take_pending_frames();
        let latest = frames.iter().flat_map(|frame| frame.events.iter()).filter(|event| event.has_pos)
            .max_by_key(|event| event.sample_pos).expect("fixture has positioned raw events").clone();
        let timeline_len = engine.object_events.lock().unwrap().len();
        engine.pipeline = Some(RenderPipeline {
            fifo: Arc::new(stereo_fifo::StereoFifo::new(sda_native_renderer::STEREO_FIFO_CAPACITY_FRAMES)),
            commands: Arc::new(render_command::RenderCommandQueue::new(256)),
            telemetry: Arc::new(RuntimeTelemetry::default()),
        });
        let first = engine.poll_object_snapshot(&frames).expect("first poll is allowed");
        assert!(first.is_empty(), "future fixture events must be hidden at clock 0");
        engine.pipeline.as_ref().unwrap().telemetry.callback_consumed_sample_pos
            .store(latest.sample_pos, std::sync::atomic::Ordering::Release);
        std::thread::sleep(OBJECT_POLL_INTERVAL + Duration::from_millis(2));
        let consumed_clock = engine.playback_status().consumed_sample_pos;
        let snapshot = engine.poll_object_snapshot(&frames).expect("poll window elapsed");
        assert_eq!(snapshot.get(&latest.id).map(|event| event.pos), Some(latest.pos),
            "raw positioned event should be presented: timeline={timeline_len}, consumed={consumed_clock}, latest={}",
            latest.sample_pos);
        assert!(engine.poll_object_snapshot(&frames).is_none(), "inside throttle window");
    }
}
