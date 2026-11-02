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
    av3a: Option<Av3aReference>,
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
        Self::open_source(path.as_ref(), alac, false)
    }
    pub fn open_av3a(path: impl AsRef<Path>) -> Result<Self, String> {
        Self::open_source(path.as_ref(), false, true)
    }
    fn open_source(path: &Path, alac: bool, is_av3a: bool) -> Result<Self, String> {
        let path = path.to_path_buf();
        let before = fs::metadata(&path).map_err(|e| e.to_string())?;
        if !before.is_file() || before.len() == 0 { return Err("empty stereo source".into()); }
        let decoder = if is_av3a { None } else { Some(if alac { Mp3FileDecoder::open_alac(&path)? } else { Mp3FileDecoder::open(&path)? }) };
        let input_rate = decoder.as_ref().map_or(48000, Mp3FileDecoder::input_rate);
        let av3a = if is_av3a { Some(Av3aReference::open(&path)?) } else { None };
        Ok(Self { decoder, av3a, meter: LoudnessMeter::new(48000), path, bytes: before.len(), modified: before.modified().ok(),
            frames: 0, codec: if is_av3a { "av3a" } else if alac { "alac" } else { "mp3" }, input_rate, complete: None })
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
        if let Some(av3a) = &mut self.av3a {
            let finished = av3a.step(budget)?;
            self.frames = av3a.frames;
            if finished {
                let measurement = av3a.latest.clone();
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

/// Bounded source-only AV3A scan: the decoder supplies a role-weighted bed or
/// neutral scene reference. Neither raw object sums nor the listening HRTF are metered.
struct Av3aReference {
    input: std::io::BufReader<fs::File>,
    decoder: sda_core::StreamingDecoder,
    frames: u64,
    latest: Measurement,
    flushed: bool,
}
impl Av3aReference {
    fn open(path: &Path) -> Result<Self, String> {
        Ok(Self { input: std::io::BufReader::new(fs::File::open(path).map_err(|e|e.to_string())?),
            decoder: sda_core::StreamingDecoder::new("av3a")?, frames: 0,
            latest: Measurement::default(), flushed: false })
    }
    fn step(&mut self, budget: usize) -> Result<bool, String> {
        use std::io::Read;
        let end = self.frames.saturating_add(budget as u64);
        loop {
            let errors = self.decoder.drain_errors();
            if !errors.is_empty() { return Err(format!("Audio Vivid reference decode failed: {}", errors.join("; "))); }
            if let Some(frame) = self.decoder.next_frame() {
                if frame.sample_rate != 48000 || frame.sample_pos != self.frames
                    || frame.channels.is_empty() || frame.channels.iter().flatten().any(|x|!x.is_finite()) {
                    return Err("invalid Audio Vivid reference timeline or samples".into());
                }
                self.latest = frame.reference_loudness.ok_or("Audio Vivid programme reference unavailable")?;
                self.frames += frame.channels[0].len() as u64;
                if self.frames >= end { return Ok(false); }
            } else if self.flushed {
                if self.frames == 0 { return Err("no Audio Vivid reference samples".into()); }
                return Ok(true);
            } else {
                let mut bytes = [0u8;4096];
                let n = self.input.read(&mut bytes).map_err(|e|e.to_string())?;
                if n == 0 { self.decoder.flush(); self.flushed = true; }
                else { self.decoder.push(&bytes[..n])?; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_av3a_reference_reads_import_file_without_assuming_its_extension() {
        let bytes=include_bytes!("../../../packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a");
        let path=std::env::temp_dir().join(format!("sda-vivid-reference-{}-{}.mhas",std::process::id(),SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()));
        fs::write(&path,bytes).unwrap();
        let mut reference=FreshStereoReference::open_av3a(&path).unwrap();
        let mut result=None;
        for _ in 0..10 {
            let step=reference.step(4096).unwrap();
            if step.done {result=step.measurement;break;}
        }
        let result=result.expect("complete short AV3A reference");
        assert_eq!(result.codec,"av3a");assert!(result.complete);assert_eq!(result.sample_rate,48000);assert!(result.frames>0);
        result.validate().unwrap();
        let repeated=reference.step(65536).unwrap().measurement.unwrap();
        assert_eq!(repeated.frames,result.frames,"completed scans do not decode again");
        drop(reference);fs::remove_file(path).unwrap();
    }
}
