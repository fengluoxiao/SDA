//! Diagnostic capture from the actual shared C decoder, before HRTF rendering.
//! Used only by scripts/test-mpegh-native.mjs; never supplies playback PCM.
use std::{fs, io::Write};
use sda_native::mpegh::MpeghDecoder;
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let bytes = fs::read(&args[1]).map_err(|e| e.to_string())?;
    let chunk: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut decoder = MpeghDecoder::new()?;
    let mut pcm = fs::File::create(format!("{}.pcm",args[2])).map_err(|e|e.to_string())?;
    let mut frames = Vec::new();
    for bytes in bytes.chunks(chunk) {
        decoder.push(bytes)?;
        while let Some(frame) = decoder.next_frame() {
            for channel in &frame.channels { for sample in channel { pcm.write_all(&sample.to_le_bytes()).map_err(|e|e.to_string())?; } }
            frames.push(serde_json::json!({
                "codec":frame.codec,"sampleRate":frame.sample_rate,"samplePos":frame.sample_pos,
                "samples":frame.channels[0].len(),"labels":frame.labels,"events":frame.events,
                "objectChannels":frame.object_channels,"rawBedLabels":frame.raw_bed_labels,
            }));
        }
    }
    decoder.flush()?;
    fs::write(format!("{}.json",args[2]),serde_json::to_vec(&frames).unwrap()).map_err(|e|e.to_string())?;
    println!("captured {} MPEG-H frames",frames.len());
    Ok(())
}
