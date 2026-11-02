//! Seekable compressed input. Index entries contain byte offsets/timestamps only.
use std::{fs::File,io::{Read,Seek,SeekFrom},path::Path,time::SystemTime};
use symphonia::core::{formats::{FormatOptions,FormatReader,SeekMode,SeekTo},io::MediaSourceStream,meta::MetadataOptions,probe::Hint,units::{Time,TimeBase}};
const MAX_PACKET:usize=16*1024*1024;
#[derive(Clone,Copy,Debug)]
struct Point { offset:u64,bytes:usize,sample:u64,rate:u32,restart:bool }
pub struct SeekPoint { pub input_sample:u64,pub sample_rate:u32,pub output_sample:u64 }
enum Input {
 Mhas(crate::mhas_source::MhasSource),
 Container {format:Box<dyn FormatReader>,track:u32,timebase:TimeBase},
 Raw {file:File,points:Vec<Point>,next:usize},
}
pub struct PacketSource { input:Input, path:std::path::PathBuf, bytes:u64,modified:Option<SystemTime>,pub codec:String,pub duration_samples:u64 }
impl PacketSource {
 pub fn open(path:impl AsRef<Path>,codec:&str)->Result<Self,String>{
  let path=path.as_ref();let meta=std::fs::metadata(path).map_err(|e|e.to_string())?;
  let mut file=File::open(path).map_err(|e|e.to_string())?;let mut prefix=[0u8;12];let n=file.read(&mut prefix).map_err(|e|e.to_string())?;file.rewind().map_err(|e|e.to_string())?;
  let (input,duration_samples)=if codec=="mpegh" {
   let source=crate::mhas_source::MhasSource::open(file)?;let duration=source.duration;(Input::Mhas(source),duration)
  }else if n>=8&&&prefix[4..8]==b"ftyp" {
   let mut hint=Hint::new();hint.with_extension("mp4");
   let mut format=symphonia::default::get_probe().format(&hint,MediaSourceStream::new(Box::new(file),Default::default()),&FormatOptions::default(),&MetadataOptions::default()).map_err(|e|format!("MP4 packet index: {e}"))?.format;
   let tracks:Vec<_>=format.tracks().iter().filter_map(|t|t.codec_params.time_base.map(|tb|(t.id,tb,t.codec_params.n_frames))).collect();
   let mut selected=None;
   for (id,tb,duration) in tracks {
    if format.seek(SeekMode::Accurate,SeekTo::Time{time:Time::from(0.),track_id:Some(id)}).is_err(){continue;}
    for _ in 0..512 {let Ok(packet)=format.next_packet()else{break;};if packet.track_id()!=id{continue;}
     if sda_core::seek_header::inspect(codec,&packet.data).is_ok(){selected=Some((id,tb,duration));}break;
    }
    if selected.is_some(){break;}
   }
   let (track,timebase,duration)=selected.ok_or("MP4 does not contain the declared clear audio codec")?;
   format.seek(SeekMode::Accurate,SeekTo::Time{time:Time::from(0.),track_id:Some(track)}).map_err(|e|e.to_string())?;
   let samples=duration.map_or(0,|n|(u128::from(n)*u128::from(timebase.numer)*48000/u128::from(timebase.denom))as u64);
   (Input::Container{format,track,timebase},samples)
  }else{
   let mut points:Vec<Point>=Vec::new();let mut offset=0;let mut sample=0;let mut rate=0;
   while offset<meta.len(){
    file.seek(SeekFrom::Start(offset)).map_err(|e|e.to_string())?;let mut head=[0u8;64];let count=file.read(&mut head).map_err(|e|e.to_string())?;
    let h=sda_core::seek_header::inspect(codec,&head[..count])?;
    if h.bytes==0||h.bytes>MAX_PACKET||offset.checked_add(h.bytes as u64).is_none_or(|end|end>meta.len()){return Err("truncated access unit in seek index".into());}
    if rate!=0&&rate!=h.sample_rate{return Err("seek index requires a new segment for a sample-rate change".into());}rate=h.sample_rate;
    if h.advances_clock {points.push(Point{offset,bytes:h.bytes,sample,rate,restart:h.random_access});sample=sample.checked_add(u64::from(h.samples)).ok_or("audio duration overflow")?;}
    else if let Some(last)=points.last_mut(){last.bytes+=h.bytes;}else{return Err("dependent audio precedes its primary stream".into());}
    offset+=h.bytes as u64;
   }
   if points.is_empty(){return Err("no indexed audio frames".into());}file.rewind().map_err(|e|e.to_string())?;
   let duration=(u128::from(sample)*48000/u128::from(rate))as u64;
   (Input::Raw{file,points,next:0},duration)
  };
  Ok(Self{input,path:path.to_path_buf(),bytes:meta.len(),modified:meta.modified().ok(),codec:codec.into(),duration_samples})
 }
 pub fn seek(&mut self,target:u64,preroll:u64)->Result<SeekPoint,String>{
  let meta=std::fs::metadata(&self.path).map_err(|e|e.to_string())?;
  if meta.len()!=self.bytes||meta.modified().ok()!=self.modified{return Err("source changed after its seek index was built".into());}
  let warm=target.saturating_sub(preroll);
  match &mut self.input {
   Input::Mhas(source)=>{let sample=source.seek(target,preroll)?;Ok(SeekPoint{input_sample:sample,sample_rate:48000,output_sample:sample})},
   Input::Container{format,track,timebase}=>{
    let at=format.seek(SeekMode::Accurate,SeekTo::Time{time:Time::from(warm as f64/48000.),track_id:Some(*track)}).map_err(|e|e.to_string())?;
    let sample=(u128::from(at.actual_ts)*u128::from(timebase.numer)*48000/u128::from(timebase.denom))as u64;
    // Native EC-3 is 48 kHz. AV3A uses the raw normalized import path below.
    if self.codec!="eac3"&&self.codec!="ac3"{return Err("container codec needs input-rate phase metadata".into());}
    Ok(SeekPoint{input_sample:sample,sample_rate:48000,output_sample:sample})
   },
   Input::Raw{points,next,..}=>{
    let mut point=points.partition_point(|p|(u128::from(p.sample)*48000/u128::from(p.rate))as u64<=warm).saturating_sub(1);
    while point>0&&!points[point].restart{point-=1;}
    if !points[point].restart{return Err("source lacks an independent restart frame".into());}
    *next=point;let p=points[point];let (mut a,mut b)=(p.rate,48000u32);while b!=0{let r=a%b;a=b;b=r;}
    let phase=p.sample%u64::from(p.rate/a);let output=((u128::from(p.sample-phase)*48000)/u128::from(p.rate))as u64;
    Ok(SeekPoint{input_sample:p.sample,sample_rate:p.rate,output_sample:output})
   },
  }
 }
 pub fn next(&mut self)->Result<Option<Vec<u8>>,String>{match &mut self.input{
  Input::Mhas(source)=>source.next(),
  Input::Container{format,track,..}=>loop{match format.next_packet(){Ok(p)if p.track_id()==*track=>return Ok(Some(p.data.to_vec())),Ok(_)=>{},Err(symphonia::core::errors::Error::IoError(e))if e.kind()==std::io::ErrorKind::UnexpectedEof=>return Ok(None),Err(e)=>return Err(format!("read indexed packet: {e}"))}},
  Input::Raw{file,points,next}=>{let Some(point)=points.get(*next).copied()else{return Ok(None);};file.seek(SeekFrom::Start(point.offset)).map_err(|e|e.to_string())?;let mut bytes=vec![0u8;point.bytes];file.read_exact(&mut bytes).map_err(|e|e.to_string())?;*next+=1;Ok(Some(bytes))},
 }}
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn indexed_av3a_seek_preserves_phase_and_channel_energy(){
  let Some(path)=std::env::var_os("SDA_AV3A_SEEK_FILE")else{return;};let path=std::path::Path::new(&path);
  let targets=if std::env::var_os("SDA_AV3A_SHORT").is_some(){[701u64*48,3017*48,5013*48]}else{[20_017u64*48,100_013*48,150_233*48]};let span=4800u64;
  let mut source=PacketSource::open(path,"av3a").unwrap();let mut decoder=sda_core::StreamingDecoder::new("av3a").unwrap();
  let mut reference:Vec<Vec<Vec<f32>>>=targets.iter().map(|_|Vec::new()).collect();
  while let Some(bytes)=source.next().unwrap(){decoder.push(&bytes).unwrap();while let Some(frame)=decoder.next_frame(){
   for (i,target)in targets.iter().enumerate(){let start=frame.sample_pos.max(*target);let end=(frame.sample_pos+frame.channels[0].len()as u64).min(target+span);if start>=end{continue;}
    if reference[i].is_empty(){reference[i]=vec![Vec::new();frame.channels.len()];}
    for (out,input)in reference[i].iter_mut().zip(&frame.channels){out.extend_from_slice(&input[(start-frame.sample_pos)as usize..(end-frame.sample_pos)as usize]);}
   }
  }if reference.iter().all(|r|r.first().is_some_and(|c|c.len()==span as usize)){break;}}
  for (i,target)in targets.iter().enumerate(){let point=source.seek(*target,6144).unwrap();decoder.reset_for_seek(point.input_sample);let mut actual=Vec::<Vec<f32>>::new();
   while let Some(bytes)=source.next().unwrap(){decoder.push(&bytes).unwrap();while let Some(frame)=decoder.next_frame(){let pos=point.output_sample+frame.sample_pos;let start=pos.max(*target);let end=(pos+frame.channels[0].len()as u64).min(target+span);if start>=end{continue;}
    if actual.is_empty(){actual=vec![Vec::new();frame.channels.len()];}for(out,input)in actual.iter_mut().zip(&frame.channels){out.extend_from_slice(&input[(start-pos)as usize..(end-pos)as usize]);}
   }if actual.first().is_some_and(|c|c.len()==span as usize){break;}}
   assert_eq!(actual.len(),reference[i].len());let error=actual.iter().zip(&reference[i]).map(|(a,b)|{assert_eq!(a.len(),b.len());a.iter().zip(b).map(|(x,y)|(x-y).abs()).fold(0f32,f32::max)}).fold(0f32,f32::max);
   if let Some(out)=std::env::var_os("SDA_SEEK_AUDIT_DIR"){
    let out=std::path::Path::new(&out);for (name,pcm) in [("expected",&reference[i]),("actual",&actual)]{let mut data=Vec::new();for n in 0..span as usize{for channel in pcm{data.extend_from_slice(&channel[n].to_le_bytes());}}std::fs::write(out.join(format!("av3a-{target}-{name}.f32")),data).unwrap();}
   }
   eprintln!("AV3A seek {target}: origin={} input={} max={error}",point.output_sample,point.input_sample);
   // AVS3 noise filling uses a free-running PRNG (vendor/avs3a/src/random.rs).
   // A random-access decoder has different noise draws; this cannot be bit-exact
   // without decoding all preceding audio. Verify phase using the non-noise LFE,
   // and bound full-mix residual rather than concealing a one-sample SRC error.
   let lfe_error=actual[3].iter().zip(&reference[i][3]).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);
   assert!(lfe_error<1e-6,"AV3A LFE resampling phase changed: {lfe_error}");
   let mut signal=0f64;let mut noise=0f64;
   for (a,b)in actual.iter().zip(&reference[i]){for (x,y)in a.iter().zip(b){signal+=f64::from(*y).powi(2);noise+=f64::from(x-y).powi(2);}}
   let mut dot=0f64;let mut actual_energy=0f64;
   for(a,b)in actual.iter().zip(&reference[i]){
    let (mut a_power,mut b_power)=(0f64,0f64);
    for(x,y)in a.iter().zip(b){a_power+=f64::from(*x).powi(2);b_power+=f64::from(*y).powi(2);dot+=f64::from(*x)*f64::from(*y);}
    actual_energy+=a_power;
    if b_power>1e-12{let rms_ratio=(a_power/b_power).sqrt();assert!((0.98..=1.02).contains(&rms_ratio),"AV3A channel energy changed after seek: {rms_ratio}");}
   }
   let correlation=dot/(actual_energy*signal).sqrt();
   eprintln!("AV3A seek correlation={correlation}, random-noise residualDb={}",10.*(noise/signal).log10());
   assert!(correlation>0.995,"AV3A source time alignment changed: {correlation}");
  }
 }
}
