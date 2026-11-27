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
    identity: bool,
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
            identity: left[0] == 1. && right[0] == 1.
                && left[1..].iter().chain(&right[1..]).all(|v| *v == 0.),
            pending: None,
            queued: None,
            transition_frame: 0,
            fade_frames: 2400,
        })
    }

    /// Queue the latest selection; never discard an audible convolution history.
    pub(super) fn transition_to(&mut self, mut next: Self, sample_rate: u32) {
        let selected = self
            .queued
            .as_deref()
            .or(self.pending.as_deref())
            .unwrap_or(self);
        if selected.preamp == next.preamp
            && selected.left.same_static_response(&next.left)
            && selected.right.same_static_response(&next.right)
        {
            return;
        }
        next.fade_frames = (sample_rate as usize / 20).max(1);
        self.queued = Some(Box::new(next));
    }

    pub(super) fn begin_block(&mut self) {
        // A superseded filter still warming up has never been audible. Start
        // the latest choice directly instead of first playing that stale EQ.
        // Once a fade has become audible, retain its convolution history.
        let replace_warming = self.queued.is_some()
            && self.pending.as_ref().is_some_and(|next| {
                self.transition_frame <= next.history_frames + convolution::DEFAULT_PARTITION
            });
        if self.pending.is_none() || replace_warming {
            self.pending = self.queued.take();
            self.transition_frame = 0;
        }
        if let Some(next) = &mut self.pending {
            next.begin_block();
        }
        self.input_left.fill(0.0);
        self.input_right.fill(0.0);
    }

    pub(super) fn add(&mut self, frame: usize, input: [f32; 2]) {
        if let Some(next) = &mut self.pending {
            next.add(frame, input);
        }
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
        if self.identity {
            // The output block is still published at the same partition
            // boundary. A bypass profile needs the delay, not four identity FFTs.
            self.output_left.copy_from_slice(&self.input_left);
            self.output_right.copy_from_slice(&self.input_right);
        } else {
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
        }
        if let Some(next) = &mut self.pending {
            next.finish_block()?;
            self.transition_frame += convolution::DEFAULT_PARTITION;
            if self.transition_frame
                >= next.history_frames + convolution::DEFAULT_PARTITION + next.fade_frames
            {
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
    fn repeated_same_headphone_selection_does_not_restart_warmup() {
        let mut p = HeadphoneCompensation::bypass().unwrap();
        let mut fir = vec![0.0; 8192];
        fir[8191] = 0.25;
        let mut last = 0.0;
        for _ in 0..24 {
            p.transition_to(HeadphoneCompensation::new(&fir, &fir, 1.0).unwrap(), 48000);
            p.begin_block();
            for i in 0..convolution::DEFAULT_PARTITION {
                last = p.output_at(i)[0];
                p.add(i, [1.0; 2]);
            }
            p.finish_block().unwrap();
        }
        assert!(
            (last - 0.25).abs() < 1e-5,
            "repeated identical settings starved profile warmup: {last}"
        );
        assert!(p.pending.is_none() && p.queued.is_none());
    }
    #[test]
    fn latest_headphone_choice_replaces_a_profile_that_has_not_become_audible() {
        let mut processor = HeadphoneCompensation::bypass().unwrap();
        let mut long = vec![0.0; 32768];
        long[32767] = 0.25;
        processor.transition_to(
            HeadphoneCompensation::new(&long, &long, 1.0).unwrap(),
            48000,
        );
        let mut previous = 1.0_f32;
        for block in 0..12 {
            if block == 1 {
                processor.transition_to(
                    HeadphoneCompensation::new(&[0.5, 0.0], &[0.5, 0.0], 1.0).unwrap(),
                    48000,
                );
            }
            processor.begin_block();
            for i in 0..convolution::DEFAULT_PARTITION {
                let y = processor.output_at(i);
                if block > 0 {
                    assert!((y[0] - previous).abs() < 0.001);
                    previous = y[0];
                }
                assert!((y[0] - y[1]).abs() < 1e-6);
                processor.add(i, [1.0; 2]);
            }
            processor.finish_block().unwrap();
        }
        assert!(
            (previous - 0.5).abs() < 1e-5,
            "latest selection still waiting behind an inaudible stale profile: {previous}"
        );
        assert!(processor.pending.is_none() && processor.queued.is_none());
    }

    #[test]
    fn audible_headphone_transition_keeps_history_until_the_next_choice_is_ready() {
        let mut p = HeadphoneCompensation::bypass().unwrap();
        p.transition_to(
            HeadphoneCompensation::new(&[0.25, 0.0], &[0.25, 0.0], 1.0).unwrap(),
            48000,
        );
        let mut previous = 1.0_f32;
        for block in 0..16 {
            if block == 2 {
                assert!(
                    p.transition_frame
                        > p.pending.as_ref().unwrap().history_frames
                            + convolution::DEFAULT_PARTITION
                );
                p.transition_to(
                    HeadphoneCompensation::new(&[0.8, 0.0], &[0.8, 0.0], 1.0).unwrap(),
                    48000,
                );
            }
            let elapsed = p.transition_frame;
            p.begin_block();
            if block == 2 {
                assert_eq!(p.transition_frame, elapsed);
                assert!(p.queued.is_some());
            }
            for i in 0..convolution::DEFAULT_PARTITION {
                let y = p.output_at(i);
                if block > 0 {
                    assert!((y[0] - previous).abs() < 0.001);
                    previous = y[0];
                }
                p.add(i, [1.0; 2]);
            }
            p.finish_block().unwrap();
        }
        assert!((previous - 0.8).abs() < 1e-5);
        assert!(p.pending.is_none() && p.queued.is_none());
    }

    #[test]
    fn headphone_compensation_preserves_the_authored_interaural_delay_and_polarity() {
        let mut left = vec![0.0; 16];
        left[3] = 1.0;
        let mut right = vec![0.0; 16];
        right[7] = 0.5;
        let mut p = HeadphoneCompensation::new(&left, &right, 1.0).unwrap();
        let mut output = Vec::new();
        for block in 0..3 {
            p.begin_block();
            for i in 0..convolution::DEFAULT_PARTITION {
                output.push(p.output_at(i));
                p.add(
                    i,
                    if block == 0 && i == 0 {
                        [0.25, -0.8]
                    } else {
                        [0.0; 2]
                    },
                );
            }
            p.finish_block().unwrap();
        }
        for (ear, offset, amplitude) in [(0, 3, 0.25_f32), (1, 7, -0.4_f32)] {
            for (i, frame) in output.iter().enumerate() {
                let expected = if i == convolution::DEFAULT_PARTITION + offset {
                    amplitude
                } else {
                    0.0
                };
                assert!(
                    (frame[ear] - expected).abs() < 1e-6,
                    "unexpected ear delay or crossfeed at {i}/{ear}"
                );
            }
        }
    }

    #[test]
    fn identity_fast_path_keeps_delay_and_profile_transition_pcm() {
        let mut a=HeadphoneCompensation::bypass().unwrap();let mut b=HeadphoneCompensation::bypass().unwrap();b.identity=false;
        for block in 0..30 {
            if block==8 {
                let make=||HeadphoneCompensation::new(&[0.8,0.05,0.,-0.03],&[0.7,-0.04,0.02,0.],1.).unwrap();
                a.transition_to(make(),48000);b.transition_to(make(),48000);
            }
            a.begin_block();b.begin_block();
            for i in 0..convolution::DEFAULT_PARTITION {
                let x=((block*convolution::DEFAULT_PARTITION+i)as f32*0.17).sin()*0.1;
                let ya=a.output_at(i);let yb=b.output_at(i);
                for ear in 0..2 {assert!((ya[ear]-yb[ear]).abs()<2e-7,"block={block} frame={i} ear={ear}");}
                a.add(i,[x,-x*0.3]);b.add(i,[x,-x*0.3]);
            }
            a.finish_block().unwrap();b.finish_block().unwrap();
        }
    }
    #[test]
    fn switching_headphone_profiles_is_continuous_and_latest_selection_wins() {
        let mut compensation = HeadphoneCompensation::bypass().unwrap();
        let mut previous = 1.0_f32;
        for block in 0..100 {
            if block == 4 {
                // A delayed FIR must fill its history before it becomes audible.
                let mut fir = vec![0.0; 8192];
                fir[8191] = 0.25;
                compensation
                    .transition_to(HeadphoneCompensation::new(&fir, &fir, 1.0).unwrap(), 48000);
            }
            if block == 5 {
                compensation.transition_to(
                    HeadphoneCompensation::new(&[0.5, 0.0], &[0.5, 0.0], 1.0).unwrap(),
                    48000,
                );
                compensation.transition_to(HeadphoneCompensation::bypass().unwrap(), 48000);
            }
            compensation.begin_block();
            for frame in 0..convolution::DEFAULT_PARTITION {
                let output = compensation.output_at(frame);
                if block > 0 {
                    assert!(
                        (output[0] - previous).abs() < 0.001,
                        "discontinuity at {block}:{frame}"
                    );
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
