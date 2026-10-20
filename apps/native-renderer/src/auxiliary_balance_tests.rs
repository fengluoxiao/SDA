//! Audible-path regressions: source domain, actual HRTF/room, final programme gain.
use super::*;
struct Rendered {
    pcm: Vec<f32>,
    maximum_cut: f64,
    guard: f32,
}
fn render(
    objects: bool,
    volume_balance: bool,
    spatial: bool,
    reference: bool,
    fast: bool,
    front: f32,
    rear: f32,
    chunk: usize,
    head: Option<[f32; 4]>,
) -> Rendered {
    render_scheduled(
        objects,
        volume_balance,
        spatial,
        reference,
        fast,
        front,
        rear,
        chunk,
        head,
        &[],
    )
}
fn render_scheduled(
    objects: bool,
    volume_balance: bool,
    spatial: bool,
    reference: bool,
    fast: bool,
    front: f32,
    rear: f32,
    chunk: usize,
    head: Option<[f32; 4]>,
    schedule: &[(usize, bool, bool)],
) -> Rendered {
    let mut e = Engine::new(48000, 2);
    e.set_layout(vbap::LayoutId::Sony360Ra13).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(), 0.)
        .unwrap();
    e.set_spatial_enhancement(spatial).unwrap();
    if reference {
        e.auxiliary_balance = auxiliary_balance::AuxiliaryBalance::with_policy(
            48000,
            auxiliary_balance::Policy {
                maximum_cut_db: 0.,
                ..Default::default()
            },
        );
    }
    e.set_program_codec("mpegh".into());
    e.set_direct_objects(true).unwrap();
    e.set_directional_hrtf(true);
    e.set_output_volume(0.5, true);
    e.set_master_preamp_db(-12.).unwrap();
    e.program_enabled = volume_balance;
    e.set_program_target(0.25, true);
    e.direct_mix = 1.;
    e.disable_fast_objects = !fast;
    e.head_pose = head;
    for (index, (id, label, position)) in
        [("front", "C", [0., 1., 0.]), ("rear", "Lb", [0., -1., 0.])]
            .into_iter()
            .enumerate()
    {
        let mut s = Source {
            kind: if objects {
                SourceKind::Object
            } else {
                SourceKind::Bed
            },
            bed_label: (!objects).then(|| label.to_owned()),
            object_id: objects.then_some(index as u32),
            position,
            gain: 1.,
            target_gain: 1.,
            availability: 1.,
            availability_target: 1.,
            ..Default::default()
        };
        if !objects {
            Engine::set_source_route(&mut s, bed_route(label, &e.vbap), 0);
        }
        e.sources.insert(id.into(), s);
        e.route_source_now(id, 0).unwrap();
    }
    e.output_active = true;
    e.paused = false;
    let total = 48000 * if schedule.is_empty() { 4 } else { 6 };
    let mut next_event = 0;
    let mut pcm = Vec::with_capacity(total * 2);
    let mut start = 0;
    let mut guard = 1f32;
    while start < total {
        if let Some(&(at, volume, spatial)) = schedule.get(next_event) {
            if at == start {
                e.program_enabled = volume;
                e.set_program_target(0.25, false);
                e.set_spatial_enhancement(spatial).unwrap();
                next_event += 1;
            }
        }
        let n = chunk.min(total - start).min(
            schedule
                .get(next_event)
                .map_or(usize::MAX, |event| event.0 - start),
        );
        for (id, amplitude, hz) in [("front", front, 1000.), ("rear", rear, 3000.)] {
            let input: Vec<f32> = (start..start + n)
                .map(|i| (i as f64 * std::f64::consts::TAU * hz / 48000.).sin() as f32 * amplitude)
                .collect();
            e.sources
                .get_mut(id)
                .unwrap()
                .samples
                .write(start as u64, start as u64, &input);
        }
        let mut out = vec![0.; n * 2];
        e.render_into(&mut out, 2);
        pcm.extend_from_slice(&out);
        guard = guard.min(e.peak_guard.diagnostic_gain());
        start += n;
    }
    Rendered {
        pcm,
        maximum_cut: e.auxiliary_balance.maximum_attenuation_db,
        guard,
    }
}
fn amplitude(pcm: &[f32], hz: f64) -> f64 {
    let n = 48000;
    let start = pcm.len() / 2 - n;
    let mut power = 0.;
    for ear in 0..2 {
        let (mut re, mut im) = (0., 0.);
        for i in 0..n {
            let phase = (start + i) as f64 * std::f64::consts::TAU * hz / 48000.;
            let x = f64::from(pcm[(start + i) * 2 + ear]);
            re += x * phase.cos();
            im += x * phase.sin();
        }
        power += re * re + im * im;
    }
    power.sqrt() / n as f64
}
#[test]
fn auxiliary_control_protects_front_beds_and_objects_in_both_volume_states() {
    for objects in [false, true] {
        for volume in [false, true] {
            let a = render(objects, volume, true, true, true, 0.02, 0.1, 1024, None);
            let b = render(objects, volume, true, false, true, 0.02, 0.1, 1024, None);
            assert_eq!(a.guard, 1.);
            assert_eq!(b.guard, 1.);
            let front_db = 20. * (amplitude(&b.pcm, 1000.) / amplitude(&a.pcm, 1000.)).log10();
            let rear_db = 20. * (amplitude(&b.pcm, 3000.) / amplitude(&a.pcm, 3000.)).log10();
            eprintln!(
                "objects={objects}, volume={volume}, front={front_db:.4} dB, rear={rear_db:.4} dB, max={:.3}",
                b.maximum_cut
            );
            assert!(
                front_db.abs() < 0.1,
                "front was pulled down by rear: {front_db}"
            );
            assert!(rear_db < -2., "rear dominance was not reduced: {rear_db}");
            assert!(b.maximum_cut > 2. && b.maximum_cut <= 6.00001);
        }
    }
}
#[test]
fn both_spatial_states_apply_programme_gain_exactly_once() {
    for spatial in [false, true] {
        let off = render(true, false, spatial, false, true, 0.02, 0.1, 1024, None);
        let on = render(true, true, spatial, false, true, 0.02, 0.1, 1024, None);
        assert_eq!(off.guard, 1.);
        assert_eq!(on.guard, 1.);
        let last = off.pcm.len() - 96000;
        let error = off.pcm[last..]
            .iter()
            .zip(&on.pcm[last..])
            .map(|(a, b)| (a * 0.25 - b).abs())
            .fold(0f32, f32::max);
        assert!(
            error < 3e-7,
            "programme gain doubled/cancelled for spatial={spatial}: {error}"
        );
    }
}
#[test]
fn rear_only_and_front_only_scenes_retain_the_static_recipe() {
    for (front, rear) in [(0.02, 0.), (0., 0.1)] {
        let a = render(true, true, true, true, true, front, rear, 1024, None);
        let b = render(true, true, true, false, true, front, rear, 1024, None);
        assert_eq!(b.maximum_cut, 0.);
        assert_eq!(a.pcm, b.pcm);
    }
}
#[test]
fn callback_sizes_and_fast_legacy_paths_share_the_same_control() {
    let a = render(true, true, true, false, true, 0.02, 0.1, 1024, None);
    for (fast, chunk) in [(true, 137), (false, 1024), (false, 137)] {
        let b = render(true, true, true, false, fast, 0.02, 0.1, chunk, None);
        let error = a
            .pcm
            .iter()
            .zip(&b.pcm)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(
            error < 2e-6,
            "callback/path changed output: fast={fast}, chunk={chunk}, error={error}"
        );
        assert!((a.maximum_cut - b.maximum_cut).abs() < 1e-5);
    }
}
#[test]
fn head_pose_cannot_change_world_group_control() {
    let a = render(true, true, true, false, true, 0.02, 0.1, 1024, None);
    let angle = std::f32::consts::FRAC_PI_4 / 2.;
    let b = render(
        true,
        true,
        true,
        false,
        true,
        0.02,
        0.1,
        1024,
        Some([0., 0., angle.sin(), angle.cos()]),
    );
    assert!((a.maximum_cut - b.maximum_cut).abs() < 1e-5);
}

#[test]
fn live_four_state_switches_preserve_foreground_and_the_codec_clock() {
    let schedule = [
        (48000, false, true),
        (96000, false, false),
        (144000, true, false),
        (192000, true, true),
        (240000, false, true),
    ];
    let a = render_scheduled(
        true, true, true, true, true, 0.02, 0.1, 137, None, &schedule,
    );
    let b = render_scheduled(
        true, true, true, false, true, 0.02, 0.1, 137, None, &schedule,
    );
    assert_eq!(a.pcm.len(), 48000 * 6 * 2);
    assert_eq!(a.pcm.len(), b.pcm.len());
    assert_eq!(a.guard, 1.);
    assert_eq!(b.guard, 1.);
    for second in 0..6 {
        let range = (second * 48000 + 24000) * 2..(second * 48000 + 48000) * 2;
        let power = |pcm: &[f32]| {
            let (mut re, mut im) = (0., 0.);
            for (i, frame) in pcm[range.clone()].chunks_exact(2).enumerate() {
                let phase = i as f64 * std::f64::consts::TAU * 1000. / 48000.;
                re += f64::from(frame[0]) * phase.cos();
                im += f64::from(frame[0]) * phase.sin();
            }
            re * re + im * im
        };
        let db = 10. * (power(&b.pcm) / power(&a.pcm)).log10();
        assert!(db.abs() < 0.1, "live state {second} pulled front {db} dB");
    }
}
