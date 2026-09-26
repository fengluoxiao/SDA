//! Opt-in profiling. Audio threads never wait for a log writer or disk.
use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static ENABLED: AtomicBool = AtomicBool::new(false);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static CALLBACK_SAMPLE: AtomicU64 = AtomicU64::new(0);
static CALLBACK_TIME: AtomicU64 = AtomicU64::new(0);
static WORKLOAD_AT: AtomicU64 = AtomicU64::new(0);
static CHANNEL: OnceLock<SyncSender<Message>> = OnceLock::new();
enum Message {
    Workload(serde_json::Value),
    Configure(Option<std::fs::File>),
    Sample(&'static str, String, f64, u64, u64),
    Ingress(String, u64, u64),
    Reset,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    stage: String,
    id: String,
    count: u64,
    total_ms: f64,
    min_ms: f64,
    max_ms: f64,
    max_at_ms: u64,
    units: u64,
}
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
pub fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}
fn send(message: Message) {
    if let Some(tx) = CHANNEL.get() {
        if tx.try_send(message).is_err() {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
}
pub fn configure(active: bool, path: Option<String>) -> Result<(), String> {
    if active && path.as_ref().is_none_or(|p| p.is_empty()) {
        return Err("performance log path required".into());
    }
    let opened = if active {
        Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path.as_ref().unwrap())
                .map_err(|e| format!("performance log: {e}"))?,
        )
    } else {
        None
    };
    let tx=CHANNEL.get_or_init(||{let(tx,rx)=sync_channel::<Message>(8192);std::thread::Builder::new().name("sda-performance-writer".into()).spawn(move||{
        let mut file:Option<std::fs::File>=None;let mut rows=BTreeMap::<(String,String),Row>::new();let mut pending=VecDeque::new();let mut last=Instant::now();let mut bytes=0usize;let mut generation=0u64;let mut reset_at=now_us()/1000;let mut workload=None;
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Message::Configure(opened))=>{file=opened;rows.clear();pending.clear();bytes=file.as_ref().and_then(|f|f.metadata().ok()).map(|m|m.len() as usize).unwrap_or(0);reset_at=now_us()/1000;}
                Ok(Message::Workload(value))=>workload=Some(value),
                Ok(Message::Reset)=>{pending.clear();reset_at=now_us()/1000;},
                Ok(Message::Ingress(id,sample,time))=>{if pending.len()<16384{pending.push_back((id,sample,time));}else{DROPPED.fetch_add(1,Ordering::Relaxed);}}
                Ok(Message::Sample(stage,id,ms,units,at))=>add(&mut rows,stage,id,ms,units,at),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>break,
                Err(_)=>{}
            }
            if last.elapsed()>=Duration::from_secs(1){
                let sample=CALLBACK_SAMPLE.load(Ordering::Acquire);let time=CALLBACK_TIME.load(Ordering::Acquire);
                let mut presentations=Vec::new();
                pending.retain(|(id,start,received)|{
                    if time>=*received && sample>=*start {
                        let projected=time.saturating_sub((sample-start)*1_000_000/48000);
                        if projected>=*received {presentations.push(serde_json::json!({"id":id,"sample":start,"receivedMs":*received as f64/1000.0,"callbackMs":projected as f64/1000.0}));add(&mut rows,"pcm.to_output_callback_estimate",id.clone(),(projected-received) as f64/1000.0,1,projected/1000);}
                        false
                    }else{now_us().saturating_sub(*received)<120_000_000}
                });
                let value=serde_json::json!({"type":"native-performance","time":now_us()/1000,"intervalMs":last.elapsed().as_secs_f64()*1000.0,"pid":std::process::id(),"dropped":DROPPED.swap(0,Ordering::Relaxed),"writerAvailable":file.is_some(),"generation":generation,"resetAtMs":reset_at,"workload":workload.take(),"presentations":presentations,"rows":rows.values().collect::<Vec<_>>()});
                if let Some(f)=&mut file {if let Ok(line)=serde_json::to_vec(&value){
                    // Bound each native capture. Truncation is explicitly marked
                    // by a generation marker; never allocate an unbounded disk log.
                    if bytes>64*1024*1024 {use std::io::{Seek,SeekFrom};let _=f.set_len(0);let _=f.seek(SeekFrom::Start(0));bytes=0;generation+=1;let marker=serde_json::json!({"type":"rotation","generation":generation,"time":now_us()/1000});let _=writeln!(f,"{marker}");}
                    if f.write_all(&line).and_then(|_|f.write_all(b"\n")).is_err(){ENABLED.store(false,Ordering::Relaxed);}bytes+=line.len()+1;
                }}
                rows.clear();last=Instant::now();
            }
        }
    }).expect("performance writer thread");tx});
    tx.try_send(Message::Configure(opened))
        .map_err(|_| "performance writer busy".to_string())?;
    ENABLED.store(active, Ordering::Release);
    Ok(())
}
fn add(
    rows: &mut BTreeMap<(String, String), Row>,
    stage: &str,
    id: String,
    ms: f64,
    units: u64,
    at: u64,
) {
    if rows.len() > 2048 {
        return;
    }
    let row = rows.entry((stage.into(), id.clone())).or_insert(Row {
        stage: stage.into(),
        id,
        count: 0,
        total_ms: 0.0,
        min_ms: f64::INFINITY,
        max_ms: 0.0,
        max_at_ms: at,
        units: 0,
    });
    row.count += 1;
    row.total_ms += ms;
    row.min_ms = row.min_ms.min(ms);
    if ms >= row.max_ms {
        row.max_ms = ms;
        row.max_at_ms = at;
    }
    row.units += units;
}
pub fn sample(stage: &'static str, id: &str, ms: f64, units: u64) {
    if enabled() {
        send(Message::Sample(
            stage,
            id.into(),
            ms,
            units,
            now_us() / 1000,
        ));
    }
}
pub fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}
pub fn finish(start: Option<Instant>, stage: &'static str, id: &str, units: u64) {
    if let Some(at) = start {
        sample(stage, id, at.elapsed().as_secs_f64() * 1000.0, units);
    }
}
pub fn ingress(id: &str, sample: u64) {
    if enabled() {
        send(Message::Ingress(id.into(), sample, now_us()));
    }
}
pub fn reset() {
    if enabled() {
        send(Message::Reset);
    }
}
pub fn callback(sample: u64) {
    if enabled() {
        CALLBACK_SAMPLE.store(sample, Ordering::Release);
        CALLBACK_TIME.store(now_us(), Ordering::Release);
    }
}
pub struct Span {
    start: Option<Instant>,
    stage: &'static str,
    id: String,
    units: u64,
}
pub fn span(stage: &'static str, id: &str, units: u64) -> Span {
    Span {
        start: start(),
        stage,
        id: if enabled() { id.into() } else { String::new() },
        units,
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        finish(self.start, self.stage, &self.id, self.units);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_keeps_the_time_of_the_slowest_call() {
        let mut rows = BTreeMap::new();
        add(&mut rows, "hrtf.test", "obj:1".into(), 4.0, 128, 1000);
        add(&mut rows, "hrtf.test", "obj:1".into(), 2.0, 128, 2000);
        let row = rows.get(&("hrtf.test".into(), "obj:1".into())).unwrap();
        assert_eq!(row.count, 2);
        assert_eq!(row.units, 256);
        assert_eq!(row.min_ms, 2.0);
        assert_eq!(row.max_ms, 4.0);
        assert_eq!(row.max_at_ms, 1000);
    }
}

pub fn workload_due() -> bool {
    if !enabled() {
        return false;
    }
    let now = now_us() / 1000;
    let previous = WORKLOAD_AT.load(Ordering::Relaxed);
    now.saturating_sub(previous) >= 1000
        && WORKLOAD_AT
            .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}
pub fn workload(value: serde_json::Value) {
    send(Message::Workload(value));
}
