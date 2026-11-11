//! Real shared mobile HRTF output tests, using deterministic synthetic PCM.
use super::*;
const FRAMES: usize = 65536;
fn yaw(degrees: f32) -> [f32; 4] {
    let h = degrees.to_radians() * 0.5; [0.0, 0.0, h.sin(), h.cos()]
}
fn render(kind: SourceKind, position: [f32; 3], turns: &[(usize, f32)]) -> Vec<f32> {
    render_balanced(kind, position, turns, false)
}
fn render_balanced(kind: SourceKind, position: [f32; 3], turns: &[(usize, f32)], balanced: bool) -> Vec<f32> {
    let mut e = Engine::new(48000, 2);
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
    e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap(), 0.0).unwrap();
    e.set_direct_objects(true).unwrap(); e.set_directional_hrtf(true); e.direct_mix = 1.0;
    // Isolate orientation/HRTF behavior from programme dynamics and world-group EQ.
    e.set_spatial_balance(balanced); e.front_common.enabled = balanced;
    e.set_spatial_enhancement(balanced).unwrap();
    if balanced { e.program_enabled=true; e.set_program_target(0.8,true); }
    e.set_program_codec("eac3".into()); e.output_active = true; e.paused = false;
    let mut seed=0x7531acefu32;
    let input: Vec<f32> = (0..FRAMES).map(|i| {
        seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5;
        if i < 4096 { 0.0 } else { (seed as f64/u32::MAX as f64*2.0-1.0) as f32*0.01 }
    }).collect();
    let mut source = Source { kind, object_id: (kind==SourceKind::Object).then_some(1),
        bed_label: (kind==SourceKind::Bed).then(||"Center".into()),
        authored_bed_position: (kind==SourceKind::Bed).then_some(position), position,
        gain:1.0, target_gain:1.0, availability:1.0, availability_target:1.0, ..Default::default() };
    source.samples.write(0,0,&input); e.sources.insert("source".into(),source);
    e.head_pose=Some(yaw(0.0)); e.route_source_now("source",0).unwrap();
    let fifo=stereo_fifo::StereoFifo::new(65536);let telemetry=RuntimeTelemetry::default();
    let mut output=vec![0.0;FRAMES*2];
    for frame in (0..FRAMES).step_by(convolution::DEFAULT_PARTITION) {
        if let Some((_,degrees))=turns.iter().find(|(at,_)|*at==frame) {
            // Offline rendering is faster than the live wall clock. Admit this
            // scheduled sample exactly as a fresh 30Hz native provider sample.
            e.last_pose_apply=None;
            super::handle_command(&mut e,Command::HeadPose{orientation:yaw(*degrees)},&fifo,&telemetry);
        }
        e.render_into(&mut output[frame*2..(frame+convolution::DEFAULT_PARTITION)*2],2);
    }
    assert!(output.iter().all(|v|v.is_finite()));
    assert!(output.iter().any(|v|v.abs()>1e-6));
    output
}
fn relative_error(a: &[f32], b: &[f32]) -> f64 {
    let error=a.iter().zip(b).map(|(a,b)|f64::from(a-b).powi(2)).sum::<f64>();
    let energy=b.iter().map(|v|f64::from(*v).powi(2)).sum::<f64>(); (error/energy.max(1e-30)).sqrt()
}
#[test]
fn head_tracking_return_to_forward_restores_the_same_pcm_without_a_muffled_tail() {
    for kind in [SourceKind::Object,SourceKind::Bed] {
        let reference=render(kind,[0.0,1.0,0.0],&[]);
        let moved=render(kind,[0.0,1.0,0.0],&[(16384,45.0),(32768,0.0)]);
        let error=relative_error(&moved[45056*2..],&reference[45056*2..]);
        println!("head return object={} relative waveform error={error}",kind==SourceKind::Object);
        assert!(error<2e-4,"pose return leaves extra filtering or a stale route: {error}");
    }
}
#[test]
fn held_head_turn_matches_the_equivalent_authored_direction_and_preserves_ear_waveforms() {
    for kind in [SourceKind::Object,SourceKind::Bed] {
        let pose=yaw(45.0);
        let physical=spatial::head_relative_adm([0.0,1.0,0.0],Some(pose));
        let reference=render(kind,physical,&[]);
        let moved=render(kind,[0.0,1.0,0.0],&[(16384,45.0)]);
        let error=relative_error(&moved[45056*2..],&reference[45056*2..]);
        println!("head held object={} relative waveform error={error}",kind==SourceKind::Object);
        assert!(error<2e-4,"head tracking adds loss beyond the measured HRTF for that direction: {error}");
    }
}

#[test]
fn head_tracking_with_spatial_and_programme_balance_does_not_leave_a_timbre_change_after_return() {
    for kind in [SourceKind::Object,SourceKind::Bed] {
        let reference=render_balanced(kind,[0.0,1.0,0.0],&[],true);
        let moved=render_balanced(kind,[0.0,1.0,0.0],&[(16384,-55.0),(32768,0.0)],true);
        let error=relative_error(&moved[45056*2..],&reference[45056*2..]);
        println!("head return balanced object={} relative waveform error={error}",kind==SourceKind::Object);
        assert!(error<2e-4,"spatial/programme controls retained a head-turn timbre change: {error}");
    }
}
