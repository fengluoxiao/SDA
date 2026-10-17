//! Caller-participating, bounded DSP dispatch. Not a real-time thread policy.
pub(crate) fn run<T: Send, E: Send, F>(items: &mut [T], process: F) -> Result<(), E>
where F: Fn(&mut T) -> Result<(), E> + Sync {
    let Some(pool) = crate::direct_renderer::workers().filter(|_| items.len() >= 8) else {
        return items.iter_mut().try_for_each(process);
    };
    run_on(pool, items, process)
}

fn run_on<T: Send, E: Send, F>(pool: &rayon::ThreadPool, items: &mut [T], process: F) -> Result<(), E>
where F: Fn(&mut T) -> Result<(), E> + Sync {
    let split = items.len().div_ceil(2);
    let (local, remote) = items.split_at_mut(split);
    let (local_result, remote_result) = join_on(pool,
        || local.iter_mut().try_for_each(&process),
        || remote.iter_mut().try_for_each(&process));
    local_result?;
    remote_result
}

pub(crate) fn join_on<A, B, L, R>(pool: &rayon::ThreadPool, local: L, remote: R) -> (A, B)
where A: Send, B: Send, L: FnOnce() -> A, R: FnOnce() -> B + Send {
    let mut remote_result = None;
    let local_result = pool.in_place_scope(|scope| {
        let result = &mut remote_result;
        scope.spawn(move |_| { *result = Some(remote()); });
        local()
    });
    (local_result, remote_result.expect("scoped DSP task completed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinator_progresses_while_auxiliary_worker_is_delayed() {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let (local_tx, local_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        std::thread::scope(|scope| {
            let job = scope.spawn(|| {
                let mut values: Vec<_> = (0..16).collect();
                run_on(&pool,&mut values,|value| -> Result<(), ()> {
                    if *value == 8 { release_rx.lock().unwrap().recv().unwrap(); }
                    if *value == 7 { local_tx.send(()).unwrap(); }
                    *value += 100;
                    Ok(())
                }).unwrap();
                values
            });
            // Release even on timeout so a failing assertion cannot strand a
            // scoped thread that still borrows this test's data.
            let progressed = local_rx.recv_timeout(std::time::Duration::from_secs(2)).is_ok();
            release_tx.send(()).unwrap();
            assert!(progressed);
            assert_eq!(job.join().unwrap(),(100..116).collect::<Vec<_>>());
        });
    }
    #[test]
    fn caller_executes_local_half_and_outputs_match_serial() {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let caller = std::thread::current().id();
        let local_count = std::sync::atomic::AtomicUsize::new(0);
        let mut values: Vec<_> = (0..17).collect();
        run_on(&pool, &mut values, |value| -> Result<(), ()> {
            if std::thread::current().id() == caller { local_count.fetch_add(1,std::sync::atomic::Ordering::Relaxed); }
            *value = *value * 3 + 1; Ok(())
        }).unwrap();
        assert_eq!(local_count.load(std::sync::atomic::Ordering::Relaxed),9);
        assert_eq!(values,(0..17).map(|v|v*3+1).collect::<Vec<_>>());
    }
    #[test]
    fn error_returns_only_after_borrowed_worker_work_completes() {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let mut values: Vec<_> = (0..16).collect();
        let result = run_on(&pool,&mut values,|value| {
            if *value == 0 { return Err("local error"); }
            *value += 100; Ok(())
        });
        assert_eq!(result,Err("local error"));
        assert!(values[8..].iter().all(|v|*v>=108));
    }
}
