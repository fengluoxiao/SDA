//! CPU/GPU sharing policy. No device model lists, clocks, gain changes or OS
//! governor writes. Thermal hints are control-plane inputs; rendering publishes
//! its measured budget and reserve. GPU still requires parity and CPU savings.
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
static EPOCH: AtomicU64 = AtomicU64::new(0);
static THERMAL: AtomicI32 = AtomicI32::new(0);
static HEADROOM: AtomicU32 = AtomicU32::new(f32::NAN.to_bits());
static FREQUENCY_CEILING:AtomicU32=AtomicU32::new(u32::MAX);
static DUTY: AtomicU32 = AtomicU32::new(0);
static PERIOD: AtomicU64 = AtomicU64::new(0);
static RESERVE: AtomicU64 = AtomicU64::new(0);
static UTIL: AtomicU64 = AtomicU64::new(0);
static PRESSURE_TIME: AtomicU64 = AtomicU64::new(0);
static RECOVERY_TIME: AtomicU64 = AtomicU64::new(0);
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}
pub fn new_render_session() {
    PERIOD.store(0, Ordering::Relaxed);
    RESERVE.store(0, Ordering::Relaxed);
    UTIL.store(0, Ordering::Relaxed);
    DUTY.store(0, Ordering::Relaxed);
    PRESSURE_TIME.store(0, Ordering::Relaxed);
    RECOVERY_TIME.store(0, Ordering::Relaxed);
    EPOCH.fetch_add(1, Ordering::AcqRel);
}
pub fn frequency_ceiling(value:Option<u32>){FREQUENCY_CEILING.store(value.unwrap_or(u32::MAX),Ordering::Relaxed);}
fn capped_and_busy(ceiling:u32,usage:u64)->bool{ceiling<850_000&&usage>=200_000}
pub fn thermal(status: i32, headroom: f32) {
    THERMAL.store(status.clamp(0, 6), Ordering::Relaxed);
    HEADROOM.store(
        if headroom.is_finite() && headroom >= 0. {
            headroom
        } else {
            f32::NAN
        }
        .to_bits(),
        Ordering::Relaxed,
    );
}
fn step(
    duty: u32,
    pressure: bool,
    recover: bool,
    period: u64,
    hot: &mut u64,
    cool: &mut u64,
) -> u32 {
    if pressure {
        *cool = 0;
        *hot = hot.saturating_add(period);
        if *hot >= 1_000_000_000 {
            *hot = 0;
            return (duty + 25).min(100);
        }
    } else if recover {
        *hot = 0;
        *cool = cool.saturating_add(period);
        if *cool >= 5_000_000_000 {
            *cool = 0;
            return duty.saturating_sub(25);
        }
    } else {
        *hot = 0;
        *cool = 0;
    }
    duty
}
pub fn observe(frames: usize, rate: usize, fifo: usize, elapsed_ns: u64) {
    if frames == 0 || rate == 0 {
        return;
    }
    let period = (frames as u64).saturating_mul(1_000_000_000) / rate as u64;
    PERIOD.store(period, Ordering::Relaxed);
    RESERVE.store(
        (fifo as u64).saturating_mul(1_000_000_000) / rate as u64,
        Ordering::Relaxed,
    );
    let usage = elapsed_ns.saturating_mul(1_000_000) / period.max(1);
    let old = UTIL.load(Ordering::Relaxed);
    let ema = (old * 31 + usage.min(4_000_000)) / 32;
    UTIL.store(ema, Ordering::Relaxed);
    let thermal = THERMAL.load(Ordering::Relaxed);
    let h = f32::from_bits(HEADROOM.load(Ordering::Relaxed));
    let pressure = thermal >= 1 || (h.is_finite() && h >= 0.7) || ema >= 650_000 || capped_and_busy(FREQUENCY_CEILING.load(Ordering::Relaxed),ema);
    let recover = thermal == 0 && (!h.is_finite() || h < 0.5) && ema < 450_000;
    let (mut hot, mut cool) = (
        PRESSURE_TIME.load(Ordering::Relaxed),
        RECOVERY_TIME.load(Ordering::Relaxed),
    );
    DUTY.store(
        step(
            DUTY.load(Ordering::Relaxed),
            pressure,
            recover,
            period,
            &mut hot,
            &mut cool,
        ),
        Ordering::Relaxed,
    );
    PRESSURE_TIME.store(hot, Ordering::Relaxed);
    RECOVERY_TIME.store(cool, Ordering::Relaxed);
}
pub fn deadline_ns() -> u64 {
    budget(
        PERIOD.load(Ordering::Relaxed),
        RESERVE.load(Ordering::Relaxed),
    )
}
fn budget(period: u64, reserve: u64) -> u64 {
    if period == 0 || reserve < period.saturating_mul(3) {
        0
    } else {
        (period / 4).min(4_000_000)
    }
}
pub fn share() -> u32 {
    DUTY.load(Ordering::Relaxed)
}
pub fn cpu_saving(cpu: u64, packing: u64, copy: u64, worker: u64) -> bool {
    cpu > 0
        && worker > 0
        && packing
            .saturating_add(copy)
            .saturating_add(worker)
            .saturating_add(15_000)
            < cpu.saturating_mul(85) / 100
}
pub fn status() -> serde_json::Value {
    let h = f32::from_bits(HEADROOM.load(Ordering::Relaxed));
    serde_json::json!({"mode":"thermal-cpu-gpu-cooperation","cpuFrequencyCeilingPpm":match FREQUENCY_CEILING.load(Ordering::Relaxed){u32::MAX=>None,x=>Some(x)},"thermalStatus":THERMAL.load(Ordering::Relaxed),"thermalHeadroom":if h.is_finite(){Some(h)}else{None},"requestedGpuSharePercent":share(),"renderUtilizationPpm":UTIL.load(Ordering::Relaxed),"renderPeriodNs":PERIOD.load(Ordering::Relaxed),"outputReserveNs":RESERVE.load(Ordering::Relaxed),"deadlineNs":deadline_ns(),"criterion":"FP32-parity-and-lower-total-host-CPU-within-audio-budget"})
}
#[cfg(target_os = "android")]
pub fn cpu_ns() -> u64 {
    #[repr(C)]
    struct Timespec {
        seconds: std::ffi::c_long,
        nanos: std::ffi::c_long,
    }
    unsafe extern "C" {
        fn clock_gettime(id: i32, time: *mut Timespec) -> i32;
    }
    let mut t = Timespec {
        seconds: 0,
        nanos: 0,
    };
    if unsafe { clock_gettime(3, &mut t) } == 0 {
        (t.seconds as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(t.nanos as u64)
    } else {
        0
    }
}
#[cfg(not(target_os = "android"))]
pub fn cpu_ns() -> u64 {
    0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_frequency_cap_requires_active_render_work(){assert!(capped_and_busy(700_000,250_000));assert!(!capped_and_busy(700_000,50_000));assert!(!capped_and_busy(1_000_000,250_000));assert!(!capped_and_busy(u32::MAX,250_000));}
    #[test]
    fn ramp_and_recovery_have_hysteresis() {
        let (mut hot, mut cool, mut duty) = (0, 0, 0);
        for _ in 0..4 {
            for _ in 0..50 {
                duty = step(duty, true, false, 20_000_000, &mut hot, &mut cool);
            }
        }
        assert_eq!(duty, 100);
        for _ in 0..249 {
            duty = step(duty, false, true, 20_000_000, &mut hot, &mut cool);
        }
        assert_eq!(duty, 100);
        duty = step(duty, false, true, 20_000_000, &mut hot, &mut cool);
        assert_eq!(duty, 75);
    }
    #[test]
    fn buffer_and_block_deadlines_bound_wait() {
        assert_eq!(budget(21_333_333, 300_000_000), 4_000_000);
        assert_eq!(budget(5_333_333, 50_000_000), 1_333_333);
        assert_eq!(budget(21_333_333, 40_000_000), 0);
        assert_eq!(budget(0, 300_000_000), 0);
    }
    #[test]
    fn wall_time_alone_does_not_prove_cpu_saving() {
        assert!(cpu_saving(400_000, 35_000, 25_000, 90_000));
        assert!(!cpu_saving(100_000, 35_000, 25_000, 90_000));
        assert!(!cpu_saving(400_000, 0, 0, 0));
    }
}
