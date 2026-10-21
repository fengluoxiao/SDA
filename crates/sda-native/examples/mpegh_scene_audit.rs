//! Offline audit of the ordinary MPEG-H capture, reference meter and gain commands.
//! Never used by application playback; accepts demuxed MHAS input.
use sda_native::{balance::VolumeBalance, mpegh::MpeghDecoder, Command};
use std::{fs, io::{BufWriter, Write}, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len()!=3 { return Err("usage: mpegh_scene_audit input.mhas output-directory".into()); }
    let output=Path::new(&args[2]);fs::create_dir_all(output)?;
    let mut pcm=BufWriter::new(fs::File::create(output.join("sources.pcm"))?);
    let mut meta=BufWriter::new(fs::File::create(output.join("frames.jsonl"))?);
    let mut reference_pcm=BufWriter::new(fs::File::create(output.join("reference-stereo.f32"))?);
    let mut measurements=BufWriter::new(fs::File::create(output.join("reference-measurements.jsonl"))?);
    let mut decoder=MpeghDecoder::new()?;
    let mut balance=VolumeBalance::default();balance.enabled=true;
    let mut staged=Vec::new();let mut started=false;
    let mut events=vec![];let(mut frames,mut samples,mut sources,mut reference_samples)=(0u64,0u64,0usize,0u64);
    for bytes in fs::read(&args[1])?.chunks(8192) {
        decoder.push(bytes)?;
        while let Some((frame,reference))=decoder.next_frame_with_reference() {
            let n=frame.channels[0].len();samples=frame.sample_pos+n as u64;sources=sources.max(frame.channels.len());
            for ch in &frame.channels {for x in ch {pcm.write_all(&x.to_le_bytes())?;}}
            writeln!(meta,"{}",serde_json::json!({"start":frame.sample_pos,"samples":n,"labels":frame.labels,"events":frame.events,"objectChannels":frame.object_channels,"rawBedLabels":frame.raw_bed_labels}))?;
            if reference.len()!=2{return Err("missing stereo programme reference".into());}
            for i in 0..reference[0].len(){for channel in &reference {reference_pcm.write_all(&channel[i].to_le_bytes())?;}}
            reference_samples+=reference[0].len() as u64;
            let pending=balance.measure(frame,Some(reference));
            if let Some(m)=&pending.measurement {writeln!(measurements,"{}",serde_json::json!({"sample":pending.frame.sample_pos,"measurement":m}))?;}
            staged.push(pending);
            if !started && (balance.startup_measurement_ready()||samples>=7*48000) {
                if let Some(Command::SetProgramGain{gain,at})=balance.prime_av3a_startup(0,false) {events.push(serde_json::json!({"sample":at.unwrap_or(0),"gain":gain}));}
                started=true;
            }
            if started {for p in staged.drain(..) {for command in balance.route(&p) {
                if let Command::SetProgramGain{gain,at}=command {events.push(serde_json::json!({"sample":at.unwrap_or(p.frame.sample_pos),"gain":gain}));}
            }}}
            frames+=1;
        }
    }
    decoder.flush()?;
    if !started {
        if let Some(Command::SetProgramGain{gain,at})=balance.prime_av3a_startup(0,true) {events.push(serde_json::json!({"sample":at.unwrap_or(0),"gain":gain}));}
        for p in staged.drain(..) {for command in balance.route(&p) {if let Command::SetProgramGain{gain,at}=command{events.push(serde_json::json!({"sample":at.unwrap_or(p.frame.sample_pos),"gain":gain}));}}}
    }
    balance.finish();
    for writer in [&mut pcm,&mut meta,&mut reference_pcm,&mut measurements]{writer.flush()?;}
    fs::write(output.join("programme-timeline.json"),serde_json::to_vec_pretty(&events)?)?;
    let report=serde_json::json!({"codec":"mpegh","sampleRate":48000,"frames":frames,"samples":samples,"sources":sources,"referenceSamples":reference_samples,"measurement":balance.complete_measurement(),"finalTargetDb":balance.gain_db,"programmeGainEvents":events.len(),"reference":"pinned native MPEG-H float master before peak limiting; real startup reference then fixed programme gain"});
    fs::write(output.join("programme-reference.json"),serde_json::to_vec_pretty(&report)?)?;println!("{report}");Ok(())
}
