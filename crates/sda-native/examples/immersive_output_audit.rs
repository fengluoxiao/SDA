//! Fresh-file, pre-device stereo loudness audit. No persistent audio/measurement cache.
//! Usage: immersive_output_audit INPUT.raw HRTF.json OUTPUT.f32 eac3|av3a|mpegh on|off spatial-on|spatial-off
use std::{fs::File, io::{BufWriter, Write}, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}, time::{Duration, Instant}};
use sda_native::{EngineConfig, MobileEngine};
use sda_native_renderer::{AudioOutput, RuntimeTelemetry, stereo_fifo::StereoFifo, render_command::RenderCommandQueue};
struct Capture { file: Mutex<BufWriter<File>>, frames: AtomicU64, done: AtomicBool }
impl AudioOutput for Capture {
    fn run(self: Arc<Self>, fifo: Arc<StereoFifo>, telemetry: Arc<RuntimeTelemetry>, commands: Arc<RenderCommandQueue>) {
        let mut block=[0f32;4096];
        let health = std::env::var("SDA_AUDIT_HEALTH").as_deref() == Ok("1");
        let mut next_health = 4800u64;
        while !self.done.load(Ordering::Acquire) {
            fifo.apply_flush_from_consumer();
            let n=fifo.pop_into_f32(&mut block,2);
            if n>0 {
                let mut bytes=Vec::with_capacity(n*8);
                for x in &block[..n*2] { assert!(x.is_finite()); bytes.extend_from_slice(&x.to_le_bytes()); }
                self.file.lock().unwrap().write_all(&bytes).unwrap();
                let consumed = self.frames.fetch_add(n as u64,Ordering::Release)+n as u64;
                if health && consumed >= next_health {
                    let _ = commands.push(sda_native_renderer::render_command::RenderCommand::Command(sda_native_renderer::Command::Health));
                    next_health=consumed+4800;
                }
                telemetry.callback_consumed_sample_pos.fetch_add(n as u64,Ordering::Release);
            } else {std::thread::sleep(Duration::from_millis(1));}
        }
    }
}
fn main() -> Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args().collect();
    if !(args.len()==7 || args.len()==8) {return Err("INPUT.m4a HRTF.json OUTPUT.f32 stereo|upmix on|off [fresh-reference.json]".into());}
    let codec=args[4].as_str(); if !["eac3","av3a","mpegh"].contains(&codec) {return Err("invalid codec".into());}
    let balance=match args[5].as_str(){"on"=>true,"off"=>false,_=>return Err("invalid balance state".into())};
    let mut engine=MobileEngine::new(EngineConfig {direct_object_hrtf:true,directional_hrtf:true,hrtf_wet_weight:0.0,..Default::default()},None)?;
    engine.load_hrtf(&args[2])?;
    let sink=Arc::new(Capture{file:Mutex::new(BufWriter::new(File::create_new(&args[3])?)),frames:AtomicU64::new(0),done:AtomicBool::new(false)});
    engine.start(sink.clone())?;
    engine.set_spatial_enhancement(args[6]=="spatial-on")?;
    engine.set_volume_balance(balance)?;
    if args[6]=="binaural-off" { engine.set_binaural_enabled(false)?; }
    if codec=="mpegh" {engine.open_mpegh()?;} else {engine.select_compressed_codec(codec)?;}
    if args.len()==8 {engine.set_measured_loudness(&std::fs::read_to_string(&args[7])?)?;}
    let mut input=std::io::BufReader::new(File::open(&args[1])?);
    let chunk_bytes=std::env::var("SDA_AUDIT_CHUNK_BYTES").ok().and_then(|v|v.parse::<usize>().ok()).unwrap_or(4096).clamp(1024,65536);
    let mut bytes=vec![0u8;chunk_bytes];
    engine.set_volume(1.0)?;
    let began=Instant::now();let mut next_report=0;let mut progress=vec![];
    loop {
        let n=std::io::Read::read(&mut input,&mut bytes)?; if n==0 {break;} let decode=engine.feed(&bytes[..n])?;if !decode.errors.is_empty(){return Err(format!("decode errors: {:?}",decode.errors).into());}
        while engine.decoded_sample_pos().saturating_sub(sink.frames.load(Ordering::Acquire))>if sink.frames.load(Ordering::Acquire)==0 {48000*8} else {48000*2} {
            if began.elapsed()>Duration::from_secs(1200) {return Err("audit timed out".into());}
            std::thread::sleep(Duration::from_millis(1));
        }
        if engine.decoded_sample_pos()>=next_report {
            let s=engine.playback_status();
            progress.push(serde_json::json!({"decodedFrames":s.decoded_sample_pos,"consumedFrames":s.consumed_sample_pos,"programmeGainDb":s.program_gain_db,"renderedProgrammeGainDb":s.rendered_program_gain_db,"volumeBalanceEligible":s.volume_balance_eligible,"status":s}));
            next_report=engine.decoded_sample_pos()+48000;
        }

    }
    engine.finish()?;
    let expected=engine.decoded_sample_pos();let drain=Instant::now();
    while sink.frames.load(Ordering::Acquire)<expected {
        if drain.elapsed()>Duration::from_secs(120) {return Err("audit drain timed out".into());}
        std::thread::sleep(Duration::from_millis(2));
    }
    sink.done.store(true,Ordering::Release);engine.stop();
    let mut file=sink.file.lock().unwrap();file.flush()?;file.get_ref().set_len(expected*8)?;
    let result=serde_json::json!({"sampleRate":48000,"frames":expected,"chunkBytes":chunk_bytes,"healthQueries":std::env::var("SDA_AUDIT_HEALTH").as_deref()==Ok("1"),"codec":codec,"balance":balance,"spatialEnhancement":args[6]=="spatial-on","freshWholeSourceReference":args.len()==8,"rendererRevision":sda_native_renderer::BUILD_REVISION,"progress":progress,"status":engine.playback_status(),"elapsedSeconds":began.elapsed().as_secs_f64()});
    std::fs::write(format!("{}.json",args[3]),serde_json::to_string_pretty(&result)?)?;
    println!("{}",serde_json::json!({"output":args[3],"frames":expected,"codec":codec,"balance":balance,"spatialEnhancement":args[6]=="spatial-on","freshWholeSourceReference":args.len()==8,"elapsedSeconds":began.elapsed().as_secs_f64()}));
    Ok(())
}
