//! Gate the real local consumer, independently of rendering/remote prebuffering.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Instant,SystemTime, UNIX_EPOCH};
pub static ENABLED: AtomicBool = AtomicBool::new(false);
static START_MS: AtomicU64 = AtomicU64::new(0);
static BUFFER_MS: AtomicU64 = AtomicU64::new(6000);
pub fn set_buffer_ms(value:u64){BUFFER_MS.store(if value==0 {6000}else{value.clamp(2000,6000)},Ordering::Release);}
pub fn buffer_frames()->usize{BUFFER_MS.load(Ordering::Acquire) as usize*48}
static STOP_MS: AtomicU64 = AtomicU64::new(u64::MAX);
fn monotonic_ms()->u64{static ORIGIN:OnceLock<Instant>=OnceLock::new();ORIGIN.get_or_init(Instant::now).elapsed().as_millis() as u64}
fn valid_deadline(enabled:bool,start_ms:u64,wall:u64)->bool{!enabled||start_ms<=1||start_ms>=wall.saturating_add(100)}
pub fn configure(enabled: bool, start_ms: u64, stop_ms: u64)->bool {
    let wall=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    if !valid_deadline(enabled,start_ms,wall){return false;}
    let now=monotonic_ms();let deadline=|time:u64|now.saturating_add(time.saturating_sub(wall)).max(1);
    START_MS.store(if start_ms==0 {0}else{deadline(start_ms)}, Ordering::Release);
    STOP_MS.store(if stop_ms == 0 {u64::MAX} else {deadline(stop_ms)}, Ordering::Release);
    ENABLED.store(enabled, Ordering::Release);
    true
}
pub fn hold() { START_MS.store(0, Ordering::Release); }
fn permits(enabled: bool, start: u64, stop: u64, now: u64) -> bool {
    !enabled || start != 0 && now >= start && now < stop
}
pub fn output_allowed() -> bool {
    if !ENABLED.load(Ordering::Acquire){return true;}
    permits(true, START_MS.load(Ordering::Acquire), STOP_MS.load(Ordering::Acquire),monotonic_ms())
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn clock_gate_holds_until_common_deadline_and_stops_at_deadline() {
        assert!(permits(false,0,0,500));
        assert!(!permits(true,0,u64::MAX,500));
        assert!(!permits(true,1000,2000,999));
        assert!(permits(true,1000,2000,1000));
        assert!(permits(true,1000,2000,1999));
        assert!(!permits(true,1000,2000,2000));
        assert!(!valid_deadline(true,1099,1000));assert!(valid_deadline(true,1100,1000));
        assert!(valid_deadline(true,0,1000));assert!(valid_deadline(true,1,1000));
    }
}
