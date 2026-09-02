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
    sync::{Arc, Mutex},
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
const NATIVE_RENDERER_MAX_JSON_BYTES: usize = 16 * 1024;

mod protocol;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Command {
    Hello { protocol: u32 },
    Configure { sample_rate: u32, channels: u16 },
    AddSource { id: String },
    RemoveSource { id: String, at: u64 },
    /// Reference transport. Production Electron integration upgrades samples to
    /// framed binary blocks while preserving `start` and the source lifecycle.
    Feed { id: String, start: u64, samples: Vec<f32> },
    SetGain { id: String, gain: f32, ramp: u32, at: Option<u64> },
    Pause { paused: bool },
    Reset { origin: u64 },
    Health,
    Shutdown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Event<'a> {
    Ready { protocol: u32, sample_rate: u32, output_channels: u16 },
    Ack { command: &'a str, accepted: bool, detail: Option<&'a str> },
    Health(Health),
    Error { detail: String },
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Health {
    sample_pos: u64,
    active_sources: usize,
    underrun_samples: u64,
    callback_count: u64,
    callback_max_micros: u64,
    output_sample_rate: u32,
    output_channels: u16,
    paused: bool,
    reference_mix: bool,
}

#[derive(Clone, Copy)]
struct GainEvent {
    gain: f32,
    ramp: u32,
}

#[derive(Default)]
struct Source {
    samples: BTreeMap<u64, f32>,
    gain_events: BTreeMap<u64, GainEvent>,
    gain: f32,
    target_gain: f32,
    ramp_remaining: u32,
    ramp_step: f32,
    remove_at: Option<u64>,
}

struct Engine {
    sample_pos: u64,
    paused: bool,
    sources: HashMap<String, Source>,
    underrun_samples: u64,
    callback_count: u64,
    callback_max: Duration,
    output_sample_rate: u32,
    output_channels: u16,
}

impl Engine {
    fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_pos: 0,
            paused: false,
            sources: HashMap::new(),
            underrun_samples: 0,
            callback_count: 0,
            callback_max: Duration::ZERO,
            output_sample_rate: sample_rate,
            output_channels: channels,
        }
    }

    fn health(&self) -> Health {
        Health {
            sample_pos: self.sample_pos,
            active_sources: self.sources.len(),
            underrun_samples: self.underrun_samples,
            callback_count: self.callback_count,
            callback_max_micros: self.callback_max.as_micros() as u64,
            output_sample_rate: self.output_sample_rate,
            output_channels: self.output_channels,
            paused: self.paused,
            reference_mix: true,
        }
    }

    fn mix(&mut self, output: &mut [f32], channels: usize) {
        let started = Instant::now();
        output.fill(0.0);
        if self.paused {
            self.callback_count += 1;
            return;
        }
        for frame in output.chunks_exact_mut(channels) {
            let at = self.sample_pos;
            let mut mono = 0.0_f32;
            self.sources.retain(|_, source| {
                if let Some(remove_at) = source.remove_at {
                    if at >= remove_at { return false; }
                }
                if let Some(event) = source.gain_events.remove(&at) {
                    source.target_gain = event.gain;
                    source.ramp_remaining = event.ramp.max(1);
                    source.ramp_step = (event.gain - source.gain) / source.ramp_remaining as f32;
                }
                if source.ramp_remaining > 0 {
                    source.gain += source.ramp_step;
                    source.ramp_remaining -= 1;
                    if source.ramp_remaining == 0 { source.gain = source.target_gain; }
                }
                if let Some(sample) = source.samples.remove(&at) {
                    mono += sample * source.gain;
                } else if source.gain != 0.0 {
                    self.underrun_samples += 1;
                }
                true
            });
            // Reference stereo mix. Native object HRTF writes distinct L/R here.
            if channels >= 2 {
                frame[0] = mono;
                frame[1] = mono;
            } else if channels == 1 {
                frame[0] = mono;
            }
            self.sample_pos += 1;
        }
        self.callback_count += 1;
        self.callback_max = self.callback_max.max(started.elapsed());
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

fn build_stream(engine: Arc<Mutex<Engine>>, device: &cpal::Device, config: &StreamConfig, format: SampleFormat) -> Result<Stream, String> {
    let channels = config.channels as usize;
    let error = |err| eprintln!("[SDA native renderer] WASAPI stream error: {err}");
    match format {
        SampleFormat::F32 => device.build_output_stream(config, move |data: &mut [f32], _| {
            if let Ok(mut state) = engine.lock() { state.mix(data, channels); }
        }, error, None),
        SampleFormat::I16 => device.build_output_stream(config, move |data: &mut [i16], _| {
            let mut scratch = vec![0.0_f32; data.len()];
            if let Ok(mut state) = engine.lock() { state.mix(&mut scratch, channels); }
            for (target, sample) in data.iter_mut().zip(scratch) { *target = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16; }
        }, error, None),
        SampleFormat::U16 => device.build_output_stream(config, move |data: &mut [u16], _| {
            let mut scratch = vec![0.0_f32; data.len()];
            if let Ok(mut state) = engine.lock() { state.mix(&mut scratch, channels); }
            for (target, sample) in data.iter_mut().zip(scratch) { *target = ((sample.clamp(-1.0, 1.0) + 1.0) * 0.5 * u16::MAX as f32) as u16; }
        }, error, None),
        other => return Err(format!("unsupported WASAPI sample format: {other:?}")),
    }.map_err(|error| error.to_string())
}

fn main() {
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(device) => device,
        None => { write_event(&Event::Error { detail: "No default WASAPI output device".into() }); return; }
    };
    let supported = match device.default_output_config() {
        Ok(config) => config,
        Err(error) => { write_event(&Event::Error { detail: format!("Cannot query WASAPI output: {error}") }); return; }
    };
    let mut config: StreamConfig = supported.config();
    // Renderer PCM is 48 kHz. Device negotiation is explicit; CPAL performs no
    // hidden resampling, so a later resampler must be added if the device refuses.
    config.channels = config.channels.max(2);
    let engine = Arc::new(Mutex::new(Engine::new(config.sample_rate.0, config.channels)));
    let stream = match build_stream(engine.clone(), &device, &config, supported.sample_format()) {
        Ok(stream) => stream,
        Err(error) => { write_event(&Event::Error { detail: format!("Cannot start WASAPI output: {error}") }); return; }
    };
    if let Err(error) = stream.play() {
        write_event(&Event::Error { detail: format!("Cannot play WASAPI stream: {error}") });
        return;
    }
    write_event(&Event::Ready { protocol: PROTOCOL, sample_rate: config.sample_rate.0, output_channels: config.channels });

    let stdin = io::stdin();
    if let Err(error) = protocol::read_frames(&mut stdin.lock(), &engine) {
        write_event(&Event::Error { detail: format!("Native renderer protocol error: {error}") });
    }
    drop(stream);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_mix_on_the_absolute_codec_clock() {
        let mut engine = Engine::new(48_000, 2);
        engine.sources.insert("obj:1".into(), Source {
            samples: BTreeMap::from([(10, 0.25), (11, 0.5)]),
            gain: 1.0,
            target_gain: 1.0,
            ..Source::default()
        });
        engine.sample_pos = 10;
        let mut out = [0.0; 4];
        engine.mix(&mut out, 2);
        assert_eq!(out, [0.25, 0.25, 0.5, 0.5]);
        assert_eq!(engine.sample_pos, 12);
    }

    #[test]
    fn future_gain_event_does_not_retime_source_removal() {
        let mut engine = Engine::new(48_000, 2);
        let mut source = Source { gain: 1.0, target_gain: 1.0, ..Source::default() };
        source.samples.insert(3, 1.0);
        source.samples.insert(4, 1.0);
        source.gain_events.insert(4, GainEvent { gain: 0.0, ramp: 1 });
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
        engine.sources.insert("obj:9".into(), Source {
            samples: BTreeMap::from([(8, 1.0), (9, 1.0)]),
            gain: 1.0,
            target_gain: 1.0,
            remove_at: Some(9),
            ..Source::default()
        });
        engine.sample_pos = 8;
        let mut out = [0.0; 4];
        engine.mix(&mut out, 2);
        assert_eq!(out, [1.0, 1.0, 0.0, 0.0]);
        assert!(!engine.sources.contains_key("obj:9"));
    }
}
