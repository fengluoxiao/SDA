use super::*;
fn manifest() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json")
}

#[test]
fn stereo_bass_retains_mid_side_and_single_channel_low_frequency_energy() {
    let set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    let frequencies = [32., 64., 120., 240., 1000., 4000.];
    const N: usize = 48000;
    let mono: Vec<f32> = (0..N)
        .map(|i| {
            frequencies
                .iter()
                .map(|f| (std::f64::consts::TAU * f * i as f64 / 48000.).sin() * 0.005)
                .sum::<f64>() as f32
        })
        .collect();
    for spatial in [false, true] {
        for upmix in [false, true] {
            for mode in ["mid", "side", "left"] {
                let mut e = Engine::new(48000, 2);
                e.set_layout(vbap::LayoutId::Dolby7_1_4).unwrap();
                e.replace_hrtf(set.clone(), 0.).unwrap();
                e.set_spatial_enhancement(spatial).unwrap();
                e.set_direct_objects(true).unwrap();
                e.set_directional_hrtf(true);
                e.direct_mix = 1.;
                e.set_program_codec("alac".into());
                e.output_active = true;
                e.paused = false;
                let labels = [
                    "L", "R", "C", "LFE", "Ls", "Rs", "Lb", "Rb", "Tfl", "Tfr", "Trl", "Trr",
                ];
                let mut channels = vec![Vec::with_capacity(N); 12];
                let mut low = 0.;
                let alpha = 1. - (-2. * std::f32::consts::PI * 120. / 48000.).exp();
                for x in &mono {
                    let (l, r) = (
                        *x,
                        if mode == "mid" {
                            *x
                        } else if mode == "side" {
                            -*x
                        } else {
                            0.
                        },
                    );
                    let mid = (l + r) * 0.5;
                    let side = (l - r) * 0.5;
                    low += alpha * (mid - low);
                    let values = if upmix {
                        [
                            l * 0.8,
                            r * 0.8,
                            mid * 0.5,
                            low * 0.25,
                            side * 0.34,
                            -side * 0.34,
                            side * 0.2,
                            -side * 0.2,
                            side * 0.14,
                            -side * 0.14,
                            side * 0.1,
                            -side * 0.1,
                        ]
                    } else {
                        [l, r, 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.]
                    };
                    for (c, v) in channels.iter_mut().zip(values) {
                        c.push(v);
                    }
                }
                for (label, pcm) in labels.into_iter().zip(channels) {
                    let mut s = Source {
                        kind: SourceKind::Bed,
                        bed_label: Some(label.into()),
                        gain: 1.,
                        target_gain: 1.,
                        availability: 1.,
                        availability_target: 1.,
                        ..Default::default()
                    };
                    Engine::set_source_route(&mut s, bed_route(label, &e.vbap), 0);
                    s.samples.write(0, 0, &pcm);
                    e.sources.insert(label.into(), s);
                }
                let mut output = vec![0.; N * 2];
                e.render_into(&mut output, 2);
                let db: Vec<_> = frequencies
                    .iter()
                    .map(|f| {
                        let amp = |ear: usize| {
                            let (mut re, mut im) = (0f64, 0f64);
                            for i in 24000..48000 {
                                let a = std::f64::consts::TAU * f * i as f64 / 48000.;
                                re += output[2 * i + ear] as f64 * a.cos();
                                im += output[2 * i + ear] as f64 * a.sin();
                            }
                            2. * re.hypot(im) / 24000.
                        };
                        let gain = ((amp(0).powi(2) + amp(1).powi(2))
                            / if mode == "left" { 1. } else { 2. })
                        .sqrt()
                            / 0.005;
                        (20. * gain.max(1e-12).log10() * 100.).round() / 100.
                    })
                    .collect();
                println!(
                    "STEREO_BASS spatial={spatial} upmix={upmix} mode={mode} frequencies={frequencies:?} db={db:?}"
                );
                assert!(output.iter().all(|x| x.is_finite()));
                if mode == "side" {
                    assert!(
                        db[0] > -3. && db[1] > -4. && db[2] > -5.,
                        "stereo bass cancelled: {db:?}"
                    );
                }
                if mode == "left" {
                    assert!(
                        db[0].abs() < 1. && db[1].abs() < 1.,
                        "single-ear bass lost energy: {db:?}"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "local PCM audit; requires SDA_STEREO_AUDIT_DIR"]
fn replay_stereo_pcm_for_bass_audit() {
    let root = std::path::PathBuf::from(
        std::env::var_os("SDA_STEREO_AUDIT_DIR").expect("external audit directory"),
    );
    let bytes = std::fs::read(root.join("fixture-stereo-48k.f32")).unwrap();
    let pcm: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    assert!(pcm.len() % 2 == 0 && pcm.iter().all(|v| v.is_finite()));
    let set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    let tag = std::env::var("SDA_STEREO_AUDIT_TAG").expect("explicit output tag");
    let baseline = std::env::var("SDA_STEREO_AUDIT_BASELINE").ok().as_deref() == Some("1");
    for amplitude in [0.25f32] {
        for upmix in [false, true] {
            let mut e = Engine::new(48000, 2);
            e.set_layout(vbap::LayoutId::Dolby7_1_4).unwrap();
            e.replace_hrtf(set.clone(), 0.).unwrap();
            e.set_spatial_enhancement(true).unwrap();
            e.set_direct_objects(true).unwrap();
            e.set_directional_hrtf(true);
            e.direct_mix = 1.;
            e.set_program_codec("alac".into());
            e.program_enabled = true;
            e.refresh_programme_output_makeup(true);
            if baseline {
                e.program_codec = Some("pcm".into());
            }
            e.output_active = true;
            e.paused = false;
            let labels = [
                "L", "R", "C", "LFE", "Ls", "Rs", "Lb", "Rb", "Tfl", "Tfr", "Trl", "Trr",
            ];
            let mut channels = vec![Vec::with_capacity(pcm.len() / 2); 12];
            let mut low = 0.;
            let alpha = 1. - (-2. * std::f32::consts::PI * 120. / 48000.).exp();
            for pair in pcm.chunks_exact(2) {
                let (l, r) = (pair[0] * amplitude, pair[1] * amplitude);
                let mid = (l + r) * 0.5;
                let side = (l - r) * 0.5;
                low += alpha * (mid - low);
                let values = if upmix {
                    [
                        l * 0.8,
                        r * 0.8,
                        mid * 0.5,
                        low * 0.25,
                        side * 0.34,
                        -side * 0.34,
                        side * 0.2,
                        -side * 0.2,
                        side * 0.14,
                        -side * 0.14,
                        side * 0.1,
                        -side * 0.1,
                    ]
                } else {
                    [l, r, 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.]
                };
                for (c, v) in channels.iter_mut().zip(values) {
                    c.push(v);
                }
            }
            for label in labels {
                let mut s = Source {
                    kind: SourceKind::Bed,
                    bed_label: Some(label.into()),
                    gain: 1.,
                    target_gain: 1.,
                    availability: 1.,
                    availability_target: 1.,
                    ..Default::default()
                };
                Engine::set_source_route(&mut s, bed_route(label, &e.vbap), 0);
                e.sources.insert(label.into(), s);
            }
            let mut output = vec![0.; pcm.len()];
            let mut minimum_guard = 1f32;
            for (index, block) in output.chunks_mut(8192).enumerate() {
                let start = index * 4096;
                let end = start + block.len() / 2;
                for (label, channel) in labels.iter().zip(&channels) {
                    e.sources.get_mut(*label).unwrap().samples.write(
                        e.sample_pos,
                        start as u64,
                        &channel[start..end],
                    );
                }
                for partition in block.chunks_mut(256) {
                    e.render_into(partition, 2);
                    minimum_guard = minimum_guard.min(e.peak_guard.diagnostic_gain());
                }
            }
            let file = std::fs::File::create(
                root.join(format!("native-{tag}-upmix-{upmix}-scale-{amplitude}.f32")),
            )
            .unwrap();
            use std::io::Write;
            let mut file = std::io::BufWriter::new(file);
            for v in &output {
                file.write_all(&v.to_le_bytes()).unwrap();
            }
            file.flush().unwrap();
            println!(
                "BASS_AUDIT upmix={upmix} scale={amplitude} minGuard={minimum_guard} frames={}",
                output.len() / 2
            );
        }
    }
}

#[test]
fn stereo_bass_follows_codec_and_survives_hrtf_or_layout_changes() {
    let set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    let mut e = Engine::new(48000, 2);
    e.set_layout(vbap::LayoutId::Dolby7_1_4).unwrap();
    e.replace_hrtf(set.clone(), 0.).unwrap();
    for codec in ["alac", "mp3", "mp3-stereo-bed"] {
        e.set_program_codec(codec.into());
        assert!(
            e.bus_renderer
                .as_ref()
                .unwrap()
                .stereo_bass_preservation_active()
        );
        let clock = e.sample_pos;
        e.replace_hrtf(set.clone(), 0.).unwrap();
        assert_eq!(clock, e.sample_pos);
        assert!(
            e.bus_renderer
                .as_ref()
                .unwrap()
                .stereo_bass_preservation_active()
        );
        e.set_layout(vbap::LayoutId::Stereo2_0).unwrap();
        assert!(
            e.bus_renderer
                .as_ref()
                .unwrap()
                .stereo_bass_preservation_active()
        );
        e.set_layout(vbap::LayoutId::Dolby7_1_4).unwrap();
        for other in ["eac3", "truehd", "mpegh", "av3a", "ac4", "pcm"] {
            e.set_program_codec(other.into());
            assert!(
                !e.bus_renderer
                    .as_ref()
                    .unwrap()
                    .stereo_bass_preservation_active(),
                "non-stereo profile changed: {other}"
            );
        }
    }
}


#[test]
#[ignore = "local stereo image FIR audit; requires SDA_STEREO_AUDIT_DIR"]
fn export_stereo_image_responses() {
    let root = std::path::PathBuf::from(std::env::var_os("SDA_STEREO_AUDIT_DIR").unwrap());
    std::fs::create_dir_all(&root).unwrap();
    let mut set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
    set.configure_spatial_cues(solver.layout()).unwrap();
    let mut dump = serde_json::Map::new();
    for (tag, wet) in [("dry", 0.), ("room", set.effective_wet(0.))] {
        let bank = crate::stereo_bass::prepare(&set, &solver, wet).unwrap().unwrap();
        for (name, pairs) in [("normal", bank.normal), ("preserved", bank.preserved)] {
            for (i, pair) in pairs.into_iter().enumerate() {
                dump.insert(format!("{tag}-{name}-{i}"), serde_json::json!([pair.0, pair.1]));
            }
        }
        for speaker in vbap::speakers(solver.layout()) {
            let pair = set.mixed_speaker(speaker.name, solver.layout().as_str(),
                speaker.azimuth as f64, speaker.elevation as f64, wet).unwrap();
            dump.insert(format!("{tag}-{}", speaker.name), serde_json::json!([pair.0,pair.1]));
        }
    }
    std::fs::write(root.join("filters.json"), serde_json::to_vec(&dump).unwrap()).unwrap();
}
