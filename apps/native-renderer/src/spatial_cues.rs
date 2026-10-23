//! Opted-in, versioned short spatial cues shared by every native platform.
//! These are derived measurements, NOT source-distance metadata or Apple emulation.
use crate::{cinema, vbap::LayoutId};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CueWindow {
    #[default]
    Short35,
    Extended80,
}

pub fn settings() -> cinema::Settings {
    cinema::Settings {
        enabled: true,
        reflection_mode: cinema::ReflectionMode::Early,
        direct_db: 0.0,
        early_db: 0.0,
        late_db: -40.0,
        // The profile includes the accepted residual taper through 80ms.
        // Avoid an additional truncation in the cinema mixer.
        early_ms: 90.0,
        ..Default::default()
    }
}

pub fn profile(layout: LayoutId) -> Result<Option<Arc<cinema::RoomProfile>>, String> {
    profile_with_window(layout, CueWindow::Short35)
}

pub(crate) fn profile_with_window(layout: LayoutId, window: CueWindow) -> Result<Option<Arc<cinema::RoomProfile>>, String> {
    static DOLBY: OnceLock<Result<Arc<cinema::RoomProfile>, String>> = OnceLock::new();
    static SONY: OnceLock<Result<Arc<cinema::RoomProfile>, String>> = OnceLock::new();
    static DOLBY_LONG: OnceLock<Result<Arc<cinema::RoomProfile>, String>> = OnceLock::new();
    static SONY_LONG: OnceLock<Result<Arc<cinema::RoomProfile>, String>> = OnceLock::new();
    let (cache, data) = match (layout, window) {
        (LayoutId::Dolby7_1_4, CueWindow::Short35) => (&DOLBY, include_str!("../assets/spatial-cues/height714.json")),
        (LayoutId::Sony360Ra13, CueWindow::Short35) => (&SONY, include_str!("../assets/spatial-cues/height360.json")),
        (LayoutId::Dolby7_1_4, CueWindow::Extended80) => (&DOLBY_LONG, include_str!("../assets/spatial-cues/height714.json")),
        (LayoutId::Sony360Ra13, CueWindow::Extended80) => (&SONY_LONG, include_str!("../assets/spatial-cues/height360.json")),
        // Never apply a 7.1.4 response to a layout containing different speakers.
        _ => return Ok(None),
    };
    let loaded = cache.get_or_init(|| {
        let mut p: cinema::RoomProfile = serde_json::from_str(data).map_err(|e| e.to_string())?;
        p.validate()?;
        validate_short(&p)?;
        balance_rear_residuals(&mut p);
        shape_residual_spectrum(&mut p, RESIDUAL_HIGH_GAIN);
        taper_residual_tail(&mut p, 20.0);
        match window {
            CueWindow::Short35 => apply_temporal_reflection_trial(&mut p),
            CueWindow::Extended80 => apply_temporal_reflection_window(&mut p, 80.0),
        }
        validate_short(&p)?;
        Ok(Arc::new(p))
    }).clone()?;
    Ok(Some(loaded))
}

// Accepted three-song residual policy: keep the earliest 20ms intact and
// gently reduce the tail through 80ms. Applied once to bundled profiles only,
// not user rooms. No measured direct response, source gain or delay changes.
fn taper_residual_tail(p: &mut cinema::RoomProfile, start_ms: f64) {
    let start = start_ms * 48.0;
    let end = 80.0 * 48.0;
    let floor = 10.0_f64.powf(-6.0 / 20.0);
    for s in &mut p.speakers {
        for (dry, room) in [(&s.direct_left, &mut s.room_left),
                            (&s.direct_right, &mut s.room_right)] {
            for (i, (&d, r)) in dry.iter().zip(room).enumerate() {
                let age = i.saturating_sub(s.onset_sample) as f64;
                if age <= start { continue; }
                let t = ((age - start) / (end - start)).clamp(0.0, 1.0);
                let gain = 1.0 - (1.0-floor) * t*t*(3.0-2.0*t);
                *r = (f64::from(d) + (f64::from(*r)-f64::from(d))*gain) as f32;
            }
        }
    }
}

#[cfg(test)]
mod diagnostic_tail_tests {
    use super::*;
    #[test]
    fn temporal_window_preserves_direct_first_arrivals_and_mirror_equivariance() {
        for data in [include_str!("../assets/spatial-cues/height714.json"),include_str!("../assets/spatial-cues/height360.json")] {
            let before:cinema::RoomProfile=serde_json::from_str(data).unwrap();
            let mut a=before.clone();let mut b=before.clone();
            for s in &mut b.speakers {
                std::mem::swap(&mut s.direct_left,&mut s.direct_right);
                std::mem::swap(&mut s.room_left,&mut s.room_right);
                s.azimuth = -s.azimuth;
            }
            apply_temporal_reflection_trial(&mut a);
            apply_temporal_reflection_trial(&mut b);
            validate_short(&a).unwrap();
            for ((old,new),mirror) in before.speakers.iter().zip(&a.speakers).zip(&b.speakers) {
                assert_eq!(old.direct_left,new.direct_left);assert_eq!(old.direct_right,new.direct_right);
                assert_eq!(new.room_left,mirror.room_right);assert_eq!(new.room_right,mirror.room_left);
                for (dry,room,original) in [(&new.direct_left,&new.room_left,&old.room_left),(&new.direct_right,&new.room_right,&old.room_right)] {
                    let end=(old.onset_sample+576).min(room.len());assert_eq!(&room[..end],&original[..end]);
                    for i in old.onset_sample+1680..room.len() {assert_eq!(room[i],dry[i]);}
                }
            }
        }
    }
    #[test]
    fn rejected_front_reflection_diagnostic_preserves_direct_and_height_rear_kernels() {
        for data in [include_str!("../assets/spatial-cues/height714.json"),
                     include_str!("../assets/spatial-cues/height360.json")] {
            let before:cinema::RoomProfile=serde_json::from_str(data).unwrap();
            // Rejected diagnostic remains test-only; never called by profile().
            let mut after=before.clone();
            for s in &mut after.speakers {
                if s.elevation.abs()>0.01 || s.azimuth.abs()>60.0 {continue;}
                for (dry,room) in [(&s.direct_left,&mut s.room_left),(&s.direct_right,&mut s.room_right)] {
                    for (&d,r) in dry.iter().zip(room) { *r=d+(*r-d)*0.25; }
                }
            }
            validate_short(&after).unwrap();
            let mut changed=0;
            for (a,b) in before.speakers.iter().zip(&after.speakers) {
                assert_eq!(a.direct_left,b.direct_left);assert_eq!(a.direct_right,b.direct_right);
                if a.elevation.abs()>0.01 || a.azimuth.abs()>60.0 {
                    assert_eq!(a.room_left,b.room_left);assert_eq!(a.room_right,b.room_right);
                } else {changed+=1;assert!(residual_direct_ratio(b)<residual_direct_ratio(a));}
            }
            assert!(changed>0);
        }
    }
    #[test]
    fn tail_taper_preserves_direct_early_support_and_never_adds_energy() {
        for data in [include_str!("../assets/spatial-cues/height714.json"),
                     include_str!("../assets/spatial-cues/height360.json")] {
            let original: cinema::RoomProfile=serde_json::from_str(data).unwrap();
            let mut p=original.clone();
            taper_residual_tail(&mut p,20.0);
            validate_short(&p).unwrap();
            for (before, after) in original.speakers.iter().zip(&p.speakers) {
                assert_eq!(before.direct_left,after.direct_left);
                assert_eq!(before.direct_right,after.direct_right);
                for (dry,old,new) in [(&before.direct_left,&before.room_left,&after.room_left),
                                      (&before.direct_right,&before.room_right,&after.room_right)] {
                    for (i,((&d,&a),&b)) in dry.iter().zip(old).zip(new).enumerate() {
                        if i<=before.onset_sample+960 { assert_eq!(a,b); }
                        assert!((b-d).abs()<=(a-d).abs()+1e-7);
                        if a==d { assert_eq!(b,d); }
                    }
                }
            }
        }
    }
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
            // Test rear balancing independently of subsequent spectral shaping.
            let mut balanced = raw.clone();
            balance_rear_residuals(&mut balanced);
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
        let companion_path = path.parent().unwrap().parent().unwrap().join("hrtf/hrtf-set.json");
        let companion = crate::hrtf::NativeHrtfSet::load_calibrated(&companion_path).unwrap();
        for layout in [LayoutId::Dolby7_1_4, LayoutId::Sony360Ra13] {
            let mut enabled = dry.clone();
            enabled.configure_spatial_cues(layout).unwrap();
            assert!(enabled.spatial_cues_active());
            // Keep the selected directional lookup and layout identical. A
            // nonconfigured set uses legacy nearest-speaker fallback, whereas
            // missing bundled anchors now use the requested actual direction.
            let reference = enabled.diagnostic_without_spatial_residual();
            let p = profile(layout).unwrap().unwrap();
            for speaker in &p.speakers {
                let az = speaker.azimuth as f64;
                let el = speaker.elevation as f64;
                let baseline = reference.mixed_speaker(&speaker.name, &p.layout, az, el, 0.04).unwrap();
                if dry.has_exact_measurement(az,el) || companion.has_exact_measurement(az,el) {
                    let original = dry.mixed_speaker(&speaker.name,&p.layout,az,el,0.0).unwrap();
                    for (a,b) in [(&original.0,&baseline.0),(&original.1,&baseline.1)] {
                        assert_eq!(&b[..a.len()],a,"{} stored anchor changed",speaker.name);
                        assert!(b[a.len()..].iter().all(|v| *v==0.0));
                    }
                }
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


// Accepted residual-only B tuning (360RA, Ether and MONTERO auditions).
// This is a listening-tuned shelf, not a psychoacoustic threshold or source EQ.
const RESIDUAL_HIGH_GAIN: f64 = 0.5011872336272722; // -6 dB high-frequency limit

// Identical centered 17-tap binomial FIR for every bundled speaker and ear.
// Bake into the existing 5-80 ms residual window: no extra playback latency,
// no change to direct HRIRs, source gains/positions or MotionCues policy.
// Zero extension/clipping at the window edges keeps the original time support.
// User-supplied room profiles do not pass through this bundled-profile helper.
fn shape_residual_spectrum(p: &mut cinema::RoomProfile, high_gain: f64) {
    assert!((0.0..=1.0).contains(&high_gain));
    if high_gain == 1.0 { return; } // bit-exact bypass
    let taps: [f64;17] = [1.,16.,120.,560.,1820.,4368.,8008.,11440.,12870.,11440.,8008.,4368.,1820.,560.,120.,16.,1.];
    for s in &mut p.speakers {
        let start=s.onset_sample+240;
        let end=s.onset_sample+3840;
        for (dry,room) in [(&s.direct_left,&mut s.room_left),(&s.direct_right,&mut s.room_right)] {
            let residual:Vec<f64>=room.iter().zip(dry).map(|(&r,&d)| f64::from(r)-f64::from(d)).collect();
            for i in start..end.min(room.len()) {
                let mut low=0.0;
                for (k,tap) in taps.iter().enumerate() {
                    let j=i as isize+k as isize-8;
                    if j>=start as isize && j<(end.min(residual.len())) as isize {
                        low+=residual[j as usize]*tap/65536.0;
                    }
                }
                room[i]=(f64::from(dry[i])+high_gain*residual[i]+(1.0-high_gain)*low) as f32;
            }
        }
    }
}
#[cfg(test)]
mod residual_spectrum_tests {
    use super::*;
    #[test]
    fn both_layouts_bypass_direct_support_and_finite() {
        for data in [include_str!("../assets/spatial-cues/height714.json"),include_str!("../assets/spatial-cues/height360.json")] {
            let mut p:cinema::RoomProfile=serde_json::from_str(data).unwrap();
            balance_rear_residuals(&mut p);
            let original=p.clone();
            shape_residual_spectrum(&mut p,1.0);
            assert_eq!(format!("{:?}",p),format!("{:?}",original));
            shape_residual_spectrum(&mut p,RESIDUAL_HIGH_GAIN);
            validate_short(&p).unwrap();p.validate().unwrap();
            for (a,b) in original.speakers.iter().zip(&p.speakers) {
                assert_eq!(a.direct_left,b.direct_left);assert_eq!(a.direct_right,b.direct_right);
                assert_eq!(a.azimuth,b.azimuth);assert_eq!(a.elevation,b.elevation);
                assert_eq!(a.onset_sample,b.onset_sample);
                assert!(b.room_left.iter().chain(&b.room_right).all(|v|v.is_finite()));
            }
        }
    }
    #[test]
    fn ear_exchange_commutes_and_energy_does_not_increase() {
        let mut p:cinema::RoomProfile=serde_json::from_str(include_str!("../assets/spatial-cues/height360.json")).unwrap();
        let original=p.clone();let mut swapped=p.clone();
        for s in &mut swapped.speakers {std::mem::swap(&mut s.direct_left,&mut s.direct_right);std::mem::swap(&mut s.room_left,&mut s.room_right);}
        shape_residual_spectrum(&mut p,RESIDUAL_HIGH_GAIN);shape_residual_spectrum(&mut swapped,RESIDUAL_HIGH_GAIN);
        for ((a,b),old) in p.speakers.iter().zip(&swapped.speakers).zip(&original.speakers) {
            assert_eq!(a.room_left,b.room_right);assert_eq!(a.room_right,b.room_left);
            assert!(residual_direct_ratio(a)<=residual_direct_ratio(old)*(1.0+1e-6));
        }
    }
}

#[cfg(test)]
mod residual_profile_integration_tests {
    use super::*;
    #[test]
    fn both_profiles_apply_accepted_stage_order() {
        for (layout, data) in [
            (LayoutId::Dolby7_1_4, include_str!("../assets/spatial-cues/height714.json")),
            (LayoutId::Sony360Ra13, include_str!("../assets/spatial-cues/height360.json")),
        ] {
            let mut expected: cinema::RoomProfile = serde_json::from_str(data).unwrap();
            balance_rear_residuals(&mut expected);
            let before = expected.clone();
            shape_residual_spectrum(&mut expected, RESIDUAL_HIGH_GAIN);
            taper_residual_tail(&mut expected, 20.0);
            apply_temporal_reflection_trial(&mut expected);
            let actual = profile(layout).unwrap().unwrap();
            assert_eq!(format!("{:?}", expected), format!("{:?}", actual));
            assert!(before.speakers.iter().zip(&actual.speakers).any(|(a,b)| a.room_left != b.room_left));
        }
    }
}

// User-requested trial build: identical arrival-time envelope for every route.
fn apply_temporal_reflection_trial(p: &mut cinema::RoomProfile) {
    let end_ms = 35.0;
    // Offline isolation only. Release always retains the accepted envelope.
    #[cfg(test)]
    let end_ms = std::env::var("SDA_DIAGNOSTIC_CUE_END_MS").ok().map(|v| {
        let value: f64 = v.parse().expect("invalid diagnostic cue window");
        assert!((35.0..=80.0).contains(&value)); value
    }).unwrap_or(end_ms);
    apply_temporal_reflection_window(p, end_ms);
}

fn apply_temporal_reflection_window(p: &mut cinema::RoomProfile, end_ms: f64) {
    for s in &mut p.speakers {
        // Diagnostic isolation: retain the accepted long frontal cue while
        // restoring the short tail progressively towards rear/upper speakers.
        // Release has no environment-controlled behavior.
        #[cfg(test)]
        let end_ms = if std::env::var("SDA_DIAGNOSTIC_REAR_TOP_SHORT").as_deref()==Ok("1") {
            directional_cue_end_ms(f64::from(s.azimuth), f64::from(s.elevation), end_ms)
        } else { end_ms };
        for (dry,room) in [(&s.direct_left,&mut s.room_left),(&s.direct_right,&mut s.room_right)] {
            for (i,(&d,r)) in dry.iter().zip(room).enumerate() {
                let ms=i.saturating_sub(s.onset_sample) as f64/48.0;
                if ms<=12.0 {continue;}
                let t=((ms-12.0)/(end_ms-12.0)).clamp(0.0,1.0);
                let gain=1.0-t*t*(3.0-2.0*t);
                *r=(f64::from(d)+(f64::from(*r)-f64::from(d))*gain) as f32;
            }
        }
    }
}

#[cfg(test)]
fn directional_cue_end_ms(azimuth: f64, elevation: f64, front_end: f64) -> f64 {
    let rear=((azimuth.abs()-60.0)/60.0).clamp(0.0,1.0);
    let upper=(elevation/45.0).clamp(0.0,1.0);
    let t=rear.max(upper); let t=t*t*(3.0-2.0*t);
    front_end+(35.0-front_end)*t
}

#[cfg(test)]
mod reflection_window_isolation_tests {
    use super::*;
    #[test]
    fn manifest_cue_window_caches_are_independent_and_direct_is_unchanged() {
        for layout in [LayoutId::Dolby7_1_4, LayoutId::Sony360Ra13] {
            let long=profile_with_window(layout,CueWindow::Extended80).unwrap().unwrap();
            let short=profile_with_window(layout,CueWindow::Short35).unwrap().unwrap();
            assert!(!Arc::ptr_eq(&long,&short));
            assert!(Arc::ptr_eq(&long,&profile_with_window(layout,CueWindow::Extended80).unwrap().unwrap()));
            assert!(Arc::ptr_eq(&short,&profile(layout).unwrap().unwrap()));
            let mut restored_energy=0.0f64;
            for (a,b) in short.speakers.iter().zip(&long.speakers) {
                assert_eq!(a.direct_left,b.direct_left);assert_eq!(a.direct_right,b.direct_right);
                assert_eq!(a.onset_sample,b.onset_sample);
                for (dry,old,new) in [(&a.direct_left,&a.room_left,&b.room_left),(&a.direct_right,&a.room_right,&b.room_right)] {
                    let first=(a.onset_sample+576).min(dry.len());
                    assert_eq!(&old[..first],&new[..first]);
                    for i in a.onset_sample+1680..dry.len() {
                        assert_eq!(old[i],dry[i]);
                        restored_energy+=f64::from(new[i]-dry[i]).powi(2);
                    }
                }
            }
            assert!(restored_energy>0.0);
        }
        assert!(profile_with_window(LayoutId::Stereo2_0,CueWindow::Extended80).unwrap().is_none());
    }
    #[test]
    fn directional_window_preserves_front_and_bounds_rear_upper_transition() {
        for az in [-60.0,-30.0,0.0,30.0,60.0] {
            assert_eq!(directional_cue_end_ms(az,0.0,80.0),80.0);
        }
        for az in [-180.0,-140.0,140.0,180.0] {
            assert_eq!(directional_cue_end_ms(az,0.0,80.0),35.0);
        }
        for el in [45.0,60.0,90.0] { assert_eq!(directional_cue_end_ms(0.0,el,80.0),35.0); }
        for az in 0..180 {
            let x=directional_cue_end_ms(az as f64,20.0,80.0);
            assert!((35.0..=80.0).contains(&x));
            assert_eq!(x,directional_cue_end_ms(-(az as f64),20.0,80.0));
        }
    }
    #[test]
    fn longer_windows_keep_direct_and_first12ms_and_do_not_exceed_reference() {
        for data in [include_str!("../assets/spatial-cues/height714.json"),include_str!("../assets/spatial-cues/height360.json")] {
            let mut reference: cinema::RoomProfile=serde_json::from_str(data).unwrap();
            balance_rear_residuals(&mut reference);
            shape_residual_spectrum(&mut reference,RESIDUAL_HIGH_GAIN);
            taper_residual_tail(&mut reference,20.0);
            for end_ms in [35.0,50.0,80.0] {
                let mut candidate=reference.clone();
                apply_temporal_reflection_window(&mut candidate,end_ms);
                candidate.validate().unwrap(); validate_short(&candidate).unwrap();
                let mut changed=0;
                for (a,b) in reference.speakers.iter().zip(&candidate.speakers) {
                    assert_eq!(a.direct_left,b.direct_left);assert_eq!(a.direct_right,b.direct_right);
                    for (dry,original,current) in [(&a.direct_left,&a.room_left,&b.room_left),(&a.direct_right,&a.room_right,&b.room_right)] {
                        for (i,((&d,&x),&y)) in dry.iter().zip(original).zip(current).enumerate() {
                            if i<=a.onset_sample+576 {assert_eq!(x,y);}
                            assert!((y-d).abs()<=(x-d).abs()+1e-6);
                            if i>=a.onset_sample+(end_ms*48.0) as usize {assert_eq!(y,d);}
                            if x!=y {changed+=1;}
                        }
                    }
                }
                assert!(changed>0);
            }
        }
    }
}
