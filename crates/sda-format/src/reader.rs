use crate::{
    metadata::{self, exact, Metadata, PcmSource},
    mp4::AacSource,
};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::{collections::VecDeque, fs::File, path::Path};
enum Source {
    Pcm {
        file: File,
        format: PcmSource,
        at: u64,
    },
    Aac(AacSource),
}
impl Source {
    fn read(&mut self, max: usize) -> Result<Vec<Vec<f32>>, String> {
        match self {
            Self::Pcm { file, format, at } => {
                let total = format.bytes / (format.channels as u64 * 3);
                let count = ((total - *at) as usize).min(max.clamp(1, 4096));
                let b = exact(
                    file,
                    format.offset + *at * format.channels as u64 * 3,
                    count * format.channels * 3,
                )?;
                let mut out = vec![vec![0.; count]; format.channels];
                for i in 0..count {
                    for ch in 0..format.channels {
                        let k = (i * format.channels + ch) * 3;
                        let sign = if b[k + 2] & 128 != 0 { 255 } else { 0 };
                        out[ch][i] =
                            i32::from_le_bytes([b[k], b[k + 1], b[k + 2], sign]) as f32 / 8388608.;
                    }
                }
                *at += count as u64;
                Ok(out)
            }
            Self::Aac(aac) => aac.read(max),
        }
    }
}
pub struct BedFile {
    source: Source,
    pub metadata: Metadata,
    pub input_rate: u32,
    pub transport: Option<serde_json::Value>,
    resampler: Option<SincFixedIn<f32>>,
    delay: usize,
    queue: Vec<VecDeque<f32>>,
    generated: u64,
    delivered: u64,
    eof: bool,
}
impl BedFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        Self::open_internal(path.as_ref(), false)
    }
    pub fn open_master_wave(path: impl AsRef<Path>) -> Result<Self, String> {
        Self::open_internal(path.as_ref(), true)
    }
    fn open_internal(path: &Path, allow_wave: bool) -> Result<Self, String> {
        let mut probe = File::open(path).map_err(|e| e.to_string())?;
        let header = exact(&mut probe, 0, 12)?;
        let (source, metadata, input_rate, transport) = if &header[..4] == b"SDAR"
            || allow_wave && [b"RIFF".as_slice(), b"RF64".as_slice()].contains(&&header[..4])
        {
            let (file, format, metadata) = metadata::read_master(path, allow_wave)?;
            let rate = format.rate;
            (
                Source::Pcm {
                    file,
                    format,
                    at: 0,
                },
                metadata,
                rate,
                None,
            )
        } else {
            let mut aac = AacSource::open(path)?;
            let metadata = std::mem::take(&mut aac.metadata);
            let rate = aac.sample_rate;
            let transport = Some(aac.transport.clone());
            (Source::Aac(aac), metadata, rate, transport)
        };
        let channels = metadata.channels();
        let resampler = if input_rate == 48000 {
            None
        } else {
            Some(
                SincFixedIn::<f32>::new(
                    48000. / input_rate as f64,
                    1.,
                    SincInterpolationParameters {
                        sinc_len: 128,
                        f_cutoff: 0.95,
                        oversampling_factor: 128,
                        interpolation: SincInterpolationType::Cubic,
                        window: WindowFunction::BlackmanHarris2,
                    },
                    1024,
                    channels,
                )
                .map_err(|e| e.to_string())?,
            )
        }; // SincFixedIn retains lookahead by returning fewer initial frames. Its
           // reported delay is processing latency, not leading PCM to discard.
        let delay = 0;
        if std::env::var_os("SDA_FORMAT_DEBUG").is_some() {
            eprintln!("SDAR SRC rate={input_rate} delay={delay}");
        }
        Ok(Self {
            source,
            metadata,
            input_rate,
            transport,
            resampler,
            delay,
            queue: (0..channels).map(|_| VecDeque::new()).collect(),
            generated: 0,
            delivered: 0,
            eof: false,
        })
    }
    pub fn is_sda(path: impl AsRef<Path>) -> bool {
        let Ok(mut file) = File::open(path) else {
            return false;
        };
        let Ok(h) = exact(&mut file, 0, 12) else {
            return false;
        };
        (&h[..4] == b"SDAR" && &h[8..12] == b"BCH1")
            || (&h[4..8] == b"ftyp" && &h[8..12] == b"sda1")
    }
    pub fn seek(&mut self, target: u64) -> Result<(), String> {
        let target = target.min(self.frames());
        let base = match &mut self.source {
            Source::Pcm { at, .. } => {
                *at = target;
                target
            }
            Source::Aac(aac) => aac.seek_base(target)?,
        };
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
        for q in &mut self.queue {
            q.clear();
        }
        self.generated = base;
        self.delivered = base;
        self.delay = 0;
        self.eof = false;
        while self.position() < target {
            let remaining = (target - self.position()).min(4096) as usize;
            let data = self.read(remaining)?;
            if data[0].is_empty() {
                return Err("SDAR seek ended early".into());
            }
        }
        Ok(())
    }
    pub fn frames(&self) -> u64 {
        self.metadata.master_frames()
    }
    pub fn position(&self) -> u64 {
        self.delivered
    }
    pub fn finished(&self) -> bool {
        self.delivered >= self.frames()
    }
    pub fn read(&mut self, max_frames: usize) -> Result<Vec<Vec<f32>>, String> {
        let count = ((self.frames() - self.delivered) as usize).min(max_frames.clamp(1, 4096));
        if count == 0 {
            return Ok(vec![vec![]; self.metadata.channels()]);
        }
        while self.queue[0].len() < count && self.generated < self.frames() {
            let needed = self
                .resampler
                .as_ref()
                .map_or(4096, Resampler::input_frames_next);
            let mut input = if self.eof {
                vec![vec![]; self.metadata.channels()]
            } else {
                self.source.read(needed)?
            };
            let got = input[0].len();
            if got < needed {
                self.eof = true;
            }
            let output = if let Some(resampler) = &mut self.resampler {
                for channel in &mut input {
                    channel.resize(needed, 0.);
                }
                resampler
                    .process(&input, None)
                    .map_err(|e| format!("SDAR resampling: {e}"))?
            } else {
                if got == 0 {
                    return Err("SDAR audio ended before its declared duration".into());
                }
                input
            };
            let skip = self.delay.min(output[0].len());
            self.delay -= skip;
            let available = output[0].len() - skip;
            let keep = (available as u64).min(self.frames() - self.generated) as usize;
            if keep == 0 && self.eof && self.delay == 0 {
                return Err("SDAR resampler did not produce a tail".into());
            }
            for (ch, channel) in output.iter().enumerate() {
                self.queue[ch].extend(channel[skip..skip + keep].iter().copied());
            }
            self.generated += keep as u64;
        }
        if self.queue[0].len() < count {
            return Err("Incomplete SDAR audio frame".into());
        }
        self.delivered += count as u64;
        Ok(self
            .queue
            .iter_mut()
            .map(|q| q.drain(..count).collect())
            .collect())
    }
}
