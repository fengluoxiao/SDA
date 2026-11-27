//! Rear-motion regression and optional offline attribution of the shipped profile.
//! No listening certification: these checks measure PCM cues, not perception.
use super::*;
const RATE: u32 = 48000;
fn manifest() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json")
}
fn position(az: f64, el: f64) -> [f32; 3] {
    let (a, e) = (az.to_radians(), el.to_radians());
    [
        (-a.sin() * e.cos()) as f32,
        (a.cos() * e.cos()) as f32,
        e.sin() as f32,
    ]
}
fn tone(pcm: &[f32], frequency: f64, ear: usize) -> f64 {
    let n = pcm.len() / 2;
    let (mut re, mut im) = (0., 0.);
    for (i, frame) in pcm.chunks_exact(2).enumerate() {
        let phase = std::f64::consts::TAU * frequency * i as f64 / f64::from(RATE);
        let window = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos();
        re += f64::from(frame[ear]) * window * phase.cos();
        im += f64::from(frame[ear]) * window * phase.sin();
    }
    4. * (re * re + im * im).sqrt() / n as f64
}
fn source(pos: [f32; 3]) -> Source {
    Source {
        kind: SourceKind::Object,
        position: pos,
        gain: 1.,
        target_gain: 1.,
        availability: 1.,
        availability_target: 1.,
        ..Default::default()
    }
}
fn render(
    enhanced: bool,
    depth: bool,
    fast: bool,
    layout: vbap::LayoutId,
    el: f64,
) -> (Vec<f32>, Vec<serde_json::Value>) {
    let mut e = Engine::new(RATE, 2);
    e.set_layout(layout).unwrap();
    let set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    e.replace_hrtf(
        if depth {
            set
        } else {
            set.diagnostic_without_shared_depth()
        },
        0.,
    )
    .unwrap();
    e.set_spatial_enhancement(enhanced).unwrap();
    e.set_program_codec(
        if layout == vbap::LayoutId::Sony360Ra13 {
            "mpegh"
        } else {
            "eac3"
        }
        .into(),
    );
    e.set_direct_objects(true).unwrap();
    e.set_directional_hrtf(true);
    e.set_output_volume(1., true);
    e.set_master_preamp_db(0.).unwrap();
    e.disable_fast_objects = !fast;
    e.direct_mix = 1.;
    e.output_active = true;
    e.paused = false;
    e.sources.insert("rear".into(), source(position(110., el)));
    e.sources.insert("front".into(), source([0., 1., 0.]));
    for id in ["rear", "front"] {
        e.route_source_now(id, 0).unwrap();
    }
    let frames = RATE as usize * 4;
    let mut pcm = vec![0.; frames * 2];
    let mut rows = Vec::new();
    let block = convolution::DEFAULT_PARTITION;
    for start in (0..frames).step_by(block) {
        let t = start as f64 / f64::from(RATE);
        let az = 110. + 140. * ((t - 0.75) / 2.5).clamp(0., 1.);
        let pos = position(az, el);
        e.sources.get_mut("rear").unwrap().position = pos;
        e.route_source_now("rear", 0).unwrap();
        let count = (frames - start).min(block);
        for (id, frequencies, amplitude) in [
            ("rear", &[250., 1000., 3000., 6000.][..], 0.0005),
            ("front", &[430., 1310., 4010.][..], 0.008),
        ] {
            let input: Vec<_> = (start..start + count)
                .map(|i| {
                    frequencies
                        .iter()
                        .map(|hz| {
                            (std::f64::consts::TAU * hz * i as f64 / f64::from(RATE)).sin() as f32
                                * amplitude
                        })
                        .sum::<f32>()
                })
                .collect();
            e.sources
                .get_mut(id)
                .unwrap()
                .samples
                .write(start as u64, start as u64, &input);
        }
        e.render_into(&mut pcm[start * 2..(start + count) * 2], 2);
        let s = &e.sources["rear"];
        let actual = s
            .continuous
            .as_ref()
            .unwrap()
            .effective_route()
            .unwrap()
            .0
            .position;
        let error = pos
            .iter()
            .zip(actual)
            .map(|(a, b)| f64::from(a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(
            error < 0.025,
            "rear route did not follow metadata: {az} {actual:?}, error={error}"
        );
        assert!(s.distance_gain == 1., "distance confounds rear test");
        assert_eq!(
            e.peak_guard.diagnostic_gain(),
            1.,
            "peak guard confounds rear test"
        );
        rows.push(serde_json::json!({"time":t,"azimuth":az,"route":actual,"detail":e.auxiliary_balance.directional_strength(),"occlusion":s.occlusion_targets}));
    }
    assert!(pcm.iter().all(|v| v.is_finite()));
    (pcm, rows)
}

#[test]
#[ignore = "offline native PCM audit; set SDA_REAR_AUDIT_DIR"]
fn export_rear_motion_attribution() {
    use std::io::Write;
    let out = std::path::PathBuf::from(std::env::var_os("SDA_REAR_AUDIT_DIR").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let mut reports = Vec::new();
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        for depth in [false, true] {
            for enhanced in [false, true] {
                let name = format!("{}-depth{}-balance{}", layout.as_str(), depth, enhanced);
                let (pcm, routes) = render(enhanced, depth, true, layout, 0.);
                let mut f = std::io::BufWriter::new(
                    std::fs::File::create(out.join(format!("{name}.f32"))).unwrap(),
                );
                for v in &pcm {
                    f.write_all(&v.to_le_bytes()).unwrap();
                }
                f.flush().unwrap();
                let windows:Vec<_>=(5..39).map(|i|{
                let start=i*4800;let chunk=&pcm[start*2..(start+4800)*2];
                let levels:Vec<_>=[250.,1000.,3000.,6000.].into_iter().map(|hz| {
                    let a=tone(chunk,hz,0);let b=tone(chunk,hz,1);
                    serde_json::json!({"hz":hz,"left":a,"right":b,"ildDb":20.*(a/b).max(1e-30).log10()})
                }).collect();
                serde_json::json!({"time":i as f64/10.,"bands":levels})
            }).collect();
                eprintln!("rendered {name}");
                reports.push(serde_json::json!({"name":name,"depth":depth,"balance":enhanced,"layout":layout.as_str(),"windows":windows,"routes":routes}));
                std::fs::write(
                    out.join("report.json"),
                    serde_json::to_vec_pretty(&reports).unwrap(),
                )
                .unwrap();
            }
        }
    }
}

#[test]
#[ignore = "offline HRTF sweep; set SDA_REAR_AUDIT_DIR"]
fn export_rear_hrtf_sweep() {
    use std::io::Write;
    let out = std::path::PathBuf::from(std::env::var_os("SDA_REAR_AUDIT_DIR").unwrap());
    let mut rows = Vec::new();
    for clarity in [false, true] {
        let set = if clarity {
            hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap()
        } else {
            hrtf::NativeHrtfSet::load_without_shared_clarity(&manifest()).unwrap()
        };
        for el in [0., 30.] {
            for step in 0..=720 {
                let az = step as f64 * 0.5;
                let pos = position(az, el);
                let direction = directional::Direction { perceptual_width: false,
                    position: pos,
                    head: None,
                    width: 0.,
                    height: 0.,
                    depth: 0.,
                    diffuse: 0.,
                    horizontal_only: false,
                };
                let gains = bus_renderer::route(&vbap::VbapSolver::new(), pos, None, 0.);
                let pair = set
                    .directional_dry_compact(
                        direction,
                        vbap::LayoutId::Dolby7_1_4,
                        gains,
                        [0.; vbap::MAX_BUS_COUNT],
                    )
                    .unwrap();
                let energy: [f64; 2] =
                    [&pair.0, &pair.1].map(|ear| ear.iter().map(|v| f64::from(*v).powi(2)).sum());
                let nearest = set.nearest(az as f64, el).unwrap();
                let taps = nearest.dry.len() / 2;
                let measured_energy: f64 = nearest.dry.iter().map(|v| f64::from(*v).powi(2)).sum();
                let frequencies:Vec<_>=[250.,500.,1000.,2000.,3000.,6000.,8000.].into_iter().map(|hz| {
                let response=|ear:&[f32]| {
                    let(mut re,mut im)=(0.,0.);
                    for(i,x)in ear.iter().enumerate(){let phase=std::f64::consts::TAU*hz*i as f64/f64::from(RATE);re+=f64::from(*x)*phase.cos();im+=f64::from(*x)*phase.sin();}
                    (re*re+im*im).sqrt()
                };
                let left=response(&pair.0);let right=response(&pair.1);
                let ml=response(&nearest.dry[..taps]);let mr=response(&nearest.dry[taps..]);
                serde_json::json!({"hz":hz,"left":left,"right":right,"ildDb":20.*(left/right).max(1e-30).log10(),"vsNearestDb":10.*((left*left+right*right)/(ml*ml+mr*mr)).max(1e-30).log10()})
            }).collect();
                let name = format!("ir-clarity{clarity}-el{el}-az{az}.f32");
                let mut f = std::io::BufWriter::new(std::fs::File::create(out.join(name)).unwrap());
                for i in 0..pair.0.len() {
                    for ear in [&pair.0, &pair.1] {
                        f.write_all(&ear[i].to_le_bytes()).unwrap();
                    }
                }
                f.flush().unwrap();
                rows.push(serde_json::json!({"clarity":clarity,"azimuth":az,"elevation":el,"energy":energy,"vsNearestDb":10.*(energy.iter().sum::<f64>()/measured_energy).log10(),"frequencies":frequencies}));
            }
        }
    }
    std::fs::write(
        out.join("hrtf-sweep.json"),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
}

#[test]
fn rear_motion_preserves_routes_and_ear_cues_in_both_mixers() {
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        let (off, _) = render(false, true, true, layout, 0.);
        let (on, _) = render(true, true, true, layout, 0.);
        let (general, _) = render(true, true, false, layout, 0.);
        let energy = on.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
        let error = on
            .iter()
            .zip(&general)
            .map(|(a, b)| f64::from(a - b).powi(2))
            .sum::<f64>();
        assert!(
            (error / energy).sqrt() < 1e-4,
            "rear PCM differs between mixers: {}",
            layout.as_str()
        );
        for i in 12..=30 {
            let start = i * 4800;
            let a = &off[start * 2..(start + 4800) * 2];
            let b = &on[start * 2..(start + 4800) * 2];
            for hz in [250., 1000., 3000., 6000.] {
                if hz <= 1000. {
                    for ear in 0..2 {
                        let db = 20. * (tone(b, hz, ear) / tone(a, hz, ear)).log10();
                        assert!(
                            db >= -0.5,
                            "rear body lost {db}dB at {hz}Hz, t={}: {}",
                            i as f64 / 10.,
                            layout.as_str()
                        );
                    }
                }
                let ild = |x: &[f32]| 20. * (tone(x, hz, 0) / tone(x, hz, 1)).log10();
                assert!(
                    (ild(a) - ild(b)).abs() < 0.05,
                    "balance changed ear cue at {hz}Hz, t={}: {}",
                    i as f64 / 10.,
                    layout.as_str()
                );
            }
        }
        let ild = |i: usize| {
            let start = i * 4800;
            let pcm = &on[start * 2..(start + 4800) * 2];
            20. * (tone(pcm, 1000., 0) / tone(pcm, 1000., 1)).log10()
        };
        assert!(
            ild(12) > 5. && ild(30) < -5.,
            "rear source did not move across the listener: {}",
            layout.as_str()
        );
    }
}

#[test]
#[ignore = "offline overhead HRTF audit; set SDA_REAR_AUDIT_DIR"]
fn export_overhead_response_audit() {
    let out = std::path::PathBuf::from(std::env::var_os("SDA_REAR_AUDIT_DIR").unwrap());
    let mut rows = Vec::new();
    for clarity in [false, true] {
        let set = if clarity {
            hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap()
        } else {
            hrtf::NativeHrtfSet::load_without_shared_clarity(&manifest()).unwrap()
        };
        for (mode, width, height, diffuse) in [
            ("point", 0., 0., 0.),
            ("wide", 30., 0., 0.),
            ("tall", 0., 30., 0.),
            ("square", 30., 30., 0.),
            ("diffuse", 0., 0., 0.35),
        ] {
            let mut previous: Option<(f64, Vec<f32>, f64)> = None;
            for theta in [
                0.0f64, 30., 60., 75., 85., 89., 89.9, 89.99, 90., 90.01, 90.1, 91., 95., 105.,
                120., 150., 180.,
            ] {
                let t = theta.to_radians();
                let pos = [0., t.cos() as f32, t.sin() as f32];
                let direction = directional::Direction { perceptual_width: false,
                    position: pos,
                    head: None,
                    width,
                    height,
                    depth: 0.,
                    diffuse,
                    horizontal_only: false,
                };
                let gains = bus_renderer::route(&vbap::VbapSolver::new(), pos, None, 0.);
                let pair = set
                    .directional_dry_compact(
                        direction,
                        vbap::LayoutId::Dolby7_1_4,
                        gains,
                        [0.; vbap::MAX_BUS_COUNT],
                    )
                    .unwrap();
                let joined: Vec<_> = pair.0.iter().chain(&pair.1).copied().collect();
                let energy = joined.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
                let neighbor=previous.as_ref().map(|(angle,old,old_energy)| {
                    let n=joined.len().max(old.len());
                    let delta=(0..n).map(|i|f64::from(joined.get(i).copied().unwrap_or(0.)-old.get(i).copied().unwrap_or(0.)).powi(2)).sum::<f64>();
                    serde_json::json!({"from":angle,"relativeDifference":(delta/((energy+old_energy)*0.5)).sqrt()})
                });
                let bands:Vec<_>=[250.,1000.,2000.,3000.,6000.,10000.].into_iter().map(|hz| {
                    let response=|ear:&[f32]| {
                        let(mut re,mut im)=(0.,0.);for(i,x)in ear.iter().enumerate(){let p=std::f64::consts::TAU*hz*i as f64/f64::from(RATE);re+=f64::from(*x)*p.cos();im+=f64::from(*x)*p.sin();}(re*re+im*im).sqrt()
                    };
                    serde_json::json!({"hz":hz,"left":response(&pair.0),"right":response(&pair.1)})
                }).collect();
                assert!(joined.iter().all(|v| v.is_finite()));
                rows.push(serde_json::json!({"clarity":clarity,"mode":mode,"theta":theta,"position":pos,"energy":energy,"neighbor":neighbor,"bands":bands}));
                previous = Some((theta, joined, energy));
            }
        }
    }
    std::fs::write(
        out.join("overhead-responses.json"),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "offline overhead PCM audit; set SDA_REAR_AUDIT_DIR"]
fn export_overhead_pcm_audit() {
    use std::io::Write;
    let out = std::path::PathBuf::from(std::env::var_os("SDA_REAR_AUDIT_DIR").unwrap());
    let mut reports = Vec::new();
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        for depth in [false, true] {
            for balance in [false, true] {
                let mut e = Engine::new(RATE, 2);
                e.set_layout(layout).unwrap();
                let set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
                e.replace_hrtf(
                    if depth {
                        set
                    } else {
                        set.diagnostic_without_shared_depth()
                    },
                    0.,
                )
                .unwrap();
                e.set_spatial_enhancement(balance).unwrap();
                e.set_program_codec(
                    if layout == vbap::LayoutId::Sony360Ra13 {
                        "mpegh"
                    } else {
                        "eac3"
                    }
                    .into(),
                );
                e.set_direct_objects(true).unwrap();
                e.set_directional_hrtf(true);
                e.set_output_volume(1., true);
                e.set_master_preamp_db(0.).unwrap();
                e.direct_mix = 1.;
                e.output_active = true;
                e.paused = false;
                e.sources.insert("top".into(), source([0., 0.8660254, 0.5]));
                e.sources.insert("front".into(), source([0., 1., 0.]));
                for id in ["top", "front"] {
                    e.route_source_now(id, 0).unwrap();
                }
                let frames = RATE as usize * 4;
                let mut pcm = vec![0.; frames * 2];
                let block = convolution::DEFAULT_PARTITION;
                let mut path = Vec::new();
                for start in (0..frames).step_by(block) {
                    let time = start as f64 / f64::from(RATE);
                    let theta = 30. + 120. * ((time - 0.75) / 2.5).clamp(0., 1.);
                    let t = theta.to_radians();
                    let pos = [0., t.cos() as f32, t.sin() as f32];
                    e.sources.get_mut("top").unwrap().position = pos;
                    e.route_source_now("top", 0).unwrap();
                    let count = (frames - start).min(block);
                    for (id, frequencies, amplitude) in [
                        ("top", &[250., 1000., 6000., 10000.][..], 0.0005),
                        ("front", &[430., 1310., 4010.][..], 0.008),
                    ] {
                        let samples: Vec<f32> = (start..start + count)
                            .map(|i| {
                                frequencies
                                    .iter()
                                    .map(|hz| {
                                        (std::f64::consts::TAU * hz * i as f64 / f64::from(RATE))
                                            .sin() as f32
                                            * amplitude
                                    })
                                    .sum()
                            })
                            .collect();
                        e.sources.get_mut(id).unwrap().samples.write(
                            start as u64,
                            start as u64,
                            &samples,
                        );
                    }
                    e.render_into(&mut pcm[start * 2..(start + count) * 2], 2);
                    let s = &e.sources["top"];
                    let actual = s
                        .continuous
                        .as_ref()
                        .unwrap()
                        .effective_route()
                        .unwrap()
                        .0
                        .position;
                    let error = pos
                        .iter()
                        .zip(actual)
                        .map(|(a, b)| f64::from(a - b).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    assert!(
                        error < 0.025,
                        "overhead route stopped following position: {theta} {actual:?}"
                    );
                    assert_eq!(s.distance_gain, 1.);
                    assert_eq!(s.occlusion_targets, [1.; 2]);
                    assert_eq!(e.peak_guard.diagnostic_gain(), 1.);
                    path.push(serde_json::json!({"time":time,"theta":theta,"position":pos,"actual":actual,"motionResidualGain":s.motion_cues.gain(),"detailStrength":e.auxiliary_balance.directional_strength()}));
                }
                let name = format!("top-{}-depth{}-balance{}", layout.as_str(), depth, balance);
                let mut f = std::io::BufWriter::new(
                    std::fs::File::create(out.join(format!("{name}.f32"))).unwrap(),
                );
                for x in &pcm {
                    f.write_all(&x.to_le_bytes()).unwrap();
                }
                f.flush().unwrap();
                let windows:Vec<_>=(5..39).map(|i|{let chunk=&pcm[i*4800*2..(i+1)*4800*2];let bands:Vec<_>=[250.,1000.,6000.,10000.].into_iter().map(|hz|serde_json::json!({"hz":hz,"left":tone(chunk,hz,0),"right":tone(chunk,hz,1)})).collect();serde_json::json!({"time":i as f64/10.,"bands":bands})}).collect();
                reports.push(serde_json::json!({"name":name,"layout":layout.as_str(),"depth":depth,"balance":balance,"path":path,"windows":windows}));
                std::fs::write(
                    out.join("overhead-pcm.json"),
                    serde_json::to_vec_pretty(&reports).unwrap(),
                )
                .unwrap();
                eprintln!("rendered {name}");
            }
        }
    }
}


#[test]
fn overhead_extent_and_diffuse_targets_cross_poles_continuously() {
    let set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        let solver = vbap::VbapSolver::with_layout(layout);
        for pole in [90.0f64, -90.0] {
            for (width, height, diffuse) in [
                (0., 0., 0.),
                (30., 0., 0.),
                (0., 30., 0.),
                (30., 30., 0.),
                (0., 0., 0.35),
                (30., 30., 0.35),
            ] {
                let pair = |theta: f64| {
                    let t = theta.to_radians();
                    let p = [0., t.cos() as f32, t.sin() as f32];
                    set.directional_dry_compact(
                        directional::Direction { perceptual_width: false,
                            position: p,
                            head: None,
                            width,
                            height,
                            depth: 0.,
                            diffuse,
                            horizontal_only: false,
                        },
                        layout,
                        bus_renderer::route(&solver, p, None, 0.),
                        [0.; vbap::MAX_BUS_COUNT],
                    )
                    .unwrap()
                };
                let a = pair(pole - 0.01);
                let b = pair(pole + 0.01);
                let energy = |p: &(Vec<f32>, Vec<f32>)| {
                    p.0.iter()
                        .chain(&p.1)
                        .map(|v| f64::from(*v).powi(2))
                        .sum::<f64>()
                };
                let ea = energy(&a);
                let eb = energy(&b);
                let delta =
                    a.0.iter()
                        .chain(&a.1)
                        .zip(b.0.iter().chain(&b.1))
                        .map(|(x, y)| f64::from(x - y).powi(2))
                        .sum::<f64>();
                let relative = (delta / ((ea + eb) * 0.5)).sqrt();
                let db = 10. * (eb / ea).log10();
                eprintln!("pole={pole} layout={} width={width} height={height} diffuse={diffuse} error={relative:.8} energyDb={db:.6}",layout.as_str());
                assert!(relative<0.005 && db.abs()<0.02,"pole discontinuity: {} pole={pole} extent={width}/{height} diffuse={diffuse} delta={relative} db={db}",layout.as_str());
            }
        }
    }
}


#[test]
fn oblique_extents_retain_bearing_at_exact_poles() {
    let set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        let solver = vbap::VbapSolver::with_layout(layout);
        for azimuth in [0.0f64, 45., 90., 135.] {
            for (width, height) in [(30., 0.), (0., 30.), (30., 30.)] {
                let mut previous: Option<(Vec<f32>, Vec<f32>)> = None;
                for theta in [89.99f64, 90., 90.01, -89.99, -90., -90.01] {
                    if theta == -89.99 {
                        previous = None;
                    }
                    let t = theta.to_radians();
                    let a = azimuth.to_radians();
                    let p = [
                        (-a.sin() * t.cos()) as f32,
                        (a.cos() * t.cos()) as f32,
                        t.sin() as f32,
                    ];
                    let pair = set
                        .directional_dry_compact(
                            directional::Direction { perceptual_width: false,
                                position: p,
                                head: None,
                                width,
                                height,
                                depth: 0.,
                                diffuse: 0.,
                                horizontal_only: false,
                            },
                            layout,
                            bus_renderer::route(&solver, p, None, 0.),
                            [0.; vbap::MAX_BUS_COUNT],
                        )
                        .unwrap();
                    if let Some(old) = previous {
                        let energy = old
                            .0
                            .iter()
                            .chain(&old.1)
                            .map(|v| f64::from(*v).powi(2))
                            .sum::<f64>();
                        let delta = old
                            .0
                            .iter()
                            .chain(&old.1)
                            .zip(pair.0.iter().chain(&pair.1))
                            .map(|(x, y)| f64::from(x - y).powi(2))
                            .sum::<f64>();
                        assert!((delta/energy).sqrt()<0.005,"oblique pole jump: {} az={azimuth} theta={theta} extent={width}/{height} error={}",layout.as_str(),(delta/energy).sqrt());
                    }
                    previous = Some(pair);
                }
            }
        }
    }
}


#[test]
#[ignore = "offline rear clarity attribution; set SDA_REAR_AUDIT_DIR"]
fn export_rear_continuation_matrix() {
    let out = std::path::PathBuf::from(std::env::var_os("SDA_REAR_AUDIT_DIR").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let mut report = Vec::new();
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        for (name, speed, spread, diffuse, depth, occlusion, distance) in [
            ("slow-point", 5., 0., 0., false, false, None),
            ("medium-point", 15., 0., 0., false, false, None),
            ("fast-point", 45., 0., 0., false, false, None),
            ("slow-residual", 5., 0., 0., true, false, None),
            ("medium-residual", 15., 0., 0., true, false, None),
            ("fast-residual", 45., 0., 0., true, false, None),
            ("wide", 15., 0.5, 0., false, false, None),
            ("diffuse", 15., 0., 0.35, false, false, None),
            ("wide-diffuse", 15., 0.5, 0.35, false, false, None),
            ("shadow", 15., 0., 0., false, true, None),
            ("physical-near", 15., 0., 0., false, false, Some(0.35)),
        ] {
            let mut e = Engine::new(RATE, 2);
            e.set_layout(layout).unwrap();
            let set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
            e.replace_hrtf(
                if depth {
                    set
                } else {
                    set.diagnostic_without_shared_depth()
                },
                0.,
            )
            .unwrap();
            e.set_spatial_enhancement(true).unwrap();
            e.set_program_codec(
                if layout == vbap::LayoutId::Sony360Ra13 {
                    "mpegh"
                } else {
                    "eac3"
                }
                .into(),
            );
            e.set_direct_objects(true).unwrap();
            e.set_directional_hrtf(true);
            e.set_output_volume(1., true);
            e.set_master_preamp_db(0.).unwrap();
            e.source_occlusion_enabled = occlusion;
            e.direct_mix = 1.;
            e.output_active = true;
            e.paused = false;
            let mut rear = source(position(130., 0.));
            rear.spread = spread;
            rear.diffuse = diffuse;
            rear.distance_m = distance;
            e.sources.insert("rear".into(), rear);
            e.sources.insert("front".into(), source([0., 1., 0.]));
            if occlusion {
                e.sources.insert(
                    "occluder".into(),
                    source(position(130., 0.).map(|v| v * 0.2)),
                );
            }
            for id in ["rear", "front", "occluder"] {
                if e.sources.contains_key(id) {
                    e.route_source_now(id, 0).unwrap();
                }
            }
            let frames = RATE as usize * 3;
            let block = convolution::DEFAULT_PARTITION;
            let mut pcm = vec![0.; frames * 2];
            let mut previous = None;
            let mut held = 0usize;
            let mut max_held = 0;
            let mut max_angle = 0.0f64;
            let mut changed = 0;
            let mut controls = Vec::new();
            let mut render_seconds = 0.0;
            for start in (0..frames).step_by(block) {
                let time = start as f64 / f64::from(RATE);
                let az = 130. + speed * (time - 0.5).max(0.);
                let p = position(az, 0.);
                e.sources.get_mut("rear").unwrap().position = p;
                e.route_source_now("rear", 0).unwrap();
                if occlusion {
                    e.sources.get_mut("occluder").unwrap().position = p.map(|v| v * 0.2);
                    e.route_source_now("occluder", 0).unwrap();
                }
                let count = (frames - start).min(block);
                for (id, frequencies, amplitude) in [
                    ("rear", &[250., 1000., 3000., 6000.][..], 0.0005),
                    ("front", &[430., 1310., 4010.][..], 0.008),
                    ("occluder", &[170., 2370.][..], 0.002),
                ] {
                    if let Some(s) = e.sources.get_mut(id) {
                        let input: Vec<_> = (start..start + count)
                            .map(|i| {
                                frequencies
                                    .iter()
                                    .map(|hz| {
                                        (std::f64::consts::TAU * hz * i as f64 / f64::from(RATE))
                                            .sin() as f32
                                            * amplitude
                                    })
                                    .sum()
                            })
                            .collect();
                        s.samples.write(start as u64, start as u64, &input);
                    }
                }
                let started = std::time::Instant::now();
                e.render_into(&mut pcm[start * 2..(start + count) * 2], 2);
                render_seconds += started.elapsed().as_secs_f64();
                let s = &e.sources["rear"];
                let route = s.continuous.as_ref().unwrap().effective_route().unwrap().0;
                let cross = [
                    p[1] * route.position[2] - p[2] * route.position[1],
                    p[2] * route.position[0] - p[0] * route.position[2],
                    p[0] * route.position[1] - p[1] * route.position[0],
                ];
                let angle = f64::from(cross.iter().map(|v| v * v).sum::<f32>().sqrt())
                    .atan2(f64::from(
                        p.iter()
                            .zip(route.position)
                            .map(|(a, b)| a * b)
                            .sum::<f32>(),
                    ))
                    .to_degrees();
                if time > 0.6 {
                    max_angle = max_angle.max(angle);
                    if previous == Some(route.position) {
                        held += count;
                    } else {
                        max_held = max_held.max(held);
                        held = 0;
                        changed += 1;
                    }
                }
                previous = Some(route.position);
                controls.push(serde_json::json!({"time":time,"angle":az,"actual":route.position,"lagDegrees":angle,"distanceGain":s.distance_gain,"near":s.near_target,"shadow":s.occlusion_targets,"residual":s.motion_cues.gain()}));
            }
            max_held = max_held.max(held);
            let windows:Vec<_>=(10..29).map(|i|{let a=&pcm[i*4800*2..(i+1)*4800*2];let bands:Vec<_>=[250.,1000.,3000.,6000.].into_iter().map(|hz|serde_json::json!({"hz":hz,"left":tone(a,hz,0),"right":tone(a,hz,1)})).collect();serde_json::json!({"time":i as f64/10.,"bands":bands})}).collect();
            let row = serde_json::json!({"layout":layout.as_str(),"case":name,"speed":speed,"spread":spread,"diffuse":diffuse,"depth":depth,"occlusion":occlusion,"distance":distance,"maxLagDegrees":max_angle,"maxHeldMs":max_held as f64*1000./f64::from(RATE),"routeChanges":changed,"nativeRenderSeconds":render_seconds,"controls":controls,"windows":windows});
            eprintln!(
                "rear {} {name}: max lag={max_angle:.5} deg max hold={:.3} ms updates={changed}",
                layout.as_str(),
                max_held as f64 * 1000. / f64::from(RATE)
            );
            report.push(row);
            std::fs::write(
                out.join("matrix.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            )
            .unwrap();
        }
    }
}


#[test]
fn slow_rear_motion_does_not_hold_subdegree_filters_for_hundreds_of_ms() {
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        for fast in [false, true] {
            let mut e = Engine::new(RATE, 2);
            e.set_layout(layout).unwrap();
            let set = hrtf::NativeHrtfSet::load_calibrated(&manifest())
                .unwrap()
                .diagnostic_without_shared_depth();
            e.replace_hrtf(set, 0.).unwrap();
            e.set_direct_objects(true).unwrap();
            e.set_directional_hrtf(true);
            e.set_output_volume(1., true);
            e.set_master_preamp_db(0.).unwrap();
            e.disable_fast_objects = !fast;
            e.source_occlusion_enabled = false;
            e.direct_mix = 1.;
            e.output_active = true;
            e.paused = false;
            e.sources.insert("rear".into(), source(position(130., 0.)));
            e.route_source_now("rear", 0).unwrap();
            let block = convolution::DEFAULT_PARTITION;
            let mut previous = None;
            let mut held = 0;
            let mut max_held = 0;
            let mut max_angle = 0.0f64;
            for start in (0..RATE as usize * 2).step_by(block) {
                let p = position(130. + 5. * start as f64 / f64::from(RATE), 0.);
                e.sources.get_mut("rear").unwrap().position = p;
                e.route_source_now("rear", 0).unwrap();
                let count = block.min(RATE as usize * 2 - start);
                let input: Vec<_> = (start..start + count)
                    .map(|i| {
                        (std::f64::consts::TAU * 1000. * i as f64 / f64::from(RATE)).sin() as f32
                            * 0.001
                    })
                    .collect();
                e.sources.get_mut("rear").unwrap().samples.write(
                    start as u64,
                    start as u64,
                    &input,
                );
                let mut pcm = vec![0.; count * 2];
                e.render_into(&mut pcm, 2);
                let actual = e.sources["rear"]
                    .continuous
                    .as_ref()
                    .unwrap()
                    .effective_route()
                    .unwrap()
                    .0
                    .position;
                let angle = f64::from(p[0] * actual[1] - p[1] * actual[0])
                    .abs()
                    .atan2(f64::from(
                        p.iter().zip(actual).map(|(a, b)| a * b).sum::<f32>(),
                    ))
                    .to_degrees();
                max_angle = max_angle.max(angle);
                if previous == Some(actual) {
                    held += count;
                } else {
                    max_held = max_held.max(held);
                    held = 0;
                }
                previous = Some(actual);
            }
            max_held = max_held.max(held);
            assert!(max_angle<0.4 && max_held<=3*block,"slow rear filter held too long: layout={} fast={fast} lag={max_angle} deg hold={}ms",layout.as_str(),max_held as f64*1000./f64::from(RATE));
        }
    }
}


#[test]
#[ignore = "offline one-degree rear hemisphere IR scan; set SDA_REAR_AUDIT_DIR"]
fn export_rear_vertical_hemisphere_grid() {
    use std::io::Write;
    let out = std::path::PathBuf::from(std::env::var_os("SDA_REAR_AUDIT_DIR").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let set = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    let length = set.directional_filter_len();
    let frequencies = [250., 1000., 3000., 6000.];
    let basis: Vec<Vec<(f64, f64)>> = frequencies
        .iter()
        .map(|hz| {
            (0..length)
                .map(|i| {
                    let p = std::f64::consts::TAU * hz * i as f64 / f64::from(RATE);
                    (p.cos(), p.sin())
                })
                .collect()
        })
        .collect();
    let mut csv =
        std::io::BufWriter::new(std::fs::File::create(out.join("rear-vertical-grid.csv")).unwrap());
    writeln!(csv,"layout,mode,azimuth,elevation,energy,adjacentDb,relativeIrStep,ild250,ild1000,ild3000,ild6000").unwrap();
    let mut reports = Vec::new();
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        let solver = vbap::VbapSolver::with_layout(layout);
        for (mode, width, height, diffuse) in [
            ("point", 0., 0., 0.),
            ("wide", 30., 0., 0.),
            ("tall", 0., 30., 0.),
            ("diffuse", 0., 0., 0.35),
            ("extent-diffuse", 30., 30., 0.35),
        ] {
            let mut max_step = 0.0f64;
            let mut max_db = 0.0f64;
            let mut worst = None;
            let mut minimum_energy = f64::INFINITY;
            let mut maximum_energy = 0.0f64;
            let mut columns = Vec::new();
            for az in 90..=270 {
                let mut previous: Option<((Vec<f32>, Vec<f32>), f64)> = None;
                let mut column_step = 0.0f64;
                let mut column_db = 0.0f64;
                for elevation in (-90..=90).rev() {
                    let p = position(az as f64, elevation as f64);
                    let pair = set
                        .directional_dry_compact(
                            directional::Direction { perceptual_width: false,
                                position: p,
                                head: None,
                                width,
                                height,
                                depth: 0.,
                                diffuse,
                                horizontal_only: false,
                            },
                            layout,
                            solver.pan(p, 0.),
                            [0.; vbap::MAX_BUS_COUNT],
                        )
                        .unwrap();
                    assert!(
                        pair.0.iter().chain(&pair.1).all(|x| x.is_finite()),
                        "nonfinite rear IR {mode} {az}/{elevation}"
                    );
                    let energy = pair
                        .0
                        .iter()
                        .chain(&pair.1)
                        .map(|x| f64::from(*x).powi(2))
                        .sum::<f64>();
                    assert!(
                        energy > 1e-12,
                        "rear direction lost output {mode} {az}/{elevation}"
                    );
                    minimum_energy = minimum_energy.min(energy);
                    maximum_energy = maximum_energy.max(energy);
                    let (db, step) = if let Some((old, old_energy)) = previous.as_ref() {
                        let delta = old
                            .0
                            .iter()
                            .chain(&old.1)
                            .zip(pair.0.iter().chain(&pair.1))
                            .map(|(a, b)| f64::from(a - b).powi(2))
                            .sum::<f64>();
                        (
                            10. * (energy / old_energy).log10(),
                            (delta / ((energy + old_energy) * 0.5)).sqrt(),
                        )
                    } else {
                        (0., 0.)
                    };
                    let ild: Vec<_> = basis
                        .iter()
                        .map(|w| {
                            let magnitude = |ear: &[f32]| {
                                let (re, im) =
                                    ear.iter().zip(w).fold((0., 0.), |(re, im), (x, (c, s))| {
                                        (re + f64::from(*x) * c, im + f64::from(*x) * s)
                                    });
                                re.hypot(im)
                            };
                            20. * (magnitude(&pair.0).max(1e-20) / magnitude(&pair.1).max(1e-20))
                                .log10()
                        })
                        .collect();
                    writeln!(
                        csv,
                        "{},{mode},{az},{elevation},{energy},{db},{step},{},{},{},{}",
                        layout.as_str(),
                        ild[0],
                        ild[1],
                        ild[2],
                        ild[3]
                    )
                    .unwrap();
                    if step > max_step {
                        max_step = step;
                        worst = Some(
                            serde_json::json!({"azimuth":az,"elevation":elevation,"relativeIrStep":step,"energyDeltaDb":db}),
                        );
                    }
                    max_db = max_db.max(db.abs());
                    column_step = column_step.max(step);
                    column_db = column_db.max(db.abs());
                    previous = Some((pair, energy));
                }
                columns.push(serde_json::json!({"azimuth":az,"maxAdjacentDb":column_db,"maxRelativeIrStep":column_step}));
                if az % 30 == 0 {
                    eprintln!(
                        "vertical rear grid {} {mode}: completed azimuth {az}",
                        layout.as_str()
                    );
                }
            }
            reports.push(serde_json::json!({"layout":layout.as_str(),"mode":mode,"positions":181*181,"azimuthStepDegrees":1,"elevationStepDegrees":1,"minimumEnergy":minimum_energy,"maximumEnergy":maximum_energy,"maximumAdjacentEnergyDb":max_db,"maximumRelativeIrStep":max_step,"worst":worst,"columns":columns}));
            csv.flush().unwrap();
            std::fs::write(
                out.join("rear-vertical-grid.json"),
                serde_json::to_vec_pretty(&reports).unwrap(),
            )
            .unwrap();
        }
    }
}


#[test]
fn sony_lower_beds_keep_negative_height_and_separate_rendered_signals() {
    let layout = vbap::LayoutId::Sony360Ra13;
    for (label, azimuth, elevation) in [
        ("LowerFrontLeft", 30., -20.),
        ("LowerFrontRight", -30., -20.),
        ("LowerCenter", 0., -20.),
        ("I_L_L045", 45., -15.),
        ("I_L_R045", -45., -15.),
        ("I_L_000", 0., -15.),
    ] {
        let mut output = Vec::new();
        for balance in [false, true] {
            let mut e = Engine::new(RATE, 2);
            e.set_layout(layout).unwrap();
            e.replace_hrtf(
                hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap(),
                0.,
            )
            .unwrap();
            e.set_spatial_enhancement(balance).unwrap();
            e.program_enabled = false;
            e.set_program_codec("mpegh".into());
            e.set_output_volume(1., true);
            e.set_master_preamp_db(0.).unwrap();
            e.output_active = true;
            e.paused = false;
            let mut s = source(position(azimuth, elevation));
            s.kind = SourceKind::Bed;
            s.bed_label = Some(label.into());
            e.sources.insert("lower".into(), s);
            e.route_source_now("lower", 0).unwrap();
            let route = e.sources["lower"].bus_gains;
            let lower_power: f32 = vbap::speakers(layout)
                .iter()
                .zip(route)
                .filter(|(s, _)| s.elevation < 0.)
                .map(|(_, g)| g * g)
                .sum();
            assert!(lower_power > 0.05, "lower bed flattened: {label} {route:?}");
            assert_eq!(e.sources["lower"].lfe_target, 0.);
            let expected = e.vbap.pan(position(azimuth, elevation), 0.);
            assert!(
                route
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| (*a - b).abs() < 1e-5),
                "bed geometry changed: {label}"
            );
            let frames = RATE as usize / 2;
            let pcm: Vec<_> = (0..frames)
                .map(|i| {
                    [250., 1000., 6000.]
                        .iter()
                        .map(|hz| {
                            (std::f64::consts::TAU * hz * i as f64 / f64::from(RATE)).sin() as f32
                                * 0.001
                        })
                        .sum()
                })
                .collect();
            e.sources
                .get_mut("lower")
                .unwrap()
                .samples
                .write(0, 0, &pcm);
            let mut rendered = vec![0.; frames * 2];
            e.render_into(&mut rendered, 2);
            assert!(rendered.iter().all(|v| v.is_finite()));
            assert!(
                rendered.iter().any(|v| v.abs() > 1e-6),
                "lower bed silent: {label} balance={balance}"
            );
            output.push(rendered);
            let below = e
                .active_hrtf_set
                .as_ref()
                .unwrap()
                .nearest(azimuth, elevation)
                .unwrap();
            let horizontal = e
                .active_hrtf_set
                .as_ref()
                .unwrap()
                .nearest(azimuth, 0.)
                .unwrap();
            let n = below.dry.len().min(horizontal.dry.len());
            let delta = below.dry[..n]
                .iter()
                .zip(&horizontal.dry[..n])
                .map(|(a, b)| f64::from(a - b).powi(2))
                .sum::<f64>();
            assert!(delta > 1e-6, "bottom HRTF equals horizontal: {label}");
        }
        let energy = output[0].iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        let controlled = output[1].iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        let db = 10. * (controlled / energy).log10();
        assert!(
            db >= -0.5,
            "spatial balance thinned lower bed {label}: {db}dB"
        );
    }
}


#[test]
#[ignore = "offline full native vertical PCM coverage at every rear azimuth"]
fn export_rear_vertical_pcm_grid() {
    use std::io::Write;
    let out = std::path::PathBuf::from(std::env::var_os("SDA_REAR_AUDIT_DIR").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let frames = RATE as usize * 3;
    let frequencies = [250., 1000., 3000., 6000.];
    let input: Vec<f32> = (0..frames)
        .map(|i| {
            frequencies
                .iter()
                .map(|hz| {
                    (std::f64::consts::TAU * hz * i as f64 / f64::from(RATE)).sin() as f32 * 0.0005
                })
                .sum()
        })
        .collect();
    let front: Vec<f32> = (0..frames)
        .map(|i| {
            [430., 1310., 4010.]
                .iter()
                .map(|hz| {
                    (std::f64::consts::TAU * hz * i as f64 / f64::from(RATE)).sin() as f32 * 0.008
                })
                .sum()
        })
        .collect();
    let weights: Vec<Vec<(f64, f64)>> = frequencies
        .iter()
        .map(|hz| {
            (0..4800)
                .map(|i| {
                    let phase = std::f64::consts::TAU * hz * i as f64 / f64::from(RATE);
                    let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 4799.).cos();
                    (w * phase.cos(), w * phase.sin())
                })
                .collect()
        })
        .collect();
    let amplitudes = |pcm: &[f32], start: usize| -> Vec<[f64; 2]> {
        weights
            .iter()
            .map(|w| {
                std::array::from_fn(|ear| {
                    let (re, im) = pcm[start * 2..(start + 4800) * 2]
                        .chunks_exact(2)
                        .zip(w)
                        .fold((0., 0.), |(re, im), (p, (c, s))| {
                            (re + f64::from(p[ear]) * c, im + f64::from(p[ear]) * s)
                        });
                    re.hypot(im)
                })
            })
            .collect()
    };
    let mut all = Vec::new();
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        let template = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
        let mut worst_mid = 0.0f64;
        let mut worst_ild = 0.0f64;
        let mut records = Vec::new();
        for azimuth in 90..=270 {
            let mut results = Vec::new();
            let mut maximum_lag = 0.0f64;
            for balance in [false, true] {
                let set = std::sync::Arc::try_unwrap(template.preparation_snapshot())
                    .ok()
                    .expect("exclusive diagnostic snapshot");
                let mut e = Engine::new(RATE, 2);
                e.set_layout(layout).unwrap();
                e.replace_hrtf(set, 0.).unwrap();
                e.set_spatial_enhancement(balance).unwrap();
                e.program_enabled = false;
                e.set_program_codec(
                    if layout == vbap::LayoutId::Sony360Ra13 {
                        "mpegh"
                    } else {
                        "eac3"
                    }
                    .into(),
                );
                e.set_direct_objects(true).unwrap();
                e.set_directional_hrtf(true);
                e.set_output_volume(1., true);
                e.set_master_preamp_db(0.).unwrap();
                e.source_occlusion_enabled = false;
                e.output_active = true;
                e.paused = false;
                e.direct_mix = 1.;
                e.sources
                    .insert("rear".into(), source(position(azimuth as f64, 85.)));
                e.sources.insert("front".into(), source([0., 1., 0.]));
                for id in ["rear", "front"] {
                    e.route_source_now(id, 0).unwrap();
                }
                let mut pcm = vec![0.; frames * 2];
                for start in (0..frames).step_by(convolution::DEFAULT_PARTITION) {
                    let t = start as f64 / f64::from(RATE);
                    let elevation = 85. - 170. * t / 3.;
                    let p = position(azimuth as f64, elevation);
                    e.sources.get_mut("rear").unwrap().position = p;
                    e.route_source_now("rear", 0).unwrap();
                    let count = (frames - start).min(convolution::DEFAULT_PARTITION);
                    e.sources.get_mut("rear").unwrap().samples.write(
                        start as u64,
                        start as u64,
                        &input[start..start + count],
                    );
                    e.sources.get_mut("front").unwrap().samples.write(
                        start as u64,
                        start as u64,
                        &front[start..start + count],
                    );
                    e.render_into(&mut pcm[start * 2..(start + count) * 2], 2);
                    let actual = e.sources["rear"]
                        .continuous
                        .as_ref()
                        .unwrap()
                        .effective_route()
                        .unwrap()
                        .0
                        .position;
                    let cross = [
                        p[1] * actual[2] - p[2] * actual[1],
                        p[2] * actual[0] - p[0] * actual[2],
                        p[0] * actual[1] - p[1] * actual[0],
                    ];
                    let angle = f64::from(cross.iter().map(|v| v * v).sum::<f32>().sqrt())
                        .atan2(f64::from(
                            p.iter().zip(actual).map(|(a, b)| a * b).sum::<f32>(),
                        ))
                        .to_degrees();
                    maximum_lag = maximum_lag.max(angle);
                    assert_eq!(
                        e.peak_guard.diagnostic_gain(),
                        1.,
                        "peak guard confounds vertical audit"
                    );
                    assert_eq!(e.sources["rear"].distance_gain, 1.);
                }
                assert!(pcm.iter().all(|v| v.is_finite()));
                assert!(pcm.iter().any(|v| v.abs() > 1e-6));
                results.push(pcm);
            }
            let mut min_db = [f64::INFINITY; 4];
            let mut max_db = [f64::NEG_INFINITY; 4];
            let mut max_ild = [0.0f64; 4];
            for window in 5..29 {
                let a = amplitudes(&results[0], window * 4800);
                let b = amplitudes(&results[1], window * 4800);
                for band in 0..4 {
                    let power = |a: [f64; 2]| a[0] * a[0] + a[1] * a[1];
                    let db = 10. * (power(b[band]) / power(a[band]).max(1e-30)).log10();
                    min_db[band] = min_db[band].min(db);
                    max_db[band] = max_db[band].max(db);
                    let ild = |a: [f64; 2]| 20. * (a[0].max(1e-20) / a[1].max(1e-20)).log10();
                    max_ild[band] = max_ild[band].max((ild(a[band]) - ild(b[band])).abs());
                }
            }
            if min_db[1] < worst_mid {
                worst_mid = min_db[1];
                for (i, name) in ["off", "on"].iter().enumerate() {
                    let mut f = std::io::BufWriter::new(
                        std::fs::File::create(
                            out.join(format!("vertical-worst-{}-{name}.f32", layout.as_str())),
                        )
                        .unwrap(),
                    );
                    for x in &results[i] {
                        f.write_all(&x.to_le_bytes()).unwrap();
                    }
                }
            }
            worst_ild = worst_ild.max(max_ild.into_iter().fold(0.0, f64::max));
            records.push(serde_json::json!({"azimuth":azimuth,"minimumBalanceDb":min_db,"maximumBalanceDb":max_db,"maximumIldDifferenceDb":max_ild,"maxRouteLagDegrees":maximum_lag}));
            if azimuth % 30 == 0 {
                eprintln!(
                    "native vertical PCM {} az={azimuth} worst 1kHz change={worst_mid:.5}dB",
                    layout.as_str()
                );
            }
        }
        all.push(serde_json::json!({"layout":layout.as_str(),"horizontalColumns":181,"azimuthStepDegrees":1,"startElevation":85,"endElevation":-85,"durationSeconds":3,"spatialBalanceStates":2,"minimum1kHzBalanceDb":worst_mid,"maximumIldDifferenceDb":worst_ild,"columns":records}));
        std::fs::write(
            out.join("rear-vertical-pcm.json"),
            serde_json::to_vec_pretty(&all).unwrap(),
        )
        .unwrap();
    }
}
