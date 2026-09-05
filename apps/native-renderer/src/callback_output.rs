//! Callback-owned recovery for a temporarily empty rendered-audio FIFO.

use crate::stereo_fifo::StereoFifo;

pub(super) struct CallbackOutput {
    previous: [f32; 2],
    fade_out_start: [f32; 2],
    fade_frames: usize,
    refill_frames: usize,
    fade_out_remaining: usize,
    fade_in_remaining: usize,
    recovering: bool,
}

impl CallbackOutput {
    pub(super) fn new(sample_rate: u32, refill_frames: usize) -> Self {
        Self {
            previous: [0.0; 2],
            fade_out_start: [0.0; 2],
            fade_frames: (sample_rate as usize / 500).max(1),
            refill_frames,
            fade_out_remaining: 0,
            fade_in_remaining: 0,
            recovering: false,
        }
    }

    pub(super) fn reset(&mut self) {
        self.previous = [0.0; 2];
        self.fade_out_start = [0.0; 2];
        self.fade_out_remaining = 0;
        self.fade_in_remaining = 0;
        self.recovering = false;
    }

    fn recovery_frame(&mut self) -> [f32; 2] {
        if self.fade_out_remaining == 0 {
            self.previous = [0.0; 2];
        } else {
            self.fade_out_remaining -= 1;
            let gain = self.fade_out_remaining as f32 / self.fade_frames as f32;
            self.previous = [self.fade_out_start[0] * gain, self.fade_out_start[1] * gain];
        }
        self.previous
    }

    /// The caller applies pending FIFO flushes and resets this state first.
    /// Only returned, real FIFO frames advance the codec clock; recovery output
    /// is synthetic and leaves all queued program samples intact.
    pub(super) fn fill(
        &mut self,
        fifo: &StereoFifo,
        output_enabled: bool,
        requested: usize,
        mut write: impl FnMut(usize, [f32; 2]),
    ) -> usize {
        if !output_enabled {
            self.reset();
            for offset in 0..requested {
                write(offset, [0.0; 2]);
            }
            return 0;
        }

        if self.recovering {
            if self.fade_out_remaining == 0 && fifo.available_read() >= self.refill_frames {
                self.recovering = false;
                self.fade_in_remaining = self.fade_frames;
            } else {
                for offset in 0..requested {
                    write(offset, self.recovery_frame());
                }
                return 0;
            }
        }

        let popped = fifo.pop_frames(requested, |offset, mut frame| {
            if self.fade_in_remaining > 0 {
                self.fade_in_remaining -= 1;
                let gain = 1.0 - self.fade_in_remaining as f32 / self.fade_frames as f32;
                frame[0] *= gain;
                frame[1] *= gain;
            }
            self.previous = frame;
            write(offset, frame);
        });
        if popped < requested {
            self.recovering = true;
            self.fade_in_remaining = 0;
            self.fade_out_start = self.previous;
            self.fade_out_remaining = self.fade_frames;
            for offset in popped..requested {
                write(offset, self.recovery_frame());
            }
        }
        popped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(
        output: &mut CallbackOutput,
        fifo: &StereoFifo,
        enabled: bool,
        frames: usize,
    ) -> (usize, Vec<[f32; 2]>) {
        let mut result = vec![[9.0; 2]; frames];
        if fifo.apply_flush_from_consumer() {
            output.reset();
        }
        let popped = output.fill(fifo, enabled, frames, |offset, frame| {
            result[offset] = frame
        });
        (popped, result)
    }

    #[test]
    fn normal_output_is_unchanged() {
        let fifo = StereoFifo::new(16);
        let mut output = CallbackOutput::new(2_000, 8);
        let samples = [0.2, -0.4, 0.8, -0.1, -0.6, 0.3, 0.0, 0.5];
        assert_eq!(fifo.push(&samples), 4);
        let (popped, frames) = fill(&mut output, &fifo, true, 4);
        assert_eq!(popped, 4);
        assert_eq!(frames.into_iter().flatten().collect::<Vec<_>>(), samples);
        assert_eq!(fifo.available_read(), 0);
    }

    #[test]
    fn partial_underrun_fades_across_callbacks_without_consuming_refill() {
        let fifo = StereoFifo::new(16);
        let mut output = CallbackOutput::new(2_000, 8);
        assert_eq!(fifo.push(&[0.8, -0.4]), 1);
        let (popped, frames) = fill(&mut output, &fifo, true, 2);
        assert_eq!(popped, 1);
        assert_eq!(frames, [[0.8, -0.4], [0.6, -0.3]]);
        assert_eq!(fifo.push(&[0.4, -0.2, 0.5, -0.25]), 2);
        let (popped, frames) = fill(&mut output, &fifo, true, 2);
        assert_eq!(popped, 0);
        assert_eq!(frames, [[0.4, -0.2], [0.2, -0.1]]);
        let (popped, frames) = fill(&mut output, &fifo, true, 2);
        assert_eq!(popped, 0);
        assert_eq!(frames, [[0.0; 2]; 2]);
        assert_eq!(fifo.available_read(), 2);
    }

    #[test]
    fn entirely_empty_callback_fades_from_previous_callback() {
        let fifo = StereoFifo::new(16);
        let mut output = CallbackOutput::new(2_000, 8);
        assert_eq!(fifo.push(&[0.8, -0.4]), 1);
        assert_eq!(fill(&mut output, &fifo, true, 1).0, 1);
        let (popped, frames) = fill(&mut output, &fifo, true, 6);
        assert_eq!(popped, 0);
        assert_eq!(
            frames,
            [
                [0.6, -0.3],
                [0.4, -0.2],
                [0.2, -0.1],
                [0.0; 2],
                [0.0; 2],
                [0.0; 2]
            ]
        );
    }

    #[test]
    fn burst_refill_waits_for_watermark_and_preserves_all_sample_order() {
        let fifo = StereoFifo::new(16);
        let mut output = CallbackOutput::new(2_000, 8);
        assert_eq!(fill(&mut output, &fifo, true, 4).0, 0);
        for value in 1..8 {
            assert_eq!(fifo.push(&[value as f32, -(value as f32)]), 1);
            assert_eq!(fill(&mut output, &fifo, true, 4), (0, vec![[0.0; 2]; 4]));
            assert_eq!(fifo.available_read(), value);
        }
        assert_eq!(fifo.push(&[8.0, -8.0]), 1);
        let (first_count, first) = fill(&mut output, &fifo, true, 3);
        assert_eq!(first_count, 3);
        assert_eq!(first, [[0.25, -0.25], [1.0, -1.0], [2.25, -2.25]]);
        let (second_count, second) = fill(&mut output, &fifo, true, 5);
        assert_eq!(second_count, 5);
        assert_eq!(
            second,
            [
                [4.0, -4.0],
                [5.0, -5.0],
                [6.0, -6.0],
                [7.0, -7.0],
                [8.0, -8.0]
            ]
        );
        assert_eq!(first_count + second_count, 8);
        assert_eq!(fifo.available_read(), 0);
    }

    #[test]
    fn disabled_output_and_flush_clear_recovery_tail() {
        let fifo = StereoFifo::new(16);
        let mut output = CallbackOutput::new(2_000, 8);
        assert_eq!(fifo.push(&[0.8, -0.4]), 1);
        assert_eq!(fill(&mut output, &fifo, true, 2).0, 1);
        assert_eq!(fill(&mut output, &fifo, false, 2), (0, vec![[0.0; 2]; 2]));
        assert_eq!(fifo.push(&[0.4, -0.2]), 1);
        assert_eq!(fill(&mut output, &fifo, true, 2).0, 1);
        let epoch = fifo.clear_from_producer();
        assert_eq!(fill(&mut output, &fifo, true, 2), (0, vec![[0.0; 2]; 2]));
        assert!(fifo.flush_acknowledged(epoch));
    }
}
