//! Exercise the actual Engine final-output node, independent of source format.
use super::*;
#[test]
fn generic_output_preserves_manual_and_program_volume_for_every_codec() {
    for codec in ["eac3", "mpegh", "av3a", "future-object-format"] {
        let mut full = Engine::new(48000, 2);
        let mut quiet = Engine::new(48000, 2);
        full.set_spatial_enhancement(true).unwrap();
        quiet.set_spatial_enhancement(true).unwrap();
        full.set_program_codec(codec.into());
        quiet.set_program_codec(codec.into());
        quiet.set_output_volume(0.5, true);
        quiet.set_master_preamp_db(-6.).unwrap();
        quiet.program_enabled = true;
        quiet.set_program_target(0.5, true);
        let ratio = 0.25 * 10f32.powf(-6. / 20.) * 0.5;
        let mut error = 0f32;
        for i in 0..240000 {
            let level = if i < 48000 { 1.5 } else { 0.3 };
            let x = (i as f32 * 0.13).sin() * level;
            let a = full.process_final_output([x, -0.4 * x]);
            let b = quiet.process_final_output([x, -0.4 * x]);
            if i > 192000 {
                for ear in 0..2 {
                    error = error.max((a[ear] * ratio - b[ear]).abs());
                }
            }
        }
        assert!(
            error < 2e-7,
            "{codec}: manual/program volume cancelled: {error}"
        );
        assert!(
            (full.output_dynamics.maximum_attenuation_db
                - quiet.output_dynamics.maximum_attenuation_db)
                .abs()
                < 1e-10
        );
        assert!(full.output_dynamics.maximum_attenuation_db > 5.);
    }
}
#[test]
fn reset_discards_dynamic_audio_envelopes_and_peak_history() {
    let mut old = Engine::new(48000, 2);
    old.set_spatial_enhancement(true).unwrap();
    for i in 0..48000 {
        old.process_final_output([(i as f32 * 0.4).sin() * 3., 0.4]);
    }
    assert!(old.output_dynamics.maximum_attenuation_db > 5.);
    old.reset_session(96000);
    let mut fresh = Engine::new(48000, 2);
    fresh.set_spatial_enhancement(true).unwrap();
    for i in 0..12000 {
        let x = (i as f32 * 0.07).sin() * 0.01;
        assert_eq!(
            old.process_final_output([x, -x]),
            fresh.process_final_output([x, -x])
        );
    }
}
#[test]
fn real_output_peak_guard_covers_manual_makeup_after_normalization() {
    let mut e = Engine::new(48000, 2);
    e.set_spatial_enhancement(true).unwrap();
    e.set_master_preamp_db(12.).unwrap();
    for i in 0..96000 {
        let x = (i as f32 * 1.57 + 0.79).sin() * 0.8;
        let y = e.process_final_output([x, -0.7 * x]);
        assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 0.871));
        assert!((y[1] + 0.7 * y[0]).abs() < 2e-7);
    }
}

#[test]
fn enhancement_and_plain_switches_bypass_only_slow_control_without_new_audio_delay() {
    let mut e = Engine::new(48000, 2);
    e.set_spatial_enhancement(true).unwrap();
    e.output_active = true;
    e.paused = false;
    let mut enabled_power = 0f64;
    for i in 0..96000 {
        let x = (i as f32 * 0.13).sin() * 0.3;
        let y = e.process_final_output([x, -x]);
        if i >= 48000 {
            enabled_power += f64::from(y[0]).powi(2);
        }
    }
    assert!(
        (enabled_power / 48000.).sqrt() < 0.12,
        "enabled output must actually attenuate, not merely update detector status"
    );
    assert!(e.output_dynamics.maximum_attenuation_db > 3.);
    for plain in [false, true] {
        e.set_spatial_enhancement(true).unwrap();
        e.set_binaural_enabled(true);
        for i in 0..12000 {
            let x = (i as f32 * 0.13).sin() * 0.3;
            e.process_final_output([x, -x]);
        }
        if plain {
            e.set_binaural_enabled(false);
        } else {
            e.set_spatial_enhancement(false).unwrap();
        }
        let delay = e.peak_guard.latency_frames();
        for i in 0..12000 {
            let x = (i as f32 * 0.13).sin() * 0.3;
            let y = e.process_final_output([x, -x]);
            if i > 4800 + delay {
                let expected = ((i - delay) as f32 * 0.13).sin() * 0.3;
                assert_eq!(y, [expected, -expected]);
            }
        }
    }
}
