//! Fixed-capacity SPSC stereo frame FIFO between the render worker and WASAPI.
//!
//! The producer is the render worker and the consumer is CPAL's audio callback.
//! Both operations are bounded and allocation-free after construction.

use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

struct FrameCell(UnsafeCell<[f32; 2]>);

unsafe impl Sync for FrameCell {}

pub struct StereoFifo {
    frames: Box<[FrameCell]>,
    capacity: usize,
    read: AtomicUsize,
    write: AtomicUsize,
}

impl StereoFifo {
    pub fn new(capacity_frames: usize) -> Self {
        assert!(capacity_frames >= 2);
        let frames = (0..capacity_frames)
            .map(|_| FrameCell(UnsafeCell::new([0.0, 0.0])))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            frames,
            capacity: capacity_frames,
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }

    pub fn available_read(&self) -> usize {
        let write = self.write.load(Ordering::Acquire);
        let read = self.read.load(Ordering::Acquire);
        write.wrapping_sub(read).min(self.capacity)
    }

    pub fn available_write(&self) -> usize {
        self.capacity.saturating_sub(self.available_read())
    }

    /// Single producer only. Returns the number of whole stereo frames written.
    pub fn push(&self, interleaved: &[f32]) -> usize {
        let requested = interleaved.len() / 2;
        let count = requested.min(self.available_write());
        if count == 0 {
            return 0;
        }
        let write = self.write.load(Ordering::Relaxed);
        for offset in 0..count {
            let index = (write.wrapping_add(offset)) % self.capacity;
            unsafe {
                *self.frames[index].0.get() =
                    [interleaved[offset * 2], interleaved[offset * 2 + 1]];
            }
        }
        self.write
            .store(write.wrapping_add(count), Ordering::Release);
        count
    }

    fn pop_frames(&self, requested: usize, mut write: impl FnMut(usize, [f32; 2])) -> usize {
        let count = requested.min(self.available_read());
        let read = self.read.load(Ordering::Relaxed);
        for offset in 0..count {
            let index = (read.wrapping_add(offset)) % self.capacity;
            write(offset, unsafe { *self.frames[index].0.get() });
        }
        self.read.store(read.wrapping_add(count), Ordering::Release);
        count
    }

    /// Single consumer only. Missing frames are written as silence.
    pub fn pop_into_f32(&self, output: &mut [f32], channels: usize) -> usize {
        let requested = output.len() / channels;
        let count = self.pop_frames(requested, |offset, frame| {
            let target = &mut output[offset * channels..(offset + 1) * channels];
            target.fill(0.0);
            target[0] = frame[0];
            if channels > 1 {
                target[1] = frame[1];
            }
        });
        for target in output[count * channels..].chunks_exact_mut(channels) {
            target.fill(0.0);
        }
        count
    }

    pub fn pop_into_i16(&self, output: &mut [i16], channels: usize) -> usize {
        let requested = output.len() / channels;
        let count = self.pop_frames(requested, |offset, frame| {
            let target = &mut output[offset * channels..(offset + 1) * channels];
            target.fill(0);
            target[0] = (frame[0].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            if channels > 1 {
                target[1] = (frame[1].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            }
        });
        for target in output[count * channels..].chunks_exact_mut(channels) {
            target.fill(0);
        }
        count
    }

    pub fn pop_into_u16(&self, output: &mut [u16], channels: usize) -> usize {
        let requested = output.len() / channels;
        let count = self.pop_frames(requested, |offset, frame| {
            let target = &mut output[offset * channels..(offset + 1) * channels];
            target.fill(u16::MAX / 2);
            target[0] = ((frame[0].clamp(-1.0, 1.0) + 1.0) * 0.5 * u16::MAX as f32) as u16;
            if channels > 1 {
                target[1] = ((frame[1].clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
            }
        });
        for target in output[count * channels..].chunks_exact_mut(channels) {
            target.fill(u16::MAX / 2);
        }
        count
    }

    /// Producer-side flush. A concurrently executing callback may consume at most
    /// one already-loaded frame; all later reads observe the new empty boundary.
    pub fn clear_from_producer(&self) {
        let write = self.write.load(Ordering::Acquire);
        self.read.store(write, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_frame_order_across_wrap() {
        let fifo = StereoFifo::new(3);
        assert_eq!(fifo.push(&[1.0, 10.0, 2.0, 20.0]), 2);
        let mut first = [0.0; 2];
        assert_eq!(fifo.pop_into_f32(&mut first, 2), 1);
        assert_eq!(first, [1.0, 10.0]);
        assert_eq!(fifo.push(&[3.0, 30.0, 4.0, 40.0]), 2);
        let mut all = [0.0; 6];
        assert_eq!(fifo.pop_into_f32(&mut all, 2), 3);
        assert_eq!(all, [2.0, 20.0, 3.0, 30.0, 4.0, 40.0]);
    }

    #[test]
    fn refuses_overflow_and_silences_missing_frames() {
        let fifo = StereoFifo::new(2);
        assert_eq!(fifo.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 2);
        let mut output = [9.0; 6];
        assert_eq!(fifo.pop_into_f32(&mut output, 2), 2);
        assert_eq!(output, [1.0, 2.0, 3.0, 4.0, 0.0, 0.0]);
    }
}
