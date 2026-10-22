use super::{Batch, Shape};
use std::sync::{
    Mutex, MutexGuard, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::time::{Duration, Instant};

pub(super) trait Backend {
    fn configure(&mut self, shape: Shape) -> Result<String, String>;
    fn execute(&mut self, batch: &mut Batch, timeout_ns: u64) -> Result<(), String>;
    fn details(&self) -> serde_json::Value { serde_json::json!({}) }
    fn deactivate(&mut self) {}
}
struct Metrics {
    available: AtomicBool,
    active: AtomicBool,
    applied: AtomicU64,
    audio_jobs: AtomicU64,
    attempts: AtomicU64,
    fallbacks: AtomicU64,
    last_ns: AtomicU64,
    native_ns: AtomicU64,
    detail: Mutex<serde_json::Value>,
}
impl Default for Metrics {
    fn default() -> Self {
        Self {
            available: AtomicBool::new(false),
            active: AtomicBool::new(false),
            applied: AtomicU64::new(0),
            audio_jobs: AtomicU64::new(0),
            attempts: AtomicU64::new(0),
            fallbacks: AtomicU64::new(0),
            last_ns: AtomicU64::new(0),
            native_ns: AtomicU64::new(0),
            detail: Mutex::new(serde_json::json!({"state":"starting"})),
        }
    }
}
impl Metrics {
    fn detail(&self, state: &str, reason: &str) {
        if let Ok(mut d) = self.detail.lock() {
            *d = serde_json::json!({"state":state,"reason":reason});
        }
    }
}
enum Command {
    Configure(Shape),
    Deactivate,
    Run {
        id: u64,
        batch: Batch,
        submitted: Instant,
        timeout_ns: u64,
        qualifying: bool,
    },
}
enum Response {
    Configured(Result<Batch, String>),
    Executed {
        id: u64,
        batch: Batch,
        elapsed_ns: u64,
        result: Result<(), String>,
        qualifying: bool,
    },
}
struct Client {
    tx: SyncSender<Command>,
    rx: Receiver<Response>,
    pending: bool,
    shape: Option<Shape>,
    ready: Option<Batch>,
    active: bool,
    qualified: u32,
    warmup: u32,
    frame: u64,
    id: u64,
    cpu_ns: u64,
    pack_ns: u64,
    copy_ns: u64,
    native_ns: u64,
    block_base_ns: u64,
    failed: Vec<Shape>,
}
pub(super) struct Service {
    client: Mutex<Client>,
    metrics: std::sync::Arc<Metrics>,
}
static SERVICE: OnceLock<Service> = OnceLock::new();
fn ns(d: Duration) -> u64 {
    d.as_nanos().min(u64::MAX as u128) as u64
}
impl Service {
    fn spawn(factory: impl FnOnce() -> Result<Box<dyn Backend>, String> + Send + 'static) -> Self {
        let metrics = std::sync::Arc::new(Metrics::default());
        let wm = metrics.clone();
        let (tx, input) = mpsc::sync_channel(1);
        let (output, rx) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new().name("sda-audio-compute".into()).spawn(move || {
            #[cfg(target_os = "android")]
            unsafe { unsafe extern "C" { fn setpriority(which: i32, who: u32, priority: i32) -> i32; } let _ = setpriority(0, 0, 10); }
            #[cfg(target_os = "ios")]
            unsafe { unsafe extern "C" { fn pthread_set_qos_class_self_np(class:u32,priority:i32)->i32; } let _=pthread_set_qos_class_self_np(0x11,0); }
            let mut backend = match factory() { Ok(b) => b, Err(e) => { wm.detail("unavailable", &e); return; } };
            wm.available.store(true, Ordering::Release); wm.detail("awaiting-audio-shape", "");
            while let Ok(command) = input.recv() {
                let response = match command {
                    Command::Deactivate => { backend.deactivate(); continue; }
                    Command::Configure(shape) => {
                        wm.active.store(false, Ordering::Release); wm.detail("compiling", "");
                        let result = backend.configure(shape).map(|device| {
                            if let Ok(mut d) = wm.detail.lock() {
                                *d = serde_json::json!({"state":"qualifying-audio", "deviceName":device,
                                    "rows":shape.rows,"partitions":shape.parts,"bins":shape.bins,
                                    "supportedGraphOperations":super::Graph::new(shape.parts).operations.len(),
                                    "explicitAcceleratorCount":if cfg!(target_os="android"){1}else{0},
                                    "backendDetails":backend.details()});
                            }
                            Batch::new(shape)
                        });
                        if let Err(e) = &result { wm.detail("shape-unavailable", e); }
                        Response::Configured(result)
                    }
                    Command::Run { id, mut batch, submitted, timeout_ns, qualifying } => {
                        wm.attempts.fetch_add(1, Ordering::Relaxed);
                        let result = backend.execute(&mut batch, timeout_ns);
                        let elapsed_ns = ns(submitted.elapsed()); wm.last_ns.store(elapsed_ns, Ordering::Relaxed);
                        if result.is_ok() { wm.audio_jobs.fetch_add(1, Ordering::Relaxed); }
                        Response::Executed { id, batch, elapsed_ns, result, qualifying }
                    }
                };
                if output.send(response).is_err() { break; }
            }
        });
        if spawned.is_err() {
            metrics.detail("unavailable", "worker-start-failed");
        }
        Self {
            client: Mutex::new(Client {
                tx,
                rx,
                pending: false,
                shape: None,
                ready: None,
                active: false,
                qualified: 0,
                warmup: 0,
                frame: 0,
                id: 0,
                cpu_ns: 0,
                pack_ns: 0,
                copy_ns: 0,
                native_ns: 0,
                block_base_ns: 0,
                failed: Vec::new(),
            }),
            metrics,
        }
    }
    fn begin(&self, shape: Shape) -> Option<Lease<'_>> {
        if !self.metrics.available.load(Ordering::Acquire) {
            return None;
        }
        let mut client = self.client.try_lock().ok()?;
        while let Ok(response) = client.rx.try_recv() {
            client.accept(response, &self.metrics);
        }
        client.frame = client.frame.wrapping_add(1);
        if client.failed.contains(&shape) || client.failed.len() >= 8 {
            return None;
        }
        if client.shape != Some(shape) {
            if !client.pending {
                if client.tx.try_send(Command::Configure(shape)).is_ok() {
                    client.ready = None;
                    client.shape = Some(shape);
                    client.pending = true;
                    client.active = false;
                    client.qualified = 0;
                    client.warmup = 0;
                    self.metrics.active.store(false, Ordering::Release);
                }
            }
            return None;
        }
        if client.pending || (!client.active && client.frame % 16 != 0) {
            return None;
        }
        let clearing = Instant::now();
        let mut packet = client.ready.take()?;
        packet.clear();
        let clear_ns = ns(clearing.elapsed());
        client.native_ns = self.metrics.native_ns.load(Ordering::Relaxed);
        Some(Lease {
            client,
            metrics: &self.metrics,
            packet: Some(packet),
            clear_ns,
            applied: false,
        })
    }
}
impl Client {
    fn reject(&mut self, metrics: &Metrics, reason: &str) {
        // Release rejected driver models/QoS on their worker, never in rendering.
        let _ = self.tx.try_send(Command::Deactivate);
        self.active = false;
        self.ready = None;
        if let Some(shape) = self.shape {
            if !self.failed.contains(&shape) && self.failed.len() < 8 {
                self.failed.push(shape);
            }
        }
        metrics.active.store(false, Ordering::Release);
        metrics.fallbacks.fetch_add(1, Ordering::Relaxed);
        // Called only on qualification completion or a bounded active failure.
        // try_lock avoids a diagnostics reader ever holding the audio renderer.
        if let Ok(mut d) = metrics.detail.try_lock() {
            d["state"] = "cpu-fallback".into(); d["reason"] = reason.into();
        }
    }
    fn accept(&mut self, response: Response, metrics: &Metrics) {
        self.pending = false;
        match response {
            Response::Configured(Ok(batch)) => self.ready = Some(batch),
            Response::Configured(Err(error)) => {
                if let Ok(mut d) = metrics.detail.try_lock() { d["driverError"] = error.into(); }
                self.reject(metrics, "driver-does-not-support-fp32-audio-graph");
            }
            Response::Executed {
                id,
                batch,
                elapsed_ns,
                result,
                qualifying,
            } => {
                if id != self.id || self.shape.is_none_or(|s| self.failed.contains(&s)) {
                    return;
                }
                if let Err(error) = result {
                    if let Ok(mut d) = metrics.detail.try_lock() { d["driverError"] = error.into(); }
                    self.reject(metrics, "driver-execution-failed");
                    return;
                }
                if !batch.output.iter().all(|x| x.is_finite()) {
                    self.reject(metrics, "nonfinite-driver-output");
                    return;
                }
                if qualifying {
                    if !batch.reference_matches() {
                        self.reject(metrics, "fp32-audio-parity-failed");
                        return;
                    }
                    // Warm the driver with real audio while CPU output continues.
                    // No warmup result is ever used as a playback result.
                    if self.warmup < 2 {
                        self.warmup += 1;
                        self.ready = Some(batch);
                        return;
                    }
                    let full_cost = self
                        .block_base_ns
                        .saturating_add(elapsed_ns)
                        .saturating_add(self.copy_ns);
                    if !profitable(self.cpu_ns, self.pack_ns, self.copy_ns, elapsed_ns)
                        || (self.native_ns > 0 && full_cost >= self.native_ns * 95 / 100)
                    {
                        self.reject(metrics, "transfer-and-dispatch-slower-than-native-cpu");
                        return;
                    }
                    self.qualified += 1;
                    if self.qualified >= 8 {
                        self.active = true;
                        // Enabled only after a result has actually been applied to PCM.
                        if let Ok(mut d) = metrics.detail.try_lock() {
                            d["state"] = "qualified".into();
                        }
                    }
                }
                self.ready = Some(batch);
            }
        }
    }
}
fn profitable(cpu_ns: u64, pack_ns: u64, copy_ns: u64, execution_ns: u64) -> bool {
    // Keep 20% margin for scheduler jitter; include packing and result unpacking.
    cpu_ns > 0
        && pack_ns.saturating_add(copy_ns).saturating_add(execution_ns)
            < cpu_ns.saturating_mul(8) / 10
}
pub(crate) struct Lease<'a> {
    client: MutexGuard<'a, Client>,
    metrics: &'a Metrics,
    packet: Option<Batch>,
    clear_ns: u64,
    applied: bool,
}
impl Lease<'_> {
    pub fn batch(&self) -> &Batch {
        self.packet.as_ref().unwrap()
    }
    pub fn batch_mut(&mut self) -> &mut Batch {
        self.packet.as_mut().unwrap()
    }
    pub fn is_qualifying(&self) -> bool {
        !self.client.active
    }
    pub fn packed_in(&mut self, duration: Duration) {
        self.client.pack_ns = ns(duration).saturating_add(self.clear_ns);
    }
    pub fn submit_reference(&mut self, cpu: Duration, copy: Duration) {
        self.client.cpu_ns = ns(cpu);
        self.client.copy_ns = ns(copy);
        self.submit(true, if self.client.warmup < 2 { 100_000_000 } else { 1_000_000 });
    }
    fn submit(&mut self, qualifying: bool, timeout_ns: u64) -> bool {
        self.client.id = self.client.id.wrapping_add(1);
        let command = Command::Run {
            id: self.client.id,
            batch: self.packet.take().unwrap(),
            submitted: Instant::now(),
            timeout_ns,
            qualifying,
        };
        match self.client.tx.try_send(command) {
            Ok(()) => {
                self.client.pending = true;
                true
            }
            Err(
                mpsc::TrySendError::Full(Command::Run { batch, .. })
                | mpsc::TrySendError::Disconnected(Command::Run { batch, .. }),
            ) => {
                self.packet = Some(batch);
                self.client.reject(self.metrics, "worker-unavailable");
                false
            }
            Err(_) => unreachable!(),
        }
    }
    pub fn completed_in(&mut self, elapsed: Duration) {
        if self.applied {
            self.metrics.applied.fetch_add(1, Ordering::Relaxed);
            self.metrics.active.store(true, Ordering::Release);
            if self.client.native_ns > 0 && ns(elapsed) >= self.client.native_ns {
                self.client.reject(
                    self.metrics,
                    "complete-render-block-slower-than-cpu-reference",
                );
            }
        } else if self.client.pending {
            // Qualification uses CPU output. Replace its MAC and reference copy
            // with measured NPU execution + output copy to compare whole blocks,
            // including extra executor barriers and clearing/packing buffers.
            self.client.block_base_ns = ns(elapsed)
                .saturating_sub(self.client.cpu_ns)
                .saturating_sub(self.client.copy_ns);
        }
    }
    pub fn execute(&mut self) -> bool { self.execute_with_limit(1_000_000) }
    fn execute_with_limit(&mut self, limit_ns:u64) -> bool {
        let budget = (self.client.cpu_ns * 8 / 10)
            .saturating_sub(self.client.pack_ns + self.client.copy_ns)
            .min(limit_ns);
        if budget == 0 {
            self.client.reject(self.metrics, "no-profitable-deadline");
            return false;
        }
        let started = Instant::now();
        if !self.submit(false, budget) {
            return false;
        }
        match self.client.rx.recv_timeout(Duration::from_nanos(budget)) {
            Ok(response) => {
                // recv_timeout can return a queued value after the deadline if
                // this thread was descheduled. Check the actual clock as well.
                let late = ns(started.elapsed()) > budget || matches!(&response,
                    Response::Executed { elapsed_ns, .. } if *elapsed_ns > budget);
                if late {
                    self.client.pending = false;
                    self.client.reject(self.metrics,"audio-deadline-missed");
                    return false;
                }
                self.client.accept(response, self.metrics);
                self.packet = self.client.ready.take();
                if self.packet.is_some() && self.client.active {
                    self.applied = true;
                    return true;
                }
            }
            Err(_) => self.client.reject(self.metrics, "audio-deadline-missed"),
        }
        false
    }
}
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if self.client.active
            || self
                .client
                .shape
                .is_some_and(|s| !self.client.failed.contains(&s))
        {
            if let Some(packet) = self.packet.take() {
                self.client.ready = Some(packet);
            }
        }
    }
}
#[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
pub(crate) fn start() {
    #[cfg(target_os = "android")]
    if !super::android::configured() { return; }
    SERVICE.get_or_init(|| {
        #[cfg(target_os = "android")]
        { Service::spawn(|| super::android::Backend::open().map(|b|Box::new(b)as Box<dyn Backend>)) }
        #[cfg(target_os = "ios")]
        { Service::spawn(|| super::coreml::Backend::open().map(|b|Box::new(b)as Box<dyn Backend>)) }
    });
}
#[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
pub(crate) fn begin(
    rows: usize,
    shapes: impl Iterator<Item = (usize, usize)>,
) -> Option<Lease<'static>> {
    let mut bins = None;
    let mut parts = 0;
    for (p, b) in shapes {
        if bins.is_some_and(|n| n != b) {
            return None;
        }
        bins = Some(b);
        parts = parts.max(p);
    }
    let shape = Shape::bounded(rows, parts, bins?)?;
    SERVICE.get()?.begin(shape)
}
#[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
pub(crate) fn observe_native_block(duration: Duration) {
    if let Some(s) = SERVICE.get() {
        s.metrics.native_ns.store(ns(duration), Ordering::Relaxed);
    }
}
#[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
pub(crate) fn status() -> serde_json::Value {
    let Some(s) = SERVICE.get() else {
        return serde_json::json!({"state":"not-started","npuAudioEnabled":false});
    };
    let m = &s.metrics;
    let mut result = m
        .detail
        .lock()
        .map(|x| x.clone())
        .unwrap_or(serde_json::json!({}));
    #[cfg(target_os = "android")]
    {
        result["runtime"] = result["backendDetails"]["runtime"].as_str().unwrap_or("android-audio-backend-selection").into();
        let active = m.active.load(Ordering::Acquire);
        let kind = result["backendDetails"]["acceleratorKind"].as_str();
        let dsp = kind == Some("dsp");
        let gpu = kind == Some("gpu");
        result["audioOffloadEnabled"] = active.into();
        result["dspAudioEnabled"] = (active && dsp).into();
        result["runtimeLibraryLifetime"] = "process-pinned-nodelete".into();
        result["gpuAudioEnabled"] = (active && gpu).into();
        result["npuAudioEnabled"] = (active && !dsp && !gpu).into();
        result["fp16Relaxation"] = false.into();
        result["frameworkCpuFallbackAllowed"] = false.into();
        result["driverInternalPlacement"] = (if gpu {"explicit-Vulkan-physical-GPU-queue"}else{"not-observable-through-public-api"}).into();
    }
    #[cfg(target_os = "ios")]
    {
        result["runtime"] = "apple-coreml-cpu-and-neural-engine".into();
        result["coremlAudioEnabled"] = m.active.load(Ordering::Acquire).into();
        result["npuAudioEnabled"] = false.into();
        result["neuralEngineExecutionVerified"] = false.into();
        result["frameworkCpuFallbackAllowed"] = true.into();
        result["gpuAllowed"] = false.into();
        result["internalPrecision"] = "Core-ML-managed-fp32-output-qualified".into();
        result["hardwarePlacement"] = "not-observable-per-prediction-through-public-API".into();
    }
    result["operation"] = "hrtf-fp32-complex-spectral-mac".into();
    result["audioBatchesSubmitted"] = m.attempts.load(Ordering::Relaxed).into();
    result["audioBatchesExecuted"] = m.audio_jobs.load(Ordering::Relaxed).into();
    result["audioBatchesApplied"] = m.applied.load(Ordering::Relaxed).into();
    result["fallbacks"] = m.fallbacks.load(Ordering::Relaxed).into();
    result["lastExecutionNs"] = m.last_ns.load(Ordering::Relaxed).into();
    result["lastNativeRenderBlockNs"] = m.native_ns.load(Ordering::Relaxed).into();

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Mock {
        delay: Duration,
        wrong: bool,
        fail: bool,
    }
    impl Backend for Mock {
        fn configure(&mut self, _: Shape) -> Result<String, String> {
            Ok("test-only-explicit-accelerator".into())
        }
        fn execute(&mut self, b: &mut Batch, _: u64) -> Result<(), String> {
            if self.delay > Duration::ZERO {
                std::thread::sleep(self.delay);
            }
            if self.fail {
                return Err("injected".into());
            }
            b.scalar_reference();
            if self.wrong {
                b.output[0] += 1.;
            }
            Ok(())
        }
    }
    fn service(delay: Duration, wrong: bool, fail: bool) -> Service {
        let s = Service::spawn(move || Ok(Box::new(Mock { delay, wrong, fail })));
        for _ in 0..100 {
            if s.metrics.available.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        s
    }
    fn lease(s: &Service, shape: Shape) -> Lease<'_> {
        for _ in 0..100 {
            if let Some(l) = s.begin(shape) {
                return l;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("no lease")
    }
    fn reference(l: &mut Lease<'_>) {
        let b = l.batch_mut();
        b.input.fill(0.125);
        b.scalar_reference();
        b.reference.copy_from_slice(&b.output);
        b.output.fill(0.);
        l.packed_in(Duration::from_nanos(1));
        l.submit_reference(Duration::from_millis(50), Duration::from_nanos(1));
    }
    #[test]
    fn qualification_requires_two_warmups_and_eight_actual_audio_matches() {
        let s = service(Duration::ZERO, false, false);
        let shape = Shape::bounded(2, 2, 9).unwrap();
        for _ in 0..10 {
            let mut l = lease(&s, shape);
            assert!(l.is_qualifying());
            reference(&mut l);
        }
        let mut l = lease(&s, shape);
        assert!(!l.is_qualifying());
        assert!(!s.metrics.active.load(Ordering::Acquire));
        l.batch_mut().input.fill(0.25);
        assert!(l.execute_with_limit(100_000_000));
        assert!(l.batch().output.iter().any(|x| *x != 0.));
        l.completed_in(Duration::from_micros(100));
        assert_eq!(s.metrics.applied.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn incorrect_audio_result_disables_shape() {
        let s = service(Duration::ZERO, true, false);
        let shape = Shape::bounded(2, 2, 9).unwrap();
        {
            let mut l = lease(&s, shape);
            reference(&mut l);
        }
        std::thread::sleep(Duration::from_millis(5));
        assert!(s.begin(shape).is_none());
        assert_eq!(s.metrics.fallbacks.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn failed_driver_does_not_qualify() {
        let s = service(Duration::ZERO, false, true);
        let shape = Shape::bounded(2, 2, 9).unwrap();
        {
            let mut l = lease(&s, shape);
            reference(&mut l);
        }
        std::thread::sleep(Duration::from_millis(5));
        assert!(s.begin(shape).is_none());
        assert_eq!(s.metrics.applied.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn late_result_never_applies_to_next_block() {
        let s = service(Duration::from_millis(8), false, false);
        let shape = Shape::bounded(2, 2, 9).unwrap();
        let mut l = lease(&s, shape);
        l.client.active = true;
        l.client.cpu_ns = 50_000_000;
        l.batch_mut().input.fill(0.25);
        assert!(!l.execute());
        drop(l);
        std::thread::sleep(Duration::from_millis(12));
        assert!(s.begin(shape).is_none());
        assert_eq!(s.metrics.applied.load(Ordering::Relaxed), 0);
        assert!(!s.metrics.active.load(Ordering::Acquire));
    }
    #[test]
    fn rejected_graph_releases_driver_resources_off_the_render_thread() {
        struct Releasing(std::sync::Arc<AtomicBool>);
        impl Backend for Releasing {
            fn configure(&mut self,_:Shape)->Result<String,String>{self.0.store(true,Ordering::Release);Ok("test-driver".into())}
            fn execute(&mut self,b:&mut Batch,_:u64)->Result<(),String>{b.output.fill(1000.);Ok(())}
            fn deactivate(&mut self){self.0.store(false,Ordering::Release);}
        }
        let live=std::sync::Arc::new(AtomicBool::new(false));let worker_live=live.clone();
        let s=Service::spawn(move||Ok(Box::new(Releasing(worker_live))));
        let shape=Shape::bounded(2,2,9).unwrap();
        {let mut l=lease(&s,shape);assert!(live.load(Ordering::Acquire));reference(&mut l);}
        for _ in 0..100 {let _=s.begin(shape);if !live.load(Ordering::Acquire){break;}std::thread::sleep(Duration::from_millis(1));}
        assert!(!live.load(Ordering::Acquire));assert_eq!(s.metrics.applied.load(Ordering::Relaxed),0);
    }
    #[test]
    fn packing_and_dispatch_must_beat_cpu() {
        assert!(profitable(1000, 100, 100, 500));
        assert!(!profitable(1000, 200, 200, 500));
        assert!(!profitable(0, 0, 0, 0));
    }
    #[test]
    fn fast_operation_with_slow_whole_block_cannot_qualify() {
        let s=service(Duration::ZERO,false,false);let shape=Shape::bounded(2,2,9).unwrap();
        let mut l=lease(&s,shape);let mut batch=l.packet.take().unwrap();
        batch.input.fill(0.125);batch.scalar_reference();batch.reference.copy_from_slice(&batch.output);
        l.client.cpu_ns=10_000_000;l.client.pack_ns=100;l.client.copy_ns=100;
        l.client.warmup=2;l.client.native_ns=5_000_000;l.client.block_base_ns=6_000_000;
        let id=l.client.id;
        l.client.accept(Response::Executed{id,batch,elapsed_ns:1000,result:Ok(()),qualifying:true},l.metrics);
        assert!(!l.client.active);assert!(l.client.failed.contains(&shape));
        assert_eq!(s.metrics.applied.load(Ordering::Relaxed),0);
    }
    #[test]
    fn pending_previous_shape_cannot_supply_pcm_for_new_shape() {
        let s=service(Duration::from_millis(5),false,false);
        let old=Shape::bounded(2,2,9).unwrap();let new=Shape::bounded(32,3,17).unwrap();
        {let mut l=lease(&s,old);reference(&mut l);}
        assert!(s.begin(new).is_none());
        std::thread::sleep(Duration::from_millis(10));
        let l=lease(&s,new);assert_eq!(l.batch().shape,new);
        assert_eq!(s.metrics.applied.load(Ordering::Relaxed),0);
    }

}
