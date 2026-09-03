//! Fixed-capacity mono PCM storage indexed by the absolute codec clock.
//!
//! Slots carry their clock tag, so blocks may arrive out of order without
//! retiming playback. The write window is bounded from the engine clock to
//! avoid a future block aliasing an unread slot after the ring wraps.

#[derive(Clone, Copy)]
struct Slot {
    clock: u64,
    sample: f32,
}

const EMPTY_CLOCK: u64 = u64::MAX;

pub(super) struct AbsolutePcmRing {
    slots: Vec<Slot>,
}

impl AbsolutePcmRing {
    pub(super) fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            slots: vec![
                Slot {
                    clock: EMPTY_CLOCK,
                    sample: 0.0
                };
                capacity
            ],
        }
    }

    pub(super) fn clear(&mut self) {
        for slot in &mut self.slots {
            slot.clock = EMPTY_CLOCK;
        }
    }

    /// Returns whether this whole block fits in the bounded future window.
    /// Samples before `now` are intentionally accepted (and discarded): as with
    /// the old sparse store, late PCM cannot be rendered retroactively.
    pub(super) fn can_write(&self, now: u64, start: u64, samples: usize) -> bool {
        let Some(end) = start.checked_add(samples as u64) else {
            return false;
        };
        if samples == 0 {
            return true;
        }
        let capacity = self.slots.len() as u64;
        if samples as u64 > capacity {
            return false;
        }
        let future_start = start.max(now);
        let future_len = end.saturating_sub(future_start);
        future_len <= capacity
            && future_start
                .checked_sub(now)
                .is_some_and(|offset| offset <= capacity - future_len)
    }

    /// Writes a block after `can_write` has succeeded. This operation cannot
    /// allocate and only changes slots in the current absolute-clock window.
    pub(super) fn write(&mut self, now: u64, start: u64, samples: &[f32]) {
        debug_assert!(self.can_write(now, start, samples.len()));
        let capacity = self.slots.len();
        for (offset, &sample) in samples.iter().enumerate() {
            let Some(clock) = start.checked_add(offset as u64) else {
                break;
            };
            if clock < now {
                continue;
            }
            let slot = &mut self.slots[(clock % capacity as u64) as usize];
            slot.clock = clock;
            slot.sample = sample;
        }
    }

    /// Whether the ring currently holds any unplayed samples in its future window.
    pub(super) fn has_any(&self) -> bool {
        self.slots.iter().any(|slot| slot.clock != EMPTY_CLOCK)
    }

    pub(super) fn has_at(&self, clock: u64) -> bool {
        self.slots[(clock % self.slots.len() as u64) as usize].clock == clock
    }

    /// Consumes exactly one absolute-clock sample.
    pub(super) fn take(&mut self, clock: u64) -> Option<f32> {
        let index = (clock % self.slots.len() as u64) as usize;
        let slot = &mut self.slots[index];
        (slot.clock == clock).then(|| {
            slot.clock = EMPTY_CLOCK;
            slot.sample
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_fetched_by_absolute_clock() {
        let mut ring = AbsolutePcmRing::new(4);
        assert!(ring.can_write(10, 12, 2));
        ring.write(10, 12, &[0.25, 0.5]);
        assert_eq!(ring.take(10), None);
        assert_eq!(ring.take(12), Some(0.25));
        assert_eq!(ring.take(13), Some(0.5));
    }

    #[test]
    fn bounded_future_window_prevents_wrap_aliasing() {
        let ring = AbsolutePcmRing::new(4);
        assert!(ring.can_write(20, 20, 4));
        assert!(!ring.can_write(20, 24, 1));
        assert!(!ring.can_write(20, 23, 2));
    }

    #[test]
    fn late_samples_are_ignored_without_retiming_future_pcm() {
        let mut ring = AbsolutePcmRing::new(4);
        assert!(ring.can_write(10, 8, 4));
        ring.write(10, 8, &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(ring.take(10), Some(3.0));
        assert_eq!(ring.take(11), Some(4.0));
    }
}
