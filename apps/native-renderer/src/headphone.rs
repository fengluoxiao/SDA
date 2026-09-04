//! Final stereo headphone-compensation FIR path.
//!
//! Compensation stays after the binaural merge. Two fixed partitioned
//! convolvers preserve independent left/right corrections without changing the
//! number of convolvers with source count.

use crate::convolution;

pub(super) struct HeadphoneCompensation {
    left: convolution::StereoPartitionedConvolver,
    right: convolution::StereoPartitionedConvolver,
    input_left: Vec<f32>,
    input_right: Vec<f32>,
    output_left: Vec<f32>,
    output_right: Vec<f32>,
    ignored_left: Vec<f32>,
    ignored_right: Vec<f32>,
    preamp: f32,
}

impl HeadphoneCompensation {
    pub(super) fn bypass() -> Result<Self, String> {
        Self::new(&[1.0, 0.0], &[1.0, 0.0], 1.0)
    }

    pub(super) fn new(left: &[f32], right: &[f32], preamp: f32) -> Result<Self, String> {
        if left.len() < 2 || right.len() < 2 || !preamp.is_finite() || preamp <= 0.0 {
            return Err("invalid headphone compensation payload".into());
        }
        let left_zeros = vec![0.0; left.len()];
        let right_zeros = vec![0.0; right.len()];
        Ok(Self {
            left: convolution::StereoPartitionedConvolver::new(
                left,
                &left_zeros,
                convolution::DEFAULT_PARTITION,
            )?,
            right: convolution::StereoPartitionedConvolver::new(
                &right_zeros,
                right,
                convolution::DEFAULT_PARTITION,
            )?,
            input_left: vec![0.0; convolution::DEFAULT_PARTITION],
            input_right: vec![0.0; convolution::DEFAULT_PARTITION],
            output_left: vec![0.0; convolution::DEFAULT_PARTITION],
            output_right: vec![0.0; convolution::DEFAULT_PARTITION],
            ignored_left: vec![0.0; convolution::DEFAULT_PARTITION],
            ignored_right: vec![0.0; convolution::DEFAULT_PARTITION],
            preamp,
        })
    }

    pub(super) fn begin_block(&mut self) {
        self.input_left.fill(0.0);
        self.input_right.fill(0.0);
    }

    pub(super) fn add(&mut self, frame: usize, input: [f32; 2]) {
        self.input_left[frame] = input[0];
        self.input_right[frame] = input[1];
    }

    pub(super) fn output_at(&self, frame: usize) -> [f32; 2] {
        [
            self.output_left[frame] * self.preamp,
            self.output_right[frame] * self.preamp,
        ]
    }

    pub(super) fn finish_block(&mut self) -> Result<(), String> {
        self.output_left.fill(0.0);
        self.output_right.fill(0.0);
        self.ignored_left.fill(0.0);
        self.ignored_right.fill(0.0);
        self.left.process_block(
            &self.input_left,
            &mut self.output_left,
            &mut self.ignored_left,
        )?;
        self.right.process_block(
            &self.input_right,
            &mut self.ignored_right,
            &mut self.output_right,
        )?;
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
        self.input_left.fill(0.0);
        self.input_right.fill(0.0);
        self.output_left.fill(0.0);
        self.output_right.fill(0.0);
        self.ignored_left.fill(0.0);
        self.ignored_right.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_independent_ears_after_one_block_delay() {
        let mut compensation = HeadphoneCompensation::new(&[1.0, 0.0], &[0.5, 0.0], 1.0).unwrap();
        compensation.begin_block();
        compensation.add(0, [0.25, 0.8]);
        compensation.finish_block().unwrap();
        compensation.begin_block();
        let output = compensation.output_at(0);
        assert!((output[0] - 0.25).abs() < 1e-5);
        assert!((output[1] - 0.4).abs() < 1e-5);
    }
}
