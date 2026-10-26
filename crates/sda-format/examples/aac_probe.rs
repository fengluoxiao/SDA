use std::{fs,io::Write};
use sda_format::aac::AacDecoder;
fn main()->Result<(),String>{
 let args=std::env::args().collect::<Vec<_>>();let bytes=fs::read(args.get(1).ok_or("input ADTS required")?).map_err(|e|e.to_string())?;
 let mut at=0;let mut decoder=None;let mut frames=0usize;let mut packets=0;let mut first_positions=vec![];let mut values=Vec::new();let mut rate=0;let mut channels=0;
 while at+7<=bytes.len(){let h=&bytes[at..];if h[0]!=0xff||h[1]&0xf6!=0xf0{return Err(format!("invalid ADTS at {at}"));}let length=(((h[3]&3)as usize)<<11)|((h[4]as usize)<<3)|((h[5]as usize)>>5);let head=if h[1]&1!=0{7}else{9};if length<head||at+length>bytes.len(){return Err("truncated ADTS".into());}
 let sr=(h[2]>>2)&15;let ch=((h[2]&1)<<2)|(h[3]>>6);let object=(h[2]>>6)+1;
 if decoder.is_none(){let asc=[(object<<3)|(sr>>1),((sr&1)<<7)|(ch<<3)];let d=AacDecoder::new(&asc)?;rate=d.sample_rate;channels=d.channels;decoder=Some(d);}
 let output=decoder.as_mut().unwrap().decode(&h[head..length])?;if !output.positions.is_empty()&&first_positions.is_empty(){first_positions=output.positions.clone();}
 let count=output.channels[0].len();frames+=count;packets+=1;for i in 0..count{for ch in &output.channels{values.extend_from_slice(&ch[i].to_le_bytes());}}
 at+=length;
 }
 if let Some(path)=args.get(2){let mut file=fs::File::create(path).map_err(|e|e.to_string())?;file.write_all(&values).map_err(|e|e.to_string())?;}
 println!("{}",serde_json::json!({"rate":rate,"channels":channels,"positions":first_positions,"packets":packets,"frames":frames,"decodedBytes":values.len()}));Ok(())
}
