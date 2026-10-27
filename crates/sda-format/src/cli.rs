use crate::{BedFile, PreparedResources};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::{BufRead, Write},
};
fn message(output: &mut impl Write, header: Value, payload: &[u8]) -> Result<(), String> {
    let mut header = header;
    header["dataBytes"] = json!(payload.len());
    let bytes = serde_json::to_vec(&header).map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("SDAR response metadata is too large".into());
    }
    output
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .and_then(|_| output.write_all(&bytes))
        .and_then(|_| output.write_all(payload))
        .and_then(|_| output.flush())
        .map_err(|e| e.to_string())
}
fn interleaved(pcm: &[Vec<f32>]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(pcm[0].len() * pcm.len() * 4);
    for i in 0..pcm[0].len() {
        for channel in pcm {
            bytes.extend_from_slice(&channel[i].to_le_bytes());
        }
    }
    bytes
}
pub fn run(args: &[String]) -> Result<(), String> {
    let command = args.first().map(String::as_str).unwrap_or("--version");
    if command == "--version" {
        println!(
            "{}",
            json!({"format":"SDAR Bed Channel","protocol":1,"faad":"2.11.4","bedCodec":"sdar-bed-channel-v2","codecName":"SDAR Bed Channel","audioTracks":1,"notice":"Code from FAAD2 is copyright (c) Nero AG, www.nero.com"})
        );
        return Ok(());
    }
    let input = args.get(1).ok_or("SDAR input file required")?;
    let allow_wave = args.iter().any(|s| s == "--allow-wave");
    let mut file = if allow_wave {
        BedFile::open_master_wave(input)?
    } else {
        BedFile::open(input)?
    };
    if command == "serve" {
        let layout = args
            .iter()
            .position(|s| s == "--layout")
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
            .unwrap_or("7.1.4");
        let resources = PreparedResources::new(&file.metadata)?;
        let scene = resources.renderer_scene(&file.metadata, layout)?;
        let ready = json!({"type":"ready","protocol":1,"sampleRate":48000,"inputRate":file.input_rate,"channels":file.metadata.channels(),"frames":file.frames(),"labels":file.metadata.renderer_labels(),"summary":file.metadata.summary(),"render":file.metadata.render,"nativeScene":scene,"resources":resources});
        file.metadata.assets.clear();
        let mut output = std::io::BufWriter::new(std::io::stdout().lock());
        message(&mut output, ready, &[])?;
        for line in std::io::stdin().lock().lines() {
            let line = line.map_err(|e| e.to_string())?;
            if line.len() > 16384 {
                return Err("SDAR control message too large".into());
            }
            let command: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let result: Result<(), String> = (|| match command["type"].as_str() {
                Some("pull") => {
                    let max = command["frames"].as_u64().unwrap_or(4096).clamp(1, 4096) as usize;
                    let start = file.position();
                    let pcm = file.read(max)?;
                    let count = pcm[0].len();
                    message(
                        &mut output,
                        json!({"type":"frame","start":start,"frames":count,"sampleRate":48000,"channels":file.metadata.channels(),"eof":file.finished()}),
                        &interleaved(&pcm),
                    )
                }
                Some("seek") => {
                    file.seek(
                        command["sample"]
                            .as_u64()
                            .ok_or("SDAR seek requires a sample position")?,
                    )?;
                    message(
                        &mut output,
                        json!({"type":"seeked","sample":file.position()}),
                        &[],
                    )
                }
                Some("close") => Ok(()),
                _ => Err("Unknown SDAR control command".into()),
            })();
            if let Err(error) = result {
                message(&mut output, json!({"type":"error","error":error}), &[])?;
            }
            if command["type"] == "close" {
                break;
            }
        }
        return Ok(());
    }
    if command == "info" {
        println!(
            "{}",
            json!({"summary":file.metadata.summary(),"render":file.metadata.render,"transport":file.transport,"inputRate":file.input_rate,"assets":file.metadata.assets.len()})
        );
        return Ok(());
    }
    let mut output = if command == "decode" {
        Some(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(args.get(2).ok_or("PCM output file required")?)
                .map_err(|e| e.to_string())?,
        )
    } else if command == "verify" {
        None
    } else {
        return Err("Unknown SDAR codec command".into());
    };
    let mut frames = 0u64;
    let mut peak = 0f32;
    loop {
        let pcm = file.read(4096)?;
        let count = pcm[0].len();
        if count == 0 {
            break;
        }
        frames += count as u64;
        for channel in &pcm {
            for value in channel {
                if !value.is_finite() {
                    return Err("Nonfinite SDAR PCM".into());
                }
                peak = peak.max(value.abs());
            }
        }
        if let Some(output) = &mut output {
            output
                .write_all(&interleaved(&pcm))
                .map_err(|e| e.to_string())?;
        }
    }
    if let Some(output) = &mut output {
        output.sync_all().map_err(|e| e.to_string())?;
    }
    println!(
        "{}",
        json!({"verified":true,"frames":frames,"sampleRate":48000,"channels":file.metadata.channels(),"inputRate":file.input_rate,"samplePeak":peak})
    );
    Ok(())
}
pub fn main(args: &[String]) {
    if let Err(error) = run(args) {
        if args.first().is_some_and(|v| v == "serve") {
            let _ = message(
                &mut std::io::stdout().lock(),
                json!({"type":"error","error":error}),
                &[],
            );
        } else {
            eprintln!("{error}");
        }
        std::process::exit(1);
    }
}
