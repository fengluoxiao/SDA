use sda_core::eac3_pipeline::Eac3Pipeline;
use sda_core::Pipeline;
use std::fs;

fn main() {
    let path = std::env::args().nth(1).expect("path");
    let bytes = fs::read(path).unwrap();
    let mut pipeline = Eac3Pipeline::new();
    let mut out = std::collections::VecDeque::new();
    let mut errors = Vec::new();
    pipeline.push(&bytes, &mut out, &mut errors);

    // Approximate binaural sum: average all non-LFE channels into L and R by index parity,
    // apply +6 dB makeup, then run the same -1 dBFS lookahead limiter as the peak guard.
    let ceiling = 10f32.powf(-1.0 / 20.0);
    let makeup = 10f32.powf(6.0 / 20.0);
    let mut total = 0usize;
    let mut limited = 0usize;
    while let Some(frame) = out.pop_front() {
        let n = frame.channels[0].len();
        for i in 0..n {
            let mut l = 0.0f32; let mut r = 0.0f32;
            for (ci, ch) in frame.channels.iter().enumerate() {
                if frame.labels.get(ci).map(|l| l.as_str()) == Some("LFE") { continue; }
                if ci % 2 == 0 { l += ch[i]; } else { r += ch[i]; }
            }
            let peak = (l.abs() * makeup).max(r.abs() * makeup);
            total += 1;
            if peak > ceiling { limited += 1; }
        }
    }
    println!("samples={total} over -1dBFS after +6dB makeup: {limited} ({:.2}%)", limited as f64 / total as f64 * 100.0);
}
