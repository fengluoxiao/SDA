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

/// Complete analysis with a bounded two-frame handoff: decoding and metering
/// run concurrently without retaining any programme PCM or calculated disk data.
struct ReferenceFrame { frame:sda_core::FrameData, scene:bool }
struct Av3aReference {
    receiver: Option<std::sync::mpsc::Receiver<Result<Option<ReferenceFrame>, String>>>,
    worker: Option<std::thread::JoinHandle<()>>,
    native_frames: u64,
    sample_rate: u32,
    latest: Measurement,
    speakers: sda_core::loudness::SpeakerReference,
    scene: Option<sda_core::scene_reference::SceneMeter>,
    finished: bool,
    use_speakers: bool,
    renderer: Option<crate::av3a_reference_renderer::CanonicalReference>,
}
impl Av3aReference {
    fn open(path: &Path) -> Result<Self, String> {
        // Open before spawning so an invalid input fails synchronously.
        let input = fs::File::open(path).map_err(|e| e.to_string())?;
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        let worker = std::thread::Builder::new().name("sda-vivid-reference".into())
            .stack_size(4 * 1024 * 1024).spawn(move || {
                let result = decode_reference(input, &sender);
                // Dropping the receiver cancels a blocked send. No detached job
                // can outlive its reference owner, including on source changes.
                let _ = sender.send(result.map(|()| None));
            }).map_err(|e| e.to_string())?;
        Ok(Self { receiver: Some(receiver), worker: Some(worker), native_frames: 0,
            sample_rate: 48000, latest: Measurement::default(), speakers: Default::default(), scene: None, finished: false, use_speakers: false, renderer: None })
    }
}
fn decode_reference(input: fs::File, sender: &std::sync::mpsc::SyncSender<Result<Option<ReferenceFrame>,String>>) -> Result<(),String> {
    use std::io::Read;
    let mut input = std::io::BufReader::new(input);
    let mut decoder = sda_core::StreamingDecoder::av3a_pipelined_source_reference();
    let mut flushed = false;
    loop {
        let errors = decoder.drain_errors();
        if !errors.is_empty() { return Err(format!("Audio Vivid reference decode failed: {}", errors.join("; "))); }
        if let Some(frame) = decoder.next_frame() {
            let scene=decoder.av3a_reference_is_scene();
            if sender.send(Ok(Some(ReferenceFrame{frame,scene}))).is_err() { return Ok(()); }
        } else if flushed { return Ok(()); }
        else {
            let mut bytes = [0u8;4096];
            let n = input.read(&mut bytes).map_err(|e|e.to_string())?;
            if n == 0 { decoder.flush(); flushed = true; }
            else { decoder.push(&bytes[..n])?; }
        }
    }
}
impl Drop for Av3aReference {
    fn drop(&mut self) {
        drop(self.receiver.take());
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}
impl ProgrammeReference for Av3aReference {
    fn frames(&self) -> u64 { (self.native_frames*48000+u64::from(self.sample_rate)/2)/u64::from(self.sample_rate) }
    fn input_sample_rate(&self)->u32{self.sample_rate}
    fn measurement_sample_rate(&self)->u32{self.sample_rate}
    fn measurement(&self) -> Measurement { if self.use_speakers { self.speakers.measurement().unwrap_or_default() } else { self.latest.clone() } }
    fn rendered_reference(&self)->Option<crate::stereo_reference::RenderedReference>{self.renderer.as_ref().and_then(|r|r.measurement())}
    fn step(&mut self, budget: usize) -> Result<bool, String> {
        if self.finished { return Ok(true); }
        let end = self.frames().saturating_add(budget as u64);
        loop {
            match self.receiver.as_ref().ok_or("Audio Vivid reference was closed")?.recv().map_err(|_|"Audio Vivid reference worker stopped unexpectedly")?? {
                Some(reference) => {
                    let mut frame=reference.frame;
                    if self.native_frames==0 && frame.sample_rate>0 {self.sample_rate=frame.sample_rate;}
                    if frame.sample_rate == 0 || frame.sample_rate != self.sample_rate || frame.sample_pos != self.native_frames
                        || frame.channels.is_empty()
                        || frame.channels.iter().any(|c| c.is_empty() || c.len()!=frame.channels[0].len()) {
                        return Err("invalid Audio Vivid reference timeline or samples".into());
                    }
                    // This private handoff receives only AV3A core frames. The
                    // producer rejects nonfinite synthesis samples before native-rate
                    // normalization, and scene routing validates its gained outputs.
                    // Keep that boundary check rather than repeat the PCM scan here.
                    debug_assert!(frame.channels.iter().flatten().all(|x|x.is_finite()));
                    if let Some(renderer)=&mut self.renderer{renderer.push(&frame)?;}
                    if let Some(measurement)=frame.reference_loudness.take() {
                        self.latest=measurement;self.use_speakers=false;
                    } else if reference.scene {
                        // Metadata/events stay on the original sample timeline.
                        // Only the owning execution thread changes; the scene
                        // mixer, ramps and BS.1770 policy are unchanged.
                        self.latest=self.scene.get_or_insert_with(||sda_core::scene_reference::SceneMeter::with_sample_rate(frame.sample_rate)).measure(&frame);
                        self.use_speakers=false;
                    } else {
                        // Programme references are published only at EOF. Accumulate
                        // every sample, then apply the exact gates once instead of
                        // repeatedly re-gating the entire history on each frame.
                        self.speakers.push(&frame.channels,&frame.labels,frame.sample_rate)
                            .ok_or("Audio Vivid programme reference unavailable")?;
                        self.use_speakers=true;
                    }
                    self.native_frames += frame.channels[0].len() as u64;
                    if self.frames() >= end { return Ok(false); }
                }
                None => {
                    if self.native_frames == 0 { return Err("no Audio Vivid reference samples".into()); }
                    if let Some(renderer)=&mut self.renderer{renderer.finish()?;}
                    self.finished = true;
                    return Ok(true);
                }
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
    #[test]
    fn pipelined_reference_matches_serial_source_meter_and_joins_on_cancel() {
        let bytes=include_bytes!("../../../packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a").repeat(96);
        let path=std::env::temp_dir().join(format!("sda-pipelined-reference-{}-{}.av3a",std::process::id(),SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()));
        fs::write(&path,&bytes).unwrap();
        let mut decoder=sda_core::StreamingDecoder::av3a_source_reference();
        let mut serial=None;let mut frames=0;
        for chunk in bytes.chunks(4096) {
            decoder.push(chunk).unwrap();
            while let Some(frame)=decoder.next_frame(){frames+=frame.channels[0].len() as u64;serial=frame.reference_loudness;}
            assert!(decoder.drain_errors().is_empty());
        }
        decoder.flush();while let Some(frame)=decoder.next_frame(){frames+=frame.channels[0].len() as u64;serial=frame.reference_loudness;}
        let serial=serial.unwrap();let mut parallel=Av3aReference::open(&path).unwrap();
        while !parallel.step(4096).unwrap() {}
        let actual=parallel.measurement();
        assert_eq!(parallel.native_frames,frames);assert_eq!(actual.blocks,serial.blocks);
        assert_eq!(actual.integrated_lufs,serial.integrated_lufs);
        assert_eq!(actual.true_peak_dbtp,serial.true_peak_dbtp);
        drop(parallel);
        // Stop with the producer still decoding/backpressured, repeatedly. Drop
        // must release the receiver and join, rather than leak a blocked worker.
        for _ in 0..8 {let mut reference=Av3aReference::open(&path).unwrap();assert!(!reference.step(1).unwrap());drop(reference);}
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn pipelined_reference_reports_empty_input_and_closes_its_worker() {
        let path=std::env::temp_dir().join(format!("sda-empty-reference-{}-{}.av3a",std::process::id(),SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()));
        fs::write(&path,[]).unwrap();let mut reference=Av3aReference::open(&path).unwrap();
        assert!(reference.step(4096).unwrap_err().contains("no Audio Vivid reference samples"));
        drop(reference);fs::remove_file(path).unwrap();
    }

}
