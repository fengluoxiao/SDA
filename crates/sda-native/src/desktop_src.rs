//! Bounded scene/compatible-bed SRC for desktop codecs above/below 48 kHz.
//! Source lanes remain aligned; native automation is split on its original clock.
use super::*;
use rubato::{Resampler,SincFixedIn,SincInterpolationParameters,SincInterpolationType,WindowFunction};
const LANES:usize=sda_core::MAX_DECODED_SOURCE_CHANNELS+8;
#[derive(Default)]pub(crate) struct Converter{seed:Option<(u64,u32)>,state:Option<State>}
struct State{rate:u32,prefix:u64,src:SincFixedIn<f32>,cursor:u64,input:u64,templates:VecDeque<(u64,FrameData)>,events:VecDeque<ObjectEvent>,trims:VecDeque<sda_native_renderer::dolby_trim::TrimUpdate>,tail:Vec<usize>}
impl Converter{
 pub fn reset(&mut self,sample:u64,rate:u32){self.state=None;self.seed=Some((sample,rate));}
 pub fn push(&mut self,mut frame:FrameData)->EngineResult<Vec<FrameData>>{
  if frame.sample_rate==48000{return Ok(vec![frame]);}
  frame.validate_source_count()?;let count=frame.channels.first().map_or(0,Vec::len);
  if count==0{return Ok(vec![]);}if count>32768||frame.channels.len()!=frame.labels.len()||frame.channels.iter().any(|c|c.len()!=count){return Err("invalid native SRC scene shape".into());}
  if let Some(bed)=&frame.compatibility_bed{if bed.channels.len()>8||bed.channels.iter().any(|c|c.len()!=count){return Err("native SRC needs aligned compatible programme chunks".into());}}
  if self.state.is_none(){let rate=frame.sample_rate;let(mut a,mut b)=(rate,48000);while b!=0{let rem=a%b;a=b;b=rem;}
   let origin=self.seed.map_or(0,|(sample,source_rate)|((u128::from(sample)*u128::from(rate))/u128::from(source_rate.max(1)))as u64);let prefix=origin%u64::from(rate/a);
   let src=SincFixedIn::<f32>::new(48000./f64::from(rate),1.,SincInterpolationParameters{sinc_len:128,f_cutoff:0.95,oversampling_factor:128,interpolation:SincInterpolationType::Cubic,window:WindowFunction::BlackmanHarris2},32768,LANES).map_err(|e|e.to_string())?;
   self.state=Some(State{rate,prefix,src,cursor:0,input:prefix,templates:VecDeque::new(),events:VecDeque::new(),trims:VecDeque::new(),tail:vec![0;LANES]});
  }
  let state=self.state.as_mut().unwrap();if state.rate!=frame.sample_rate{return Err("native SRC requires a new source segment for a rate change".into());}
  let scale=|n:u64|((u128::from(n+state.prefix)*48000)/u128::from(state.rate))as u64;
  let ramp=|n:u32|((u64::from(n)*48000+u64::from(state.rate)/2)/u64::from(state.rate))as u32;
  for mut event in std::mem::take(&mut frame.events){event.sample_pos=scale(event.sample_pos);event.ramp_duration=ramp(event.ramp_duration);state.events.push_back(event);}
  for mut trim in std::mem::take(&mut frame.downmix_trims){trim.sample_pos=scale(trim.sample_pos);state.trims.push_back(trim);}
  frame.ramp_duration=ramp(frame.ramp_duration);let start=if state.templates.is_empty(){0}else{scale(frame.sample_pos)};
  let scene=std::mem::take(&mut frame.channels);let compatible=frame.compatibility_bed.as_mut().map(|b|std::mem::take(&mut b.channels));
  if let Some(bed)=&mut frame.compatibility_bed{bed.channels=vec![vec![];bed.labels.len()];}
  state.templates.push_back((start,frame));let mut result=vec![];
  if state.input==state.prefix&&state.prefix>0{result.extend(state.process(vec![vec![0.;state.prefix as usize];scene.len()],compatible.as_ref().map(|c|vec![vec![0.;state.prefix as usize];c.len()]),false)?);}
  state.input+=count as u64;result.extend(state.process(scene,compatible,false)?);Ok(result)
 }
 pub fn finish(&mut self)->EngineResult<Vec<FrameData>>{
  let Some(state)=&mut self.state else{return Ok(vec![])};let expected=(u128::from(state.input)*48000/u128::from(state.rate))as u64;let mut out=vec![];
  for _ in 0..32{if state.cursor>=expected{return Ok(out);}let Some((_,template))=state.templates.back()else{return Ok(out)};
   let n=template.labels.len();let bed=template.compatibility_bed.as_ref().map(|b|vec![vec![0.;1024];b.labels.len()]);out.extend(state.process(vec![vec![0.;1024];n],bed,true)?);
  }Err("native SRC tail did not drain".into())
 }
}
impl State{
 fn process(&mut self,scene:Vec<Vec<f32>>,bed:Option<Vec<Vec<f32>>>,finishing:bool)->EngineResult<Vec<FrameData>>{
  let count=scene[0].len();self.src.set_chunk_size(count).map_err(|e|e.to_string())?;
  let mut data=vec![vec![];LANES];let mut active=vec![false;LANES];
  for(i,channel)in scene.into_iter().enumerate(){data[i]=channel;active[i]=true;}
  if let Some(bed)=bed{for(i,channel)in bed.into_iter().enumerate(){let lane=sda_core::MAX_DECODED_SOURCE_CHANNELS+i;data[lane]=channel;active[lane]=true;}}
  for i in 0..LANES{if active[i]{self.tail[i]=128;}else if self.tail[i]>0{data[i]=vec![0.;count];active[i]=true;self.tail[i]=self.tail[i].saturating_sub(count);}}
  let output=self.src.process(&data,Some(&active)).map_err(|e|e.to_string())?;let got=output.iter().map(Vec::len).max().unwrap_or(0);
  let expected=(u128::from(self.input)*48000/u128::from(self.rate))as u64;
  let available=if finishing{got.min(expected.saturating_sub(self.cursor)as usize)}else{got};let limit=self.cursor+available as u64;let first=self.cursor;let mut frames=vec![];
  while self.cursor<limit{
   while self.templates.len()>1&&self.templates[1].0<=self.cursor{self.templates.pop_front();}
   let(_,template)=self.templates.front().ok_or("native SRC lacks its scene declaration")?;let end=self.templates.get(1).map_or(limit,|v|limit.min(v.0));if end<=self.cursor{return Err("native SRC automation did not advance".into());}
   let from=(self.cursor-first)as usize;let to=(end-first)as usize;
   let mut events=vec![];while self.events.front().is_some_and(|v|v.sample_pos<end){events.push(self.events.pop_front().unwrap());}
   let mut trims=vec![];while self.trims.front().is_some_and(|v|v.sample_pos<end){trims.push(self.trims.pop_front().unwrap());}
   let compatibility_bed=template.compatibility_bed.as_ref().map(|b|sda_core::CompatibilityBed{labels:b.labels.clone(),channels:(0..b.labels.len()).map(|i|output[sda_core::MAX_DECODED_SOURCE_CHANNELS+i][from..to].to_vec()).collect()});
   frames.push(FrameData{codec:template.codec,sample_rate:48000,sample_pos:self.cursor,channels:(0..template.labels.len()).map(|i|output[i][from..to].to_vec()).collect(),labels:template.labels.clone(),raw_bed_labels:template.raw_bed_labels.clone(),events,object_channels:template.object_channels.clone(),program_loudness:template.program_loudness.clone(),ramp_duration:template.ramp_duration,downmix_trims:trims,reference_loudness:template.reference_loudness.clone(),compatibility_bed});self.cursor=end;
  }Ok(frames)
 }
}

#[cfg(test)]mod tests{
 use super::*;
 fn frame(rate:u32,start:u64,count:usize)->FrameData{
  let events=if start==0{vec![ObjectEvent{id:7,sample_pos:0,has_pos:true,pos:[0.,1.,0.],gain_db:0.,size:[0.;3],anchor:"room".into(),diffuse:0.,distance_m:None,distance_infinite:false,screen_factor:None,depth_factor:None,ramp_duration:rate/2,zone_exclusion:vec![],horizontal_only:false}]}else{vec![]};
  FrameData{codec:"truehd",sample_rate:rate,sample_pos:start,channels:vec![(0..count).map(|i|((start+i as u64)as f32*0.01).sin()*0.1).collect(),vec![0.05;count]],labels:vec!["Obj_7".into(),"FrontRight".into()],raw_bed_labels:vec!["FrontRight".into()],events,object_channels:vec![sda_core::ObjectChannelDecl{id:7,channel:0}],program_loudness:None,ramp_duration:rate/10,downmix_trims:vec![],reference_loudness:None,compatibility_bed:None}
 }
 #[test]fn desktop_non48k_preserves_duration_metadata_and_contiguous_clock(){for rate in [44100,96000,192000]{let mut converter=Converter::default();let mut frames=vec![];let mut at=0;while at<rate as u64{let count=80.min((rate as u64-at)as usize);frames.extend(converter.push(frame(rate,at,count)).unwrap());at+=count as u64;}frames.extend(converter.finish().unwrap());let mut clock=0;let mut events=vec![];for frame in frames{assert_eq!(frame.sample_pos,clock);assert_eq!(frame.sample_rate,48000);assert_eq!(frame.labels,["Obj_7","FrontRight"]);clock+=frame.channels[0].len()as u64;events.extend(frame.events);}assert_eq!(clock,48000,"rate={rate}");assert_eq!(events.len(),1);assert_eq!(events[0].ramp_duration,24000);}}
 #[test]fn desktop_src_seek_matches_continuous_rational_phase(){
  for rate in [44100,96000,192000]{
   fn run(rate:u32,origin:u64)->Vec<f32>{let mut src=Converter::default();src.reset(origin,rate);let mut at=origin;let mut pcm=vec![];
    while at<u64::from(rate)*2{let n=80.min((u64::from(rate)*2-at)as usize);let mut f=frame(rate,at,n);f.sample_pos=at-origin;f.events.clear();for f in src.push(f).unwrap(){pcm.extend_from_slice(&f.channels[0]);}at+=n as u64;}
    for f in src.finish().unwrap(){pcm.extend_from_slice(&f.channels[0]);}pcm
   }
   let full=run(rate,0);let origin=u64::from(rate)*9/10+13;let mut a=rate;let mut b=48000;while b!=0{let c=a%b;a=b;b=c;}let prefix=origin%u64::from(rate/a);
   let seek=run(rate,origin);let offset=((origin-prefix)*48000/u64::from(rate))as usize;
   let error=seek.iter().enumerate().skip(6144).take(4096).map(|(i,v)|(v-full[offset+i]).abs()).fold(0f32,f32::max);
   assert!(error<0.00001,"rate={rate} offset={offset} residual={error}");
  }
 }
 #[test]fn desktop_48k_is_a_zero_copy_pass_through(){let mut converter=Converter::default();let original=frame(48000,0,80);let pointer=original.channels[0].as_ptr();let out=converter.push(original).unwrap();assert_eq!(out[0].channels[0].as_ptr(),pointer);assert_eq!(out[0].events[0].ramp_duration,24000);}
}
