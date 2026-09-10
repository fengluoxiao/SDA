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
    history_frames: usize,
    pending: Option<Box<Self>>,
    queued: Option<Box<Self>>,
    transition_frame: usize,
    fade_frames: usize,
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
            history_frames: left.len().max(right.len()),
            pending: None,
            queued: None,
            transition_frame: 0,
            fade_frames: 2400,
        })
    }

    /// Queue the latest selection; never discard an audible convolution history.
    pub(super) fn transition_to(&mut self, mut next: Self, sample_rate: u32) {
        next.fade_frames = (sample_rate as usize / 20).max(1);
        self.queued = Some(Box::new(next));
    }

    pub(super) fn begin_block(&mut self) {
        if self.pending.is_none() {
            self.pending = self.queued.take();
            self.transition_frame = 0;
        }
        if let Some(next) = &mut self.pending { next.begin_block(); }
        self.input_left.fill(0.0);
        self.input_right.fill(0.0);
    }

    pub(super) fn add(&mut self, frame: usize, input: [f32; 2]) {
        if let Some(next) = &mut self.pending { next.add(frame, input); }
        self.input_left[frame] = input[0];
        self.input_right[frame] = input[1];
    }

    pub(super) fn output_at(&self, frame: usize) -> [f32; 2] {
        let current = [
            self.output_left[frame] * self.preamp,
            self.output_right[frame] * self.preamp,
        ];
        if let Some(next) = &self.pending {
            // Include the output block latency before exposing the new FIR.
            let warmup = next.history_frames + convolution::DEFAULT_PARTITION;
            let mix = (self.transition_frame + frame).saturating_sub(warmup) as f32
                / next.fade_frames as f32;
            let mix = mix.min(1.0);
            let target = next.output_at(frame);
            return std::array::from_fn(|ear| current[ear] * (1.0 - mix) + target[ear] * mix);
        }
        current
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
        if let Some(next) = &mut self.pending {
            next.finish_block()?;
            self.transition_frame += convolution::DEFAULT_PARTITION;
            if self.transition_frame >= next.history_frames + convolution::DEFAULT_PARTITION + next.fade_frames {
                let queued = self.queued.take();
                *self = *self.pending.take().unwrap();
                self.queued = queued;
            }
        }
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        // A seek resets history but preserves the latest requested profile.
        if let Some(next) = self.queued.take().or_else(|| self.pending.take()) {
            *self = *next;
        }
        self.pending = None;
        self.transition_frame = 0;
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
    fn switching_headphone_profiles_is_continuous_and_latest_selection_wins() {
        let mut compensation = HeadphoneCompensation::bypass().unwrap();
        let mut previous = 1.0_f32;
        for block in 0..100 {
            if block == 4 {
                // A delayed FIR must fill its history before it becomes audible.
                let mut fir = vec![0.0; 8192];
                fir[8191] = 0.25;
                compensation.transition_to(HeadphoneCompensation::new(&fir, &fir, 1.0).unwrap(), 48000);
            }
            if block == 5 {
                compensation.transition_to(HeadphoneCompensation::new(&[0.5, 0.0], &[0.5, 0.0], 1.0).unwrap(), 48000);
                compensation.transition_to(HeadphoneCompensation::bypass().unwrap(), 48000);
            }
            compensation.begin_block();
            for frame in 0..convolution::DEFAULT_PARTITION {
                let output = compensation.output_at(frame);
                if block > 0 {
                    assert!((output[0] - previous).abs() < 0.001, "discontinuity at {block}:{frame}");
                    assert!((output[0] - output[1]).abs() < 1e-5);
                    previous = output[0];
                }
                compensation.add(frame, [1.0, 1.0]);
            }
            compensation.finish_block().unwrap();
        }
        assert!(compensation.pending.is_none() && compensation.queued.is_none());
        assert!((previous - 1.0).abs() < 1e-5);
    }

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
