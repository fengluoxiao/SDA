//! Fixed, optional 8351B reference-room coefficients. These are generated from
//! normalized manufacturer directivity and an assumed room, not personal BRIRs.
use crate::{cinema::{RoomProfile,RoomSpeaker},vbap};
use serde::Deserialize;
use std::{collections::BTreeMap,sync::{Arc,OnceLock}};
pub const ID:&str="genelec-8351b";
pub const ALGORITHM:&str="genelec8351b-reference-room-v1";
pub const FILTER_CAPACITY:usize=6144;
const DATA:&[u8]=include_bytes!("../assets/genelec-reference/coefficients.f32");
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Manifest{version:u32,sample_rate:u32,taps:usize,source_model:String,phase_model:String,common_residual_gain:f32,layouts:BTreeMap<String,Vec<Entry>>,responses:Vec<Response>}
#[derive(Deserialize)]struct Entry{name:String,key:String}
#[derive(Deserialize)]#[serde(rename_all="camelCase")]
struct Response{key:String,azimuth:f32,elevation:f32,offset_floats:usize,direct_taps:usize,residual_taps:usize}
fn manifest()->Result<&'static Manifest,String>{
 static BANK:OnceLock<Result<Manifest,String>>=OnceLock::new();
 BANK.get_or_init(||{
  let m:Manifest=serde_json::from_str(include_str!("../assets/genelec-reference/manifest.json")).map_err(|e|e.to_string())?;
  if m.version!=1||m.sample_rate!=48000||m.taps!=4608||DATA.len()%4!=0||m.source_model!="genelec-8351b-normalized-directivity"{return Err("Invalid reference-room bank".into());}
  for r in &m.responses{let count=r.direct_taps.checked_add(r.residual_taps).and_then(|n|n.checked_mul(2)).ok_or("Reference bank length overflow")?;
   if r.direct_taps>m.taps||r.residual_taps!=m.taps||r.offset_floats.checked_add(count).is_none_or(|n|n>DATA.len()/4){return Err("Reference bank range outside coefficients".into());}}
  Ok(m)
 }).as_ref().map_err(Clone::clone)
}
fn floats(offset:usize,count:usize)->Vec<f32>{DATA[offset*4..(offset+count)*4].chunks_exact(4).map(|x|f32::from_le_bytes(x.try_into().unwrap())).collect()}
pub fn is_profile(p:&RoomProfile)->bool{p.measurement=="simulated"&&p.simulation.as_ref().and_then(|v|v.get("referenceMonitor")).and_then(|v|v.as_str())==Some(ID)}
pub fn strength(p:&RoomProfile)->f32{p.simulation.as_ref().and_then(|v|v.get("referenceStrength")).and_then(|v|v.as_f64()).unwrap_or(1.)as f32}
pub fn profile(layout:vbap::LayoutId,strength:f32)->Result<Arc<RoomProfile>,String>{
 if !strength.is_finite()||!(0.0..=2.0).contains(&strength){return Err("Invalid reference-room strength".into());}
 let m=manifest()?;let entries=m.layouts.get(layout.as_str()).ok_or("Reference room does not cover layout")?;let mut speakers=Vec::with_capacity(entries.len());
 for entry in entries{
  let r=m.responses.iter().find(|r|r.key==entry.key).ok_or("Missing reference response")?;
  let direct=floats(r.offset_floats,r.direct_taps*2);let residual=floats(r.offset_floats+r.direct_taps*2,r.residual_taps*2);
  let mut dl=vec![0.;m.taps];let mut dr=dl.clone();dl[..r.direct_taps].copy_from_slice(&direct[..r.direct_taps]);dr[..r.direct_taps].copy_from_slice(&direct[r.direct_taps..]);
  let room_left=dl.iter().zip(&residual[..m.taps]).map(|(d,r)|d+r*strength).collect();let room_right=dr.iter().zip(&residual[m.taps..]).map(|(d,r)|d+r*strength).collect();
  let peak=|v:&[f32]|v.iter().enumerate().max_by(|a,b|a.1.abs().total_cmp(&b.1.abs())).map_or(0,|x|x.0);
  let onset_sample=peak(&dl).min(peak(&dr));speakers.push(RoomSpeaker{name:entry.name.clone(),azimuth:r.azimuth,elevation:r.elevation,onset_sample,direct_left:dl,direct_right:dr,room_left,room_right});
 }
 let p=RoomProfile{version:1,name:"8351B 参考监听室".into(),source:"Genelec 8351B normalized directivity + SDA assumed reference room; shared HRTFHQ receiver".into(),license:"Generated reference simulation; see genelec-reference/NOTICE.md and original HRTF notices".into(),measurement:"simulated".into(),sample_rate:48000,layout:layout.as_str().into(),speakers,simulation:Some(serde_json::json!({"sourceModel":m.source_model,"phaseModel":m.phase_model,"referenceMonitor":ID,"referenceAlgorithm":std::hint::black_box(ALGORITHM),"preserveSelectedDirect":true,"referenceStrength":strength,"commonResidualGain":m.common_residual_gain,"roomMetres":[6,5,3.2],"imageOrder":2,"notPersonalMeasurement":true}))};
 p.validate()?;Ok(Arc::new(p))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn all_supported_layouts_have_complete_finite_reference_responses(){
  for name in ["2.0","2.1","5.1","5.1.2","5.1.4","7.1","7.1.2","7.1.4","9.1","9.1.2","9.1.4","9.1.6","360RA-13","22.2","11.1.8"]{
   let l=vbap::LayoutId::parse(name).unwrap();let p=profile(l,1.).unwrap();assert_eq!(p.speakers.len(),vbap::speakers(l).len());assert!(is_profile(&p));
   for s in &p.speakers{assert!(s.direct_left.iter().chain(&s.direct_right).chain(&s.room_left).chain(&s.room_right).all(|v|v.is_finite()));assert!(s.room_left.iter().zip(&s.direct_left).any(|(r,d)|r!=d));}
   if l==vbap::LayoutId::Sony360Ra13{assert_eq!(p.speakers.iter().filter(|s|s.elevation<0.).count(),3);}
  }
 }
 #[test]fn reference_strength_preserves_direct_and_only_scales_residual(){let a=profile(vbap::LayoutId::Dolby7_1_4,0.).unwrap();let b=profile(vbap::LayoutId::Dolby7_1_4,1.).unwrap();let c=profile(vbap::LayoutId::Dolby7_1_4,2.).unwrap();for((a,b),c)in a.speakers.iter().zip(&b.speakers).zip(&c.speakers){assert_eq!(a.direct_left,b.direct_left);assert_eq!(a.room_left,a.direct_left);for((x,y),z)in a.room_left.iter().zip(&b.room_left).zip(&c.room_left){assert!((z-x-2.*(y-x)).abs()<2e-7);}}for s in [-1.,2.01,f32::NAN,f32::INFINITY]{assert!(profile(vbap::LayoutId::Dolby7_1_4,s).is_err());}}
}
