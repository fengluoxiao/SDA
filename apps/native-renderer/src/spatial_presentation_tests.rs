use super::*;

fn configured(enhanced: bool, objects: usize) -> Engine {
    let mut e = Engine::new(48000, 2);
    let set = hrtf::NativeHrtfSet::synthetic(48, 256, 1024).unwrap();
    e.replace_hrtf(set, 0.15).unwrap();
    e.set_directional_hrtf(true);
    e.set_spatial_enhancement(enhanced).unwrap();
    for id in 0..objects {
        let angle = (id as f32 * 137.5).to_radians();
        let mut source = Source {
            kind: SourceKind::Object,
            object_id: Some(id as u32),
            position: [angle.sin(), -angle.cos().abs(), 0.3],
            extent: [0.3, 0.2, 0.1],
            spread: 0.2,
            gain: 1.0,
            target_gain: 1.0,
            availability: 1.0,
            availability_target: 1.0,
            ..Source::default()
        };
        let pcm: Vec<_> = (0..16000)
            .map(|n| ((n + id * 7) as f32 * 0.13).sin() * 0.002)
            .collect();
        source.samples.write(0, 0, &pcm);
        let key = format!("obj:{id}");
        e.sources.insert(key.clone(), source);
        e.route_source_now(&key, 0).unwrap();
    }
    e.paused = false;
    e.output_active = true;
    e
}

#[test]
fn enhanced_width_preserves_independent_dimensions_and_point_sources() {
    let settings = source_extent::Settings::default();
    let w = source_extent::presentation_angles([0.5, 0.7, 0.1], 0.8, false, settings, true);
    assert!((w[0] - 39.0).abs() < 1e-5 && (w[1] - 4.44).abs() < 1e-5 && w[2] == 0.7);
    assert_eq!(
        source_extent::presentation_angles([0.; 3], 0., false, settings, true),
        [0.; 3]
    );
    assert_eq!(
        source_extent::presentation_angles([0.5, 0.7, 0.1], 0.8, false, settings, false),
        [0.8 * 0.8 * 120., 0.8 * 0.8 * 120., 0.]
    );
    assert_eq!(
        source_extent::presentation_angles([0.5, 0.7, 0.1], 0.8, true, settings, true)[1],
        0.
    );
}

#[test]
fn actual_enhanced_pcm_is_finite_and_independent_of_callback_size() {
    let mut a = configured(true, 3);
    let mut b = configured(true, 3);
    let frames = 12000;
    let mut whole = vec![0.; frames * 2];
    a.render_into(&mut whole, 2);
    let mut split = Vec::new();
    while split.len() < whole.len() {
        let n = (whole.len() - split.len()).min(274);
        let mut out = vec![0.; n];
        b.render_into(&mut out, 2);
        split.extend(out);
    }
    let peak = whole.iter().copied().map(f32::abs).fold(0., f32::max);
    assert!(peak > 1e-6 && peak <= 1.0, "peak={peak}");
    let error = whole
        .iter()
        .zip(&split)
        .map(|(a, b)| (a - b).abs())
        .fold(0., f32::max);
    assert!(error < 1e-5, "callback-size difference {error}");
    let mut off = configured(false, 3);
    let mut original = vec![0.; frames * 2];
    off.render_into(&mut original, 2);
    let energy = original.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>();
    let difference = whole
        .iter()
        .zip(&original)
        .map(|(a, b)| f64::from(a - b).powi(2))
        .sum::<f64>();
    assert!(
        difference / energy > 1e-4,
        "enhancement must change PCM: relative difference={} peak={peak}",
        (difference / energy).sqrt()
    );
    eprintln!(
        "enhanced PCM relative difference={:.5} peak={peak:.6}",
        (difference / energy).sqrt()
    );
    a.reset_session(9000);
    assert!(a.sources.is_empty());
    let mut silence = vec![0.; 16000];
    a.render_into(&mut silence, 2);
    assert!(
        silence.iter().all(|v| v.abs() < 1e-7),
        "seek must clear delayed sound"
    );
}

#[test]
fn late_send_excludes_front_leads_lfe_and_muted_sources() {
    for (position, label, muted, expected) in [
        ([0., 1., 0.], None, false, false),
        ([0., -1., 0.], None, false, true),
        ([0., -1., 0.], None, true, false),
        ([0., -1., 0.], Some("LFE"), false, false),
    ] {
        let mut e = configured(true, 1);
        let s = e.sources.get_mut("obj:0").unwrap();
        s.position = position;
        s.muted = muted;
        if let Some(label) = label {
            s.kind = SourceKind::Bed;
            s.bed_label = Some(label.into());
        }
        e.route_source_now("obj:0", 0).unwrap();
        let mut output = vec![0.; 8192];
        e.render_into(&mut output, 2);
        let s = &e.sources["obj:0"];
        if label.is_some() {
            assert_eq!(s.auxiliary_weight, 0.0);
        }
        let route_energy: f32 = s.envelopment_route.iter().map(|x| x * x).sum();
        assert_eq!(
            route_energy > 0.0,
            expected,
            "position={position:?} label={label:?} muted={muted}"
        );
    }
}

#[test]
fn presentation_bypasses_authored_hardware_and_non_binaural_output() {
    let mut e = configured(true, 0);
    assert!(e.spatial_presentation_active());
    e.binaural_enabled = false;
    assert!(!e.spatial_presentation_active());
    e.binaural_enabled = true;
    e.authored.enabled = true;
    assert!(!e.spatial_presentation_active());
    e.authored.enabled = false;
    e.cinema.monitor.hardware.enabled = true;
    assert!(!e.spatial_presentation_active());
}

#[test]
fn enhanced_object_fast_path_matches_general_path() {
    let mut fast = configured(true, 12);
    let mut general = configured(true, 12);
    general.disable_fast_objects = true;
    let (mut a, mut b) = (vec![0.; 32000], vec![0.; 32000]);
    for (left, right) in a.chunks_mut(512).zip(b.chunks_mut(512)) {
        fast.render_into(left, 2);
        general.render_into(right, 2);
    }
    assert!(
        fast.sources.values().any(|s| s.fast_mixed),
        "fixture must exercise the source-major path"
    );
    let error = a
        .iter()
        .zip(&b)
        .map(|(a, b)| (a - b).abs())
        .fold(0., f32::max);
    assert!(error < 1e-6, "fast/general presentation diverged: {error}");
}

#[test]
fn timestamped_tracking_reaches_engine_and_reset_has_no_prediction_kick() {
    let mut e = configured(true, 0);
    let now = Instant::now();
    let q = |d: f32| {
        let a = d.to_radians() / 2.;
        [0., 0., a.sin(), a.cos()]
    };
    assert!(e.accept_head_pose(q(0.), now - Duration::from_millis(40), 0.03, 256));
    assert!(e.accept_head_pose(q(0.2), now - Duration::from_millis(7), 0.03, 256));
    assert!(e.pose_predictor.lead_degrees > 0.1 && e.pose_predictor.lead_degrees <= 6.);
    assert!(!e.accept_head_pose(q(90.), now - Duration::from_secs(1), 0.03, 256));
    e.pose_predictor.reset();
    assert!(e.accept_head_pose(q(-10.), Instant::now(), 0.03, 256));
    assert_eq!(e.pose_predictor.lead_degrees, 0.);
}

#[test]
#[ignore = "offline performance measurement"]
fn benchmark_enhanced_pipeline() {
    for enabled in [false, true] {
        let mut e = configured(enabled, 108);
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap(), 0.)
            .unwrap();
        e.set_spatial_enhancement(enabled).unwrap();
        let mut times = Vec::new();
        let mut checksum = 0.;
        for block in 0..220 {
            let start = e.sample_pos;
            for (id, s) in e.sources.values_mut().enumerate() {
                let pcm: Vec<_> = (0..convolution::DEFAULT_PARTITION)
                    .map(|n| ((start as usize + n + id) as f32 * 0.13).sin() * 0.0002)
                    .collect();
                s.samples.write(start, start, &pcm);
            }
            let mut output = [0.; convolution::DEFAULT_PARTITION * 2];
            let t = Instant::now();
            e.render_into(&mut output, 2);
            if block > 60 {
                times.push(t.elapsed().as_secs_f64() * 1e6);
            }
            checksum += output.iter().map(|x| f64::from(*x)).sum::<f64>();
        }
        times.sort_by(f64::total_cmp);
        eprintln!("enhanced={enabled} sources=108 mean_us={:.1} p95_us={:.1} max_us={:.1} budget_us={:.1} checksum={checksum:.9}",
            times.iter().sum::<f64>()/times.len()as f64,times[times.len()*95/100],times[times.len()-1],convolution::DEFAULT_PARTITION as f64/48000.*1e6);
    }
}
