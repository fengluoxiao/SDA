use macindecode_ac4_bitstream::{
    SyncFrameIter,
    audio_substream::{Ac4AudioSubstream, SubstreamContext},
    substream::SubstreamInfo,
    topology::Ac4Topology,
};
use serde_json::json;
use std::{
    error::Error,
    fs::File,
    io::{BufWriter, Write},
};

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("Usage: sda-ac4-ims-inspect input.ac4 output.jsonl".into());
    }
    if std::fs::metadata(&args[1])?.len() > 512 * 1024 * 1024 {
        return Err("Input exceeds the 512 MiB research limit".into());
    }
    let input = std::fs::read(&args[1])?;
    let mut output = BufWriter::new(File::create_new(&args[2])?);
    let mut count = 0;
    for (index, frame) in SyncFrameIter::new(&input).enumerate() {
        let frame = frame?;
        let topology = Ac4Topology::parse(frame.raw_frame)?;
        let mut candidates = topology
            .presentations()
            .iter()
            .enumerate()
            .filter(|(_, presentation)| presentation.presentation_version == 2);
        let (presentation_index, presentation) = candidates.next().ok_or("No IMS presentation")?;
        if candidates.next().is_some() {
            return Err("Multiple IMS presentations require explicit selection".into());
        }
        if presentation
            .substream
            .as_ref()
            .is_some_and(|stream| stream.alternative)
        {
            return Err("Alternative IMS metadata context is not implemented".into());
        }
        if presentation.presentation_version != 2
            || presentation.frame_rate_factor != 1
            || presentation.frame_rate_fraction != 1
        {
            return Err("Expected an unfragmented IMS presentation".into());
        }
        let mut streams = Vec::new();
        for &group_index in presentation.group_indices() {
            let group = topology
                .groups()
                .get(group_index as usize)
                .ok_or("Invalid group index")?;
            for stream in group.substreams() {
                let SubstreamInfo::Chan(info) = stream else {
                    return Err("Expected channel-coded IMS".into());
                };
                if !matches!(info.channel_mode.ch_mode, 1 | 5..=10) {
                    return Err(
                        "IMS physical stereo mapping is not established for this channel mode"
                            .into(),
                    );
                }
                let stream_index = info.substream_index().ok_or("Missing substream index")?;
                let payload = topology.substream_payload(frame.raw_frame, stream_index)?;
                let context = SubstreamContext {
                    sus_ver: 1,
                    alternative: false,
                    ajoc: false,
                    channel_mode: Some(1),
                    b_iframe: Some(info.audio_ndot()),
                    alternative_oamd: None,
                };
                let metadata = Ac4AudioSubstream::parse(payload, context)?;
                let mut extensions = Vec::new();
                if let Some(emdf) = metadata.emdf_payloads {
                    for extension in emdf.payloads() {
                        let bytes: Vec<_> = extension
                            .bytes(payload)
                            .ok_or("Invalid EMDF byte range")?
                            .iter()
                            .collect();
                        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                        extensions.push(json!({"id": extension.id, "bytes": extension.size_bytes,
                            "bitOffset": extension.bit_offset(), "hex": hex,
                            "discardUnknown": extension.config.discard_unknown_payload,
                            "frameAligned": extension.config.payload_frame_aligned,
                            "sampleOffset": extension.config.sample_offset, "duration": extension.config.duration}));
                    }
                }
                streams.push(
                    json!({"index": stream_index, "independent": info.audio_ndot(),
                    "signaledChannelMode": info.channel_mode.ch_mode,
                    "audioBytes": metadata.audio_size, "metadataBytes": metadata.metadata_bytes,
                    "dialogEnhancement": metadata.tools_metadata.dialog_enhancement.data_present,
                    "payloads": extensions}),
                );
            }
        }
        serde_json::to_writer(
            &mut output,
            &json!({"frame": index, "sequence": topology.toc.sequence_counter,
            "fsIndex": topology.toc.fs_index, "frameRateIndex": topology.toc.frame_rate_index,
            "presentationIndex": presentation_index,
            "preVirtualized": presentation.pre_virtualized, "streams": streams}),
        )?;
        writeln!(output)?;
        count += 1;
    }
    if count == 0 {
        return Err("No AC-4 frames".into());
    }
    output.flush()?;
    eprintln!(
        "Extracted bounded EMDF payloads from {count} IMS frames; payload semantics remain unassigned"
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("IMS inspection failed: {error}");
        std::process::exit(1);
    }
}
