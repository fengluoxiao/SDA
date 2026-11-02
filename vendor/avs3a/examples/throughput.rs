//! Fresh-process source decoder benchmark; discards PCM, emits an exact-bit checksum.
use avs3a::{BuiltinDecoder,FrameStream,StreamEvent};
use std::{io::{Read,Write},time::Instant};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let path=std::env::args().nth(1).ok_or("INPUT.av3a")?;
 let raw=std::env::args().nth(2).as_deref()==Some("--raw-f32-stdout");let stdout=std::io::stdout();let mut stream=std::io::BufWriter::with_capacity(65536,stdout.lock());
 let mut input=std::io::BufReader::new(std::fs::File::open(path)?);
 let mut parser=FrameStream::new();let mut decoder:Option<BuiltinDecoder>=None;
 let mut block=[0u8;65536];let mut pcm=Vec::new();let mut frames=0u64;let mut hash=0xcbf29ce484222325u64;
 let start=Instant::now();
 loop{let n=input.read(&mut block)?;if n==0{break;}for event in parser.push(&block[..n])? {match event{
   StreamEvent::Frame(frame)=>{
    if decoder.is_none(){let d=BuiltinDecoder::configure(frame.header())?;pcm.resize(d.sample_count()?,0.);decoder=Some(d);}
    decoder.as_mut().unwrap().decode_into_f32(&frame,&mut pcm)?;
    for x in &pcm{if !x.is_finite(){return Err("nonfinite PCM".into())}hash=(hash^x.to_bits()as u64).wrapping_mul(0x100000001b3);}
    if raw{for x in &pcm{stream.write_all(&(x/avs3a::decode::FLOAT_FULL_SCALE).to_le_bytes())?;}}
    frames+=1;
   },StreamEvent::Skipped{bytes}=>return Err(format!("skipped {bytes} bytes").into())}
  }
 }
 if parser.buffered_len()!=0{return Err("truncated input".into());}
 stream.flush()?;let report=format!("frames={frames} pcmChecksum={hash:016x} seconds={:.6}",start.elapsed().as_secs_f64());if raw{eprintln!("{report}")}else{println!("{report}")};Ok(())
}
