#[path = "../../../packages/core/src/loudness.rs"]
mod loudness;
#[path = "support/loudness_reference.rs"]
mod reference;

#[test]
fn programme_meter_matches_scalar_reference() {
    // Generate one input for both meters on this platform. sin()/libm bits can
    // differ between MSVC/GNU; the optimized meter must match the scalar meter.
    let base: Vec<Vec<f32>> = (0..12).map(|c| (0..1024).map(|i|
        (i as f32 * (0.01 + c as f32 * 0.003)).sin() * 0.12
    ).collect()).collect();
    let mut current = loudness::LoudnessMeter::speakers_7_1_4();
    let mut scalar = reference::LoudnessMeter::speakers_7_1_4();
    for frame in 0..12*48000/1024 {
        let scale = [1.0f32,0.001,0.15,0.0][frame / 141 % 4];
        let pcm: Vec<Vec<f32>> = base.iter().map(|ch| ch.iter().map(|x| x*scale).collect()).collect();
        current.push(&pcm); scalar.push(&pcm);
        if frame % 53 == 0 {
            let a = current.integrated(); let b = scalar.integrated();
            assert_eq!(a.blocks,b.blocks);
            assert_eq!(a.integrated_lufs.map(f64::to_bits),b.integrated_lufs.map(f64::to_bits));
            assert_eq!(a.true_peak_dbtp.map(f64::to_bits),b.true_peak_dbtp.map(f64::to_bits));
        }
    }
    let a = current.integrated(); let b = scalar.integrated();
    assert!(a.blocks > 50);
    assert_eq!(a.blocks,b.blocks);
    assert_eq!(a.integrated_lufs.map(f64::to_bits),b.integrated_lufs.map(f64::to_bits));
    assert_eq!(a.true_peak_dbtp.map(f64::to_bits),b.true_peak_dbtp.map(f64::to_bits));
}
