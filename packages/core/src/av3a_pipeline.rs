//! Audio Vivid / AV3A channel beds and metadata-mapped Mix scenes. One source decoder and sample clock on
//! native and WASM hosts; no vendor binaural mix, gain boost or object guessing.
use crate::av3a_objects::ObjectScene;
use crate::{FrameData, ObjectChannelDecl, ObjectEvent, Pipeline};
use avs3a::backend::{
    McDecoderBackend, MixDecoderBackend, MonoDecoderBackend, StereoDecoderBackend,
};
use avs3a::decode::PendingDecoder;
use avs3a::header::{ChannelConfig, CodecProfile};
use avs3a::{BuiltinDecoder, EncodedFrame, FrameStream, StreamEvent};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::collections::VecDeque;

const OUTPUT_RATE: u32 = 48_000;
const FRAME_SAMPLES: usize = 1024;

/// AV3A's direct-speaker PCM follows the channel-mask order used by the
/// reference FFmpeg decoder (7.1.4 BACK: backs precede sides). Use explicit
/// labels so Dolby target layouts can remap beds without interchanging them.
fn bed_labels(config: ChannelConfig) -> Result<Vec<String>, String> {
    use ChannelConfig::*;
    let labels: &[&str] = match config {
        Mono => &["C"],
        Stereo => &["L", "R"],
        Mc4_0 => &["L", "R", "Ls", "Rs"],
        Mc5_1 => &["L", "R", "C", "LFE", "Ls", "Rs"],
        Mc7_1 => &["L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs"],
        Mc5_1_2 => &["L", "R", "C", "LFE", "Ls", "Rs", "Tsl", "Tsr"],
        Mc5_1_4 => &["L", "R", "C", "LFE", "Ls", "Rs", "Tfl", "Tfr", "Tbl", "Tbr"],
        Mc7_1_2 => &["L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs", "Tsl", "Tsr"],
        Mc7_1_4 => &[
            "L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs", "Tfl", "Tfr", "Tbl", "Tbr",
        ],
        _ => {
            return Err(format!(
                "unsupported Audio Vivid speaker configuration {config:?}"
            ))
        }
    };
    Ok(labels.iter().map(|s| (*s).into()).collect())
}

// Do not call the upstream all-profile factory here: its HOA construction
// branch reserves enough stack to overflow a 512 KiB Apple worker even for a
// channel-bed input. Separate no-inline constructors bound the active stack
// to the backend actually selected; synthesis itself is unchanged.
#[inline(never)]
fn mono_decoder(h: &avs3a::FrameHeader) -> Result<BuiltinDecoder, String> {
    let backend = MonoDecoderBackend::new_builtin().map_err(|e| e.to_string())?;
    Ok(BuiltinDecoder::Mono(Box::new(
        PendingDecoder::new(backend)
            .configure(h)
            .map_err(|e| e.to_string())?,
    )))
}
#[inline(never)]
fn stereo_decoder(h: &avs3a::FrameHeader) -> Result<BuiltinDecoder, String> {
    let backend = StereoDecoderBackend::new_builtin().map_err(|e| e.to_string())?;
    Ok(BuiltinDecoder::Stereo(Box::new(
        PendingDecoder::new(backend)
            .configure(h)
            .map_err(|e| e.to_string())?,
    )))
}
#[inline(never)]
fn mc_decoder(h: &avs3a::FrameHeader) -> Result<BuiltinDecoder, String> {
    let backend = McDecoderBackend::new_builtin().map_err(|e| e.to_string())?;
    Ok(BuiltinDecoder::Mc(Box::new(
        PendingDecoder::new(backend)
            .configure(h)
            .map_err(|e| e.to_string())?,
    )))
}

#[inline(never)]
fn mix_decoder(h: &avs3a::FrameHeader) -> Result<BuiltinDecoder, String> {
    let backend = MixDecoderBackend::new_builtin().map_err(|e| e.to_string())?;
    Ok(BuiltinDecoder::Mix(Box::new(
        PendingDecoder::new(backend)
            .configure(h)
            .map_err(|e| e.to_string())?,
    )))
}

pub struct Av3aPipeline {
    parser: FrameStream,
    decoder: Option<BuiltinDecoder>,
    resampler: Option<SincFixedIn<f32>>,
    labels: Vec<String>,
    raw_beds: Vec<String>,
    declarations: Vec<ObjectChannelDecl>,
    objects: Option<ObjectScene>,
    events: VecDeque<ObjectEvent>,
    transport_channels: usize,
    source_rate: u32,
    input_samples: u64,
    output_samples: u64,
    delay: usize,
    failed: bool,
    finished: bool,
}
impl Av3aPipeline {
    pub fn new() -> Self {
        Self {
            parser: FrameStream::new(),
            decoder: None,
            resampler: None,
            labels: Vec::new(),
            raw_beds: Vec::new(),
            declarations: Vec::new(),
            objects: None,
            events: VecDeque::new(),
            transport_channels: 0,
            source_rate: OUTPUT_RATE,
            input_samples: 0,
            output_samples: 0,
            delay: 0,
            failed: false,
            finished: false,
        }
    }
    fn configure(&mut self, h: &avs3a::FrameHeader) -> Result<(), String> {
        if h.samples_per_channel as usize != FRAME_SAMPLES {
            return Err("invalid Audio Vivid sample count".into());
        }
        self.transport_channels = usize::from(h.channels);
        match h.profile {
            CodecProfile::ChannelBased => {
                self.objects = None;
                self.labels = bed_labels(h.channel_config.ok_or("Audio Vivid channel configuration missing")?)?;
                if self.labels.len() != self.transport_channels { return Err("invalid Audio Vivid channel count".into()); }
                self.raw_beds = self.labels.clone();
            }
            CodecProfile::Mixed => {
                if self.objects.as_ref().map(ObjectScene::transport_channels) != Some(self.transport_channels) {
                    self.objects = Some(ObjectScene::new(self.transport_channels));
                }
            }
            CodecProfile::Hoa => return Err("Audio Vivid HOA routing is not validated; refusing to treat coefficients as speaker beds".into()),
        }
        self.source_rate = h.sample_rate;
        self.decoder = Some(if h.profile == CodecProfile::Mixed {
            mix_decoder(h)?
        } else {
            match h.channel_config {
                Some(ChannelConfig::Mono) => mono_decoder(h)?,
                Some(ChannelConfig::Stereo) => stereo_decoder(h)?,
                _ => mc_decoder(h)?,
            }
        });
        if h.sample_rate != OUTPUT_RATE && h.profile != CodecProfile::Mixed {
            let resampler = SincFixedIn::<f32>::new(
                OUTPUT_RATE as f64 / h.sample_rate as f64,
                1.0,
                SincInterpolationParameters {
                    sinc_len: 256,
                    f_cutoff: 0.95,
                    interpolation: SincInterpolationType::Cubic,
                    oversampling_factor: 256,
                    window: WindowFunction::BlackmanHarris2,
                },
                FRAME_SAMPLES,
                self.labels.len(),
            )
            .map_err(|e| e.to_string())?;
            self.delay = resampler.output_delay();
            self.resampler = Some(resampler);
        }
        Ok(())
    }
    fn decode(
        &mut self,
        frame: &EncodedFrame,
        out: &mut VecDeque<FrameData>,
    ) -> Result<(), String> {
        if !frame.crc_is_valid() {
            return Err("Audio Vivid CRC mismatch".into());
        }
        if self.decoder.is_none() {
            self.configure(frame.header())?;
        }
        let decoder = self.decoder.as_mut().unwrap();
        let mut pcm = vec![0.0; decoder.sample_count().map_err(|e| e.to_string())?];
        decoder
            .decode_into_f32(frame, &mut pcm)
            .map_err(|e| e.to_string())?;
        // Upstream float synthesis is in PCM16 units. Normalise exactly once,
        // retain headroom (no PCM16 clipping/quantisation before spatial mixing).
        let n = self.transport_channels;
        let mut planar = vec![vec![0.0; FRAME_SAMPLES]; n];
        for (i, samples) in pcm.chunks_exact(n).enumerate() {
            for (ch, value) in samples.iter().enumerate() {
                if !value.is_finite() {
                    return Err("non-finite Audio Vivid PCM".into());
                }
                planar[ch][i] = value / avs3a::decode::FLOAT_FULL_SCALE;
            }
        }
        if let Some(scene) = &mut self.objects {
            let at = (self.input_samples * u64::from(OUTPUT_RATE)
                + u64::from(self.source_rate) / 2)
                / u64::from(self.source_rate);
            let end = ((self.input_samples + FRAME_SAMPLES as u64) * u64::from(OUTPUT_RATE)
                + u64::from(self.source_rate) / 2)
                / u64::from(self.source_rate);
            let meta = decoder
                .last_metadata_values()
                .ok_or("Audio Vivid Mix metadata missing")?;
            let routed = scene.process(planar, meta, at, (end - at) as u32)?;
            self.labels = routed.labels;
            self.raw_beds = routed.beds;
            self.declarations = routed.declarations;
            self.events.extend(routed.events);
            planar = routed.channels;
            if self.source_rate != OUTPUT_RATE && self.resampler.is_none() {
                let src = SincFixedIn::<f32>::new(
                    OUTPUT_RATE as f64 / self.source_rate as f64,
                    1.0,
                    SincInterpolationParameters {
                        sinc_len: 256,
                        f_cutoff: 0.95,
                        interpolation: SincInterpolationType::Cubic,
                        oversampling_factor: 256,
                        window: WindowFunction::BlackmanHarris2,
                    },
                    FRAME_SAMPLES,
                    self.labels.len(),
                )
                .map_err(|e| e.to_string())?;
                self.delay = src.output_delay();
                self.resampler = Some(src);
            }
        }
        self.input_samples += FRAME_SAMPLES as u64;
        let output = if let Some(src) = &mut self.resampler {
            src.process(&planar, None).map_err(|e| e.to_string())?
        } else {
            planar
        };
        self.emit(output, out);
        Ok(())
    }
    fn expected_samples(&self) -> u64 {
        (self.input_samples * OUTPUT_RATE as u64 + self.source_rate as u64 / 2)
            / self.source_rate as u64
    }
    fn emit(&mut self, mut channels: Vec<Vec<f32>>, out: &mut VecDeque<FrameData>) {
        let available = channels[0].len();
        let skip = self.delay.min(available);
        self.delay -= skip;
        let count = (available - skip)
            .min(self.expected_samples().saturating_sub(self.output_samples) as usize);
        if count == 0 {
            return;
        }
        for channel in &mut channels {
            channel.drain(..skip);
            channel.truncate(count);
        }
        out.push_back(FrameData {
            compatibility_bed: None,
            codec: "av3a",
            sample_rate: OUTPUT_RATE,
            sample_pos: self.output_samples,
            channels,
            labels: self.labels.clone(),
            raw_bed_labels: self.raw_beds.clone(),
            events: {
                let end = self.output_samples + count as u64;
                let mut events = Vec::new();
                while self.events.front().is_some_and(|e| e.sample_pos < end) {
                    let e = self.events.pop_front().unwrap();
                    debug_assert!(e.sample_pos >= self.output_samples);
                    events.push(e);
                }
                events
            },
            object_channels: self.declarations.clone(),
            program_loudness: None,
            ramp_duration: 0,
        });
        self.output_samples += count as u64;
    }
}
impl Pipeline for Av3aPipeline {
    fn codec_name(&self) -> &'static str {
        "av3a"
    }
    fn push(&mut self, data: &[u8], out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        if self.failed || self.finished {
            return;
        }
        let result = (|| -> Result<(), String> {
            for event in self.parser.push(data).map_err(|e| e.to_string())? {
                match event {
                    StreamEvent::Frame(frame) => self.decode(&frame, out)?,
                    StreamEvent::Skipped { bytes } => {
                        return Err(format!("Audio Vivid lost framing ({bytes} bytes)"))
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.failed = true;
            errors.push(format!("Audio Vivid: {error}"));
        }
    }
    fn flush(&mut self, out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        if self.failed || self.finished {
            return;
        }
        self.finished = true;
        if self.parser.buffered_len() != 0 {
            self.failed = true;
            errors.push("Audio Vivid: truncated final frame".into());
            return;
        }
        // Drain sinc lookahead once, retaining the exact rational programme
        // length. No extra silence/filter tail is added to playback duration.
        while self.output_samples < self.expected_samples() {
            let Some(src) = &mut self.resampler else {
                break;
            };
            match src.process(&vec![vec![0.0; FRAME_SAMPLES]; self.labels.len()], None) {
                Ok(pcm) => self.emit(pcm, out),
                Err(error) => {
                    self.failed = true;
                    errors.push(format!("Audio Vivid SRC: {error}"));
                    break;
                }
            }
        }
    }
    fn reset(&mut self) {
        // Reset is a seek within a programme. Keep its static scene mapping;
        // fresh metadata can replace it before new PCM is emitted.
        let mut scene = self.objects.take();
        if let Some(scene) = &mut scene {
            scene.reset_dynamic();
        }
        *self = Self::new();
        self.objects = scene;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/av3a-7-1-4-three-frames.av3a");
    fn decode(chunk: usize) -> Vec<Vec<f32>> {
        let mut decoder = crate::StreamingDecoder::new("auto").unwrap();
        let mut pcm = vec![vec![]; 12];
        let mut at = 0;
        for bytes in FIXTURE.chunks(chunk) {
            decoder.push(bytes).unwrap();
        }
        decoder.flush();
        assert!(decoder.drain_errors().is_empty());
        while let Some(f) = decoder.next_frame() {
            assert_eq!(f.codec, "av3a");
            assert_eq!(f.sample_rate, 48000);
            assert_eq!(f.sample_pos, at);
            assert_eq!(f.labels, bed_labels(ChannelConfig::Mc7_1_4).unwrap());
            assert!(f.events.is_empty() && f.object_channels.is_empty());
            at += f.channels[0].len() as u64;
            for (dst, src) in pcm.iter_mut().zip(f.channels) {
                assert!(src.iter().all(|s| s.is_finite()));
                dst.extend(src);
            }
        }
        assert_eq!(at, (3 * 1024 * 48000 + 22050) / 44100);
        pcm
    }
    #[test]
    #[cfg(all(not(debug_assertions), not(target_arch = "wasm32")))]
    fn av3a_apple_sized_worker_stack() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| decode(8192))
            .unwrap()
            .join()
            .unwrap();
    }
    #[test]
    fn av3a_mix_selects_object_transport_without_guessing_speaker_labels() {
        let frame = FrameStream::new()
            .push(FIXTURE)
            .unwrap()
            .into_iter()
            .find_map(|e| {
                if let StreamEvent::Frame(frame) = e {
                    Some(frame)
                } else {
                    None
                }
            })
            .unwrap();
        let mut h = frame.header().clone();
        h.profile = CodecProfile::Mixed;
        h.channel_config = None;
        h.sound_bed_type = Some(avs3a::header::SoundBedType::ObjectsOnly);
        h.objects = 16;
        h.bed_channels = 0;
        h.channels = 16;
        h.has_lfe = false;
        h.sample_rate = 48000;
        h.bitrate = 1536000;
        h.bed_bitrate = None;
        h.object_bitrate = Some(96000);
        h.header_len = 8;
        h.payload_len = 4088;
        h.payload_bits = 4088 * 8;
        h.frame_len = 4096;
        let mut pipeline = Av3aPipeline::new();
        pipeline.configure(&h).unwrap();
        assert_eq!(
            pipeline.decoder.as_ref().unwrap().sample_count().unwrap(),
            16 * 1024
        );
        assert_eq!(
            pipeline.decoder.as_ref().unwrap().config().profile,
            CodecProfile::Mixed
        );
        assert!(pipeline.labels.is_empty() && pipeline.raw_beds.is_empty());
        assert_eq!(pipeline.objects.as_ref().unwrap().transport_channels(), 16);
    }
    #[test]
    fn av3a_chunking_clock_and_layout() {
        assert_eq!(decode(1), decode(8192));
    }
    #[test]
    fn av3a_corruption_is_not_output_as_audio() {
        let mut bytes = FIXTURE.to_vec();
        bytes[40] ^= 1;
        let mut pipeline = Av3aPipeline::new();
        let mut out = VecDeque::new();
        let mut errors = vec![];
        pipeline.push(&bytes, &mut out, &mut errors);
        assert!(out.is_empty());
        assert!(errors[0].contains("CRC"));
        pipeline.push(FIXTURE, &mut out, &mut errors);
        assert!(out.is_empty());
        pipeline.reset();
        pipeline.push(FIXTURE, &mut out, &mut errors);
        assert!(!out.is_empty());
    }
    #[test]
    fn av3a_truncated_tail_is_reported() {
        let mut p = Av3aPipeline::new();
        let mut out = VecDeque::new();
        let mut errors = vec![];
        p.push(&FIXTURE[..FIXTURE.len() - 1], &mut out, &mut errors);
        p.flush(&mut out, &mut errors);
        assert!(errors.iter().any(|s| s.contains("truncated")));
    }
}
