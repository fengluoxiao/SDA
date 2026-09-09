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

use serde::{Deserialize, Serialize};

const PROTOCOL: u32 = 7;
const MAX_SOURCES: usize = 128;
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
// Calibrated HRTFs and VBAP already define speaker levels. Acoustic reference
// SPL and a program's LKFS delivery limit do not imply per-source attenuation.
const ROOM_SPEAKER_REFERENCE_GAIN: f32 = 1.0;
const STEREO_FIFO_CAPACITY_FRAMES: usize = 32_768;
const STEREO_FIFO_TARGET_FRAMES: usize = 16_384;
/// Prebuffer before the WASAPI callback may start pulling: 8192 frames is
/// about 170 ms of program, enough to absorb decode jitter at startup.
const STEREO_FIFO_START_FRAMES: usize = 8_192;
/// Keep browser object-activity semantics: −60 dBFS source signal held for 200 ms.
const OBJECT_ACTIVITY_THRESHOLD: f32 = 0.001;
const OBJECT_ACTIVITY_QUEUE_CAPACITY: usize = 16;

mod bus_renderer;
mod adm_zone;
mod direct_renderer;
mod callback_output;
mod convolution;
mod dsp;
#[allow(dead_code)]
mod headphone;
mod cinema;
mod focus;
mod hrtf;
mod output_monitor;
mod monitor;
mod hardware;
mod pcm_ring;
mod protocol;
mod render_command;
mod spatial;
mod stereo_fifo;
mod vbap;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SpeakerFocus {
    Single(String),
    Multiple(Vec<String>),
}

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
    SetSpeakerMutes { names: Vec<String>, focus: Option<SpeakerFocus> },
    SetVolume {
        volume: f32,
    },
    SetComparisonGain { #[serde(rename = "gainDb")] gain_db: f32 },
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
    SetStereoMode { mode: StereoMode },
    SetCinema { settings: cinema::Settings, profile: Option<String> },
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
    ListOutputDevices,
    SetOutputDevice { #[serde(flatten)] settings: output_manager::Settings },
    Shutdown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Event<'a> {
    OutputDevices {status:output_manager::Status,devices:Vec<output_manager::Endpoint>},
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
    #[serde(default)]
    diffuse: f32,
    #[serde(default)]
    horizontal_only: bool,
    #[serde(default)]
    zone_exclusion: Vec<adm_zone::Zone>,
    #[serde(default = "default_object_ramp")]
    ramp_duration: u32,
}

fn default_object_ramp() -> u32 {
    DEFAULT_OBJECT_RAMP
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

#[derive(Clone)]
struct SpatialEvent {
    position: [f32; 3],
    spread: f32,
    diffuse: f32,
    horizontal_only: bool,
    zone_exclusion: std::sync::Arc<[adm_zone::Zone]>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum StereoMode { Original, Dry, Room }

struct Source {
    bass_split: Option<cinema::BassSplit>,
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
    diffuse: f32,
    horizontal_only: bool,
    zone_exclusion: std::sync::Arc<[adm_zone::Zone]>,
    spatial_events: BTreeMap<u64, SpatialEvent>,
    motion: Option<SpatialEvent>,
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
            bass_split: None,
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
            diffuse: 0.0,
            horizontal_only: false,
            zone_exclusion: Default::default(),
            spatial_events: BTreeMap::new(),
            motion: None,
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

#[cfg_attr(test, derive(Default))]
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
    hardware_lfe: hardware::Chain,
    hardware_stereo: [hardware::Chain; 2],
    lfe_muted: bool,
    speaker_mutes: Vec<String>,
    focused_speakers: Vec<String>,
    speaker_levels: [f32; vbap::MAX_BUS_COUNT],
    speaker_background: [f32; vbap::MAX_BUS_COUNT],
    stereo_mode: StereoMode,
    cinema: cinema::Settings,
    cinema_bass_mix: f32,
    cinema_bass_delay: [f32; convolution::DEFAULT_PARTITION],
    cinema_sub_delay: Vec<f32>,
    cinema_sub_cursor: usize,
    room_profile: Option<Arc<cinema::RoomProfile>>,
    stereo_weights: [f32; 3],
    stereo_delay: [[f32; 2]; convolution::DEFAULT_PARTITION],
    stereo_background: [focus::BackgroundFilter; 2],
    stereo_dry_bus: Option<bus_renderer::BusRenderer>,
    speaker_lfe_level: f32,
    output_gain: f32,
    comparison_gain: f32,
    comparison_target: f32,
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
    fn set_speaker_monitor(&mut self, names: Vec<String>, focus: Vec<String>) {
        self.speaker_mutes = if !focus.is_empty() { Vec::new() } else { names };
        self.focused_speakers = focus;
    }

    fn speaker_target(&self, name: &str) -> f32 {
        if self.speaker_mutes.iter().any(|muted| muted == name) { return 0.0; }
        let has_focus = self.focused_speakers.iter().any(|focus| {
            vbap::speakers(self.layout).iter().any(|speaker| speaker.name == focus)
                || (focus == "LFE" && self.layout != vbap::LayoutId::Stereo2_0)
        });
        if has_focus && !self.focused_speakers.iter().any(|focus| focus == name) { focus::BACKGROUND_GAIN } else { 1.0 }
    }

    fn new(sample_rate: u32, channels: u16) -> Self {
        let _ = direct_renderer::workers();
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
            hardware_lfe: hardware::Chain::new(&Default::default()),
            hardware_stereo: std::array::from_fn(|_| hardware::Chain::new(&Default::default())),
            lfe_muted: false,
            speaker_mutes: Vec::new(),
            focused_speakers: Vec::new(),
            speaker_levels: [1.0; vbap::MAX_BUS_COUNT],
            speaker_background: [0.0; vbap::MAX_BUS_COUNT],
            stereo_mode: StereoMode::Room,
            cinema: cinema::Settings::default(),
            cinema_bass_mix: 0.0,
            cinema_bass_delay: [0.0; convolution::DEFAULT_PARTITION],
            cinema_sub_delay: Vec::new(),
            cinema_sub_cursor: 0,
            room_profile: None,
            stereo_weights: [0.0, 0.0, 1.0],
            stereo_delay: [[0.0; 2]; convolution::DEFAULT_PARTITION],
            stereo_background: std::array::from_fn(|_| focus::BackgroundFilter::default()),
            stereo_dry_bus: None,
            speaker_lfe_level: 1.0,
            output_gain: 1.0,
            comparison_gain: 1.0,
            comparison_target: 1.0,
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
        if enabled && !self.cinema.monitor.hardware.enabled {
            let set = self.active_hrtf_set.as_mut().ok_or("native HRTF set is not configured")?;
            let mut prepared = Vec::new();
            for (id, source) in &self.sources {
                if source.kind == SourceKind::Object && source.direct.is_none() {
                    let mut direct = direct_renderer::DirectSource::new(set, self.hrtf_wet_weight)?;
                    direct.update(set, &self.vbap, self.hrtf_wet_weight, std::array::from_fn(|bus| source.bus_gains[bus] * self.speaker_levels[bus]))?;
                    prepared.push((id.clone(), direct));
                }
            }
            for (id, direct) in prepared { self.sources.get_mut(&id).unwrap().direct = Some(direct); }
        }
        self.direct_objects = enabled;
        Ok(())
    }

    fn rebuild_bus_renderer(&mut self) -> Result<(), String> {
        self.stereo_dry_bus = None;
        if let Some(set) = &mut self.active_hrtf_set { set.configure_cinema(self.cinema.clone(), self.room_profile.clone()); }
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
        if let Some(mut motion) = source.motion.take() {
            let elapsed = samples.min(motion.ramp);
            let fraction = elapsed as f32 / motion.ramp.max(1) as f32;
            for axis in 0..3 {
                source.position[axis] += (motion.position[axis] - source.position[axis]) * fraction;
            }
            source.spread += (motion.spread - source.spread) * fraction;
            source.diffuse += (motion.diffuse - source.diffuse) * fraction;
            motion.ramp -= elapsed;
            source.motion = if motion.ramp == 0 {
                source.position = motion.position;
                source.spread = motion.spread;
                source.diffuse = motion.diffuse;
                None
            } else { Some(motion) };
        }
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
        let (position, spread, diffuse, horizontal_only, zones, kind) = self
            .sources
            .get(id)
            .map(|source| (source.position, source.spread, source.diffuse, source.horizontal_only, source.zone_exclusion.clone(), source.kind))
            .ok_or("unknown source")?;
        if kind != SourceKind::Object {
            return Ok(());
        }
        let route = RouteGains {
            buses: bus_renderer::route_zoned(&self.vbap, position, self.head_pose, spread, diffuse, horizontal_only, &zones),
            lfe: 0.0,
        };
        let source = self.sources.get_mut(id).expect("source was checked above");
        if source.motion.is_some() {
            Self::route_motion_block(source, &self.vbap, self.head_pose, convolution::DEFAULT_PARTITION as u32);
        } else {
            Self::set_source_route(source, route, ramp);
        }
        Ok(())
    }

    fn start_source_motion(source: &mut Source, event: SpatialEvent) -> bool {
        if source.motion.is_none() && source.position == event.position && source.spread == event.spread && source.diffuse == event.diffuse && source.horizontal_only == event.horizontal_only && source.zone_exclusion == event.zone_exclusion {
            return false;
        }
        source.horizontal_only = event.horizontal_only;
        source.zone_exclusion = event.zone_exclusion.clone();
        if event.ramp == 0 {
            source.position = event.position;
            source.spread = event.spread;
            source.diffuse = event.diffuse;
            source.motion = None;
        } else {
            source.motion = Some(event);
        }
        true
    }

    fn route_motion_block(source: &mut Source, solver: &vbap::VbapSolver, head: Option<[f32; 4]>, samples: u32) {
        let Some(motion) = source.motion.as_ref() else { return; };
        let samples = samples.min(motion.ramp).max(1);
        let fraction = samples as f32 / motion.ramp as f32;
        let position = std::array::from_fn(|axis| source.position[axis] + (motion.position[axis] - source.position[axis]) * fraction);
        let spread = source.spread + (motion.spread - source.spread) * fraction;
        let diffuse = source.diffuse + (motion.diffuse - source.diffuse) * fraction;
        // Re-pan points along the Cartesian trajectory, not just its endpoints.
        // Gain interpolation only bridges this short segment of the route.
        Self::set_source_route(source, RouteGains {
            buses: bus_renderer::route_zoned(solver, position, head, spread, diffuse, source.horizontal_only, &source.zone_exclusion), lfe: 0.0,
        }, samples);
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
        self.comparison_gain += (self.comparison_target - self.comparison_gain) / 960.0;
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
        self.cinema_bass_delay.fill(0.0);
        self.cinema_sub_delay.fill(0.0);
        self.cinema_sub_cursor = 0;
        self.cinema_bass_mix = 0.0;
        self.stereo_delay.fill([0.0; 2]);
        self.stereo_background = std::array::from_fn(|_| focus::BackgroundFilter::default());
        self.stereo_dry_bus = None;
        self.stereo_weights = [0.0, 0.0, 1.0];
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
        self.hardware_lfe.reset();
        for chain in &mut self.hardware_stereo { chain.reset(); }
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

    /// Wake probe for suspended sources, run at most once per convolution block
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
    /// stopping all beds and objects at the next convolution boundary.
    fn render_into(&mut self, output: &mut [f32], channels: usize) {
        output.fill(0.0);
        if self.paused || !self.output_active || self.bus_renderer.is_none() {
            return;
        }
        let stereo = self.sources.len() == 2
            && self.sources.values().all(|source| source.kind == SourceKind::Bed)
            && self.sources.values().any(|source| matches!(source.bed_label.as_deref(), Some("FrontLeft" | "L" | "Left")))
            && self.sources.values().any(|source| matches!(source.bed_label.as_deref(), Some("FrontRight" | "R" | "Right")));
        if stereo && self.stereo_dry_bus.is_none() {
            self.stereo_dry_bus = self.active_hrtf_set.as_ref()
                .and_then(|set| bus_renderer::BusRenderer::new(set, &self.vbap, 0.0).ok());
        }
        let stereo_index = if stereo && self.stereo_dry_bus.is_some() {
            match self.stereo_mode { StereoMode::Original => 0, StereoMode::Dry => 1, StereoMode::Room => 2 }
        } else { 2 };
        let mut underruns = 0_u64;
        let vbap = self.vbap.clone();
        let head_pose = self.head_pose;
        let speaker_targets: [f32; vbap::MAX_BUS_COUNT] = std::array::from_fn(|bus| {
            vbap::speakers(self.layout).get(bus).map_or(1.0, |speaker| self.speaker_target(speaker.name))
        });
        let lfe_target = self.speaker_target("LFE");
        for frame in output.chunks_exact_mut(channels) {
            let at = self.sample_pos;
            let block_index = self.block_offset;
            let bass_target = if self.cinema.monitor.enabled && self.cinema.monitor.bass_enabled && self.layout != vbap::LayoutId::Stereo2_0 { 1.0 } else { 0.0 };
            self.cinema_bass_mix += (bass_target - self.cinema_bass_mix) / 256.0;
            let bass_output = self.cinema_bass_delay[block_index];
            self.cinema_bass_delay[block_index] = 0.0;
            for (i, weight) in self.stereo_weights.iter_mut().enumerate() {
                let target = if i == stereo_index { 1.0 } else { 0.0 };
                *weight += (target - *weight) / 256.0;
            }
            for (level, target) in self.speaker_levels.iter_mut().zip(speaker_targets) {
                *level += (target - *level).clamp(-1.0 / 2048.0, 1.0 / 2048.0);
            }
            for (amount, target) in self.speaker_background.iter_mut().zip(speaker_targets) {
                let background = if target == focus::BACKGROUND_GAIN { 1.0 } else { 0.0 };
                *amount += (background - *amount).clamp(-1.0 / 2048.0, 1.0 / 2048.0);
            }
            self.speaker_lfe_level += (lfe_target - self.speaker_lfe_level).clamp(-1.0 / 2048.0, 1.0 / 2048.0);
            if block_index == 0 {
                self.bus_renderer
                    .as_mut()
                    .expect("checked above")
                    .begin_block();
                self.headphone.begin_block();
                if let Some(bus) = &mut self.stereo_dry_bus { bus.begin_block(); }
            }
            let original = std::array::from_fn::<_, 2, _>(|ear| self.hardware_stereo[ear].process(self.stereo_delay[block_index][ear]));
            self.stereo_delay[block_index] = [0.0; 2];
            let mut lfe_sum = 0.0_f32;
            let mut direct_sum = [0.0_f32; 2];
            // Fade the excitation, retaining both paths' convolution tails.
            let effective_direct = self.direct_objects && !self.cinema.monitor.hardware.enabled;
            let target_mix = if effective_direct { 1.0 } else { 0.0 };
            self.direct_mix += (target_mix - self.direct_mix).clamp(-1.0 / 9600.0, 1.0 / 9600.0);
            for source in self.sources.values_mut() {
                if source.kind == SourceKind::Object && effective_direct && source.direct.is_none() {
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
                    let mut changed = false;
                    if let Some(event) = source.spatial_events.remove(&at) {
                        changed = Self::start_source_motion(source, event);
                    }
                    // Preserve the authored motion resolution independently of
                    // the FFT partition used by long room/headphone filters.
                    const MOTION_QUANTUM: usize = 128;
                    if source.motion.is_some() && (changed || block_index % MOTION_QUANTUM == 0) {
                        Self::route_motion_block(source, &vbap, head_pose, (MOTION_QUANTUM - block_index % MOTION_QUANTUM) as u32);
                        self.route_update_count = self.route_update_count.saturating_add(1);
                    } else if changed {
                        Self::set_source_route(source, RouteGains {
                            buses: bus_renderer::route_zoned(&vbap, source.position, head_pose, source.spread, source.diffuse, source.horizontal_only, &source.zone_exclusion),
                            lfe: 0.0,
                        }, 0);
                        self.route_update_count = self.route_update_count.saturating_add(1);
                    }
                }
                if let Some(event) = source.gain_events.remove(&at) {
                    source.target_gain = event.gain;
                    source.ramp_remaining = event.ramp;
                    source.ramp_step = if event.ramp == 0 {
                        source.gain = event.gain;
                        0.0
                    } else {
                        (event.gain - source.gain) / event.ramp as f32
                    };
                    source.suspended = false;
                }
                if source.suspended {
                    if let Some(direct) = &mut source.direct {
                        if block_index == 0 { direct.schedule_focus(self.layout, self.hrtf_wet_weight, std::array::from_fn(|bus| source.bus_gains[bus] * self.speaker_levels[bus]), self.speaker_background); }
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
                let mut sample = raw.unwrap_or(0.0)
                    * source.availability
                    * source.gain
                    * if source.muted { 0.0 } else { 1.0 };
                let original_sample = sample;
                if (self.cinema_bass_mix > 1e-6 || bass_target > 0.0) && source.lfe_gain == 0.0 {
                    if source.bass_split.as_ref().is_none_or(|filter| filter.frequency != self.cinema.monitor.crossover_hz) {
                        source.bass_split = cinema::BassSplit::new(self.cinema.monitor.crossover_hz).ok();
                    }
                    if let Some(filter) = &mut source.bass_split {
                        let (low, high) = filter.process(sample);
                        let contribution: f32 = source.bus_gains.iter().zip(&self.speaker_levels).map(|(gain, level)| gain * level).sum();
                        self.cinema_bass_delay[block_index] += low * contribution * self.cinema_bass_mix;
                        sample += (high - sample) * self.cinema_bass_mix;
                    }
                }
                if source.object_id.is_some() && sample.abs() >= OBJECT_ACTIVITY_THRESHOLD {
                    source.activity_until =
                        at.saturating_add((self.output_sample_rate as f32 * 0.2).round() as u64);
                }
                if raw.is_none() && source.gain != 0.0 {
                    underruns += 1;
                }
                if stereo {
                    let ear = if matches!(source.bed_label.as_deref(), Some("FrontLeft" | "L" | "Left")) { 0 } else { 1 };
                    self.stereo_delay[block_index][ear] += original_sample;
                    if let Some(bus) = &mut self.stereo_dry_bus {
                        bus.add(sample * ROOM_SPEAKER_REFERENCE_GAIN,
                            &std::array::from_fn(|i| source.bus_gains[i] * self.speaker_levels[i]), block_index);
                    }
                }
                // ADM masters carry silent PCM for inactive objects. Keep their
                // clocks, filters and envelopes running, but avoid zero bus work.
                let bus_sample = sample * ROOM_SPEAKER_REFERENCE_GAIN
                    * if source.direct.is_some() { 1.0 - self.direct_mix } else { 1.0 };
                if bus_sample != 0.0 {
                    self.bus_renderer.as_mut().expect("checked above").add(
                        bus_sample,
                        &std::array::from_fn(|bus| source.bus_gains[bus] * self.speaker_levels[bus]),
                        block_index,
                    );
                }
                if let Some(direct) = &mut source.direct {
                    if block_index == 0 { direct.schedule_focus(self.layout, self.hrtf_wet_weight, std::array::from_fn(|bus| source.bus_gains[bus] * self.speaker_levels[bus]), self.speaker_background); }
                    direct.input[block_index] = sample * ROOM_SPEAKER_REFERENCE_GAIN * self.direct_mix;
                }
                if !self.lfe_muted {
                    lfe_sum += sample * source.lfe_gain * self.speaker_lfe_level;
                }
                Self::advance_source_envelopes(source, 1);
            }
            for ear in 0..2 {
                let input = self.stereo_delay[block_index][ear] * self.speaker_levels[ear];
                let filtered = self.stereo_background[ear].process(input);
                self.stereo_delay[block_index][ear] = input + (filtered - input) * self.speaker_background[ear];
            }
            if let Some(event) = self.program_events.remove(&at) {
                self.program_metadata_gain = event.gain;
                self.set_program_target(event.gain, false);
            }
            self.bus_renderer.as_mut().expect("checked above").shape_background(block_index, &self.speaker_background);
            if let Some(bus) = &mut self.stereo_dry_bus { bus.shape_background(block_index, &self.speaker_background); }
            let dry = self.stereo_dry_bus.as_ref().map_or([0.0; 2], |bus| bus.output_at(block_index));
            let binaural = self
                .bus_renderer
                .as_ref()
                .expect("checked above")
                .output_at(block_index);
            let mut lfe = self.lfe_path.process(lfe_sum) * 0.5
                + bass_output * cinema::db(self.cinema.monitor.bass_db) * self.speaker_lfe_level * if self.lfe_muted { 0.0 } else { 1.0 };
            lfe = self.hardware_lfe.process(lfe);
            if self.cinema.enabled {
                lfe *= self.cinema.speakers.get("LFE").map_or(1.0, |s| cinema::db(s.gain_db));
            }
            lfe *= self.cinema.monitor.gain("LFE");
            {
                if !self.cinema_sub_delay.is_empty() {
                    let delayed = self.cinema_sub_delay[self.cinema_sub_cursor];
                    self.cinema_sub_delay[self.cinema_sub_cursor] = lfe;
                    self.cinema_sub_cursor = (self.cinema_sub_cursor + 1) % self.cinema_sub_delay.len();
                    lfe = delayed;
                }
            }
            let compensated = self.headphone.output_at(block_index);
            self.headphone
                .add(block_index, std::array::from_fn(|ear|
                    original[ear] * self.stereo_weights[0] + (dry[ear] + lfe) * self.stereo_weights[1]
                    + (lfe + binaural[ear] + direct_sum[ear]) * self.stereo_weights[2]));
            // Match master binaural ordering: summed HRTF/LFE -> headphone FIR
            // -> EQ -> +6 dB makeup -> volume/program -> linked guard.
            let equalized = self.binaural_eq.process(compensated[0], compensated[1]);
            let pre_guard = [
                equalized[0] * 10.0_f32.powf(6.0 / 20.0) * self.output_gain * self.comparison_gain,
                equalized[1] * 10.0_f32.powf(6.0 / 20.0) * self.output_gain * self.comparison_gain,
            ];
            let guarded = self.peak_guard.process(
                pre_guard[0] * self.program_gain * self.cinema.monitor.master_gain(),
                pre_guard[1] * self.program_gain * self.cinema.monitor.master_gain(),
            );
            if channels >= 2 {
                frame[0] = guarded[0];
                frame[1] = guarded[1];
            } else if channels == 1 {
                frame[0] = 0.5 * (guarded[0] + guarded[1]);
            }
            self.advance_output_envelopes();
            if block_index + 1 == convolution::DEFAULT_PARTITION {
                if let Some(set) = &mut self.active_hrtf_set {
                    let _ = direct_renderer::finish_sources(self.sources.values_mut().filter_map(|source| source.direct.as_mut()),
                        set, &self.vbap, self.hrtf_wet_weight);
                }
                let _ = self
                    .bus_renderer
                    .as_mut()
                    .expect("checked above")
                    .finish_block();
                let _ = self.headphone.finish_block();
                if let Some(bus) = &mut self.stereo_dry_bus { let _ = bus.finish_block(); }
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
        // A 7.1.2 bed's middle-height channel spans the same-side overhead pair
        // in .4 layouts. Generic 3D VBAP can leak it into ear-level surrounds.
        let overhead_pair = match name {
            "TopMiddleLeft" => Some(["TopFrontLeft", "TopRearLeft"]),
            "TopMiddleRight" => Some(["TopFrontRight", "TopRearRight"]),
            _ => None,
        };
        if let Some([front, rear]) = overhead_pair {
            if let (Some(front), Some(rear)) = (solver.speaker_index(front), solver.speaker_index(rear)) {
                let mut route = RouteGains { buses: [0.0; vbap::MAX_BUS_COUNT], lfe: 0.0 };
                route.buses[front] = std::f32::consts::FRAC_1_SQRT_2;
                route.buses[rear] = std::f32::consts::FRAC_1_SQRT_2;
                return route;
            }
        }
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

mod device_output;

mod output_manager;
fn main() {
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
    spawn_render_worker(Engine::new(48000, 2), commands.clone(), fifo.clone(), telemetry.clone());
    output_manager::run(fifo, telemetry, commands);
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
    fn hardware_uses_summed_buses_with_room_disabled_and_retains_direct_preference() {
        let render = |direct: bool, hardware_enabled: bool, gains: &[f32]| {
            let mut engine = calibrated_engine();
            engine.cinema.enabled = false;
            engine.cinema.monitor.hardware.enabled = hardware_enabled;
            engine.cinema.monitor.hardware.input_db = 0.0;
            engine.cinema.monitor.hardware.rail_v = 1.0;
            engine.rebuild_bus_renderer().unwrap();
            let pcm: Vec<f32> = (0..4096).map(|i| 0.1*(std::f32::consts::TAU*i as f32/48.0).sin()).collect();
            for (id,gain) in gains.iter().enumerate() {
                let mut source=Source {kind:SourceKind::Object,gain:1.0,target_gain:1.0,
                    availability:1.0,availability_target:1.0,..Source::default()};
                source.samples.write(0,0,&pcm.iter().map(|v|v*gain).collect::<Vec<_>>());
                let key=format!("obj:{id}");engine.sources.insert(key.clone(),source);
                engine.route_source_now(&key,0).unwrap();
            }
            engine.set_direct_objects(direct).unwrap();
            let mut output=vec![0.0;8192];engine.render_into(&mut output,2);
            assert_eq!(engine.direct_objects,direct);
            if hardware_enabled {assert!(engine.sources.values().all(|s|s.direct.is_none()));}
            output
        };
        let split=render(true,true,&[2.0,-1.0]);
        let summed=render(false,true,&[1.0]);
        assert!(split.iter().zip(&summed).all(|(a,b)|(a-b).abs()<1e-6));
        assert!(summed.iter().any(|v|v.abs()>1e-5));
        let bypass=render(false,false,&[1.0]);
        let energy=|v:&[f32]|v.iter().map(|x|x*x).sum::<f32>();
        assert!(energy(&summed)<energy(&bypass)*0.5);
    }

    #[test]
    fn headphone_identity_matches_bypass_through_full_output_chain() {
        let render=|identity:bool| {
            let mut engine=calibrated_engine();
            if identity {
                let mut fir=vec![0.0;8192];fir[0]=1.0;
                engine.headphone=headphone::HeadphoneCompensation::new(&fir,&fir,1.0).unwrap();
            }
            let pcm:Vec<f32>=(0..16384).map(|i|0.005*((i*37%97) as f32-48.0)).collect();
            let mut source=Source {kind:SourceKind::Object,gain:1.0,target_gain:1.0,
                availability:1.0,availability_target:1.0,..Source::default()};
            source.samples.write(0,0,&pcm);engine.sources.insert("obj:0".into(),source);
            engine.route_source_now("obj:0",0).unwrap();
            let mut output=vec![0.0;pcm.len()*2];engine.render_into(&mut output,2);output
        };
        let bypass=render(false);let identity=render(true);
        assert!(bypass.iter().any(|v|v.abs()>1e-4));
        let max_error=bypass.iter().zip(&identity).map(|(a,b)|(a-b).abs()).fold(0.0_f32,f32::max);
        assert!(max_error<1e-6,"identity output changed by {max_error}");
    }

    #[test]
    #[ignore = "offline performance measurement"]
    fn benchmark_adm_full_engine() {
        let mut engine = calibrated_engine();
        engine.direct_objects = true;
        engine.direct_mix = 1.0;
        for id in 0..118 {
            let mut source = Source { kind: if id < 108 { SourceKind::Object } else { SourceKind::Bed },
                gain: 1.0, target_gain: 1.0, availability: 1.0, availability_target: 1.0,
                ..Source::default() };
            if id >= 108 { source.bed_label = Some(vbap::speakers(engine.layout)[id - 108].name.into()); }
            engine.sources.insert(format!("source:{id}"), source);
            engine.route_source_now(&format!("source:{id}"), 0).unwrap();
        }
        engine.set_direct_objects(true).unwrap();
        for moving in [false, true] {
            let mut times = Vec::new();
            let mut checksum = 0.0_f64;
            for block in 0..220 {
                let now = engine.sample_pos;
                for (id, source) in engine.sources.values_mut().enumerate() {
                    let pcm: [f32; convolution::DEFAULT_PARTITION] = std::array::from_fn(|i|
                        ((now as usize + i + id) as f32 * 0.13).sin() * 0.001);
                    source.samples.write(now, now, &pcm);
                    if source.kind == SourceKind::Object {
                        let phase = id as f32 * 0.17 + if moving { block as f32 * 0.01 } else { 0.0 };
                        let buses = bus_renderer::route(&engine.vbap, [phase.cos() * 0.7, phase.sin() * 0.7, 0.4], None, 0.3);
                        Engine::set_source_route(source, RouteGains { buses, lfe: 0.0 }, 128);
                    }
                }
                let mut output = [0.0; convolution::DEFAULT_PARTITION * 2];
                let start = Instant::now();
                engine.render_into(&mut output, 2);
                if block >= 20 { times.push(start.elapsed().as_secs_f64() * 1e6); }
                checksum += output.iter().map(|v| *v as f64).sum::<f64>();
            }
            times.sort_by(f64::total_cmp);
            eprintln!("118 sources full engine moving={moving} mean_us={:.1} p95_us={:.1} max_us={:.1} budget_us={:.1} checksum={checksum:.9}",
                times.iter().sum::<f64>() / times.len() as f64, times[times.len() * 95 / 100], times[times.len() - 1],
                convolution::DEFAULT_PARTITION as f64 / 48000.0 * 1e6);
        }
    }

    #[test]
    fn room_comparison_gain_scales_both_ears_without_changing_direction() {
        let render = |gain: f32| {
            let mut engine = calibrated_engine();
            engine.comparison_gain = gain;
            engine.comparison_target = gain;
            engine.paused = false;
            let mut source = Source { kind: SourceKind::Bed, bed_label: Some("FrontLeft".into()),
                gain:1.0,target_gain:1.0,availability:1.0,availability_target:1.0,..Source::default() };
            Engine::set_source_route(&mut source,bed_route("FrontLeft",&engine.vbap),0);
            let pcm:Vec<f32>=(0..4096).map(|i|0.001*(i as f32*0.1).sin()).collect();
            source.samples.write(0,0,&pcm);engine.sources.insert("FrontLeft".into(),source);
            let mut output=vec![0.0;8192];engine.render_into(&mut output,2);output
        };
        let unity=render(1.0);let half=render(0.5);
        assert!(unity.iter().any(|v|v.abs()>1e-5));
        for(a,b)in unity.iter().zip(&half){assert!((a*0.5-b).abs()<1e-7);}
        let command:Command=serde_json::from_str(r#"{"type":"setComparisonGain","gainDb":-6}"#).unwrap();
        assert!(matches!(command,Command::SetComparisonGain{gain_db} if gain_db == -6.0));
    }

    #[test]
    fn stereo_comparison_preserves_original_channels_and_isolates_dry_room_processing() {
        let count = 8192;
        let pcm: Vec<f32> = (0..count).map(|i| 0.005 * (i as f32 * 0.173).sin()).collect();
        let render = |mode, wet, extra_object| {
            let mut engine = calibrated_engine();
            engine.set_layout(vbap::LayoutId::Stereo2_0).unwrap();
            engine.hrtf_wet_weight = wet;
            engine.rebuild_bus_renderer().unwrap();
            engine.stereo_mode = mode;
            engine.paused = false;
            // Exercise the common final-output compensation, not just routing.
            engine.headphone = headphone::HeadphoneCompensation::new(&[0.5, 0.0], &[0.5, 0.0], 1.0).unwrap();
            for (ear, label) in ["FrontLeft", "FrontRight"].iter().enumerate() {
                let mut source = Source { kind: SourceKind::Bed, bed_label: Some((*label).into()),
                    gain: 1.0, target_gain: 1.0, availability: 1.0, availability_target: 1.0, ..Source::default() };
                Engine::set_source_route(&mut source, bed_route(label, &engine.vbap), 0);
                source.samples.write(0, 0, &if ear == 0 { pcm.clone() } else { vec![0.0; count] });
                engine.sources.insert((*label).into(), source);
            }
            if extra_object { engine.sources.insert("obj:1".into(), Source { kind: SourceKind::Object, ..Source::default() }); }
            let mut output = vec![0.0; count * 2];
            engine.render_into(&mut output, 2);
            output
        };
        let original = render(StereoMode::Original, 0.04, false);
        let delay = 2 * convolution::DEFAULT_PARTITION + 240;
        for i in 4096..count {
            assert!(original[i * 2 + 1].abs() < 1e-7, "original leaked into opposite ear");
            let expected = pcm[i - delay] * 0.5 * 10.0_f32.powf(6.0 / 20.0);
            assert!((original[i * 2] - expected).abs() < 2e-6, "original sample mismatch");
        }
        let dry = render(StereoMode::Dry, 0.04, false);
        let dry_reference = render(StereoMode::Room, 0.0, false);
        assert!(dry[8192..].iter().zip(&dry_reference[8192..]).all(|(a,b)| (a-b).abs() < 2e-6));
        let room = render(StereoMode::Room, 0.04, false);
        assert!(dry[8192..].iter().zip(&room[8192..]).any(|(a,b)| (a-b).abs() > 1e-6));
        assert!(dry[8192..].chunks_exact(2).any(|frame| frame[1].abs() > 1e-5));
        let immersive = render(StereoMode::Original, 0.04, true);
        let immersive_reference = render(StereoMode::Room, 0.04, true);
        assert_eq!(immersive, immersive_reference, "stereo preference affected object programme");
    }

    #[test]
    fn egaku_front_to_rear_trajectory_routes_through_the_side() {
        for direct in [false, true] {
            let mut engine = calibrated_engine();
            engine.direct_objects = direct;
            engine.direct_mix = if direct { 1.0 } else { 0.0 };
            let mut source = Source { kind: SourceKind::Object, object_id: Some(14),
                position: [-1.0, 1.0, 0.0], gain: 1.0, target_gain: 1.0, ..Source::default() };
            source.samples.write(0, 0, &[0.001; 4096]);
            // Obj14's captured OAMD uses this path and 1536-sample duration.
            // Retain its 577-sample QMF offset and shift the event to the first frame.
            source.spatial_events.insert(577, SpatialEvent { zone_exclusion: Default::default(), horizontal_only: false, diffuse: 0.0,
                position: [-1.0, -1.0, 0.0], spread: 0.0, ramp: 1536,
            });
            engine.sources.insert("obj:14".into(), source);
            engine.route_source_now("obj:14", 0).unwrap();
            let start = engine.sources["obj:14"].bus_gains;
            let end = bus_renderer::route(&engine.vbap, [-1.0, -1.0, 0.0], None, 0.0);
            let side = bus_renderer::route(&engine.vbap, [-1.0, 0.0, 0.0], None, 0.0);
            engine.render_into(&mut vec![0.0; (577 + 768) * 2], 2);
            let source = &engine.sources["obj:14"];
            assert!(source.position[1].abs() < 1e-5);
            for bus in 0..vbap::MAX_BUS_COUNT {
                assert!((source.bus_gains[bus] - side[bus]).abs() < 0.02,
                    "direct={direct}, bus={bus}: midpoint must route through the side");
            }
            assert!((0..vbap::MAX_BUS_COUNT).any(|bus| ((start[bus] + end[bus]) * 0.5 - side[bus]).abs() > 0.1),
                "fixture must distinguish position motion from endpoint crossfade");
            engine.render_into(&mut vec![0.0; 768 * 2], 2);
            assert_eq!(engine.sources["obj:14"].position, [-1.0, -1.0, 0.0]);
            assert!(engine.sources["obj:14"].motion.is_none());
        }
    }

    #[test]
    fn every_egaku_object_id_has_an_independent_audible_route() {
        let mut engine = calibrated_engine();
        let pcm: Vec<f32> = (0..8192).map(|i| 0.001 * ((i * 37 % 97) as f32 - 48.0)).collect();
        for direct in [false, true] {
            for id in 10..=24 {
                engine.reset_session(0);
                engine.output_active = true;
                engine.paused = false;
                engine.direct_objects = direct;
                engine.direct_mix = if direct { 1.0 } else { 0.0 };
                let mut source = Source { kind: SourceKind::Object, object_id: Some(id),
                    gain: 1.0, target_gain: 1.0, ..Source::default() };
                source.samples.write(0, 0, &pcm);
                let key = format!("obj:{id}");
                engine.sources.insert(key.clone(), source);
                engine.route_source_now(&key, 0).unwrap();
                let mut output = vec![0.0; pcm.len() * 2];
                engine.render_into(&mut output, 2);
                assert!(output.iter().all(|v| v.is_finite()));
                assert!(output.iter().any(|v| v.abs() > 1e-5), "inaudible object {id}, direct={direct}");
            }
        }
    }

    #[test]
    fn scheduled_object_motion_moves_the_audible_image_in_both_modes() {
        for direct in [false, true] {
            let mut engine = calibrated_engine();
            engine.direct_objects = direct;
            engine.direct_mix = if direct { 1.0 } else { 0.0 };
            let pcm: Vec<f32> = (0..48000).map(|i| 0.001 * ((i * 37 % 97) as f32 - 48.0)).collect();
            let mut source = Source { kind: SourceKind::Object, object_id: Some(14),
                position: [-1.0, 1.0, 0.0], gain: 1.0, target_gain: 1.0, ..Source::default() };
            source.samples.write(0, 0, &pcm);
            source.spatial_events.insert(12000, SpatialEvent { zone_exclusion: Default::default(), horizontal_only: false, diffuse: 0.0,
                position: [1.0, 1.0, 0.0], spread: 0.0, ramp: 24000,
            });
            engine.sources.insert("obj:14".into(), source);
            engine.route_source_now("obj:14", 0).unwrap();
            let mut output = vec![0.0; pcm.len() * 2];
            engine.render_into(&mut output, 2);
            let balance = |start: usize| {
                let mut ears = [0.0_f64; 2];
                for frame in output[start * 2..(start + 4000) * 2].chunks_exact(2) {
                    for ear in 0..2 { ears[ear] += (frame[ear] as f64).powi(2); }
                }
                (ears[0] - ears[1]) / (ears[0] + ears[1])
            };
            let left = balance(6000);
            let middle = balance(22000);
            let right = balance(40000);
            assert!(left > 0.05 && right < -0.05, "direct={direct}: left={left}, right={right}");
            assert!(middle > right + 0.05 && middle < left - 0.05,
                "motion must retain its 500 ms ramp: direct={direct}, middle={middle}");
        }
    }

    #[test]
    fn source_level_matches_calibrated_speaker_reference_in_both_modes() {
        let count = 32_768;
        let pcm: Vec<f32> = (0..count)
            .map(|i| 0.001 * (std::f32::consts::TAU * i as f32 / 48.0).sin())
            .collect();
        for direct in [false, true] {
            let mut engine = calibrated_engine();
            engine.direct_objects = direct;
            let position = [0.0, 1.0, 0.0];
            let route = bus_renderer::route(&engine.vbap, position, None, 0.0);
            let mut reference = bus_renderer::BusRenderer::new(
                engine.active_hrtf_set.as_ref().unwrap(), &engine.vbap, engine.hrtf_wet_weight,
            ).unwrap();
            let mut expected = Vec::new();
            for block in pcm.chunks_exact(convolution::DEFAULT_PARTITION) {
                reference.begin_block();
                for (i, &sample) in block.iter().enumerate() { reference.add(sample, &route, i); }
                reference.finish_block().unwrap();
                for i in 0..block.len() { expected.extend(reference.output_at(i)); }
            }
            let mut source = Source { kind: SourceKind::Object, position,
                gain: 1.0, target_gain: 1.0, ..Source::default() };
            source.samples.write(0, 0, &pcm);
            engine.sources.insert("obj:14".into(), source);
            engine.route_source_now("obj:14", 0).unwrap();
            let mut actual = vec![0.0; count * 2];
            engine.render_into(&mut actual, 2);
            // Measure complete steady-state tone periods after fades and FIR tails.
            let rms = |values: &[f32]| {
                let tail = &values[values.len() - 12_288 * 2..];
                (tail.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / tail.len() as f64).sqrt()
            };
            let expected_rms = rms(&expected) * 10.0_f64.powf(6.0 / 20.0);
            let ratio = rms(&actual) / expected_rms;
            assert!((ratio - 1.0).abs() < 0.001, "direct={direct}: level ratio={ratio}");
        }
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
        // B mutes 12 of the 15 objects after they have been routed.
        for id in 10..25_u32 {
            if matches!(id, 14 | 15 | 22) {
                continue;
            }
            engine_b.sources.get_mut(&format!("obj:{id}")).unwrap().muted = true;
        }
        let mut out_a = vec![0.0_f32; block * 8 * 2];
        let mut out_b = vec![0.0_f32; block * 8 * 2];
        let mut out_c = vec![0.0_f32; block * 8 * 2];
        // Render block by block so availability ramps settle identically.
        for index in 0..8 {
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
        assert!(out_b.iter().any(|sample| sample.abs() > 1e-6), "comparison must extend past convolution latency");
        assert!(out_a.iter().zip(&out_b).any(|(a, b)| (a - b).abs() > 1e-6), "muting audible objects must change the full mix");
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
            SpatialEvent { zone_exclusion: Default::default(), horizontal_only: false, diffuse: 0.0,
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
        assert_eq!(source.position, [1.0 / 32.0, 31.0 / 32.0, 0.0]);
        assert_eq!(source.spread, 0.2 / 32.0);
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
        assert_eq!(engine.sources["obj:14"].position, target_position);
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
    fn queued_adm_jump_updates_position_route_and_gain_at_the_authored_sample() {
        let mut engine = calibrated_engine();
        let mut source = Source {
            kind: SourceKind::Object,
            object_id: Some(7),
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        };
        let position = [1.0, 0.0, 0.0];
        source.spatial_events.insert(96, SpatialEvent { zone_exclusion: Default::default(), horizontal_only: false, diffuse: 0.0, position, spread: 0.2, ramp: 0 });
        source.gain_events.insert(96, GainEvent { gain: 0.25, ramp: 0 });
        engine.sources.insert("obj:7".into(), source);
        engine.route_source_now("obj:7", 0).unwrap();
        let target_route = bus_renderer::route(&engine.vbap, position, None, 0.2);

        engine.mix(&mut [0.0; 96 * 2], 2);
        assert_eq!(engine.sources["obj:7"].position, [0.0, 1.0, 0.0]);
        assert_eq!(engine.sources["obj:7"].gain, 1.0);
        engine.mix(&mut [0.0; 2], 2);
        let source = &engine.sources["obj:7"];
        assert_eq!(source.position, position);
        assert_eq!(source.spread, 0.2);
        assert_eq!(source.bus_gains, target_route);
        assert_eq!(source.gain, 0.25);
        assert_eq!(source.ramp_remaining, 0);
        assert_eq!(source.bus_ramp_remaining, 0);
        assert!(source.motion.is_none());
        assert!(source.ramp_step.is_finite());
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
    fn speaker_monitor_filters_bed_and_object_contributions_in_both_modes() {
        let count = 16_384;
        for layout in [vbap::LayoutId::Dolby5_1_2, vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Dolby9_1_6] {
            let mut engine = calibrated_engine();
            engine.set_layout(layout).unwrap();
            for (focus, direct, focus_lfe) in [(false, false, false), (false, true, false), (true, false, false), (true, true, false), (true, false, true), (true, true, true)] {
                let mut outputs = Vec::new();
                for reference in [false, true] {
                    engine.reset_session(0);
                    engine.paused = false;
                    engine.output_active = true;
                    engine.direct_objects = direct;
                    engine.direct_mix = if direct { 1.0 } else { 0.0 };
                    engine.focused_speakers = if focus && !reference { vec!["FrontRight".into()] } else { Vec::new() };
                    if focus && !reference && focus_lfe { engine.focused_speakers.push("LFE".into()); }
                    engine.speaker_mutes = if reference || focus { Vec::new() } else {
                        vbap::speakers(layout).iter().filter(|s| s.name != "FrontRight")
                            .map(|s| s.name.to_string()).chain(std::iter::once("LFE".into())).collect()
                    };
                    for (id, kind, label) in [("bed:0", SourceKind::Bed, "FrontLeft"), ("obj:14", SourceKind::Object, ""), ("bed:1", SourceKind::Bed, "LFE")] {
                        if reference && !focus && kind == SourceKind::Bed { continue; }
                        let mut source = Source { kind, gain: 1.0, target_gain: 1.0,
                            availability: 1.0, availability_target: 1.0, ..Source::default() };
                        let route = if kind == SourceKind::Object {
                            let mut route = RouteGains { buses: [0.0; vbap::MAX_BUS_COUNT], lfe: 0.0 };
                            route.buses[0] = if reference { 0.0 } else { 0.6 };
                            route.buses[1] = 0.8;
                            route
                        } else {
                            let mut route = bed_route(label, &engine.vbap);
                            if reference && focus {
                                for gain in &mut route.buses { *gain *= focus::BACKGROUND_GAIN; }
                                if !focus_lfe { route.lfe *= focus::BACKGROUND_GAIN; }
                            }
                            route
                        };
                        Engine::set_source_route(&mut source, route, 0);
                        let mut pcm: Vec<f32> = (0..count).map(|i| 0.002 * ((i * 37 % 97) as f32 - 48.0) / 48.0).collect();
                        if reference && focus {
                            let mut filter = focus::BackgroundFilter::default();
                            let filtered: Vec<f32> = pcm.iter().map(|sample| filter.process(*sample)).collect();
                            if kind == SourceKind::Object {
                                let mut background = Source { kind, gain: 1.0, target_gain: 1.0,
                                    availability: 1.0, availability_target: 1.0, ..Source::default() };
                                let mut route = RouteGains { buses: [0.0; vbap::MAX_BUS_COUNT], lfe: 0.0 };
                                route.buses[0] = 0.6 * focus::BACKGROUND_GAIN;
                                Engine::set_source_route(&mut background, route, 0);
                                background.samples.write(0, 0, &filtered);
                                engine.sources.insert("reference-background".into(), background);
                            } else if label != "LFE" { pcm = filtered; }
                        }
                        source.samples.write(0, 0, &pcm);
                        engine.sources.insert(id.into(), source);
                    }
                    let mut output = vec![0.0; count * 2];
                    engine.render_into(&mut output, 2);
                    outputs.push(output);
                }
                let error = outputs[0][16384..].iter().zip(&outputs[1][16384..])
                    .map(|(a,b)| (a-b).abs()).fold(0.0_f32, f32::max);
                assert!(outputs[1][16384..].iter().any(|v| v.abs() > 1e-5));
                assert!(error < 2e-6, "layout={layout:?} direct={direct} focus={focus} error={error}");
            }
        }
    }

    #[test]
    fn dense_object_assets_preserve_physical_speaker_directions() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/public");
        for (dense, standard) in [("hrtf-dense", "hrtf"), ("hrtf-dense-raw", "hrtf-raw")] {
            let dense = hrtf::NativeHrtfSet::load_calibrated(&root.join(dense).join("hrtf-set.json")).unwrap();
            let standard = hrtf::NativeHrtfSet::load_calibrated(&root.join(standard).join("hrtf-set.json")).unwrap();
            for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Dolby9_1_6] {
                for speaker in vbap::speakers(layout) {
                    let expected = standard.mixed_speaker(speaker.name, layout.as_str(), speaker.azimuth as f64, speaker.elevation as f64, 0.04).unwrap();
                    let actual = dense.mixed_speaker(speaker.name, layout.as_str(), speaker.azimuth as f64, speaker.elevation as f64, 0.04).unwrap();
                    for (actual, expected) in [(&actual.0, &expected.0), (&actual.1, &expected.1)] {
                        for i in 0..actual.len().max(expected.len()) {
                            assert_eq!(actual.get(i).copied().unwrap_or(0.0), expected.get(i).copied().unwrap_or(0.0), "{} sample={i}", speaker.name);
                        }
                    }
                }
            }
            assert_eq!(dense.nearest(-30.0, 45.0).unwrap().azimuth, -30.0, "dense object directions remain available");
        }
    }

    #[test]
    fn overhead_bed_output_matches_measured_direction_impulse() {
        let count = 12288;
        let impulse_at = 2048;
        let latency = 2 * convolution::DEFAULT_PARTITION + 240;
        for (layout, label, azimuth) in [
            (vbap::LayoutId::Dolby7_1_4, "TopFrontRight", -45.0),
            (vbap::LayoutId::Dolby7_1_4, "TopRearRight", -135.0),
            (vbap::LayoutId::Dolby9_1_6, "TopMiddleRight", -90.0),
        ] {
            for wet in [0.0, 0.04] {
                let mut engine = calibrated_engine();
                engine.hrtf_wet_weight = wet;
                engine.set_layout(layout).unwrap();
                engine.rebuild_bus_renderer().unwrap();
                engine.paused = false;
                let measured = engine.active_hrtf_set.as_ref().unwrap().nearest(azimuth, 45.0).unwrap();
                assert_eq!((measured.azimuth, measured.elevation), (azimuth, 45.0));
                let dry_len = measured.dry.len() / 2;
                let wet_len = measured.wet.len() / 2;
                let mut source = Source { kind: SourceKind::Bed, bed_label: Some(label.into()),
                    gain: 1.0, target_gain: 1.0, availability: 1.0, availability_target: 1.0,
                    ..Source::default() };
                let mut pcm = vec![0.0; count];
                pcm[impulse_at] = 0.01;
                source.samples.write(0, 0, &pcm);
                Engine::set_source_route(&mut source, bed_route(label, &engine.vbap), 0);
                engine.sources.insert("bed:top".into(), source);
                let mut output = vec![0.0; count * 2];
                engine.render_into(&mut output, 2);
                // Independent time-domain oracle, without the bus mixer or FFT convolver.
                let mut error = 0.0_f32;
                let mut peak = 0.0_f32;
                for frame in 0..count {
                    for ear in 0..2 {
                        let expected = frame.checked_sub(impulse_at + latency).map_or(0.0, |i| {
                            let dry = if i < dry_len { measured.dry[ear * dry_len + i] } else { 0.0 };
                            let room = if i < wet_len { measured.wet[ear * wet_len + i] } else { 0.0 };
                            (dry + wet * (room - dry)) * 0.01 * 10.0_f32.powf(6.0 / 20.0)
                        });
                        peak = peak.max(expected.abs());
                        error = error.max((output[frame * 2 + ear] - expected).abs());
                    }
                }
                assert!(peak > 1e-5);
                assert!(error < 1e-6, "{label} wet={wet} error={error} peak={peak}");
            }
        }
    }

    #[test]
    fn overhead_bed_monitor_preserves_selected_speaker_filter() {
        let count = 8192;
        for (layout, label) in [
            (vbap::LayoutId::Dolby7_1_4, "TopFrontRight"),
            (vbap::LayoutId::Dolby7_1_4, "TopRearRight"),
            (vbap::LayoutId::Dolby9_1_6, "TopMiddleRight"),
        ] {
            let mut engine = calibrated_engine();
            engine.set_layout(layout).unwrap();
            let target = engine.vbap.speaker_index(label).unwrap();
            let route = bed_route(label, &engine.vbap);
            for (index, gain) in route.buses.iter().enumerate() {
                assert_eq!(*gain, if index == target { 1.0 } else { 0.0 });
            }
            let mut reference = Vec::new();
            for mode in 0..3 {
                engine.reset_session(0);
                engine.paused = false;
                engine.output_active = true;
                engine.focused_speakers = if mode == 2 { vec![label.into()] } else { Vec::new() };
                engine.speaker_mutes = if mode == 1 {
                    vbap::speakers(layout).iter().filter(|s| s.name != label)
                        .map(|s| s.name.to_string()).collect()
                } else { Vec::new() };
                let mut source = Source { kind: SourceKind::Bed, bed_label: Some(label.into()),
                    gain: 1.0, target_gain: 1.0, availability: 1.0, availability_target: 1.0,
                    ..Source::default() };
                let pcm: Vec<f32> = (0..count).map(|i|
                    if i < 4096 { 0.002 * ((i * 37 % 97) as f32 - 48.0) / 48.0 } else { 0.0 }
                ).collect();
                source.samples.write(0, 0, &pcm);
                Engine::set_source_route(&mut source, bed_route(label, &engine.vbap), 0);
                engine.sources.insert("bed:top".into(), source);
                let mut output = vec![0.0; count * 2];
                engine.render_into(&mut output, 2);
                assert!(output.iter().all(|v| v.is_finite()));
                assert!(output.iter().any(|v| v.abs() > 1e-5));
                if mode == 0 { reference = output; } else {
                    let error = output.iter().zip(&reference).map(|(a,b)| (a-b).abs()).fold(0.0_f32, f32::max);
                    assert!(error < 2e-6, "{label} mode={mode} error={error}");
                }
            }
        }
    }

    #[test]
    fn speaker_focus_excludes_mutes_and_ignores_missing_layout_speakers() {
        let mut engine = Engine::new(48000, 2);
        engine.set_speaker_monitor(vec!["FrontLeft".into()], vec!["FrontLeft".into(), "TopRearRight".into()]);
        assert!(engine.speaker_mutes.is_empty());
        assert_eq!(engine.speaker_target("FrontLeft"), 1.0);
        assert_eq!(engine.speaker_target("TopRearRight"), 1.0);
        assert!((engine.speaker_target("FrontRight") - focus::BACKGROUND_GAIN).abs() < 1e-6);
        engine.set_speaker_monitor(vec!["FrontLeft".into()], Vec::new());
        assert!(engine.focused_speakers.is_empty());
        assert_eq!(engine.speaker_target("FrontLeft"), 0.0);
        engine.set_speaker_monitor(Vec::new(), vec!["LFE".into()]);
        assert_eq!(engine.speaker_target("LFE"), 1.0);
        engine.layout = vbap::LayoutId::Stereo2_0;
        assert_eq!(engine.speaker_target("FrontRight"), 1.0);
        engine.focused_speakers.clear();
        assert_eq!(engine.speaker_target("FrontRight"), 1.0);
    }

    #[test]
    fn authored_bed_motion_preserves_channel_mix_in_both_object_modes() {
        let count = 16_384;
        let partition = convolution::DEFAULT_PARTITION;
        let delay = 2 * partition + 240; // Speaker block, headphone block, 5 ms peak guard.
        for layout in [
            vbap::LayoutId::Stereo2_0, vbap::LayoutId::Stereo2_1,
            vbap::LayoutId::Dolby5_1, vbap::LayoutId::Dolby5_1_2,
            vbap::LayoutId::Dolby5_1_4, vbap::LayoutId::Dolby7_1_2,
            vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Dolby9_1_2,
            vbap::LayoutId::Dolby9_1_4, vbap::LayoutId::Dolby9_1_6,
        ] {
            let speakers = vbap::speakers(layout);
            // A closed tour exercises every bed channel as both the departing
            // and arriving signal, including wides and each overhead channel.
            for index in 0..speakers.len() {
                let start_label = speakers[index].name;
                let target_label = speakers[(index + 1) % speakers.len()].name;
                let mut engine = calibrated_engine();
                engine.set_layout(layout).unwrap();
                let channels: [Vec<f32>; 2] = std::array::from_fn(|channel| {
                    (0..count).map(|i| {
                        if i >= 8192 { return 0.0; }
                        let phase = ((i as f32 - 2048.0) / 4096.0).clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
                        let gain = if channel == 0 { phase.cos() } else { phase.sin() };
                        0.002 * ((i * 37 % 97) as f32 - 48.0) / 48.0 * gain
                    }).collect()
                });
                let target_speakers = vec![target_label];
                let mut expected = vec![0.0_f32; count * 2];
                // Reference mixes the authored channel PCM through individual
                // speaker filters; it does not use bed_route or the engine mixer.
                for (channel, speakers) in [vec![start_label], target_speakers].iter().enumerate() {
                    let weight = 1.0 / (speakers.len() as f32).sqrt();
                    for name in speakers {
                        let bus = engine.vbap.speaker_index(name).unwrap();
                        let (az, el) = engine.vbap.speaker_direction(bus);
                        let (_, _, left, right) = engine.active_hrtf_set.as_ref().unwrap()
                            .mixed_nearest(az as f64, el as f64, 0.04).unwrap();
                        let mut filter = convolution::StereoPartitionedConvolver::new(&left, &right, partition).unwrap();
                        for (block, input) in channels[channel].chunks_exact(partition).enumerate() {
                            let mut left = vec![0.0; partition];
                            let mut right = vec![0.0; partition];
                            filter.process_block(input, &mut left, &mut right).unwrap();
                            for i in 0..partition {
                                let at = block * partition + i + delay;
                                if at < count {
                                    expected[at * 2] += left[i] * weight * 10.0_f32.powf(6.0 / 20.0);
                                    expected[at * 2 + 1] += right[i] * weight * 10.0_f32.powf(6.0 / 20.0);
                                }
                            }
                        }
                    }
                }
                for direct in [false, true] {
                    engine.reset_session(0);
                    engine.paused = false;
                    engine.output_active = true;
                    engine.direct_objects = direct;
                    for (channel, label) in [start_label, target_label].iter().enumerate() {
                        let mut source = Source { kind: SourceKind::Bed, bed_label: Some((*label).into()),
                            gain: 1.0, target_gain: 1.0, availability: 1.0, availability_target: 1.0,
                            ..Source::default() };
                        source.samples.write(0, 0, &channels[channel]);
                        Engine::set_source_route(&mut source, bed_route(label, &engine.vbap), 0);
                        engine.sources.insert(format!("bed:{channel}"), source);
                    }
                    let mut actual = vec![0.0; count * 2];
                    engine.render_into(&mut actual, 2);
                    let error = actual.iter().zip(&expected).map(|(a,b)| (a-b).abs()).fold(0.0_f32, f32::max);
                    assert!(expected.iter().any(|v| v.abs() > 1e-5));
                    assert!(actual.iter().all(|v| v.is_finite()));
                    assert!(error < 2e-6, "layout={layout:?}, target={target_label}, direct={direct}, error={error}");
                    assert!(engine.sources.values().all(|source| source.direct.is_none()));
                }
            }
        }
    }

    #[test]
    fn middle_height_bed_stays_in_the_height_layer_when_layout_has_four_tops() {
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        for (label, names) in [("Ltm", ["TopFrontLeft", "TopRearLeft"]), ("Rtm", ["TopFrontRight", "TopRearRight"])] {
            let route = bed_route(label, &solver);
            for bus in 0..solver.bus_count() {
                let expected = if names.iter().any(|name| solver.speaker_index(name) == Some(bus)) {
                    std::f32::consts::FRAC_1_SQRT_2
                } else { 0.0 };
                assert!((route.buses[bus] - expected).abs() < 1e-6,
                    "{label} bus {bus}: expected {expected}, got {}", route.buses[bus]);
            }
            assert_eq!(route.lfe, 0.0);
        }
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
