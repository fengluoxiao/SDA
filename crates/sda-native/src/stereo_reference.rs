//! A fresh complete source reference, measured before playback.
//! The legacy stereo bridge also accepts extracted AV3A programme sources.
//! The feeder owns this bounded decoder. No PCM or measurement is persisted.
use crate::{balance::{LoudnessMeter, Measurement}, mp3::Mp3FileDecoder};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}, time::SystemTime};

pub const POLICY: &str = "fresh-complete-stereo-reference-v1";
pub const RENDER_REFERENCE_PROFILE:&str="sda-hrtfhq-canonical-bed-v1";
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RenderedReference {
 pub profile:String,pub layout:String,pub hrtf_sha256:String,pub sample_rate:u32,
 pub measurement:Measurement,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteReference {
    #[serde(flatten)] pub measurement: Measurement,
    pub policy: String,
    pub complete: bool,
    pub frames: u64,
    pub sample_rate: u32,
    pub input_sample_rate: u32,
    #[serde(default="default_reference_rate")]
    pub measurement_sample_rate:u32,
    pub codec: String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub rendered_reference:Option<RenderedReference>,
}
fn default_reference_rate()->u32{48000}
impl CompleteReference {
    pub fn validate(&self) -> Result<(), String> {
        if self.policy != POLICY || !self.complete || self.frames == 0 || self.sample_rate != 48000
            || self.input_sample_rate == 0 || !(8000..=384000).contains(&self.measurement_sample_rate) || !matches!(self.codec.as_str(), "alac" | "mp3" | "av3a")
            || self.measurement.integrated_lufs.is_some_and(|v| !v.is_finite())
            || self.measurement.true_peak_dbtp.is_some_and(|v| !v.is_finite())
            || self.measurement.integrated_lufs.is_some() && self.measurement.blocks == 0 {
            return Err("invalid complete stereo source reference".into());
        }
        if let Some(r)=&self.rendered_reference {
            if self.codec!="av3a"||r.profile!=RENDER_REFERENCE_PROFILE||r.layout!="7.1.4"||r.sample_rate!=self.measurement_sample_rate
                ||r.hrtf_sha256.len()!=64||!r.hrtf_sha256.bytes().all(|c|c.is_ascii_hexdigit()&&!c.is_ascii_uppercase())
                ||r.measurement.integrated_lufs.is_some_and(|x|!x.is_finite())||r.measurement.true_peak_dbtp.is_some_and(|x|!x.is_finite())
                ||r.measurement.integrated_lufs.is_some()&&r.measurement.blocks==0 {return Err("invalid canonical programme reference".into());}
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
    /// Timings only: no source path, song identity, PCM or persisted analysis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<serde_json::Value>,
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
        let input_sample_rate=self.programme.as_ref().map_or(self.input_rate,|p|p.input_sample_rate());
        let measurement_sample_rate=self.programme.as_ref().map_or(48000,|p|p.measurement_sample_rate());
        let reference = CompleteReference { measurement, policy: POLICY.into(), complete: true,
            frames: self.frames, sample_rate: 48000, input_sample_rate, measurement_sample_rate, rendered_reference:self.programme.as_ref().and_then(|p|p.rendered_reference()), codec: self.codec.into() };
        reference.validate()?;
        self.complete = Some(reference);
        Ok(())
    }
    /// At most 65536 decoded frames per call; cancellation is checked by the host between calls.
    pub fn step(&mut self, max_frames: usize) -> Result<Step, String> {
        if let Some(measurement) = &self.complete {
            return Ok(Step { done: true, frames: self.frames, measurement: Some(measurement.clone()), diagnostics: self.programme.as_ref().and_then(|p|p.diagnostics()) });
        }
        let budget = max_frames.clamp(4096, 65536);
        if let Some(programme) = &mut self.programme {
            let finished = programme.step(budget)?;
            self.frames = programme.frames();
            if finished {
                let measurement = programme.measurement();
                self.finish_reference(measurement)?;
            }
            return Ok(Step { done: self.complete.is_some(), frames: self.frames, measurement: self.complete.clone(), diagnostics: if self.complete.is_some() { self.programme.as_ref().and_then(|p|p.diagnostics()) } else { None } });
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
        Ok(Step { done: self.complete.is_some(), frames: self.frames, measurement: self.complete.clone(), diagnostics: None })
    }
}

/// A bounded source-only reader supplied by a format-specific module.
/// Keeping this interface decoder-neutral preserves the isolated ALAC/MP3 regression target.
pub(crate) trait ProgrammeReference {
    fn diagnostics(&self)->Option<serde_json::Value>{None}
    fn step(&mut self, budget: usize) -> Result<bool, String>;
    fn frames(&self) -> u64;
    fn measurement(&self) -> Measurement;
    fn input_sample_rate(&self)->u32{48000}
    fn measurement_sample_rate(&self)->u32{48000}
    fn rendered_reference(&self)->Option<RenderedReference>{None}
}
