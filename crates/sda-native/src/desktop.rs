//! Desktop file transport: only controls and presentation metadata cross process IO.
//! Codec PCM goes directly from the shared mobile decoder into the render worker.
use super::*;
use serde_json::{Value,json};
use std::sync::{mpsc,atomic::Ordering};
use sda_native_renderer::protocol::{ControlExtension,install_control_extension};

struct Controller { tx: mpsc::SyncSender<Value> }
impl ControlExtension for Controller {
 fn handle(&self,value:&Value)->bool {
  let kind=value["type"].as_str().unwrap_or("");
  if !matches!(kind,"fileMetadata"|"fileProbe"|"fileOpen"|"fileSeek"|"filePause"|"fileClose"|"fileConfigure"|"fileStatus") {return false;}
  if self.tx.try_send(value.clone()).is_err(){emit(json!({"kind":"reply","requestId":value["requestId"],"session":value["session"],"ok":false,"error":"native file control queue full"}));}
  true
 }
}
fn emit(value:Value){sda_native_renderer::write_event(&Event::FilePlayback{data:value});}
fn reply(command:&Value,result:EngineResult<Value>){emit(match result{Ok(value)=>json!({"kind":"reply","requestId":command["requestId"],"session":command["session"],"ok":true,"value":value}),Err(error)=>json!({"kind":"reply","requestId":command["requestId"],"session":command["session"],"ok":false,"error":error})});}
#[derive(Clone,Copy)] enum Source {Mp3,Alac,Packet,Sdar,Stereo,Wave}
struct Playback { session:String,epoch:u64,source:Source,codec:String,rate:u32,duration:u64,eof:bool,finished:bool,upmix:bool,stereo:Option<mp3::Mp3FileDecoder>,wave:Option<desktop_wave::Wave> }
fn packet(engine:&mut MobileEngine,path:&str)->Option<(Source,String,u32,u64)> {
 for codec in ["eac3","av3a","ac4","mpegh","truehd","dts"]{if let Ok(duration)=engine.open_packet_file(path,codec){return Some((Source::Packet,codec.into(),engine.packet_source.as_ref().map_or(48000,|p|p.input_rate()),duration));}}None
}
fn probe(path:&str)->EngineResult<Value> {
 if sda_format::BedFile::is_sda(path){return Ok(json!({"supported":true,"codec":"sdar"}));}
 // Prefer the same clear immersive programme over an AAC compatibility track.
 for codec in ["eac3","av3a","ac4","mpegh","truehd","dts"] {if packet_source::PacketSource::open(path,codec).is_ok(){return Ok(json!({"supported":true,"codec":codec}));}}
 if desktop_wave::probe(path){return Ok(json!({"supported":true,"codec":"pcm","waveMetadata":true}));}
 if let Ok(input)=mp3::Mp3FileDecoder::open_audio(path){return Ok(json!({"supported":true,"codec":input.source_codec(),"sampleRate":input.input_rate()}));}
 Ok(json!({"supported":false,"detail":"input has no verified native random-access decoder"}))
}
struct Metadata {session:String,path:String,description:Value,events:Vec<sda_format::objects::ObjectEvent>,bytes:usize,expected:usize}
fn open(engine:&mut MobileEngine,command:&Value,prepared:Option<Metadata>)->EngineResult<Playback>{
 let path=command["path"].as_str().ok_or("missing native input path")?;
 let metadata=std::fs::metadata(path).map_err(|e|e.to_string())?;if !metadata.is_file(){return Err("native source is not a regular file".into());}
 engine.reset_source_state();engine.begin_seek(0,0);
 engine.startup.lock().expect("startup lock").paused=command["paused"].as_bool().unwrap_or(false);
 engine.balance.enabled=command["volumeBalance"].as_bool().unwrap_or(false);
 engine.config.object_rendering=command["objectRendering"].as_bool().unwrap_or(true);engine.config.binaural_enabled=command["binauralEnabled"].as_bool().unwrap_or(true);
 if let Some(measurement)=command.get("measurement").filter(|v|!v.is_null()){engine.set_measured_loudness(&measurement.to_string())?;}
 let hint=command["codec"].as_str().unwrap_or("auto");
 let mut stereo=None;let mut wave=None;
 let (source,codec,rate,duration)=if command["masterEditable"].as_bool()==Some(true){
  let rate=engine.open_sdar_master(path,command["layout"].as_str().unwrap_or("7.1.4"))?;let duration=engine.sdar_info()["frames"].as_u64().unwrap_or(0)/48;(Source::Sdar,"sdar".into(),rate,duration)
 }else if let Some(metadata)=prepared {
  if metadata.session!=command["session"].as_str().unwrap_or("")||metadata.path!=path||metadata.expected!=metadata.events.len(){return Err("BWF metadata session mismatch".into());}
  let input=desktop_wave::Wave::open(path,metadata.description,metadata.events)?;let values=(Source::Wave,input.codec.into(),input.rate,input.duration);wave=Some(input);values
 }else if hint=="sdar" {
  let rate=engine.open_sdar_with_layout(path,None,command["layout"].as_str().unwrap_or("7.1.4"))?;let duration=engine.sdar_info()["frames"].as_u64().unwrap_or(0)/48;(Source::Sdar,"sdar".into(),rate,duration)
 }else if hint=="alac" {let rate=engine.open_alac(path)?;let duration=engine.alac_decoder.as_ref().and_then(|d|d.duration_frames()).unwrap_or(0)/48;(Source::Alac,"alac".into(),rate,duration)
 }else if hint=="mp3" {let rate=engine.open_mp3(path)?;let duration=engine.mp3_decoder.as_ref().and_then(|d|d.duration_frames()).unwrap_or(0)/48;(Source::Mp3,"mp3".into(),rate,duration)
 }else if hint!="auto" {let duration=engine.open_packet_file(path,hint)?;(Source::Packet,hint.into(),engine.packet_source.as_ref().map_or(48000,|p|p.input_rate()),duration)
 }else if let Ok(rate)=engine.open_sdar_with_layout(path,None,command["layout"].as_str().unwrap_or("7.1.4")) {let duration=engine.sdar_info()["frames"].as_u64().unwrap_or(0)/48;(Source::Sdar,"sdar".into(),rate,duration)
 }else if let Some(input)=packet(engine,path) { input
 }else if let Ok(decoder)=mp3::Mp3FileDecoder::open_audio(path) {
  let rate=decoder.input_rate();let duration=decoder.duration_frames().unwrap_or(0)/48;let codec=decoder.source_codec().to_string();stereo=Some(decoder);(Source::Stereo,codec,rate,duration)
 }else {return Err(format!("source lacks a native indexed input: {}",mp3::Mp3FileDecoder::open_audio(path).err().unwrap_or_default()));
 };
 engine.decoder.set_object_rendering(engine.config.object_rendering);
 Ok(Playback{session:command["session"].as_str().ok_or("missing native file session")?.into(),epoch:command["epoch"].as_u64().unwrap_or(0),source,codec,rate,duration,eof:false,finished:false,upmix:command["upmix"].as_bool().unwrap_or(false),stereo,wave})
}
fn pull(engine:&mut MobileEngine,p:&mut Playback)->EngineResult<()>{
 let (_,done)=match p.source {Source::Wave=>{
  let input=p.wave.as_mut().ok_or("native wave source missing")?;
  if let Some(frame)=input.read()?{let count=frame.channels[0].len();
   if engine.announced_codec.as_deref()!=Some(p.codec.as_str()){if let Some(pipeline)=&engine.pipeline{pipeline.commands.push(render_command::RenderCommand::Command(Command::SetProgramCodec{codec:p.codec.clone()})).map_err(|_|"native codec queue full")?;}engine.announced_codec=Some(p.codec.clone());}
   for mut frame in engine.desktop_converter.push(frame)?{frame.sample_pos+=engine.decode_origin;for e in &mut frame.events{e.sample_pos+=engine.decode_origin;}engine.queue_object_visual_events(&frame.events,engine.playback_status().consumed_sample_pos)?;let end=frame.sample_pos+frame.channels[0].len()as u64;let pending=engine.balance.measure(frame,None);engine.pending.lock().expect("pending lock").push_back(pending);*engine.newest_sample_pos.lock().expect("clock lock")=end;}
   engine.drain_pending_into_pipeline()?;(count,false)
  }else{(0,true)}
 },Source::Stereo=>{
  let decoder=p.stereo.as_mut().ok_or("native PCM source missing")?;let codec=decoder.source_codec();let samples=decoder.read_interleaved(4096)?;let count=samples.len()/2;let done=decoder.is_finished();
  if count>0{if engine.announced_codec.as_deref()!=Some(codec){engine.pipeline.as_ref().ok_or("native render pipeline missing")?.commands.push(render_command::RenderCommand::Command(Command::SetProgramCodec{codec:codec.into()})).map_err(|_|"native codec command queue full")?;engine.announced_codec=Some(codec.into());}let reference=vec![samples.iter().step_by(2).copied().collect(),samples.iter().skip(1).step_by(2).copied().collect()];let mut frame=engine.alac_adapter.live_sda_frame(&samples,p.upmix,engine.decoded_sample_pos())?;frame.codec=codec;engine.feed_alac_pcm(frame,reference)?;}
  (count,done)
 },Source::Mp3=>engine.pull_mp3(4096)?,Source::Alac=>engine.pull_alac(4096,p.upmix)?,Source::Packet=>engine.pull_packet_file(4096)?,Source::Sdar=>engine.pull_sdar(4096)?};
 if done&&!p.eof {if engine.decoded_sample_pos()==0{return Err("native input produced no audio frames".into());}engine.finish()?;p.eof=true;if p.duration==0{p.duration=engine.decoded_sample_pos()/48;}}
 Ok(())
}
fn seek(engine:&mut MobileEngine,p:&mut Playback,ms:f64,paused:bool,epoch:u64)->EngineResult<()> {
 if !ms.is_finite()||ms<0. {return Err("invalid native seek time".into());}
 engine.startup.lock().expect("startup lock").paused=paused;
 if let Some(input)=p.wave.as_mut(){let(origin,target)=input.seek(ms as u64)?;engine.begin_seek(origin,target);engine.decode_origin=origin;engine.desktop_converter.reset(input.position(),input.rate);}else if let Some(decoder)=p.stereo.as_mut(){let target=(ms*48.)as u64;let origin=decoder.seek(target.saturating_sub(6144)/48)?;engine.alac_adapter=Default::default();engine.begin_seek(origin,target);}else{engine.seek(ms as u64)?;}
 p.epoch=epoch;p.eof=false;p.finished=false;Ok(())
}
fn snapshot(engine:&MobileEngine,p:&Playback)->Value{
 let status=engine.playback_status();let (beds,objects)=engine.frame_router.topology();
 json!({"session":p.session,"epoch":p.epoch,"codec":p.codec,"sampleRate":p.rate,"outputSampleRate":48000,"durationMs":p.duration,"positionMs":status.position_ms,"decodedMs":status.decoded_sample_pos/48,
 "paused":engine.startup.lock().expect("startup lock").paused,"preparing":status.preparing_audio,"ended":p.finished,"channels":p.wave.as_ref().map(|w|w.channels).unwrap_or_else(||p.stereo.as_ref().map_or_else(||if matches!(p.source,Source::Alac|Source::Mp3){2}else{beds.len()+objects},|d|d.source_channels())),"bedLabels":beds,"objectChannels":objects,"objects":engine.object_snapshot(),"soundingObjectIds":status.sounding_object_ids})
}
pub fn install(commands:Arc<render_command::RenderCommandQueue>,fifo:Arc<stereo_fifo::StereoFifo>,telemetry:Arc<RuntimeTelemetry>)->std::thread::JoinHandle<()>{
 let(tx,rx)=mpsc::sync_channel(32);install_control_extension(Arc::new(Controller{tx}));
 std::thread::Builder::new().name("sda-native-file".into()).stack_size(16*1024*1024).spawn(move||{
  let mut engine=MobileEngine::new(EngineConfig::default(),None).expect("desktop engine config");engine.renderer=None;
  engine.pipeline=Some(RenderPipeline{commands:commands.clone(),fifo,telemetry:telemetry.clone()});engine.external_transport=true;
  let mut metadata:Option<Metadata>=None;let mut playback:Option<Playback>=None;let mut last=Instant::now();
  while !telemetry.shutdown_requested.load(Ordering::Acquire){
   match rx.recv_timeout(Duration::from_millis(2)){
    Ok(command)=>{
     let kind=command["type"].as_str().unwrap_or("");
     if kind=="fileMetadata" {
      let result=(||->EngineResult<Value>{
       if let Some(header)=command.get("description"){
        let expected=command["eventCount"].as_u64().filter(|v|*v<=2_000_000).ok_or("BWF event count exceeds its metadata limit")? as usize;
        metadata=Some(Metadata{session:command["session"].as_str().ok_or("missing metadata session")?.into(),path:command["path"].as_str().ok_or("missing BWF path")?.into(),description:header.clone(),events:Vec::new(),bytes:0,expected});
       }
       let current=metadata.as_mut().filter(|m|Some(m.session.as_str())==command["session"].as_str()).ok_or("BWF metadata was replaced")?;
       if let Some(events)=command.get("events"){current.bytes+=events.to_string().len();if current.bytes>64*1024*1024{return Err("BWF automation exceeds its metadata bound".into());}let events:Vec<sda_format::objects::ObjectEvent>=serde_json::from_value(events.clone()).map_err(|e|e.to_string())?;if current.events.len()+events.len()>current.expected{return Err("too many BWF events".into());}current.events.extend(events);}
       Ok(json!({"events":current.events.len()}))
      })();if result.is_err(){metadata=None;}reply(&command,result);
     }
     else if kind=="fileClose"&&playback.as_ref().is_none_or(|p|command["session"].as_str()!=Some(p.session.as_str())){
      if metadata.as_ref().is_some_and(|m|command["session"].as_str()==Some(m.session.as_str())){metadata=None;}
      reply(&command,Ok(json!({})));
     }
     else if kind=="fileProbe" {reply(&command,command["path"].as_str().ok_or_else(||"missing native probe path".to_string()).and_then(probe));}
     else if kind=="fileOpen" {match open(&mut engine,&command,metadata.take()){Ok(mut p)=>{let result=pull(&mut engine,&mut p).map(|_|{let mut value=snapshot(&engine,&p);value["authored"]=engine.sdar_info();value});playback=Some(p);reply(&command,result);},Err(error)=>{playback=None;reply(&command,Err(error));}}}
     else if let Some(p)=playback.as_mut().filter(|p|command["session"].as_str()==Some(&p.session)) {
      let result:EngineResult<Value>=(||{match kind{
       "fileSeek"=>{let ms=command["positionMs"].as_f64().ok_or("missing native seek time")?;seek(&mut engine,p,ms,command["paused"].as_bool().unwrap_or(false),command["epoch"].as_u64().unwrap_or(p.epoch+1))?;},
       "filePause"=>engine.set_paused(command["paused"].as_bool().ok_or("missing native pause intent")?)?,
       "fileClose"=>{engine.startup.lock().expect("startup lock").paused=true;engine.begin_seek(0,0);},
       "fileConfigure"=>{if let Some(scene)=command.get("authoredScene"){
          let source=engine.sdar_source.as_mut().ok_or("authored source is unavailable")?;if !source.file.is_editable_master(){return Err("only a WAV master accepts scene edits".into());}source.scene=serde_json::from_value(scene.clone()).map_err(|e|e.to_string())?;
        }if let Some(layout)=command["layout"].as_str(){if engine.sdar_source.is_some(){engine.set_sdar_layout(layout)?;}else{engine.config.layout=layout.into();}}if let Some(value)=command["upmix"].as_bool(){p.upmix=value;}if let Some(value)=command["volumeBalance"].as_bool(){engine.set_volume_balance(value)?;}if engine.sdar_source.is_none(){if let Some(value)=command["objectRendering"].as_bool(){engine.config.object_rendering=value;engine.decoder.set_object_rendering(value);}}},
       "fileStatus"=>{},_=>return Err("unknown native file operation".into())};Ok(snapshot(&engine,p))})();reply(&command,result);
      if kind=="fileClose"{playback=None;engine.reset_source_state();}
     }else{reply(&command,Err("native file session was replaced".into()));}
    },Err(mpsc::RecvTimeoutError::Disconnected)=>break,Err(mpsc::RecvTimeoutError::Timeout)=>{}
   }
   if let Some(p)=playback.as_mut(){
    let status=engine.playback_status();let preparing=!engine.startup.lock().expect("startup lock").started;
    let reserve=if preparing&&engine.balance.enabled{8*48000}else{48000};
    if !p.eof&&status.decoded_sample_pos.saturating_sub(status.consumed_sample_pos)<reserve {
     if let Err(error)=pull(&mut engine,p){let _=engine.set_paused(true);p.eof=true;p.finished=true;emit(json!({"kind":"error","session":p.session,"error":error}));}
    }
    if p.eof&&!p.finished&&engine.playback_status().consumed_sample_pos>=engine.decoded_sample_pos(){p.finished=true;}
    if last.elapsed()>=Duration::from_millis(33){emit(json!({"kind":"state","session":p.session,"snapshot":snapshot(&engine,p)}));last=Instant::now();}
   }
  }
 }).expect("desktop native file worker")
}

#[cfg(test)] mod tests {
 use super::*;
 use std::sync::OnceLock;
 #[derive(Default)] struct Output{state:OnceLock<(Arc<stereo_fifo::StereoFifo>,Arc<RuntimeTelemetry>)>}
 impl AudioOutput for Output{fn run(self:Arc<Self>,fifo:Arc<stereo_fifo::StereoFifo>,telemetry:Arc<RuntimeTelemetry>,_:Arc<render_command::RenderCommandQueue>){self.state.set((fifo,telemetry.clone())).ok();while !telemetry.shutdown_requested.load(Ordering::Acquire){std::thread::sleep(Duration::from_millis(1));}}}
 impl Output{fn callback(&self)->(u64,usize,bool){let(fifo,t)=self.state.get().unwrap();fifo.apply_flush_from_consumer();let before=t.callback_consumed_sample_pos.load(Ordering::Acquire);let enabled=t.callback_output_enabled.load(Ordering::Acquire)&&!t.output_device_recovering.load(Ordering::Acquire);let mut pcm=[0f32;1024];let n=if enabled{fifo.pop_into_f32(&mut pcm,2)}else{0};sda_native_renderer::record_callback(t,Instant::now(),512,n,enabled);(before,n,pcm[..n*2].iter().any(|x|x.abs()>1e-6))}}
 fn exercise(path:&str){
  let mut e=MobileEngine::new(EngineConfig{direct_object_hrtf:true,directional_hrtf:true,..Default::default()},None).unwrap();e.load_hrtf(concat!(env!("CARGO_MANIFEST_DIR"),"/../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json")).unwrap();let output=Arc::new(Output::default());e.start(output.clone()).unwrap();let until=Instant::now()+Duration::from_secs(3);while output.state.get().is_none(){assert!(Instant::now()<until);std::thread::yield_now();}
  let mut p=open(&mut e,&json!({"path":path,"session":"test","paused":true,"volumeBalance":false,"upmix":false}),None).unwrap_or_else(|error|panic!("{path}: {error}; packet: {:?}",packet_source::PacketSource::open(path,if path.ends_with(".thd"){"truehd"}else{"dts"}).err()));pull(&mut e,&mut p).unwrap();
  let targets=if p.duration>0&&p.duration<200{[p.duration/8,p.duration/4,p.duration/2]}else{[20,60,120]};
  for (epoch,target)in targets.into_iter().enumerate(){
   seek(&mut e,&mut p,target as f64,true,epoch as u64+1).unwrap();let until=Instant::now()+Duration::from_secs(3);while !e.playback_status().paused{output.callback();assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(1));}
   for _ in 0..4{assert_eq!(output.callback().1,0);}assert_eq!(e.playback_status().consumed_sample_pos,target*48);
   e.set_paused(false).unwrap();let until=Instant::now()+Duration::from_secs(5);let mut audible=false;
   while Instant::now()<until{let status=e.playback_status();if !p.eof&&status.decoded_sample_pos<status.consumed_sample_pos+16384{pull(&mut e,&mut p).unwrap();}let(before,n,sound)=output.callback();if n>0{assert!(before>=target*48,"old/preroll audio leaked through desktop seek");if sound{audible=true;break;}}std::thread::sleep(Duration::from_millis(1));}
   assert!(audible,"native desktop did not resume audible output");assert_eq!(snapshot(&e,&p)["epoch"],epoch as u64+1);
  }
  for (epoch,target)in [targets[2],targets[0],targets[1]].into_iter().enumerate(){seek(&mut e,&mut p,target as f64,false,10+epoch as u64).unwrap();}
  assert_eq!(snapshot(&e,&p)["epoch"],12);assert_eq!(e.playback_status().consumed_sample_pos,targets[1]*48);e.stop();
 }
 #[test] fn desktop_native_alac_and_mp3_seek_keep_pause_intent_and_callback_clock(){for name in ["tone-48000-2ch.m4a","tone-44100-2ch.mp3"]{exercise(&format!("{}/tests/fixtures/alac/{name}",env!("CARGO_MANIFEST_DIR")));}}
 #[test] fn desktop_native_packet_callbacks_and_optional_encoded_fixtures(){
  let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  for path in [root.join("tests/fixtures/ios-stereo-tones.eac3"),root.join("../../packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a"),root.join("../../packages/core/mpegh/fixtures/motion.mhas")]{exercise(path.to_str().unwrap());}
  if let Some(paths)=std::env::var_os("SDA_DESKTOP_SEEK_FIXTURES"){for path in serde_json::from_str::<Vec<String>>(&paths.to_string_lossy()).unwrap(){exercise(&path);}}
 }

 #[test]fn desktop_stereo_container_probe(){if let Ok(path)=std::env::var("SDA_DESKTOP_STEREO_FILE"){let decoder=mp3::Mp3FileDecoder::open_audio(&path).unwrap_or_else(|e|panic!("{path}: {e}"));assert!(decoder.input_rate()>0);}}
 #[test] fn desktop_packet_pcm_seek_matches_continuous_decode(){
  fn read(path:&str,target:u64,seek_to:Option<u64>)->Vec<Vec<f32>>{
   let mut e=MobileEngine::new(EngineConfig::default(),None).unwrap();let mut p=open(&mut e,&json!({"path":path,"session":"pcm","paused":true}),None).unwrap();
   if let Some(ms)=seek_to{seek(&mut e,&mut p,ms as f64,true,1).unwrap();}
   let span=4800u64;let mut pcm:Vec<Vec<f32>>=vec![];
   for _ in 0..20000{pull(&mut e,&mut p).unwrap();for f in e.take_pending_frames(){let from=target.max(f.sample_pos);let to=(target+span).min(f.sample_pos+f.channels[0].len()as u64);if from>=to{continue;}if pcm.is_empty(){pcm=vec![vec![];f.channels.len()];}assert_eq!(pcm.len(),f.channels.len());for(a,b)in pcm.iter_mut().zip(f.channels){a.extend_from_slice(&b[(from-f.sample_pos)as usize..(to-f.sample_pos)as usize]);}}
    if pcm.first().is_some_and(|p|p.len()>=span as usize)||p.eof{break;}
   }assert_eq!(pcm.first().map(Vec::len),Some(span as usize),"missing samples for {path}");pcm
  }
  let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));let mut paths=vec![root.join("tests/fixtures/ios-stereo-tones.eac3").to_string_lossy().into_owned()];
  if let Some(value)=std::env::var_os("SDA_DESKTOP_PCM_FIXTURES"){paths.extend(serde_json::from_str::<Vec<String>>(&value.to_string_lossy()).unwrap());}
  for path in paths {let target=401*48;let expected=read(&path,target,None);let actual=read(&path,target,Some(401));let error=expected.iter().zip(&actual).flat_map(|(a,b)|a.iter().zip(b).map(|(a,b)|(a-b).abs())).fold(0f32,f32::max);assert!(error<0.0001,"seek changed decoded programme: {path} residual={error}");}
 }

}

/// Analysis runs in a short-lived native child without opening an output device.
/// Only progress and a complete measurement leave this process; PCM is bounded.
pub fn reference_entry(args:&[String])->bool {
 if args.get(1).map(String::as_str)!=Some("--programme-reference"){return false;}
 let path=args.get(2).cloned().unwrap_or_default();
 let result=std::thread::Builder::new().name("sda-desktop-reference".into()).stack_size(16*1024*1024).spawn(move||->EngineResult<()>{
  let details=probe(&path)?;let codec=details["codec"].as_str().unwrap_or("");
  let mut reference=match codec {
   "av3a"=>crate::av3a_reference::open(&path)?,
   "alac"|"mp3"|"pcm"|"flac"|"aac"|"vorbis"=>match stereo_reference::FreshStereoReference::open_audio(&path){Ok(r)=>r,Err(_)=>{println!("{}",json!({"type":"unsupported"}));return Ok(());}},
   _=>{println!("{}",json!({"type":"unsupported"}));return Ok(());}
  };
  let mut last=std::time::Instant::now();
  loop{let step=reference.step(65536)?;
   if let Some(measurement)=step.measurement{let mut value=serde_json::to_value(&measurement).map_err(|e|e.to_string())?;
    value["type"]=json!("measurement");value["protocol"]=json!(1);value["channels"]=json!(reference.channels());
    value["sampleRate"]=json!(measurement.input_sample_rate);value["nativeReference"]=serde_json::to_value(&measurement).map_err(|e|e.to_string())?;
    println!("{value}");return Ok(());}
   if last.elapsed()>=std::time::Duration::from_millis(200){println!("{}",json!({"type":"progress","progress":null,"frames":step.frames}));last=std::time::Instant::now();}
  }
 }).map_err(|e|e.to_string()).and_then(|t|t.join().map_err(|_|"native reference panicked".to_string()).and_then(|r|r));
 if let Err(error)=result {println!("{}",json!({"type":"error","error":error}));std::process::exit(1);}
 true
}
