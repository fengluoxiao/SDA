//! Source-owned PCM reader. ADM parsing is metadata-only; no PCM crosses IPC.
use super::*;
use std::{fs::File,io::{Read,Seek,SeekFrom}};
use serde::Deserialize;
use serde_json::Value;
#[derive(Clone,Debug,Deserialize,PartialEq)]
#[serde(rename_all="camelCase")]
pub(crate) struct Format {encoding:u16,channels:usize,sample_rate:u32,bits:u16,block_align:usize,channel_mask:u32}
#[derive(Deserialize)]#[serde(rename_all="camelCase")]
pub(crate) struct Description {format:Format,data_offset:u64,data_size:u64,labels:Vec<String>,#[serde(default)]raw_bed_labels:Vec<String>,#[serde(default)]object_channels:Vec<sda_format::objects::ObjectChannel>,#[serde(default)]adm:bool}
pub(crate) struct Wave {file:File,description:Description,timeline:sda_format::objects::ObjectTimeline,at:u64,origin:u64,pub rate:u32,pub codec:&'static str,pub channels:usize,pub duration:u64,meter:sda_core::scene_reference::SceneMeter}
fn u16le(b:&[u8],i:usize)->u16{u16::from_le_bytes(b[i..i+2].try_into().unwrap())}
fn u32le(b:&[u8],i:usize)->u32{u32::from_le_bytes(b[i..i+4].try_into().unwrap())}
fn u64le(b:&[u8],i:usize)->u64{u64::from_le_bytes(b[i..i+8].try_into().unwrap())}
/// Validate the actual file headers independently of the metadata sender.
fn headers(file:&mut File)->EngineResult<(Format,u64,u64)>{
 let size=file.metadata().map_err(|e|e.to_string())?.len();let mut head=[0;12];file.read_exact(&mut head).map_err(|e|e.to_string())?;
 if !matches!(&head[..4],b"RIFF"|b"RF64"|b"BW64")||&head[8..]!=b"WAVE"{return Err("not a BWF/PCM source".into());}
 let mut at=12u64;let(mut format,mut data,mut ds64)=(None,None,None);let mut chunks=0;
 while at+8<=size {chunks+=1;if chunks>1_000_000{return Err("too many WAVE chunks".into());}
  file.seek(SeekFrom::Start(at)).map_err(|e|e.to_string())?;let mut h=[0;8];file.read_exact(&mut h).map_err(|e|e.to_string())?;let mut n=u64::from(u32le(&h,4));
  if &h[..4]==b"ds64"{if n<28||n>16*1024*1024{return Err("invalid ds64".into());}let mut b=vec![0;n as usize];file.read_exact(&mut b).map_err(|e|e.to_string())?;ds64=Some(u64le(&b,8));}
  if n==u64::from(u32::MAX){if &h[..4]==b"data"{n=ds64.ok_or("missing RF64 data size")?;}else{return Err("unsupported RF64 size-table chunk".into());}}
  if at.checked_add(8).and_then(|v|v.checked_add(n)).is_none_or(|v|v>size){return Err("truncated WAVE data".into());}
  if &h[..4]==b"fmt "{if format.is_some()||!(16..=4096).contains(&n){return Err("invalid WAVE format chunk".into());}let mut b=vec![0;n as usize];file.read_exact(&mut b).map_err(|e|e.to_string())?;let mut encoding=u16le(&b,0);let mut mask=0;
   if encoding==0xfffe{if b.len()<40||u16le(&b,16)<22{return Err("invalid extensible WAVE format".into());}let valid=u16le(&b,18);if valid!=0&&valid!=u16le(&b,14){return Err("unsupported packed valid bit depth".into());}if b[26..40]!=[0,0,0,0,16,0,128,0,0,170,0,56,155,113]{return Err("unsupported WAVE subformat".into());}encoding=u16le(&b,24);mask=u32le(&b,20);}
   let f=Format{encoding,channels:u16le(&b,2)as usize,sample_rate:u32le(&b,4),bits:u16le(&b,14),block_align:u16le(&b,12)as usize,channel_mask:mask};
   if !(1..=128).contains(&f.channels)||!(8000..=384000).contains(&f.sample_rate)||!matches!((f.encoding,f.bits),(1,16|24|32)|(3,32|64))||f.block_align!=f.channels*usize::from(f.bits)/8||u32le(&b,8)!=f.sample_rate*f.block_align as u32{return Err("invalid PCM sample shape".into());}format=Some(f);
  }else if &h[..4]==b"data"{if data.replace((at+8,n)).is_some(){return Err("multiple WAVE data chunks".into());}}
  at=at.checked_add(8+n+(n&1)).ok_or("WAVE offset overflow")?;
 }
 if at!=size{return Err("invalid WAVE chunk padding".into());}let f=format.ok_or("missing WAVE format")?;let(offset,size)=data.ok_or("missing WAVE PCM")?;if size%f.block_align as u64!=0{return Err("partial WAVE frame".into());}Ok((f,offset,size))
}
pub(crate) fn probe(path:&str)->bool{File::open(path).ok().is_some_and(|mut f|headers(&mut f).is_ok())}
impl Wave {
 pub fn open(path:&str,description:Value,events:Vec<sda_format::objects::ObjectEvent>)->EngineResult<Self>{
  let description:Description=serde_json::from_value(description).map_err(|e|e.to_string())?;let mut file=File::open(path).map_err(|e|e.to_string())?;let(format,offset,size)=headers(&mut file)?;
  if description.format!=format||description.data_offset!=offset||description.data_size!=size||description.labels.len()!=format.channels||description.labels.iter().any(|s|s.is_empty()||s.len()>128){return Err("BWF metadata does not match its PCM source".into());}
  for c in &description.object_channels{if description.labels.get(c.channel as usize)!=Some(&format!("Obj_{}",c.id)){return Err("ADM labels do not match source mapping".into());}}
  let frames=size/format.block_align as u64;let timeline=sda_format::objects::ObjectTimeline::from_adm(description.object_channels.clone(),events,format.channels,frames)?;
  let codec=if description.adm{"adm"}else{"pcm"};let rate=format.sample_rate;let channels=format.channels;let duration=(u128::from(frames)*1000/u128::from(rate))as u64;
  Ok(Self{file,description,timeline,at:0,origin:0,rate,codec,channels,duration,meter:sda_core::scene_reference::SceneMeter::with_sample_rate(rate)})
 }
 pub fn seek(&mut self,ms:u64)->EngineResult<(u64,u64)>{let target=ms.saturating_mul(48).min(self.duration.saturating_mul(48).saturating_sub(1));let warm=target.saturating_sub(6144);self.at=(u128::from(warm)*u128::from(self.rate)/48000)as u64;self.origin=self.at;self.meter=sda_core::scene_reference::SceneMeter::with_sample_rate(self.rate);let(mut a,mut b)=(self.rate,48000);while b!=0{let r=a%b;a=b;b=r;}let phase=self.at%u64::from(self.rate/a);Ok((((self.at-phase)*48000/u64::from(self.rate)),target))}
 pub fn position(&self)->u64{self.origin}
 pub fn read(&mut self)->EngineResult<Option<FrameData>>{
  let f=&self.description.format;let total=self.description.data_size/f.block_align as u64;let count=(total-self.at).min(4096)as usize;if count==0{return Ok(None);}
  self.file.seek(SeekFrom::Start(self.description.data_offset+self.at*f.block_align as u64)).map_err(|e|e.to_string())?;let mut bytes=vec![0;count*f.block_align];self.file.read_exact(&mut bytes).map_err(|e|e.to_string())?;let width=usize::from(f.bits)/8;let mut channels=vec![vec![0.;count];f.channels];
  for n in 0..count{for(ch,plane)in channels.iter_mut().enumerate(){let at=n*f.block_align+ch*width;plane[n]=match(f.encoding,f.bits){(1,16)=>i16::from_le_bytes(bytes[at..at+2].try_into().unwrap())as f32/32768.,(1,24)=>((i32::from(bytes[at])|i32::from(bytes[at+1])<<8|i32::from(bytes[at+2])<<16)<<8>>8)as f32/8388608.,(1,32)=>i32::from_le_bytes(bytes[at..at+4].try_into().unwrap())as f32/2147483648.,(3,32)=>f32::from_le_bytes(bytes[at..at+4].try_into().unwrap()),(3,64)=>f64::from_le_bytes(bytes[at..at+8].try_into().unwrap())as f32,_=>unreachable!()};if !plane[n].is_finite(){return Err("nonfinite WAVE PCM".into());}}}
  let events=self.timeline.events(self.at,count).into_iter().map(|mut e|{e.sample_pos=e.sample_pos.saturating_sub(self.origin);serde_json::from_value(serde_json::to_value(e).unwrap()).map_err(|e|e.to_string())}).collect::<Result<Vec<_>,_>>()?;
  let mut frame=FrameData{codec:self.codec,sample_rate:self.rate,sample_pos:self.at-self.origin,channels,labels:self.description.labels.clone(),raw_bed_labels:self.description.raw_bed_labels.clone(),events,object_channels:self.description.object_channels.iter().map(|c|sda_core::ObjectChannelDecl{id:c.id,channel:c.channel}).collect(),program_loudness:None,ramp_duration:0,downmix_trims:vec![],reference_loudness:None,compatibility_bed:None};
  frame.reference_loudness=Some(self.meter.measure(&frame));self.at+=count as u64;Ok(Some(frame))
 }
}
#[cfg(test)] mod tests {
 use super::*;
 use serde_json::json;
 #[test]fn native_wave_reads_original_pcm_and_restores_object_ramps_after_seek(){
  for(rate,bits,encoding)in[(44100u32,24u16,1u16),(96000,32,3)]{
   let path=std::env::temp_dir().join(format!("sda-native-wave-{}-{rate}.wav",std::process::id()));let frames=rate as usize*3;let width=usize::from(bits)/8;let size=frames*2*width;
   let mut bytes=b"RIFF".to_vec();bytes.extend_from_slice(&(36u32+size as u32).to_le_bytes());bytes.extend_from_slice(b"WAVEfmt ");bytes.extend_from_slice(&16u32.to_le_bytes());bytes.extend_from_slice(&encoding.to_le_bytes());bytes.extend_from_slice(&2u16.to_le_bytes());bytes.extend_from_slice(&rate.to_le_bytes());bytes.extend_from_slice(&(rate*2*width as u32).to_le_bytes());bytes.extend_from_slice(&(2u16*width as u16).to_le_bytes());bytes.extend_from_slice(&bits.to_le_bytes());bytes.extend_from_slice(b"data");bytes.extend_from_slice(&(size as u32).to_le_bytes());
   for n in 0..frames{for ch in 0..2{let v=(n as f32*0.02+ch as f32).sin()*0.1;if encoding==3{bytes.extend_from_slice(&v.to_le_bytes());}else{let n=(v*8388608.)as i32;bytes.extend_from_slice(&n.to_le_bytes()[..3]);}}}std::fs::write(&path,bytes).unwrap();
   let description=json!({"format":{"encoding":encoding,"channels":2,"sampleRate":rate,"bits":bits,"blockAlign":2*width,"channelMask":0},"dataOffset":44,"dataSize":size,"labels":["L","Obj_7"],"rawBedLabels":["L"],"objectChannels":[{"id":7,"channel":1}],"adm":true});
   let event=|at:u64,pos:f64,ramp:u32|sda_format::objects::ObjectEvent{id:7,sample_pos:at,has_pos:true,pos:[pos,1.,0.],gain_db:0.,size:[0.;3],anchor:"room".into(),diffuse:0.,distance_m:None,distance_infinite:false,screen_factor:None,depth_factor:None,ramp_duration:ramp,zone_exclusion:vec![],horizontal_only:false};
   let events=vec![event(0,0.,0),event(u64::from(rate),1.,rate)];let mut full=Wave::open(path.to_str().unwrap(),description.clone(),events.clone()).unwrap();let first=full.read().unwrap().unwrap();assert_eq!(first.sample_rate,rate);assert_eq!(first.object_channels[0].id,7);assert_eq!(first.events[0].sample_pos,0);assert!(first.channels[1].iter().any(|v|v.abs()>0.01));
   let(mut origin,mut target)=full.seek(1500).unwrap();assert_eq!(target,72000);assert!(origin<target);let resumed=full.read().unwrap().unwrap();assert_eq!(resumed.sample_pos,0);assert_eq!(resumed.events[0].sample_pos,0);assert!(resumed.events[0].pos[0]>0.3&&resumed.events[0].pos[0]<0.5);assert!(resumed.events.iter().any(|e|e.ramp_duration>0));
   (origin,target)=full.seek(u64::MAX/48).unwrap();assert!(origin<=target);assert!(full.read().unwrap().is_some());
   let mut invalid=description;invalid["dataOffset"]=json!(45);assert!(Wave::open(path.to_str().unwrap(),invalid,events).is_err());std::fs::remove_file(path).unwrap();
  }
 }
}
