//! Header-only access-unit indexing. Never runs an audio or spatial renderer.
#[derive(Clone,Copy,Debug)]
pub struct AudioFrameHeader { pub bytes:usize,pub samples:u32,pub sample_rate:u32,pub advances_clock:bool,pub random_access:bool }
pub fn inspect(codec:&str, bytes:&[u8])->Result<AudioFrameHeader,String>{
 match codec {
  "av3a"=>{let value=avs3a::parse_header(bytes).map_err(|e|e.to_string())?;
   if value.offset!=0{return Err("AV3A index is not on a sync frame".into());}let h=value.header;
   Ok(AudioFrameHeader{bytes:h.frame_len,samples:h.samples_per_channel,sample_rate:h.sample_rate,advances_clock:true,random_access:true})},
  "eac3"|"ac3"=>{let h=if bytes.get(5).is_some_and(|b|b>>3<=10){eac3::parser::parse_legacy_ac3_header(bytes)}else{eac3::parser::parse_header(bytes)}.map_err(|e|format!("AC-3 seek header: {e:?}"))?;
   Ok(AudioFrameHeader{bytes:h.frame_size,samples:h.samples as u32,sample_rate:h.sample_rate,
    advances_clock:h.stream_type!=eac3::parser::StreamType::Dependent&&h.substream_id==0,random_access:true})},
  "ac4"=>{
   if bytes.len()<7||bytes[0]!=0xac||!matches!(bytes[1],0x40|0x41){return Err("invalid AC-4 sync header".into());}
   let mut size=u16::from_be_bytes([bytes[2],bytes[3]])as usize;let mut head=4;
   if size==0xffff{size=(usize::from(bytes[4])<<16)|(usize::from(bytes[5])<<8)|usize::from(bytes[6]);head=7;}
   if size==0{return Err("empty AC-4 frame".into());}
   let toc=macindecode_ac4_bitstream::Ac4Toc::parse(&bytes[head..]).map_err(|e|e.to_string())?;
   let rate=toc.base_sampling_frequency_hz().ok_or("unsupported AC-4 sampling rate")?;
   if rate!=48000{return Err("AC-4 seek currently requires its supported 48 kHz timeline".into());}
   Ok(AudioFrameHeader{bytes:head+size+if bytes[1]==0x41{2}else{0},samples:u32::from(toc.frame_len_base().ok_or("reserved AC-4 frame rate")?),sample_rate:rate,advances_clock:true,random_access:toc.iframe_global})
  },
  _=>Err(format!("{codec} needs a codec-specific restart index")),
 }
}
