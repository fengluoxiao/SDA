use super::*;
struct Result {
    pcm: Vec<f32>,
    extra: f64,
    clock: u64,
    fast_blocks: u64,
}
fn scene(fast: bool, volume: bool, spatial: bool, detail: bool, chunk: usize) -> Result {
    let mut e = Engine::new(48000, 2);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(), 0.)
        .unwrap();
    e.set_spatial_enhancement(spatial).unwrap();
    e.auxiliary_balance.set_temporal_for_diagnostics(detail);
    e.set_direct_objects(true).unwrap();
    e.set_directional_hrtf(true);
    e.set_program_codec("mpegh".into());
    e.program_enabled = volume;
    e.set_program_target(0.25, true);
    e.set_master_preamp_db(-12.).unwrap();
    e.set_output_volume(0.5, true);
    e.direct_mix = 1.;
    e.disable_fast_objects = !fast;
    let mut seed = 13u64;
    let mut envelopes = vec![];
    let mut level = 0.;
    for i in 0..500 {
        if i % 4 == 0 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            level = 0.03 + 0.25 * ((seed >> 32) as u32 as f32 / u32::MAX as f32);
        }
        envelopes.push(level);
    }
    for i in 0..8 {
        let id = i.to_string();
        let position = if i == 0 { [0., 1., 0.] } else { [0., -1., 0.] };
        let source = Source {
            kind: SourceKind::Object,
            object_id: Some(i as u32),
            position,
            gain: 1.,
            target_gain: 1.,
            availability: 1.,
            availability_target: 1.,
            ..Default::default()
        };
        e.sources.insert(id.clone(), source);
        e.route_source_now(&id, 0).unwrap();
    }
    e.output_active = true;
    e.paused = false;
    let mut start = 0;
    let total = 48000 * 5;
    let mut pcm = vec![];
    while start < total {
        let n = chunk.min(total - start);
        for i in 0..8 {
            let input: Vec<f32> = (start..start + n)
                .map(|at| {
                    let t = at / 480;
                    if i == 0 {
                        envelopes[t]
                            * ((at as f32 * std::f32::consts::TAU * 1000. / 48000.).sin()
                                + 0.25 * (at as f32 * std::f32::consts::TAU * 100. / 48000.).sin())
                    } else if i == 1 && t >= 25 {
                        envelopes[t - 25]
                            * 0.15
                            * (at as f32 * std::f32::consts::TAU * 3500. / 48000.).sin()
                    } else {
                        0.
                    }
                })
                .collect();
            e.sources.get_mut(&i.to_string()).unwrap().samples.write(
                start as u64,
                start as u64,
                &input,
            );
        }
        let mut output = vec![0.; n * 2];
        e.render_into(&mut output, 2);
        pcm.extend(output);
        start += n;
    }
    assert!(pcm.iter().all(|x| x.is_finite() && x.abs() < 0.871));
    Result {
        pcm,
        extra: e.auxiliary_balance.temporal_levels()[1],
        clock: e.sample_pos,
        fast_blocks: e.fast_object_blocks as u64,
    }
}
fn amplitude(pcm: &[f32], hz: f64) -> f64 {
    let start = pcm.len() / 2 - 48000;
    let mut energy = 0.;
    for ear in 0..2 {
        let (mut re, mut im) = (0., 0.);
        for i in start..pcm.len() / 2 {
            let p = i as f64 * std::f64::consts::TAU * hz / 48000.;
            re += f64::from(pcm[i * 2 + ear]) * p.cos();
            im += f64::from(pcm[i * 2 + ear]) * p.sin();
        }
        energy += re * re + im * im;
    }
    energy.sqrt()
}
#[test]
fn delayed_spatial_detail_is_audible_without_changing_foreground_or_bass() {
    for volume in [false, true] {
        let off = scene(true, volume, true, false, 997);
        let on = scene(true, volume, true, true, 997);
        assert!(
            on.extra > 0.1,
            "no real detail enhancement was applied: {}",
            on.extra
        );
        let echo = 20. * (amplitude(&on.pcm, 3500.) / amplitude(&off.pcm, 3500.)).log10();
        for hz in [100., 1000.] {
            let difference = 20. * (amplitude(&on.pcm, hz) / amplitude(&off.pcm, hz)).log10();
            assert!(
                difference.abs() < 0.1,
                "foreground changed at {hz} Hz: {difference}"
            );
        }
        assert!(
            echo > 0.1,
            "delayed component did not gain audibility: {echo}"
        );
        assert!(on.extra <= 3.011);
    }
}
#[test]
fn ordinary_and_parallel_mixers_preserve_active_temporal_control_and_callback_clock() {
    let a = scene(true, true, true, true, 173);
    let b = scene(false, true, true, true, 997);
    assert_eq!(a.clock, 240000);
    assert_eq!(a.clock, b.clock);
    assert!(a.fast_blocks > 0);
    assert_eq!(b.fast_blocks, 0);
    let error = a
        .pcm
        .iter()
        .zip(&b.pcm)
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(error < 1e-6, "mixer or callback mismatch {error}");
    assert!((a.extra - b.extra).abs() < 0.001);
}
#[test]
fn spatial_off_is_exact_bypass_for_the_added_detail_layer() {
    let a = scene(true, true, false, false, 997);
    let b = scene(true, true, false, true, 997);
    assert_eq!(a.pcm, b.pcm);
    assert_eq!(b.extra, 0.);
}
