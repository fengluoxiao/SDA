//! Fixed-block uniform partitioned stereo convolution for the native renderer.
//!
//! Filter preparation happens before the audio callback. `process_block` reuses
//! every buffer it owns and accepts exactly one fixed PCM block, so the callback
//! can run FFT convolution without allocations or HRTF file access.

use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex32};

pub const DEFAULT_PARTITION: usize = 128;

/// Prepared spectral filters for one measured direction. Runtime state lives
/// in `StereoPartitionedConvolver`; a set of these partitions is therefore
/// reusable when multiple source directions map to the same HRTF.
#[derive(Debug, Clone)]
pub struct PreparedStereoFilter {
    filters_left: Vec<Vec<Complex32>>,
    filters_right: Vec<Vec<Complex32>>,
}

pub struct StereoPartitionedConvolver {
    partition: usize,
    fft_len: usize,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    filters_left: Vec<Vec<Complex32>>,
    filters_right: Vec<Vec<Complex32>>,
    history: Vec<Vec<Complex32>>,
    history_cursor: usize,
    previous_input: Vec<f32>,
    input_spectrum: Vec<Complex32>,
    work: Vec<Complex32>,
    sum_left: Vec<Complex32>,
    sum_right: Vec<Complex32>,
    output_left: Vec<Complex32>,
    output_right: Vec<Complex32>,
}

impl StereoPartitionedConvolver {
    pub fn new(left: &[f32], right: &[f32], partition: usize) -> Result<Self, String> {
        if partition == 0 || !partition.is_power_of_two() {
            return Err("partition size must be a non-zero power of two".into());
        }
        if left.is_empty() || left.len() != right.len() {
            return Err("stereo IR ears must have matching non-zero lengths".into());
        }
        if !left.iter().chain(right).all(|sample| sample.is_finite()) {
            return Err("stereo IR contains non-finite samples".into());
        }
        let fft_len = partition * 2;
        let count = left.len().div_ceil(partition);
        let mut planner = FftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(fft_len);
        let inverse = planner.plan_fft_inverse(fft_len);
        let filters_left = prepare_filters(left, partition, fft_len, &forward);
        let filters_right = prepare_filters(right, partition, fft_len, &forward);
        Ok(Self {
            partition,
            fft_len,
            forward,
            inverse,
            filters_left,
            filters_right,
            history: vec![vec![Complex32::new(0.0, 0.0); fft_len]; count],
            history_cursor: 0,
            previous_input: vec![0.0; partition],
            input_spectrum: vec![Complex32::new(0.0, 0.0); fft_len],
            work: vec![Complex32::new(0.0, 0.0); fft_len],
            sum_left: vec![Complex32::new(0.0, 0.0); fft_len],
            sum_right: vec![Complex32::new(0.0, 0.0); fft_len],
            output_left: vec![Complex32::new(0.0, 0.0); fft_len],
            output_right: vec![Complex32::new(0.0, 0.0); fft_len],
        })
    }

    pub fn partition(&self) -> usize {
        self.partition
    }
    pub fn latency_samples(&self) -> usize {
        self.partition
    }
    /// Reuses the convolver's allocated history/output state with a prepared
    /// direction filter, avoiding allocation while sources move.
    pub fn set_prepared_filter(&mut self, filter: PreparedStereoFilter) {
        self.filters_left = filter.filters_left;
        self.filters_right = filter.filters_right;
        self.reset();
    }
    pub fn prepared_filter(&self) -> PreparedStereoFilter {
        PreparedStereoFilter {
            filters_left: self.filters_left.clone(),
            filters_right: self.filters_right.clone(),
        }
    }
    pub fn reset(&mut self) {
        self.previous_input.fill(0.0);
        for spectrum in &mut self.history {
            spectrum.fill(Complex32::new(0.0, 0.0));
        }
        self.history_cursor = 0;
    }

    /// Adds the convolved stereo result into the supplied output buffers.
    pub fn process_block(
        &mut self,
        input: &[f32],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), String> {
        if input.len() != self.partition
            || left.len() != self.partition
            || right.len() != self.partition
        {
            return Err("partitioned convolver block length mismatch".into());
        }
        for index in 0..self.partition {
            self.work[index] = Complex32::new(self.previous_input[index], 0.0);
            self.work[index + self.partition] = Complex32::new(input[index], 0.0);
        }
        self.forward.process(&mut self.work);
        self.input_spectrum.copy_from_slice(&self.work);
        self.history[self.history_cursor].copy_from_slice(&self.input_spectrum);
        self.sum_left.fill(Complex32::new(0.0, 0.0));
        self.sum_right.fill(Complex32::new(0.0, 0.0));
        for filter_index in 0..self.history.len() {
            let history_index =
                (self.history_cursor + self.history.len() - filter_index) % self.history.len();
            let spectrum = &self.history[history_index];
            let filter_left = &self.filters_left[filter_index];
            let filter_right = &self.filters_right[filter_index];
            for bin in 0..self.fft_len {
                self.sum_left[bin] += spectrum[bin] * filter_left[bin];
                self.sum_right[bin] += spectrum[bin] * filter_right[bin];
            }
        }
        self.output_left.copy_from_slice(&self.sum_left);
        self.output_right.copy_from_slice(&self.sum_right);
        self.inverse.process(&mut self.output_left);
        self.inverse.process(&mut self.output_right);
        let scale = 1.0 / self.fft_len as f32;
        for index in 0..self.partition {
            left[index] += self.output_left[index + self.partition].re * scale;
            right[index] += self.output_right[index + self.partition].re * scale;
        }
        self.previous_input.copy_from_slice(input);
        self.history_cursor = (self.history_cursor + 1) % self.history.len();
        Ok(())
    }
}

fn prepare_filters(
    ir: &[f32],
    partition: usize,
    fft_len: usize,
    forward: &Arc<dyn Fft<f32>>,
) -> Vec<Vec<Complex32>> {
    (0..ir.len().div_ceil(partition))
        .map(|part| {
            let mut spectrum = vec![Complex32::new(0.0, 0.0); fft_len];
            let begin = part * partition;
            let end = (begin + partition).min(ir.len());
            for (offset, sample) in ir[begin..end].iter().enumerate() {
                spectrum[offset] = Complex32::new(*sample, 0.0);
            }
            forward.process(&mut spectrum);
            spectrum
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct(input: &[f32], ir: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0; input.len() + ir.len() - 1];
        for (i, x) in input.iter().enumerate() {
            for (j, h) in ir.iter().enumerate() {
                out[i + j] += x * h;
            }
        }
        out
    }

    #[test]
    fn partitioned_stereo_matches_direct_convolution_across_blocks() {
        let left = vec![0.25, -0.5, 1.0, 0.125, -0.0625, 0.03, -0.01];
        let right = vec![-0.2, 0.75, 0.1, -0.25, 0.05, 0.02, 0.01];
        let input: Vec<f32> = (0..23)
            .map(|n| ((n * 17 % 11) as f32 - 5.0) / 7.0)
            .collect();
        let mut convolver = StereoPartitionedConvolver::new(&left, &right, 4).unwrap();
        let padded = input.len().next_multiple_of(4) + left.len().next_multiple_of(4);
        let mut rendered_l = Vec::new();
        let mut rendered_r = Vec::new();
        for offset in (0..padded).step_by(4) {
            let mut block = [0.0; 4];
            for index in 0..4 {
                block[index] = *input.get(offset + index).unwrap_or(&0.0);
            }
            let mut out_l = [0.0; 4];
            let mut out_r = [0.0; 4];
            convolver
                .process_block(&block, &mut out_l, &mut out_r)
                .unwrap();
            rendered_l.extend(out_l);
            rendered_r.extend(out_r);
        }
        for (actual, expected) in rendered_l.iter().zip(direct(&input, &left)) {
            assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
        }
        for (actual, expected) in rendered_r.iter().zip(direct(&input, &right)) {
            assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
        }
    }
}
