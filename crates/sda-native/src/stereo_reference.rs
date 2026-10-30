//! A fresh complete stereo-source reference, measured before playback.
//! The feeder owns this bounded decoder. No PCM or measurement is persisted.
use crate::{balance::{LoudnessMeter, Measurement}, mp3::Mp3FileDecoder};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}, time::SystemTime};

pub const POLICY: &str = "fresh-complete-stereo-reference-v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteReference {
    #[serde(flatten)] pub measurement: Measurement,
    pub policy: String,
    pub complete: bool,
    pub frames: u64,
    pub sample_rate: u32,
    pub input_sample_rate: u32,
    pub codec: String,
}
impl CompleteReference {
    pub fn validate(&self) -> Result<(), String> {
        if self.policy != POLICY || !self.complete || self.frames == 0 || self.sample_rate != 48000
            || self.input_sample_rate == 0 || !matches!(self.codec.as_str(), "alac" | "mp3")
            || self.measurement.integrated_lufs.is_some_and(|v| !v.is_finite())
            || self.measurement.true_peak_dbtp.is_some_and(|v| !v.is_finite())
            || self.measurement.integrated_lufs.is_some() && self.measurement.blocks == 0 {
            return Err("invalid complete stereo source reference".into());
        }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub done: bool,
    pub frames: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measurement: Option<CompleteReference>,
}
pub struct FreshStereoReference {
    decoder: Mp3FileDecoder,
    meter: LoudnessMeter,
    path: PathBuf,
    bytes: u64,
    modified: Option<SystemTime>,
    frames: u64,
    codec: &'static str,
    input_rate: u32,
    complete: Option<CompleteReference>,
}
impl FreshStereoReference {
    pub fn open(path: impl AsRef<Path>, alac: bool) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let before = fs::metadata(&path).map_err(|e| e.to_string())?;
        if !before.is_file() || before.len() == 0 { return Err("empty stereo source".into()); }
        let decoder = if alac { Mp3FileDecoder::open_alac(&path)? } else { Mp3FileDecoder::open(&path)? };
        let input_rate = decoder.input_rate();
        Ok(Self { decoder, meter: LoudnessMeter::new(48000), path, bytes: before.len(), modified: before.modified().ok(),
            frames: 0, codec: if alac { "alac" } else { "mp3" }, input_rate, complete: None })
    }
    /// At most 65536 decoded frames per call; cancellation is checked by the host between calls.
    pub fn step(&mut self, max_frames: usize) -> Result<Step, String> {
        if let Some(measurement) = &self.complete {
            return Ok(Step { done: true, frames: self.frames, measurement: Some(measurement.clone()) });
        }
        let budget = max_frames.clamp(4096, 65536);
        let end = self.frames.saturating_add(budget as u64);
        loop {
            let pcm = self.decoder.read_interleaved(((end-self.frames) as usize).min(4096))?;
            if pcm.iter().any(|v| !v.is_finite()) { return Err("nonfinite stereo source".into()); }
            if !pcm.is_empty() {
                let planes = vec![pcm.iter().step_by(2).copied().collect(), pcm.iter().skip(1).step_by(2).copied().collect()];
                self.meter.push(&planes);
                self.frames += (pcm.len()/2) as u64;
            }
            if self.decoder.is_finished() {
                let after = fs::metadata(&self.path).map_err(|e| e.to_string())?;
                if after.len() != self.bytes || after.modified().ok() != self.modified {
                    return Err("stereo source changed during loudness analysis".into());
                }
                let measurement = CompleteReference { measurement: self.meter.integrated(), policy: POLICY.into(), complete: true,
                    frames: self.frames, sample_rate: 48000, input_sample_rate: self.input_rate, codec: self.codec.into() };
                measurement.validate()?;
                self.complete = Some(measurement);
                break;
            }
            if self.frames >= end { break; }
            if pcm.is_empty() { return Err("stereo decoder made no progress".into()); }
        }
        Ok(Step { done: self.complete.is_some(), frames: self.frames, measurement: self.complete.clone() })
    }
}
