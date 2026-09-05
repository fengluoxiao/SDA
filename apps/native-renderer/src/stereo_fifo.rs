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
    /// Producer requests an empty boundary; only the consumer advances `read`.
    flush_before: AtomicUsize,
    flush_epoch: AtomicUsize,
    flush_ack_epoch: AtomicUsize,
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
            flush_before: AtomicUsize::new(0),
            flush_epoch: AtomicUsize::new(0),
            flush_ack_epoch: AtomicUsize::new(0),
        }
    }

    fn effective_read(&self, read: usize, write: usize) -> usize {
        let flush_before = self.flush_before.load(Ordering::Acquire);
        if flush_before.wrapping_sub(read) <= self.capacity
            && write.wrapping_sub(flush_before) <= self.capacity
        {
            flush_before
        } else {
            read
        }
    }

    pub fn available_read(&self) -> usize {
        let write = self.write.load(Ordering::Acquire);
        let read = self.effective_read(self.read.load(Ordering::Acquire), write);
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

    /// Consumer-owned application of a pending producer flush request.
    pub fn apply_flush_from_consumer(&self) -> bool {
        // Read the published epoch before its boundary, so a concurrent request
        // cannot be acknowledged without applying the corresponding boundary.
        let epoch = self.flush_epoch.load(Ordering::Acquire);
        self.apply_flush_epoch_from_consumer(epoch)
    }

    fn apply_flush_epoch_from_consumer(&self, epoch: usize) -> bool {
        let changed = self.flush_ack_epoch.load(Ordering::Relaxed) != epoch;
        let write = self.write.load(Ordering::Acquire);
        let previous_read = self.read.load(Ordering::Relaxed);
        let read = self.effective_read(previous_read, write);
        self.read.store(read, Ordering::Release);
        self.flush_ack_epoch
            .store(epoch, Ordering::Release);
        changed || read != previous_read
    }

    /// Single consumer only, after applying pending flushes at callback entry.
    pub(super) fn pop_frames(&self, requested: usize, mut write: impl FnMut(usize, [f32; 2])) -> usize {
        let read = self.read.load(Ordering::Relaxed);
        let write_cursor = self.write.load(Ordering::Acquire);
        let count = requested.min(write_cursor.wrapping_sub(read).min(self.capacity));
        for offset in 0..count {
            let index = (read.wrapping_add(offset)) % self.capacity;
            write(offset, unsafe { *self.frames[index].0.get() });
        }
        self.read.store(read.wrapping_add(count), Ordering::Release);
        count
    }

    /// Single consumer only. Missing frames are written as silence.
    pub fn pop_into_f32(&self, output: &mut [f32], channels: usize) -> usize {
        self.apply_flush_from_consumer();
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
        self.apply_flush_from_consumer();
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
        self.apply_flush_from_consumer();
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

    /// Producer-side flush request. The callback owns the read cursor and moves
    /// it to this boundary before its next pop, so reset/start cannot race a stale
    /// callback store and resurrect pre-reset frames.
    pub fn clear_from_producer(&self) -> usize {
        let write = self.write.load(Ordering::Acquire);
        self.flush_before.store(write, Ordering::Release);
        self.flush_epoch
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    pub fn flush_acknowledged(&self, epoch: usize) -> bool {
        self.flush_ack_epoch.load(Ordering::Acquire) >= epoch
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

    #[test]
    fn producer_flush_is_applied_only_by_consumer() {
        let fifo = StereoFifo::new(4);
        assert_eq!(fifo.push(&[1.0, 10.0, 2.0, 20.0]), 2);
        let flush = fifo.clear_from_producer();
        assert!(!fifo.flush_acknowledged(flush));
        assert_eq!(fifo.read.load(Ordering::Acquire), 0);
        let mut output = [9.0; 4];
        assert_eq!(fifo.pop_into_f32(&mut output, 2), 0);
        assert_eq!(output, [0.0; 4]);
        assert_eq!(fifo.read.load(Ordering::Acquire), 2);
        assert!(fifo.flush_acknowledged(flush));
    }

    #[test]
    fn flush_requested_after_epoch_snapshot_is_not_acknowledged_early() {
        let fifo = StereoFifo::new(4);
        assert_eq!(fifo.push(&[1.0, 10.0, 2.0, 20.0]), 2);
        let observed_epoch = fifo.flush_epoch.load(Ordering::Acquire);
        let next_epoch = fifo.clear_from_producer();
        assert!(fifo.apply_flush_epoch_from_consumer(observed_epoch));
        assert!(!fifo.flush_acknowledged(next_epoch));
        assert!(fifo.apply_flush_from_consumer());
        assert!(fifo.flush_acknowledged(next_epoch));
        assert!(!fifo.apply_flush_from_consumer());
        assert_eq!(fifo.available_read(), 0);
    }
}
