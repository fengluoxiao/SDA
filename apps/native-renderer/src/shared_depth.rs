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
