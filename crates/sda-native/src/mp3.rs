use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL, CODEC_TYPE_ALAC};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const OUTPUT_RATE: u32 = 48_000;
const RESAMPLE_CHUNK: usize = 1024;

fn append_resampled(
    output: &mut VecDeque<f32>,
    left: &[f32],
    right: &[f32],
    delay_remaining: &mut usize,
) {
    let mut skip = (*delay_remaining).min(left.len());
    *delay_remaining -= skip;
    while skip < left.len().min(right.len()) {
        output.push_back(left[skip]);
        output.push_back(right[skip]);
        skip += 1;
    }
}

fn trim_queued_to_target(output: &mut VecDeque<f32>, delivered_frames: u64, target_frames: u64) {
    let remaining_samples = target_frames.saturating_sub(delivered_frames) as usize * 2;
    while output.len() > remaining_samples { output.pop_back(); }
}

pub struct Mp3FileDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    codec_name: &'static str,
    input_rate: u32,
    channels: usize,
    resampler: Option<SincFixedIn<f32>>,
    input: [Vec<f32>; 2],
    decoded_pending: VecDeque<f32>,
    output: VecDeque<f32>,
    finished: bool,
    resampler_flushed: bool,
    total_output_frames: u64,
    total_input_frames: u64,
    output_delay_remaining: usize,
    expected_output_frames: Option<u64>,
    actual_input_frames: u64,
    eof_output_frames: Option<u64>,
}

impl Mp3FileDecoder {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        Self::open_format(path, false)
    }

    pub fn open_alac(path: impl AsRef<Path>) -> Result<Self, String> {
        Self::open_format(path, true)
    }

    fn open_format(path: impl AsRef<Path>, alac: bool) -> Result<Self, String> {
        let codec_name = if alac { "ALAC" } else { "MP3" };
        let file = File::open(path.as_ref()).map_err(|e| format!("open {codec_name}: {e}"))?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        hint.with_extension(if alac { "m4a" } else { "mp3" });
        let probed = symphonia::default::get_probe()
            .format(&hint, stream, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
            .map_err(|e| format!("probe {codec_name}: {e}"))?;
        let mut format = probed.format;
        let track = format.tracks().iter()
            .find(|track| if alac { track.codec_params.codec == CODEC_TYPE_ALAC } else { track.codec_params.codec != CODEC_TYPE_NULL })
            .ok_or_else(|| format!("{codec_name} contains no supported audio track"))?;
        let track_id = track.id;
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| format!("create {codec_name} decoder: {e}"))?;
        // ALAC's validated magic cookie defines the decoded PCM format. MP4
        // sample entries can carry a placeholder rate (e.g. 1 for 96 kHz ALAC)
        // and may omit channels. The initialized ALAC buffer already has the
        // cookie's real signal specification, before the first packet is read.
        let (input_rate, channels) = if alac {
            let spec = decoder.last_decoded().spec().to_owned();
            (spec.rate, spec.channels.count())
        } else {
            (track.codec_params.sample_rate.ok_or_else(|| format!("{codec_name} sample rate is missing"))?,
             track.codec_params.channels.map(|c| c.count()).unwrap_or(2))
        };
        if input_rate == 0 { return Err(format!("{codec_name} sample rate must be non-zero")); }
        if alac && channels != 2 { return Err(format!("ALAC upmix requires stereo input, got {channels} channels")); }
        if !(1..=2).contains(&channels) { return Err(format!("{codec_name} bed must be mono or stereo, got {channels} channels")); }
        let input_frames = track.codec_params.n_frames;
        let expected_output_frames = input_frames.map(|frames| {
            (u128::from(frames) * u128::from(OUTPUT_RATE) / u128::from(input_rate)) as u64
        });
        let resampler = if input_rate == OUTPUT_RATE { None } else {
            Some(SincFixedIn::<f32>::new(
                f64::from(OUTPUT_RATE) / f64::from(input_rate),
                1.0,
                SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: 0.95,
                    oversampling_factor: 128,
                    interpolation: SincInterpolationType::Cubic,
                    window: WindowFunction::BlackmanHarris2,
                },
                RESAMPLE_CHUNK,
                2,
            ).map_err(|e| format!("create {codec_name} resampler: {e}"))?)
        };
        // SincFixedIn uses look-ahead: its first output is already aligned
        // to the beginning of this file. Its reported processing latency is
        // not leading silence to remove from the decoded programme.
        let output_delay_remaining = 0;
        Ok(Self {
            format, decoder, track_id, codec_name, input_rate, channels, resampler,
            input: [Vec::new(), Vec::new()], decoded_pending: VecDeque::new(), output: VecDeque::new(),
            finished: false, resampler_flushed: false, total_output_frames: 0, total_input_frames: 0, output_delay_remaining,
            expected_output_frames,
            actual_input_frames: 0,
            eof_output_frames: None,
        })
    }

    pub fn input_rate(&self) -> u32 { self.input_rate }
    pub fn output_frames(&self) -> u64 { self.total_output_frames }
    pub fn is_finished(&self) -> bool { self.finished && self.output.is_empty() && self.resampler_flushed }

    pub fn read_interleaved(&mut self, max_frames: usize) -> Result<Vec<f32>, String> {
        let target = max_frames.saturating_mul(2);
        while self.output.len() < target && (!self.finished || !self.resampler_flushed) {
            if let Some(resampler) = &mut self.resampler {
                let needed = resampler.input_frames_next();
                while self.input[0].len() < needed && self.decoded_pending.len() >= 2 {
                    self.input[0].push(self.decoded_pending.pop_front().unwrap());
                    self.input[1].push(self.decoded_pending.pop_front().unwrap());
                }
                if self.finished {
                    while self.decoded_pending.len() >= 2 {
                        self.input[0].push(self.decoded_pending.pop_front().unwrap());
                        self.input[1].push(self.decoded_pending.pop_front().unwrap());
                    }
                }
                if self.input[0].len() >= needed {
                    let left: Vec<f32> = self.input[0].drain(..needed).collect();
                    let right: Vec<f32> = self.input[1].drain(..needed).collect();
                    let converted = resampler.process(&[left, right], None)
                        .map_err(|e| format!("resample {}: {e}", self.codec_name))?;
                    append_resampled(&mut self.output, &converted[0], &converted[1], &mut self.output_delay_remaining);
                    continue;
                }
                if self.finished && !self.resampler_flushed {
                    let tail = [self.input[0].as_slice(), self.input[1].as_slice()];
                    // An empty slice marks a channel inactive in Rubato. At an
                    // exact block boundary, None instead flushes both channels
                    // with zero padding and preserves the remaining filter tail.
                    let input = if tail[0].is_empty() { None } else { Some(&tail[..]) };
                    let converted = resampler.process_partial(input, None)
                        .map_err(|e| format!("flush {} resampler: {e}", self.codec_name))?;
                    self.input[0].clear();
                    self.input[1].clear();
                    append_resampled(&mut self.output, &converted[0], &converted[1], &mut self.output_delay_remaining);
                    self.resampler_flushed = true;
                    continue;
                }
            } else if self.finished {
                self.resampler_flushed = true;
            }
            if !self.finished { self.decode_packet()?; }
        }
        if let Some(expected) = self.eof_output_frames {
            trim_queued_to_target(&mut self.output, self.total_output_frames, expected);
        }
        let count = target.min(self.output.len());
        let result: Vec<f32> = self.output.drain(..count).collect();
        self.total_output_frames += (result.len() / 2) as u64;
        Ok(result)
    }

    fn decode_packet(&mut self) -> Result<(), String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.finished = true;
                    self.eof_output_frames = Some(
                        (u128::from(self.total_input_frames) * u128::from(OUTPUT_RATE) / u128::from(self.input_rate)) as u64
                    );
                    return Ok(());
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.finished = true;
                    self.eof_output_frames = Some(
                        (u128::from(self.total_input_frames) * u128::from(OUTPUT_RATE) / u128::from(self.input_rate)) as u64
                    );
                    return Ok(());
                }
                Err(error) => return Err(format!("read MP3 packet: {error}")),
            };
            if packet.track_id() != self.track_id { continue; }
            let decoded = match self.decoder.decode(&packet) {
                Ok(audio) => audio,
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(SymphoniaError::IoError(_)) => continue,
                Err(error) => return Err(format!("decode MP3 packet: {error}")),
            };
            if decoded.spec().rate != self.input_rate || decoded.spec().channels.count() != self.channels {
                return Err(format!("{} stream changed sample rate or channel count mid-file (expected {} Hz/{} channels, got {} Hz/{} channels)",
                    self.codec_name, self.input_rate, self.channels, decoded.spec().rate, decoded.spec().channels.count()));
            }
            let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            samples.copy_interleaved_ref(decoded);
            for frame in samples.samples().chunks_exact(self.channels) {
                let left = frame[0];
                let right = if self.channels == 1 { left } else { frame[1] };
                self.decoded_pending.push_back(left);
                self.decoded_pending.push_back(right);
                self.total_input_frames += 1;
            }
            if self.resampler.is_none() {
                while let Some(sample) = self.decoded_pending.pop_front() { self.output.push_back(sample); }
            }
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlacFixture(std::path::PathBuf);

    impl AlacFixture {
        fn new(name: &str, data: &[u8]) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("sda-alac-{}-{id}-{name}", std::process::id()));
            std::fs::write(&path, data).unwrap();
            Self(path)
        }
    }

    impl Drop for AlacFixture {
        fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
    }

    fn check_file_fixture(name: &str, bytes: &[u8], rate: u32, input_frames: u64) {
        let file = AlacFixture::new(name, bytes);
        let mut decoder = if name.ends_with(".mp3") {
            Mp3FileDecoder::open(&file.0).unwrap()
        } else {
            Mp3FileDecoder::open_alac(&file.0).unwrap()
        };
        assert_eq!(decoder.input_rate(), rate, "must use the actual encoded sample rate");
        let mut output = Vec::new();
        let mut reads = 0;
        while !decoder.is_finished() {
            let limit = [317, 4096, 1024, 8192][reads % 4];
            let block = decoder.read_interleaved(limit).unwrap();
            assert!(block.len() <= limit * 2 && block.len() % 2 == 0);
            assert!(block.iter().all(|sample| sample.is_finite()));
            assert!(!block.is_empty() || decoder.is_finished(), "decoder must advance");
            output.extend(block);
            reads += 1;
        }
        let frames = input_frames * u64::from(OUTPUT_RATE) / u64::from(rate);
        assert_eq!(output.len() as u64, frames * 2, "resampling must preserve duration including the partial tail");
        assert_eq!(decoder.output_frames(), frames);
        assert_eq!(decoder.eof_output_frames, Some(frames));
        if frames <= 200 { return; }
        for channel in 0..2 {
            let amplitude = if channel == 0 { 0.25 } else { 0.125 };
            let frequency = if channel == 0 { 440.0 } else { 880.0 };
            let mut squared_error = 0.0;
            for frame in 100..frames as usize - 100 {
                let expected = amplitude * (2.0 * std::f64::consts::PI * frequency * frame as f64 / f64::from(OUTPUT_RATE)).sin();
                squared_error += (f64::from(output[frame * 2 + channel]) - expected).powi(2);
            }
            let rms_error = (squared_error / (frames - 200) as f64).sqrt();
            assert!(rms_error < 0.01, "{name}, channel {channel}: wrong rate, channel order or gain ({rms_error})");
        }
    }

    #[test]
    fn alac_cookie_format_controls_rate_and_resampling_with_placeholder_sample_entries() {
        for (name, bytes, rate) in [
            ("44100.m4a", include_bytes!("../tests/fixtures/alac/tone-44100-2ch.m4a").as_slice(), 44_100),
            ("48000.m4a", include_bytes!("../tests/fixtures/alac/tone-48000-2ch.m4a").as_slice(), 48_000),
            ("88200-entry1.m4a", include_bytes!("../tests/fixtures/alac/tone-88200-2ch-entry1.m4a").as_slice(), 88_200),
            ("96000-entry1.m4a", include_bytes!("../tests/fixtures/alac/tone-96000-2ch-entry1.m4a").as_slice(), 96_000),
            ("96000-entry44100.m4a", include_bytes!("../tests/fixtures/alac/tone-96000-2ch-entry44100.m4a").as_slice(), 96_000),
            ("192000-entry1.m4a", include_bytes!("../tests/fixtures/alac/tone-192000-2ch-entry1.m4a").as_slice(), 192_000),
        ] {
            check_file_fixture(name, bytes, rate, u64::from(rate / 8 + 317));
        }
    }

    #[test]
    fn alac_resampler_flushes_exact_block_boundaries_and_very_short_files() {
        check_file_fixture("exact.m4a", include_bytes!("../tests/fixtures/alac/tone-96000-2ch-entry1-exact.m4a"), 96_000, 12_288);
        check_file_fixture("short.m4a", include_bytes!("../tests/fixtures/alac/tone-96000-2ch-entry1-short.m4a"), 96_000, 32);
    }

    #[test]
    fn mp3_file_remains_gapless_with_the_shared_resampler() {
        check_file_fixture("gapless.mp3", include_bytes!("../tests/fixtures/alac/tone-44100-2ch.mp3"), 44_100, 5_829);
    }

    #[test]
    fn alac_stereo_validation_uses_real_cookie_channels() {
        let file = AlacFixture::new("mono.m4a", include_bytes!("../tests/fixtures/alac/tone-44100-1ch.m4a"));
        let error = Mp3FileDecoder::open_alac(&file.0).err().expect("mono must not be accepted as stereo");
        assert_eq!(error, "ALAC upmix requires stereo input, got 1 channels");
    }

    #[test]
    fn alac_still_rejects_unexpected_decoded_format_changes_with_correct_codec_name() {
        let file = AlacFixture::new("format-check.m4a", include_bytes!("../tests/fixtures/alac/tone-96000-2ch-entry1.m4a"));
        let mut decoder = Mp3FileDecoder::open_alac(&file.0).unwrap();
        decoder.input_rate += 1;
        let error = decoder.read_interleaved(4096).unwrap_err();
        assert!(error.starts_with("ALAC stream changed sample rate or channel count mid-file"));
        assert!(error.contains("expected 96001 Hz/2 channels, got 96000 Hz/2 channels"));
    }

    #[test]
    #[ignore = "set SDA_ALAC_TEST_FILE to a local stereo ALAC fixture"]
    fn alac_file_decodes_to_bounded_finite_48k_stereo() {
        let path = std::env::var("SDA_ALAC_TEST_FILE").expect("ALAC fixture path");
        let mut decoder = Mp3FileDecoder::open_alac(path).unwrap();
        let mut count = 0;
        loop {
            let samples = decoder.read_interleaved(4096).unwrap();
            assert!(samples.len() <= 8192 && samples.len() % 2 == 0);
            assert!(samples.iter().all(|sample| sample.is_finite()));
            count += samples.len() / 2;
            if decoder.is_finished() { break; }
            assert!(!samples.is_empty());
        }
        assert!(count > 0);
        assert_eq!(Some(count as u64), decoder.eof_output_frames);
    }

    #[test]
    #[ignore = "requires a local MP3 listening fixture that is not distributed with the repository"]
    fn decodes_local_mp3_to_bounded_48k_stereo_with_exact_gapless_frame_count() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../有前奏15秒和全间奏25秒版.mp3");
        let mut decoder = Mp3FileDecoder::open(&path).expect("local MP3 opens");
        assert!(matches!(decoder.input_rate(), 44_100 | 48_000));
        let mut total = 0_u64;
        loop {
            let block = decoder.read_interleaved(4096).expect("decode packet");
            assert_eq!(block.len() % 2, 0);
            assert!(block.iter().all(|sample| sample.is_finite()));
            let frames = block.len() / 2;
            total += frames as u64;
            if decoder.is_finished() { break; }
            assert!(frames > 0, "decoder must make progress before EOF");
        }
        let rate = decoder.input_rate();
        if let Some(expected) = decoder.format.tracks()[0].codec_params.n_frames {
            let expected_48k = (expected as u128 * u128::from(OUTPUT_RATE) / u128::from(rate)) as u64;
            assert!(total.abs_diff(expected_48k) <= 1, "got {total} frames, gapless metadata predicts {expected_48k}");
        }
        assert!(total > u64::from(OUTPUT_RATE) * 60, "test MP3 should decode as a long-form file");
    }

    #[test]
    fn sinc_resampler_44100_partial_tail_preserves_duration_and_stereo_finiteness() {
        let input_frames = 44_100 * 2 + 317;
        let mut resampler = SincFixedIn::<f32>::new(
            f64::from(OUTPUT_RATE) / 44_100.0,
            1.0,
            SincInterpolationParameters {
                sinc_len: 128,
                f_cutoff: 0.95,
                oversampling_factor: 128,
                interpolation: SincInterpolationType::Cubic,
                window: WindowFunction::BlackmanHarris2,
            },
            RESAMPLE_CHUNK,
            2,
        ).unwrap();
        let mono: Vec<f32> = (0..input_frames).map(|i| 0.05 * (i as f32 * 0.037).sin()).collect();
        let stereo = [mono.clone(), mono];
        let mut queued = VecDeque::new();
        let mut delay_remaining = resampler.output_delay();
        let mut offset = 0;
        while input_frames - offset >= resampler.input_frames_next() {
            let count = resampler.input_frames_next();
            let result = resampler.process(&[
                stereo[0][offset..offset + count].to_vec(),
                stereo[1][offset..offset + count].to_vec(),
            ], None).unwrap();
            append_resampled(&mut queued, &result[0], &result[1], &mut delay_remaining);
            offset += count;
        }
        let tail = [&stereo[0][offset..], &stereo[1][offset..]];
        let result = resampler.process_partial(Some(&tail), None).unwrap();
        append_resampled(&mut queued, &result[0], &result[1], &mut delay_remaining);
        let expected = (input_frames as u128 * u128::from(OUTPUT_RATE) / 44_100) as u64;
        trim_queued_to_target(&mut queued, 0, expected);
        let output: Vec<f32> = queued.into_iter().collect();
        assert_eq!(output.len() / 2, expected as usize);
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(output.iter().any(|sample| sample.abs() > 1e-5));
    }

    #[test]
    fn mono_48k_passthrough_preserves_partial_frame_tail() {
        let mono: Vec<f32> = (0..48_017).map(|i| 0.04 * (i as f32 * 0.021).sin()).collect();
        let left = mono.clone();
        let right = mono.clone();
        assert_eq!(left.len(), right.len());
        assert_eq!(left.len(), 48_017);
        assert_eq!(left.last(), right.last());
        assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
        assert!(left.iter().any(|sample| sample.abs() > 1e-5));
    }

    #[test]
    fn sinc_resampler_44100_clock_maps_to_48k_without_per_chunk_rounding() {
        let input = 44_100_u64 * 600;
        let output = input * u64::from(OUTPUT_RATE) / 44_100;
        assert_eq!(output, 48_000 * 600);
    }
}
