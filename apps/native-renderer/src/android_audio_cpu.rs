//! Per-thread audio placement using allowed CPUs and public kernel topology.
//! No model whitelist, root, global governor changes or DSP precision changes.
fn select_fast(scores: &[(usize, u64)], minimum: usize) -> Option<Vec<usize>> {
    if scores.len() < minimum || scores.iter().any(|(_, s)| *s == 0) {
        return None;
    }
    let low = scores.iter().map(|x| x.1).min()?;
    let high = scores.iter().map(|x| x.1).max()?;
    if high.saturating_mul(100) < low.saturating_mul(110) {
        return None;
    }
    let fast: Vec<_> = scores
        .iter()
        .filter(|(_, s)| *s > low)
        .map(|x| x.0)
        .collect();
    (fast.len() >= minimum).then_some(fast)
}
// CPU presence is stable across hotplug. sched_getaffinity only reports the
// currently usable subset and must never define a process-lifetime topology.
fn present_cpu_ids(text: &str) -> Option<Vec<usize>> {
    let mut ids = Vec::new();
    for group in text.trim().split(',') {
        let mut range = group.trim().split('-');
        let first = range.next()?.parse::<usize>().ok()?;
        let last = match range.next() {
            Some(value) => value.parse::<usize>().ok()?,
            None => first,
        };
        if range.next().is_some() || first > last || last >= 64 { return None; }
        ids.extend(first..=last);
    }
    ids.sort_unstable();
    ids.dedup();
    (!ids.is_empty()).then_some(ids)
}
#[cfg(target_os = "android")]
mod platform {
    use std::{
        ffi::c_void,
        sync::{Mutex, OnceLock},
    };
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Mask([u64; 16]);
    impl Mask {
        fn current() -> Option<Self> {
            Self::for_tid(0)
        }
        fn for_tid(tid: i32) -> Option<Self> {
            let mut m = Self::default();
            (unsafe { sched_getaffinity(tid, 128, (&mut m as *mut Self).cast()) } == 0).then_some(m)
        }
        fn ids(&self) -> Vec<usize> {
            (0..1024)
                .filter(|i| self.0[i / 64] & (1u64 << (i % 64)) != 0)
                .collect()
        }
        fn from_ids(ids: &[usize]) -> Self {
            let mut m = Self::default();
            for &i in ids {
                if i < 1024 {
                    m.0[i / 64] |= 1u64 << (i % 64);
                }
            }
            m
        }
        fn intersect(self, other: Self) -> Self {
            let mut m = Self::default();
            for i in 0..16 {
                m.0[i] = self.0[i] & other.0[i];
            }
            m
        }
    }
    unsafe extern "C" {
        fn sched_getaffinity(tid: i32, size: usize, mask: *mut c_void) -> i32;
        fn sched_setaffinity(tid: i32, size: usize, mask: *const c_void) -> i32;
        fn gettid() -> i32;
    }
    struct Topology {
        fast: Vec<usize>,
        source: &'static str,
    }
    static TOPOLOGY: OnceLock<Topology> = OnceLock::new();
    static THREADS: Mutex<Vec<(i32, String)>> = Mutex::new(Vec::new());
    fn thread_name(tid: i32) -> Option<String> {
        std::fs::read_to_string(format!("/proc/self/task/{tid}/comm")).ok()
    }
    pub fn refresh() {
        let Some(topology) = TOPOLOGY.get() else {
            return;
        };
        if topology.fast.is_empty() {
            return;
        }
        let registered = THREADS.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut stale = Vec::new();
        for (tid, name) in registered {
            // Do not reuse an expired TID belonging to a different thread.
            if thread_name(tid).as_ref() != Some(&name) {
                stale.push((tid, name));
                continue;
            }
            let Some(current) = Mask::for_tid(tid) else {
                continue;
            };
            // Reapply the full preferred set, not its intersection with our
            // previous pin. The kernel intersects this with the task's cpuset
            // and online CPUs. This also restores cores after hotplug/cpuset
            // transitions instead of permanently shrinking the audio mask.
            let target = Mask::from_ids(&topology.fast);
            if target.0 != current.0 {
                let rc = unsafe { sched_setaffinity(tid, 128, (&target as *const Mask).cast()) };
                eprintln!(
                    "SDA audio CPU refresh tid={tid} target={:?} result={rc}",
                    target.ids()
                );
            }
        }
        if !stale.is_empty() {
            THREADS
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .retain(|x| !stale.contains(x));
        }
    }
    fn discover() -> Topology {
        let Some(ids) = std::fs::read_to_string("/sys/devices/system/cpu/present")
            .ok().and_then(|text| super::present_cpu_ids(&text)) else {
            return Topology {
                fast: vec![],
                source: "present-topology-unavailable",
            };
        };
        for (file, source) in [
            ("cpu_capacity", "capacity"),
            ("cpufreq/cpuinfo_max_freq", "maximum-frequency"),
        ] {
            let scores: Option<Vec<_>> = ids
                .iter()
                .map(|&i| {
                    let s =
                        std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{i}/{file}"))
                            .ok()?
                            .trim()
                            .parse::<u64>()
                            .ok()?;
                    Some((i, s))
                })
                .collect();
            if let Some(scores) = scores {
                return Topology {
                    fast: super::select_fast(&scores, 2).unwrap_or_default(),
                    source,
                };
            }
        }
        Topology {
            fast: vec![],
            source: "topology-not-public",
        }
    }
    pub fn frequency_ceiling_ppm()->Option<u32> {
        let topology=TOPOLOGY.get_or_init(discover);
        let mut ratios:Vec<u32>=topology.fast.iter().filter_map(|cpu| {
            let root=format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq");
            let nominal=std::fs::read_to_string(format!("{root}/cpuinfo_max_freq")).ok()?.trim().parse::<u64>().ok()?;
            let limit=std::fs::read_to_string(format!("{root}/scaling_max_freq")).ok()?.trim().parse::<u64>().ok()?;
            (nominal>0&&limit>0).then_some((limit.saturating_mul(1_000_000)/nominal.max(1)).min(1_000_000)as u32)
        }).collect();
        if ratios.len()<2{return None;}ratios.sort_unstable();Some(ratios[ratios.len()/2])
    }
    pub fn dsp_capacity() -> Option<usize> {
        let topology = TOPOLOGY.get_or_init(discover);
        let current = Mask::current()?;
        let n = current.intersect(Mask::from_ids(&topology.fast)).ids().len();
        (n >= 2).then_some(n)
    }
    pub fn prepare() {
        TOPOLOGY.get_or_init(discover);
    }
    pub fn enter(role: &str) -> serde_json::Value {
        let topology = TOPOLOGY.get_or_init(discover);
        let before = Mask::current();
        let mut result = None;
        if before.is_some() && topology.fast.len() >= 2 {
            let target = Mask::from_ids(&topology.fast);
            // sched_setaffinity enforces the system cpuset/online restriction.
            // Failure (e.g. no fast core allowed in background) leaves the
            // system-selected affinity intact. Homogeneous CPUs stay untouched.
            result = Some(unsafe { sched_setaffinity(0, 128, (&target as *const Mask).cast()) });
        }
        let actual = Mask::current().map(|m| m.ids()).unwrap_or_default();
        let tid = unsafe { gettid() };
        if let Some(name) = thread_name(tid) {
            let mut threads = THREADS.lock().unwrap_or_else(|p| p.into_inner());
            if threads.len() < 64 && !threads.iter().any(|x| x.0 == tid) {
                threads.push((tid, name));
            }
        }
        eprintln!(
            "SDA audio CPU role={role} tid={tid} source={} preferred={:?} actual={actual:?} result={result:?}",
            topology.source, topology.fast
        );
        serde_json::json!({"role":role,"tid":tid,"source":topology.source,"preferredCpus":topology.fast,
            "currentCpus":actual,"applied":result==Some(0),"errorCode":result.unwrap_or(0)})
    }
}
#[cfg(target_os = "android")]
pub use platform::{dsp_capacity, enter, prepare, refresh, frequency_ceiling_ppm};
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_hotplug_does_not_hide_present_performance_cores() {
        let present = present_cpu_ids("0-7\n").unwrap();
        // At startup affinity may expose only 0..=5; cores 6/7 still belong
        // in the cached topology so later playback can use them.
        let scores: Vec<_> = present.into_iter()
            .map(|id| (id, if id < 4 { 1843200 } else { 2208000 })).collect();
        assert_eq!(select_fast(&scores, 2), Some(vec![4, 5, 6, 7]));
    }
    #[test]
    fn present_cpu_ranges_are_bounded_and_sparse_lists_are_supported() {
        assert_eq!(present_cpu_ids("0-3,6-7"), Some(vec![0,1,2,3,6,7]));
        assert_eq!(present_cpu_ids("2,0-2"), Some(vec![0,1,2]));
        for invalid in ["", "1-0", "0-64", "0-3-7", "0,", "a"] {
            assert_eq!(present_cpu_ids(invalid), None, "{invalid}");
        }
    }
    #[test]
    fn two_online_fast_cores_are_usable_when_other_cores_are_offline() {
        assert_eq!(select_fast(&[(0,1843200),(1,1843200),(2,1843200),(3,1843200),(6,2208000),(7,2208000)],2),Some(vec![6,7]));
    }
    #[test]
    fn little_cores_are_not_the_only_dsp_targets() {
        let scores: Vec<_> = (0..8)
            .map(|i| (i, if i < 4 { 1843200 } else { 2208000 }))
            .collect();
        assert_eq!(select_fast(&scores, 3), Some(vec![4, 5, 6, 7]));
    }
    #[test]
    fn three_cluster_soc_keeps_middle_and_prime() {
        assert_eq!(
            select_fast(
                &[
                    (0, 512),
                    (1, 512),
                    (2, 512),
                    (3, 512),
                    (4, 768),
                    (5, 768),
                    (6, 768),
                    (7, 1024)
                ],
                3
            ),
            Some(vec![4, 5, 6, 7])
        );
    }
    #[test]
    fn homogeneous_or_too_few_fast_cores_remain_unrestricted() {
        assert_eq!(
            select_fast(&[(0, 1024), (1, 1024), (2, 1024), (3, 1024)], 3),
            None
        );
        assert_eq!(
            select_fast(
                &[
                    (0, 900),
                    (1, 900),
                    (2, 900),
                    (3, 900),
                    (4, 900),
                    (5, 900),
                    (6, 1024),
                    (7, 1024)
                ],
                3
            ),
            None
        );
        assert_eq!(select_fast(&[(0, 0), (1, 768), (2, 1024)], 3), None);
    }
}
