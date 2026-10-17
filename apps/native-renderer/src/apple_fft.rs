//! Accelerate complex DFT with invocation-local, caller-owned workspace.
use rustfft::{Direction, Fft, FftDirection, Length, num_complex::Complex32};
use std::{ffi::c_void, sync::Arc};

#[cfg_attr(target_vendor = "apple", link(name = "Accelerate", kind = "framework"))]
unsafe extern "C" {
    fn vDSP_DFT_zop_CreateSetup(
        previous: *mut c_void,
        length: usize,
        direction: i32,
    ) -> *mut c_void;
    fn vDSP_DFT_Execute(
        setup: *const c_void,
        ir: *const f32,
        ii: *const f32,
        or: *mut f32,
        oi: *mut f32,
    );
    fn vDSP_DFT_DestroySetup(setup: *mut c_void);
}
struct AppleFft {
    setup: *mut c_void,
    length: usize,
    direction: FftDirection,
}
// Setup is immutable during Execute; workspace belongs to each invocation.
unsafe impl Send for AppleFft {}
unsafe impl Sync for AppleFft {}
impl Drop for AppleFft {
    fn drop(&mut self) {
        unsafe {
            vDSP_DFT_DestroySetup(self.setup);
        }
    }
}
impl Length for AppleFft {
    fn len(&self) -> usize {
        self.length
    }
}
impl Direction for AppleFft {
    fn fft_direction(&self) -> FftDirection {
        self.direction
    }
}
impl AppleFft {
    fn transform(&self, input: &[Complex32], output: &mut [Complex32], scratch: &mut [Complex32]) {
        assert_eq!(input.len(), output.len());
        assert!(!input.is_empty() && input.len() % self.length == 0);
        assert!(scratch.len() >= 2 * self.length);
        let floats = unsafe {
            std::slice::from_raw_parts_mut(scratch.as_mut_ptr().cast::<f32>(), 4 * self.length)
        };
        let (a, b) = floats.split_at_mut(2 * self.length);
        let (ir, ii) = a.split_at_mut(self.length);
        let (or, oi) = b.split_at_mut(self.length);
        for (input, output) in input
            .chunks_exact(self.length)
            .zip(output.chunks_exact_mut(self.length))
        {
            for (i, x) in input.iter().enumerate() {
                ir[i] = x.re;
                ii[i] = x.im;
            }
            unsafe {
                vDSP_DFT_Execute(
                    self.setup,
                    ir.as_ptr(),
                    ii.as_ptr(),
                    or.as_mut_ptr(),
                    oi.as_mut_ptr(),
                );
            }
            for (i, x) in output.iter_mut().enumerate() {
                *x = Complex32::new(or[i], oi[i]);
            }
        }
    }
}
impl Fft<f32> for AppleFft {
    fn process_with_scratch(&self, buffer: &mut [Complex32], scratch: &mut [Complex32]) {
        assert!(!buffer.is_empty() && buffer.len() % self.length == 0);
        assert!(scratch.len() >= 2 * self.length);
        let floats = unsafe {
            std::slice::from_raw_parts_mut(scratch.as_mut_ptr().cast::<f32>(), 4 * self.length)
        };
        let (a, b) = floats.split_at_mut(2 * self.length);
        let (ir, ii) = a.split_at_mut(self.length);
        let (or, oi) = b.split_at_mut(self.length);
        for block in buffer.chunks_exact_mut(self.length) {
            for (i, x) in block.iter().enumerate() {
                ir[i] = x.re;
                ii[i] = x.im;
            }
            unsafe {
                vDSP_DFT_Execute(
                    self.setup,
                    ir.as_ptr(),
                    ii.as_ptr(),
                    or.as_mut_ptr(),
                    oi.as_mut_ptr(),
                );
            }
            for (i, x) in block.iter_mut().enumerate() {
                *x = Complex32::new(or[i], oi[i]);
            }
        }
    }
    fn process_outofplace_with_scratch(
        &self,
        input: &mut [Complex32],
        output: &mut [Complex32],
        scratch: &mut [Complex32],
    ) {
        self.transform(input, output, scratch);
    }
    fn process_immutable_with_scratch(
        &self,
        input: &[Complex32],
        output: &mut [Complex32],
        scratch: &mut [Complex32],
    ) {
        self.transform(input, output, scratch);
    }
    fn get_inplace_scratch_len(&self) -> usize {
        2 * self.length
    }
    fn get_outofplace_scratch_len(&self) -> usize {
        2 * self.length
    }
    fn get_immutable_scratch_len(&self) -> usize {
        2 * self.length
    }
}
pub(super) fn plan(length: usize, direction: FftDirection) -> Option<Arc<dyn Fft<f32>>> {
    if length < 8 || !length.is_power_of_two() {
        return None;
    }
    let sign = if direction == FftDirection::Forward {
        1
    } else {
        -1
    };
    let setup = unsafe { vDSP_DFT_zop_CreateSetup(std::ptr::null_mut(), length, sign) };
    if setup.is_null() {
        None
    } else {
        Some(Arc::new(AppleFft {
            setup,
            length,
            direction,
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apple_fft_roundtrip_preserves_impulse_and_silence() {
        for n in [16, 256, 2048] {
            let forward = plan(n, FftDirection::Forward).unwrap();
            let inverse = plan(n, FftDirection::Inverse).unwrap();
            for impulse in [None, Some(0), Some(n - 1)] {
                let mut input = vec![Complex32::default(); n];
                if let Some(i) = impulse {
                    input[i] = Complex32::new(0.25, -0.5);
                }
                let expected = input.clone();
                let mut scratch = vec![Complex32::default(); 2 * n];
                forward.process_with_scratch(&mut input, &mut scratch);
                inverse.process_with_scratch(&mut input, &mut scratch);
                for (actual, expected) in input.iter().zip(expected) {
                    assert!((*actual / n as f32 - expected).norm() < 2e-6);
                }
            }
        }
    }
    #[test]
    #[ignore = "Apple host performance measurement, not a correctness gate"]
    fn benchmark_apple_against_rustfft() {
        let n = 2048;
        let input: Vec<_> = (0..n)
            .map(|i| Complex32::new((i as f32 * 0.13).sin() * 0.1, 0.0))
            .collect();
        for direction in [FftDirection::Forward, FftDirection::Inverse] {
            let reference = rustfft::FftPlanner::new().plan_fft(n, direction);
            for (name, fft) in [
                ("rustfft", reference),
                ("accelerate", plan(n, direction).unwrap()),
            ] {
                let mut buffer = input.clone();
                let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
                let mut times = Vec::with_capacity(1000);
                for i in 0..1100 {
                    buffer.copy_from_slice(&input);
                    let start = std::time::Instant::now();
                    fft.process_with_scratch(&mut buffer, &mut scratch);
                    if i >= 100 {
                        times.push(start.elapsed().as_nanos());
                    }
                }
                times.sort();
                eprintln!(
                    "{name} {direction:?} median_ns={} p99_ns={}",
                    times[500], times[990]
                );
            }
        }
    }
    #[test]
    fn apple_fft_matches_reference_and_concurrent_execution() {
        for n in [8, 16, 256, 2048] {
            for direction in [FftDirection::Forward, FftDirection::Inverse] {
                let apple = plan(n, direction).unwrap();
                let reference = rustfft::FftPlanner::new().plan_fft(n, direction);
                let input: Vec<_> = (0..n * 2)
                    .map(|i| {
                        Complex32::new(
                            (i as f32 * 0.17).sin() * 0.1,
                            (i as f32 * 0.23).cos() * 0.07,
                        )
                    })
                    .collect();
                let mut expected = input.clone();
                reference.process(&mut expected);
                std::thread::scope(|scope| {
                    let handles: Vec<_> = (0..2)
                        .map(|_| {
                            scope.spawn(|| {
                                let mut out = input.clone();
                                apple.process(&mut out);
                                out
                            })
                        })
                        .collect();
                    for h in handles {
                        for (a, b) in h.join().unwrap().iter().zip(&expected) {
                            assert!((*a - *b).norm() < 2e-5 * (1.0 + b.norm()));
                        }
                    }
                });
                let mut output = input.clone();
                let mut scratch = vec![Complex32::default(); 2 * n];
                apple.process_immutable_with_scratch(&input, &mut output, &mut scratch);
                for (a, b) in output.iter().zip(&expected) {
                    assert!((*a - *b).norm() < 2e-5 * (1.0 + b.norm()));
                }
            }
        }
    }
}
