//! User-approved retained61 residual only. Measured HRTFHQ direct responses stay intact.
//! Fixed banks are prepared outside the callback and use the existing reflection buses.
use crate::vbap::{self, LayoutId};
use serde::Deserialize;
use std::sync::{Arc, OnceLock};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: u32,
    sample_rate: u32,
    layout: String,
    speakers: Vec<Speaker>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Speaker {
    name: String,
    azimuth: f64,
    elevation: f64,
    offset_floats: usize,
    taps: usize,
}
#[derive(Debug)]
pub(crate) struct Bank {
    manifest: Manifest,
    samples: Vec<f32>,
    max_taps: usize,
}
impl Bank {
    pub(crate) fn max_taps(&self) -> usize {
        self.max_taps
    }
    pub(crate) fn add(
        &self,
        name: &str,
        layout: &str,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), String> {
        if layout != self.manifest.layout {
            return Err("Shared HRTFHQ depth profile v1: layout mismatch".into());
        }
        let s = self
            .manifest
            .speakers
            .iter()
            .find(|s| s.name == name)
            .ok_or("Shared HRTFHQ depth profile v1: missing speaker")?;
        if left.len() < s.taps || right.len() < s.taps {
            return Err("Shared HRTFHQ depth profile v1: short output".into());
        }
        for (ear, out) in [left, right].into_iter().enumerate() {
            for (dst, src) in out.iter_mut().zip(
                &self.samples[s.offset_floats + ear * s.taps..s.offset_floats + (ear + 1) * s.taps],
            ) {
                *dst += src;
            }
        }
        Ok(())
    }
}
fn decode(layout: LayoutId, json: &str, bytes: &[u8]) -> Result<Arc<Bank>, String> {
    let m: Manifest =
        serde_json::from_str(json).map_err(|e| format!("Shared HRTFHQ depth profile v1: {e}"))?;
    if m.version != 1
        || m.sample_rate != 48000
        || m.layout != layout.as_str()
        || bytes.len() % 4 != 0
        || m.speakers.len() != vbap::speakers(layout).len()
    {
        return Err("Shared HRTFHQ depth profile v1: invalid bank".into());
    }
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if samples.iter().any(|x| !x.is_finite()) {
        return Err("Shared HRTFHQ depth profile v1: nonfinite response".into());
    }
    let mut expected = 0;
    let mut max_taps = 0;
    for (s, anchor) in m.speakers.iter().zip(vbap::speakers(layout)) {
        if s.name != anchor.name
            || s.azimuth != anchor.azimuth as f64
            || s.elevation != anchor.elevation as f64
            || s.offset_floats != expected
            || s.taps == 0
            || s.taps > 16384
        {
            return Err("Shared HRTFHQ depth profile v1: invalid geometry".into());
        }
        expected += 2 * s.taps;
        max_taps = max_taps.max(s.taps);
    }
    if expected != samples.len() {
        return Err("Shared HRTFHQ depth profile v1: invalid response length".into());
    }
    Ok(Arc::new(Bank {
        manifest: m,
        samples,
        max_taps,
    }))
}
pub(crate) fn bank(layout: LayoutId) -> Result<Option<Arc<Bank>>, String> {
    static ATMOS: OnceLock<Result<Arc<Bank>, String>> = OnceLock::new();
    static SONY: OnceLock<Result<Arc<Bank>, String>> = OnceLock::new();
    let result = match layout {
        LayoutId::Dolby7_1_4 => ATMOS.get_or_init(|| {
            decode(
                layout,
                include_str!("../assets/shared-depth/atmos714.json"),
                include_bytes!("../assets/shared-depth/atmos714.f32"),
            )
        }),
        LayoutId::Sony360Ra13 => SONY.get_or_init(|| {
            decode(
                layout,
                include_str!("../assets/shared-depth/sony360ra13.json"),
                include_bytes!("../assets/shared-depth/sony360ra13.f32"),
            )
        }),
        _ => return Ok(None),
    };
    result.clone().map(Some)
}

/// One SDA Spatial bank: keep the accepted short residual and add the fixed
/// monitor-directionality reflections. Older authored manifests keep v1 alone.
pub(crate) fn merged_bank(layout: LayoutId) -> Result<Option<Arc<Bank>>, String> {
    if matches!(layout,LayoutId::Stereo2_0|LayoutId::Stereo2_1){return bank(layout);}
    static BANKS: OnceLock<std::sync::Mutex<std::collections::HashMap<&'static str,Arc<Bank>>>>=OnceLock::new();
    let cache=BANKS.get_or_init(Default::default);
    if let Some(bank)=cache.lock().map_err(|_|"SDA Spatial bank lock poisoned")?.get(layout.as_str()).cloned(){return Ok(Some(bank));}
    let old=bank(layout)?;
    let reference=crate::reference_room::profile(layout,1.)?;
    let taps=old.as_ref().map_or(4608,|b|b.max_taps.max(4608));
    let mut samples=Vec::new();let mut speakers=Vec::new();
    for source in &reference.speakers {
        let mut left=vec![0.;taps];let mut right=left.clone();
        if let Some(old)=&old {old.add(&source.name,layout.as_str(),&mut left,&mut right)?;}
        for (out,room,direct) in [(&mut left,&source.room_left,&source.direct_left),(&mut right,&source.room_right,&source.direct_right)] {
            for ((out,room),direct) in out.iter_mut().zip(room).zip(direct){*out+=room-direct;}
        }
        speakers.push(Speaker{name:source.name.clone(),azimuth:source.azimuth as f64,elevation:source.elevation as f64,offset_floats:samples.len(),taps});
        samples.extend(left);samples.extend(right);
    }
    let merged=Arc::new(Bank{manifest:Manifest{version:2,sample_rate:48000,layout:layout.as_str().into(),speakers},samples,max_taps:taps});
    cache.lock().map_err(|_|"SDA Spatial bank lock poisoned")?.insert(layout.as_str(),merged.clone());
    Ok(Some(merged))
}
#[cfg(test)]mod merged_tests {
 use super::*;
 #[test]fn integrated_bank_keeps_the_original_residual_and_adds_the_monitor_component_once(){
  for layout in [LayoutId::Dolby7_1_4,LayoutId::Sony360Ra13,LayoutId::parse("22.2").unwrap()] {
   let merged=merged_bank(layout).unwrap().unwrap();let old=bank(layout).unwrap();let reference=crate::reference_room::profile(layout,1.).unwrap();
   for source in &reference.speakers {
    let(mut left,mut right)=(vec![0.;merged.max_taps()],vec![0.;merged.max_taps()]);
    merged.add(&source.name,layout.as_str(),&mut left,&mut right).unwrap();
    let(mut before_l,mut before_r)=(vec![0.;merged.max_taps()],vec![0.;merged.max_taps()]);
    if let Some(old)=&old {old.add(&source.name,layout.as_str(),&mut before_l,&mut before_r).unwrap();}
    for ((out,before),(room,direct)) in left.iter().zip(&before_l).zip(source.room_left.iter().zip(&source.direct_left)) {assert!((out-before-(room-direct)).abs()<1e-7);}
    assert!(left.iter().zip(&before_l).any(|(a,b)|(a-b).abs()>1e-7));
    assert!(left.iter().chain(&right).all(|x|x.is_finite()));
   }
   assert!(Arc::ptr_eq(&merged,&merged_bank(layout).unwrap().unwrap()));
  }
 }
}
