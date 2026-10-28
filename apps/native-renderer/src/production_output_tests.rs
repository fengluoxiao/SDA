//! Common output controls remain independent of source-group balance.
use super::*;
#[test]
fn generic_output_preserves_manual_and_program_volume_for_every_codec() {
    for codec in ["eac3", "mpegh", "av3a", "alac", "mp3", "mp3-stereo-bed", "adm", "future-object-format"] {
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
            let calibration = if ["mpegh","av3a","ac4","adm","alac","mp3","mp3-stereo-bed"].contains(&codec) { 10f32.powf(2.5/20.) } else { 1.0 };
            let ratio = 0.25 * 10f32.powf(-6. / 20.) * 0.5 * calibration;
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
        e.set_program_codec("adm".into());e.program_enabled=true;e.set_program_target(1.0,true);
        assert_eq!(e.programme_output_makeup.target_db(),2.5);
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
            assert!(e.output_dynamics.enabled(), "spatial-off must share the same final protection");
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
fn programme_makeup_obeys_format_volume_and_metadata_policy() {
    for codec in ["eac3", "ac4", "truehd", "mlpa", "ac3", "mpegh", "av3a", "alac", "mp3", "mp3-stereo-bed", "adm", "future-object-format"] {
        for volume in [false, true] {
            for spatial in [false, true] {
                for (object, label) in [(false, "FrontLeft"), (false, "Tfl"), (true, "")] {
                    let mut e = Engine::new(48000, 2);
                    e.set_spatial_enhancement(spatial).unwrap();
                    e.set_program_codec(codec.into());
                    e.program_enabled = volume;
                    e.set_program_target(0.5, true);
                    declare_makeup_source(&mut e, object, label);
                    let eligible = volume && (["mpegh","av3a","ac4","adm","alac","mp3","mp3-stereo-bed"].contains(&codec)
                        || (["eac3", "truehd", "mlpa"].contains(&codec) && (object || label == "Tfl")));
                    assert_eq!(e.programme_output_makeup.target_db(), if eligible { 2.5 } else { 0.0 },
                        "{codec} volume={volume} spatial={spatial} object={object} label={label}");
                    assert_eq!(e.program_metadata_gain, 1.0, "makeup must not rewrite reference metadata");
                    assert_eq!(e.program_gain, if volume { 0.5 } else { 1.0 });
                }
            }
        }
    }
}

#[test]
fn programme_makeup_is_linked_and_leaves_unselected_codecs_bit_identical() {
    for codec in ["eac3", "ac4", "truehd", "mlpa", "mpegh", "av3a", "alac", "mp3", "mp3-stereo-bed", "adm"] {
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
            // Reference bypasses only the selected format calibration.
            old.programme_output_makeup = Default::default();
            let expected = if ["eac3", "truehd", "mlpa", "mpegh", "av3a", "ac4", "adm", "alac", "mp3", "mp3-stereo-bed"].contains(&codec) { 10f32.powf(2.5 / 20.) } else { 1.0 };
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
        let before = e.programme_output_makeup.gain();
        assert!(protocol::apply_render_command(&mut e,
            render_command::RenderCommand::Command(Command::SetProgramEnabled { enabled }), &fifo, &telemetry));
        assert_eq!(e.programme_output_makeup.gain(), before, "toggle must not step the gain");
        for _ in 0..960 {
            let previous = e.programme_output_makeup.gain();
            e.advance_output_envelopes();
            assert!((e.programme_output_makeup.gain() - previous).abs() <= (10f32.powf(2.5 / 20.) - 1.0) / 960.0 + 960.0 * f32::EPSILON);
        }
        let expected = if enabled { 10f32.powf(2.5 / 20.) } else { 1.0 };
        assert!((e.programme_output_makeup.gain() - expected).abs() < 1e-6);
    }
    let before = e.programme_output_makeup.gain();
    e.set_spatial_enhancement(false).unwrap();
    assert_eq!(e.programme_output_makeup.gain(), before);
    for _ in 0..960 { e.advance_output_envelopes(); }
    assert!((e.programme_output_makeup.gain()-10f32.powf(2.5/20.)).abs()<1e-6,
        "Atmos programme compensation belongs to volume balance in both spatial states");
    e.paused = true;
    e.set_spatial_enhancement(true).unwrap();
    assert_eq!(e.programme_output_makeup.target_db(), 2.5);
    e.sources.clear();
    e.refresh_programme_output_makeup(true);
    assert_eq!(e.programme_output_makeup.target_db(), 2.5, "silent object intervals retain programme identity");
    e.set_program_codec("alac".into());
    assert_eq!(e.programme_output_makeup.target_db(), 2.5, "stereo retains the shared calibration");
    assert!(!e.dolby_spatial_programme_seen);
    e.set_program_codec("adm".into());
    assert_eq!(e.programme_output_makeup.target_db(), 2.5,"ADM retains the shared immersive calibration");
    e.set_program_codec("future-object-format".into());
    assert_eq!(e.programme_output_makeup.target_db(), 0.0);
    e.reset_session(0);
    assert_eq!(e.programme_output_makeup.gain(), 1.0);
    assert!(!e.dolby_spatial_programme_seen);
}

#[test]
fn atmos_makeup_keeps_peak_protection_without_live_loudness_ducking() {
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
        // Programme normalization is upstream. The final path meters loudness
        // without riding it; the linked true-peak checks above still apply.
        assert!((sum / 96000.).sqrt() > 0.1, "fixed compensation was silently ducked");
        assert_eq!(e.output_dynamics.gain_db(), 0.0);
        assert_eq!(e.output_dynamics.maximum_attenuation_db, 0.0);
        assert_eq!(e.program_gain, 1.0);
        assert_eq!(e.programme_output_makeup.target_db(), 2.5);
    }
}

#[test]
fn spatial_toggle_keeps_the_same_loudness_reference_and_envelope() {
    let mut reference = Engine::new(48000,2);
    let mut switched = Engine::new(48000,2);
    for e in [&mut reference,&mut switched] {
        e.set_spatial_enhancement(true).unwrap();
        e.output_active=true;e.paused=false;
    }
    for i in 0..48000 * 4 {
        let x=(i as f32 * 0.13).sin()*0.5;
        let a=reference.process_final_output([x,-x*0.7]);
        let b=switched.process_final_output([x,-x*0.7]);
        assert_eq!(a,b);
    }
    assert!(reference.output_dynamics.gain_db() < -5.);
    for enabled in [false,true,false] {
        let before=switched.output_dynamics.gain_db();
        switched.set_spatial_enhancement(enabled).unwrap();
        assert_eq!(switched.output_dynamics.gain_db(),before,"spatial toggle restarted final control");
        assert!(switched.output_dynamics.enabled());
        for i in 0..48000 {
            let x=(i as f32 * 0.13).sin()*0.5;
            assert_eq!(reference.process_final_output([x,-x*0.7]),
                switched.process_final_output([x,-x*0.7]),
                "same final mix changed level merely because spatial control toggled");
        }
    }
}

#[test]
fn atmos_volume_compensation_is_identical_across_spatial_switch_states() {
    let mut reference=Engine::new(48000,2);
    let mut switched=Engine::new(48000,2);
    for e in [&mut reference,&mut switched] {
        e.set_spatial_enhancement(true).unwrap();
        e.set_program_codec("eac3".into());
        e.program_enabled=true;
        e.set_program_target(0.5,true);
        declare_makeup_source(e,true,"");
        e.output_active=true;e.paused=false;
    }
    for enabled in [true,false,true,false] {
        let gain=switched.programme_output_makeup.gain();
        switched.set_spatial_enhancement(enabled).unwrap();
        assert_eq!(switched.programme_output_makeup.gain(),gain);
        assert_eq!(switched.programme_output_makeup.target_db(),2.5);
        for i in 0..12000 {
            reference.advance_output_envelopes();
            switched.advance_output_envelopes();
            let x=(i as f32*0.13).sin()*0.02;
            assert_eq!(reference.process_final_output([x,-x*0.7]),
                switched.process_final_output([x,-x*0.7]),
                "spatial toggle changed the common Dolby programme compensation");
        }
    }
}

#[test]
fn programme_makeup_survives_supported_format_switches_and_clears_for_unselected_programmes() {
    let mut e=Engine::new(48000,2);
    e.set_spatial_enhancement(true).unwrap();
    e.program_enabled=true;
    e.set_program_codec("eac3".into());
    e.set_program_target(0.5,true);
    declare_makeup_source(&mut e,true,"");
    e.output_active=true;e.paused=false;
    for codec in ["mpegh","av3a","ac4","truehd","mlpa","eac3","alac","mp3","mp3-stereo-bed"] {
        let previous=e.programme_output_makeup.gain();
        e.set_program_codec(codec.into());
        assert_eq!(e.programme_output_makeup.target_db(),2.5,"{codec}");
        assert_eq!(e.programme_output_makeup.gain(),previous,"format change must not restart calibration");
        assert_eq!(e.sources.len(),1);
        assert_eq!(e.program_gain,0.5);
    }
    e.set_program_codec("pcm".into());
    assert_eq!(e.programme_output_makeup.target_db(),0.0);
    for _ in 0..960 {e.advance_output_envelopes();}
    assert_eq!(e.programme_output_makeup.gain(),1.0);
    assert!(e.spatial_enhancement_enabled);
}

#[test]
fn programme_balanced_output_recovers_after_loud_sections_for_all_selected_formats() {
    for codec in ["eac3", "mpegh", "av3a", "ac4"] {
        for spatial in [false, true] {
            let mut e = Engine::new(48000, 2);
            e.set_program_codec(codec.into());
            e.program_enabled = true;
            e.set_program_target(0.5, true);
            e.set_spatial_enhancement(spatial).unwrap();
            declare_makeup_source(&mut e, true, "");
            let mut quiet_energy = [0f64; 2];
            for (phase, seconds, amplitude) in [(0, 2, 0.01), (1, 4, 0.5), (2, 20, 0.01)] {
                for i in 0..seconds * 48000 {
                    e.advance_output_envelopes();
                    let x = (i as f32 * std::f32::consts::TAU * 1000. / 48000.).sin() * amplitude;
                    let y = e.process_final_output([x, -0.7 * x]);
                    assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 0.871));
                    if phase != 1 && i >= seconds * 48000 - 24000 {
                        quiet_energy[phase / 2] += f64::from(y[0]).powi(2);
                    }
                }
            }
            let delta = 10. * (quiet_energy[1] / quiet_energy[0]).log10();
            assert!(delta.abs() < 0.15, "{codec} spatial={spatial}: the same quiet section stayed {delta} dB quieter");
        }
    }
}
#[test]
fn programme_gain_handoff_keeps_protected_output_continuous_for_spatial_formats() {
    for codec in ["eac3", "mpegh", "av3a", "ac4"] {
        for spatial in [false, true] {
            let mut e = Engine::new(48000, 2);
            e.set_program_codec(codec.into());
            e.program_enabled = true;
            e.set_program_target(1., true);
            e.set_spatial_enhancement(spatial).unwrap();
            declare_makeup_source(&mut e, true, "");
            e.set_master_preamp_db(0.).unwrap();
            e.set_output_volume(1., true);
            e.output_active = true;
            e.paused = false;
            let mut reference = 0.;
            for i in 0..48000 * 8 {
                e.advance_output_envelopes();
                let x = 0.4 * (i as f32 * std::f32::consts::TAU * 1000. / 48000.).sin();
                let y = e.process_final_output([x,-0.7*x]);
                if i >= 48000 * 7 { reference += f64::from(y[0]).powi(2); }
            }
            reference /= 48000.;
            assert!(e.output_dynamics.gain_db() < -6.);
            e.set_program_target(0.5, false);
            let mut worst = 0f64;
            for window in 0..40 {
                let mut power = 0.;
                for j in 0..4800 {
                    let i = window*4800+j;
                    e.advance_output_envelopes();
                    let x = 0.4 * (i as f32 * std::f32::consts::TAU * 1000. / 48000.).sin();
                    let y = e.process_final_output([x,-0.7*x]);
                    assert!(y.iter().all(|v|v.is_finite() && v.abs()<0.872));
                    power += f64::from(y[0]).powi(2);
                }
                worst = worst.min(10.*(power/4800./reference).log10());
            }
            assert!(worst > -0.2,"{codec}, spatial={spatial}: normalization stacked another {worst} dB cut");
        }
    }
}


#[test]
fn stereo_and_immersive_formats_share_peak_protection_at_full_output() {
    fn render(codec:&str,balance:bool,binaural:bool)->Vec<[f32;2]> {
        let mut e=Engine::new(48000,2);
        e.set_binaural_enabled(binaural);e.set_spatial_enhancement(binaural).unwrap();
        e.set_program_codec(codec.into());declare_makeup_source(&mut e,true,"");
        e.program_enabled=balance;e.set_program_target(1.,true);e.set_output_volume(1.,true);
        assert_eq!(e.programme_output_makeup.target_db(),if balance {2.5} else {0.});
        let ceiling=10f32.powf(true_peak_guard::CEILING_DB as f32/20.);
        let output:Vec<_>=(0..24000).map(|i| {
            let x=if i%3000<8 {2.5} else {(i as f32*0.51).sin()*1.2+(i as f32*0.013).sin()*0.2};
            let y=e.process_final_output([x,-x*0.43]);
            assert!(y.iter().all(|v|v.is_finite() && v.abs()<=ceiling+1e-7),"peak protection bypassed: {codec} balance={balance} binaural={binaural}: {y:?}");
            assert!((y[1]+y[0]*0.43).abs()<2e-7,"ears limited independently");
            y
        }).collect();
        assert!(output.iter().any(|v|v[0].abs()>0.4),"test accidentally muted output");
        assert_eq!(e.output_dynamics.maximum_attenuation_db,0.,"peak test introduced loudness ducking");
        output
    }
    for balance in [false,true] {for binaural in [false,true] {
        let reference=render("alac",balance,binaural);
        for codec in ["mp3","mp3-stereo-bed","eac3","truehd","mlpa","mpegh","av3a","ac4"] {
            assert_eq!(render(codec,balance,binaural),reference,"final peak protection differs for {codec}");
        }
    }}
}

#[test]
fn adm_file_metadata_and_baked_programme_gain_match_live_exactly_once() {
    for db in [0.0_f32,-6.0] { for spatial in [false,true] {
        let mut live=Engine::new(48000,2);live.set_spatial_enhancement(spatial).unwrap();
        live.set_program_codec("adm".into());live.program_enabled=true;
        live.set_program_target(cinema::db(db),true);
        let mut owned=Engine::new(48000,2);
        owned.configure_authored_processing(authored::Processing {enabled:true,version:1,programme_handled:false,
            source_spatial_handled:false,spatial_presentation:spatial,programme_gain_db:db,makeup_db:2.5,
            ..Default::default()}).unwrap();
        let mut baked=Engine::new(48000,2);
        baked.configure_authored_processing(authored::Processing {enabled:true,version:1,programme_handled:true,
            source_spatial_handled:true,spatial_presentation:spatial,programme_gain_db:db,makeup_db:2.5,
            ..Default::default()}).unwrap();
        assert_eq!(owned.programme_output_makeup.target_db(),2.5);
        assert_eq!(baked.programme_output_makeup.target_db(),0.0);
        for i in 0..24000 {
            let x=(i as f32*0.13).sin()*0.003;let input=[x,-0.4*x];
            let a=live.process_final_output(input);let b=owned.process_final_output(input);
            let c=baked.process_final_output(input.map(|v|v*cinema::db(db+2.5)));
            for ear in 0..2 {assert!((a[ear]-b[ear]).abs()<3e-7,"metadata gain mismatch");
                assert!((a[ear]-c[ear]).abs()<3e-7,"already baked gain applied twice");}
        }
    }}
}
