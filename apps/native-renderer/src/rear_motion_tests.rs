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
                let direction = directional::Direction {
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
                let direction = directional::Direction {
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
                        directional::Direction {
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
                            directional::Direction {
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
