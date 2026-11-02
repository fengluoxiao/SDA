//! Complete source-only Audio Vivid analysis, separate from the stereo reader's dependencies.
use crate::{balance::Measurement,stereo_reference::{FreshStereoReference,ProgrammeReference}};
use std::{fs,path::Path};
#[cfg(test)] use std::time::SystemTime;

pub fn open(path: impl AsRef<Path>) -> Result<FreshStereoReference,String> {
    let path=path.as_ref();
    FreshStereoReference::from_programme(path,"av3a",Box::new(Av3aReference::open(path)?))
}
pub fn open_with_hrtf(path:impl AsRef<Path>,hrtf:impl AsRef<Path>)->Result<FreshStereoReference,String>{
    let path=path.as_ref();let mut reference=Av3aReference::open(path)?;
    reference.renderer=Some(crate::av3a_reference_renderer::CanonicalReference::new(hrtf.as_ref())?);
    FreshStereoReference::from_programme(path,"av3a",Box::new(reference))
}

/// Bounded source-only AV3A scan: the decoder supplies a role-weighted bed or
/// neutral scene reference. Neither raw object sums nor the listening HRTF are metered.
struct Av3aReference {
    input: std::io::BufReader<fs::File>,
    decoder: sda_core::StreamingDecoder,
    native_frames: u64,
    sample_rate:u32,
    latest: Measurement,
    flushed: bool,
    renderer:Option<crate::av3a_reference_renderer::CanonicalReference>,
}
impl Av3aReference {
    fn open(path: &Path) -> Result<Self, String> {
        Ok(Self { input: std::io::BufReader::new(fs::File::open(path).map_err(|e|e.to_string())?),
            decoder: sda_core::StreamingDecoder::av3a_source_reference(), native_frames: 0, sample_rate:48000,
            latest: Measurement::default(), flushed: false,renderer:None })
    }
}
impl ProgrammeReference for Av3aReference {
    fn frames(&self) -> u64 { (self.native_frames*48000+u64::from(self.sample_rate)/2)/u64::from(self.sample_rate) }
    fn input_sample_rate(&self)->u32{self.sample_rate}
    fn measurement_sample_rate(&self)->u32{self.sample_rate}
    fn measurement(&self) -> Measurement { self.latest.clone() }
    fn rendered_reference(&self)->Option<crate::stereo_reference::RenderedReference>{self.renderer.as_ref().and_then(|r|r.measurement())}
    fn step(&mut self, budget: usize) -> Result<bool, String> {
        use std::io::Read;
        let end = self.frames().saturating_add(budget as u64);
        loop {
            let errors = self.decoder.drain_errors();
            if !errors.is_empty() { return Err(format!("Audio Vivid reference decode failed: {}", errors.join("; "))); }
            if let Some(frame) = self.decoder.next_frame() {
                if self.native_frames==0 && frame.sample_rate>0 {self.sample_rate=frame.sample_rate;}
                if frame.sample_rate == 0 || frame.sample_rate != self.sample_rate || frame.sample_pos != self.native_frames
                    || frame.channels.is_empty() || frame.channels.iter().flatten().any(|x|!x.is_finite()) {
                    return Err("invalid Audio Vivid reference timeline or samples".into());
                }
                if let Some(renderer)=&mut self.renderer{renderer.push(&frame)?;}
                self.latest = frame.reference_loudness.ok_or("Audio Vivid programme reference unavailable")?;
                self.native_frames += frame.channels[0].len() as u64;
                if self.frames() >= end { return Ok(false); }
            } else if self.flushed {
                if self.native_frames == 0 { return Err("no Audio Vivid reference samples".into()); }
                if let Some(renderer)=&mut self.renderer{renderer.finish()?;}
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
        assert_eq!(result.codec,"av3a");assert!(result.complete);assert_eq!(result.sample_rate,48000);assert_eq!(result.input_sample_rate,44100);assert_eq!(result.measurement_sample_rate,44100);assert!(result.frames>0);
        result.validate().unwrap();
        let repeated=reference.step(65536).unwrap().measurement.unwrap();
        assert_eq!(repeated.frames,result.frames,"completed scans do not decode again");
        drop(reference);fs::remove_file(path).unwrap();
    }
}
