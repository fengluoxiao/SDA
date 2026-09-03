use sda_core::eac3_pipeline::Eac3Pipeline;
use sda_core::Pipeline;
use std::env;
use std::fs;

fn main() {
    let path = env::args().nth(1).expect("missing raw EC-3 path");
    let bytes = fs::read(path).expect("read raw EC-3");
    let mut pipeline = Eac3Pipeline::new();
    let mut out = std::collections::VecDeque::new();
    let mut errors = Vec::new();
    // Push in 8 KiB chunks like the streaming demuxer.
    const CHUNK: usize = 8192;
    let mut offset = 0usize;
    let mut first_obj_frame_at_bytes = None;
    while offset < bytes.len() {
        let end = (offset + CHUNK).min(bytes.len());
        pipeline.push(&bytes[offset..end], &mut out, &mut errors);
        while let Some(frame) = out.pop_front() {
            if first_obj_frame_at_bytes.is_none() && !frame.object_channels.is_empty() {
                first_obj_frame_at_bytes = Some((end, frame.sample_pos));
                println!("first object-declaration frame after pushing {} bytes ({:.2} MB), sample_pos={} ({:.2}s)",
                    end, end as f64 / 1048576.0, frame.sample_pos, frame.sample_pos as f64 / 48000.0);
            }
        }
        offset = end;
    }
    println!("first object frame at: {:?}", first_obj_frame_at_bytes);
    let mut frames = 0usize;
    let mut frames = 0usize;
    let mut events_total = 0usize;
    let mut with_pos = 0usize;
    let mut sparse_rejects = 0usize;
    while let Some(frame) = out.pop_front() {
        frames += 1;
        events_total += frame.events.len();
        with_pos += frame.events.iter().filter(|e| e.has_pos).count();
        if frame.object_channels.is_empty() && frame.labels.iter().any(|l| l.starts_with("Obj_")) {
            sparse_rejects += 1;
        }
        if frames <= 2 {
            for e in frame.events.iter().take(6) {
                println!(
                    "  event id={} pos=[{:.2},{:.2},{:.2}] hasPos={} gainDb={} size=[{:.2},{:.2},{:.2}] samplePos={}",
                    e.id, e.pos[0], e.pos[1], e.pos[2], e.has_pos, e.gain_db, e.size[0], e.size[1], e.size[2], e.sample_pos,
                );
            }
        }
        if frames <= 5 || frames % 500 == 0 {
            println!(
                "frame {frames}: channels={} labels={:?} events={} withPos={} objDecl={} errors={}",
                frame.channels.len(),
                frame.labels.len(),
                frame.events.len(),
                frame.events.iter().filter(|e| e.has_pos).count(),
                frame.object_channels.len(),
                errors.len(),
            );
        }

    }
    let sparse = errors.iter().filter(|e| e.contains("joc-sparse")).count();
    println!("TOTAL frames={frames} events={events_total} withPos={with_pos} objDeclEmptyFrames={sparse_rejects} sparseErrors={sparse}");
    for e in errors.iter().take(8) { println!("ERR: {e}"); }
}
