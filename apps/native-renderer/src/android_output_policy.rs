//! Hardware burst budgets; no DSP, gain changes, CPU models or vendor IDs.
pub(crate) struct BufferPolicy {
    burst: usize,
    capacity: usize,
    current: usize,
    xruns: Option<u32>,
}
impl BufferPolicy {
    pub fn new(burst: usize, capacity: usize, current: usize) -> Self {
        Self {
            burst: burst.max(1),
            capacity: capacity.max(1),
            current,
            xruns: None,
        }
    }
    pub fn initial(&self) -> usize {
        self.burst.saturating_mul(4).min(self.capacity)
    }
    pub fn accepted(&mut self, size: usize) {
        self.current = size.min(self.capacity);
    }
    pub fn observe_xruns(&mut self, count: i32) -> Option<usize> {
        if count < 0 {
            return None;
        }
        let count = count as u32;
        let previous = self.xruns.replace(count)?;
        let limit = self.burst.saturating_mul(8).min(self.capacity);
        if count > previous && self.current < limit {
            Some((self.current + self.burst).min(limit))
        } else {
            None
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_and_large_vendor_bursts_remain_within_granted_capacity() {
        for burst in [96, 192, 240, 256, 384, 512, 1024, 1536] {
            for capacity in [1024, 2048, 4096] {
                let mut p = BufferPolicy::new(burst, capacity, 0);
                assert!(p.initial() <= capacity);
                p.accepted(p.initial());
                assert_eq!(p.observe_xruns(0), None);
                for xrun in 1..100 {
                    if let Some(n) = p.observe_xruns(xrun) {
                        assert!(n <= capacity && n <= burst * 8);
                        p.accepted(n);
                    }
                }
                assert_eq!(p.current, (burst * 8).min(capacity));
                assert_eq!(p.observe_xruns(100), None);
            }
        }
    }
    #[test]
    fn steady_or_unsupported_counters_never_resize_and_a_reset_is_handled() {
        let mut p = BufferPolicy::new(192, 4096, 768);
        assert_eq!(p.observe_xruns(-1), None);
        assert_eq!(p.observe_xruns(7), None);
        assert_eq!(p.observe_xruns(7), None);
        assert_eq!(p.observe_xruns(0), None);
        assert_eq!(p.observe_xruns(1), Some(960));
    }
}
