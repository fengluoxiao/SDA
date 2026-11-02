//! MHAS random access uses configuration snapshots and real independent frames.
//! No decoded/rendered audio is stored in the index.
use std::{fs::File,io::{Read,Seek,SeekFrom}};
const LIMIT:usize=1024*1024;
struct Bits<'a>{data:&'a[u8],bit:usize}
impl Bits<'_>{
 fn get(&mut self,n:usize)->Option<u64>{if self.bit+n>self.data.len()*8{return None;}let mut v=0;for _ in 0..n{v=(v<<1)|((self.data[self.bit/8]>>(7-self.bit%8))&1)as u64;self.bit+=1;}Some(v)}
 fn esc(&mut self,a:usize,b:usize,c:usize)->Option<u64>{let mut v=self.get(a)?;if v==(1<<a)-1{let w=self.get(b)?;v+=w;if w==(1<<b)-1{v+=self.get(c)?;}}Some(v)}
}
struct Point{offset:u64,end:u64,sample:u64,restart:bool,state:std::sync::Arc<Vec<u8>>}
pub struct MhasSource{file:File,points:Vec<Point>,next:usize,bootstrap:bool,pub duration:u64}
impl MhasSource{
 pub fn open(mut file:File)->Result<Self,String>{
  let length=file.metadata().map_err(|e|e.to_string())?.len();let mut at=0;let mut au_start=0;let mut sample=0;
  let mut points=Vec::new();let mut controls=std::collections::BTreeMap::<(u64,u64),Vec<u8>>::new();let mut snapshot=std::sync::Arc::new(Vec::new());let mut dirty=false;let mut rate=None;
  while at<length{
   file.seek(SeekFrom::Start(at)).map_err(|e|e.to_string())?;let mut header=[0u8;16];let n=file.read(&mut header).map_err(|e|e.to_string())?;
   let mut b=Bits{data:&header[..n],bit:0};let kind=b.esc(3,8,8).ok_or("truncated MHAS type")?;let label=b.esc(2,8,32).ok_or("truncated MHAS label")?;let size=b.esc(11,24,24).ok_or("truncated MHAS size")? as usize;
   if b.bit%8!=0||size>LIMIT{return Err("invalid MHAS packet boundary".into());}let head=b.bit/8;let end=at.checked_add((head+size)as u64).filter(|v|*v<=length).ok_or("truncated MHAS packet")?;
   file.seek(SeekFrom::Start(at)).map_err(|e|e.to_string())?;let mut packet=vec![0;head+size];file.read_exact(&mut packet).map_err(|e|e.to_string())?;let payload=&packet[head..];
   match kind{
    1=>{
     let mut cfg=Bits{data:payload,bit:8};let index=cfg.get(5).ok_or("truncated MPEG-H sample rate")?;
     let fs=if index==31{cfg.get(24).ok_or("truncated MPEG-H explicit rate")?}else{*[0,0,0,48000,44100,32000,24000,22050,16000].get(index as usize).ok_or("invalid MPEG-H sample rate")?};
     let frame=cfg.get(3).ok_or("truncated MPEG-H frame length")?;
     // The pinned source-capture bridge uses a 1024-sample core. Refuse to
     // invent clocks for unsupported sampling/PCM configurations.
     if fs!=48000||frame>4{return Err("MPEG-H seek requires the supported 48 kHz core".into());}
     if rate.is_some_and(|r|r!=fs){return Err("MPEG-H seek needs separate rate segments".into());}rate=Some(fs);
     controls.insert((kind,label),packet);dirty=true;
    },
    2=>{
     if rate.is_none()||payload.is_empty(){return Err("MPEG-H audio lacks configuration".into());}
     if dirty{let bytes:Vec<u8>=controls.values().flatten().copied().collect();if bytes.len()>LIMIT{return Err("MPEG-H control state exceeds input bound".into());}snapshot=std::sync::Arc::new(bytes);dirty=false;}
     points.push(Point{offset:au_start,end,sample,restart:payload[0]&0x80!=0,state:snapshot.clone()});sample+=1024;au_start=end;
    },
    // Sync, audio-scene and decoder control packets. Filler/CRC are local to
    // their access unit; do not replay their checksum against another frame.
    0|7|9=>{},
    _=>{controls.insert((kind,label),packet);dirty=true;},
   }
   at=end;
  }
  if points.is_empty()||!points[0].restart{return Err("MPEG-H stream lacks an initial independent frame".into());}
  if au_start!=length{return Err("MHAS ends in an incomplete access unit".into());}
  file.rewind().map_err(|e|e.to_string())?;Ok(Self{file,points,next:0,bootstrap:false,duration:sample})
 }
 pub fn seek(&mut self,target:u64,preroll:u64)->Result<u64,String>{
  let warm=target.saturating_sub(preroll);let mut i=self.points.partition_point(|p|p.sample<=warm).saturating_sub(1);
  while i>0&&!self.points[i].restart{i-=1;}
  self.next=i;self.bootstrap=i>0;Ok(self.points[i].sample)
 }
 pub fn next(&mut self)->Result<Option<Vec<u8>>,String>{
  let Some(p)=self.points.get(self.next)else{return Ok(None);};let n=usize::try_from(p.end-p.offset).map_err(|_|"MHAS unit size overflow")?;
  if n>LIMIT{return Err("MHAS access unit exceeds input bound".into());}
  let mut bytes=if std::mem::take(&mut self.bootstrap){p.state.as_ref().clone()}else{Vec::new()};let prefix=bytes.len();bytes.resize(prefix+n,0);
  self.file.seek(SeekFrom::Start(p.offset)).map_err(|e|e.to_string())?;self.file.read_exact(&mut bytes[prefix..]).map_err(|e|e.to_string())?;self.next+=1;Ok(Some(bytes))
 }
}
#[cfg(test)] mod tests {
 use super::*;
 use crate::mpegh::MpeghDecoder;
 #[test] fn indexed_mhas_seek_preserves_source_pcm_and_clock(){
  let path=std::env::temp_dir().join(format!("sda-mhas-seek-{}.mhas",std::process::id()));
  std::fs::write(&path,include_bytes!("../../../packages/core/mpegh/fixtures/motion.mhas")).unwrap();
  compare(&path);let _=std::fs::remove_file(path);
 }
 fn compare(path:&std::path::Path){
  let mut input=MhasSource::open(File::open(path).unwrap()).unwrap();let mut decoder=MpeghDecoder::new().unwrap();
  let mut reference=Vec::new();
  while let Some(packet)=input.next().unwrap(){decoder.push(&packet).unwrap();while let Some(frame)=decoder.next_frame(){reference.push(frame);}}
  assert!(!reference.is_empty());let end=reference.last().unwrap();assert_eq!(input.duration,end.sample_pos+end.channels[0].len()as u64);
  for target in [input.duration/2,input.duration*3/4,0,input.duration/3] {
   let origin=input.seek(target,6144).unwrap();decoder.reset().unwrap();let mut compared=0;
   while let Some(packet)=input.next().unwrap(){decoder.push(&packet).unwrap();while let Some(frame)=decoder.next_frame(){
    let absolute=origin+frame.sample_pos;
    if absolute<target{continue;}let original=reference.iter().find(|f|f.sample_pos==absolute).unwrap();
    assert_eq!(frame.labels,original.labels);let error=frame.channels.iter().zip(&original.channels).flat_map(|(a,b)|a.iter().zip(b).map(|(x,y)|(x-y).abs())).fold(0f32,f32::max);
    assert!(error<0.0001,"MPEG-H seek PCM differs at {absolute}: {error}");compared+=1;
   }if compared>=5{break;}}
   assert!(compared>0);
  }
 }
 #[test] fn actual_mhas_seek(){if let Ok(path)=std::env::var("SDA_MHAS_SEEK_FILE"){compare(std::path::Path::new(&path));}}
}
