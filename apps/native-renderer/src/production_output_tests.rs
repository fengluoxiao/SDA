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
                let x = (i as f32 * 0.13).sin() * 0.003;
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
fn quiet_output_keeps_programme_gain_without_unneeded_attenuation() {
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
            let x = (i as f32 * 0.13).sin() * 0.003;
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

#[test]
fn loud_processed_output_uses_the_shared_guard_for_both_volume_states() {
    for codec in ["eac3", "mpegh", "av3a", "ac4", "future-object-format"] {
        for volume in [false, true] {
            let mut e = Engine::new(48000, 2);
            e.set_spatial_enhancement(true).unwrap();
            e.set_program_codec(codec.into());
            e.program_enabled = volume;
            e.set_program_target(if volume { 0.25 } else { 1. }, true);
            for i in 0..48000 * 6 {
                let x = (i as f32 * std::f32::consts::TAU * 1000. / 48000.).sin() * 0.7;
                let y = e.process_final_output([x, -x * 0.7]);
                assert!((y[1] + y[0] * 0.7).abs() < 2e-7);
            }
            assert!(e.output_dynamics.enabled());
            assert!(
                e.output_dynamics.maximum_attenuation_db > 2.,
                "guard bypassed for {codec}/{volume}"
            );
            e.set_spatial_enhancement(false).unwrap();
            for _ in 0..48000 {
                e.process_final_output([0.001, -0.0007]);
            }
            assert!(!e.output_dynamics.enabled());
            assert!(e.output_dynamics.gain_db().abs() < 1e-6);
        }
    }
}

// The playback protocol, rather than a codec extension alone, establishes Atmos.
fn declare_makeup_source(e: &mut Engine, object: bool, label: &str) {
    let fifo = stereo_fifo::StereoFifo::new(4096);
    let telemetry = RuntimeTelemetry::default();
    assert!(protocol::apply_render_command(e, render_command::RenderCommand::Command(Command::AddSource {
        id: if object { "obj:12" } else { "bed:0" }.into(),
        bed_label: (!object).then(|| label.into()), at: None,
    }), &fifo, &telemetry));
}

#[test]
fn atmos_makeup_requires_both_volume_balance_and_spatial_programme_metadata() {
    for codec in ["eac3", "ac4", "truehd", "mlpa", "ac3", "mpegh", "av3a", "alac", "adm", "future-object-format"] {
        for volume in [false, true] {
            for spatial in [false, true] {
                for (object, label) in [(false, "FrontLeft"), (false, "Tfl"), (true, "")] {
                    let mut e = Engine::new(48000, 2);
                    e.set_spatial_enhancement(spatial).unwrap();
                    e.set_program_codec(codec.into());
                    e.program_enabled = volume;
                    e.set_program_target(0.5, true);
                    declare_makeup_source(&mut e, object, label);
                    let eligible = volume && spatial && ["eac3", "ac4", "truehd", "mlpa"].contains(&codec)
                        && (object || label == "Tfl");
                    assert_eq!(e.atmos_output_makeup.target_db(), if eligible { 1.5 } else { 0.0 },
                        "{codec} volume={volume} spatial={spatial} object={object} label={label}");
                    assert_eq!(e.program_metadata_gain, 1.0, "makeup must not rewrite reference metadata");
                    assert_eq!(e.program_gain, if volume { 0.5 } else { 1.0 });
                }
            }
        }
    }
}

#[test]
fn atmos_makeup_is_a_final_linked_gain_and_leaves_other_codecs_bit_identical() {
    for codec in ["eac3", "ac4", "truehd", "mlpa", "mpegh", "av3a", "alac", "adm"] {
        for spatial in [false, true] {
            let mut old = Engine::new(48000, 2);
            let mut new = Engine::new(48000, 2);
            for e in [&mut old, &mut new] {
                e.set_spatial_enhancement(spatial).unwrap();
                e.set_program_codec(codec.into());
                e.program_enabled = true;
                e.set_program_target(0.5, true);
            }
            declare_makeup_source(&mut new, true, "");
            let expected = if spatial && ["eac3", "ac4", "truehd", "mlpa"].contains(&codec) { 10f32.powf(1.5 / 20.) } else { 1.0 };
            for i in 0..24000 {
                let x = (i as f32 * 0.13).sin() * 0.003;
                let a = old.process_final_output([x, -0.4 * x]);
                let b = new.process_final_output([x, -0.4 * x]);
                for ear in 0..2 {
                    if expected == 1.0 { assert_eq!(a[ear], b[ear], "{codec} must stay bit-identical"); }
                    else { assert!((a[ear] * expected - b[ear]).abs() < 2e-7); }
                }
            }
        }
    }
}

#[test]
fn atmos_makeup_ramps_on_live_toggle_and_cannot_leak_into_the_next_programme() {
    let mut e = Engine::new(48000, 2);
    e.set_spatial_enhancement(true).unwrap();
    // Declarations may arrive before the codec announcement or balance setting.
    declare_makeup_source(&mut e, true, "");
    e.set_program_codec("eac3".into());
    e.output_active = true;
    e.paused = false;
    let fifo = stereo_fifo::StereoFifo::new(4096);
    let telemetry = RuntimeTelemetry::default();
    for enabled in [true, false, true] {
        let before = e.atmos_output_makeup.gain();
        assert!(protocol::apply_render_command(&mut e,
            render_command::RenderCommand::Command(Command::SetProgramEnabled { enabled }), &fifo, &telemetry));
        assert_eq!(e.atmos_output_makeup.gain(), before, "toggle must not step the gain");
        for _ in 0..960 {
            let previous = e.atmos_output_makeup.gain();
            e.advance_output_envelopes();
            assert!((e.atmos_output_makeup.gain() - previous).abs() < 0.00025);
        }
        let expected = if enabled { 10f32.powf(1.5 / 20.) } else { 1.0 };
        assert!((e.atmos_output_makeup.gain() - expected).abs() < 1e-6);
    }
    let before = e.atmos_output_makeup.gain();
    e.set_spatial_enhancement(false).unwrap();
    assert_eq!(e.atmos_output_makeup.gain(), before);
    for _ in 0..960 { e.advance_output_envelopes(); }
    assert_eq!(e.atmos_output_makeup.gain(), 1.0, "spatial-off comparison must retain its old level");
    e.paused = true;
    e.set_spatial_enhancement(true).unwrap();
    assert_eq!(e.atmos_output_makeup.target_db(), 1.5);
    e.sources.clear();
    e.refresh_atmos_output_makeup(true);
    assert_eq!(e.atmos_output_makeup.target_db(), 1.5, "silent object intervals retain programme identity");
    e.set_program_codec("mpegh".into());
    assert_eq!(e.atmos_output_makeup.target_db(), 0.0);
    e.reset_session(0);
    assert_eq!(e.atmos_output_makeup.gain(), 1.0);
    assert!(!e.atmos_programme_seen);
}

#[test]
fn atmos_makeup_remains_inside_the_existing_loudness_and_peak_guards() {
    for spatial in [false, true] {
        let mut e = Engine::new(48000, 2);
        e.set_spatial_enhancement(spatial).unwrap();
        e.set_program_codec("eac3".into());
        e.program_enabled = true;
        e.set_program_target(1.0, true);
        declare_makeup_source(&mut e, true, "");
        let mut sum = 0f64;
        for i in 0..48000 * 6 {
            let x = (i as f32 * std::f32::consts::TAU * 1000. / 48000.).sin() * 0.9;
            let y = e.process_final_output([x, -0.7 * x]);
            assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 0.871));
            assert!((y[1] + y[0] * 0.7).abs() < 2e-7);
            if i >= 48000 * 5 { sum += y.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>(); }
        }
        if spatial {
            // At 1 kHz the BS.1770 K weighting is approximately +0.7 dB.
            let lufs = -0.691 + 10. * (sum / 48000.).log10() + 0.7;
            assert!(lufs <= -18.0, "guard must include makeup: {lufs} LUFS");
            assert!(lufs > -19.1, "final makeup must not be cancelled by the nominal -20 target: {lufs}");
            assert!((e.output_dynamics.detector_target_lufs() + 19.0).abs() < 1e-6);
            assert!(e.output_dynamics.maximum_attenuation_db > 1.5);
        }
    }
}
