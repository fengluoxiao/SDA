//! Offline raw-codec diagnostic. Usage: programme_loudness_probe input.es
use std::{fs::File, io::Read};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("input raw bitstream required")?;
    let mut file = File::open(path)?;
    let mut decoder = sda_core::StreamingDecoder::new("auto")?;
    let mut buffer = vec![0; 65536];
    let mut samples = 0u64;
    let mut frames = 0usize;
    let mut reference = None;
    let mut codec = "";
    let mut checksum = 0xcbf29ce484222325u64;
    let mut drain =
        |decoder: &mut sda_core::StreamingDecoder| -> Result<(), Box<dyn std::error::Error>> {
            let errors = decoder.drain_errors();
            if !errors.is_empty() {
                return Err(format!("{errors:?}").into());
            }
            while let Some(f) = decoder.next_frame() {
                assert_eq!(f.sample_pos, samples);
                assert_eq!(f.sample_rate, 48000);
                codec = f.codec;
                frames += 1;
                samples += f.channels[0].len() as u64;
                reference = f.reference_loudness.clone();
                for ch in &f.channels {
                    for sample in ch {
                        assert!(sample.is_finite());
                        for byte in sample.to_le_bytes() {
                            checksum = (checksum ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                        }
                    }
                }
            }
            Ok(())
        };
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        decoder.push(&buffer[..n])?;
        drain(&mut decoder)?;
    }
    decoder.flush();
    drain(&mut decoder)?;
    drop(drain);
    println!(
        "{}",
        serde_json::json!({"codec":codec,"frames":frames,"samples":samples,"referenceLoudness":reference,"planarFnv1a":format!("{checksum:016x}")})
    );
    Ok(())
}
