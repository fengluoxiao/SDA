//! Offline audit tool: real source PCM/metadata and the ordinary programme controller.
//! Runs offline only and never participates in application playback.
use sda_core::{loudness::ReferenceLoudness, scene_reference::SceneMeter, FrameData};
use sda_native::balance::{PendingFrame, VolumeBalance};
use sda_native_renderer::Command;
use std::{
    fs,
    io::{BufRead, Read, Write},
    path::Path,
};
struct Audit {
    balance: VolumeBalance,
    meter: Option<SceneMeter>,
    last: Option<ReferenceLoudness>,
    pending: Vec<PendingFrame>,
    started: bool,
    origin: Option<u64>,
    end: u64,
    frames: usize,
    gain_events: Vec<serde_json::Value>,
    hash: u64,
    objects: usize,
    beds: usize,
    fallback: bool,
}
impl Audit {
    fn new() -> Self {
        let mut balance = VolumeBalance::default();
        balance.enabled = true;
        Self {
            balance,
            meter: None,
            last: None,
            pending: vec![],
            started: false,
            origin: None,
            end: 0,
            frames: 0,
            gain_events: vec![],
            hash: 0xcbf29ce484222325,
            objects: 0,
            beds: 0,
            fallback: true,
        }
    }
    fn command(&mut self, c: Command) {
        if let Command::SetProgramGain { gain, at } = c {
            self.gain_events
                .push(serde_json::json!({"sample":at.unwrap_or(0),"gain":gain}));
        }
    }
    fn drain(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for p in pending {
            for c in self.balance.route(&p) {
                self.command(c);
            }
        }
    }
    fn frame(&mut self, mut f: FrameData) {
        self.frames += 1;
        self.origin.get_or_insert(f.sample_pos);
        self.end = f.sample_pos + f.channels[0].len() as u64;
        let objects = f.object_channels.len().max(
            f.labels
                .iter()
                .filter(|label| label.starts_with("Obj_"))
                .count(),
        );
        self.objects = self.objects.max(objects);
        self.beds = self.beds.max(f.channels.len() - objects);
        for x in f.channels.iter().flatten() {
            for b in x.to_le_bytes() {
                self.hash ^= u64::from(b);
                self.hash = self.hash.wrapping_mul(0x100000001b3);
            }
        }
        if self.fallback
            && f.reference_loudness.is_none()
            && (!f.object_channels.is_empty()
                || f.labels.iter().any(|label| label.starts_with("Obj_")))
        {
            let m = self
                .meter
                .get_or_insert_with(|| SceneMeter::with_sample_rate(f.sample_rate));
            f.reference_loudness = Some(m.measure(&f));
        }
        if let Some(reference) = &f.reference_loudness {
            self.last = Some(reference.clone());
        }
        let rate = f.sample_rate;
        let pending = self.balance.measure(f, None);
        self.pending.push(pending);
        if !self.started
            && (self.balance.startup_measurement_ready()
                || self.end - self.origin.unwrap() >= u64::from(rate) * 7)
        {
            if let Some(c) = self.balance.prime_av3a_startup(self.origin.unwrap(), false) {
                self.command(c);
            }
            self.started = true;
        }
        if self.started {
            self.drain();
        }
    }
    fn finish(mut self, output: &Path) {
        self.balance.finish();
        if !self.started {
            if let Some(c) = self
                .balance
                .prime_av3a_startup(self.origin.unwrap_or(0), true)
            {
                self.command(c);
            }
        }
        self.drain();
        let measurement = self
            .last
            .clone()
            .or_else(|| self.balance.complete_measurement());
        fs::write(
            output.join("programme-timeline.json"),
            serde_json::to_vec_pretty(&self.gain_events).unwrap(),
        )
        .unwrap();
        let report = serde_json::json!({"frames":self.frames,"samples":self.end,"sourceFnv":format!("{:016x}",self.hash),
   "objects":self.objects,"beds":self.beds,"measurement":measurement,"eligible":self.balance.eligible,
   "finalTargetDb":self.balance.gain_db,"programmeGainEvents":self.gain_events.len(),
   "reference":"ordinary shared scene/speaker measurement and volume controller; no HRTF or spatial enhancement"});
        fs::write(
            output.join("programme-reference.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("{report}");
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let mode = &args[1];
    let input = Path::new(&args[2]);
    let codec = &args[3];
    let codec: &'static str = match codec.as_str() {
        "av3a" => "av3a",
        "ac4" => "ac4",
        "eac3" => "eac3",
        _ => return Err("expected codec av3a, ac4 or eac3".into()),
    };
    let output = Path::new(&args[4]);
    fs::create_dir_all(output)?;
    let mut audit = Audit::new();
    if mode == "frames" {
        let mut pcm = std::io::BufReader::new(fs::File::open(input.join("sources.pcm"))?);
        for line in std::io::BufReader::new(fs::File::open(input.join("frames.jsonl"))?).lines() {
            let row: serde_json::Value = serde_json::from_str(&line?)?;
            let n = row["samples"].as_u64().unwrap() as usize;
            let labels: Vec<String> = serde_json::from_value(row["labels"].clone())?;
            let mut channels = vec![];
            for _ in &labels {
                let mut bytes = vec![0; n * 4];
                pcm.read_exact(&mut bytes)?;
                channels.push(
                    bytes
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                        .collect(),
                );
            }
            audit.frame(FrameData {
                reference_loudness: None,
                compatibility_bed: None,
                codec,
                sample_rate: 48000,
                sample_pos: row["start"].as_u64().unwrap(),
                channels,
                labels,
                raw_bed_labels: vec![],
                events: serde_json::from_value(row["events"].clone())?,
                object_channels: serde_json::from_value(row["objectChannels"].clone())?,
                program_loudness: None,
                ramp_duration: 0,
            });
        }
    } else if mode == "raw" {
        audit.fallback = false;
        let bytes = fs::read(input)?;
        let mut decoder = sda_core::StreamingDecoder::new(codec)?;
        let mut pcm = std::io::BufWriter::new(fs::File::create(output.join("sources.pcm"))?);
        let mut meta = std::io::BufWriter::new(fs::File::create(output.join("frames.jsonl"))?);
        let mut drain = |decoder: &mut sda_core::StreamingDecoder,
                         audit: &mut Audit|
         -> Result<(), Box<dyn std::error::Error>> {
            let errors = decoder.drain_errors();
            if !errors.is_empty() {
                return Err(format!("decode errors {errors:?}").into());
            }
            while let Some(f) = decoder.next_frame() {
                for ch in &f.channels {
                    for x in ch {
                        pcm.write_all(&x.to_le_bytes())?;
                    }
                }
                writeln!(
                    meta,
                    "{}",
                    serde_json::json!({"start":f.sample_pos,"samples":f.channels[0].len(),"labels":f.labels,
     "events":f.events,"objectChannels":f.object_channels,"rawBedLabels":f.raw_bed_labels})
                )?;
                audit.frame(f);
            }
            Ok(())
        };
        for chunk in bytes.chunks(8192) {
            decoder.push(chunk)?;
            drain(&mut decoder, &mut audit)?;
        }
        decoder.flush();
        drain(&mut decoder, &mut audit)?;
    } else {
        return Err("mode must be frames or raw".into());
    }
    if audit.frames == 0 {
        return Err("no decoded scene frames".into());
    }
    audit.finish(output);
    Ok(())
}
