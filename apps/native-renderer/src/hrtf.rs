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
    #[serde(default, rename = "preserveMeasurements")]
    preserve_measurements: bool,
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
    pub cinema: crate::cinema::Settings,
    pub room_profile: Option<std::sync::Arc<crate::cinema::RoomProfile>>,
    speaker_prepared: std::collections::HashMap<(String, String, u32), std::sync::Arc<crate::convolution::PreparedStereoFilter>>,
    pub sample_rate: u32,
    pub subject_id: Option<String>,
    pub complete_subject: bool,
    positions: Vec<Position>,
    cache: Vec<StereoIr>,
    speaker_set: Option<Box<NativeHrtfSet>>,
    prepared: std::collections::HashMap<(i32, i32, u32), crate::convolution::PreparedStereoFilter>,
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
        (
            (azimuth * 1000.0).round() as i32,
            (elevation * 1000.0).round() as i32,
        )
    }

    pub fn load_calibrated(manifest_path: &Path) -> Result<Self, String> {
        let manifest: Manifest =
            serde_json::from_slice(&read(manifest_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let raw_ku100 = manifest.subject_id.as_deref() == Some("ku100")
            && manifest.calibration_version == Some(0)
            && !manifest.processing.calibrated && manifest.processing.preserve_measurements;
        if !raw_ku100 && (manifest.calibration_version.unwrap_or(0) < 4 || !manifest.processing.calibrated) {
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
        // Dense object grids omit some physical speaker anchors (e.g. +/-45
        // degrees overhead). Keep the matching standard set for speaker buses.
        let speaker_set = match root.file_name().and_then(|name| name.to_str()) {
            Some("hrtf-dense") => Some("hrtf"),
            Some("hrtf-dense-raw") => Some("hrtf-raw"),
            _ => None,
        }.map(|name| {
            let parent = root.parent().ok_or("HRTF asset root has no parent")?;
            Self::load_calibrated(&parent.join(name).join("hrtf-set.json")).map(Box::new)
        }).transpose()?;
        Ok(Self {
            cinema: crate::cinema::Settings::default(),
            room_profile: None,
            speaker_prepared: std::collections::HashMap::new(),
            sample_rate: manifest.sample_rate,
            subject_id: manifest.subject_id,
            complete_subject,
            positions: manifest.positions,
            cache,
            speaker_set,
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
        let mut cached = self
            .cache
            .get(index)
            .ok_or("HRTF cache is missing a manifest position")?
            .clone();
        cached.azimuth = position.azimuth;
        cached.elevation = position.elevation;
        Ok(cached)
    }

    pub fn configure_cinema(&mut self, settings: crate::cinema::Settings, profile: Option<std::sync::Arc<crate::cinema::RoomProfile>>) {
        self.cinema = settings;
        self.room_profile = profile;
        self.speaker_prepared.clear();
    }

    pub fn speaker_filter_len(&self) -> usize {
        let base = self.cache.iter().map(|ir| ir.wet.len() / 2).max().unwrap_or(8192)
            .max(self.speaker_set.as_ref().map_or(0, |set| set.speaker_filter_len()));
        let monitor_delay = self.cinema.monitor.max_delay();
        if !self.cinema.enabled { return base + monitor_delay; }
        let room = self.room_profile.as_ref().map_or(0, |p| p.speakers.iter().map(|s| s.room_left.len()).max().unwrap_or(0));
        let delay = self.cinema.speakers.values().map(|s| (s.delay_ms * 48.0).round() as usize).max().unwrap_or(0);
        let tail = if self.cinema.speakers.values().any(|s| s.low_db != 0.0 || s.high_db != 0.0) { 2048 } else { 0 };
        base.max(room) + delay + tail + monitor_delay
    }

    pub fn mixed_speaker(&self, name: &str, layout: &str, azimuth: f64, elevation: f64, wet: f32) -> Result<(Vec<f32>, Vec<f32>), String> {
        let profile = self.room_profile.as_ref().filter(|profile| self.cinema.enabled && profile.layout == layout);
        let pair = if let Some(profile) = profile {
            let speaker = profile.speakers.iter().find(|s| s.name == name).ok_or("room profile missing speaker")?;
            self.cinema.mix((&speaker.direct_left, &speaker.direct_right), (&speaker.room_left, &speaker.room_right),
                if wet == 0.0 { 0.0 } else { 1.0 }, speaker.onset_sample)
        } else {
            let ir = self.speaker_set.as_deref().unwrap_or(self).nearest(azimuth, elevation)?;
            let d = ir.dry.len() / 2;
            let r = ir.wet.len() / 2;
            self.cinema.mix((&ir.dry[..d], &ir.dry[d..]), (&ir.wet[..r], &ir.wet[r..]), wet, 128)
        };
        let (mut left, mut right) = self.cinema.monitor.filter(name, self.cinema.calibrate(name, pair)?);
        left.resize(self.speaker_filter_len(), 0.0);
        right.resize(self.speaker_filter_len(), 0.0);
        Ok((left, right))
    }

    pub fn prepared_speaker(&mut self, name: &str, layout: &str, azimuth: f64, elevation: f64, wet: f32) -> Result<crate::convolution::PreparedStereoFilter, String> {
        let key = (name.to_string(), layout.to_string(), wet.to_bits());
        if let Some(filter) = self.speaker_prepared.get(&key) { return Ok((**filter).clone()); }
        let (left, right) = self.mixed_speaker(name, layout, azimuth, elevation, wet)?;
        let convolver = crate::convolution::StereoPartitionedConvolver::new(&left, &right, crate::convolution::DEFAULT_PARTITION)?;
        let filter = convolver.prepared_filter();
        self.speaker_prepared.insert(key, std::sync::Arc::new(filter.clone()));
        Ok(filter)
    }

    pub fn prepared_focus_speaker(&mut self, name: &str, layout: &str, azimuth: f64, elevation: f64, wet: f32, background: bool) -> Result<std::sync::Arc<crate::convolution::PreparedStereoFilter>, String> {
        let key = (format!("focus:{background}:{name}"), layout.to_string(), wet.to_bits());
        if let Some(filter) = self.speaker_prepared.get(&key) { return Ok(filter.clone()); }
        let (mut left, mut right) = self.mixed_speaker(name, layout, azimuth, elevation, wet)?;
        // The 1.5 kHz one-pole tail falls below 2e-11 within 128 samples.
        // Reserve the same partition for both paths so focus can crossfade.
        for ear in [&mut left, &mut right] {
            ear.resize(self.speaker_filter_len() + crate::convolution::DEFAULT_PARTITION, 0.0);
            if background {
                let mut filter = crate::focus::BackgroundFilter::default();
                for tap in ear { *tap = filter.process(*tap); }
            }
        }
        let filter = crate::convolution::StereoPartitionedConvolver::new(&left, &right, crate::convolution::DEFAULT_PARTITION)?.prepared_filter();
        let filter = std::sync::Arc::new(filter);
        self.speaker_prepared.insert(key, filter.clone());
        Ok(filter)
    }

    pub fn nearest_direction(&self, azimuth: f64, elevation: f64) -> Result<(f64, f64), String> {
        Ok(*self
            .nearest_directions(azimuth, elevation, 1)?
            .first()
            .expect("non-empty direction result"))
    }

    /// Returns the closest distinct measured directions in descending angular
    /// proximity. Native object spread uses these local HRTF taps rather than
    /// smearing energy across the entire measurement set.
    pub fn nearest_directions(
        &self,
        azimuth: f64,
        elevation: f64,
        count: usize,
    ) -> Result<Vec<(f64, f64)>, String> {
        if count == 0 || self.positions.is_empty() {
            return Err("HRTF set is empty".into());
        }
        let target = unit(azimuth, elevation);
        let mut directions: Vec<_> = self
            .positions
            .iter()
            .map(|position| {
                (
                    dot(unit(position.azimuth, position.elevation), target),
                    (position.azimuth, position.elevation),
                )
            })
            .collect();
        directions.sort_by(|left, right| right.0.total_cmp(&left.0));
        Ok(directions
            .into_iter()
            .take(count.min(self.positions.len()))
            .map(|(_, direction)| direction)
            .collect())
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
        let position = self
            .positions
            .iter()
            .find(|position| {
                position.azimuth == measured_azimuth && position.elevation == measured_elevation
            })
            .ok_or("measured HRTF direction is not in this set")?;
        let index = self
            .positions
            .iter()
            .position(|entry| std::ptr::eq(entry, position))
            .expect("position iterator and find refer to the same set");
        let ir = self
            .cache
            .get(index)
            .ok_or("HRTF cache is missing a manifest position")?;
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
        for sample in 0..wet_len {
            let dry_left = if sample < dry_len {
                ir.dry[sample]
            } else {
                0.0
            };
            let dry_right = if sample < dry_len {
                ir.dry[sample + dry_len]
            } else {
                0.0
            };
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
        let (az, el) = Self::direction_key(azimuth, elevation);
        let key = (az, el, wet_weight.clamp(0.0, 1.0).to_bits());
        if let Some(filter) = self.prepared.get(&key) {
            return Ok(filter.clone());
        }
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

#[cfg(test)]
mod raw_tests {
    use super::*;

    #[test]
    fn ku100_reflection_stages_preserve_direct_and_separate_actual_room_tail() {
        use crate::cinema::{Settings,ReflectionMode};
        let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/public");
        for name in ["hrtf-raw","hrtf"] {
            let mut set=NativeHrtfSet::load_calibrated(&root.join(name).join("hrtf-set.json")).unwrap();
            let ir=set.nearest(30.0,0.0).unwrap();
            let baseline=set.mixed_speaker("FrontLeft","7.1.4",30.0,0.0,0.04).unwrap();
            let mut outputs=Vec::new();
            for mode in [ReflectionMode::Direct,ReflectionMode::Early,ReflectionMode::Full] {
                set.configure_cinema(Settings{enabled:true,reflection_mode:mode,..Default::default()},None);
                outputs.push(set.mixed_speaker("FrontLeft","7.1.4",30.0,0.0,0.04).unwrap());
            }
            let dry_len=ir.dry.len()/2;
            for (ear,dry) in [&ir.dry[..dry_len],&ir.dry[dry_len..]].iter().enumerate() {
                let channels:Vec<&Vec<f32>>=outputs.iter().map(|p|if ear==0{&p.0}else{&p.1}).collect();
                assert_eq!(&channels[0][..dry_len],*dry);
                assert!(channels[0][dry_len..].iter().all(|v|*v==0.0));
                assert!(channels[1][2768..].iter().all(|v|*v==0.0));
                assert!(channels[2][2768..].iter().any(|v|v.abs()>1e-8));
                assert!(channels[1].iter().zip(channels[0]).any(|(a,b)|(a-b).abs()>1e-8));
            }
            assert_eq!(outputs[2],baseline,"full stage must keep the existing room amount");
        }
    }

    #[test]
    fn raw_ku100_preserves_samples_and_room_tail() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/public");
        for name in ["hrtf-raw", "hrtf-dense-raw"] {
            let set = NativeHrtfSet::load_calibrated(&root.join(name).join("hrtf-set.json")).unwrap();
            let original = set.nearest(0.0, 0.0).unwrap();
            assert_eq!(original.dry.len(), 512);
            assert_eq!(original.wet.len(), 28800);
            let (_, _, left, right) = set.mixed_nearest(0.0, 0.0, 0.0).unwrap();
            assert_eq!(&left[..256], &original.dry[..256]);
            assert_eq!(&right[..256], &original.dry[256..]);
            assert!(left[256..].iter().all(|v| *v == 0.0));
            let (_, _, wet_left, wet_right) = set.mixed_nearest(0.0, 0.0, 1.0).unwrap();
            for i in 0..14400 {
                assert!((wet_left[i] - original.wet[i]).abs() < 1e-6);
                assert!((wet_right[i] - original.wet[14400 + i]).abs() < 1e-6);
            }
        }
    }
}
