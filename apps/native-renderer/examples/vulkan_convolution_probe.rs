//! Real GPU FFT + MAC + packed stereo IFFT, compared with production convolution.
//! Diagnostic only: never opens an audio device or changes runtime selection.
#[cfg(not(target_os="android"))]
fn main(){eprintln!("This diagnostic requires Android");std::process::exit(2);}
#[cfg(target_os="android")]
fn main(){android::main();}
#[cfg(target_os="android")]
mod android {
use rustfft::{FftPlanner, num_complex::Complex32};
use sda_native_renderer::convolution::StereoPartitionedConvolver;
use std::{
    ffi::{CStr, c_char, c_void},
    time::Instant,
};
const P: usize = 1024;
const N: usize = 2048;
const B: usize = 1025;
#[repr(C)]
struct Info {
    vendor: u32,
    device: u32,
    api: u32,
    driver: u32,
    queue: u32,
    name: [c_char; 256],
}
#[repr(C)]
#[derive(Default)]
struct Stats {
    upload: u64,
    dispatch: u64,
    readback: u64,
    total: u64,
    cpu: u64,
    input_flags: u64,
    output_flags: u64,
    input_bytes: u64,
    output_bytes: u64,
}
#[link(name = "sdaconvolutionprobe", kind = "static")]
unsafe extern "C" {
    fn sda_chain_open(
        code: *const u32,
        bytes: usize,
        info: *mut Info,
        err: *mut c_char,
        cap: usize,
    ) -> *mut c_void;
    fn sda_chain_set_poll(ptr: *mut c_void, ns: u64);
    fn sda_chain_configure(
        ptr: *mut c_void,
        rows: u32,
        parts: u32,
        cpu: u64,
        err: *mut c_char,
        cap: usize,
    ) -> i32;
    fn sda_chain_run(
        ptr: *mut c_void,
        input: *const f32,
        output: *mut f32,
        dirty: i32,
        timeout: u64,
        err: *mut c_char,
        cap: usize,
    ) -> i32;
    fn sda_chain_stats(ptr: *mut c_void, out: *mut Stats);
    fn sda_chain_close(ptr: *mut c_void);
}
fn error(e: &[c_char; 256]) -> String {
    unsafe { CStr::from_ptr(e.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}
fn random(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    ((*seed >> 8) as i32 - 8388608) as f32 / 8388608.
}
struct Source {
    conv: StereoPartitionedConvolver,
    blocks: Vec<Vec<f32>>,
    input: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
}
fn process_sources(sources: &mut [Source], pool: &rayon::ThreadPool) -> Result<(), String> {
    let process = |s: &mut Source| {
        s.left.fill(0.);
        s.right.fill(0.);
        s.conv.process_block(&s.input, &mut s.left, &mut s.right)
    };
    let (head, tail) = sources.split_first_mut().unwrap();
    let queue = std::sync::Mutex::new(tail.iter_mut());
    let drain = || -> Result<(), String> {
        loop {
            let next = queue.lock().unwrap().next();
            match next {
                Some(s) => process(s)?,
                None => return Ok(()),
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
        process(head).and_then(|_| drain())
    });
    local.and(remote)
}
pub(super) fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: vulkan_convolution_probe SHADER.spv [POLL_NS]")?;
    let shader = std::fs::read(path).map_err(|e| e.to_string())?;
    let poll_ns = std::env::args()
        .nth(2)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
        .min(200_000);
    let code: Vec<_> = shader
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let mut info = Info {
        vendor: 0,
        device: 0,
        api: 0,
        driver: 0,
        queue: 0,
        name: [0; 256],
    };
    let mut err = [0; 256];
    let ptr = unsafe {
        sda_chain_open(
            code.as_ptr(),
            shader.len(),
            &mut info,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    if ptr.is_null() {
        return Err(error(&err));
    }
    struct Guard(*mut c_void);
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { sda_chain_close(self.0) }
        }
    }
    let _guard = Guard(ptr);
    unsafe { sda_chain_set_poll(ptr, poll_ns) };
    let placement = sda_native_renderer::android_audio_cpu::enter("gpu-chain-probe");
    eprintln!("CPU placement {placement}");
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .start_handler(|_| {
            sda_native_renderer::android_audio_cpu::enter("gpu-chain-cpu-reference");
        })
        .build()
        .map_err(|e| e.to_string())?;
    let mut seed = 0x12345678;
    let mut cases = vec![];
    // Active rows include padded/inactive rows, quiet inputs, long tails and max partitions.
    for (rows, active, parts, quiet) in [
        (8, 8, 1, false),
        (16, 15, 2, false),
        (32, 15, 2, false),
        (32, 30, 2, true),
        (32, 15, 4, false),
        (64, 60, 2, false),
        (128, 120, 2, false),
        (128, 120, 4, false),
        (256, 240, 2, false),
    ] {
        let pcm_base = rows + N * 2;
        let filters_base = pcm_base + rows * parts * N;
        let ni = filters_base + rows * parts * B * 4;
        let no = rows * P * 2;
        let mut packet = vec![0f32; ni];
        let mut output = vec![0f32; no];
        let mut reference = vec![0f32; no];
        for row in 0..active {
            packet[row] = 1.;
        }
        for i in 0..N {
            let a = (-2. * std::f64::consts::PI / N as f64) * i as f64;
            packet[rows + 2 * i] = a.cos() as f32;
            packet[rows + 2 * i + 1] = a.sin() as f32;
        }
        let mut planner = FftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(N);
        let mut sources = vec![];
        for row in 0..rows {
            let left: Vec<_> = (0..parts * P)
                .map(|i| {
                    let decay = (-6. * i as f32 / (parts * P) as f32).exp();
                    random(&mut seed) * decay * 0.03 + if i == 0 { 0.3 } else { 0. }
                })
                .collect();
            let right: Vec<_> = left
                .iter()
                .enumerate()
                .map(|(i, x)| *x * 0.71 + if i == 17 { 0.12 } else { 0. })
                .collect();
            for part in 0..parts {
                let mut work = vec![Complex32::new(0., 0.); N];
                for i in 0..P {
                    work[i] = Complex32::new(left[part * P + i], right[part * P + i]);
                }
                forward.process(&mut work);
                for bin in 0..B {
                    let a = work[bin];
                    let b = work[(N - bin) % N];
                    let base = filters_base + ((row * parts + part) * B + bin) * 4;
                    packet[base] = (a.re + b.re) * 0.5;
                    packet[base + 1] = (a.im - b.im) * 0.5;
                    packet[base + 2] = (a.im + b.im) * 0.5;
                    packet[base + 3] = (b.re - a.re) * 0.5;
                }
            }
            sources.push(Source {
                conv: StereoPartitionedConvolver::new(&left, &right, P)?,
                blocks: vec![vec![0.; P]; parts + 1],
                input: vec![0.; P],
                left: vec![0.; P],
                right: vec![0.; P],
            });
        }
        let cpu_bytes = (ni + no * 2) * 4;
        if unsafe {
            sda_chain_configure(
                ptr,
                rows as u32,
                parts as u32,
                cpu_bytes as u64,
                err.as_mut_ptr(),
                err.len(),
            )
        } != 0
        {
            cases.push(serde_json::json!({"rows":rows,"parts":parts,"error":error(&err)}));
            continue;
        }
        let mut failed_frames = vec![];
        let mut worst_rms = 0f64;
        let mut worst_peak = 0f64;
        let mut cpu_ns = 0u128;
        let mut pack_ns = 0u128;
        let mut gpu_ns = 0u64;
        let mut upload_ns = 0;
        let mut wait_ns = 0;
        let mut read_ns = 0;
        let mut host_cpu_ns = 0;
        let mut measured = 0;
        for frame in 0..32 {
            if frame == 12 {
                for pair in 0..active / 2 {
                    let target = sources[pair * 2 + 1].conv.prepared_filter();
                    sources[pair * 2].conv.transition_to(target, P / 8);
                }
            }
            if frame == 13 {
                for pair in 0..active / 2 {
                    let count = parts * B * 4;
                    let from = filters_base + (pair * 2 + 1) * count;
                    packet.copy_within(from..from + count, filters_base + pair * 2 * count);
                }
            }
            for (row, source) in sources.iter_mut().enumerate() {
                source.blocks.rotate_left(1);
                for i in 0..P {
                    let value = if row >= active || frame >= 24 {
                        0.
                    } else if frame == 0 {
                        if i == 0 { 0.5 } else { 0. }
                    } else {
                        random(&mut seed) * if quiet { 1e-6 } else { 0.1 }
                    };
                    source.input[i] = value;
                    source.blocks[parts][i] = value;
                }
            }
            // Paired rows share one source history and carry old/new filters for transitions.
            for pair in 0..active / 2 {
                let (before, after) = sources.split_at_mut(pair * 2 + 1);
                let old = &before[pair * 2];
                let new = &mut after[0];
                new.input.copy_from_slice(&old.input);
                for (dst, src) in new.blocks.iter_mut().zip(&old.blocks) {
                    dst.copy_from_slice(src);
                }
            }
            let started = Instant::now();
            for (row, source) in sources.iter().enumerate() {
                for part in 0..parts {
                    let b = pcm_base + (row * parts + part) * N;
                    packet[b..b + P].copy_from_slice(&source.blocks[parts - part - 1]);
                    packet[b + P..b + N].copy_from_slice(&source.blocks[parts - part]);
                }
            }
            let packed = started.elapsed().as_nanos();
            let started = Instant::now();
            process_sources(&mut sources, &pool)?;
            let cpu = started.elapsed().as_nanos();
            for (row, source) in sources.iter().enumerate() {
                for i in 0..P {
                    reference[(row * P + i) * 2] = source.left[i];
                    reference[(row * P + i) * 2 + 1] = source.right[i];
                }
            }
            // All output samples, including quiet input, overlap boundaries, inactive rows and drained tails.
            if unsafe {
                sda_chain_run(
                    ptr,
                    packet.as_ptr(),
                    output.as_mut_ptr(),
                    (frame == 0 || frame == 13) as i32,
                    100_000_000,
                    err.as_mut_ptr(),
                    err.len(),
                )
            } != 0
            {
                return Err(format!(
                    "rows={rows} parts={parts} frame={frame}: {}",
                    error(&err)
                ));
            }
            let mut stats = Stats::default();
            unsafe { sda_chain_stats(ptr, &mut stats) };
            if frame == 12 {
                for pair in 0..active / 2 {
                    for i in 0..P {
                        let mix = (i as f32 / (P / 8) as f32).min(1.);
                        for ear in 0..2 {
                            let a = (pair * 2 * P + i) * 2 + ear;
                            let b = ((pair * 2 + 1) * P + i) * 2 + ear;
                            output[a] = output[a] * (1. - mix) + output[b] * mix;
                        }
                    }
                }
            }
            let mut energy = 0.;
            let mut delta = 0.;
            let mut peak = 0f64;
            let mut peak_err = 0f64;
            for (&actual, &expected) in output.iter().zip(&reference) {
                if !actual.is_finite() {
                    return Err("nonfinite chain output".into());
                }
                let e = actual as f64 - expected as f64;
                energy += (expected as f64).powi(2);
                delta += e * e;
                peak = peak.max(expected.abs() as f64);
                peak_err = peak_err.max(e.abs());
            }
            let rms = if energy > 0. {
                (delta / energy).sqrt()
            } else if delta == 0. {
                0.
            } else {
                f64::INFINITY
            };
            let pe = if peak > 0. {
                peak_err / peak
            } else if peak_err == 0. {
                0.
            } else {
                f64::INFINITY
            };
            worst_rms = worst_rms.max(rms);
            worst_peak = worst_peak.max(pe);
            if rms > 1e-6 || pe > 4e-6 {
                failed_frames.push(frame);
            }
            if (5..24).contains(&frame) {
                cpu_ns += cpu;
                pack_ns += packed;
                gpu_ns += stats.total;
                upload_ns += stats.upload;
                wait_ns += stats.dispatch;
                read_ns += stats.readback;
                host_cpu_ns += stats.cpu;
                measured += 1;
            }
        }
        let result = serde_json::json!({"rows":rows,"activeRows":active,"parts":parts,"quiet":quiet,"passed":failed_frames.is_empty(),"failedFrames":failed_frames,"framesChecked":32,"measuredFrames":measured,"worstRelativeRms":worst_rms,"worstRelativePeak":worst_peak,"cpuParallelFullConvolutionAverageNs":cpu_ns/measured,"packingAverageNs":pack_ns/measured,"gpuFullChainAverageNs":gpu_ns/measured as u64,"gpuUploadAverageNs":upload_ns/measured as u64,"gpuDispatchWaitAverageNs":wait_ns/measured as u64,"gpuReadbackAverageNs":read_ns/measured as u64,"gpuHostCpuAverageNs":host_cpu_ns/measured as u64,"pollBudgetNs":poll_ns,"filterTransitionFrame":12,"filterRefreshFrame":13,"packetAndGpuBudgetBytes":cpu_bytes+ni*4+no*4});
        println!("{result}");
        cases.push(result);
    }
    let all_passed = cases.iter().all(|c| c["passed"] == true);
    println!(
        "{}",
        serde_json::json!({"device":unsafe{CStr::from_ptr(info.name.as_ptr())}.to_string_lossy(),"vulkanApi":info.api,"driver":info.driver,"precision":"FP32-no-relaxation-ordered-spectral-MAC","operation":"forward-FFT-partitioned-MAC-packed-stereo-IFFT-overlap-save","allPassed":all_passed,"cases":cases})
    );
    if all_passed {
        Ok(())
    } else {
        Err("PCM precision gate rejected one or more cases; runtime adoption forbidden".into())
    }
}
}
