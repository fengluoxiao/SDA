use avs3a::{
    FrameStream, StreamEvent,
    backend::McDecoderBackend,
    decode::{Decoder, PendingDecoder},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let repeat: usize = args.get(1).ok_or("repeat")?.parse()?;
    let workers: usize = args.get(2).ok_or("workers")?.parse()?;
    let playback = args.get(3).is_some_and(|s| s == "playback");
    let fixture = include_bytes!(env!("SDA_AV3A_BENCH_FIXTURE"));
    let mut stream = FrameStream::new();
    let mut decoder: Option<Decoder<McDecoderBackend>> = None;
    let mut pcm = vec![];
    let mut hash = 0xcbf29ce484222325u64;
    let mut frames = 0u64;
    let started = std::time::Instant::now();
    for _ in 0..repeat {
        for event in stream.push(fixture)? {
            let StreamEvent::Frame(frame) = event else {
                return Err("unexpected skipped frame".into());
            };
            if decoder.is_none() {
                pcm.resize(frame.header().channels as usize * 1024, 0.);
                let backend = if playback {
                    McDecoderBackend::new_builtin()?
                } else {
                    McDecoderBackend::new_builtin_with_worker_limit(workers)?
                };
                decoder = Some(PendingDecoder::new(backend).configure(frame.header())?);
            }
            decoder
                .as_mut()
                .unwrap()
                .decode_into_f32(&frame, &mut pcm)?;
            for sample in &pcm {
                hash = (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x100000001b3);
            }
            frames += 1;
        }
    }
    if !stream.finish()?.is_empty() {
        return Err("unexpected terminal frame".into());
    }
    println!(
        "{{\"seconds\":{},\"frames\":{},\"pcmHash\":\"{:016x}\",\"workers\":{},\"playback\":{}}}",
        started.elapsed().as_secs_f64(),
        frames,
        hash,
        workers,
        playback
    );
    Ok(())
}
