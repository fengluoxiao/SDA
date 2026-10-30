//! Fresh-file, pre-device stereo loudness audit. No persistent audio/measurement cache.
//! Usage: stereo_loudness_audit INPUT.m4a HRTF.json OUTPUT.f32 stereo|upmix on|off [fresh-reference.json]
use std::{fs::File, io::{BufWriter, Write}, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}, time::{Duration, Instant}};
use sda_native::{EngineConfig, MobileEngine};
use sda_native_renderer::{AudioOutput, RuntimeTelemetry, stereo_fifo::StereoFifo, render_command::RenderCommandQueue};
struct Capture { file: Mutex<BufWriter<File>>, frames: AtomicU64, done: AtomicBool }
impl AudioOutput for Capture {
    fn run(self: Arc<Self>, fifo: Arc<StereoFifo>, telemetry: Arc<RuntimeTelemetry>, _: Arc<RenderCommandQueue>) {
        let mut block=[0f32;4096];
        while !self.done.load(Ordering::Acquire) {
            fifo.apply_flush_from_consumer();
            let n=fifo.pop_into_f32(&mut block,2);
            if n>0 {
                let mut bytes=Vec::with_capacity(n*8);
                for x in &block[..n*2] { assert!(x.is_finite()); bytes.extend_from_slice(&x.to_le_bytes()); }
                self.file.lock().unwrap().write_all(&bytes).unwrap();
                self.frames.fetch_add(n as u64,Ordering::Release);
                telemetry.callback_consumed_sample_pos.fetch_add(n as u64,Ordering::Release);
            } else {std::thread::sleep(Duration::from_millis(1));}
        }
    }
}
fn main() -> Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args().collect();
    if !(args.len()==6 || args.len()==7) {return Err("INPUT.m4a HRTF.json OUTPUT.f32 stereo|upmix on|off [fresh-reference.json]".into());}
    let upmix=match args[4].as_str(){"upmix"=>true,"stereo"=>false,_=>return Err("invalid mode".into())};
    let balance=match args[5].as_str(){"on"=>true,"off"=>false,_=>return Err("invalid balance state".into())};
    let mut engine=MobileEngine::new(EngineConfig {direct_object_hrtf:true,directional_hrtf:true,..Default::default()},None)?;
    engine.load_hrtf(&args[2])?;
    let sink=Arc::new(Capture{file:Mutex::new(BufWriter::new(File::create_new(&args[3])?)),frames:AtomicU64::new(0),done:AtomicBool::new(false)});
    engine.start(sink.clone())?;
    engine.set_spatial_enhancement(true)?;
    engine.set_volume_balance(balance)?;
    let input_rate=engine.open_alac(&args[1])?;
    if args.len()==7 {engine.set_measured_loudness(&std::fs::read_to_string(&args[6])?)?;}
    engine.set_volume(1.0)?;
    let began=Instant::now();let mut next_report=48000*10;let mut progress=vec![];
    loop {
        let (_,eof)=engine.pull_alac(4096,upmix)?;
        while engine.decoded_sample_pos().saturating_sub(sink.frames.load(Ordering::Acquire))>48000*2 {
            if began.elapsed()>Duration::from_secs(1200) {return Err("audit timed out".into());}
            std::thread::sleep(Duration::from_millis(1));
        }
        if engine.decoded_sample_pos()>=next_report {
            let s=engine.playback_status();
            progress.push(serde_json::json!({"decodedFrames":s.decoded_sample_pos,"consumedFrames":s.consumed_sample_pos,"programmeGainDb":s.program_gain_db,"renderedProgrammeGainDb":s.rendered_program_gain_db,"volumeBalanceEligible":s.volume_balance_eligible}));
            next_report+=48000*30;
        }
        if eof {break;}
    }
    engine.finish()?;
    let expected=engine.decoded_sample_pos();let drain=Instant::now();
    while sink.frames.load(Ordering::Acquire)<expected {
        if drain.elapsed()>Duration::from_secs(120) {return Err("audit drain timed out".into());}
        std::thread::sleep(Duration::from_millis(2));
    }
    sink.done.store(true,Ordering::Release);engine.stop();
    let mut file=sink.file.lock().unwrap();file.flush()?;file.get_ref().set_len(expected*8)?;
    let result=serde_json::json!({"inputSampleRate":input_rate,"sampleRate":48000,"frames":expected,"upmix":upmix,"balance":balance,"providedReference":args.len()==7,"rendererRevision":sda_native_renderer::BUILD_REVISION,"progress":progress,"status":engine.playback_status(),"elapsedSeconds":began.elapsed().as_secs_f64()});
    std::fs::write(format!("{}.json",args[3]),serde_json::to_string_pretty(&result)?)?;
    println!("{}",serde_json::json!({"output":args[3],"frames":expected,"upmix":upmix,"balance":balance,"providedReference":args.len()==7,"elapsedSeconds":began.elapsed().as_secs_f64()}));
    Ok(())
}
