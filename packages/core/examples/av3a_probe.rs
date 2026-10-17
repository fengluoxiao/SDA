//! Offline validation only: decode AV3A M4A through the actual shared pipeline.
use avs3a::Mp4FrameReader;
use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: av3a_probe input.m4a [output.f32le] [frame_limit]")?;
    let output = args.next();
    let limit: usize = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(usize::MAX);
    let mut writer = output.map(File::create).transpose()?.map(BufWriter::new);
    let mut reader = Mp4FrameReader::open(BufReader::new(File::open(input)?))?;
    let mut decoder = sda_core::StreamingDecoder::new("av3a")?;
    let started = std::time::Instant::now();
    let (mut frames, mut samples, mut peak, mut sum2) = (0, 0u64, 0.0f32, vec![0.0f64; 12]);
    let mut checksum = 0xcbf29ce484222325u64;
    let mut drain =
        |decoder: &mut sda_core::StreamingDecoder| -> Result<(), Box<dyn std::error::Error>> {
            let errors = decoder.drain_errors();
            if !errors.is_empty() {
                return Err(format!("{errors:?}").into());
            }
            while let Some(f) = decoder.next_frame() {
                if f.sample_pos != samples || f.sample_rate != 48000 || f.channels.len() != 12 {
                    return Err("invalid output clock/topology".into());
                }
                let count = f.channels[0].len();
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
        decoder.push(frame.bytes())?;
        drain(&mut decoder)?;
        frames += 1;
    }
    decoder.flush();
    drain(&mut decoder)?;
    drop(drain);
    let expected = (frames as u64 * 1024 * 48000 + 22050) / 44100;
    if samples != expected {
        return Err(format!("duration {samples} != {expected}").into());
    }
    let rms: Vec<_> = sum2.iter().map(|v| (v / samples as f64).sqrt()).collect();
    println!(
        "{}",
        serde_json::json!({"frames":frames,"samples":samples,"sampleRate":48000,
        "channels":12,"seconds":samples as f64/48000.0,"peak":peak,"rms":rms,
        "fnv1a":format!("{checksum:016x}"),"elapsedSeconds":started.elapsed().as_secs_f64()})
    );
    Ok(())
}
