use sda_alac_file_regression::Mp3FileDecoder;
use std::path::{Path,PathBuf};
fn fixture(name:&str)->PathBuf {Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/sda-native/tests/fixtures").join(name)}
fn all(decoder:&mut Mp3FileDecoder)->Vec<f32>{let mut out=vec![];loop{let chunk=decoder.read_interleaved(4096).unwrap();if chunk.is_empty(){break;}out.extend(chunk);}out}
fn check(path:&Path,alac:bool,targets:&[u64]) {
 let open=||if alac{Mp3FileDecoder::open_alac(path).unwrap()}else{Mp3FileDecoder::open(path).unwrap()};
 let mut continuous=open();let pcm=all(&mut continuous);let mut seeked=open();
 for &ms in targets {let frame=(ms*48) as usize;if frame*2>=pcm.len(){continue;}
  let began=std::time::Instant::now();assert_eq!(seeked.seek(ms).unwrap(),frame as u64);let got=seeked.read_interleaved(4800).unwrap();
  let expected=&pcm[frame*2..frame*2+got.len()];let max=got.iter().zip(expected).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);
  eprintln!("seek {ms}ms elapsed={:?},max={max},samples={}",began.elapsed(),got.len());
  assert!(max<3e-5,"seek changed waveform at {ms}ms: {max}");
 }
}
#[test]fn alac_seek_keeps_absolute_clock_and_src_phase(){for file in ["alac/tone-44100-2ch.m4a","alac/tone-96000-2ch-entry1.m4a","loudness/quiet-intro-96000.m4a"]{check(&fixture(file),true,&[701,123,1090,503,0]);}}
#[test]fn seek_after_eof_restores_data_and_original_duration(){let path=fixture("alac/tone-44100-2ch.m4a");let mut d=Mp3FileDecoder::open_alac(path).unwrap();let original=all(&mut d);assert!(d.is_finished());assert_eq!(d.seek(123).unwrap(),5904);let remaining=all(&mut d);assert_eq!(remaining.len(),original.len()-5904*2);assert!(remaining.iter().zip(&original[5904*2..]).all(|(a,b)|(a-b).abs()<3e-5));}
#[test]fn supplied_stereo_seek_audit(){let Ok(path)=std::env::var("SDA_SEEK_STEREO_FILE")else{return;};check(Path::new(&path),true,&[100_013,30_791,150_233,20_017,0]);}
#[test]fn mp3_seek_preserves_reservoir_clock_and_src(){let Ok(path)=std::env::var("SDA_SEEK_MP3_FILE")else{return;};check(Path::new(&path),false,&[7001,123,3091,2017,0]);}
