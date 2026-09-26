//! Transparent digital monitor controls, independent of room correction.
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub hardware: crate::hardware::Settings,
    pub enabled: bool,
    pub level_db: f32,
    pub dim: bool,
    pub dim_db: f32,
    pub muted: bool,
    pub bass_enabled: bool,
    pub crossover_hz: f32,
    pub bass_db: f32,
    pub outputs: BTreeMap<String, Output>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            hardware: Default::default(),
            enabled: false,
            level_db: 0.0,
            dim: false,
            dim_db: -20.0,
            muted: false,
            bass_enabled: false,
            crossover_hz: 80.0,
            bass_db: 0.0,
            outputs: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Output {
    pub trim_db: f32,
    pub delay_ms: f32,
    pub invert: bool,
    pub muted: bool,
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        self.hardware.validate()?;
        let bounded = |v: f32, lo, hi| v.is_finite() && v >= lo && v <= hi;
        if !bounded(self.level_db, -80.0, 0.0)
            || !bounded(self.dim_db, -40.0, 0.0)
            || !bounded(self.crossover_hz, 40.0, 160.0)
            || !bounded(self.bass_db, -24.0, 6.0)
            || self.outputs.len() > 32
        {
            return Err("invalid monitor settings".into());
        }
        for (name, output) in &self.outputs {
            if (name != "LFE"
                && name != "LFE2"
                && !crate::vbap::speakers(crate::vbap::LayoutId::Dolby9_1_6)
                    .iter()
                    .chain(crate::vbap::speakers(crate::vbap::LayoutId::Sony360Ra13).iter())
                    .chain(crate::vbap::speakers(crate::vbap::LayoutId::Itu22_2).iter())
                    .chain(crate::vbap::speakers(crate::vbap::LayoutId::Dolby11_1_8).iter())
                    .any(|s| s.name == name))
                || !bounded(output.trim_db, -24.0, 6.0)
                || !bounded(output.delay_ms, 0.0, 20.0)
            {
                return Err("invalid monitor output".into());
            }
        }
        Ok(())
    }
    pub fn gain(&self, name: &str) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        self.outputs.get(name).map_or(1.0, |o| {
            if o.muted {
                0.0
            } else {
                crate::cinema::db(o.trim_db) * if o.invert { -1.0 } else { 1.0 }
            }
        })
    }
    pub fn master_gain(&self) -> f32 {
        if !self.enabled {
            1.0
        } else if self.muted {
            0.0
        } else {
            crate::cinema::db(self.level_db + if self.dim { self.dim_db } else { 0.0 })
        }
    }
    pub fn delay(&self, name: &str) -> usize {
        if !self.enabled {
            0
        } else {
            self.outputs
                .get(name)
                .map_or(0, |o| (o.delay_ms * 48.0).round() as usize)
        }
    }
    pub fn max_delay(&self) -> usize {
        self.outputs
            .keys()
            .map(|n| self.delay(n))
            .max()
            .unwrap_or(0)
    }
    /// Room output alignment is part of the selected virtual room. It retains
    /// speaker trim and arrival alignment, but intentionally excludes monitor
    /// controls such as mute and phase inversion.
    pub fn alignment_filter(&self, name: &str, pair: (Vec<f32>, Vec<f32>)) -> (Vec<f32>, Vec<f32>) {
        let output = self.outputs.get(name);
        let delay = output.map_or(0, |o| (o.delay_ms * 48.0).round() as usize);
        let gain = output.map_or(1.0, |o| crate::cinema::db(o.trim_db));
        let process = |v: Vec<f32>| {
            std::iter::repeat_n(0.0, delay)
                .chain(v.into_iter().map(|x| x * gain))
                .collect()
        };
        (process(pair.0), process(pair.1))
    }
    pub fn max_alignment_delay(&self) -> usize {
        self.outputs
            .values()
            .map(|o| (o.delay_ms * 48.0).round() as usize)
            .max()
            .unwrap_or(0)
    }
    pub fn filter(&self, name: &str, pair: (Vec<f32>, Vec<f32>)) -> (Vec<f32>, Vec<f32>) {
        let delay = self.delay(name);
        let gain = self.gain(name);
        let process = |v: Vec<f32>| {
            std::iter::repeat_n(0.0, delay)
                .chain(v.into_iter().map(|x| x * gain))
                .collect()
        };
        (process(pair.0), process(pair.1))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bypass_and_controls_preserve_ear_relationship() {
        let mut s = Settings::default();
        s.outputs.insert(
            "FrontLeft".into(),
            Output {
                trim_db: -6.0,
                delay_ms: 1.0,
                invert: true,
                muted: false,
            },
        );
        let pair = (vec![1.0, 0.5], vec![-0.25, 0.75]);
        assert_eq!(s.filter("FrontLeft", pair.clone()), pair);
        s.enabled = true;
        let (l, r) = s.filter("FrontLeft", pair);
        assert_eq!(l.len(), 50);
        assert!(l[..48].iter().all(|x| *x == 0.0));
        assert!((l[48] + 0.5011872).abs() < 1e-6);
        assert!((r[48] / l[48] + 0.25).abs() < 1e-6);
        s.dim = true;
        assert!((s.master_gain() - 0.1).abs() < 1e-6);
        s.muted = true;
        assert_eq!(s.master_gain(), 0.0);
        s.outputs.get_mut("FrontLeft").unwrap().delay_ms = f32::NAN;
        assert!(s.validate().is_err());
    }
    #[test]
    fn room_alignment_survives_monitor_bypass_without_monitor_controls() {
        let mut s = Settings::default();
        s.outputs.insert(
            "RearLeft".into(),
            Output {
                trim_db: -6.0,
                delay_ms: 1.0,
                invert: true,
                muted: true,
            },
        );
        let pair = (vec![1.0, 0.5], vec![-0.25, 0.75]);

        let (left, right) = s.alignment_filter("RearLeft", pair.clone());
        assert_eq!(left.len(), 50);
        assert!(left[..48].iter().all(|x| *x == 0.0));
        assert!((left[48] - 0.5011872).abs() < 1e-6);
        assert!((right[48] + 0.1252968).abs() < 1e-6);
        assert_eq!(
            s.filter("RearLeft", pair.clone()),
            pair,
            "monitor controls remain bypassed"
        );

        s.enabled = true;
        let (left, right) = s.filter("RearLeft", pair);
        assert!(left.iter().all(|x| *x == 0.0));
        assert!(right.iter().all(|x| *x == 0.0));
        assert_eq!(s.max_alignment_delay(), 48);
    }
}
