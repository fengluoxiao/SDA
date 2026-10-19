//! Offline shared AV3A validation: beds or metadata-mapped Mix, with optional scene dump.
use avs3a::Mp4FrameReader;
use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(value) = std::env::var("SDA_AV3A_PROBE_STACK_BYTES") {
        let size = value.parse()?;
        return std::thread::Builder::new()
            .stack_size(size)
            .spawn(|| run().map_err(|e| e.to_string()))?
            .join()
            .map_err(|_| "AV3A worker panicked")?
            .map_err(Into::into);
    }
    run()
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: av3a_probe input.m4a [output.f32le|-] [frame_limit] [scene_dir]")?;
    let output = args.next().filter(|s| s != "-");
    let limit: usize = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(usize::MAX);
    let scene = args.next().map(PathBuf::from);
    let mut writer = output.map(File::create).transpose()?.map(BufWriter::new);
    let mut scene_pcm = None;
    let mut scene_json = None;
    if let Some(dir) = &scene {
        std::fs::create_dir_all(dir)?;
        scene_pcm = Some(BufWriter::new(File::create(dir.join("sources.pcm"))?));
        scene_json = Some(BufWriter::new(File::create(dir.join("frames.jsonl"))?));
    }
    let mut reader = Mp4FrameReader::open(BufReader::new(File::open(input)?))?;
    let mut decoder = sda_core::StreamingDecoder::new("av3a")?;
    let started = std::time::Instant::now();
    let (mut frames, mut samples, mut peak) = (0usize, 0u64, 0f32);
    let mut sum2 = Vec::new();
    let mut labels = Vec::new();
    let mut beds = Vec::new();
    let mut declarations = Vec::new();
    let mut events = 0usize;
    let mut checksum = 0xcbf29ce484222325u64;
    let mut source_rate = 48000u32;
    let mut transport_channels = 0;
    let mut profile = String::new();
    let mut loudness = None;
    let mut drain =
        |decoder: &mut sda_core::StreamingDecoder| -> Result<(), Box<dyn std::error::Error>> {
            let errors = decoder.drain_errors();
            if !errors.is_empty() {
                return Err(format!("{errors:?}").into());
            }
            while let Some(f) = decoder.next_frame() {
                loudness = f.reference_loudness.clone();
                if f.sample_pos != samples || f.sample_rate != 48000 || f.channels.is_empty() {
                    return Err("invalid output clock".into());
                }
                let count = f.channels[0].len();
                if f.channels.iter().any(|c| c.len() != count) || count == 0 {
                    return Err("invalid output PCM shape".into());
                }
                if sum2.is_empty() {
                    sum2 = vec![0f64; f.channels.len()];
                    labels = f.labels.clone();
                    beds = f.raw_bed_labels.clone();
                    declarations = f.object_channels.clone();
                }
                if labels != f.labels || declarations != f.object_channels {
                    return Err("output topology changed".into());
                }
                if f.events
                    .iter()
                    .any(|e| e.sample_pos < samples || e.sample_pos >= samples + count as u64)
                {
                    return Err("metadata clock is outside output frame".into());
                }
                events += f.events.len();
                if let Some(out) = &mut scene_json {
                    serde_json::to_writer(
                        &mut *out,
                        &serde_json::json!({"start":f.sample_pos,"samples":count,"labels":f.labels,"objectChannels":f.object_channels,"events":f.events}),
                    )?;
                    out.write_all(b"\n")?;
                }
                if let Some(out) = &mut scene_pcm {
                    for pcm in &f.channels {
                        for value in pcm {
                            out.write_all(&value.to_le_bytes())?;
                        }
                    }
                }
                for i in 0..count {
                    for (ch, pcm) in f.channels.iter().enumerate() {
                        let value = pcm[i];
                        if !value.is_finite() {
                            return Err("non-finite PCM".into());
                        }
                        peak = peak.max(value.abs());
                        sum2[ch] += f64::from(value).powi(2);
                        let bytes = value.to_le_bytes();
                        for byte in bytes {
                            checksum = (checksum ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                        }
                        if let Some(out) = &mut writer {
                            out.write_all(&bytes)?;
                        }
                    }
                }
                samples += count as u64;
            }
            Ok(())
        };
    while frames < limit {
        let Some(frame) = reader.next_frame()? else {
            break;
        };
        if frames == 0 {
            source_rate = frame.header().sample_rate;
            transport_channels = frame.header().channels;
            profile = format!("{:?}", frame.header().profile);
        }
        decoder.push(frame.bytes())?;
        drain(&mut decoder)?;
        frames += 1;
    }
    decoder.flush();
    drain(&mut decoder)?;
    drop(drain);
    let expected =
        (frames as u64 * 1024 * 48000 + u64::from(source_rate) / 2) / u64::from(source_rate);
    if samples != expected {
        return Err(format!("duration {samples} != {expected}").into());
    }
    let rms: Vec<_> = sum2.iter().map(|v| (v / samples as f64).sqrt()).collect();
    println!(
        "{}",
        serde_json::json!({"referenceLoudness":loudness,"profile":profile,"frames":frames,"samples":samples,"sampleRate":48000,"sourceRate":source_rate,"transportChannels":transport_channels,"channels":sum2.len(),"labels":labels,"rawBedLabels":beds,"objectChannels":declarations,"eventCount":events,"seconds":samples as f64/48000.,"peak":peak,"rms":rms,"fnv1a":format!("{checksum:016x}"),"elapsedSeconds":started.elapsed().as_secs_f64()})
    );
    Ok(())
}
