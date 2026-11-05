//! Header-only access-unit indexing. Never runs an audio or spatial renderer.
#[derive(Clone,Copy,Debug)]
pub struct AudioFrameHeader { pub bytes:usize,pub samples:u32,pub sample_rate:u32,pub advances_clock:bool,pub random_access:bool }
pub fn inspect(codec:&str, bytes:&[u8])->Result<AudioFrameHeader,String>{ inspect_with_rate(codec,bytes,0) }
pub fn inspect_with_rate(codec:&str, bytes:&[u8], previous_rate:u32)->Result<AudioFrameHeader,String>{
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
  "truehd"=>{
   if bytes.len()>=6&&bytes[..2]==[0x0b,0x77]{let mut h=inspect("eac3",bytes)?;h.advances_clock=false;h.random_access=false;return Ok(h);}
   if bytes.len()<8{return Err("truncated TrueHD access-unit header".into());}
   let size=usize::from(u16::from_be_bytes([bytes[0],bytes[1]])&0xfff)*2;
   if size<8{return Err("invalid TrueHD access-unit size".into());}
   let major=bytes[4..8]==[0xf8,0x72,0x6f,0xba]||bytes[4..8]==[0xf8,0x72,0x6f,0xbb];
   let rate=if major {if bytes.len()<10{return Err("truncated TrueHD major sync".into());}let code=if bytes[7]==0xba{bytes[8]>>4}else{bytes[9]>>4};match code{0..=2=>48000u32<<code,8..=10=>44100u32<<(code-8),_=>return Err("invalid TrueHD sample rate".into())}}else{previous_rate};
   if rate==0{return Err("TrueHD requires a major sync before dependent access units".into());}
   Ok(AudioFrameHeader{bytes:size,samples:40*(rate/44100),sample_rate:rate,advances_clock:true,random_access:major})
  },
  "dts"=>{
   if bytes.starts_with(&[0x64,0x58,0x20,0x25]){
    if bytes.len()<12{return Err("truncated DTS-HD size header".into());}
    let bits=|start:usize,n:usize|->usize{(start..start+n).fold(0,|v,i|(v<<1)|usize::from((bytes[i/8]>>(7-i%8))&1))};
    let wide=bits(42,1);let size=bits(51+wide*4,16+wide*4)+1;
    Ok(AudioFrameHeader{bytes:size,samples:0,sample_rate:previous_rate,advances_clock:false,random_access:bits(67+wide*8,1)!=0})
   }else{let info=dca::parse_header(bytes).map_err(|e|format!("DTS seek header: {e:?}"))?;Ok(AudioFrameHeader{bytes:info.frame_size,samples:info.samples_per_channel() as u32,sample_rate:info.sample_rate,advances_clock:true,random_access:true})}
  },
  _=>Err(format!("{codec} needs a codec-specific restart index")),
 }
}
