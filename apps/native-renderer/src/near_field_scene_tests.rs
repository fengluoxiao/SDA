//! Combined regression for emitter overlap, physical proximity and output budget.
//! Uses the production native engine/filter/controller; no decoder or song cache.
use super::*;

fn engine(layout: vbap::LayoutId, fast: bool, scale: f32) -> Engine {
    let mut e = Engine::new(48000, 2);
    e.set_layout(layout).unwrap();
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
    e.replace_hrtf(
        hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap(),
        0.04,
    )
    .unwrap();
    e.set_direct_objects(true).unwrap();
    e.set_directional_hrtf(true);
    e.configure_near_field(near_field::Settings {
        enabled: true,
        metres_per_unit: scale,
    })
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
    e.set_output_volume(1.0, true);
    e.set_master_preamp_db(0.0).unwrap();
    e.disable_fast_objects = !fast;
    e.direct_mix = 1.0;
    e.output_active = true;
    e.paused = false;
    e
}

fn render(layout: vbap::LayoutId, fast: bool, scale: f32, chunk: usize) -> (Vec<f32>, Engine) {
    let mut e = engine(layout, fast, scale);
    let frames = 16384;
    // Same-bearing close/far objects, rear/top/bottom, plus two front objects.
    // All distances here are explicit physical metres and must beat manual scale.
    for (id, (p, distance)) in [
        ([0.8, 0.6, 0.0], 0.12),
        ([0.4, 0.3, 0.0], 0.8),
        ([-0.5, -0.5, 0.70710677], 0.3),
        ([0.0, 0.0, 1.0], 0.25),
        ([0.0, 0.0, -1.0], 0.25),
        ([0.0, 1.0, 0.0], 1.5),
    ]
    .into_iter()
    .enumerate()
    {
        let mut source = Source {
            kind: SourceKind::Object,
            object_id: Some(id as u32),
            position: p,
            distance_m: Some(distance),
            gain: 1.0,
            target_gain: 1.0,
            availability: 1.0,
            availability_target: 1.0,
            ..Default::default()
        };
        let samples: Vec<_> = (0..frames)
            .map(|i| {
                let t = i as f32 / 48000.0;
                0.001
                    * ((std::f32::consts::TAU * (173.0 + id as f32 * 59.0) * t).sin()
                        + 0.3 * (std::f32::consts::TAU * (3001.0 + id as f32 * 71.0) * t).sin())
            })
            .collect();
        source.samples.write(0, 0, &samples);
        let key = format!("obj:{id}");
        e.sources.insert(key.clone(), source);
        e.route_source_now(&key, 0).unwrap();
    }
    let labels = if layout == vbap::LayoutId::Sony360Ra13 {
        ["LowerFrontLeft", "LowerFrontRight", "LowerCenter", "LFE"]
    } else {
        ["TopFrontLeft", "TopFrontRight", "RearLeft", "LFE"]
    };
    for (i, label) in labels.into_iter().enumerate() {
        let mut s = Source {
            kind: SourceKind::Bed,
            bed_label: Some(label.into()),
            // Even this tiny authored radius must not turn a fixed bed into
            // a close object or depend on the manual metre scale.
            position: [0.01, 0.01, -0.01],
            gain: 1.0,
            target_gain: 1.0,
            availability: 1.0,
            availability_target: 1.0,
            ..Default::default()
        };
        let samples: Vec<_> = (0..frames)
            .map(|n| {
                0.0005
                    * (std::f32::consts::TAU * (83.0 + i as f32 * 127.0) * n as f32 / 48000.0).sin()
            })
            .collect();
        s.samples.write(0, 0, &samples);
        let key = format!("bed:{i}");
        e.sources.insert(key.clone(), s);
        e.route_source_now(&key, 0).unwrap();
    }
    let mut pcm = vec![0.0; frames * 2];
    for block in pcm.chunks_mut(chunk * 2) {
        e.render_into(block, 2);
    }
    (pcm, e)
}

#[test]
fn physical_distance_wins_in_both_mixers_and_both_immersive_layouts() {
    let mut comparisons = 0;
    for layout in [vbap::LayoutId::Dolby7_1_4, vbap::LayoutId::Sony360Ra13] {
        let (reference, baseline) = render(layout, true, 1.0, 1024);
        assert!(reference.iter().any(|v| v.abs() > 1e-5));
        assert!(
            baseline
                .sources
                .values()
                .all(|s| s.occlusion_targets == [1.0; 2])
        );
        for (fast, scale, chunk) in [
            (true, 0.25, 173),
            (true, 4.0, 257),
            (false, 0.25, 173),
            (false, 4.0, 257),
        ] {
            let (actual, state) = render(layout, fast, scale, chunk);
            let delta = actual
                .iter()
                .zip(&reference)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            assert!(
                delta < 2e-6,
                "{layout:?} fast={fast} scale={scale} delta={delta}"
            );
            assert_eq!(state.sources.len(), 10);
            assert!(
                state
                    .sources
                    .values()
                    .filter(|s| s.kind == SourceKind::Bed)
                    .all(|s| s.near_target == [1.0; 2])
            );
            assert_eq!(state.output_dynamics.gain_db(), 0.0);
            assert_eq!(state.peak_guard.diagnostic_gain(), 1.0);
            assert!(actual.iter().all(|x| x.is_finite()));
            comparisons += 1;
        }
    }
    println!(
        "NEAR_NATIVE_SCENE comparisons={comparisons} layouts=2 objects=6 beds=4 physicalScaleInvariant=true bottomBedsAndLfePreserved=true"
    );
}

#[test]
fn spherical_grid_preserves_mirrors_front_back_and_unit_pair_energy() {
    let s = near_field::Settings {
        enabled: true,
        ..Default::default()
    };
    let mut checked = 0;
    for r in [0.12, 0.2, 0.4, 0.8, 0.999, 1.0] {
        for elevation in (-90..=90).step_by(5) {
            let el = (elevation as f32).to_radians();
            for azimuth in (-180..=180).step_by(5) {
                let az = (azimuth as f32).to_radians();
                let p = [
                    r * el.cos() * az.sin(),
                    r * el.cos() * az.cos(),
                    r * el.sin(),
                ];
                let a = near_field::gains(p, None, s);
                let mirror = near_field::gains([-p[0], p[1], p[2]], None, s);
                assert_eq!(a, [mirror[1], mirror[0]]);
                assert_eq!(a, near_field::gains([p[0], -p[1], p[2]], None, s));
                assert!(a.iter().all(|v| v.is_finite() && *v > 0.0));
                assert!(((a[0] * a[0] + a[1] * a[1]) * 0.5 - 1.0).abs() < 2e-6);
                checked += 1;
            }
        }
    }
    println!(
        "NEAR_SPHERICAL_GRID positions={checked} mirror=true frontBackFactorPreserved=true pairEnergy=1"
    );
}

#[test]
fn emitter_overlap_never_creates_ordinary_music_obstacles_even_after_scene_reset() {
    let mut e = Engine::new(48000, 2);
    let block = convolution::DEFAULT_PARTITION;
    for i in 0..MAX_SOURCES {
        let mut s = Source {
            kind: SourceKind::Object,
            object_id: Some(i as u32),
            position: [0.0, if i % 2 == 0 { 1.0 } else { 0.15 }, 0.0],
            gain: 1.0,
            target_gain: 1.0,
            availability: 1.0,
            availability_target: 1.0,
            occlusion_targets: [0.6, 0.8],
            ..Default::default()
        };
        s.samples.write(0, 0, &vec![0.01; block]);
        e.sources.insert(format!("obj:{i}"), s);
    }
    assert!(!e.source_occlusion_enabled);
    for head in [None, Some([0.0, 0.0, 0.70710677, 0.70710677])] {
        e.update_occlusion(head);
        assert!(e.sources.values().all(|s| s.occlusion_targets == [1.0; 2]));
    }
    e.source_occlusion_enabled = true; // Loading a legacy authored effect is allowed.
    e.configure_authored_processing(Default::default()).unwrap();
    e.update_occlusion(None);
    assert!(!e.source_occlusion_enabled);
    assert!(e.sources.values().all(|s| s.occlusion_targets == [1.0; 2]));
}

#[test]
fn proximity_and_spatial_balance_share_real_output_headroom_without_common_ducking() {
    let rate = 48000;
    for balance in [false, true] {
        for position in [
            [-0.12, 0.0, 0.0],
            [0.0, -0.12, 0.0],
            [0.0, 0.0, 0.12],
            [0.0, 0.0, -0.12],
        ] {
            let mut e = engine(vbap::LayoutId::Dolby7_1_4, true, 1.0);
            e.program_enabled = balance;
            e.set_program_target(if balance { 0.6 } else { 1.0 }, true);
            let settings = near_field::Settings {
                enabled: true,
                ..Default::default()
            };
            let target = near_field::gains(position, None, settings);
            let mut near = near_field::Filter::default();
            for i in 0..rate * 5 {
                let t = i as f32 / rate as f32;
                let front = 0.02 * (std::f32::consts::TAU * 997.0 * t).sin();
                let auxiliary = 0.001 * (std::f32::consts::TAU * 173.0 * t).sin();
                let mut reference = auxiliary_balance::Reference::default();
                reference.groups[0][0] = f64::from(front);
                reference.groups[1][0] = f64::from(auxiliary);
                e.auxiliary_balance
                    .observe_at(reference, e.program_gain, i as u64);
                let block_index = i as usize % convolution::DEFAULT_PARTITION;
                let lifted = auxiliary * e.auxiliary_balance.gains[block_index];
                let corrected = near.process([lifted; 2], target);
                e.advance_output_envelopes();
                let out = e.process_final_output(corrected.map(|v| v + front));
                assert!(out.iter().all(|v| v.is_finite() && v.abs() < 0.1));
                if block_index + 1 == convolution::DEFAULT_PARTITION {
                    let room = if e.peak_guard.is_limiting() {
                        None
                    } else {
                        e.output_dynamics.available_headroom_db()
                    };
                    e.auxiliary_balance.set_output_headroom(room);
                    e.auxiliary_balance.finish_block();
                }
            }
            assert!(
                e.auxiliary_balance.gains[0] > 1.1,
                "lost measured room: {position:?}/{balance}"
            );
            assert_eq!(e.output_dynamics.gain_db(), 0.0);
            assert_eq!(e.peak_guard.diagnostic_gain(), 1.0);
            // The actual linked peak guard vetoes further lift after overload.
            for _ in 0..1024 {
                e.process_final_output([3.0, -2.0]);
            }
            assert!(e.peak_guard.is_limiting());
            e.render_into(&mut vec![0.0; convolution::DEFAULT_PARTITION * 2], 2);
            assert_eq!(e.auxiliary_balance.headroom_db(), None);
            e.reset_seek(0, rate as u64 * 12);
            assert!(e.auxiliary_balance.gains.iter().all(|g| *g == 1.0));
        }
    }
    println!(
        "NEAR_OUTPUT_BUDGET positions=4 volumeBalanceStates=2 liftPreserved=true peakVeto=true seekReset=true"
    );
}
