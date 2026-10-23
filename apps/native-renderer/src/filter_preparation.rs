//! Bounded latest-target mailboxes for non-rendering filter preparation.
use std::sync::{Arc, Mutex, OnceLock, mpsc};

type Job = Arc<dyn Fn() + Send + Sync>;
fn executor() -> &'static mpsc::SyncSender<Job> {
    static EXECUTOR: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();
    EXECUTOR.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<Job>(128);
        std::thread::Builder::new().name("sda-filter-prepare".into()).spawn(move || {
            while let Ok(job) = receiver.recv() { job(); }
        }).expect("filter preparation worker");
        sender
    })
}
struct State<T, R> {
    target: Option<(u64, T, Arc<dyn Fn(T) -> R + Send + Sync>, std::time::Instant)>,
    result: Option<(u64, R, std::time::Instant)>,
    running: bool,
    version: u64,
}
pub(crate) struct LatestPreparation<T, R> {
    state: Arc<Mutex<State<T, R>>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    runner: OnceLock<Job>,
}
impl<T: Send + 'static, R: Send + 'static> LatestPreparation<T, R> {
    pub fn new() -> Self {
        Self { state: Arc::new(Mutex::new(State { target: None, result: None, running: false, version: 0 })), cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)), runner: OnceLock::new() }
    }
    pub fn warm(&self) {
        let _ = executor();
        let _ = self.runner();
    }
    fn runner(&self) -> &Job {
        self.runner.get_or_init(|| {
            let shared = self.state.clone();
            let cancelled = self.cancelled.clone();
            let recursive = Arc::new(OnceLock::<std::sync::Weak<dyn Fn() + Send + Sync>>::new());
            let weak_runner = recursive.clone();
            let runner: Job = Arc::new(move || {
                if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                    let mut state = shared.lock().unwrap(); state.target = None; state.running = false; return;
                }
                let target = {
                    let mut state = shared.lock().unwrap();
                    match state.target.take() {
                        Some(target) => target,
                        None => { state.running = false; return; }
                    }
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (target.2)(target.1)));
                let mut state = shared.lock().unwrap();
                if let Ok(result) = result {
                    if !cancelled.load(std::sync::atomic::Ordering::Acquire) { state.result = Some((target.0, result, target.3)); }
                }
                state.running = false;
                // One preparation per dispatch, then go to the back of the
                // bounded queue so a moving source cannot monopolize the worker.
                if state.target.is_some() && !cancelled.load(std::sync::atomic::Ordering::Acquire) {
                    if let Some(runner) = weak_runner.get().and_then(std::sync::Weak::upgrade) {
                        state.running = true;
                        if executor().try_send(runner).is_err() { state.running = false; }
                    }
                }
            });
            let _ = recursive.set(Arc::downgrade(&runner));
            runner
        })
    }
    pub fn submit(&self, target: T, prepare: Arc<dyn Fn(T) -> R + Send + Sync>) -> Option<u64> {
        if self.cancelled.load(std::sync::atomic::Ordering::Acquire) { return None; }
        let mut state = self.state.try_lock().ok()?;
        state.version += 1;
        let version = state.version;
        state.target = Some((version, target, prepare, std::time::Instant::now()));
        // Leave obsolete prepared data for the preparation worker to replace
        // and destroy, instead of freeing its spectral arrays on this thread.
        if !state.running {
            state.running = true;
            let job = self.runner();
            if executor().try_send(job.clone()).is_err() { state.running = false; return None; }
        }
        Some(version)
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn take(&self) -> Option<(u64, R)> {
        if self.cancelled.load(std::sync::atomic::Ordering::Acquire) { return None; }
        let mut state = self.state.try_lock().ok()?;
        if state.result.as_ref().is_some_and(|(version, _, _)| *version == state.version) {
            state.result.take().map(|(version, result, _)| (version, result))
        } else { None }
    }
    pub fn take_recent(&self, after: u64, max_age: std::time::Duration) -> Option<(u64, R)> {
        if self.cancelled.load(std::sync::atomic::Ordering::Acquire) { return None; }
        let mut state = self.state.try_lock().ok()?;
        if state.result.as_ref().is_some_and(|(version, _, submitted)| *version > after && submitted.elapsed() <= max_age) {
            state.result.take().map(|(version, result, _)| (version, result))
        } else { None }
    }
    pub fn needs_retry(&self, max_age: std::time::Duration) -> bool {
        self.state.try_lock().is_ok_and(|state| !state.running &&
            state.result.as_ref().is_none_or(|(version, _, submitted)| *version != state.version || submitted.elapsed() > max_age))
    }
    pub fn idle(&self) -> bool {
        self.state.try_lock().is_ok_and(|state| !state.running &&
            state.result.as_ref().is_none_or(|(version, _, _)| *version != state.version))
    }
}
impl<T, R> Drop for LatestPreparation<T, R> {
    fn drop(&mut self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_results_advance_monotonically_but_expired_results_are_rejected() {
        let mailbox = LatestPreparation::<u64,u64>::new();
        {
            let mut state = mailbox.state.lock().unwrap();
            state.version = 3;
            state.result = Some((2, 20_u64, std::time::Instant::now()));
        }
        assert_eq!(mailbox.take_recent(1,std::time::Duration::from_secs(1)),Some((2,20)));
        {
            let mut state = mailbox.state.lock().unwrap();
            state.result = Some((1,10,std::time::Instant::now()));
        }
        assert!(mailbox.take_recent(2,std::time::Duration::from_secs(1)).is_none());
        {
            let mut state = mailbox.state.lock().unwrap();
            state.result = Some((3,30,std::time::Instant::now()-std::time::Duration::from_secs(1)));
        }
        assert!(mailbox.take_recent(2,std::time::Duration::from_millis(50)).is_none());
        assert!(mailbox.needs_retry(std::time::Duration::from_millis(50)));
    }
    #[test]
    fn blocked_preparation_coalesces_a_burst_to_one_latest_target() {
        let mailbox = LatestPreparation::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let processed = Arc::new(Mutex::new(Vec::new()));
        let worker_processed = processed.clone();
        let prepare = Arc::new(move |value: u64| {
            worker_processed.lock().unwrap().push(value);
            if value == 0 { started_tx.send(()).unwrap(); release_rx.lock().unwrap().recv().unwrap(); }
            value
        });
        mailbox.submit(0, prepare.clone()).unwrap();
        started_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        for value in 1..=100 { mailbox.submit(value, prepare.clone()).unwrap(); }
        release_tx.send(()).unwrap();
        let deadline = std::time::Instant::now()+std::time::Duration::from_secs(2);
        loop {
            if let Some((_,result)) = mailbox.take() { assert_eq!(result,100); break; }
            assert!(std::time::Instant::now()<deadline);std::thread::yield_now();
        }
        assert_eq!(*processed.lock().unwrap(),vec![0,100]);
    }
    #[test]
    fn preparation_panic_does_not_kill_executor_or_block_future_work() {
        let mailbox = LatestPreparation::new();
        mailbox.submit(1, Arc::new(|_| -> u64 { panic!("test preparation failure") })).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !mailbox.idle() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let version = mailbox.submit(2, Arc::new(|value| value)).unwrap();
        loop {
            if let Some(result) = mailbox.take() { assert_eq!(result, (version, 2)); break; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    #[ignore = "host scheduling experiment, not a deterministic correctness gate"]
    fn provider_rate_updates_make_progress_without_unbounded_backlog() {
        let mailbox = LatestPreparation::new();
        let prepare = Arc::new(|value: u64| value);
        let mut completed = 0;
        for target in 0..20 {
            let version = mailbox.submit(target, prepare.clone()).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(33);
            let mut seen = false;
            while std::time::Instant::now() < deadline {
                if let Some((received, value)) = mailbox.take() {
                    assert_eq!(received, version); assert_eq!(value, target);
                    seen = true; completed += 1; break;
                }
                std::thread::yield_now();
            }
            assert!(seen, "latest target did not complete inside provider interval");
        }
        assert_eq!(completed, 20);
    }
    #[test]
    fn cancelled_inflight_result_cannot_be_published() {
        let mailbox = LatestPreparation::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        mailbox.submit(1, Arc::new(move |value| {
            started_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap(); value
        })).unwrap();
        started_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        mailbox.cancel(); release_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !mailbox.idle() {
            assert!(mailbox.take().is_none());
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(mailbox.take().is_none());
    }
    #[test]
    fn many_sources_make_progress_without_one_job_draining_forever() {
        let mailboxes: Vec<_> = (0..16).map(|_| LatestPreparation::new()).collect();
        let prepare = Arc::new(|value: u64| value + 1);
        let versions: Vec<_> = mailboxes.iter().enumerate()
            .map(|(i,m)| m.submit(i as u64, prepare.clone()).unwrap()).collect();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut seen = vec![false; mailboxes.len()];
        while seen.iter().any(|value| !value) {
            for (i,m) in mailboxes.iter().enumerate() {
                if seen[i] { continue; }
                if let Some((version,result)) = m.take() {
                    assert_eq!(version,versions[i]); assert_eq!(result,i as u64 + 1); seen[i]=true;
                }
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    fn newer_target_replaces_pending_and_rejects_obsolete_result() {
        let mailbox = LatestPreparation::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let prepare = Arc::new(move |value| {
            if value == 1 { started_tx.send(()).unwrap(); release_rx.lock().unwrap().recv().unwrap(); }
            value * 2
        });
        mailbox.submit(1, prepare.clone()).unwrap();
        started_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let newest = mailbox.submit(3, prepare.clone()).unwrap();
        release_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(result) = mailbox.take() { assert_eq!(result, (newest, 6)); break; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
