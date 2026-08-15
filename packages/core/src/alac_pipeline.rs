//! ALAC packet pipeline. MP4 demuxing remains in TypeScript; this module only
//! receives ALAC access units plus the complete MP4 `alac` configuration atom.

use std::collections::VecDeque;

use alac::{Decoder, StreamInfo};

use crate::{FrameData, Pipeline};

pub struct AlacPipeline {
    decoder: Decoder,
    sample_rate: u32,
    channels: usize,
    max_samples: usize,
    total_samples: u64,
}

impl AlacPipeline {
    pub fn from_cookie(cookie: &[u8]) -> Result<Self, String> {
        let info = StreamInfo::from_cookie(cookie).map_err(|error| error.to_string())?;
        let channels = usize::from(info.channels());
        let bit_depth = info.bit_depth();
        if !(1..=2).contains(&channels) {
            return Err(format!("ALAC channel count {channels} is not supported yet (mono and stereo only)"));
        }
        if !(1..=32).contains(&bit_depth) {
            return Err(format!("invalid ALAC bit depth {bit_depth}"));
        }
        let max_samples = info.max_samples_per_packet() as usize;
        if max_samples == 0 {
            return Err("ALAC packet size is zero".to_string());
        }
        Ok(Self {
            decoder: Decoder::new(info.clone()),
            sample_rate: info.sample_rate(),
            channels,
            max_samples,
            total_samples: 0,
        })
    }

    fn labels(&self) -> Vec<String> {
        match self.channels {
            1 => vec!["Mono".to_string()],
            2 => vec!["L".to_string(), "R".to_string()],
            _ => unreachable!(),
        }
    }
}

impl Pipeline for AlacPipeline {
    fn codec_name(&self) -> &'static str {
        "alac"
    }

    fn push(&mut self, data: &[u8], out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        let mut interleaved = vec![0_i32; self.max_samples];
        let samples = match self.decoder.decode_packet(data, &mut interleaved) {
            Ok(samples) => samples,
            Err(error) => {
                errors.push(format!("ALAC packet rejected: {error}"));
                return;
            }
        };
        if samples.len() % self.channels != 0 {
            errors.push("ALAC packet yielded an incomplete interleaved frame".to_string());
            return;
        }
        let frames = samples.len() / self.channels;
        if frames == 0 {
            return;
        }
        // `alac::Decoder` expands i32 output to a signed 32-bit full-scale
        // representation, independent of the source bit depth.
        let scale = 2_147_483_648_f32;
        let mut channels = (0..self.channels)
            .map(|_| Vec::with_capacity(frames))
            .collect::<Vec<_>>();
        for frame in samples.chunks_exact(self.channels) {
            for (index, sample) in frame.iter().enumerate() {
                channels[index].push((*sample as f32) / scale);
            }
        }
        let sample_pos = self.total_samples;
        self.total_samples += frames as u64;
        let labels = self.labels();
        out.push_back(FrameData {
            codec: "alac",
            sample_rate: self.sample_rate,
            sample_pos,
            channels,
            raw_bed_labels: labels.clone(),
            labels,
            events: Vec::new(),
            object_channels: Vec::new(),
            program_loudness: None,
            ramp_duration: 0,
        });
    }

    fn reset(&mut self) {
        self.total_samples = 0;
    }
}
