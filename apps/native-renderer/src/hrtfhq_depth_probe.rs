//! Offline listening attribution only. No production settings or assets change.
use super::*;
use std::io::Write;

#[test]
#[ignore = "requires SDA_DEPTH_AUDITION_DIR; synthetic movement audition only"]
fn export_moving_depth_attribution() {
    let out = std::path::PathBuf::from(std::env::var("SDA_DEPTH_AUDITION_DIR").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let legacy = hrtf::NativeHrtfSet::load_calibrated(
        &repo.join("apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json"),
    )
    .unwrap();
    let measured = hrtf::NativeHrtfSet::load_calibrated(
        &repo.join("assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json"),
    )
    .unwrap();
    let rate = 48000usize;
    let frames = rate * 17;
    let block = convolution::DEFAULT_PARTITION;
    let mut seed = 0x432174abu32;
    let mut low = 0.0f32;
    let input: Vec<f32> = (0..frames)
        .map(|n| {
            if n == rate / 4 {
                return 0.08;
            }
            let t = n as f32 / rate as f32;
            if !(1.0..16.0).contains(&t) {
                return 0.0;
            }
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 8) as f32 / 8388608.0 - 1.0;
            low += 0.12 * (noise - low);
            let pulse = (t * std::f32::consts::TAU * 3.0).sin().max(0.0).powi(2);
            let fade = ((t - 1.0) * 10.0).min((16.0 - t) * 10.0).clamp(0.0, 1.0);
            0.035
                * fade
                * pulse
                * (0.75 * low + 0.20 * (t * std::f32::consts::TAU * 220.0).sin() + 0.05 * noise)
        })
        .collect();
    let overhead = std::env::var("SDA_DEPTH_AUDITION_OVERHEAD").as_deref() == Ok("1");
    let trajectory_name = std::env::var("SDA_DEPTH_AUDITION_TRAJECTORY").unwrap_or_default();
    assert!(["", "bottom-to-top", "upper-left-to-right"].contains(&trajectory_name.as_str()));
    let mut report = Vec::new();
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        if (overhead || !trajectory_name.is_empty()) && layout != vbap::LayoutId::Sony360Ra13 {
            continue;
        }
        for mode in ["legacy61", "legacy61-dry", "hrtfhq", "integrated"] {
            if std::env::var("SDA_DEPTH_INTEGRATED_ONLY").as_deref() == Ok("1")
                && mode != "integrated"
            {
                continue;
            }
            let set = if mode == "integrated" {
                measured.clone()
            } else if mode == "hrtfhq" {
                measured.diagnostic_without_shared_depth()
            } else if mode == "legacy61-dry" {
                legacy.diagnostic_without_spatial_residual()
            } else {
                legacy.clone()
            };
            let mut e = Engine::new(rate as u32, 2);
            e.set_layout(layout).unwrap();
            e.replace_hrtf(set, 0.0).unwrap();
            e.set_spatial_enhancement(true).unwrap();
            e.set_master_preamp_db(-12.0).unwrap();
            e.set_program_codec(
                if layout == vbap::LayoutId::Sony360Ra13 {
                    "mpegh"
                } else {
                    "eac3"
                }
                .into(),
            );
            e.set_spatial_balance(false);
            e.front_common.enabled = false;
            e.set_direct_objects(true).unwrap();
            e.set_directional_hrtf(true);
            e.direct_mix = 1.0;
            e.output_active = true;
            e.paused = false;
            let source = Source {
                kind: SourceKind::Object,
                position: [0.0, 1.0, 0.0],
                gain: 1.0,
                target_gain: 1.0,
                availability: 1.0,
                availability_target: 1.0,
                ..Default::default()
            };
            e.sources.insert("moving-object".into(), source);
            let mut pcm = vec![0.0f32; frames * 2];
            let mut trajectory = Vec::new();
            for start in (0..frames).step_by(block) {
                let t = start as f64 / rate as f64;
                // Constant radius, identical authored gain. Front -> side -> rear -> other side -> front.
                // Then a raised front/rear pass to expose elevation and moving-object cues.
                let az = if t < 2.0 {
                    0.0
                } else if t < 10.0 {
                    (t - 2.0) / 8.0 * 360.0
                } else if t < 15.0 {
                    ((t - 10.0) / 5.0 * std::f64::consts::TAU).sin() * 150.0
                } else {
                    0.0
                };
                let el: f64 = if (10.0..15.0).contains(&t) { 25.0 } else { 0.0 };
                let (az, el) = if overhead {
                    if t < 2.0 {
                        (0.0, 0.0)
                    } else if t < 5.0 {
                        (0.0, (t - 2.0) * 30.0)
                    } else if t < 6.0 {
                        (0.0, 90.0)
                    } else if t < 9.0 {
                        (180.0, 90.0 - (t - 6.0) * 30.0)
                    } else if t < 10.0 {
                        (180.0, 0.0)
                    } else if t < 12.5 {
                        (90.0, (t - 10.0) * 36.0)
                    } else if t < 15.0 {
                        (-90.0, 90.0 - (t - 12.5) * 36.0)
                    } else {
                        (-90.0, 0.0)
                    }
                } else {
                    (az, el)
                };
                let (az, el) = requested_height_trajectory(&trajectory_name, t).unwrap_or((az, el));
                let (a, h) = (az.to_radians(), el.to_radians());
                let position = [
                    (-a.sin() * h.cos()) as f32,
                    (a.cos() * h.cos()) as f32,
                    h.sin() as f32,
                ];
                e.sources.get_mut("moving-object").unwrap().position = position;
                e.route_source_now("moving-object", 0).unwrap();
                let n = (frames - start).min(block);
                e.sources.get_mut("moving-object").unwrap().samples.write(
                    start as u64,
                    start as u64,
                    &input[start..start + n],
                );
                e.render_into(&mut pcm[start * 2..(start + n) * 2], 2);
                assert_eq!(
                    e.peak_guard.diagnostic_gain(),
                    1.0,
                    "peak guard would confound attribution"
                );
                assert_eq!(e.sources["moving-object"].distance_gain, 1.0);
                if mode == "hrtfhq" {
                    trajectory
                        .push(serde_json::json!({"sample":start,"azimuth":az,"elevation":el}));
                }
            }
            assert!(pcm.iter().all(|x| x.is_finite()));
            let name = format!("{}-{mode}", layout.as_str());
            let mut f = std::io::BufWriter::new(
                std::fs::File::create(out.join(format!("{name}.f32"))).unwrap(),
            );
            for x in &pcm {
                f.write_all(&x.to_le_bytes()).unwrap();
            }
            f.flush().unwrap();
            report.push(serde_json::json!({"name":name,"mode":mode,"layout":layout.as_str(),"frames":frames,"sampleRate":rate,"guard":1.0,"spatialCues":e.active_hrtf_set.as_ref().unwrap().spatial_cues_active(),"trajectory":trajectory}));
        }
    }
    std::fs::write(out.join("native-report.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"Synthetic constant-radius object and identical authored level; actual shared engine. No physical distance simulation or listening certification. Legacy residual is derived from matched full-minus-dry runs, not a second direct HRTF mixed into the candidate.","overhead":overhead,"trajectoryName":trajectory_name,"renders":report})).unwrap()).unwrap();
}

// The calibration impulse remains frontal. Listening starts at t=1s.
// Both paths stay one unit from the listener: no pass through a singular origin.
fn requested_height_trajectory(name: &str, t: f64) -> Option<(f64, f64)> {
    if name.is_empty() {
        return None;
    }
    if t < 1.0 {
        return Some((0.0, 0.0));
    }
    let u = t - 1.0;
    match name {
        "bottom-to-top" => Some((0.0, -90.0 + 180.0 * ((u - 2.0) / 10.0).clamp(0.0, 1.0))),
        "upper-left-to-right" => {
            let angle = if u < 7.0 {
                -45.0 + 45.0 * ((u - 2.0) / 5.0).clamp(0.0, 1.0)
            } else {
                45.0 * ((u - 8.0) / 5.0).clamp(0.0, 1.0)
            };
            Some((if angle <= 0.0 { 90.0 } else { -90.0 }, 90.0 - angle.abs()))
        }
        _ => panic!("unknown height trajectory"),
    }
}

#[test]
fn height_trajectory_geometry_is_continuous_and_reaches_requested_positions() {
    let xyz = |name, t| {
        let (a, e) = requested_height_trajectory(name, t).unwrap();
        let (a, e) = (a.to_radians(), e.to_radians());
        [-a.sin() * e.cos(), a.cos() * e.cos(), e.sin()]
    };
    let bottom = xyz("bottom-to-top", 1.0);
    let middle = xyz("bottom-to-top", 8.0);
    let top = xyz("bottom-to-top", 13.0);
    assert!(bottom[2] < -0.999 && middle[1] > 0.999 && top[2] > 0.999);
    let left = xyz("upper-left-to-right", 1.0);
    let zenith = xyz("upper-left-to-right", 8.5);
    let right = xyz("upper-left-to-right", 14.0);
    assert!(
        left[0] < -0.7 && right[0] > 0.7 && left[2] > 0.7 && right[2] > 0.7 && zenith[2] > 0.999
    );
    for name in ["bottom-to-top", "upper-left-to-right"] {
        let mut previous = xyz(name, 1.0);
        for ms in 1001..=16000 {
            let p = xyz(name, ms as f64 / 1000.0);
            assert!((p.iter().map(|v| v * v).sum::<f64>() - 1.0).abs() < 1e-12);
            assert!(
                p.iter()
                    .zip(previous)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    < 0.001,
                "jump at {ms}"
            );
            previous = p;
        }
    }
}

#[test]
#[ignore = "offline generation of the user-approved fixed residual bank"]
fn export_approved_residual_bank() {
    let out = std::path::PathBuf::from(std::env::var("SDA_DEPTH_BANK_DIR").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json");
    let legacy = hrtf::NativeHrtfSet::load_calibrated(&path).unwrap();
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        let mut set = legacy.clone();
        set.configure_spatial_cues(layout).unwrap();
        let reference = set.diagnostic_without_spatial_residual();
        let mut rows = Vec::new();
        let mut bytes = Vec::new();
        for s in vbap::speakers(layout) {
            let full = set
                .mixed_speaker(
                    s.name,
                    layout.as_str(),
                    s.azimuth as f64,
                    s.elevation as f64,
                    0.04,
                )
                .unwrap();
            let dry = reference
                .mixed_speaker(
                    s.name,
                    layout.as_str(),
                    s.azimuth as f64,
                    s.elevation as f64,
                    0.04,
                )
                .unwrap();
            let offset = bytes.len() / 4;
            assert_eq!(full.0.len(), dry.0.len());
            assert_eq!(full.1.len(), dry.1.len());
            for (f, d) in [(&full.0, &dry.0), (&full.1, &dry.1)] {
                for (&a, &b) in f.iter().zip(d) {
                    let x = ((a - b) as f64 * 0.5 * 1.0006917288377972) as f32;
                    assert!(x.is_finite());
                    bytes.extend_from_slice(&x.to_le_bytes());
                }
            }
            rows.push(serde_json::json!({"name":s.name,"azimuth":s.azimuth,"elevation":s.elevation,"offsetFloats":offset,"taps":full.0.len()}));
        }
        let tag = if layout == vbap::LayoutId::Dolby7_1_4 {
            "atmos714"
        } else {
            "sony360ra13"
        };
        std::fs::write(out.join(format!("{tag}.f32")), bytes).unwrap();
        std::fs::write(out.join(format!("{tag}.json")),serde_json::to_vec_pretty(&serde_json::json!({"version":1,"sampleRate":48000,"layout":layout.as_str(),"residualGain":0.5,"commonCalibration":1.0006917288377972,"commonShiftSamples":0,"source":"retained61 default35ms residual only","speakers":rows})).unwrap()).unwrap();
    }
}
