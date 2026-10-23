//! The same inter-sample FIR, evaluated across independent phases with SIMD.
//! Each lane keeps the original tap order; no shorter filter, FMA, new ceiling
//! or changed lookahead is used. Coefficients are merely transposed at setup.

#[inline]
pub(super) fn peak(history: &[f64], coefficients: &[[f64; 4]], factor: usize) -> f64 {
    debug_assert_eq!(history.len(), coefficients.len());
    debug_assert!(matches!(factor, 1 | 2 | 4));
    if factor == 1 { return (history[0] * coefficients[0][0]).abs(); }
    #[cfg(target_arch = "aarch64")]
    { return unsafe { neon(history, coefficients, factor) }; }
    #[cfg(target_arch = "x86_64")]
    { return unsafe { sse2(history, coefficients, factor) }; }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    scalar(history, coefficients, factor)
}

#[cfg(any(test, not(any(target_arch = "aarch64", target_arch = "x86_64"))))]
pub(super) fn scalar(history: &[f64], coefficients: &[[f64; 4]], factor: usize) -> f64 {
    let mut peak = 0f64;
    for phase in 0..factor {
        let mut value = 0.;
        for (sample, weights) in history.iter().rev().zip(coefficients) { value += sample * weights[phase]; }
        peak = peak.max(value.abs());
    }
    peak
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn neon(history: &[f64], coefficients: &[[f64; 4]], factor: usize) -> f64 {
    use std::arch::aarch64::*;
    unsafe {
        let mut a = vdupq_n_f64(0.);
        let mut b = vdupq_n_f64(0.);
        for (sample, weights) in history.iter().rev().zip(coefficients) {
            let x = vdupq_n_f64(*sample);
            a = vaddq_f64(a, vmulq_f64(x, vld1q_f64(weights.as_ptr())));
            if factor == 4 { b = vaddq_f64(b, vmulq_f64(x, vld1q_f64(weights.as_ptr().add(2)))); }
        }
        let mut values = [0f64; 4];
        vst1q_f64(values.as_mut_ptr(), a);vst1q_f64(values.as_mut_ptr().add(2), b);
        values[..factor].iter().fold(0f64, |p,x| p.max(x.abs()))
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn sse2(history: &[f64], coefficients: &[[f64; 4]], factor: usize) -> f64 {
    use std::arch::x86_64::*;
    unsafe {
        let mut a = _mm_setzero_pd();
        let mut b = _mm_setzero_pd();
        for (sample, weights) in history.iter().rev().zip(coefficients) {
            let x = _mm_set1_pd(*sample);
            a = _mm_add_pd(a, _mm_mul_pd(x, _mm_loadu_pd(weights.as_ptr())));
            if factor == 4 { b = _mm_add_pd(b, _mm_mul_pd(x, _mm_loadu_pd(weights.as_ptr().add(2)))); }
        }
        let mut values = [0f64; 4];
        _mm_storeu_pd(values.as_mut_ptr(), a);_mm_storeu_pd(values.as_mut_ptr().add(2), b);
        values[..factor].iter().fold(0f64, |p,x| p.max(x.abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simd_is_bit_exact_to_original_phase_and_tap_order() {
        let mut seed = 13u64;
        for (taps, factor) in [(49,4),(97,2),(1,1)] {
            let mut history = vec![0.;taps];let mut coefficients = vec![[0.;4];taps];
            for i in 0..1200 {
                for x in &mut history {
                    seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    *x=(((seed>>32)as u32 as f64/u32::MAX as f64)*2.-1.)*if i%7==0{1e-310}else{2.};
                }
                for weights in &mut coefficients { for x in weights {
                    seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    *x=((seed>>32)as u32 as f64/u32::MAX as f64)-0.5;
                }}
                assert_eq!(peak(&history,&coefficients,factor).to_bits(),scalar(&history,&coefficients,factor).to_bits(),"taps={taps} factor={factor} index={i}");
            }
        }
    }
}
