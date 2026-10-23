//! CPU quota discovery is control-plane work, never repeated per DSP block.
use std::sync::atomic::{AtomicUsize, Ordering};
static AVAILABLE: AtomicUsize = AtomicUsize::new(0);
fn discover() -> usize {
    let n=std::thread::available_parallelism().map_or(1,usize::from).max(1);
    #[cfg(target_os="android")]
    {return crate::android_audio_cpu::dsp_capacity().map_or(n,|fast|n.min(fast));}
    #[cfg(not(target_os="android"))]
    {n}
}
fn cached_with(cache:&AtomicUsize, discover:impl FnOnce()->usize)->usize {
    let n=cache.load(Ordering::Relaxed);
    if n>0 {n} else {let n=discover().max(1);cache.store(n,Ordering::Relaxed);n}
}
pub fn available() -> usize {cached_with(&AVAILABLE,discover)}
pub fn refresh() -> usize {let n=discover();AVAILABLE.store(n,Ordering::Relaxed);n}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialized_budget_never_queries_the_os_on_repeated_reads() {
        let cache=AtomicUsize::new(0);let calls=AtomicUsize::new(0);
        assert_eq!(cached_with(&cache,||{calls.fetch_add(1,Ordering::Relaxed);8}),8);
        for _ in 0..10000 {assert_eq!(cached_with(&cache,||panic!("OS query on cached DSP read")),8);}
        cache.store(2,Ordering::Relaxed);
        assert_eq!(cached_with(&cache,||panic!("refreshed value must be used")),2);
        assert_eq!(calls.load(Ordering::Relaxed),1);
    }
}
