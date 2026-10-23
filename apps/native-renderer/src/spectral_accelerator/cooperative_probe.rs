//! Diagnostic-only physical GPU cooperation check; no OS thermal mutation or audio device.
// Diagnostic resource constraint, scoped to this probe process only. It models
// reduced CPU availability without changing a governor or heating the phone.
struct CpuConstraint {
    saved: Vec<(i32, String, [u64; 16])>,
    cores: Vec<usize>,
}
impl CpuConstraint {
    fn enter() -> Result<Self, String> {
        unsafe extern "C" {
            fn sched_getaffinity(tid: i32, size: usize, mask: *mut std::ffi::c_void) -> i32;
            fn sched_setaffinity(tid: i32, size: usize, mask: *const std::ffi::c_void) -> i32;
        }
        let mut scores = vec![];
        for cpu in 0..64 {
            if let Ok(text) =
                std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{cpu}/cpu_capacity"))
            {
                if let Ok(n) = text.trim().parse::<u64>() {
                    scores.push((cpu, n));
                }
            }
        }
        let low = scores
            .iter()
            .map(|v| v.1)
            .min()
            .ok_or("No public CPU capacity topology")?;
        let cores: Vec<_> = scores.iter().filter(|v| v.1 == low).map(|v| v.0).collect();
        if cores.len() < 2 || cores.len() == scores.len() {
            return Err("No suitable heterogeneous CPU constraint".into());
        }
        let mut target = [0u64; 16];
        for &cpu in &cores {
            target[cpu / 64] |= 1u64 << (cpu % 64);
        }
        let mut saved = vec![];
        for entry in std::fs::read_dir("/proc/self/task").map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let Ok(tid) = entry.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            let Ok(name) = std::fs::read_to_string(entry.path().join("comm")) else {
                continue;
            };
            let mut mask = [0u64; 16];
            if unsafe { sched_getaffinity(tid, 128, mask.as_mut_ptr().cast()) } == 0
                && unsafe { sched_setaffinity(tid, 128, target.as_ptr().cast()) } == 0
            {
                saved.push((tid, name, mask));
            }
        }
        if saved.is_empty() {
            return Err("Probe affinity restriction was not permitted".into());
        }
        Ok(Self { saved, cores })
    }
    fn restore(&mut self) {
        unsafe extern "C" {
            fn sched_setaffinity(tid: i32, size: usize, mask: *const std::ffi::c_void) -> i32;
        }
        for (tid, name, mask) in self.saved.drain(..) {
            if std::fs::read_to_string(format!("/proc/self/task/{tid}/comm"))
                .ok()
                .as_ref()
                == Some(&name)
            {
                unsafe {
                    sched_setaffinity(tid, 128, mask.as_ptr().cast());
                }
            }
        }
    }
}
impl Drop for CpuConstraint {
    fn drop(&mut self) {
        self.restore();
    }
}
pub(super) fn run(count: usize, constrain: bool) -> Result<serde_json::Value, String> {
    use super::cooperation as policy;
    use crate::convolution::StereoPartitionedConvolver;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::Instant,
    };
    const P: usize = 1024;
    struct Source {
        conv: StereoPartitionedConvolver,
        reference: StereoPartitionedConvolver,
        input: Vec<f32>,
        left: Vec<f32>,
        right: Vec<f32>,
        ref_left: Vec<f32>,
        ref_right: Vec<f32>,
    }
    let count = count.clamp(8, 128);
    crate::android_audio_cpu::enter("cooperation-probe-caller");
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .start_handler(|_| {
            crate::android_audio_cpu::enter("cooperation-probe-helper");
        })
        .build()
        .map_err(|e| e.to_string())?;
    fn dispatch<T: Send, F>(pool: &rayon::ThreadPool, items: &mut [T], f: F) -> Result<(), String>
    where
        F: Fn(&mut T) -> Result<(), String> + Sync,
    {
        let (head, tail) = items.split_first_mut().unwrap();
        let queue = std::sync::Mutex::new(tail.iter_mut());
        let drain = || {
            loop {
                let next = queue.lock().unwrap().next();
                match next {
                    Some(s) => f(s)?,
                    None => return Ok::<(), String>(()),
                }
            }
        };
        let mut remote = Ok(());
        let local = pool.in_place_scope(|scope| {
            let r = &mut remote;
            scope.spawn(|_| {
                let (a, b) = rayon::join(&drain, &drain);
                *r = a.and(b);
            });
            f(head).and_then(|_| drain())
        });
        local.and(remote)
    }
    let mut sources = vec![];
    for row in 0..count {
        let left: Vec<_> = (0..P * 2)
            .map(|i| {
                if i == 0 {
                    0.2
                } else {
                    ((i * 17 + row * 31) % 53) as f32 / 53000.
                        * (-5. * i as f32 / (P * 2) as f32).exp()
                }
            })
            .collect();
        let right: Vec<_> = left
            .iter()
            .enumerate()
            .map(|(i, x)| x * 0.8 + if i == row % 37 + 13 { 0.03 } else { 0. })
            .collect();
        sources.push(Source {
            conv: StereoPartitionedConvolver::new(&left, &right, P)?,
            reference: StereoPartitionedConvolver::new(&left, &right, P)?,
            input: vec![0.; P],
            left: vec![0.; P],
            right: vec![0.; P],
            ref_left: vec![0.; P],
            ref_right: vec![0.; P],
        });
    }
    policy::new_render_session();
    policy::thermal(0, f32::NAN);
    policy::frequency_ceiling(None);
    super::configure_android_runtime("", 26, "diagnostic-system-vulkan")?;
    let mut phases = vec![];
    let mut frame = 0usize;
    let mut worst = 0f64;
    let mut constraint: Option<CpuConstraint> = None;
    for (phase, frames, status) in [
        ("cool-calibration", 240, 0),
        ("simulated-pressure", 400, 1),
        ("simulated-recovery", 20, 0),
        ("next-render-session", 180, 0),
    ] {
        if phase == "simulated-pressure" && constrain {
            constraint = Some(CpuConstraint::enter()?);
        }
        if phase == "simulated-recovery" {
            if let Some(mut c) = constraint.take() {
                c.restore();
            }
        }
        if phase == "next-render-session" {
            policy::new_render_session();
        }
        policy::thermal(status, f32::NAN);
        if phase == "simulated-recovery" {
            for _ in 0..1200 {
                policy::observe(P, 48000, 16000, 1_000_000);
            }
        }
        let before = super::status();
        let mut gpu_blocks = 0;
        for _ in 0..frames {
            frame += 1;
            for (row, s) in sources.iter_mut().enumerate() {
                s.left.fill(0.);
                s.right.fill(0.);
                s.ref_left.fill(0.);
                s.ref_right.fill(0.);
                for (i, x) in s.input.iter_mut().enumerate() {
                    *x = (((frame * P + i) * (row % 7 + 3)) as f32 * 0.00037).sin() * 0.03;
                }
            }
            let started = Instant::now();
            dispatch(&pool, &mut sources, |s| {
                s.conv.begin_block(&s.input)?;
                Ok(())
            })?;
            if let Some(mut lease) =
                super::begin(count * 2, sources.iter().map(|s| s.conv.spectral_shape()))
            {
                let pack = Instant::now();
                for (i, s) in sources.iter().enumerate() {
                    s.conv.pack_spectra(lease.batch_mut(), i * 2);
                }
                lease.batch_mut().finish_packet();
                lease.packed_in(pack.elapsed());
                if lease.is_qualifying() {
                    let work = AtomicU64::new(0);
                    let wall = Instant::now();
                    dispatch(&pool, &mut sources, |s| {
                        let t = policy::cpu_ns();
                        s.conv.calculate_spectra();
                        if t > 0 {
                            work.fetch_add(policy::cpu_ns().saturating_sub(t), Ordering::Relaxed);
                        }
                        Ok(())
                    })?;
                    let elapsed = wall.elapsed();
                    lease.batch_mut().cpu_work_ns = work.load(Ordering::Relaxed);
                    let copied = Instant::now();
                    for (i, s) in sources.iter().enumerate() {
                        s.conv.export_spectra(lease.batch_mut(), i * 2);
                    }
                    lease.submit_reference(elapsed, copied.elapsed());
                } else if lease.execute() {
                    gpu_blocks += 1;
                    for (i, s) in sources.iter_mut().enumerate() {
                        s.conv.import_spectra(lease.batch(), i * 2);
                    }
                } else {
                    dispatch(&pool, &mut sources, |s| {
                        s.conv.calculate_spectra();
                        Ok(())
                    })?;
                }
                dispatch(&pool, &mut sources, |s| {
                    s.conv
                        .finish_prepared_block(&s.input, &mut s.left, &mut s.right);
                    Ok(())
                })?;
                lease.completed_in(started.elapsed());
            } else {
                dispatch(&pool, &mut sources, |s| {
                    s.conv.calculate_spectra();
                    s.conv
                        .finish_prepared_block(&s.input, &mut s.left, &mut s.right);
                    Ok(())
                })?;
                super::observe_native_block(started.elapsed());
            }
            let compute_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            // Simulation supplies an audio reserve and thermal hint, never an OS sensor value.
            policy::observe(P, 48000, 16000, compute_ns);
            dispatch(&pool, &mut sources, |s| {
                s.reference
                    .process_block(&s.input, &mut s.ref_left, &mut s.ref_right)
            })?;
            for s in &sources {
                for (a, b) in s
                    .left
                    .iter()
                    .chain(&s.right)
                    .zip(s.ref_left.iter().chain(&s.ref_right))
                {
                    if !a.is_finite() {
                        return Err("nonfinite PCM".into());
                    }
                    worst = worst.max((*a as f64 - *b as f64).abs());
                }
            }
            if worst != 0. {
                return Err(format!("PCM mismatch: {worst:e}"));
            }
        }
        let after = super::status();
        let result = serde_json::json!({"phase":phase,"frames":frames,"requestedProbeCpuCores":constraint.as_ref().map(|c|&c.cores),"probeConstrainedThreads":constraint.as_ref().map(|c|c.saved.len()),"gpuBlocksApplied":gpu_blocks,"appliedBefore":before["audioBatchesApplied"],"appliedAfter":after["audioBatchesApplied"],"status":after,"maximumPcmDifference":worst});
        println!("{result}");
        phases.push(result);
    }
    let adopted = phases.iter().any(|p| {
        p["phase"] == "simulated-pressure" && p["gpuBlocksApplied"].as_u64().unwrap_or(0) > 0
    });
    let recovery = phases
        .iter()
        .filter(|p| p["phase"] == "simulated-recovery")
        .all(|p| p["gpuBlocksApplied"] == 0);
    let result = serde_json::json!({"physicalGpu":true,"syntheticSources":count,"thermalInput":"simulated-within-probe-only","cpuPressureModel":if constrain{"efficiency-core-affinity-request; driver may rebind on reconfiguration"}else{"simulated-policy-hint-with-real-unrestricted-cpu-costs"},"otherApplicationAffinityChanged":false,"osThermalSettingsChanged":false,"audioDeviceOpened":false,"usesProductionModuleImplementations":true,"gpuAdoptionVerified":adopted,"recoveryVerified":recovery,"maximumPcmDifference":worst,"phases":phases});
    println!("{result}");
    if !adopted {
        return Err("GPU did not qualify for real CPU relief on this load".into());
    }
    if !recovery {
        return Err("cool recovery did not stop sharing".into());
    }
    Ok(result)
}
