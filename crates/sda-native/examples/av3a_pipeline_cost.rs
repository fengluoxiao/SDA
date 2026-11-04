use std::{io::Read,time::Instant};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let source=std::env::args().nth(1).ok_or("input.av3a")?;
 let serial=std::env::args().nth(2).as_deref()==Some("serial");
 let mut decoder=if serial{sda_core::StreamingDecoder::av3a_source_reference()}else{sda_core::StreamingDecoder::av3a_pipelined_source_reference()};
 let mut file=std::io::BufReader::new(std::fs::File::open(source)?);let mut bytes=[0;4096];let mut count=0u64;let mut frames=0;let mut last=None;let started=Instant::now();
 loop{let n=file.read(&mut bytes)?;if n==0{decoder.flush();}else{decoder.push(&bytes[..n])?;}
  while let Some(frame)=decoder.next_frame(){if frames==0{eprintln!("SCENE {} CHANNELS {}",decoder.av3a_reference_is_scene(),frame.channels.len());}frames+=1;count+=frame.channels[0].len()as u64;last=frame.reference_loudness;}
  let errors=decoder.drain_errors();if !errors.is_empty(){return Err(errors.join(";").into());}if n==0{break;}
 }
 println!("{}",serde_json::json!({"seconds":started.elapsed().as_secs_f64(),"serialMeter":serial,"frames":frames,"sampleFrames":count,"measurement":last}));Ok(())
}
