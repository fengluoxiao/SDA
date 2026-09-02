//! SDA decoder core — WASM bindings over the harletty-bridge decoder crates.
//!
//! Feeds raw TrueHD / E-AC-3(JOC) / DTS bitstream bytes in, yields decoded
//! frames: planar f32 PCM (bed channels first, then one channel per dynamic
//! object) plus the object spatial events that were active for the frame.
//!
//! The event model mirrors Omniphony's `bridge_api::REvent`: positions are
//! ADM cartesian `[x, y, z]` (x+ = right, y+ = front, z+ = up), `sample_pos`
//! is absolute on the codec sample clock, `ramp_duration` is in samples.

use std::collections::VecDeque;

use js_sys::Float32Array;
use serde::Serialize;
use wasm_bindgen::prelude::*;

pub mod alac_pipeline;
pub mod dts_pipeline;
pub mod eac3_pipeline;
pub mod truehd_pipeline;
pub mod vbap;

/// One dynamic-object spatial event (port of `bridge_api::REvent`).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ObjectEvent {
    pub id: u32,
    pub sample_pos: u64,
    /// False = gain/ramp-only update, `pos` holds no valid coordinates.
    pub has_pos: bool,
    /// ADM cartesian [x, y, z]: x+ = right, y+ = front, z+ = up.
    pub pos: [f64; 3],
    pub gain_db: i8,
    /// Object extent (width, depth, height), each normalised to [0, 1].
    /// [0, 0, 0] = point source.
    pub size: [f64; 3],
    /// Codec metadata anchor: room, screen, or speaker.
    pub anchor: String,
    /// Finite codec object distance in metres; None means not transmitted.
    pub distance_m: Option<f64>,
    /// Codec explicitly marked this object as infinitely distant.
    pub distance_infinite: bool,
    pub screen_factor: Option<f64>,
    pub depth_factor: Option<f64>,
    pub ramp_duration: u32,
}

/// Sparse object-id ↔ PCM-channel declaration (emitted only when it changes).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ObjectChannelDecl {
    pub id: u32,
    pub channel: u32,
}

/// Program-level loudness metadata. Dynamic-range control is intentionally not
/// included: dialnorm is a static decoder gain and DRC remains disabled.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProgramLoudnessMetadata {
    pub source: &'static str,
    pub dialogue_level_db: i8,
    pub target_db: i8,
    pub gain_db: i8,
}

impl ProgramLoudnessMetadata {
    pub fn dolby(source: &'static str, dialogue_level_db: i8) -> Self {
        Self {
            source,
            dialogue_level_db,
            target_db: -31,
            gain_db: -31 - dialogue_level_db,
        }
    }
}

/// Decoded frame handed to JavaScript.
pub struct FrameData {
    pub codec: &'static str,
    pub sample_rate: u32,
    pub sample_pos: u64,
    /// Planar PCM: bed channels first, then dynamic-object channels.
    pub channels: Vec<Vec<f32>>,
    pub labels: Vec<String>,
    /// Original fixed bed labels before object reconstruction/downstream routing.
    pub raw_bed_labels: Vec<String>,
    pub events: Vec<ObjectEvent>,
    pub object_channels: Vec<ObjectChannelDecl>,
    pub program_loudness: Option<ProgramLoudnessMetadata>,
    pub ramp_duration: u32,
}

pub trait Pipeline {
    fn codec_name(&self) -> &'static str;
    fn push(&mut self, data: &[u8], out: &mut VecDeque<FrameData>, errors: &mut Vec<String>);
    fn reset(&mut self);
}

/// A decoded frame. PCM channels are fetched one at a time as typed arrays;
/// metadata comes out as JSON (events are small — a handful per frame).
#[wasm_bindgen]
pub struct DecodedFrame {
    data: FrameData,
}

#[wasm_bindgen]
impl DecodedFrame {
    #[wasm_bindgen(getter)]
    pub fn codec(&self) -> String {
        self.data.codec.to_string()
    }

    #[wasm_bindgen(getter, js_name = sampleRate)]
    pub fn sample_rate(&self) -> u32 {
        self.data.sample_rate
    }

    #[wasm_bindgen(getter, js_name = samplePos)]
    pub fn sample_pos(&self) -> f64 {
        self.data.sample_pos as f64
    }

    #[wasm_bindgen(getter, js_name = channelCount)]
    pub fn channel_count(&self) -> usize {
        self.data.channels.len()
    }

    #[wasm_bindgen(getter, js_name = samplesPerChannel)]
    pub fn samples_per_channel(&self) -> usize {
        self.data.channels.first().map_or(0, Vec::len)
    }

    #[wasm_bindgen(getter)]
    pub fn labels(&self) -> Vec<String> {
        self.data.labels.clone()
    }

    #[wasm_bindgen(getter, js_name = rawBedLabels)]
    pub fn raw_bed_labels(&self) -> Vec<String> {
        self.data.raw_bed_labels.clone()
    }

    pub fn channel(&self, index: usize) -> Option<Float32Array> {
        self.data
            .channels
            .get(index)
            .map(|c| Float32Array::from(c.as_slice()))
    }

    #[wasm_bindgen(getter, js_name = eventsJson)]
    pub fn events_json(&self) -> String {
        serde_json::to_string(&self.data.events).unwrap_or_default()
    }

    /// Sparse: empty when the object↔channel mapping didn't change.
    #[wasm_bindgen(getter, js_name = objectChannelsJson)]
    pub fn object_channels_json(&self) -> String {
        serde_json::to_string(&self.data.object_channels).unwrap_or_default()
    }

    #[wasm_bindgen(getter, js_name = programLoudnessJson)]
    pub fn program_loudness_json(&self) -> String {
        serde_json::to_string(&self.data.program_loudness).unwrap_or_default()
    }

    #[wasm_bindgen(getter, js_name = rampDuration)]
    pub fn ramp_duration(&self) -> u32 {
        self.data.ramp_duration
    }
}

/// Stateful streaming decoder. Construct with a codec name
/// (`"auto" | "truehd" | "eac3" | "dts"`), `push()` raw bytes, then drain
/// with `next_frame()` until it returns `undefined`. ALAC is constructed with
/// `with_config()` because its MP4 codec cookie is required before decoding.
#[wasm_bindgen]
pub struct SdaDecoder {
    pipeline: Box<dyn Pipeline>,
    sniff: Option<Vec<u8>>,
    queue: VecDeque<FrameData>,
    errors: Vec<String>,
}

#[wasm_bindgen]
impl SdaDecoder {
    #[wasm_bindgen(constructor)]
    pub fn new(codec: &str) -> Result<SdaDecoder, JsValue> {
        match codec {
            "auto" => Ok(SdaDecoder {
                pipeline: Box::new(NoopPipeline),
                sniff: Some(Vec::with_capacity(64 * 1024)),
                queue: VecDeque::new(),
                errors: Vec::new(),
            }),
            _ => Ok(SdaDecoder {
                pipeline: build_pipeline(codec)?,
                sniff: None,
                queue: VecDeque::new(),
                errors: Vec::new(),
            }),
        }
    }

    /// Construct a decoder whose container codec requires initialization bytes.
    /// Currently this is used by ALAC MP4 tracks and expects the full `alac` atom.
    #[wasm_bindgen(js_name = withConfig)]
    pub fn with_config(codec: &str, config: &[u8]) -> Result<SdaDecoder, JsValue> {
        let pipeline: Box<dyn Pipeline> = match codec {
            "alac" => Box::new(
                alac_pipeline::AlacPipeline::from_cookie(config)
                    .map_err(|error| JsValue::from_str(&format!("invalid ALAC configuration: {error}")))?,
            ),
            other => return Err(JsValue::from_str(&format!("codec does not accept container configuration: {other}"))),
        };
        Ok(SdaDecoder {
            pipeline,
            sniff: None,
            queue: VecDeque::new(),
            errors: Vec::new(),
        })
    }

    /// Feed raw bitstream bytes (any chunking — the extractors re-frame).
    pub fn push(&mut self, data: &[u8]) -> Result<(), JsValue> {
        if let Some(sniff) = &mut self.sniff {
            sniff.extend_from_slice(data);
            match detect_codec(sniff) {
                Some(codec) => {
                    let buffered = std::mem::take(sniff);
                    self.pipeline = build_pipeline(codec)?;
                    self.sniff = None;
                    self.pipeline
                        .push(&buffered, &mut self.queue, &mut self.errors);
                }
                None if sniff.len() >= 64 * 1024 => {
                    return Err(JsValue::from_str(
                        "could not detect codec from first 64 KiB (no TrueHD/E-AC-3/DTS syncword)",
                    ));
                }
                None => return Ok(()),
            }
            return Ok(());
        }
        self.pipeline.push(data, &mut self.queue, &mut self.errors);
        Ok(())
    }

    /// Pop the next decoded frame, or `undefined` when more input is needed.
    #[wasm_bindgen(js_name = nextFrame)]
    pub fn next_frame(&mut self) -> Option<DecodedFrame> {
        self.queue.pop_front().map(|data| DecodedFrame { data })
    }

    /// Codec actually in use (meaningful after auto-detection kicked in).
    #[wasm_bindgen(getter)]
    pub fn codec(&self) -> String {
        self.pipeline.codec_name().to_string()
    }

    /// Decode errors are non-fatal (the pipelines resync); drain them here.
    #[wasm_bindgen(js_name = drainErrors)]
    pub fn drain_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    pub fn reset(&mut self) {
        self.pipeline.reset();
        self.queue.clear();
    }
}

fn build_pipeline(codec: &str) -> Result<Box<dyn Pipeline>, JsValue> {
    match codec {
        "truehd" | "thd" | "mlp" => Ok(Box::new(truehd_pipeline::TruehdPipeline::new())),
        "eac3" | "ec3" | "ac3" => Ok(Box::new(eac3_pipeline::Eac3Pipeline::new())),
        "dts" | "dca" => Ok(Box::new(dts_pipeline::DtsPipeline::new())),
        other => Err(JsValue::from_str(&format!("unknown codec: {other}"))),
    }
}

struct NoopPipeline;
impl Pipeline for NoopPipeline {
    fn codec_name(&self) -> &'static str {
        "auto"
    }
    fn push(&mut self, _data: &[u8], _out: &mut VecDeque<FrameData>, _errors: &mut Vec<String>) {}
    fn reset(&mut self) {}
}

/// Syncword sniffing over the first bytes of the stream.
fn detect_codec(data: &[u8]) -> Option<&'static str> {
    let scan = &data[..data.len().min(64 * 1024)];
    let mut first_eac3 = None;
    let mut first_dts = None;
    for w in scan.windows(4) {
        // TrueHD major sync — strongest signal, wins immediately.
        if w[0] == 0xF8 && w[1] == 0x72 && w[2] == 0x6F {
            return Some("truehd");
        }
        if first_eac3.is_none() && w[0] == 0x0B && w[1] == 0x77 {
            first_eac3 = Some("eac3");
        }
        if first_dts.is_none()
            && ((w[0] == 0x7F && w[1] == 0xFE && w[2] == 0x80 && w[3] == 0x01) // 16-bit BE
                || (w[0] == 0xFE && w[1] == 0x7F && w[2] == 0x01 && w[3] == 0x80) // 16-bit LE
                || (w[0] == 0x1F && w[1] == 0xFF && w[2] == 0xE8 && w[3] == 0x00) // 14-bit BE
                || (w[0] == 0xFF && w[1] == 0x1F && w[2] == 0x00 && w[3] == 0xE8))
        {
            first_dts = Some("dts");
        }
    }
    first_eac3.or(first_dts)
}
