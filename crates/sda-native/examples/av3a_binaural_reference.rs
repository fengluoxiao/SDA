//! Diagnostic canonical reference; verifies metering against the actual full renderer.
use std::{io::Read,time::Instant};
use rubato::{Resampler,SincFixedIn,SincInterpolationParameters,SincInterpolationType,WindowFunction};
use sda_native_renderer::{hrtf::NativeHrtfSet,vbap::{self,LayoutId},reference_convolution::{StereoReferenceConvolver,ReferenceLfe}};
fn speaker(label:&str)->Option<&'static str>{Some(match label{"L"=>"FrontLeft","R"=>"FrontRight","C"=>"Center","Lb"=>"RearLeft","Rb"=>"RearRight","Ls"=>"SurroundLeft","Rs"=>"SurroundRight","Tfl"=>"TopFrontLeft","Tfr"=>"TopFrontRight","Tbl"=>"TopRearLeft","Tbr"=>"TopRearRight",_=>return None})}
fn resample_ir(l:Vec<f32>,r:Vec<f32>,rate:u32)->Result<(Vec<f32>,Vec<f32>,usize),String>{
 if rate==48000{return Ok((l,r,0));}let pad=256;let count=((l.len()+pad*2)as u64*rate as u64+24000)/48000;let skip=(pad as u64*rate as u64+24000)/48000;
 let mut src=SincFixedIn::<f32>::new(rate as f64/48000.,1.,SincInterpolationParameters{sinc_len:256,f_cutoff:0.95,interpolation:SincInterpolationType::Cubic,oversampling_factor:256,window:WindowFunction::BlackmanHarris2},1024,2).map_err(|e|e.to_string())?;
 let delay=src.output_delay();let mut output=[Vec::new(),Vec::new()];let inputs=[l,r];let mut at=0;
 while output[0].len()<delay+count as usize{let mut block=vec![vec![0.;1024];2];for ear in 0..2{for i in 0..1024{if at+i>=pad{if let Some(x)=inputs[ear].get(at+i-pad){block[ear][i]=*x;}}}}let pcm=src.process(&block,None).map_err(|e|e.to_string())?;for ear in 0..2{output[ear].extend_from_slice(&pcm[ear]);}at+=1024;}
 let scale=48000./rate as f32;let out:Vec<Vec<f32>>=output.into_iter().map(|v|v[delay..delay+count as usize].iter().map(|x|x*scale).collect()).collect();Ok((out[0].clone(),out[1].clone(),skip as usize))
}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<_>=std::env::args().collect();if args.len()!=3{return Err("INPUT.av3a HRTF.json".into());}
 let started=Instant::now();let mut hrtf=NativeHrtfSet::load_calibrated(std::path::Path::new(&args[2]))?;hrtf.configure_spatial_cues(LayoutId::Dolby7_1_4)?;
 let mut decoder=sda_core::StreamingDecoder::av3a_source_reference();let mut file=std::io::BufReader::new(std::fs::File::open(&args[1])?);let mut bytes=[0u8;4096];let mut eof=false;
 let mut conv:Option<StereoReferenceConvolver>=None;let mut channels=vec![];let mut lfe_channel=None;let mut lfe=None;let mut meter=None;let mut rate=0;let mut source=None;let mut frames=0usize;let mut skip=0usize;let mut removed=0usize;
 loop{
  let errors=decoder.drain_errors();if !errors.is_empty(){return Err(errors.join(";").into());}
  if let Some(frame)=decoder.next_frame(){
   if !frame.object_channels.is_empty()||!frame.events.is_empty(){return Err("diagnostic supports verified speaker beds only".into());}
   if conv.is_none(){rate=frame.sample_rate;let mut filters=vec![];
    for(i,label)in frame.labels.iter().enumerate(){if label=="LFE"{lfe_channel=Some(i);continue;}let name=speaker(label).ok_or("unknown bed label")?;let geometry=vbap::speakers(LayoutId::Dolby7_1_4).iter().find(|s|s.name==name).ok_or("unknown speaker")?;let(l,r)=hrtf.mixed_speaker(name,"7.1.4",geometry.azimuth as f64,geometry.elevation as f64,0.)?;let(l,r,padding)=resample_ir(l,r,rate)?;skip=padding;removed=padding;filters.push((l,r));channels.push(i);}
    conv=Some(StereoReferenceConvolver::new(&filters,1024)?);lfe=Some(ReferenceLfe::new(rate,1024+skip)?);meter=Some(sda_core::loudness::LoudnessMeter::new(rate));
   }
   let input:Vec<_>=channels.iter().map(|&i|frame.channels[i].as_slice()).collect();let mut left=vec![0.;1024];let mut right=vec![0.;1024];conv.as_mut().unwrap().process_block(&input,&mut left,&mut right)?;
   if let Some(i)=lfe_channel{lfe.as_mut().unwrap().add_block(&frame.channels[i],&mut left,&mut right)?;}
   let start=skip.min(1024);skip-=start;if start<1024{meter.as_mut().unwrap().push(&[left[start..].to_vec(),right[start..].to_vec()]);}
   frames+=frame.channels[0].len();source=frame.reference_loudness;
  }else if eof{break;}else{let n=file.read(&mut bytes)?;if n==0{decoder.flush();eof=true;}else{decoder.push(&bytes[..n])?;}}
 }
 if removed>0{let zeros=vec![vec![0.;1024];channels.len()];let input:Vec<_>=zeros.iter().map(Vec::as_slice).collect();let mut left=vec![0.;1024];let mut right=vec![0.;1024];conv.as_mut().unwrap().process_block(&input,&mut left,&mut right)?;if lfe_channel.is_some(){lfe.as_mut().unwrap().add_block(&[0.;1024],&mut left,&mut right)?;}meter.as_mut().unwrap().push(&[left[..removed].to_vec(),right[..removed].to_vec()]);}
 let result=meter.unwrap().integrated();let source=source.unwrap();let gain=(-18.-source.integrated_lufs.unwrap()).min(0.).min(-1.-source.true_peak_dbtp.unwrap());
 println!("{}",serde_json::json!({"sampleRate":rate,"frames":frames,"seconds":started.elapsed().as_secs_f64(),"source":source,"reference":result,"sourceGainDb":gain,"predictedBalancedOutputLufs":result.integrated_lufs.unwrap()+gain+1.8,"derivedCorrectionDb":(-18.-result.integrated_lufs.unwrap()-gain).min(0.)}));Ok(())
}
