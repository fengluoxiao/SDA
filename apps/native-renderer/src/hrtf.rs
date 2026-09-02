//! Native-side reader for SDA packed binaural assets.
//!
//! It shares the web manifest format so the future partitioned-convolution
//! engine selects exactly the same 61-direction calibrated dense measurements.

use std::{fs::read, path::{Path, PathBuf}};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    sample_rate: u32,
    calibration_version: Option<u32>,
    processing: Processing,
    positions: Vec<Position>,
}

#[derive(Debug, Deserialize)]
struct Processing {
    calibrated: bool,
}

#[derive(Debug, Deserialize, Clone)]
struct Position {
    azimuth: f64,
    elevation: f64,
    dry: String,
    wet: String,
}

#[derive(Debug)]
pub struct NativeHrtfSet {
    pub sample_rate: u32,
    positions: Vec<Position>,
    root: PathBuf,
}

#[derive(Debug)]
pub struct StereoIr {
    pub azimuth: f64,
    pub elevation: f64,
    /// Packed contiguous `[left][right]` f32 samples, matching web assets.
    pub dry: Vec<f32>,
    pub wet: Vec<f32>,
}

impl NativeHrtfSet {
    pub fn load_calibrated(manifest_path: &Path) -> Result<Self, String> {
        let manifest: Manifest = serde_json::from_slice(&read(manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        if manifest.calibration_version.unwrap_or(0) < 4 || !manifest.processing.calibrated {
            return Err("native object renderer requires calibrated dense HRTF v4".into());
        }
        if manifest.positions.is_empty() { return Err("HRTF manifest has no positions".into()); }
        Ok(Self {
            sample_rate: manifest.sample_rate,
            positions: manifest.positions,
            root: manifest_path.parent().ok_or("manifest has no parent directory")?.to_path_buf(),
        })
    }

    pub fn nearest(&self, azimuth: f64, elevation: f64) -> Result<StereoIr, String> {
        let target = unit(azimuth, elevation);
        let position = self.positions.iter().max_by(|a, b| {
            dot(unit(a.azimuth, a.elevation), target)
                .total_cmp(&dot(unit(b.azimuth, b.elevation), target))
        }).ok_or("HRTF set is empty")?;
        Ok(StereoIr {
            azimuth: position.azimuth,
            elevation: position.elevation,
            dry: read_packed_f32(&self.root.join(&position.dry))?,
            wet: read_packed_f32(&self.root.join(&position.wet))?,
        })
    }
}

fn unit(azimuth: f64, elevation: f64) -> [f64; 3] {
    let az = azimuth.to_radians();
    let el = elevation.to_radians();
    [el.cos() * az.sin(), el.cos() * az.cos(), el.sin()]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }

fn read_packed_f32(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = read(path).map_err(|error| error.to_string())?;
    if bytes.len() == 0 || bytes.len() % 8 != 0 { return Err(format!("invalid packed stereo IR: {}", path.display())); }
    Ok(bytes.chunks_exact(4).map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).collect())
}
