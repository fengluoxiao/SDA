//! Accepted common-front presentation lifecycle and warm mode transitions.
use super::*;
fn shared() -> hrtf::NativeHrtfSet {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
    hrtf::NativeHrtfSet::load_calibrated(&p).unwrap()
}
// Alternating samples measure the shelf's high-frequency asymptote. Check
// the configured filter itself across On/Off, reload and session reset.
fn output_shelf_db(e: &Engine) -> f64 {
    let mut filter=e.output_clarity[0];filter.reset();
    let mut amplitude=0.0f64;
    for n in 0..4096 {
        let out=filter.process(if n%2==0 {1.0} else {-1.0});
        if n>=3072 {amplitude+=f64::from(out.abs())/1024.;}
    }
    20.*amplitude.log10()
}
fn assert_accepted(e: &Engine, enabled: bool) {
    assert_eq!(e.spatial_enhancement_enabled, enabled);
    assert!((output_shelf_db(e)-if enabled {0.6} else {0.0}).abs()<1e-4);
    assert_eq!(e.front_common.enabled, enabled);
    assert!(e.front_common.grouped);
    assert_eq!(e.front_common.presence_db(), 3.);
    assert_eq!(e.spatial_preamp.target_db(), if enabled { 2. } else { 0. });
    assert_eq!(e.spatial_layer.db, if enabled { 4. } else { 0. });
    assert_eq!(e.spatial_layer.rear_db, 0.);
}
#[test]
fn accepted_front_policy_follows_enhancement_reload_and_transport_reset() {
    let mut e = Engine::new(48000, 2);
    e.replace_hrtf(shared(), 0.).unwrap();
    assert_accepted(&e, true);
    if let Ok(path) = std::env::var("SDA_ACCEPTED_FRONT_RECEIPT") {
        let report = serde_json::json!({"schemaVersion":1,"recipe":"shared-front-strong-v1",
            "revision":BUILD_REVISION,"sampleRate":e.output_sample_rate,
            "frontCommonEnabled":e.front_common.enabled,"grouped":e.front_common.grouped,
            "presenceDb":e.front_common.presence_db(),"presenceHz":3000,"presenceQ":0.9,
            "spatialPreampDb":e.spatial_preamp.target_db(),"auxiliaryExtraMaxDb":e.spatial_layer.db,
            "rearExtraDb":e.spatial_layer.rear_db,"outputShelfHz":4000,
            "outputShelfMaxDb":output_shelf_db(&e),"outputDynamics":{"enabled":e.spatial_enhancement_enabled&&e.binaural_enabled,"followsSpatialEnhancement":true,"targetLufs":e.output_dynamics.policy().target_lufs,"momentaryMarginDb":e.output_dynamics.policy().momentary_margin_db,"attackMs":e.output_dynamics.policy().attack_seconds*1000.,"releaseMs":e.output_dynamics.policy().release_seconds*1000.,"latencyFrames":e.output_dynamics.latency_frames(),"truePeakCeilingDb":true_peak_guard::CEILING_DB,"truePeakDetectorTaps":true_peak_guard::DETECTOR_TAPS,"peakGuardLatencyFrames":e.peak_guard.latency_frames()}});
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    e.reset_session(48000);
    assert_accepted(&e, true);
    assert!(e.front_common.center.is_none());
    e.set_spatial_enhancement(false).unwrap();
    assert_accepted(&e, false);
    e.replace_hrtf(shared(), 0.).unwrap();
    assert_accepted(&e, false);
    e.configure_hrtf_preset(shared(), 0., true, true).unwrap();
    assert_accepted(&e, false);
    e.set_spatial_enhancement(true).unwrap();
    assert_accepted(&e, true);
    let legacy = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mobile/assets/hrtf-mobile-direct/hrtf-set.json");
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&legacy).unwrap(), 0.)
        .unwrap();
    assert!(
        !e.front_common.enabled,
        "shared-only policy leaked into another HRTF family"
    );
    e.replace_hrtf(shared(), 0.).unwrap();
    assert_accepted(&e, true);
    let mut live = Engine::new(48000, 2);
    live.configure_hrtf_preset(shared(), 0., true, true)
        .unwrap();
    assert_accepted(&live, true);
}
fn scene(off: bool, slow: bool, codec: &str) -> Engine {
    let mut e = Engine::new(48000, 2);
    if codec == "mpegh" {
        e.set_layout(vbap::LayoutId::Sony360Ra13).unwrap();
    }
    e.replace_hrtf(shared(), 0.).unwrap();
    e.set_direct_objects(true).unwrap();
    e.set_directional_hrtf(true);
    e.set_program_codec(codec.into());
    e.set_binaural_enabled(!off);
    e.disable_fast_objects = slow;
    e.direct_mix = 1.;
    e.set_master_preamp_db(-12.).unwrap();
    e.set_output_volume(0.5, true);
    let mut entries = Vec::new();
    for i in 0..8 {
        let id = format!("arbitrary-geometry-{i}");
        let position = if i < 4 {
            [if i % 2 == 0 { -1. } else { 1. }, 1., 0.]
        } else {
            [if i % 2 == 0 { -1. } else { 1. }, -1., 0.4]
        };
        let source = Source {
            kind: SourceKind::Object,
            object_id: Some(101 + i),
            position,
            gain: 1.,
            target_gain: 1.,
            availability: 1.,
            availability_target: 1.,
            ..Default::default()
        };
        let pcm = (0..49152)
            .map(|n| {
                if i < 4 {
                    (n as f32 * 0.173).sin() * 0.001
                        + (n as f32 * (0.021 + i as f32 * 0.003)).cos() * 0.0001
                } else {
                    (n as f32 * (0.023 + i as f32 * 0.005)).sin() * 0.001
                }
            })
            .collect();
        e.sources.insert(id.clone(), source);
        e.route_source_now(&id, 0).unwrap();
        entries.push((id, pcm));
    }
    let compatibility = [
        (0..49152).map(|n| (n as f32 * 0.13).sin() * 0.01).collect(),
        (0..49152).map(|n| (n as f32 * 0.17).cos() * 0.01).collect(),
    ];
    let fifo = stereo_fifo::StereoFifo::new(4096);
    let telemetry = RuntimeTelemetry::default();
    let (tx, rx) = std::sync::mpsc::channel();
    assert!(protocol::apply_render_command(
        &mut e,
        render_command::RenderCommand::PcmFrameWithCompatibility {
            start: 0,
            entries,
            events: vec![],
            compatibility,
            reply: Some(tx)
        },
        &fifo,
        &telemetry
    ));
    assert!(rx.recv().unwrap());
    e.output_active = true;
    e.paused = false;
    e
}
#[test]
fn accepted_common_front_is_warm_through_off_fixed_and_head_pose_for_all_object_formats() {
    for codec in ["eac3", "mpegh", "future-object-format"] {
        for slow in [false, true] {
            let mut e = scene(true, slow, codec);
            let mut reference = scene(false, slow, codec);
            let mut ignored = vec![0.; 8192 * 2];
            e.render_into(&mut ignored, 2);
            reference.render_into(&mut ignored, 2);
            assert!(
                e.front_common.center.is_some(),
                "common path was not warm while Off: {codec}"
            );
            let center = e.front_common.center.as_deref().unwrap() as *const _ as usize;
            let objects: Vec<_> = e
                .sources
                .iter()
                .map(|(id, s)| {
                    (
                        id.clone(),
                        s.continuous.as_deref().unwrap() as *const _ as usize,
                    )
                })
                .collect();
            let epoch = e.render_epoch;
            e.set_binaural_enabled(true);
            assert_eq!(e.sample_pos, 8192);
            assert_eq!(e.render_epoch, epoch);
            assert_eq!(
                e.front_common.center.as_deref().unwrap() as *const _ as usize,
                center
            );
            for (id, pointer) in objects {
                assert_eq!(
                    e.sources[&id].continuous.as_deref().unwrap() as *const _ as usize,
                    pointer
                );
            }
            let mut actual = vec![0.; 8192 * 2];
            let mut expected = vec![0.; 8192 * 2];
            e.render_into(&mut actual, 2);
            reference.render_into(&mut expected, 2);
            let error = actual[2048..]
                .iter()
                .zip(&expected[2048..])
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(
                error < 3e-7,
                "common-path cold start: {codec}, slow={slow}, error={error}"
            );
            for engine in [&mut e, &mut reference] {
                let angle = std::f32::consts::FRAC_PI_8;
                engine.head_pose = Some([0., 0., angle.sin(), angle.cos()]);
                engine.pose_route_base = engine.head_pose;
                let ids: Vec<_> = engine.sources.keys().cloned().collect();
                for id in ids {
                    engine.route_source_now(&id, 256).unwrap();
                }
            }
            e.render_into(&mut actual, 2);
            reference.render_into(&mut expected, 2);
            let error = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(
                error < 3e-7,
                "pose changed common timing: {codec}, error={error}"
            );
            assert!(actual.iter().all(|v| v.is_finite() && v.abs() < 0.1));
            assert_eq!(e.peak_guard.diagnostic_gain(), 1.);
        }
    }
}
#[test]
fn accepted_common_front_enhancement_switch_retains_pcm_and_scene() {
    let mut e = scene(false, false, "eac3");
    let mut output = vec![0.; 8192 * 2];
    e.render_into(&mut output, 2);
    let at = e.sample_pos;
    let epoch = e.render_epoch;
    let count = e.sources.len();
    let center = e.front_common.center.as_deref().unwrap() as *const _ as usize;
    e.set_spatial_enhancement(false).unwrap();
    assert_accepted(&e, false);
    assert_eq!(e.sample_pos, at);
    assert_eq!(e.render_epoch, epoch);
    assert_eq!(e.sources.len(), count);
    assert_eq!(
        e.front_common.center.as_deref().unwrap() as *const _ as usize,
        center
    );
    e.render_into(&mut output, 2);
    e.set_spatial_enhancement(true).unwrap();
    assert_accepted(&e, true);
    e.render_into(&mut output, 2);
    assert!(output.iter().all(|x| x.is_finite() && x.abs() < 0.1));
    assert_eq!(e.sources.len(), count);
    assert_eq!(e.peak_guard.diagnostic_gain(), 1.);
}
