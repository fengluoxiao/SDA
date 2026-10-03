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

#[cfg(feature = "wasm")]
use js_sys::Float32Array;
use serde::Serialize;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

pub mod alac_pipeline;
pub mod ac4_pipeline;
pub mod dts_pipeline;
pub mod eac3_pipeline;
pub mod truehd_pipeline;
pub mod vbap;
pub mod diagnostics;
#[cfg_attr(feature = "wasm", wasm_bindgen(js_name = setDiagnostics))]
pub fn set_diagnostics(enabled:bool){diagnostics::enable(enabled);}
#[cfg_attr(feature = "wasm", wasm_bindgen(js_name = drainDiagnostics))]
pub fn drain_diagnostics()->String{diagnostics::drain()}

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
    pub gain_db: f64,
    /// Object extent (width, depth, height), each normalised to [0, 1].
    /// [0, 0, 0] = point source.
    pub size: [f64; 3],
    /// Codec metadata anchor: room, screen, or speaker.
    pub anchor: String,
    /// MPEG-H diffuse energy fraction, independent of object extent.
    pub diffuse: f64,
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
    fn flush(&mut self, _out: &mut VecDeque<FrameData>, _errors: &mut Vec<String>) {}
}

/// A decoded frame. PCM channels are fetched one at a time as typed arrays;
/// metadata comes out as JSON (events are small — a handful per frame).
#[cfg_attr(feature = "wasm", wasm_bindgen)]
pub struct DecodedFrame {
    data: FrameData,
}

#[cfg_attr(feature = "wasm", wasm_bindgen)]
impl DecodedFrame {
    #[cfg_attr(feature = "wasm", wasm_bindgen(getter))]
    pub fn codec(&self) -> String {
        self.data.codec.to_string()
    }

    #[cfg_attr(feature = "wasm", wasm_bindgen(getter, js_name = sampleRate))]
    pub fn sample_rate(&self) -> u32 {
        self.data.sample_rate
    }

    #[cfg_attr(feature = "wasm", wasm_bindgen(getter, js_name = samplePos))]
    pub fn sample_pos(&self) -> f64 {
        self.data.sample_pos as f64
    }

    #[cfg_attr(feature = "wasm", wasm_bindgen(getter, js_name = channelCount))]
    pub fn channel_count(&self) -> usize {
        self.data.channels.len()
    }

    #[cfg_attr(feature = "wasm", wasm_bindgen(getter, js_name = samplesPerChannel))]
    pub fn samples_per_channel(&self) -> usize {
        self.data.channels.first().map_or(0, Vec::len)
    }

    #[cfg_attr(feature = "wasm", wasm_bindgen(getter))]
    pub fn labels(&self) -> Vec<String> {
        self.data.labels.clone()
    }

    #[cfg_attr(feature = "wasm", wasm_bindgen(getter, js_name = rawBedLabels))]
    pub fn raw_bed_labels(&self) -> Vec<String> {
        self.data.raw_bed_labels.clone()
    }

    #[cfg(feature = "wasm")]
    pub fn channel(&self, index: usize) -> Option<Float32Array> {
        self.data
            .channels
            .get(index)
            .map(|c| Float32Array::from(c.as_slice()))
    }

    #[cfg_attr(feature = "wasm", wasm_bindgen(getter, js_name = eventsJson))]
    pub fn events_json(&self) -> String {
        serde_json::to_string(&self.data.events).unwrap_or_default()
    }

    /// Sparse: empty when the object↔channel mapping didn't change.
    #[cfg_attr(feature = "wasm", wasm_bindgen(getter, js_name = objectChannelsJson))]
    pub fn object_channels_json(&self) -> String {
        serde_json::to_string(&self.data.object_channels).unwrap_or_default()
    }

    #[cfg_attr(feature = "wasm", wasm_bindgen(getter, js_name = programLoudnessJson))]
    pub fn program_loudness_json(&self) -> String {
        serde_json::to_string(&self.data.program_loudness).unwrap_or_default()
    }

    #[cfg_attr(feature = "wasm", wasm_bindgen(getter, js_name = rampDuration))]
    pub fn ramp_duration(&self) -> u32 {
        self.data.ramp_duration
    }
}

impl DecodedFrame {
    /// Native access to the decoded planar PCM (wasm consumers read `channel()`).
    pub fn channel_samples(&self, index: usize) -> Option<&[f32]> {
        self.data.channels.get(index).map(|c| c.as_slice())
    }
}

/// Platform-neutral streaming decoder (auto-detect + pipeline dispatch). This
/// is the shared core; the wasm `SdaDecoder` below is a thin binding over it.
pub struct StreamingDecoder {
    pipeline: Box<dyn Pipeline>,
    sniff: Option<Vec<u8>>,
    queue: VecDeque<FrameData>,
    errors: Vec<String>,
}

impl StreamingDecoder {
    /// Construct with a codec name (`"auto" | "truehd" | "eac3" | "dts" | "ac4"`).
    /// ALAC requires `with_config()` because its MP4 codec cookie is needed
    /// before decoding.
    pub fn new(codec: &str) -> Result<StreamingDecoder, String> {
        match codec {
            "auto" => Ok(StreamingDecoder {
                pipeline: Box::new(NoopPipeline),
                sniff: Some(Vec::with_capacity(64 * 1024)),
                queue: VecDeque::new(),
                errors: Vec::new(),
            }),
            _ => Ok(StreamingDecoder {
                pipeline: build_pipeline(codec)?,
                sniff: None,
                queue: VecDeque::new(),
                errors: Vec::new(),
            }),
        }
    }

    /// Construct a decoder whose container codec requires initialization bytes.
    /// Currently this is used by ALAC MP4 tracks and expects the full `alac` atom.
    pub fn with_config(codec: &str, config: &[u8]) -> Result<StreamingDecoder, String> {
        let pipeline: Box<dyn Pipeline> = match codec {
            "alac" => Box::new(
                alac_pipeline::AlacPipeline::from_cookie(config)
                    .map_err(|error| format!("invalid ALAC configuration: {error}"))?,
            ),
            other => return Err(format!("codec does not accept container configuration: {other}")),
        };
        Ok(StreamingDecoder {
            pipeline,
            sniff: None,
            queue: VecDeque::new(),
            errors: Vec::new(),
        })
    }

    /// Feed raw bitstream bytes (any chunking — the extractors re-frame).
    pub fn push(&mut self, data: &[u8]) -> Result<(), String> {
        diagnostics::input(data.len());
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
                    diagnostics::checkpoint("auto.sync_not_found");
                    return Err(
                        "could not detect codec from first 64 KiB (no TrueHD/E-AC-3/AC-4/DTS syncword)"
                            .into(),
                    );
                }
                None => return Ok(()),
            }
            return Ok(());
        }
        self.pipeline.push(data, &mut self.queue, &mut self.errors);
        Ok(())
    }

    /// Pop the next decoded frame, or `None` when more input is needed.
    pub fn next_frame(&mut self) -> Option<FrameData> {
        self.queue.pop_front()
    }

    /// Codec actually in use (meaningful after auto-detection kicked in).
    pub fn codec_name(&self) -> &'static str {
        self.pipeline.codec_name()
    }

    /// Decode errors are non-fatal (the pipelines resync); drain them here.
    pub fn drain_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    pub fn reset(&mut self) {
        self.pipeline.reset();
        self.queue.clear();
    }

    pub fn flush(&mut self) {
        self.pipeline.flush(&mut self.queue, &mut self.errors);
    }
}

/// Stateful streaming decoder. Construct with a codec name
/// (`"auto" | "truehd" | "eac3" | "dts"`), `push()` raw bytes, then drain
/// with `next_frame()` until it returns `undefined`. ALAC is constructed with
/// `with_config()` because its MP4 codec cookie is required before decoding.
#[cfg(feature = "wasm")]
#[wasm_bindgen]
pub struct SdaDecoder {
    inner: StreamingDecoder,
}

#[cfg(feature = "wasm")]
#[wasm_bindgen]
impl SdaDecoder {
    #[wasm_bindgen(constructor)]
    pub fn new(codec: &str) -> Result<SdaDecoder, JsValue> {
        Ok(SdaDecoder {
            inner: StreamingDecoder::new(codec).map_err(|e| JsValue::from_str(&e))?,
        })
    }

    #[wasm_bindgen(js_name = withConfig)]
    pub fn with_config(codec: &str, config: &[u8]) -> Result<SdaDecoder, JsValue> {
        Ok(SdaDecoder {
            inner: StreamingDecoder::with_config(codec, config).map_err(|e| JsValue::from_str(&e))?,
        })
    }

    /// Feed raw bitstream bytes (any chunking — the extractors re-frame).
    pub fn push(&mut self, data: &[u8]) -> Result<(), JsValue> {
        self.inner.push(data).map_err(|e| JsValue::from_str(&e))
    }

    /// Pop the next decoded frame, or `undefined` when more input is needed.
    #[wasm_bindgen(js_name = nextFrame)]
    pub fn next_frame(&mut self) -> Option<DecodedFrame> {
        self.inner
            .next_frame()
            .map(|data| DecodedFrame { data })
    }

    /// Codec actually in use (meaningful after auto-detection kicked in).
    #[wasm_bindgen(getter)]
    pub fn codec(&self) -> String {
        self.inner.codec_name().to_string()
    }

    /// Decode errors are non-fatal (the pipelines resync); drain them here.
    #[wasm_bindgen(js_name = drainErrors)]
    pub fn drain_errors(&mut self) -> Vec<String> {
        self.inner.drain_errors()
    }

    pub fn reset(&mut self) {
        self.inner.reset();
    }

    pub fn flush(&mut self) {
        self.inner.flush();
    }
}

pub(crate) fn build_pipeline(codec: &str) -> Result<Box<dyn Pipeline>, String> {
    match codec {
        "truehd" | "thd" | "mlp" => Ok(Box::new(truehd_pipeline::TruehdPipeline::new())),
        "eac3" | "ec3" | "ac3" => Ok(Box::new(eac3_pipeline::Eac3Pipeline::new())),
        "dts" | "dca" => Ok(Box::new(dts_pipeline::DtsPipeline::new())),
        "ac4" | "ac-4" => Ok(Box::new(ac4_pipeline::Ac4Pipeline::new())),
        other => Err(format!("unknown codec: {other}")),
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
    if data.len() >= 4 && data[0] == 0xac && matches!(data[1], 0x40 | 0x41) {
        return Some("ac4");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// JOC/Atmos E-AC-3 fixture from the harletty-bridge submodule
    /// (requires `git submodule update --init`).
    fn joc_fixture() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../harletty-bridge/harletty/tests/fixtures/joc_atmos_1s.eac3"
        );
        std::fs::read(path)
            .unwrap_or_else(|error| panic!("fixture missing ({error}); run `git submodule update --init`"))
    }

    /// Event JSON keys must stay camelCase and field-complete: the web app
    /// (packages/core/index.ts) and the future Android bridge both consume
    /// this exact shape.
    #[test]
    fn object_event_json_contract_is_camel_case() {
        let event = ObjectEvent {
            diffuse: 0.0,
            id: 10,
            sample_pos: 1536,
            has_pos: true,
            pos: [-1.0, 1.0, 0.0],
            gain_db: -3.5,
            size: [0.1, 0.0, 0.2],
            anchor: "room".into(),
            distance_m: Some(1.5),
            distance_infinite: false,
            screen_factor: Some(0.8),
            depth_factor: None,
            ramp_duration: 1536,
        };
        let value: serde_json::Value = serde_json::to_value(&event).unwrap();
        for key in [
            "id",
            "samplePos",
            "hasPos",
            "pos",
            "gainDb",
            "size",
            "anchor",
            "distanceM",
            "distanceInfinite",
            "screenFactor",
            "depthFactor",
            "rampDuration",
        ] {
            assert!(value.get(key).is_some(), "missing key {key} in {value}");
        }
        assert_eq!(value["samplePos"], 1536);
        assert_eq!(value["gainDb"], -3.5);
        assert_eq!(value["distanceM"], 1.5);
        assert!(value["depthFactor"].is_null());
    }

    #[test]
    fn decodes_joc_fixture_with_odd_chunking() {
        let bytes = joc_fixture();
        let mut decoder = StreamingDecoder::new("eac3").unwrap();
        // Deliberately awkward chunking: the pipelines must re-frame.
        let mut offset = 0;
        while offset < bytes.len() {
            let take = if offset == 0 { 7 } else { 613 };
            let end = (offset + take).min(bytes.len());
            decoder.push(&bytes[offset..end]).unwrap();
            offset = end;
        }
        decoder.flush();

        let mut frames = Vec::new();
        while let Some(frame) = decoder.next_frame() {
            frames.push(frame);
        }
        assert!(!frames.is_empty(), "expected decoded frames");
        assert_eq!(decoder.drain_errors(), Vec::<String>::new());

        let first = &frames[0];
        assert_eq!(first.sample_rate, 48000);
        assert!(!first.channels.is_empty());
        assert!(first.channels.iter().all(|c| !c.is_empty()));
        let mut non_finite = 0_usize;
        for frame in &frames {
            for channel in &frame.channels {
                non_finite += channel.iter().filter(|v| !v.is_finite()).count();
            }
        }
        println!("NON_FINITE_SAMPLES={non_finite} total_frames={}", frames.len());
        assert_eq!(non_finite, 0, "decoder output must be finite");
        assert!(
            first.labels.iter().any(|l| l.starts_with("Obj_")),
            "JOC fixture should carry object labels, got {:?}",
            first.labels
        );
        assert!(
            !first.object_channels.is_empty(),
            "first frame must declare the object↔channel mapping"
        );
        // Events serialize through the same serde shape the web app parses.
        let events_json = serde_json::to_string(&first.events).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&events_json).unwrap();
        assert!(parsed.is_array());
        assert!(first.sample_pos + first.channels[0].len() as u64 > 0);
    }

    #[test]
    fn auto_detection_identifies_eac3() {
        let bytes = joc_fixture();
        let mut decoder = StreamingDecoder::new("auto").unwrap();
        // Syncword sits at byte 0, so the very first push detects and decodes.
        assert_eq!(decoder.codec_name(), "auto");
        decoder.push(&bytes[..1000]).unwrap();
        assert_eq!(decoder.codec_name(), "eac3");
        decoder.push(&bytes[1000..]).unwrap();
        decoder.flush();
        assert!(decoder.next_frame().is_some(), "sniffed bytes must still decode");
    }

    #[test]
    fn unknown_codec_and_bad_auto_stream_error_cleanly() {
        assert!(StreamingDecoder::new("aac").is_err());
        let mut decoder = StreamingDecoder::new("auto").unwrap();
        let garbage = vec![0x12u8; 64 * 1024];
        assert!(decoder.push(&garbage).is_err());
    }
}
