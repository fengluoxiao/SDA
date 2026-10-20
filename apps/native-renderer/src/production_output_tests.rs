//! Common output controls remain independent of source-group balance.
use super::*;
#[test]
fn generic_output_preserves_manual_and_program_volume_for_every_codec() {
    for codec in ["eac3", "mpegh", "av3a", "adm", "future-object-format"] {
        for spatial in [false, true] {
            let mut full = Engine::new(48000, 2);
            let mut quiet = Engine::new(48000, 2);
            for e in [&mut full, &mut quiet] {
                e.set_spatial_enhancement(spatial).unwrap();
                e.set_program_codec(codec.into());
            }
            quiet.set_output_volume(0.5, true);
            quiet.set_master_preamp_db(-6.).unwrap();
            quiet.program_enabled = true;
            quiet.set_program_target(0.5, true);
            let ratio = 0.25 * 10f32.powf(-6. / 20.) * 0.5;
            for i in 0..96000 {
                let x = (i as f32 * 0.13).sin() * 0.3;
                let a = full.process_final_output([x, -0.4 * x]);
                let b = quiet.process_final_output([x, -0.4 * x]);
                for ear in 0..2 {
                    assert!(
                        (a[ear] * ratio - b[ear]).abs() < 2e-7,
                        "{codec}: user/program volume changed"
                    );
                }
            }
        }
    }
}
#[test]
fn final_output_has_no_spatially_triggered_second_programme_normalizer() {
    for volume_balance in [false, true] {
        let mut off = Engine::new(48000, 2);
        let mut on = Engine::new(48000, 2);
        off.set_spatial_enhancement(false).unwrap();
        on.set_spatial_enhancement(true).unwrap();
        for e in [&mut off, &mut on] {
            e.program_enabled = volume_balance;
            e.set_program_target(0.25, true);
        }
        for i in 0..144000 {
            let x = (i as f32 * 0.13).sin() * 0.3;
            assert_eq!(
                off.process_final_output([x, -x]),
                on.process_final_output([x, -x])
            );
        }
    }
}
#[test]
fn real_output_peak_guard_covers_manual_makeup_in_every_switch_state() {
    for spatial in [false, true] {
        let mut e = Engine::new(48000, 2);
        e.set_spatial_enhancement(spatial).unwrap();
        e.set_master_preamp_db(12.).unwrap();
        for i in 0..96000 {
            let x = (i as f32 * 1.57 + 0.79).sin() * 0.8;
            let y = e.process_final_output([x, -0.7 * x]);
            assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 0.871));
            assert!((y[1] + 0.7 * y[0]).abs() < 2e-7);
        }
    }
}
#[test]
fn spatial_switch_does_not_reset_the_manual_master_preamp() {
    let mut e = Engine::new(48000, 2);
    e.set_master_preamp_db(-6.).unwrap();
    for enabled in [true, false, true] {
        e.set_spatial_enhancement(enabled).unwrap();
        assert_eq!(e.master_preamp.target_db(), -6.);
    }
}
#[test]
fn transport_reset_discards_auxiliary_meter_envelopes_and_peak_history() {
    let mut old = Engine::new(48000, 2);
    old.set_spatial_enhancement(true).unwrap();
    old.auxiliary_balance.set_enabled(true, true, 0);
    for i in 0..144000 {
        let mut r = auxiliary_balance::Reference::default();
        r.groups[0][0] = (i as f64 * 0.13).sin() * 0.03;
        r.groups[1][0] = (i as f64 * 0.17).sin() * 0.3;
        old.auxiliary_balance
            .mark_applied(i % convolution::DEFAULT_PARTITION);
        old.auxiliary_balance.observe(r, 1.);
        if (i + 1) % convolution::DEFAULT_PARTITION == 0 {
            old.auxiliary_balance.finish_block();
        }
        old.process_final_output([2., -2.]);
    }
    assert!(old.auxiliary_balance.maximum_attenuation_db > 3.);
    old.reset_session(96000);
    let mut fresh = Engine::new(48000, 2);
    fresh.set_spatial_enhancement(true).unwrap();
    assert_eq!(old.auxiliary_balance.maximum_attenuation_db, 0.);
    assert!(
        old.auxiliary_reference
            .iter()
            .all(|r| r.groups == [[0.; 4]; 2])
    );
    for i in 0..12000 {
        let x = (i as f32 * 0.07).sin() * 0.01;
        assert_eq!(
            old.process_final_output([x, -x]),
            fresh.process_final_output([x, -x])
        );
    }
}
