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
