//! Fixed canonical speaker reference; this is a metering route, never audible.
use std::path::{Path,PathBuf};
use sha2::{Digest,Sha256};
use rubato::{Resampler,SincFixedIn,SincInterpolationParameters,SincInterpolationType,WindowFunction};
use sda_native_renderer::{hrtf::NativeHrtfSet,vbap::LayoutId,reference_convolution::{StereoReferenceConvolver,ReferenceLfe,prepare_bed_filters}};
use crate::{balance::LoudnessMeter,stereo_reference::{RenderedReference,RENDER_REFERENCE_PROFILE}};
struct State {labels:Vec<String>,rate:u32,inputs:Vec<usize>,lfe_inputs:Vec<(usize,f32)>,convolver:StereoReferenceConvolver,lfe:ReferenceLfe,meter:LoudnessMeter,skip:usize,flush:usize}
pub(crate) struct CanonicalReference {path:PathBuf,hash:String,state:Option<State>,excluded:bool,finished:bool}
impl CanonicalReference {
 pub fn new(path:&Path)->Self{
  // Object scenes never use this bed-only reference. Decide from the first
  // decoded frame before loading/calibrating a dense HRTF asset on a phone.
  Self{path:path.to_path_buf(),hash:String::new(),state:None,excluded:false,finished:false}
 }
 pub fn push(&mut self,frame:&sda_core::FrameData)->Result<(),String>{
  if self.finished{return Err("canonical reference already finished".into());}
  if self.excluded{return Ok(());}
  // Static, explicitly labelled multi-speaker beds only. Object transports
  // keep their verified scene reference; never reinterpret objects as beds.
  if !frame.object_channels.is_empty()||!frame.events.is_empty()||frame.channels.len()<=2{self.excluded=true;self.state=None;return Ok(());}
  if self.state.is_none(){
   self.hash=format!("{:x}",Sha256::digest(std::fs::read(&self.path).map_err(|e|e.to_string())?));
   let mut set=NativeHrtfSet::load_calibrated(&self.path)?;set.configure_spatial_cues(LayoutId::Dolby7_1_4)?;
   let prepared=prepare_bed_filters(&set,LayoutId::Dolby7_1_4,&frame.labels)?;let mut filters=vec![];let mut padding=0;
   for(l,r)in prepared.filters{let(l,r,pad)=resample_ir(l,r,frame.sample_rate)?;padding=pad;filters.push((l,r));}
   self.state=Some(State{labels:frame.labels.clone(),rate:frame.sample_rate,inputs:prepared.inputs,lfe_inputs:prepared.lfe_inputs,
    convolver:StereoReferenceConvolver::new(&filters,1024)?,lfe:ReferenceLfe::new(frame.sample_rate,1024+padding)?,meter:LoudnessMeter::new(frame.sample_rate),skip:padding,flush:padding});
  }
  let state=self.state.as_mut().unwrap();
  if state.labels!=frame.labels||state.rate!=frame.sample_rate||frame.channels.iter().any(|c|c.len()!=1024){return Err("canonical bed changed shape".into());}
  let input:Vec<_>=state.inputs.iter().map(|&i|frame.channels[i].as_slice()).collect();let mut left=vec![0.;1024];let mut right=vec![0.;1024];state.convolver.process_block(&input,&mut left,&mut right)?;
  if !state.lfe_inputs.is_empty(){let mut lfe=vec![0.;1024];for&(channel,gain)in &state.lfe_inputs{for(i,x)in frame.channels[channel].iter().enumerate(){lfe[i]+=*x*gain;}}state.lfe.add_block(&lfe,&mut left,&mut right)?;}
  let skip=state.skip.min(1024);state.skip-=skip;if skip<1024{state.meter.push(&[left[skip..].to_vec(),right[skip..].to_vec()]);}Ok(())
 }
 pub fn finish(&mut self)->Result<(),String>{
  if self.finished{return Ok(());}self.finished=true;
  if let Some(state)=&mut self.state{let zeros=vec![vec![0.;1024];state.inputs.len()];let input:Vec<_>=zeros.iter().map(Vec::as_slice).collect();
   while state.flush>0{let mut left=vec![0.;1024];let mut right=vec![0.;1024];state.convolver.process_block(&input,&mut left,&mut right)?;if !state.lfe_inputs.is_empty(){state.lfe.add_block(&[0.;1024],&mut left,&mut right)?;}let count=state.flush.min(1024);state.meter.push(&[left[..count].to_vec(),right[..count].to_vec()]);state.flush-=count;}
  }Ok(())
 }
 pub fn measurement(&self)->Option<RenderedReference>{
  if !self.finished||self.excluded{return None;}self.state.as_ref().map(|state|RenderedReference{profile:RENDER_REFERENCE_PROFILE.into(),layout:"7.1.4".into(),hrtf_sha256:self.hash.clone(),sample_rate:state.rate,measurement:state.meter.integrated()})
 }
}
fn resample_ir(l:Vec<f32>,r:Vec<f32>,rate:u32)->Result<(Vec<f32>,Vec<f32>,usize),String>{
 if rate==48000{return Ok((l,r,0));}let pad=256;let count=((l.len()+pad*2)as u64*rate as u64+24000)/48000;let skip=(pad as u64*rate as u64+24000)/48000;
 let mut src=SincFixedIn::<f32>::new(rate as f64/48000.,1.,SincInterpolationParameters{sinc_len:256,f_cutoff:0.95,interpolation:SincInterpolationType::Cubic,oversampling_factor:256,window:WindowFunction::BlackmanHarris2},1024,2).map_err(|e|e.to_string())?;
 let delay=src.output_delay();let mut output=[Vec::new(),Vec::new()];let inputs=[l,r];let mut at=0;
 while output[0].len()<delay+count as usize{let mut block=vec![vec![0.;1024];2];for ear in 0..2{for i in 0..1024{if at+i>=pad{if let Some(x)=inputs[ear].get(at+i-pad){block[ear][i]=*x;}}}}let pcm=src.process(&block,None).map_err(|e|e.to_string())?;for ear in 0..2{output[ear].extend_from_slice(&pcm[ear]);}at+=1024;}
 let scale=48000./rate as f32;let out:Vec<Vec<f32>>=output.into_iter().map(|v|v[delay..delay+count as usize].iter().map(|x|x*scale).collect()).collect();Ok((out[0].clone(),out[1].clone(),skip as usize))
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn object_reference_does_not_load_an_unused_bed_hrtf() {
  let bytes=include_bytes!("../../../packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a");
  let mut decoder=sda_core::StreamingDecoder::av3a_pipelined_source_reference();
  decoder.push(bytes).unwrap();decoder.flush();
  let mut frame=decoder.next_frame().unwrap();
  let mut bed=CanonicalReference::new(Path::new("missing-hrtf-must-not-load/hrtf-set.json"));
  assert!(bed.push(&frame).is_err(),"static beds still require their real canonical HRTF");
  frame.object_channels.push(sda_core::ObjectChannelDecl{id:42,channel:0});
  let mut scene=CanonicalReference::new(Path::new("missing-hrtf-must-not-load/hrtf-set.json"));
  scene.push(&frame).unwrap();scene.finish().unwrap();
  assert!(scene.excluded && scene.measurement().is_none());
 }
}
