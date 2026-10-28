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

/// Detect a stopped hardware callback, not silence or a DSP underrun. The
/// AAudio stream is started even while the renderer is filling its reserve.
#[derive(Default)]
pub(crate) struct CallbackWatchdog {
    last: Option<(u64, u64)>,
    seen_callback: bool,
}
impl CallbackWatchdog {
    pub fn stalled(&mut self, now_ms:u64, callbacks:u64, expected_running:bool)->bool {
        let Some((count,changed))=self.last else {
            self.last=Some((callbacks,now_ms));self.seen_callback=callbacks>0;return false;
        };
        if callbacks!=count {self.seen_callback=true;self.last=Some((callbacks,now_ms));return false;}
        if !expected_running {self.last=Some((callbacks,now_ms));return false;}
        now_ms.saturating_sub(changed)>=if self.seen_callback {2000}else{5000}
    }
}
#[cfg(test)]
mod watchdog_tests {
    use super::*;
    #[test]
    fn callback_stop_is_detected_before_the_host_fatal_deadline() {
        let mut w=CallbackWatchdog::default();assert!(!w.stalled(0,10,true));
        assert!(!w.stalled(1999,10,true));assert!(w.stalled(2000,10,true));
        assert!(!w.stalled(2001,11,true));assert!(!w.stalled(4000,11,true));
    }
    #[test]
    fn startup_has_grace_and_pause_resume_restarts_the_deadline() {
        let mut w=CallbackWatchdog::default();assert!(!w.stalled(0,0,true));
        assert!(!w.stalled(4999,0,true));assert!(w.stalled(5000,0,true));
        assert!(!w.stalled(5001,1,true));
        for now in (6000..30000).step_by(500){assert!(!w.stalled(now,1,false));}
        assert!(!w.stalled(30000,1,true));assert!(w.stalled(32000,1,true));
    }
    #[test]
    fn running_callbacks_never_reconnect_due_to_silent_or_missing_pcm() {
        let mut w=CallbackWatchdog::default();
        for tick in 0..2000 {assert!(!w.stalled(tick*50,tick,true));}
    }
}
