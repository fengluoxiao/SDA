//! A fresh complete source reference, measured before playback.
//! The legacy stereo bridge also accepts extracted AV3A programme sources.
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
            || self.input_sample_rate == 0 || !matches!(self.codec.as_str(), "alac" | "mp3" | "av3a")
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
    decoder: Option<Mp3FileDecoder>,
    programme: Option<Box<dyn ProgrammeReference>>,
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
        Ok(Self { decoder:Some(decoder), programme:None, meter:LoudnessMeter::new(48000), path, bytes:before.len(), modified:before.modified().ok(),
            frames:0, codec:if alac { "alac" } else { "mp3" }, input_rate, complete:None })
    }
    pub(crate) fn from_programme(path: &Path, codec: &'static str, programme: Box<dyn ProgrammeReference>) -> Result<Self, String> {
        let before=fs::metadata(path).map_err(|e|e.to_string())?;
        if !before.is_file() || before.len()==0 { return Err("empty programme source".into()); }
        Ok(Self {decoder:None,programme:Some(programme),meter:LoudnessMeter::new(48000),path:path.to_path_buf(),
            bytes:before.len(),modified:before.modified().ok(),frames:0,codec,input_rate:48000,complete:None})
    }
    fn finish_reference(&mut self, measurement: Measurement) -> Result<(), String> {
        let after = fs::metadata(&self.path).map_err(|e| e.to_string())?;
        if after.len() != self.bytes || after.modified().ok() != self.modified {
            return Err("source changed during loudness analysis".into());
        }
        let reference = CompleteReference { measurement, policy: POLICY.into(), complete: true,
            frames: self.frames, sample_rate: 48000, input_sample_rate: self.input_rate, codec: self.codec.into() };
        reference.validate()?;
        self.complete = Some(reference);
        Ok(())
    }
    /// At most 65536 decoded frames per call; cancellation is checked by the host between calls.
    pub fn step(&mut self, max_frames: usize) -> Result<Step, String> {
        if let Some(measurement) = &self.complete {
            return Ok(Step { done: true, frames: self.frames, measurement: Some(measurement.clone()) });
        }
        let budget = max_frames.clamp(4096, 65536);
        if let Some(programme) = &mut self.programme {
            let finished = programme.step(budget)?;
            self.frames = programme.frames();
            if finished {
                let measurement = programme.measurement();
                self.finish_reference(measurement)?;
            }
            return Ok(Step { done: self.complete.is_some(), frames: self.frames, measurement: self.complete.clone() });
        }
        let end = self.frames.saturating_add(budget as u64);
        loop {
            let pcm = self.decoder.as_mut().unwrap().read_interleaved(((end-self.frames) as usize).min(4096))?;
            if pcm.iter().any(|v| !v.is_finite()) { return Err("nonfinite stereo source".into()); }
            if !pcm.is_empty() {
                let planes = vec![pcm.iter().step_by(2).copied().collect(), pcm.iter().skip(1).step_by(2).copied().collect()];
                self.meter.push(&planes);
                self.frames += (pcm.len()/2) as u64;
            }
            if self.decoder.as_ref().unwrap().is_finished() {
                self.finish_reference(self.meter.integrated())?;
                break;
            }
            if self.frames >= end { break; }
            if pcm.is_empty() { return Err("stereo decoder made no progress".into()); }
        }
        Ok(Step { done: self.complete.is_some(), frames: self.frames, measurement: self.complete.clone() })
    }
}

/// A bounded source-only reader supplied by a format-specific module.
/// Keeping this interface decoder-neutral preserves the isolated ALAC/MP3 regression target.
pub(crate) trait ProgrammeReference {
    fn step(&mut self, budget: usize) -> Result<bool, String>;
    fn frames(&self) -> u64;
    fn measurement(&self) -> Measurement;
}
