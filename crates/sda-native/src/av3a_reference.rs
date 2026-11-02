//! Complete source-only Audio Vivid analysis, separate from the stereo reader's dependencies.
use crate::{balance::Measurement,stereo_reference::{FreshStereoReference,ProgrammeReference}};
use std::{fs,path::Path};
#[cfg(test)] use std::time::SystemTime;

pub fn open(path: impl AsRef<Path>) -> Result<FreshStereoReference,String> {
    let path=path.as_ref();
    FreshStereoReference::from_programme(path,"av3a",Box::new(Av3aReference::open(path)?))
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
}
impl ProgrammeReference for Av3aReference {
    fn frames(&self) -> u64 { self.frames }
    fn measurement(&self) -> Measurement { self.latest.clone() }
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
        let mut reference=open(&path).unwrap();
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
