//! SDA native stereo renderer sidecar.
//!
//! JSONL control is deliberately separated from the audio callback. The callback
//! owns no allocation, JSON parsing, or renderer IPC; it only advances one
//! codec-clock sample position and mixes independent source rings to stereo.
//! This first foundation is a reference mix path. Object HRTF partitioned
//! convolution plugs in after the per-source sample fetch, without changing the
//! IPC clock or the WASAPI output lifecycle.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
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

const PROTOCOL: u32 = 6;
const MAX_SOURCES: usize = 64;
const MAX_PENDING_SAMPLES: usize = 480_000; // 10 s @ 48 kHz per source.
const FRAME_JSON: u8 = b'J';
const FRAME_PCM: u8 = b'P';
/// Atomic collection of all mono source blocks for one decoded codec frame.
const FRAME_PCM_BATCH: u8 = b'B';
/// Validated final-output headphone FIR payload: preamp + independent L/R taps.
const FRAME_HEADPHONE_FIR: u8 = b'H';
const NATIVE_RENDERER_MAX_JSON_BYTES: usize = 16 * 1024;
const MAX_HEADPHONE_FIR_TAPS: usize = 32_768;
const DEFAULT_OBJECT_RAMP: u32 = 128;
/// Room-calibrated per-speaker presentation level. A Genelec/Dolby Atmos
/// listening room calibrates every loudspeaker - physical or virtual - to one
/// reference SPL (79 dB for small rooms), which in the digital domain means
/// each speaker feed renders roughly 18 dB below full scale so that a full
/// object ensemble sums near one calibrated program instead of N independent
/// full-scale sources piling up ahead of the peak guard.
const ROOM_SPEAKER_REFERENCE_GAIN: f32 = 0.12589251; // 10^(-18/20)
const STEREO_FIFO_CAPACITY_FRAMES: usize = 32_768;
const STEREO_FIFO_TARGET_FRAMES: usize = 16_384;
/// Prebuffer before the WASAPI callback may start pulling: 8192 frames is
/// about 170 ms of program, enough to absorb decode jitter at startup.
const STEREO_FIFO_START_FRAMES: usize = 8_192;
/// Keep browser object-activity semantics: −60 dBFS source signal held for 200 ms.
const OBJECT_ACTIVITY_THRESHOLD: f32 = 0.001;
const OBJECT_ACTIVITY_QUEUE_CAPACITY: usize = 16;

mod bus_renderer;
mod direct_renderer;
mod callback_output;
mod convolution;
mod dsp;
#[allow(dead_code)]
mod headphone;
mod hrtf;
mod output_monitor;
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
        at: Option<u64>,
        #[serde(rename = "bedLabel")]
        bed_label: Option<String>,
    },
    SetLfeMuted {
        muted: bool,
    },
    SetVolume {
        volume: f32,
    },
    SetProgramEnabled {
        enabled: bool,
    },
    SetProgramGain {
        gain: f32,
        at: Option<u64>,
    },
    SetBinauralEq {
        low: f32,
        mid: f32,
        high: f32,
        #[serde(rename = "lowCut")]
        low_cut: bool,
    },
    ClearHeadphoneCompensation,
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
    /// Changes the virtual physical speaker layout used for VBAP, bed snapping,
    /// and HRTF buses. The sidecar remains a stereo binaural output device.
    SetLayout {
        layout: String,
    },
    SetObjectHrtf { enabled: bool },
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
    /// DAC-aligned post-source-gain/post-mute object activity for visual feedback.
    ObjectActivity {
        sample_pos: u64,
        ids: &'a [u32],
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
    render_sample_pos: u64,
    active_sources: usize,
    underrun_samples: u64,
    callback_count: u64,
    callback_max_micros: u64,
    callback_fifo_underrun_frames: u64,
    output_peak: f32,
    output_max_sample_step: f32,
    output_max_step_sample: u64,
    output_large_steps: u64,
    output_last_large_step_sample: u64,
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
    layout: &'static str,
    spatial_bus_count: usize,
    direct_object_hrtf: bool,
    object_convolver_count: usize,
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
struct ProgramGainEvent {
    gain: f32,
}

#[derive(Clone, Copy)]
struct SpatialEvent {
    position: [f32; 3],
    spread: f32,
    ramp: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Object,
    Bed,
}

#[derive(Clone, Copy)]
struct RouteGains {
    buses: [f32; vbap::MAX_BUS_COUNT],
    lfe: f32,
}

struct StereoEq {
    left: [dsp::Biquad; 4],
    right: [dsp::Biquad; 4],
    headroom_gain: f32,
}

impl StereoEq {
    fn new(sample_rate: u32, bands: [f32; 3], low_cut: bool) -> Result<Self, String> {
        let low = bands[0].clamp(-12.0, 12.0);
        let mid = bands[1].clamp(-12.0, 12.0);
        let high = bands[2].clamp(-12.0, 12.0);
        // Conservative equivalent of the browser's sampled headroom scan.
        let max_boost = low.max(0.0) + mid.max(0.0) + high.max(0.0);
        let headroom_gain = if max_boost > 1e-6 {
            10.0_f32.powf((-max_boost - 0.2) / 20.0)
        } else {
            1.0
        };
        let filters = [
            dsp::Biquad::lowshelf(sample_rate, 120.0, 0.7, low)?,
            dsp::Biquad::peaking(sample_rate, 1200.0, 0.8, mid)?,
            dsp::Biquad::highshelf(sample_rate, 6000.0, 0.7, high)?,
            dsp::Biquad::lowshelf(sample_rate, 180.0, 0.7, if low_cut { -3.0 } else { 0.0 })?,
        ];
        Ok(Self {
            left: filters,
            right: filters,
            headroom_gain,
        })
    }

    fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        let mut left = left * self.headroom_gain;
        let mut right = right * self.headroom_gain;
        for filter in &mut self.left {
            left = filter.process(left);
        }
        for filter in &mut self.right {
            right = filter.process(right);
        }
        [left, right]
    }
}

struct LfePath {
    lowpass: dsp::Lr4Lowpass,
    compressor: dsp::MonoCompressor,
    delay: [f32; convolution::DEFAULT_PARTITION],
    cursor: usize,
}

impl LfePath {
    fn new(sample_rate: u32) -> Self {
        Self {
            lowpass: dsp::Lr4Lowpass::new(sample_rate, 120.0)
                .expect("output sample rate must support the 120 Hz LFE filter"),
            compressor: dsp::MonoCompressor::lfe(sample_rate),
            delay: [0.0; convolution::DEFAULT_PARTITION],
            cursor: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.delay[self.cursor];
        self.delay[self.cursor] = self.compressor.process(self.lowpass.process(input));
        self.cursor = (self.cursor + 1) % self.delay.len();
        output
    }

    fn reset(&mut self) {
        self.lowpass.reset();
        self.compressor.reset();
        self.delay.fill(0.0);
        self.cursor = 0;
    }
}

struct Source {
    direct: Option<direct_renderer::DirectSource>,
    samples: pcm_ring::AbsolutePcmRing,
    kind: SourceKind,
    /// Canonical decoder bed label retained so a layout change can re-resolve it.
    bed_label: Option<String>,
    /// Parsed once at declaration; avoids source-ID parsing in the mix loop.
    object_id: Option<u32>,
    activity_until: u64,
    gain_events: BTreeMap<u64, GainEvent>,
    gain: f32,
    target_gain: f32,
    ramp_remaining: u32,
    ramp_step: f32,
    remove_at: Option<u64>,
    position: [f32; 3],
    spread: f32,
    spatial_events: BTreeMap<u64, SpatialEvent>,
    bus_gains: [f32; vbap::MAX_BUS_COUNT],
    bus_targets: [f32; vbap::MAX_BUS_COUNT],
    bus_steps: [f32; vbap::MAX_BUS_COUNT],
    lfe_gain: f32,
    lfe_target: f32,
    lfe_step: f32,
    bus_ramp_remaining: u32,
    availability: f32,
    availability_target: f32,
    availability_step: f32,
    availability_ramp_remaining: u32,
    last_audible_at: u64,
    muted: bool,
    mute_events: BTreeMap<u64, bool>,
    /// Suspended sources skip PCM and mixing work while metadata and envelopes
    /// retain their codec timing. PCM, gain events, or an unmute wake the source.
    suspended: bool,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            samples: pcm_ring::AbsolutePcmRing::new(MAX_PENDING_SAMPLES),
            direct: None,
            kind: SourceKind::Bed,
            bed_label: None,
            object_id: None,
            activity_until: 0,
            gain_events: BTreeMap::new(),
            gain: 0.0,
            target_gain: 0.0,
            ramp_remaining: 0,
            ramp_step: 0.0,
            remove_at: None,
            position: [0.0, 1.0, 0.0],
            spread: 0.0,
            spatial_events: BTreeMap::new(),
            bus_gains: [0.0; vbap::MAX_BUS_COUNT],
            bus_targets: [0.0; vbap::MAX_BUS_COUNT],
            bus_steps: [0.0; vbap::MAX_BUS_COUNT],
            lfe_gain: 0.0,
            lfe_target: 0.0,
            lfe_step: 0.0,
            bus_ramp_remaining: 0,
            availability: 0.0,
            availability_target: 0.0,
            availability_step: 0.0,
            availability_ramp_remaining: 0,
            last_audible_at: 0,
            muted: false,
            mute_events: BTreeMap::new(),
            suspended: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObjectActivitySnapshot {
    sample_pos: u64,
    ids: [u32; MAX_SOURCES],
    count: usize,
}

impl ObjectActivitySnapshot {
    fn empty(sample_pos: u64) -> Self {
        Self {
            sample_pos,
            ids: [0; MAX_SOURCES],
            count: 0,
        }
    }

    fn active_ids(&self) -> &[u32] {
        &self.ids[..self.count]
    }

    fn has_same_ids(&self, other: &Self) -> bool {
        self.active_ids() == other.active_ids()
    }
}

struct RuntimeTelemetry {
    callback_output_enabled: AtomicBool,
    /// Codec timeline consumed by WASAPI, never the worker's render-ahead clock.
    callback_consumed_sample_pos: AtomicU64,
    callback_count: AtomicU64,
    callback_max_micros: AtomicU64,
    callback_fifo_underrun_frames: AtomicU64,
    output: output_monitor::OutputTelemetry,
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
    /// Newest accepted pose waiting for the next throttle window, plus the
    /// timestamp of the last route rebuild triggered by a pose.
    pending_pose: Option<[f32; 4]>,
    last_pose_apply: Option<std::time::Instant>,
    /// Pose the current object routes were built against; the slerp origin.
    pose_route_base: Option<[f32; 4]>,
    pending_object_events: HashMap<String, Vec<NativeObjectEvent>>,
    active_hrtf_set: Option<hrtf::NativeHrtfSet>,
    hrtf_wet_weight: f32,
    layout: vbap::LayoutId,
    vbap: vbap::VbapSolver,
    bus_renderer: Option<bus_renderer::BusRenderer>,
    direct_objects: bool,
    direct_mix: f32,
    lfe_path: LfePath,
    lfe_muted: bool,
    output_gain: f32,
    output_target_gain: f32,
    output_gain_step: f32,
    output_gain_ramp_remaining: u32,
    program_enabled: bool,
    program_metadata_gain: f32,
    program_gain: f32,
    program_target_gain: f32,
    program_gain_step: f32,
    program_gain_ramp_remaining: u32,
    program_events: BTreeMap<u64, ProgramGainEvent>,
    peak_guard: dsp::StereoPeakGuard,
    binaural_eq: StereoEq,
    headphone: headphone::HeadphoneCompensation,
    block_offset: usize,
    output_active: bool,
    render_epoch: u64,
    route_update_count: u64,
    activity_tick_every: u64,
    next_activity_tick: u64,
    activity_snapshots: VecDeque<ObjectActivitySnapshot>,
    last_queued_activity: ObjectActivitySnapshot,
    last_emitted_activity: ObjectActivitySnapshot,
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
            pending_pose: None,
            last_pose_apply: None,
            pose_route_base: None,
            pending_object_events: HashMap::new(),
            active_hrtf_set: None,
            hrtf_wet_weight: 0.04,
            layout: vbap::LayoutId::Dolby7_1_4,
            vbap: vbap::VbapSolver::new(),
            bus_renderer: None,
            direct_objects: false,
            direct_mix: 0.0,
            lfe_path: LfePath::new(sample_rate),
            lfe_muted: false,
            output_gain: 1.0,
            output_target_gain: 1.0,
            output_gain_step: 0.0,
            output_gain_ramp_remaining: 0,
            program_enabled: false,
            program_metadata_gain: 1.0,
            program_gain: 1.0,
            program_target_gain: 1.0,
            program_gain_step: 0.0,
            program_gain_ramp_remaining: 0,
            program_events: BTreeMap::new(),
            peak_guard: dsp::StereoPeakGuard::new(sample_rate),
            binaural_eq: StereoEq::new(sample_rate, [0.0; 3], false)
                .expect("48 kHz must support final binaural EQ"),
            headphone: headphone::HeadphoneCompensation::bypass()
                .expect("bypass headphone FIR must be valid"),
            block_offset: 0,
            output_active: false,
            render_epoch: 0,
            route_update_count: 0,
            activity_tick_every: (sample_rate >> 3).max(1) as u64,
            next_activity_tick: 0,
            activity_snapshots: VecDeque::with_capacity(OBJECT_ACTIVITY_QUEUE_CAPACITY),
            last_queued_activity: ObjectActivitySnapshot::empty(0),
            last_emitted_activity: ObjectActivitySnapshot::empty(0),
        }
    }

    fn hrtf_root() -> std::path::PathBuf {
        std::env::var_os("SDA_HRTF_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("hrtf-assets"))
    }

    fn set_direct_objects(&mut self, enabled: bool) -> Result<(), String> {
        if enabled {
            let set = self.active_hrtf_set.as_mut().ok_or("native HRTF set is not configured")?;
            let mut prepared = Vec::new();
            for (id, source) in &self.sources {
                if source.kind == SourceKind::Object && source.direct.is_none() {
                    let mut direct = direct_renderer::DirectSource::new(set, self.hrtf_wet_weight)?;
                    direct.update(set, self.hrtf_wet_weight, source.position, self.head_pose, source.spread)?;
                    prepared.push((id.clone(), direct));
                }
            }
            for (id, direct) in prepared { self.sources.get_mut(&id).unwrap().direct = Some(direct); }
        }
        self.direct_objects = enabled;
        Ok(())
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
        for source in self.sources.values_mut() { source.direct = None; }
        self.direct_mix = 0.0;
        Ok(())
    }

    /// Rebuilds the worker-owned physical virtual speaker graph, then routes
    /// every retained source into the selected room layout without clearing PCM
    /// rings, object state, output gain, or the codec clock.
    fn set_layout(&mut self, layout: vbap::LayoutId) -> Result<(), String> {
        if layout == self.layout {
            return Ok(());
        }
        self.layout = layout;
        self.vbap = vbap::VbapSolver::with_layout(layout);
        if self.active_hrtf_set.is_some() {
            self.rebuild_bus_renderer()?;
        }
        let ids: Vec<String> = self.sources.keys().cloned().collect();
        for id in ids {
            let (kind, label) = self
                .sources
                .get(&id)
                .map(|source| (source.kind, source.bed_label.clone()))
                .ok_or("source vanished during layout update")?;
            if kind == SourceKind::Object {
                self.route_source_now(&id, 0)?;
            } else if let Some(label) = label {
                let route = bed_route(&label, &self.vbap);
                let source = self.sources.get_mut(&id).expect("source still exists");
                Self::set_source_route(source, route, 0);
            }
        }
        Ok(())
    }

    fn set_source_route(source: &mut Source, route: RouteGains, ramp: u32) {
        if ramp == 0 {
            source.bus_gains = route.buses;
            source.bus_targets = route.buses;
            source.bus_steps = [0.0; vbap::MAX_BUS_COUNT];
            source.lfe_gain = route.lfe;
            source.lfe_target = route.lfe;
            source.lfe_step = 0.0;
            source.bus_ramp_remaining = 0;
            return;
        }
        source.bus_targets = route.buses;
        source.lfe_target = route.lfe;
        source.bus_ramp_remaining = ramp;
        for index in 0..vbap::MAX_BUS_COUNT {
            source.bus_steps[index] = (route.buses[index] - source.bus_gains[index]) / ramp as f32;
        }
        source.lfe_step = (route.lfe - source.lfe_gain) / ramp as f32;
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
            for bus in 0..vbap::MAX_BUS_COUNT {
                source.bus_gains[bus] += source.bus_steps[bus] * route as f32;
            }
            source.lfe_gain += source.lfe_step * route as f32;
            source.bus_ramp_remaining -= route;
            if source.bus_ramp_remaining == 0 {
                source.bus_gains = source.bus_targets;
                source.lfe_gain = source.lfe_target;
            }
        }
    }

    fn route_source_now(&mut self, id: &str, ramp: u32) -> Result<(), String> {
        let (position, spread, kind) = self
            .sources
            .get(id)
            .map(|source| (source.position, source.spread, source.kind))
            .ok_or("unknown source")?;
        if kind != SourceKind::Object {
            return Ok(());
        }
        let route = RouteGains {
            buses: bus_renderer::route(&self.vbap, position, self.head_pose, spread),
            lfe: 0.0,
        };
        let source = self.sources.get_mut(id).expect("source was checked above");
        Self::set_source_route(source, route, ramp);
        Ok(())
    }

    fn set_output_volume(&mut self, volume: f32, immediate: bool) {
        let target = volume.clamp(0.0, 1.0).powi(2);
        if immediate {
            self.output_gain = target;
            self.output_target_gain = target;
            self.output_gain_step = 0.0;
            self.output_gain_ramp_remaining = 0;
            return;
        }
        let ramp = (self.output_sample_rate as f32 * 0.02).round().max(1.0) as u32;
        self.output_target_gain = target;
        self.output_gain_ramp_remaining = ramp;
        self.output_gain_step = (target - self.output_gain) / ramp as f32;
    }

    fn set_program_target(&mut self, gain: f32, immediate: bool) {
        let target = if self.program_enabled {
            gain.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if immediate {
            self.program_gain = target;
            self.program_target_gain = target;
            self.program_gain_step = 0.0;
            self.program_gain_ramp_remaining = 0;
            return;
        }
        let ramp = (self.output_sample_rate as f32 * 0.05).round().max(1.0) as u32;
        self.program_target_gain = target;
        self.program_gain_ramp_remaining = ramp;
        self.program_gain_step = (target - self.program_gain) / ramp as f32;
    }

    fn fast_forward_program_envelope(&mut self, samples: u64) {
        let elapsed = samples.min(self.program_gain_ramp_remaining as u64) as u32;
        if elapsed == 0 {
            return;
        }
        self.program_gain += self.program_gain_step * elapsed as f32;
        self.program_gain_ramp_remaining -= elapsed;
        if self.program_gain_ramp_remaining == 0 {
            self.program_gain = self.program_target_gain;
        }
    }

    fn advance_output_envelopes(&mut self) {
        if self.output_gain_ramp_remaining > 0 {
            self.output_gain += self.output_gain_step;
            self.output_gain_ramp_remaining -= 1;
            if self.output_gain_ramp_remaining == 0 {
                self.output_gain = self.output_target_gain;
            }
        }
        if self.program_gain_ramp_remaining > 0 {
            self.program_gain += self.program_gain_step;
            self.program_gain_ramp_remaining -= 1;
            if self.program_gain_ramp_remaining == 0 {
                self.program_gain = self.program_target_gain;
            }
        }
    }

    fn clear_object_activity(&mut self, sample_pos: u64) {
        self.next_activity_tick = sample_pos;
        self.activity_snapshots.clear();
        self.last_queued_activity = ObjectActivitySnapshot::empty(sample_pos);
        self.last_emitted_activity = ObjectActivitySnapshot::empty(sample_pos);
        for source in self.sources.values_mut() {
            source.activity_until = 0;
        }
    }

    fn queue_object_activity_snapshot(&mut self, sample_pos: u64) {
        if sample_pos < self.next_activity_tick {
            return;
        }
        self.next_activity_tick = sample_pos.saturating_add(self.activity_tick_every);
        let mut snapshot = ObjectActivitySnapshot::empty(sample_pos);
        for source in self.sources.values() {
            let Some(object_id) = source.object_id else {
                continue;
            };
            if !source
                .remove_at
                .is_some_and(|remove_at| sample_pos >= remove_at)
                && !source.muted
                && sample_pos <= source.activity_until
                && snapshot.count < MAX_SOURCES
            {
                snapshot.ids[snapshot.count] = object_id;
                snapshot.count += 1;
            }
        }
        snapshot.ids[..snapshot.count].sort_unstable();
        if snapshot.has_same_ids(&self.last_queued_activity) {
            return;
        }
        self.last_queued_activity = snapshot;
        if self.activity_snapshots.len() == OBJECT_ACTIVITY_QUEUE_CAPACITY {
            self.activity_snapshots.pop_front();
        }
        self.activity_snapshots.push_back(snapshot);
    }

    fn emit_consumed_object_activity(&mut self, consumed_sample_pos: u64) {
        while self
            .activity_snapshots
            .front()
            .is_some_and(|snapshot| snapshot.sample_pos <= consumed_sample_pos)
        {
            let snapshot = self.activity_snapshots.pop_front().expect("checked front");
            if !snapshot.has_same_ids(&self.last_emitted_activity) {
                write_event(&Event::ObjectActivity {
                    sample_pos: snapshot.sample_pos,
                    ids: snapshot.active_ids(),
                });
                self.last_emitted_activity = snapshot;
            }
        }
    }

    fn reset_session(&mut self, origin: u64) {
        self.sample_pos = origin;
        self.block_offset = 0;
        self.direct_mix = 0.0;
        // A replacement player can queue PCM before startAt reaches the worker.
        // Stop the old transport so it cannot consume the new origin first.
        self.output_active = false;
        self.paused = true;
        self.render_epoch = self.render_epoch.wrapping_add(1);
        self.pending_object_events.clear();
        self.sources.clear();
        self.clear_object_activity(origin);
        self.head_pose = None;
        self.lfe_muted = false;
        self.lfe_path.reset();
        self.program_metadata_gain = 1.0;
        self.program_gain = 1.0;
        self.program_target_gain = 1.0;
        self.program_gain_step = 0.0;
        self.program_gain_ramp_remaining = 0;
        self.program_events.clear();
        self.peak_guard.reset();
        self.headphone.reset();
        if let Some(bus_renderer) = &mut self.bus_renderer {
            bus_renderer.reset();
        }
    }

    fn health(&self, fifo: &stereo_fifo::StereoFifo, telemetry: &RuntimeTelemetry) -> Health {
        let render_blocks = telemetry.render_block_count.load(Ordering::Relaxed);
        let render_total = telemetry.render_block_total_micros.load(Ordering::Relaxed);
        Health {
            sample_pos: telemetry
                .callback_consumed_sample_pos
                .load(Ordering::Relaxed),
            render_sample_pos: self.sample_pos,
            active_sources: self.sources.len(),
            underrun_samples: self.underrun_samples,
            callback_count: telemetry.callback_count.load(Ordering::Relaxed),
            callback_max_micros: telemetry.callback_max_micros.load(Ordering::Relaxed),
            callback_fifo_underrun_frames: telemetry
                .callback_fifo_underrun_frames
                .load(Ordering::Relaxed),
            output_peak: telemetry.output.peak(),
            output_max_sample_step: telemetry.output.max_step(),
            output_max_step_sample: telemetry.output.max_step_sample(),
            output_large_steps: telemetry.output.large_steps(),
            output_last_large_step_sample: telemetry.output.last_large_step_sample(),
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
            layout: self.layout.as_str(),
            spatial_bus_count: self.vbap.bus_count(),
            direct_object_hrtf: self.direct_objects,
            object_convolver_count: self.sources.values().filter(|source| source.direct.is_some()).count(),
        }
    }

    /// Wake probe for suspended sources, run at most once per 128-sample block
    /// from the render loop: a suspended source resumes when fresh PCM has been
    /// queued within its lookahead window.
    fn pending_suspend_recheck(&self, at: u64) -> bool {
        at % convolution::DEFAULT_PARTITION as u64 == 0
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
        let vbap = self.vbap.clone();
        let head_pose = self.head_pose;
        for frame in output.chunks_exact_mut(channels) {
            let at = self.sample_pos;
            let block_index = self.block_offset;
            if block_index == 0 {
                self.bus_renderer
                    .as_mut()
                    .expect("checked above")
                    .begin_block();
                self.headphone.begin_block();
            }
            let mut lfe_sum = 0.0_f32;
            let mut direct_sum = [0.0_f32; 2];
            // Fade the excitation, retaining both paths' convolution tails.
            let target_mix = if self.direct_objects { 1.0 } else { 0.0 };
            self.direct_mix += (target_mix - self.direct_mix).clamp(-1.0 / 9600.0, 1.0 / 9600.0);
            for source in self.sources.values_mut() {
                if source.kind == SourceKind::Object && self.direct_objects && source.direct.is_none() {
                    source.direct = self.active_hrtf_set.as_ref().and_then(|set| direct_renderer::DirectSource::new(set, self.hrtf_wet_weight).ok());
                }
                if let Some(direct) = &source.direct {
                    direct_sum[0] += direct.left[block_index];
                    direct_sum[1] += direct.right[block_index];
                }
                if source.remove_at.is_some_and(|remove_at| at >= remove_at) {
                    continue;
                }
                // Mute/unmute events must land even while suspended: an unmute
                // is what wakes the source back up.
                if let Some(muted) = source.mute_events.remove(&at) {
                    source.muted = muted;
                    if !muted {
                        source.suspended = false;
                    }
                }
                // Metadata follows the codec clock even when a source is
                // suspended; its next audible sample must use the current state.
                if source.kind == SourceKind::Object {
                    if let Some(event) = source.spatial_events.remove(&at) {
                        // Static-position carriers re-send the same OAMD event
                        // every frame; recomputing VBAP + pose transform for an
                        // unchanged pose dominated the render budget. Only a
                        // real change may rebuild the route.
                        let unchanged = source.position == event.position
                            && source.spread == event.spread
                            && source.bus_ramp_remaining == 0
                            && head_pose.is_none();
                        source.position = event.position;
                        source.spread = event.spread;
                        if !unchanged {
                            Self::set_source_route(
                                source,
                                RouteGains {
                                buses: bus_renderer::route(
                                    &vbap,
                                    event.position,
                                    head_pose,
                                    event.spread,
                                ),
                                    lfe: 0.0,
                                },
                                event.ramp,
                            );
                            self.route_update_count = self.route_update_count.saturating_add(1);
                        }
                    }
                }
                if let Some(event) = source.gain_events.remove(&at) {
                    source.target_gain = event.gain;
                    source.ramp_remaining = event.ramp.max(1);
                    source.ramp_step = (event.gain - source.gain) / source.ramp_remaining as f32;
                    source.suspended = false;
                }
                if source.suspended {
                    if let (Some(direct), Some(set)) = (&mut source.direct, &mut self.active_hrtf_set) {
                        if block_index == 0 { let _ = direct.update(set, self.hrtf_wet_weight, source.position, head_pose, source.spread); }
                    }
                    Self::advance_source_envelopes(source, 1);
                    if at % convolution::DEFAULT_PARTITION as u64 == 0
                        && source.samples.has_future_pcm_within(at, 4800)
                    {
                        source.suspended = false;
                    }
                    continue;
                }
                // Match master worklet timing: the event boundary emits the
                // current vector/scalar first, then advances its envelopes for
                // the following sample. Advancing here would make every moving
                // object start one step ahead of its scheduled codec sample.
                let raw = source.samples.take(at);
                let target = if raw.is_some() { 1.0 } else { 0.0 };
                if target != source.availability_target {
                    // Streams legitimately encode whole silent passages per object.
                    // A hard 0.67 ms edge after minutes of encoded silence is audible
                    // as stutter, so re-entry fades track the silence length while
                    // departures stay at the fast master ramp.
                    let silence = at.saturating_sub(source.last_audible_at);
                    let ramp = if target == 1.0 && silence > self.output_sample_rate as u64 {
                        self.output_sample_rate / 100 // 10 ms de-pop on long-silence re-entry
                    } else {
                        32
                    };
                    source.availability_target = target;
                    source.availability_ramp_remaining = ramp;
                    source.availability_step = (target - source.availability) / ramp as f32;
                }
                if source.availability_ramp_remaining > 0 {
                    source.availability += source.availability_step;
                    source.availability_ramp_remaining -= 1;
                    if source.availability_ramp_remaining == 0 {
                        source.availability = source.availability_target;
                    }
                }
                if raw.is_some() {
                    source.last_audible_at = at;
                }
                // Enter suspend: a muted source with no queued future PCM has
                // nothing to render until an unmute or new PCM arrives. Its
                // whole body is skipped from the next block onward.
                if source.muted
                    && !source.suspended
                    && raw.is_none()
                    && !source.samples.has_future_pcm_within(at, 4800)
                    && source.gain_events.is_empty()
                    && source.spatial_events.is_empty()
                {
                    source.suspended = true;
                }
                let sample = raw.unwrap_or(0.0)
                    * source.availability
                    * source.gain
                    * if source.muted { 0.0 } else { 1.0 };
                if source.object_id.is_some() && sample.abs() >= OBJECT_ACTIVITY_THRESHOLD {
                    source.activity_until =
                        at.saturating_add((self.output_sample_rate as f32 * 0.2).round() as u64);
                }
                if raw.is_none() && source.gain != 0.0 {
                    underruns += 1;
                }
                self.bus_renderer.as_mut().expect("checked above").add(
                    sample * ROOM_SPEAKER_REFERENCE_GAIN * if source.direct.is_some() { 1.0 - self.direct_mix } else { 1.0 },
                    &source.bus_gains,
                    block_index,
                );
                if let (Some(direct), Some(set)) = (&mut source.direct, &mut self.active_hrtf_set) {
                    if block_index == 0 { let _ = direct.update(set, self.hrtf_wet_weight, source.position, head_pose, source.spread); }
                    direct.input[block_index] = sample * ROOM_SPEAKER_REFERENCE_GAIN * self.direct_mix;
                }
                if !self.lfe_muted {
                    lfe_sum += sample * source.lfe_gain;
                }
                Self::advance_source_envelopes(source, 1);
            }
            if let Some(event) = self.program_events.remove(&at) {
                self.program_metadata_gain = event.gain;
                self.set_program_target(event.gain, false);
            }
            let binaural = self
                .bus_renderer
                .as_ref()
                .expect("checked above")
                .output_at(block_index);
            let lfe = self.lfe_path.process(lfe_sum) * 0.5;
            let compensated = self.headphone.output_at(block_index);
            self.headphone
                .add(block_index, [lfe + binaural[0] + direct_sum[0], lfe + binaural[1] + direct_sum[1]]);
            // Match master binaural ordering: summed HRTF/LFE -> headphone FIR
            // -> EQ -> +6 dB makeup -> volume/program -> linked guard.
            let equalized = self.binaural_eq.process(compensated[0], compensated[1]);
            let pre_guard = [
                equalized[0] * 10.0_f32.powf(6.0 / 20.0) * self.output_gain,
                equalized[1] * 10.0_f32.powf(6.0 / 20.0) * self.output_gain,
            ];
            let guarded = self.peak_guard.process(
                pre_guard[0] * self.program_gain,
                pre_guard[1] * self.program_gain,
            );
            if channels >= 2 {
                frame[0] = guarded[0];
                frame[1] = guarded[1];
            } else if channels == 1 {
                frame[0] = 0.5 * (guarded[0] + guarded[1]);
            }
            self.advance_output_envelopes();
            if block_index + 1 == convolution::DEFAULT_PARTITION {
                for source in self.sources.values_mut() {
                    if let Some(direct) = &mut source.direct { direct.finish_block(); }
                }
                let _ = self
                    .bus_renderer
                    .as_mut()
                    .expect("checked above")
                    .finish_block();
                let _ = self.headphone.finish_block();
            }
            self.sample_pos += 1;
            self.queue_object_activity_snapshot(self.sample_pos);
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

fn one_hot_route(bus: usize) -> RouteGains {
    let mut buses = [0.0; vbap::MAX_BUS_COUNT];
    if bus < buses.len() {
        buses[bus] = 1.0;
    }
    RouteGains { buses, lfe: 0.0 }
}

fn static_bed_position(azimuth: f32, elevation: f32) -> [f32; 3] {
    let azimuth = azimuth.to_radians();
    let elevation = elevation.to_radians();
    [
        -elevation.cos() * azimuth.sin(),
        elevation.cos() * azimuth.cos(),
        elevation.sin(),
    ]
}

/// Mirrors master label aliases, snapping a bed to the selected room's exact
/// physical speaker when it exists and VBAP-folding only labels absent there.
fn bed_route(label: &str, solver: &vbap::VbapSolver) -> RouteGains {
    let (name, position) = match label {
        "LFE" | "LFE2" | "Lfe" | "LowFrequencyEffects" | "LowFrequencyEffects2" => {
            return RouteGains { buses: [0.0; vbap::MAX_BUS_COUNT], lfe: 1.0 };
        }
        "FrontLeft" | "L" | "Left" => ("FrontLeft", static_bed_position(30.0, 0.0)),
        "FrontRight" | "R" | "Right" => ("FrontRight", static_bed_position(-30.0, 0.0)),
        "Center" | "C" => ("Center", static_bed_position(0.0, 0.0)),
        "SurroundLeft" | "Ls" | "Lsc" | "Lsd" => ("SurroundLeft", static_bed_position(100.0, 0.0)),
        "SurroundRight" | "Rs" | "Rsc" | "Rsd" => ("SurroundRight", static_bed_position(-100.0, 0.0)),
        "RearLeft" | "Lb" | "Lrs" | "SurroundLeftRear" | "RearLeftSurround" => ("RearLeft", static_bed_position(140.0, 0.0)),
        "RearRight" | "Rb" | "Rrs" | "SurroundRightRear" | "RearRightSurround" => ("RearRight", static_bed_position(-140.0, 0.0)),
        "TopFrontLeft" | "Tfl" | "Ltf" | "TopLeft" => ("TopFrontLeft", static_bed_position(45.0, 45.0)),
        "TopFrontRight" | "Tfr" | "Rtf" | "TopRight" => ("TopFrontRight", static_bed_position(-45.0, 45.0)),
        "TopRearLeft" | "Tbl" | "Ltr" | "Trl" => ("TopRearLeft", static_bed_position(135.0, 45.0)),
        "TopRearRight" | "Tbr" | "Rtr" | "Trr" => ("TopRearRight", static_bed_position(-135.0, 45.0)),
        "TopMiddleLeft" | "Tsl" | "TopSideLeft" | "Lts" | "Ltm" | "TopSurroundLeft" => ("TopMiddleLeft", static_bed_position(90.0, 45.0)),
        "TopMiddleRight" | "Tsr" | "TopSideRight" | "Rts" | "Rtm" | "TopSurroundRight" => ("TopMiddleRight", static_bed_position(-90.0, 45.0)),
        "WideLeft" | "Lw" => ("WideLeft", static_bed_position(60.0, 0.0)),
        "WideRight" | "Rw" => ("WideRight", static_bed_position(-60.0, 0.0)),
        "RearCenter" | "Cb" | "CenterSurround" => ("RearCenter", static_bed_position(180.0, 0.0)),
        "TopCenter" | "Tc" => ("TopCenter", static_bed_position(0.0, 90.0)),
        "TopFrontCenter" | "Tfc" => ("TopFrontCenter", static_bed_position(0.0, 45.0)),
        _ => ("Center", static_bed_position(0.0, 0.0)),
    };
    if let Some(bus) = solver.speaker_index(name) {
        one_hot_route(bus)
    } else {
        RouteGains { buses: solver.pan(position, 0.0), lfe: 0.0 }
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
            let mut pending_fifo_flush = None;
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
                    engine.clear_object_activity(engine.sample_pos);
                    pending_fifo_flush = Some(fifo.clear_from_producer());
                    telemetry
                        .callback_output_enabled
                        .store(false, Ordering::Release);
                    telemetry
                        .callback_consumed_sample_pos
                        .store(engine.sample_pos, Ordering::Release);
                    observed_epoch = engine.render_epoch;
                }
                if pending_fifo_flush.is_some_and(|epoch| !fifo.flush_acknowledged(epoch)) {
                    commands.wait(Duration::from_micros(500));
                    continue;
                }
                pending_fifo_flush = None;
                engine.emit_consumed_object_activity(
                    telemetry
                        .callback_consumed_sample_pos
                        .load(Ordering::Acquire),
                );
                // Only stop rendering when there is truly nothing to play and
                // nothing queued: a source gap at the exact current sample must
                // not idle the worker, because starving the FIFO makes the
                // callback drop to zeros and the refill lands as a level-step
                // crackle. render_into already emits silence for missing
                // samples through the availability ramp.
                let all_sources_silent = !engine.has_any_pcm_at(engine.sample_pos)
                    && !engine
                        .sources
                        .values()
                        .any(|source| source.samples.has_future_pcm_within(engine.sample_pos, 4800));
                if fifo.available_read() >= STEREO_FIFO_TARGET_FRAMES - 512
                    || fifo.available_write() < convolution::DEFAULT_PARTITION
                    || !engine.output_active
                    || engine.paused
                    || (all_sources_silent && fifo.available_read() == 0)
                {
                    // A 500 us idle sleep let a burst of control commands keep
                    // re-waking the loop without crossing the render gate, so
                    // the FIFO drained by hundreds of ms before rendering
                    // resumed. Wake at most every 5 ms while idling: still
                    // cheap, but the watermark is re-evaluated in time.
                    commands.wait(Duration::from_millis(5));
                    continue;
                }
                let started = Instant::now();
                engine.render_into(&mut block, 2);
                if fifo.push(&block) != convolution::DEFAULT_PARTITION {
                    // The FIFO is full and the callback is not consuming (or a
                    // flush raced us). Back off instead of spinning: a render-
                    // discard loop burned the core and pushed stale blocks
                    // through flush race windows as audible clicks.
                    commands.wait(Duration::from_millis(2));
                    continue;
                }
                telemetry.render_block_count.fetch_add(1, Ordering::Relaxed);
                // Start pulling the callback only with a solid prebuffer.
                // Enabling at a thin watermark made the callback catch up to the
                // renderer during the start burst, and every catch-up dropped to
                // zeros and refilled as an audible level step.
                if fifo.available_read() >= STEREO_FIFO_START_FRAMES {
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
    if output_enabled && popped > 0 {
        telemetry
            .callback_consumed_sample_pos
            .fetch_add(popped as u64, Ordering::Release);
    }
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
    let mut callback_output = callback_output::CallbackOutput::new(
        config.sample_rate.0,
        STEREO_FIFO_START_FRAMES,
    );
    let mut output_monitor = output_monitor::OutputMonitor::default();
    let error = |err| eprintln!("[SDA native renderer] WASAPI stream error: {err}");
    match format {
        SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                let started = Instant::now();
                let requested = data.len() / channels;
                if fifo.apply_flush_from_consumer() {
                    callback_output.reset();
                    output_monitor.reset();
                }
                let output_enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
                let popped = callback_output.fill(&fifo, output_enabled, requested, |offset, frame| {
                    let target = &mut data[offset * channels..(offset + 1) * channels];
                    target.fill(0.0);
                    target[0] = frame[0];
                    if channels > 1 {
                        target[1] = frame[1];
                    }
                });
                if output_enabled {
                    output_monitor.observe(
                        data.chunks_exact(channels).map(|frame| [frame[0], frame[1]]),
                        telemetry.callback_consumed_sample_pos.load(Ordering::Acquire),
                        &telemetry.output,
                    );
                } else {
                    output_monitor.reset();
                }
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
                if fifo.apply_flush_from_consumer() {
                    callback_output.reset();
                    output_monitor.reset();
                }
                let output_enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
                let popped = callback_output.fill(&fifo, output_enabled, requested, |offset, frame| {
                    let target = &mut data[offset * channels..(offset + 1) * channels];
                    target.fill(0);
                    target[0] = (frame[0].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    if channels > 1 {
                        target[1] = (frame[1].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                });
                if output_enabled {
                    output_monitor.observe(
                        data.chunks_exact(channels).map(|frame| [
                            frame[0] as f32 / i16::MAX as f32,
                            frame[1] as f32 / i16::MAX as f32,
                        ]),
                        telemetry.callback_consumed_sample_pos.load(Ordering::Acquire),
                        &telemetry.output,
                    );
                } else {
                    output_monitor.reset();
                }
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
                if fifo.apply_flush_from_consumer() {
                    callback_output.reset();
                    output_monitor.reset();
                }
                let output_enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
                let popped = callback_output.fill(&fifo, output_enabled, requested, |offset, frame| {
                    let target = &mut data[offset * channels..(offset + 1) * channels];
                    target.fill(u16::MAX / 2);
                    target[0] = ((frame[0].clamp(-1.0, 1.0) + 1.0) * 0.5 * u16::MAX as f32) as u16;
                    if channels > 1 {
                        target[1] = ((frame[1].clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
                    }
                });
                if output_enabled {
                    output_monitor.observe(
                        data.chunks_exact(channels).map(|frame| [
                            frame[0] as f32 / u16::MAX as f32 * 2.0 - 1.0,
                            frame[1] as f32 / u16::MAX as f32 * 2.0 - 1.0,
                        ]),
                        telemetry.callback_consumed_sample_pos.load(Ordering::Acquire),
                        &telemetry.output,
                    );
                } else {
                    output_monitor.reset();
                }
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
        callback_consumed_sample_pos: AtomicU64::new(0),
        callback_count: AtomicU64::new(0),
        callback_max_micros: AtomicU64::new(0),
        callback_fifo_underrun_frames: AtomicU64::new(0),
        output: output_monitor::OutputTelemetry::default(),
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
    fn direct_mode_keeps_beds_on_buses_and_drains_muted_objects() {
        let mut engine = calibrated_engine();
        engine.direct_objects = true;
        engine.direct_mix = 1.0;
        let mut source = Source { kind: SourceKind::Object, gain: 1.0, target_gain: 1.0, ..Source::default() };
        source.samples.write(0, 0, &[0.1; 4096]);
        engine.sources.insert("obj:14".into(), source);
        engine.sources.insert("bed:0".into(), Source::default());
        engine.route_source_now("obj:14", 0).unwrap();
        let mut audio = [0.0; 8192];
        engine.render_into(&mut audio, 2);
        assert!(audio.iter().any(|v| v.abs() > 1e-6));
        assert!(engine.sources["obj:14"].direct.is_some());
        assert!(engine.sources["bed:0"].direct.is_none());
        let source = engine.sources.get_mut("obj:14").unwrap();
        source.muted = true;
        source.suspended = true;
        engine.direct_objects = false;
        engine.render_into(&mut audio, 2);
        assert!(audio.iter().any(|v| v.abs() > 1e-8), "convolution tail must survive suspend");
        for _ in 0..40 { engine.render_into(&mut audio, 2); }
        assert!(audio.iter().all(|v| v.is_finite() && v.abs() < 1e-6));
        assert_eq!(engine.direct_mix, 0.0);
        engine.direct_objects = true;
        engine.reset_session(0);
        assert!(engine.direct_objects, "reset must retain the setting");
        assert_eq!(engine.direct_mix, 0.0);
        assert!(engine.sources.is_empty());
    }

    /// A/B/C arbitration for the "silent objects still do work" hypothesis:
    /// (A) 15 objects render; (B) 13 of them are muted; (C) those 13 are never
    /// declared at all. If silent objects polluted the signal, B != A. If mere
    /// declaration did, C != B. The mix must be identical in A and B, and C
    /// must equal B exactly (a muted source contributes an all-zero path).
    #[test]
    fn muted_objects_do_not_change_the_mix_of_their_active_peers() {
        let mut engine_a = calibrated_engine();
        let mut engine_b = calibrated_engine();
        let mut engine_c = calibrated_engine();
        let block = convolution::DEFAULT_PARTITION;
        let pcm: Vec<f32> = (0..block * 8)
            .map(|n| ((n * 37 % 97) as f32 - 48.0) / 96.0)
            .collect();
        let make_source = |id: u32| Source {
            kind: SourceKind::Object,
            object_id: Some(id),
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        for id in 10..25_u32 {
            engine_a.sources.insert(format!("obj:{id}"), make_source(id));
            engine_b.sources.insert(format!("obj:{id}"), make_source(id));
            if matches!(id, 14 | 15 | 22) {
                engine_c.sources.insert(format!("obj:{id}"), make_source(id));
            }
        }
        for engine in [&mut engine_a, &mut engine_b, &mut engine_c] {
            for id in 10..25_u32 {
                let Some(source) = engine.sources.get_mut(&format!("obj:{id}")) else {
                    continue;
                };
                source.samples.write(0, 0, &pcm);
                let _ = engine.route_source_now(&format!("obj:{id}"), 0);
            }
            engine.paused = false;
        }
        // B mutes 13 of the 15 objects after they have been routed.
        for id in 10..25_u32 {
            if matches!(id, 14 | 15 | 22) {
                continue;
            }
            engine_b.sources.get_mut(&format!("obj:{id}")).unwrap().muted = true;
        }
        let mut out_a = vec![0.0_f32; block * 4 * 2];
        let mut out_b = vec![0.0_f32; block * 4 * 2];
        let mut out_c = vec![0.0_f32; block * 4 * 2];
        // Render block by block so availability ramps settle identically.
        for index in 0..4 {
            let mut chunk_a = vec![0.0_f32; block * 2];
            let mut chunk_b = vec![0.0_f32; block * 2];
            let mut chunk_c = vec![0.0_f32; block * 2];
            engine_a.mix(&mut chunk_a, 2);
            engine_b.mix(&mut chunk_b, 2);
            engine_c.mix(&mut chunk_c, 2);
            out_a[index * block * 2..(index + 1) * block * 2].copy_from_slice(&chunk_a);
            out_b[index * block * 2..(index + 1) * block * 2].copy_from_slice(&chunk_b);
            out_c[index * block * 2..(index + 1) * block * 2].copy_from_slice(&chunk_c);
        }
        let mut first_difference = None;
        for (index, (a, b)) in out_a.iter().zip(out_b.iter()).enumerate() {
            if (a - b).abs() > 1e-6 {
                first_difference = Some((index, *a, *b));
                break;
            }
        }
        if let Some((index, a, b)) = first_difference {
            panic!(
                "muting changed the active mix at sample {index}: with_all={a} muted={b}"
            );
        }
        assert_eq!(out_b, out_c, "declaring-but-muted objects must not change the mix vs not declaring them");
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
    fn layout_bus_graph_is_independent_of_source_count() {
        let mut engine = calibrated_engine();
        let expected = engine.vbap.bus_count();
        assert_eq!(engine.bus_renderer.as_ref().unwrap().bus_count(), expected);
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
        assert_eq!(engine.bus_renderer.as_ref().unwrap().bus_count(), expected);
    }

    #[test]
    fn layout_switch_rebuilds_physical_bus_graph_and_reroutes_beds() {
        let mut engine = calibrated_engine();
        let mut bed = Source {
            kind: SourceKind::Bed,
            bed_label: Some("TopMiddleLeft".into()),
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        // A source may be declared long before its first codec block arrives.
        // Keep only its semantic label so a room change cannot later restore a
        // numeric gain vector that was computed for a different bus topology.
        Engine::set_source_route(&mut bed, bed_route("TopMiddleLeft", &engine.vbap), 0);
        engine.sources.insert("bed:0".into(), bed);

        engine.set_layout(vbap::LayoutId::Dolby5_1_2).unwrap();
        assert_eq!(engine.layout, vbap::LayoutId::Dolby5_1_2);
        assert_eq!(engine.bus_renderer.as_ref().unwrap().bus_count(), 7);
        assert_eq!(engine.sources["bed:0"].bus_gains[5], 1.0);
        assert_eq!(engine.sources["bed:0"].bus_gains[11], 0.0);

        engine.set_layout(vbap::LayoutId::Dolby9_1_6).unwrap();
        assert_eq!(engine.bus_renderer.as_ref().unwrap().bus_count(), 15);
        assert_eq!(engine.sources["bed:0"].bus_gains[5], 0.0);
        assert_eq!(engine.sources["bed:0"].bus_gains[11], 1.0);
    }

    #[test]
    fn stereo_21_is_two_hrtf_buses_plus_a_separate_lfe_path() {
        let mut engine = calibrated_engine();
        engine.set_layout(vbap::LayoutId::Stereo2_1).unwrap();
        assert_eq!(engine.bus_renderer.as_ref().unwrap().bus_count(), 2);

        let front_left = bed_route("FrontLeft", &engine.vbap);
        assert_eq!(front_left.buses, [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(front_left.lfe, 0.0);

        let lfe = bed_route("LFE", &engine.vbap);
        assert_eq!(lfe.buses, [0.0; vbap::MAX_BUS_COUNT]);
        assert_eq!(lfe.lfe, 1.0);
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
            .write(0, 0, &[0.25; convolution::DEFAULT_PARTITION * 4]);
        Engine::set_source_route(&mut bed, one_hot_route(0), 0);
        engine.sources.insert("bed:0".into(), bed);
        engine.sources.insert(
            "obj:late".into(),
            Source {
                gain: 1.0,
                target_gain: 1.0,
                ..Source::default()
            },
        );
        let total_frames = convolution::DEFAULT_PARTITION * 4;
        let mut output = vec![0.0; total_frames * 2];
        engine.mix(&mut output, 2);
        assert_eq!(engine.sample_pos, total_frames as u64);
        let delay_frames = convolution::DEFAULT_PARTITION + 240;
        assert!(
            output[..delay_frames * 2]
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert!(
            output[delay_frames * 2..]
                .iter()
                .any(|sample| *sample != 0.0)
        );
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
        let target = [0.0; vbap::MAX_BUS_COUNT];
        Engine::set_source_route(
            &mut source,
            RouteGains {
                buses: target,
                lfe: 0.0,
            },
            4,
        );
        assert_eq!(source.bus_gains[0], 1.0);
        Engine::advance_source_envelopes(&mut source, 1);
        assert!((source.bus_gains[0] - 0.75).abs() < 1e-6);
        Engine::advance_source_envelopes(&mut source, 3);
        assert_eq!(source.bus_gains, target);
    }

    #[test]
    fn queued_object_metadata_updates_the_active_render_loop_at_its_sample() {
        let mut engine = calibrated_engine();
        let mut source = Source {
            kind: SourceKind::Object,
            object_id: Some(14),
            gain: 1.0,
            target_gain: 1.0,
            availability: 1.0,
            availability_target: 1.0,
            ..Source::default()
        };
        source.samples.write(0, 0, &[0.125; 256]);
        let target_position = [1.0, 0.0, 0.0];
        source.spatial_events.insert(
            96,
            SpatialEvent {
                position: target_position,
                spread: 0.2,
                ramp: 32,
            },
        );
        source.gain_events.insert(
            96,
            GainEvent {
                gain: 0.25,
                ramp: 32,
            },
        );
        engine.sources.insert("obj:14".into(), source);
        engine.route_source_now("obj:14", 0).unwrap();
        let initial_gains = engine.sources["obj:14"].bus_gains;
        let target_gains = bus_renderer::route(&engine.vbap, target_position, None, 0.2);
        assert_ne!(initial_gains, target_gains);

        engine.mix(&mut [0.0; 96 * 2], 2);
        assert_eq!(engine.sources["obj:14"].position, [0.0, 1.0, 0.0]);
        assert_eq!(engine.sources["obj:14"].gain, 1.0);
        engine.mix(&mut [0.0; 2], 2);
        let source = &engine.sources["obj:14"];
        assert_eq!(source.position, target_position);
        assert_eq!(source.spread, 0.2);
        assert!(source.spatial_events.is_empty());
        assert!(source.gain_events.is_empty());
        assert_eq!(source.bus_ramp_remaining, 31);
        assert_eq!(source.ramp_remaining, 31);
        assert_eq!(source.gain, 1.0 + (0.25 - 1.0) / 32.0);
        for bus in 0..vbap::MAX_BUS_COUNT {
            let expected = initial_gains[bus] + (target_gains[bus] - initial_gains[bus]) / 32.0;
            assert!((source.bus_gains[bus] - expected).abs() < 1e-6);
        }

        engine.mix(&mut [0.0; 31 * 2], 2);
        assert_eq!(engine.sources["obj:14"].bus_gains, target_gains);
        assert_eq!(engine.sources["obj:14"].gain, 0.25);
        assert_eq!(engine.route_update_count, 1);
    }

    #[test]
    fn queued_gain_is_applied_while_an_object_is_suspended() {
        let mut engine = calibrated_engine();
        let mut source = Source {
            kind: SourceKind::Object,
            object_id: Some(22),
            gain: 1.0,
            target_gain: 1.0,
            muted: true,
            suspended: true,
            ..Source::default()
        };
        source.gain_events.insert(
            64,
            GainEvent {
                gain: 0.25,
                ramp: 32,
            },
        );
        engine.sources.insert("obj:22".into(), source);

        engine.mix(&mut [0.0; 64 * 2], 2);
        assert!(engine.sources["obj:22"].suspended);
        assert_eq!(engine.sources["obj:22"].gain, 1.0);
        engine.mix(&mut [0.0; 2], 2);
        let source = &engine.sources["obj:22"];
        assert_eq!(source.target_gain, 0.25);
        assert_eq!(source.ramp_remaining, 31);
        assert_eq!(source.gain, 1.0 + (0.25 - 1.0) / 32.0);
        assert!(source.gain_events.is_empty());

        engine.mix(&mut [0.0; 31 * 2], 2);
        assert_eq!(engine.sources["obj:22"].gain, 0.25);
        assert_eq!(engine.sources["obj:22"].ramp_remaining, 0);
    }

    #[test]
    fn object_activity_uses_post_source_gain_and_waits_for_dac_consumption() {
        let mut engine = calibrated_engine();
        let mut object = Source {
            kind: SourceKind::Object,
            object_id: Some(7),
            gain: 1.0,
            target_gain: 1.0,
            availability: 1.0,
            availability_target: 1.0,
            ..Source::default()
        };
        object.samples.write(0, 0, &[OBJECT_ACTIVITY_THRESHOLD]);
        engine.sources.insert("obj:7".into(), object);

        let mut bed = Source {
            kind: SourceKind::Bed,
            gain: 1.0,
            target_gain: 1.0,
            availability: 1.0,
            availability_target: 1.0,
            ..Source::default()
        };
        bed.samples.write(0, 0, &[1.0]);
        engine.sources.insert("bed:0".into(), bed);

        engine.mix(&mut [0.0; 2], 2);
        let snapshot = engine
            .activity_snapshots
            .front()
            .expect("object snapshot queued");
        assert_eq!(snapshot.sample_pos, 1);
        assert_eq!(snapshot.active_ids(), &[7]);
        assert_eq!(engine.last_emitted_activity.active_ids(), &[] as &[u32]);

        engine.clear_object_activity(engine.sample_pos);
        assert!(engine.activity_snapshots.is_empty());
        assert_eq!(engine.last_emitted_activity.active_ids(), &[] as &[u32]);
    }

    #[test]
    fn object_activity_holds_for_200ms_and_excludes_muted_sources() {
        let mut engine = calibrated_engine();
        let hold = (engine.output_sample_rate as f32 * 0.2).round() as u64;
        let source = Source {
            kind: SourceKind::Object,
            object_id: Some(9),
            gain: 1.0,
            target_gain: 1.0,
            activity_until: hold,
            ..Source::default()
        };
        engine.sources.insert("obj:9".into(), source);

        engine.queue_object_activity_snapshot(hold);
        assert_eq!(
            engine.activity_snapshots.front().unwrap().active_ids(),
            &[9]
        );

        engine.activity_snapshots.clear();
        engine.next_activity_tick = hold + 1;
        engine.queue_object_activity_snapshot(hold + 1);
        assert_eq!(
            engine.activity_snapshots.front().unwrap().active_ids(),
            &[] as &[u32]
        );

        let source = engine.sources.get_mut("obj:9").unwrap();
        source.activity_until = hold * 2;
        source.muted = true;
        engine.next_activity_tick = hold * 2;
        engine.queue_object_activity_snapshot(hold * 2);
        assert_eq!(
            engine.activity_snapshots.back().unwrap().active_ids(),
            &[] as &[u32]
        );
    }

    #[test]
    fn reset_discards_old_sources_pose_and_lfe_tail() {
        let mut engine = calibrated_engine();
        engine.head_pose = Some([0.0, 0.0, 0.5, 0.5]);
        engine.lfe_muted = true;
        let _ = engine.lfe_path.process(1.0);
        engine.sources.insert(
            "bed:0".into(),
            Source {
                gain: 1.0,
                target_gain: 1.0,
                ..Source::default()
            },
        );
        engine.reset_session(0);
        assert!(engine.sources.is_empty());
        assert!(engine.head_pose.is_none());
        assert!(!engine.lfe_muted);
        assert_eq!(engine.lfe_path.process(0.0), 0.0);
        assert!(!engine.output_active);
        assert!(engine.paused);
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
    fn bed_labels_select_fixed_buses_and_lfe_bypasses_them() {
        let solver = vbap::VbapSolver::new();
        let front_left = bed_route("FrontLeft", &solver);
        assert_eq!(front_left.buses[0], 1.0);
        assert_eq!(front_left.buses.iter().skip(1).sum::<f32>(), 0.0);
        assert_eq!(front_left.lfe, 0.0);

        let rear_right = bed_route("Rrs", &solver);
        assert_eq!(rear_right.buses[6], 1.0);
        assert_eq!(rear_right.lfe, 0.0);

        for label in [
            "LFE",
            "LFE2",
            "Lfe",
            "LowFrequencyEffects",
            "LowFrequencyEffects2",
        ] {
            let route = bed_route(label, &solver);
            assert_eq!(route.buses, [0.0; vbap::MAX_BUS_COUNT], "{label}");
            assert_eq!(route.lfe, 1.0, "{label}");
        }
    }

    #[test]
    fn lfe_path_is_lowpassed_and_partition_aligned() {
        let mut lfe = LfePath::new(48_000);
        for _ in 0..convolution::DEFAULT_PARTITION {
            assert_eq!(lfe.process(1.0), 0.0);
        }
        let delayed = lfe.process(1.0);
        assert!(delayed > 0.0 && delayed < 1.0);

        let mut high = LfePath::new(48_000);
        let mut low = LfePath::new(48_000);
        let mut high_square = 0.0;
        let mut low_square = 0.0;
        for sample in 0..(48_000 + convolution::DEFAULT_PARTITION) {
            let low_input = (std::f32::consts::TAU * 60.0 * sample as f32 / 48_000.0).sin();
            let high_input = (std::f32::consts::TAU * 240.0 * sample as f32 / 48_000.0).sin();
            let low_output = low.process(low_input);
            let high_output = high.process(high_input);
            if sample >= 24_000 {
                low_square += low_output * low_output;
                high_square += high_output * high_output;
            }
        }
        assert!(high_square < low_square * 0.02);
    }

    #[test]
    fn lfe_mute_leaves_non_lfe_bus_routing_unchanged() {
        let mut source = Source {
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        Engine::set_source_route(
            &mut source,
            RouteGains {
                buses: [0.0; vbap::MAX_BUS_COUNT],
                lfe: 1.0,
            },
            0,
        );
        let sample = 0.75;
        let unmuted_lfe = sample * source.lfe_gain;
        let muted_lfe = 0.0_f32;
        assert_eq!(unmuted_lfe, sample);
        assert_eq!(muted_lfe, 0.0);
        Engine::set_source_route(&mut source, one_hot_route(2), 0);
        assert_eq!(source.lfe_gain, 0.0);
        assert_eq!(source.bus_gains[2], 1.0);
    }
}
