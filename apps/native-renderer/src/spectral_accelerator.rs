//! Bounded, optional FP32 HRTF spectral-MAC offload. CPU FFTs, filter motion,
//! source state and the output guard remain in the renderer. Android selects an
//! explicit NNAPI accelerator. iOS requests Core ML CPU/NE and labels its
//! framework-managed, unverified per-prediction hardware placement separately.
use rustfft::num_complex::Complex32;

mod model_graph;
pub(crate) use model_graph::{Graph, Shape};
pub(crate) struct Batch {
    pub shape: Shape,
    pub input: Vec<f32>,
    pub output: Vec<f32>,
    pub reference: Vec<f32>,
}
impl Batch {
    pub fn new(shape: Shape) -> Self {
        Self {
            shape,
            input: vec![0.; shape.plane() * 6 * shape.parts],
            output: vec![0.; shape.plane() * 4],
            reference: vec![0.; shape.plane() * 4],
        }
    }
    pub fn clear(&mut self) {
        self.input.fill(0.);
        self.output.fill(0.);
        self.reference.fill(0.);
    }
    pub fn pack(
        &mut self,
        row: usize,
        part: usize,
        input: &[Complex32],
        left: &[Complex32],
        right: &[Complex32],
    ) {
        let s = self.shape;
        assert!(row < s.rows && part < s.parts);
        assert!(input.len() >= s.bins && left.len() >= s.bins && right.len() >= s.bins);
        let base = part * 6 * s.plane() + row * s.bins;
        for (field, values, imaginary) in [
            (0, input, false),
            (1, input, true),
            (2, left, false),
            (3, left, true),
            (4, right, false),
            (5, right, true),
        ] {
            let dst = &mut self.input[base + field * s.plane()..][..s.bins];
            for (dst, value) in dst.iter_mut().zip(values) {
                *dst = if imaginary { value.im } else { value.re };
            }
        }
    }
    pub fn store_reference(&mut self, row: usize, left: &[Complex32], right: &[Complex32]) {
        let s = self.shape;
        assert!(row < s.rows);
        for (ear, values) in [left, right].into_iter().enumerate() {
            for (bin, value) in values.iter().take(s.bins).enumerate() {
                self.reference[(ear * 2) * s.plane() + row * s.bins + bin] = value.re;
                self.reference[(ear * 2 + 1) * s.plane() + row * s.bins + bin] = value.im;
            }
        }
    }
    pub fn load_output(&self, row: usize, left: &mut [Complex32], right: &mut [Complex32]) {
        let s = self.shape;
        assert!(row < s.rows);
        // inverse_stereo reconstructs negative frequencies from these unique bins.
        left.fill(Complex32::new(0., 0.));
        right.fill(Complex32::new(0., 0.));
        for (ear, values) in [left, right].into_iter().enumerate() {
            for (bin, value) in values.iter_mut().take(s.bins).enumerate() {
                *value = Complex32::new(
                    self.output[(ear * 2) * s.plane() + row * s.bins + bin],
                    self.output[(ear * 2 + 1) * s.plane() + row * s.bins + bin],
                );
            }
        }
    }
    pub fn reference_matches(&self) -> bool {
        let mut energy = 0f64;
        let mut error = 0f64;
        let mut peak = 0f32;
        let mut delta = 0f32;
        for (&actual, &expected) in self.output.iter().zip(&self.reference) {
            if !actual.is_finite() || !expected.is_finite() {
                return false;
            }
            let d = actual as f64 - expected as f64;
            energy += (expected as f64).powi(2);
            error += d * d;
            peak = peak.max(expected.abs());
            delta = delta.max(d.abs() as f32);
        }
        // <= -120 dB RMS error relative to reference; also bound individual bins.
        // No absolute floor: a quiet signal must not qualify after being rounded away.
        error <= energy * 1e-12 && delta <= peak * 4e-6
    }
    #[cfg(test)]
    pub fn scalar_reference(&mut self) {
        let n = self.shape.plane();
        self.output.fill(0.);
        for part in 0..self.shape.parts {
            let v = &self.input[part * 6 * n..];
            for i in 0..n {
                for ear in 0..2 {
                    let hr = v[(2 + 2 * ear) * n + i];
                    let hi = v[(3 + 2 * ear) * n + i];
                    self.output[(2 * ear) * n + i] += v[i] * hr - v[n + i] * hi;
                    self.output[(2 * ear + 1) * n + i] += v[i] * hi + v[n + i] * hr;
                }
            }
        }
    }
}

#[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio"), test))]
mod coordinator;
#[cfg(any(all(target_os = "ios", feature = "coreml-audio"), test))]
mod coreml_model;
#[cfg(all(target_os = "ios", feature = "coreml-audio"))]
#[path = "coreml_spectral.rs"]
mod coreml;
#[cfg(target_os = "android")]
#[path = "nnapi_spectral.rs"]
mod nnapi;
#[cfg(any(target_os = "android", test))]
mod litert_model;
#[cfg(any(target_os = "android", test))]
mod hexagon_graph;
#[cfg(target_os = "android")]
#[path = "litert_spectral.rs"]
mod litert;
#[cfg(target_os = "android")]
#[path = "hexagon_spectral.rs"]
mod hexagon;
#[cfg(target_os = "android")]
#[path = "android_spectral.rs"]
mod android;
/// Called once after the Android host prepares optional public runtime assets.
#[cfg(target_os = "android")]
pub fn configure_android_runtime(directory: &str, sdk: u32, preparation: &str) -> Result<(), String> {
    android::configure(directory, sdk, preparation)?;
    start();
    Ok(())
}

#[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
pub(crate) use coordinator::{begin, observe_native_block};

#[cfg(all(target_os = "android", feature = "pcm-diagnostic"))]
mod diagnostic;
/// Diagnostic executable only; invokes the real backend without starting audio output.
#[cfg(all(target_os = "android", feature = "pcm-diagnostic"))]
pub fn probe_legacy_runtime(directory: &str) -> Result<serde_json::Value, String> {
    diagnostic::probe_legacy(directory)
}

pub fn start() {
    #[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
    coordinator::start();
}
pub fn status() -> serde_json::Value {
    #[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
    {
        coordinator::status()
    }
    #[cfg(not(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio"))))]
    {
        serde_json::json!({"state":"unsupported-platform", "npuAudioEnabled":false})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_memory_is_bounded_for_dense_scenes() {
        assert!(Shape::bounded(256, 2, 1025).is_some());
        assert!(Shape::bounded(256, 4, 1025).is_none());
        assert!(Shape::bounded(257, 1, 1025).is_none());
        assert!(Shape::bounded(16, 5, 1025).is_none());
        assert!(Shape::bounded(0, 1, 1025).is_none());
    }
    #[test]
    fn graph_matches_complex_mac_for_each_partition_count() {
        for parts in 1..=4 {
            let shape = Shape::bounded(8, parts, 17).unwrap();
            let mut b = Batch::new(shape);
            for (i, v) in b.input.iter_mut().enumerate() {
                *v = ((i * 7919 % 1021) as f32 - 510.) / 511.;
            }
            b.scalar_reference();
            let expected = b.output.clone();
            let graph = Graph::new(parts);
            let n = shape.plane();
            let mut values = vec![vec![0.; n]; graph.operands as usize];
            for (i, v) in b.input.chunks_exact(n).enumerate() {
                values[i].copy_from_slice(v);
            }
            for (op, args, out) in &graph.operations {
                for i in 0..n {
                    values[*out as usize][i] = match op {
                        0 => values[args[0] as usize][i] + values[args[1] as usize][i],
                        18 => values[args[0] as usize][i] * values[args[1] as usize][i],
                        36 => values[args[0] as usize][i] - values[args[1] as usize][i],
                        _ => panic!(),
                    };
                }
            }
            for (i, out) in graph.outputs.iter().enumerate() {
                assert_eq!(&expected[i * n..][..n], &values[*out as usize]);
            }
            assert_eq!(graph.operations.len(), 12 * parts + 4 * (parts - 1));
        }
    }
    #[test]
    fn reduced_mantissa_results_cannot_enable_audio_offload() {
        let mut batch = Batch::new(Shape::bounded(2, 2, 17).unwrap());
        for (i, v) in batch.reference.iter_mut().enumerate() {
            *v = ((i * 7919 % 1021) as f32 - 510.) / 511.;
        }
        // Removing 13 mantissa bits approximates the precision loss of a
        // reduced-precision compute path; a framework flag cannot waive it.
        for (out, &value) in batch.output.iter_mut().zip(&batch.reference) {
            *out = f32::from_bits(value.to_bits() & !0x1fff);
        }
        assert!(!batch.reference_matches());
        batch.output.copy_from_slice(&batch.reference);
        assert!(batch.reference_matches());
    }
    #[test]
    fn quiet_and_nonfinite_outputs_cannot_pass_precision_gate() {
        let mut b = Batch::new(Shape::bounded(2, 2, 9).unwrap());
        b.reference.fill(1e-12);
        b.output.fill(0.);
        assert!(!b.reference_matches());
        b.output.copy_from_slice(&b.reference);
        assert!(b.reference_matches());
        b.output[0] = f32::NAN;
        assert!(!b.reference_matches());
        b.output[0] = f32::INFINITY;
        assert!(!b.reference_matches());
    }
    #[test]
    fn actual_convolution_pcm_transition_tail_and_fallback_match() {
        use crate::convolution::StereoPartitionedConvolver;
        for parts in 1..=4 {
            let partition = 16;
            let bins = partition + 1;
            let l: Vec<_> = (0..partition * parts)
                .map(|i| ((i * 17 % 31) as f32 - 15.) * 0.003)
                .collect();
            let rr: Vec<_> = l.iter().rev().map(|x| x * 0.7).collect();
            let mut cpu = StereoPartitionedConvolver::new(&l, &rr, partition).unwrap();
            let mut candidate = StereoPartitionedConvolver::new(&l, &rr, partition).unwrap();
            let mut batch = Batch::new(Shape::bounded(2, parts, bins).unwrap());
            for block in 0..30 {
                if block == 4 || block == 9 || block == 22 {
                    let target: Vec<_> = l.iter().map(|x| -x * 0.4).collect();
                    cpu.transition_to_pair(&rr, &target, 37);
                    candidate.transition_to_pair(&rr, &target, 37);
                }
                let input: Vec<_> = (0..partition)
                    .map(|i| {
                        if block >= 12 {
                            0.
                        } else {
                            (((block * partition + i) * 127 % 997) as f32 - 498.) / 499.
                        }
                    })
                    .collect();
                let mut a = vec![0.; partition];
                let mut ar = a.clone();
                let mut b = a.clone();
                let mut br = a.clone();
                cpu.process_block(&input, &mut a, &mut ar).unwrap();
                candidate.begin_block(&input).unwrap();
                // Exercise CPU fallback on the same already prepared block,
                // including during retargeting and while draining filter tails.
                if block % 3 == 0 {
                    candidate.calculate_spectra();
                } else {
                    batch.clear();
                    candidate.pack_spectra(&mut batch, 0);
                    batch.scalar_reference();
                    candidate.import_spectra(&batch, 0);
                }
                candidate.finish_prepared_block(&input, &mut b, &mut br);
                for (x, y) in a.iter().chain(&ar).zip(b.iter().chain(&br)) {
                    assert!(
                        (x - y).abs() < 2e-7,
                        "parts={parts} block={block}: {x} != {y}"
                    );
                }
            }
        }
    }
    #[test]
    fn reset_discards_a_prepared_block_before_fallback() {
        let mut c=crate::convolution::StereoPartitionedConvolver::new(&[0.25;32],&[0.5;32],16).unwrap();
        c.begin_block(&[1.;16]).unwrap();assert!(c.block_is_prepared());c.reset();
        assert!(!c.block_is_prepared());c.calculate_spectra();
        let mut l=[0.;16];let mut r=[0.;16];c.finish_prepared_block(&[1.;16],&mut l,&mut r);
        assert_eq!(l,[0.;16]);assert_eq!(r,[0.;16]);
    }

}
