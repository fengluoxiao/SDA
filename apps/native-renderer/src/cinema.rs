//! Measured-room playback and explicit per-speaker calibration, not brand emulation.
use crate::{dsp, vbap};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub reflection_mode: ReflectionMode,
    pub direct_db: f32,
    pub early_db: f32,
    pub late_db: f32,
    pub early_ms: f32,
    pub bass_enabled: bool,
    pub crossover_hz: f32,
    pub bass_db: f32,
    pub speakers: BTreeMap<String, SpeakerCalibration>,
}
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReflectionMode {
    Direct,
    Early,
    #[default]
    Full,
}
impl Default for Settings {
    fn default() -> Self {
        Self { enabled: false, reflection_mode: ReflectionMode::Full, direct_db: 0.0, early_db: 0.0, late_db: 0.0,
            early_ms: 50.0, bass_enabled: false, crossover_hz: 80.0, bass_db: 0.0, speakers: BTreeMap::new() }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SpeakerCalibration {
    pub gain_db: f32,
    pub delay_ms: f32,
    pub low_db: f32,
    pub high_db: f32,
}

fn bounded(value: f32, low: f32, high: f32) -> bool { value.is_finite() && value >= low && value <= high }
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !bounded(self.direct_db, -24.0, 6.0) || !bounded(self.early_db, -40.0, 6.0)
            || !bounded(self.late_db, -40.0, 6.0) || !bounded(self.early_ms, 10.0, 100.0)
            || !bounded(self.crossover_hz, 40.0, 160.0) || !bounded(self.bass_db, -24.0, 6.0)
            || self.speakers.len() > 16 { return Err("invalid cinema settings".into()); }
        for (name, speaker) in &self.speakers {
            let valid_name = name == "LFE" || vbap::speakers(vbap::LayoutId::Dolby9_1_6).iter().any(|s| s.name == name);
            if !valid_name || !bounded(speaker.gain_db, -24.0, 6.0) || !bounded(speaker.delay_ms, 0.0, 20.0)
                || !bounded(speaker.low_db, -6.0, 6.0) || !bounded(speaker.high_db, -6.0, 6.0) {
                return Err("invalid speaker calibration".into());
            }
            if name == "LFE" && (speaker.low_db != 0.0 || speaker.high_db != 0.0) { return Err("LFE calibration supports gain and delay only".into()); }
        }
        Ok(())
    }

    pub fn mix(&self, dry: (&[f32], &[f32]), room: (&[f32], &[f32]), weight: f32, onset: usize) -> (Vec<f32>, Vec<f32>) {
        let length = room.0.len().max(dry.0.len());
        let mut left = vec![0.0; length];
        let mut right = vec![0.0; length];
        let (direct_gain, early_gain, late_gain) = if self.enabled {
            (db(self.direct_db), db(self.early_db), db(self.late_db))
        } else { (1.0, 1.0, 1.0) };
        let boundary = onset as f32 + self.early_ms * 48.0;
        for i in 0..length {
            // A 10 ms complementary window avoids a discontinuity in the IR tail.
            let late = ((i as f32 - boundary + 240.0) / 480.0).clamp(0.0, 1.0);
            let reflection_gain = weight * match if self.enabled { self.reflection_mode } else { ReflectionMode::Full } {
                ReflectionMode::Direct => 0.0,
                ReflectionMode::Early => early_gain * (1.0 - late),
                ReflectionMode::Full => early_gain * (1.0 - late) + late_gain * late,
            };
            for (output, dry, room) in [(&mut left, dry.0, room.0), (&mut right, dry.1, room.1)] {
                let d = dry.get(i).copied().unwrap_or(0.0);
                let r = room.get(i).copied().unwrap_or(0.0);
                output[i] = d * direct_gain + (r - d) * reflection_gain;
            }
        }
        (left, right)
    }

    pub fn calibrate(&self, name: &str, pair: (Vec<f32>, Vec<f32>)) -> Result<(Vec<f32>, Vec<f32>), String> {
        if !self.enabled { return Ok(pair); }
        let Some(settings) = self.speakers.get(name) else { return Ok(pair); };
        let delay = (settings.delay_ms * 48.0).round() as usize;
        let process = |input: Vec<f32>| -> Result<Vec<f32>, String> {
            let mut low = dsp::Biquad::lowshelf(48000, 120.0, 0.707, settings.low_db)?;
            let mut high = dsp::Biquad::highshelf(48000, 4000.0, 0.707, settings.high_db)?;
            let mut output = vec![0.0; input.len() + delay + if settings.low_db != 0.0 || settings.high_db != 0.0 { 2048 } else { 0 }];
            for i in delay..output.len() {
                output[i] = high.process(low.process(input.get(i - delay).copied().unwrap_or(0.0))) * db(settings.gain_db);
            }
            Ok(output)
        };
        Ok((process(pair.0)?, process(pair.1)?))
    }
}
pub fn db(value: f32) -> f32 { 10.0_f32.powf(value / 20.0) }

pub struct BassSplit {
    pub frequency: f32,
    low: dsp::Lr4Lowpass,
    high: [dsp::Biquad; 2],
}
impl BassSplit {
    pub fn new(frequency: f32) -> Result<Self, String> {
        Ok(Self { frequency, low: dsp::Lr4Lowpass::new(48000, frequency)?,
            high: [dsp::Biquad::butterworth_highpass(48000, frequency)?; 2] })
    }
    pub fn process(&mut self, input: f32) -> (f32, f32) {
        let low = self.low.process(input);
        let first = self.high[0].process(input);
        let high = self.high[1].process(first);
        (low, high)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomProfile {
    pub version: u32,
    pub name: String,
    pub source: String,
    pub license: String,
    pub measurement: String,
    pub sample_rate: u32,
    pub layout: String,
    pub speakers: Vec<RoomSpeaker>,
    #[serde(default)]
    pub simulation: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomSpeaker {
    pub name: String,
    pub azimuth: f32,
    pub elevation: f32,
    pub onset_sample: usize,
    pub direct_left: Vec<f32>,
    pub direct_right: Vec<f32>,
    pub room_left: Vec<f32>,
    pub room_right: Vec<f32>,
}
impl RoomProfile {
    pub fn load(path: &str) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if meta.len() > 64 * 1024 * 1024 { return Err("room profile exceeds 64 MB".into()); }
        let profile: Self = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        profile.validate()?;
        Ok(profile)
    }
    pub fn validate(&self) -> Result<(), String> {
        let layout = vbap::LayoutId::parse(&self.layout).ok_or("invalid room layout")?;
        let expected = vbap::speakers(layout);
        if self.measurement == "simulated" && !self.simulation.as_ref().is_some_and(|v|v.is_object()) {
            return Err("simulated room requires provenance".into());
        }
        if self.version != 1 || self.sample_rate != 48000 || self.name.trim().is_empty() || self.name.len() > 256
            || self.source.trim().is_empty() || self.license.trim().is_empty()
            || !["personal", "dummy-head", "simulated"].contains(&self.measurement.as_str()) || self.speakers.len() != expected.len() {
            return Err("invalid room provenance or channel coverage".into());
        }
        for speaker in expected {
            let entries: Vec<_> = self.speakers.iter().filter(|s| s.name == speaker.name).collect();
            if entries.len() != 1 { return Err("room requires one response per layout speaker".into()); }
            let s = entries[0];
            if !s.azimuth.is_finite() || !s.elevation.is_finite() || (s.azimuth - speaker.azimuth).abs() > 1.0 || (s.elevation - speaker.elevation).abs() > 1.0 {
                return Err("room measurement directions do not match the layout".into());
            }
            let length = s.room_left.len();
            if !(512..=32768).contains(&length) || s.room_right.len() != length || s.direct_left.len() != length
                || s.direct_right.len() != length || s.onset_sample >= length {
                return Err("room responses must have matching lengths (512..32768 taps)".into());
            }
            for values in [&s.direct_left, &s.direct_right, &s.room_left, &s.room_right] {
                if !values.iter().all(|v| v.is_finite() && v.abs() <= 16.0) || !values.iter().any(|v| v.abs() > 1e-9) {
                    return Err("invalid or silent room response".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cinema_neutral_mix_and_calibration_preserve_interaural_delay() {
        let mut dry_l = vec![0.0; 512]; let mut dry_r = dry_l.clone();
        dry_l[10] = 1.0; dry_r[17] = 0.5;
        let mut room_l = dry_l.clone(); let mut room_r = dry_r.clone();
        room_l[300] = 0.2; room_r[320] = 0.1;
        let settings = Settings::default();
        let pair = settings.mix((&dry_l,&dry_r),(&room_l,&room_r),1.0,10);
        assert_eq!(pair, (room_l.clone(),room_r.clone()));
        let mut settings = Settings {enabled:true,..Settings::default()};
        settings.speakers.insert("FrontLeft".into(),SpeakerCalibration {gain_db:-6.0,delay_ms:2.0,..SpeakerCalibration::default()});
        let (left,right) = settings.calibrate("FrontLeft",(dry_l,dry_r)).unwrap();
        assert!((left[106]-db(-6.0)).abs()<1e-6);
        assert!((right[113]-0.5*db(-6.0)).abs()<1e-6);
        assert!(left[..106].iter().all(|x|*x==0.0));
    }
    #[test]
    fn cinema_crossovers_are_complementary_and_reject_invalid_values() {
        for frequency in [20.0,80.0,1000.0] {
            let mut split = BassSplit::new(80.0).unwrap();
            let mut energy=0.0;
            for i in 0..96000 {
                let input=(std::f32::consts::TAU*frequency*i as f32/48000.0).sin();
                let (low,high)=split.process(input);
                if i>=48000 {energy+=(low+high).powi(2);}
            }
            assert!((energy/48000.0-0.5).abs()<0.015,"frequency={frequency}");
        }
        let mut settings=Settings::default();settings.early_ms=f32::NAN;
        assert!(settings.validate().is_err());
        assert!(BassSplit::new(0.0).is_err());
    }
    #[test]
    fn cinema_audition_removes_reflections_without_altering_direct_ir() {
        let mut direct=vec![0.0;8192];direct[128]=1.0;direct[5000]=0.02;
        let mut room=direct.clone();room[700]+=0.5;room[6000]+=0.25;
        let mut settings=Settings {enabled:true,reflection_mode:ReflectionMode::Direct,..Settings::default()};
        assert_eq!(settings.mix((&direct,&direct),(&room,&room),1.0,128),(direct.clone(),direct.clone()));
        settings.reflection_mode=ReflectionMode::Early;
        let (early,_)=settings.mix((&direct,&direct),(&room,&room),1.0,128);
        assert_eq!(early[700],0.5);
        assert_eq!(early[6000],0.0);
        assert_eq!(early[5000],0.02);
        let old:Settings=serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(matches!(old.reflection_mode,ReflectionMode::Full));
        assert!(serde_json::from_str::<Settings>(r#"{"reflectionMode":"invalid"}"#).is_err());
    }
    #[test]
    fn cinema_reflection_controls_leave_direct_sound_intact() {
        let mut dry=vec![0.0;8192];dry[128]=1.0;
        let mut room=dry.clone();room[700]=0.5;room[6000]=0.25;
        let settings=Settings {enabled:true,early_db:-6.0,late_db:-20.0,..Settings::default()};
        let (left,_)=settings.mix((&dry,&dry),(&room,&room),1.0,128);
        assert_eq!(left[128],1.0);
        assert!((left[700]-0.5*db(-6.0)).abs()<1e-6);
        assert!((left[6000]-0.025).abs()<1e-6);
    }
}
