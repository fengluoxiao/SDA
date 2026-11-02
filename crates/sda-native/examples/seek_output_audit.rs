//! Real indexed decode/render/callback windows. Never pauses after audible seek output.
use std::{sync::{Arc,OnceLock,atomic::Ordering},time::{Duration,Instant},io::Write};
use sda_native::{EngineConfig,MobileEngine};
use sda_native_renderer::{AudioOutput,RuntimeTelemetry,stereo_fifo::StereoFifo,render_command::RenderCommandQueue};
struct Sink{state:OnceLock<(Arc<StereoFifo>,Arc<RuntimeTelemetry>)>}
impl AudioOutput for Sink{fn run(self:Arc<Self>,fifo:Arc<StereoFifo>,t:Arc<RuntimeTelemetry>,_:Arc<RenderCommandQueue>){self.state.set((fifo,t.clone())).ok();while !t.shutdown_requested.load(Ordering::Acquire){std::thread::sleep(Duration::from_millis(1));}}}
fn read(engine:&mut MobileEngine,sink:&Sink,target:u64,count:usize)->Result<Vec<f32>,String>{
 let(fifo,t)=sink.state.get().unwrap();let mut result=vec![];let deadline=Instant::now()+Duration::from_secs(30);
 while result.len()<count*2{
  let status=engine.playback_status();let lead=if status.consumed_sample_pos==0{192000}else{16384};
  if status.preparing_audio || status.decoded_sample_pos<status.consumed_sample_pos+lead{engine.pull_packet_file(4096)?;}
  fifo.apply_flush_from_consumer();let enabled=t.callback_output_enabled.load(Ordering::Acquire)&&!t.shutdown_requested.load(Ordering::Acquire);
  let before=t.callback_consumed_sample_pos.load(Ordering::Acquire);let mut pcm=[0f32;1024];let at=Instant::now();let n=if enabled{fifo.pop_into_f32(&mut pcm,2)}else{0};
  sda_native_renderer::record_callback(t,at,512,n,enabled);
  if n>0{if before<target{return Err("preroll reached callback".into());}result.extend_from_slice(&pcm[..n*2]);}else{std::thread::sleep(Duration::from_millis(1));}
  if Instant::now()>deadline{return Err(format!("callback timeout {:?}",engine.playback_status().consumed_sample_pos));}
 }
 result.truncate(count*2);Ok(result)
}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<_>=std::env::args().collect();if args.len()!=6{return Err("INPUT HRTF OUTDIR CODEC FRESH_REFERENCE_OR_DASH".into());}
 let out=std::path::Path::new(&args[3]);std::fs::create_dir_all(out)?;
 let mut e=MobileEngine::new(EngineConfig{direct_object_hrtf:true,directional_hrtf:true,hrtf_wet_weight:0.,..Default::default()},None)?;
 e.load_hrtf(&args[2])?;let sink=Arc::new(Sink{state:OnceLock::new()});e.start(sink.clone())?;e.set_spatial_enhancement(true)?;
 while sink.state.get().is_none(){std::thread::sleep(Duration::from_millis(1));}
 e.open_packet_file(&args[1],&args[4])?;e.set_volume_balance(true)?;
 if args[5]!="-"{e.set_measured_loudness(&std::fs::read_to_string(&args[5])?)?;}
 let _=read(&mut e,&sink,0,48000)?;let mut rows=vec![];
 for ms in [100013u64,20017,150233]{
  e.set_paused(true)?;e.seek(ms)?;e.set_paused(false)?;
  let pcm=read(&mut e,&sink,ms*48,96000)?;
  let file=out.join(format!("{}-{ms}.f32",args[4]));let mut f=std::fs::File::create(file)?;
  for sample in pcm{f.write_all(&sample.to_le_bytes())?;}
  let s=e.playback_status();rows.push(serde_json::json!({"targetMs":ms,"gainDb":s.program_gain_db,"appliedGainDb":s.rendered_program_gain_db,"programEnabled":s.renderer_program_enabled,"paused":s.paused,"soundingIds":s.sounding_object_ids,"objectPositions":e.object_snapshot().len()}));
 }
 e.stop();std::fs::write(out.join(format!("{}-status.json",args[4])),serde_json::to_string_pretty(&rows)?)?;println!("{}",serde_json::to_string(&rows)?);Ok(())
}
