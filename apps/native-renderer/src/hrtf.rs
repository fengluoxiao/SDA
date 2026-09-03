//! Native-side reader for SDA packed binaural assets.
//!
//! It shares the web manifest format so the future partitioned-convolution
//! engine selects exactly the same 61-direction calibrated dense measurements.

use serde::Deserialize;
use std::{fs::read, path::Path};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    sample_rate: u32,
    calibration_version: Option<u32>,
    complete_subject: Option<bool>,
    subject_id: Option<String>,
    processing: Processing,
    positions: Vec<Position>,
}

#[derive(Debug, Deserialize)]
struct Processing {
    calibrated: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Position {
    pub azimuth: f64,
    pub elevation: f64,
    dry: String,
    wet: String,
}

#[derive(Debug, Clone)]
pub struct NativeHrtfSet {
    pub sample_rate: u32,
    pub subject_id: Option<String>,
    pub complete_subject: bool,
    positions: Vec<Position>,
    cache: Vec<StereoIr>,
    prepared: std::collections::HashMap<(i32, i32), crate::convolution::PreparedStereoFilter>,
}

#[derive(Debug, Clone)]
pub struct StereoIr {
    pub azimuth: f64,
    pub elevation: f64,
    /// Packed contiguous `[left][right]` f32 samples, matching web assets.
    pub dry: Vec<f32>,
    pub wet: Vec<f32>,
}

impl NativeHrtfSet {
    fn direction_key(azimuth: f64, elevation: f64) -> (i32, i32) {
        ((azimuth * 1000.0).round() as i32, (elevation * 1000.0).round() as i32)
    }

    pub fn load_calibrated(manifest_path: &Path) -> Result<Self, String> {
        let manifest: Manifest =
            serde_json::from_slice(&read(manifest_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        if manifest.calibration_version.unwrap_or(0) < 4 || !manifest.processing.calibrated {
            return Err("native object renderer requires calibrated HRTF v4".into());
        }
        let is_dense_ku100 = manifest.positions.len() == 61;
        let complete_subject = manifest.complete_subject == Some(true);
        if !is_dense_ku100 && !complete_subject {
            return Err("native object renderer requires a complete-subject HRTF or the calibrated KU100 dense set".into());
        }
        if manifest.positions.is_empty() {
            return Err("HRTF manifest has no positions".into());
        }
        let root = manifest_path
            .parent()
            .ok_or("manifest has no parent directory")?
            .to_path_buf();
        let cache = manifest
            .positions
            .iter()
            .map(|position| {
                Ok(StereoIr {
                    azimuth: position.azimuth,
                    elevation: position.elevation,
                    dry: read_packed_f32(&root.join(&position.dry))?,
                    wet: read_packed_f32(&root.join(&position.wet))?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            sample_rate: manifest.sample_rate,
            subject_id: manifest.subject_id,
            complete_subject,
            positions: manifest.positions,
            cache,
            prepared: std::collections::HashMap::new(),
        })
    }

    pub fn nearest(&self, azimuth: f64, elevation: f64) -> Result<StereoIr, String> {
        let target = unit(azimuth, elevation);
        let (index, position) = self
            .positions
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                dot(unit(a.azimuth, a.elevation), target)
                    .total_cmp(&dot(unit(b.azimuth, b.elevation), target))
            })
            .ok_or("HRTF set is empty")?;
        let mut cached = self.cache.get(index).ok_or("HRTF cache is missing a manifest position")?.clone();
        cached.azimuth = position.azimuth;
        cached.elevation = position.elevation;
        Ok(cached)
    }

    pub fn nearest_direction(&self, azimuth: f64, elevation: f64) -> Result<(f64, f64), String> {
        let target = unit(azimuth, elevation);
        let position = self
            .positions
            .iter()
            .max_by(|a, b| {
                dot(unit(a.azimuth, a.elevation), target)
                    .total_cmp(&dot(unit(b.azimuth, b.elevation), target))
            })
            .ok_or("HRTF set is empty")?;
        Ok((position.azimuth, position.elevation))
    }

    /// Builds the calibrated runtime filter `dry + wet_weight * (wet - dry)`.
    /// No room filtering or energy normalization is repeated at runtime.
    pub fn mixed_nearest(
        &self,
        azimuth: f64,
        elevation: f64,
        wet_weight: f32,
    ) -> Result<(f64, f64, Vec<f32>, Vec<f32>), String> {
        let ir = self.nearest(azimuth, elevation)?;
        if ir.dry.len() % 2 != 0 || ir.wet.len() % 2 != 0 {
            return Err("packed stereo HRTF length is invalid".into());
        }
        let dry_len = ir.dry.len() / 2;
        let wet_len = ir.wet.len() / 2;
        if wet_len < dry_len {
            return Err("wet HRTF is shorter than dry HRTF".into());
        }
        let weight = wet_weight.clamp(0.0, 1.0);
        let mut left = vec![0.0; wet_len];
        let mut right = vec![0.0; wet_len];
        for index in 0..wet_len {
            let dry_left = if index < dry_len { ir.dry[index] } else { 0.0 };
            let dry_right = if index < dry_len {
                ir.dry[index + dry_len]
            } else {
                0.0
            };
            left[index] = dry_left + weight * (ir.wet[index] - dry_left);
            right[index] = dry_right + weight * (ir.wet[index + wet_len] - dry_right);
        }
        Ok((ir.azimuth, ir.elevation, left, right))
    }

    pub fn mixed_direction(
        &self,
        measured_azimuth: f64,
        measured_elevation: f64,
        wet_weight: f32,
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        let position = self.positions.iter()
            .find(|position| position.azimuth == measured_azimuth && position.elevation == measured_elevation)
            .ok_or("measured HRTF direction is not in this set")?;
        let index = self.positions.iter().position(|entry| std::ptr::eq(entry, position))
            .expect("position iterator and find refer to the same set");
        let ir = self.cache.get(index).ok_or("HRTF cache is missing a manifest position")?;
        if ir.dry.len() % 2 != 0 || ir.wet.len() % 2 != 0 {
            return Err("packed stereo HRTF length is invalid".into());
        }
        let dry_len = ir.dry.len() / 2;
        let wet_len = ir.wet.len() / 2;
        if wet_len < dry_len { return Err("wet HRTF is shorter than dry HRTF".into()); }
        let weight = wet_weight.clamp(0.0, 1.0);
        let mut left = vec![0.0; wet_len];
        let mut right = vec![0.0; wet_len];
        for sample in 0..wet_len {
            let dry_left = if sample < dry_len { ir.dry[sample] } else { 0.0 };
            let dry_right = if sample < dry_len { ir.dry[sample + dry_len] } else { 0.0 };
            left[sample] = dry_left + weight * (ir.wet[sample] - dry_left);
            right[sample] = dry_right + weight * (ir.wet[sample + wet_len] - dry_right);
        }
        Ok((left, right))
    }

    /// Returns a prepared FFT filter for an exact measured direction, caching
    /// the partition spectra so direction changes do not allocate or re-plan.
    pub fn prepared_direction(
        &mut self,
        azimuth: f64,
        elevation: f64,
        wet_weight: f32,
    ) -> Result<crate::convolution::PreparedStereoFilter, String> {
        let key = Self::direction_key(azimuth, elevation);
        if let Some(filter) = self.prepared.get(&key) { return Ok(filter.clone()); }
        let (left, right) = self.mixed_direction(azimuth, elevation, wet_weight)?;
        let convolver = crate::convolution::StereoPartitionedConvolver::new(
            &left,
            &right,
            crate::convolution::DEFAULT_PARTITION,
        )?;
        let filter = convolver.prepared_filter();
        self.prepared.insert(key, filter.clone());
        Ok(filter)
    }
}

fn unit(azimuth: f64, elevation: f64) -> [f64; 3] {
    let az = azimuth.to_radians();
    let el = elevation.to_radians();
    [el.cos() * az.sin(), el.cos() * az.cos(), el.sin()]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn read_packed_f32(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = read(path).map_err(|error| error.to_string())?;
    if bytes.len() == 0 || bytes.len() % 8 != 0 {
        return Err(format!("invalid packed stereo IR: {}", path.display()));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect())
}
