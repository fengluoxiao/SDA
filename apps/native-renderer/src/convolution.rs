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

impl PreparedStereoFilter {
    pub fn scale(&mut self, gain: f32) {
        for value in self.filters_left.iter_mut().chain(self.filters_right.iter_mut()).flatten() {
            *value *= gain;
        }
    }

    pub fn blend(&mut self, other: &Self, weight: f32) {
        for (value, target) in self.filters_left.iter_mut().flatten().zip(other.filters_left.iter().flatten()) {
            *value += (*target - *value) * weight;
        }
        for (value, target) in self.filters_right.iter_mut().flatten().zip(other.filters_right.iter().flatten()) {
            *value += (*target - *value) * weight;
        }
    }
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
    fft_scratch: Vec<Complex32>,
    silent_blocks: usize,
    transition: Option<(PreparedStereoFilter, usize, usize)>,
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
        let scratch_len = forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len());
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
            fft_scratch: vec![Complex32::new(0.0, 0.0); scratch_len],
            silent_blocks: count + 1,
            transition: None,
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
        self.transition = None;
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
    /// Retarget without discarding the overlap or the partition history.
    pub fn transition_to(&mut self, filter: PreparedStereoFilter, samples: usize) {
        assert_eq!(filter.filters_left.len(), self.history.len());
        if let Some((old, elapsed, duration)) = self.transition.take() {
            let mix = elapsed as f32 / duration as f32;
            for (current, target) in self.filters_left.iter_mut().flatten().zip(old.filters_left.iter().flatten()) {
                *current += (*target - *current) * mix;
            }
            for (current, target) in self.filters_right.iter_mut().flatten().zip(old.filters_right.iter().flatten()) {
                *current += (*target - *current) * mix;
            }
        }
        self.transition = Some((filter, 0, samples.max(1)));
    }
    pub fn reset(&mut self) {
        self.previous_input.fill(0.0);
        for spectrum in &mut self.history {
            spectrum.fill(Complex32::new(0.0, 0.0));
        }
        self.history_cursor = 0;
        self.silent_blocks = self.history.len() + 1;
        if let Some((filter, _, _)) = self.transition.take() {
            self.filters_left = filter.filters_left;
            self.filters_right = filter.filters_right;
        }
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
        // Overlap-save keeps the preceding block in each history spectrum.
        // Drain that overlap and every filter partition before skipping silence.
        if input.iter().all(|sample| *sample == 0.0) {
            if self.silent_blocks >= self.history.len() + 1 {
                if let Some((filter, _, _)) = self.transition.take() {
                    self.filters_left = filter.filters_left;
                    self.filters_right = filter.filters_right;
                }
                return Ok(());
            }
            self.silent_blocks += 1;
        } else {
            self.silent_blocks = 0;
        }
        for index in 0..self.partition {
            self.work[index] = Complex32::new(self.previous_input[index], 0.0);
            self.work[index + self.partition] = Complex32::new(input[index], 0.0);
        }
        self.forward
            .process_with_scratch(&mut self.work, &mut self.fft_scratch);
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
        self.inverse
            .process_with_scratch(&mut self.output_left, &mut self.fft_scratch);
        self.inverse
            .process_with_scratch(&mut self.output_right, &mut self.fft_scratch);
        let scale = 1.0 / self.fft_len as f32;
        if let Some((target, elapsed, duration)) = &mut self.transition {
            self.sum_left.fill(Complex32::new(0.0, 0.0));
            self.sum_right.fill(Complex32::new(0.0, 0.0));
            for part in 0..self.history.len() {
                let spectrum = &self.history[(self.history_cursor + self.history.len() - part) % self.history.len()];
                for bin in 0..self.fft_len {
                    self.sum_left[bin] += spectrum[bin] * target.filters_left[part][bin];
                    self.sum_right[bin] += spectrum[bin] * target.filters_right[part][bin];
                }
            }
            self.inverse.process_with_scratch(&mut self.sum_left, &mut self.fft_scratch);
            self.inverse.process_with_scratch(&mut self.sum_right, &mut self.fft_scratch);
            for index in 0..self.partition {
                let mix = ((*elapsed + index) as f32 / *duration as f32).min(1.0);
                let bin = index + self.partition;
                self.output_left[bin] = self.output_left[bin] * (1.0 - mix) + self.sum_left[bin] * mix;
                self.output_right[bin] = self.output_right[bin] * (1.0 - mix) + self.sum_right[bin] * mix;
            }
            *elapsed += self.partition;
        }
        for index in 0..self.partition {
            left[index] += self.output_left[index + self.partition].re * scale;
            right[index] += self.output_right[index + self.partition].re * scale;
        }
        if self.transition.as_ref().is_some_and(|(_, elapsed, duration)| elapsed >= duration) {
            let (filter, _, _) = self.transition.take().unwrap();
            self.filters_left = filter.filters_left;
            self.filters_right = filter.filters_right;
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

    #[test]
    fn filter_transition_preserves_history_and_retargets_continuously() {
        let a = [1.0, 0.0, 0.5, 0.2, -0.3, 0.4, 0.2, 0.1];
        let b = [0.3, -0.2, 0.1, 0.4, 0.2, 0.1, -0.3, 0.5];
        let mut actual = StereoPartitionedConvolver::new(&a, &a, 4).unwrap();
        let mut reference_a = StereoPartitionedConvolver::new(&a, &a, 4).unwrap();
        let mut reference_b = StereoPartitionedConvolver::new(&b, &b, 4).unwrap();
        for block in 0..6 {
            if block == 1 { actual.transition_to(reference_b.prepared_filter(), 16); }
            if block == 3 { actual.transition_to(reference_a.prepared_filter(), 8); }
            let input = if block < 2 { [0.2, -0.3, 0.5, 0.1] } else { [0.0; 4] };
            let (mut out, mut right, mut out_a, mut right_a, mut out_b, mut right_b) = ([0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]);
            actual.process_block(&input, &mut out, &mut right).unwrap();
            reference_a.process_block(&input, &mut out_a, &mut right_a).unwrap();
            reference_b.process_block(&input, &mut out_b, &mut right_b).unwrap();
            for i in 0..4 {
                let mix = match block {
                    0 => 0.0,
                    1 | 2 => ((block - 1) * 4 + i) as f32 / 16.0,
                    3 | 4 => 0.5 * (1.0 - ((block - 3) * 4 + i) as f32 / 8.0),
                    _ => 0.0,
                };
                let expected = out_a[i] * (1.0 - mix) + out_b[i] * mix;
                assert!((out[i] - expected).abs() < 1e-6, "block={block} sample={i}");
                assert!((right[i] - expected).abs() < 1e-6);
            }
        }
    }

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

    #[test]
    fn silence_drains_full_tail_and_preserves_reentry_and_additive_output() {
        let partition = 8;
        let left: Vec<f32> = (0..37).map(|n| (n as f32 * 0.7).sin() / 40.0).collect();
        let right: Vec<f32> = left.iter().rev().map(|sample| -*sample).collect();
        let mut input = vec![0.0; 256];
        input[3] = 0.8;
        input[9] = -0.4;
        input[151] = 0.7;
        input[159] = -0.2;
        let expected_left = direct(&input, &left);
        let expected_right = direct(&input, &right);
        let mut convolver = StereoPartitionedConvolver::new(&left, &right, partition).unwrap();
        for (block_index, block) in input.chunks_exact(partition).enumerate() {
            let mut out_left = vec![0.25; partition];
            let mut out_right = vec![-0.5; partition];
            convolver
                .process_block(block, &mut out_left, &mut out_right)
                .unwrap();
            for index in 0..partition {
                let at = block_index * partition + index;
                assert!(
                    (out_left[index] - 0.25 - expected_left[at]).abs() < 1e-5,
                    "left at {at}"
                );
                assert!(
                    (out_right[index] + 0.5 - expected_right[at]).abs() < 1e-5,
                    "right at {at}"
                );
            }
        }
        assert_eq!(convolver.silent_blocks, convolver.history.len() + 1);
        assert!(
            convolver
                .history
                .iter()
                .flatten()
                .all(|bin| *bin == Complex32::new(0.0, 0.0))
        );
    }

    #[test]
    fn reset_drops_tail_and_wakes_on_the_first_nonzero_sample() {
        let left = [0.5; 23];
        let right = [-0.25; 23];
        let mut convolver = StereoPartitionedConvolver::new(&left, &right, 8).unwrap();
        let mut out_left = [0.0; 8];
        let mut out_right = [0.0; 8];
        convolver
            .process_block(&[1.0; 8], &mut out_left, &mut out_right)
            .unwrap();
        convolver.reset();
        out_left.fill(0.0);
        out_right.fill(0.0);
        convolver
            .process_block(&[0.0; 8], &mut out_left, &mut out_right)
            .unwrap();
        assert_eq!(out_left, [0.0; 8]);
        assert_eq!(out_right, [0.0; 8]);
        convolver
            .process_block(
                &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                &mut out_left,
                &mut out_right,
            )
            .unwrap();
        assert!(out_left.iter().all(|sample| (*sample - 0.5).abs() < 1e-5));
        assert!(out_right.iter().all(|sample| (*sample + 0.25).abs() < 1e-5));
    }
}
