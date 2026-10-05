//! Opted-in, versioned short spatial cues shared by every native platform.
//! These are derived measurements, NOT source-distance metadata or Apple emulation.
use crate::{cinema, vbap::LayoutId};
use std::sync::{Arc, OnceLock};

pub fn settings() -> cinema::Settings {
    cinema::Settings {
        enabled: true,
        reflection_mode: cinema::ReflectionMode::Early,
        direct_db: 0.0,
        early_db: 0.0,
        late_db: -40.0,
        // The profile already fades out at80ms. Avoid a second, shorter taper.
        early_ms: 90.0,
        ..Default::default()
    }
}

pub fn profile(layout: LayoutId) -> Result<Option<Arc<cinema::RoomProfile>>, String> {
    static DOLBY: OnceLock<Result<Arc<cinema::RoomProfile>, String>> = OnceLock::new();
    static SONY: OnceLock<Result<Arc<cinema::RoomProfile>, String>> = OnceLock::new();
    let (cache, data) = match layout {
        LayoutId::Dolby7_1_4 => (&DOLBY, include_str!("../assets/spatial-cues/height714.json")),
        LayoutId::Sony360Ra13 => (&SONY, include_str!("../assets/spatial-cues/height360.json")),
        // Never apply a 7.1.4 response to a layout containing different speakers.
        _ => return Ok(None),
    };
    cache.get_or_init(|| {
        let mut p: cinema::RoomProfile = serde_json::from_str(data).map_err(|e| e.to_string())?;
        p.validate()?;
        validate_short(&p)?;
        balance_rear_residuals(&mut p);
        validate_short(&p)?;
        Ok(Arc::new(p))
    }).clone().map(Some)
}

// Only bundled derived spatial cues use this calibration. User room profiles
// and measured direct HRIRs must never be level-normalized by this policy.
fn residual_direct_ratio(s: &cinema::RoomSpeaker) -> f64 {
    let mut direct = 0.0;
    let mut residual = 0.0;
    for (dry, room) in [(&s.direct_left, &s.room_left), (&s.direct_right, &s.room_right)] {
        for (&d, &r) in dry.iter().zip(room) {
            direct += f64::from(d).powi(2);
            residual += (f64::from(r) - f64::from(d)).powi(2);
        }
    }
    if direct > 1e-20 { residual / direct } else { 0.0 }
}

fn balance_rear_residuals(p: &mut cinema::RoomProfile) {
    // Compare posterior speakers with the strongest forward residual at the
    // same elevation. Do not equate height layers or change side surrounds.
    // This is a bounded rendering adjustment, not a perceptual threshold:
    // at most 3 dB attenuation, no boost, no change to delays or direct sound.
    let gains: Vec<f32> = p.speakers.iter().map(|speaker| {
        if speaker.azimuth.abs() < 120.0 { return 1.0; }
        let reference = p.speakers.iter()
            .filter(|front| front.azimuth.abs() <= 60.0
                && (front.elevation - speaker.elevation).abs() < 0.01)
            .map(residual_direct_ratio)
            .fold(0.0_f64, f64::max);
        let ratio = residual_direct_ratio(speaker);
        if reference <= 0.0 || ratio <= reference { return 1.0; }
        (reference / ratio).sqrt().max(10.0_f64.powf(-3.0 / 20.0)) as f32
    }).collect();
    for (speaker, gain) in p.speakers.iter_mut().zip(gains) {
        if gain == 1.0 { continue; }
        // A single scalar on both residual ears preserves their relative levels,
        // spectral shape and arrival times. Direct arrays remain bit-identical.
        for (dry, room) in [(&speaker.direct_left, &mut speaker.room_left),
                            (&speaker.direct_right, &mut speaker.room_right)] {
            for (&d, r) in dry.iter().zip(room) {
                *r = d + (*r - d) * gain;
            }
        }
    }
}

fn validate_short(p: &cinema::RoomProfile) -> Result<(), String> {
    for s in &p.speakers {
        let mut energy = 0.0;
        for (dry, room) in [(&s.direct_left, &s.room_left), (&s.direct_right, &s.room_right)] {
            for (i, (&d, &r)) in dry.iter().zip(room).enumerate() {
                let tail = r - d;
                if (i < s.onset_sample + 240 || i >= s.onset_sample + 3840) && tail != 0.0 {
                    return Err("spatial residual outside 5-80ms window".into());
                }
                energy += tail * tail;
            }
        }
        if !energy.is_finite() || energy <= 0.0 { return Err("empty spatial residual".into()); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hrtf::NativeHrtfSet;
    fn mobile() -> NativeHrtfSet {
        NativeHrtfSet::load_calibrated(&std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mobile/assets/hrtf-mobile-direct/hrtf-set.json")).unwrap()
    }
    #[test]
    fn rear_balance_preserves_direct_front_side_and_residual_shape() {
        for (layout, data) in [
            (LayoutId::Dolby7_1_4, include_str!("../assets/spatial-cues/height714.json")),
            (LayoutId::Sony360Ra13, include_str!("../assets/spatial-cues/height360.json")),
        ] {
            let raw: cinema::RoomProfile = serde_json::from_str(data).unwrap();
            let balanced = profile(layout).unwrap().unwrap();
            let mut changed = 0;
            for (before, after) in raw.speakers.iter().zip(&balanced.speakers) {
                assert_eq!(before.direct_left, after.direct_left);
                assert_eq!(before.direct_right, after.direct_right);
                assert_eq!(before.onset_sample, after.onset_sample);
                let old = residual_direct_ratio(before);
                let new = residual_direct_ratio(after);
                let gain = (new / old).sqrt();
                assert!(gain <= 1.0 + 1e-6);
                assert!(gain >= 10.0_f64.powf(-3.0 / 20.0) - 1e-6);
                if before.azimuth.abs() < 120.0 {
                    assert_eq!(before.room_left, after.room_left);
                    assert_eq!(before.room_right, after.room_right);
                }
                if gain < 0.999999 { changed += 1; }
                for (dry, old_room, new_room) in [
                    (&before.direct_left, &before.room_left, &after.room_left),
                    (&before.direct_right, &before.room_right, &after.room_right),
                ] {
                    for ((&d, &old), &new) in dry.iter().zip(old_room).zip(new_room) {
                        let expected = f64::from(d) + (f64::from(old) - f64::from(d)) * gain;
                        assert!((f64::from(new) - expected).abs() < 1e-7);
                    }
                }
            }
            // The 360RA profile has side-rear speakers at 110 degrees, not
            // posterior 7.1.4 speakers: its previously accepted cues are intact.
            assert_eq!(changed, if layout == LayoutId::Dolby7_1_4 { 4 } else { 0 });
            balanced.validate().unwrap();
            validate_short(&balanced).unwrap();
        }
    }

    #[test]
    fn rear_balance_never_boosts_weak_residuals_or_borrows_another_height() {
        let mut p: cinema::RoomProfile = serde_json::from_str(
            include_str!("../assets/spatial-cues/height714.json")).unwrap();
        for speaker in &mut p.speakers {
            if speaker.azimuth.abs() < 120.0 { continue; }
            for (dry, room) in [(&speaker.direct_left, &mut speaker.room_left),
                                (&speaker.direct_right, &mut speaker.room_right)] {
                for (&d, r) in dry.iter().zip(room) { *r = d + (*r - d) * 0.01; }
            }
        }
        let original = p.clone();
        balance_rear_residuals(&mut p);
        for (a, b) in original.speakers.iter().zip(&p.speakers) {
            assert_eq!(a.room_left, b.room_left);
            assert_eq!(a.room_right, b.room_right);
        }
        let mut isolated: cinema::RoomProfile = serde_json::from_str(
            include_str!("../assets/spatial-cues/height714.json")).unwrap();
        isolated.speakers.retain(|s| s.elevation > 0.0 && s.azimuth.abs() >= 120.0);
        let before = isolated.clone();
        balance_rear_residuals(&mut isolated);
        for (a, b) in before.speakers.iter().zip(&isolated.speakers) {
            assert_eq!(a.room_left, b.room_left);
            assert_eq!(a.room_right, b.room_right);
        }
    }

    #[test]
    fn profiles_cover_front_rear_upper_and_lower_with_bounded_nonzero_tails() {
        for layout in [LayoutId::Dolby7_1_4, LayoutId::Sony360Ra13] {
            let p = profile(layout).unwrap().unwrap();
            assert_eq!(p.layout, layout.as_str());
            assert!(p.speakers.iter().any(|s| s.elevation > 0.0));
            if layout == LayoutId::Sony360Ra13 { assert_eq!(p.speakers.iter().filter(|s| s.elevation < 0.0).count(), 3); }
            validate_short(&p).unwrap();
        }
    }
    #[test]
    fn bundled_cues_preserve_direct_and_add_only_separate_short_residual() {
        for layout in [LayoutId::Dolby7_1_4, LayoutId::Sony360Ra13] {
            let dry = mobile();
            let mut enabled = dry.clone();
            enabled.configure_spatial_cues(layout).unwrap();
            assert!(enabled.spatial_cues_active());
            assert_eq!(enabled.effective_wet(0.0), 0.04);
            let p = profile(layout).unwrap().unwrap();
            for s in &p.speakers {
                let baseline = dry.mixed_speaker(&s.name,&p.layout,s.azimuth as f64,s.elevation as f64,0.0).unwrap();
                let direct = enabled.mixed_speaker(&s.name,&p.layout,s.azimuth as f64,s.elevation as f64,0.0).unwrap();
                let full = enabled.mixed_speaker(&s.name,&p.layout,s.azimuth as f64,s.elevation as f64,0.04).unwrap();
                for (a,b) in [(&baseline.0,&direct.0),(&baseline.1,&direct.1)] {
                    assert_eq!(&b[..a.len()],a, "{} direct changed",s.name);
                    assert!(b[a.len()..].iter().all(|v| *v == 0.0));
                }
                assert!(full.0.iter().zip(&direct.0).any(|(a,b)| a != b));
                assert!(full.1.iter().zip(&direct.1).any(|(a,b)| a != b));
            }
            // Exact measurement reads must not acquire any residual room tail.
            assert_eq!(dry.mixed_direction(0.0,0.0,0.0).unwrap(),enabled.mixed_direction(0.0,0.0,1.0).unwrap());
        }
    }
    #[test]
    fn historical61_cues_preserve_approved_direct_and_cover_both_layouts() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json");
        let dry = crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap();
        for layout in [LayoutId::Dolby7_1_4, LayoutId::Sony360Ra13] {
            let mut enabled = dry.clone();
            enabled.configure_spatial_cues(layout).unwrap();
            assert!(enabled.spatial_cues_active());
            let p = profile(layout).unwrap().unwrap();
            for speaker in &p.speakers {
                let az = speaker.azimuth as f64;
                let el = speaker.elevation as f64;
                let baseline = dry.mixed_speaker(&speaker.name, &p.layout, az, el, 0.0).unwrap();
                let direct = enabled.mixed_speaker(&speaker.name, &p.layout, az, el, 0.0).unwrap();
                let full = enabled.mixed_speaker(&speaker.name, &p.layout, az, el, 0.04).unwrap();
                for (a,b) in [(&baseline.0,&direct.0),(&baseline.1,&direct.1)] {
                    assert_eq!(&b[..a.len()],a, "{} direct changed",speaker.name);
                    assert!(b[a.len()..].iter().all(|v| *v == 0.0));
                }
                assert!(full.0.iter().chain(&full.1).all(|v| v.is_finite()));
                assert!(full.0.iter().zip(&direct.0).any(|(a,b)| a != b));
            }
            let mut e = crate::Engine::new(48000,2);
            e.replace_hrtf(dry.clone(),0.0).unwrap();
            e.set_layout(layout).unwrap();
            assert!(e.active_hrtf_set.as_ref().unwrap().spatial_cues_active());
            assert!(!e.cinema.enabled);
        }
    }

    #[test]
    fn engine_replacement_and_layout_switch_reselect_cues_without_enabling_legacy_room() {
        let mut e = crate::Engine::new(48000,2);
        e.replace_hrtf(mobile(),0.0).unwrap();
        for layout in [LayoutId::Sony360Ra13, LayoutId::Dolby7_1_4] {
            e.set_layout(layout).unwrap();
            let set=e.active_hrtf_set.as_ref().unwrap();
            assert!(set.spatial_cues_active());
            assert_eq!(set.room_profile.as_ref().unwrap().layout,layout.as_str());
            assert!(!e.cinema.enabled);
        }
    }
    #[test]
    fn preset_reload_and_layout_changes_reselect_cues() {
        let mut e = crate::Engine::new(48000, 2);
        e.replace_hrtf(mobile(), 0.0).unwrap();
        e.configure_hrtf_preset(mobile(), 0.0, true, true).unwrap();
        assert!(e.active_hrtf_set.as_ref().unwrap().spatial_cues_active());
        e.configure_room(cinema::Settings::default(), None).unwrap();
        assert!(e.active_hrtf_set.as_ref().unwrap().spatial_cues_active());
        assert!(!e.cinema.enabled);
        e.set_layout(LayoutId::Dolby9_1_6).unwrap();
        let set = e.active_hrtf_set.as_ref().unwrap();
        assert!(!set.spatial_cues_active());
        assert!(set.room_profile.is_none());
        e.set_layout(LayoutId::Sony360Ra13).unwrap();
        assert!(e.active_hrtf_set.as_ref().unwrap().spatial_cues_active());
    }

}


/// Experimental moving-source cue balance. This scales only the bundled
/// residual send, never direct HRIRs, authored object gain, beds, or user rooms.
/// Angular speed is measured from authored positions (not listener head pose).
/// Constants are listening-test tuning, not psychoacoustic thresholds.
pub(crate) struct MotionCues {
    last: Option<([f64; 3], u64)>,
    rate: u32,
    gain: f32,
    target: f32,
    attack: f32,
    release: f32,
}
impl Default for MotionCues {
    fn default() -> Self {
        Self { last: None, rate: 0, gain: 1.0, target: 1.0, attack: 0.0, release: 0.0 }
    }
}
impl MotionCues {
    // I listening candidate: only the residual floor differs from H (-9 dB).
    const MIN_RESIDUAL_GAIN: f32 = 0.17782794; // -15 dB
    #[cfg(test)]
    pub(crate) fn gain(&self) -> f32 { self.gain }
    pub(crate) fn next(&mut self, position: [f32; 3], at: u64, rate: u32, enabled: bool) -> f32 {
        if !enabled || rate == 0 {
            *self = Self::default();
            return 1.0;
        }
        if self.rate != rate {
            *self = Self::default();
            self.rate = rate;
            self.attack = 1.0 - (-1.0 / (0.040 * rate as f32)).exp();
            self.release = 1.0 - (-1.0 / (0.250 * rate as f32)).exp();
        }
        // ~375 Hz observation, independent of convolution or caller block size.
        let quantum = (rate / 375).max(1) as u64;
        if self.last.is_none_or(|(_, prev)| at < prev || at - prev >= quantum) {
            let p = position.map(f64::from);
            let norm = p.iter().map(|v| v*v).sum::<f64>().sqrt();
            if norm.is_finite() && norm > 1e-6 {
                let unit = p.map(|v| v / norm);
                self.target = 1.0;
                if let Some((prev, time)) = self.last {
                    // A seek/suspension isn't a fast-moving sound source.
                    let dt = at.saturating_sub(time);
                    if dt > 0 && dt <= (rate / 10) as u64 {
                        let cross = [prev[1]*unit[2]-prev[2]*unit[1],
                            prev[2]*unit[0]-prev[0]*unit[2], prev[0]*unit[1]-prev[1]*unit[0]];
                        let sin = cross.iter().map(|v| v*v).sum::<f64>().sqrt();
                        let cos = prev.iter().zip(unit).map(|(a,b)| a*b).sum::<f64>();
                        let speed = sin.atan2(cos).to_degrees() * rate as f64 / dt as f64;
                        let t = ((speed - 10.0) / 35.0).clamp(0.0, 1.0) as f32;
                        let motion = t*t*(3.0-2.0*t);
                        // Keep a nonzero tail; do not alter the direct object signal.
                        self.target = 1.0 - motion * (1.0 - Self::MIN_RESIDUAL_GAIN);
                    }
                }
                self.last = Some((unit, at));
            } else {
                self.last = None;
                self.target = 1.0;
            }
        }
        let coefficient = if self.target < self.gain { self.attack } else { self.release };
        self.gain += (self.target - self.gain) * coefficient;
        self.gain
    }
}

#[cfg(test)]
mod motion_tests {
    use super::MotionCues;
    fn position(degrees: f64, radius: f32) -> [f32; 3] {
        [(degrees.to_radians().sin() as f32)*radius,
         (degrees.to_radians().cos() as f32)*radius, 0.0]
    }
    #[test]
    fn stationary_radial_and_disabled_sources_keep_unity() {
        for enabled in [false, true] {
            let mut state = MotionCues::default();
            for n in 0..48000 {
                let p = if enabled { [0.0, 1.0+n as f32/48000.0, 0.0] }
                    else { position(n as f64/100.0, 1.0) };
                assert_eq!(state.next(p, n, 48000, enabled), 1.0);
            }
        }
    }
    #[test]
    fn moving_gain_is_bounded_smooth_and_sample_rate_independent() {
        let mut values = Vec::new();
        for rate in [44100, 48000, 96000] {
            let mut state = MotionCues::default();
            let mut last = 1.0;
            for n in 0..rate*2 {
                // Cross the +/-180 seam: vector angle must not spike there.
                let g = state.next(position(170.0 + 45.0*n as f64/rate as f64, 1.0), n as u64, rate, true);
                assert!((MotionCues::MIN_RESIDUAL_GAIN - 1e-6..=1.0).contains(&g));
                assert!((g-last).abs() < 0.0005);
                last = g;
            }
            values.push(last);
            for n in rate*2..rate*4 {
                last = state.next(position(260.0, 1.0), n as u64, rate, true);
            }
            assert!(last > 0.999, "stopped object must recover depth: {last}");
        }
        for v in values { assert!((v-MotionCues::MIN_RESIDUAL_GAIN).abs() < 0.001); }
    }
    #[test]
    fn slow_wrap_and_discontinuities_do_not_create_false_motion() {
        let mut state = MotionCues::default();
        for n in 0..48000 {
            assert_eq!(state.next(position(179.0+2.0*n as f64/48000.0, 1.0), n, 48000, true), 1.0);
        }
        assert_eq!(state.next([1.0,0.0,0.0], 480000, 48000, true), 1.0);
        assert_eq!(state.next([f32::NAN,0.0,0.0], 480128, 48000, true), 1.0);
        assert_eq!(state.next([0.0;3], 480256, 48000, true), 1.0);
        assert_eq!(state.next([0.0,1.0,0.0], 0, 48000, true), 1.0);
    }
}
