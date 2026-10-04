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
        let p: cinema::RoomProfile = serde_json::from_str(data).map_err(|e| e.to_string())?;
        p.validate()?;
        validate_short(&p)?;
        Ok(Arc::new(p))
    }).clone().map(Some)
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
