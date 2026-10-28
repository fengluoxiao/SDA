//! Android stereo output using the NDK's typed AAudio bindings.

use std::ffi::CString;
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ndk_sys as aaudio;

use super::record_callback;
use super::{AudioOutput, RuntimeTelemetry, render_command, stereo_fifo};

const SAMPLE_RATE: i32 = 48_000;
const CHANNELS: usize = 2;
const CALLBACK_STALLED:i32 = -1_000_001;
const WRITE_STALLED:i32 = -1_000_002;
const DISCONNECTED: i32 = -899; // AAUDIO_ERROR_DISCONNECTED, all supported API levels.
const WRITE_TIMEOUT_NS: i64 = 200_000_000;

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: i32, tag: *const std::ffi::c_char, text: *const std::ffi::c_char) -> i32;
}

fn android_log(message: &str) {
    if let Ok(text) = CString::new(message) {
        unsafe { __android_log_write(4, c"SdaAAudio".as_ptr(), text.as_ptr()); }
    }
}

// These symbols were introduced in API 32. Resolve them at runtime rather
// than linking directly: our APK must still load on Android API 26-31.
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(name: *const std::ffi::c_char, flags: i32) -> *mut std::ffi::c_void;
    fn dlsym(handle: *mut std::ffi::c_void, name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    fn dlclose(handle: *mut std::ffi::c_void) -> i32;
}

unsafe fn disable_system_spatialization(builder: *mut aaudio::AAudioStreamBuilder) {
    // Keep the library handle alive until both optional calls have completed.
    let handle = unsafe { dlopen(c"libaaudio.so".as_ptr(), 2) }; // RTLD_NOW
    if handle.is_null() {
        android_log("spatialization_policy=unavailable library=libaaudio.so");
        return;
    }
    for (name,value) in [(c"AAudioStreamBuilder_setUsage",1),(c"AAudioStreamBuilder_setContentType",2)] {
        let symbol=unsafe{dlsym(handle,name.as_ptr())};
        if !symbol.is_null(){let set:unsafe extern "C" fn(*mut aaudio::AAudioStreamBuilder,i32)=unsafe{std::mem::transmute(symbol)};unsafe{set(builder,value)};}
    }
    let behavior = unsafe { dlsym(handle, c"AAudioStreamBuilder_setSpatializationBehavior".as_ptr()) };
    let spatialized = unsafe { dlsym(handle, c"AAudioStreamBuilder_setIsContentSpatialized".as_ptr()) };
    if !behavior.is_null() {
        let set: unsafe extern "C" fn(*mut aaudio::AAudioStreamBuilder, i32) =
            unsafe { std::mem::transmute(behavior) };
        unsafe { set(builder, 2) }; // AAUDIO_SPATIALIZATION_BEHAVIOR_NEVER
    }
    if !spatialized.is_null() {
        let set: unsafe extern "C" fn(*mut aaudio::AAudioStreamBuilder, bool) =
            unsafe { std::mem::transmute(spatialized) };
        unsafe { set(builder, true) }; // SDA has already rendered binaural PCM.
    }
    android_log(&format!(
        "spatialization_policy requested_never={} requested_content_spatialized={}; vendor_global_effects_not_controlled",
        !behavior.is_null(), !spatialized.is_null(),
    ));
    unsafe { dlclose(handle) };
}

struct Builder(*mut aaudio::AAudioStreamBuilder);

impl Drop for Builder {
    fn drop(&mut self) {
        unsafe { aaudio::AAudioStreamBuilder_delete(self.0); }
    }
}

struct Stream(*mut aaudio::AAudioStream);

// The stream is opened before the renderer starts and then moved exactly once
// into the dedicated output thread. AAudio permits this ownership transfer;
// no pointer is shared with the producer thread.
unsafe impl Send for Stream {}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe { aaudio::AAudioStream_close(self.0); }
    }
}

#[derive(Default)]
struct CallbackContext {
    fifo: OnceLock<Arc<stereo_fifo::StereoFifo>>,
    telemetry: OnceLock<Arc<RuntimeTelemetry>>,
    error: AtomicI32,
    callbacks: std::sync::atomic::AtomicU64,
    ready: std::sync::atomic::AtomicBool,
}
struct PreparedStream {
    // Drop/close the stream before freeing callback userdata and its Arcs.
    stream: Stream,
    callback: Box<CallbackContext>,
    burst: usize,
    callback_mode: bool,
}
impl Drop for PreparedStream {
    fn drop(&mut self) {
        self.callback.ready.store(false,Ordering::Release);
        unsafe { aaudio::AAudioStream_requestStop(self.stream.0); }
        // Older drivers may deliver a final callback after requestStop. Keep
        // callback userdata alive until the stream has quiesced, then close it.
        for _ in 0..5 {
            let state=unsafe{aaudio::AAudioStream_getState(self.stream.0)};
            if state==aaudio::AAUDIO_STREAM_STATE_STOPPED as i32
                || state==aaudio::AAUDIO_STREAM_STATE_DISCONNECTED as i32
                || state==aaudio::AAUDIO_STREAM_STATE_CLOSED as i32 {break;}
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
unsafe extern "C" fn data_callback(_stream:*mut aaudio::AAudioStream,user:*mut std::ffi::c_void,audio:*mut std::ffi::c_void,frames:i32)->i32 {
    if user.is_null()||audio.is_null()||frames<=0 {return aaudio::AAUDIO_CALLBACK_RESULT_STOP as i32;}
    let context=unsafe{&*user.cast::<CallbackContext>()};
    context.callbacks.fetch_add(1,Ordering::Relaxed);
    let out=unsafe{std::slice::from_raw_parts_mut(audio.cast::<f32>(),frames as usize*CHANNELS)};
    if let (Some(fifo),Some(telemetry))=(context.fifo.get(),context.telemetry.get()) {
        if telemetry.shutdown_requested.load(Ordering::Acquire) {out.fill(0.);return aaudio::AAUDIO_CALLBACK_RESULT_STOP as i32;}
        if !context.ready.load(Ordering::Acquire) || telemetry.output_device_recovering.load(Ordering::Acquire) {out.fill(0.);}
        else {crate::output_pull::pull(fifo,telemetry,out);}
    } else {out.fill(0.);}
    aaudio::AAUDIO_CALLBACK_RESULT_CONTINUE as i32
}
unsafe extern "C" fn error_callback(_stream:*mut aaudio::AAudioStream,user:*mut std::ffi::c_void,error:i32) {
    if let Some(context)=unsafe{user.cast::<CallbackContext>().as_ref()} {
        if let Some(t)=context.telemetry.get(){
            // Publish recovery before the failure; the host must retain its decoder.
            t.output_device_recovering.store(error==DISCONNECTED,Ordering::Release);
            if error!=DISCONNECTED {t.output_device_error.store(error,Ordering::Release);}
        }
        context.error.store(error,Ordering::Release);
    }
}

/// Prepared AAudio device for the renderer's interleaved f32 stereo at 48 kHz.
/// The stream is negotiated on the caller thread so
/// startup can fail synchronously instead of leaving the render worker alive
/// with no device consumer.
pub struct AAudioWriterSink {
    stream: Mutex<Option<PreparedStream>>,
}

impl AAudioWriterSink {
    /// Opens and starts the device before the renderer starts producing PCM.
    /// Keeping the output contract exact prevents an Android mixer conversion
    /// from silently changing the calibrated 48 kHz KU100 render.
    pub fn open() -> Result<Self, String> {
        Ok(Self { stream: Mutex::new(Some(Self::prepare_stream()?)) })
    }

    fn prepare_stream() -> Result<PreparedStream, String> {
        let callback_mode = !cfg!(feature="pcm-diagnostic");
        let stream = Self::open_stream(callback_mode).or_else(|first| {
            if !callback_mode {return Err(first);}
            android_log(&format!("callback output unavailable; trying blocking output: {first}"));
            Self::open_stream(false).map_err(|second|format!("AAudio callback failed: {first}; blocking fallback failed: {second}"))
        })?;
        Ok(stream)
    }

    fn open_stream(callback_mode: bool) -> Result<PreparedStream, String> {
        unsafe {
            let mut builder = std::ptr::null_mut();
            let result = aaudio::AAudio_createStreamBuilder(&mut builder);
            if result != aaudio::AAUDIO_OK as i32 || builder.is_null() {
                return Err(format!("createStreamBuilder failed: {result}"));
            }
            let builder = Builder(builder);
            aaudio::AAudioStreamBuilder_setDirection(builder.0, aaudio::AAUDIO_DIRECTION_OUTPUT as i32);
            aaudio::AAudioStreamBuilder_setPerformanceMode(
                builder.0, aaudio::AAUDIO_PERFORMANCE_MODE_LOW_LATENCY as i32,
            );
            aaudio::AAudioStreamBuilder_setSharingMode(
                builder.0, aaudio::AAUDIO_SHARING_MODE_SHARED as i32,
            );
            aaudio::AAudioStreamBuilder_setChannelCount(builder.0, CHANNELS as i32);
            aaudio::AAudioStreamBuilder_setFormat(builder.0, aaudio::AAUDIO_FORMAT_PCM_FLOAT as i32);
            aaudio::AAudioStreamBuilder_setSampleRate(builder.0, SAMPLE_RATE);
            disable_system_spatialization(builder.0);
            aaudio::AAudioStreamBuilder_setBufferCapacityInFrames(builder.0,4096);
            let mut callback=Box::<CallbackContext>::default();
            let userdata=(&mut *callback as *mut CallbackContext).cast();
            if callback_mode {
                aaudio::AAudioStreamBuilder_setDataCallback(builder.0,Some(data_callback),userdata);
            }
            aaudio::AAudioStreamBuilder_setErrorCallback(builder.0,Some(error_callback),userdata);

            let mut stream = std::ptr::null_mut();
            let result = aaudio::AAudioStreamBuilder_openStream(builder.0, &mut stream);
            if result != aaudio::AAUDIO_OK as i32 || stream.is_null() {
                return Err(format!("openStream failed: {result}"));
            }
            let stream = Stream(stream);
            let format = aaudio::AAudioStream_getFormat(stream.0);
            let channels = aaudio::AAudioStream_getChannelCount(stream.0);
            let rate = aaudio::AAudioStream_getSampleRate(stream.0);
            let burst = aaudio::AAudioStream_getFramesPerBurst(stream.0);
            if format != aaudio::AAUDIO_FORMAT_PCM_FLOAT as i32
                || channels != CHANNELS as i32 || rate != SAMPLE_RATE || burst <= 0
            {
                return Err(format!(
                    "unsupported stream: format={format} channels={channels} rate={rate} burst={burst}; expected PCM_FLOAT(2), stereo, 48000 Hz",
                ));
            }
            android_log(&format!(
                "output_contract=ndk-f32-v1 format={format}(PCM_FLOAT) sample_bytes={} frame_bytes={} channels={channels} rate={rate} burst={burst}",
                size_of::<f32>(), size_of::<f32>() * CHANNELS,
            ));
            let capacity=aaudio::AAudioStream_getBufferCapacityInFrames(stream.0).max(1) as usize;
            let mut policy=crate::android_output_policy::BufferPolicy::new(burst as usize,capacity,0);
            let granted=aaudio::AAudioStream_setBufferSizeInFrames(stream.0,policy.initial() as i32);
            if granted>0 {policy.accepted(granted as usize);}
            android_log(&format!("output_mode={} buffer_frames={granted} capacity_frames={capacity} perf_mode={}",
                if callback_mode {"high-priority-callback"}else{"blocking-fallback-or-diagnostic"},
                aaudio::AAudioStream_getPerformanceMode(stream.0)));
            let result = aaudio::AAudioStream_requestStart(stream.0);
            if result != aaudio::AAUDIO_OK as i32 {
                return Err(format!("requestStart failed: {result}"));
            }
            Ok(PreparedStream{stream,callback,burst:burst as usize,callback_mode})
        }
    }
}

impl AudioOutput for AAudioWriterSink {
    fn run(
        self: Arc<Self>,
        fifo: Arc<stereo_fifo::StereoFifo>,
        telemetry: Arc<RuntimeTelemetry>,
        _commands: Arc<render_command::RenderCommandQueue>,
    ) {
        let Some(mut prepared) = self.stream.lock().ok().and_then(|mut stream| stream.take()) else {
            telemetry.output_device_error.store(-1,Ordering::Release);
            android_log("AAudio output stream was not prepared");
            return;
        };
        let mut pending = PendingWrite::default();
        let mut failures = 0;
        let mut xrun_base = 0;
        loop {
            bind_stream(&prepared,&fifo,&telemetry);
            let started=Instant::now();
            let result=if prepared.callback_mode {
                monitor_callback_stream(&prepared,&telemetry,xrun_base)
            } else {
                crate::realtime::android_priority(-16);
                writer_loop(&prepared,&fifo,&telemetry,&mut pending,xrun_base)
            };
            match result {
                Ok(()) => break,
                Err(error @ (DISCONNECTED | CALLBACK_STALLED | WRITE_STALLED)) => {
                    telemetry.output_device_recovering.store(true,Ordering::Release);
                    // Close only this device, on this worker, never in an AAudio callback.
                    // A silently dead callback can recur on the same route.
                    // The existing blocking backend keeps exact f32/48k PCM.
                    let callback_mode=prepared.callback_mode && error!=CALLBACK_STALLED;
                    android_log(&format!("output recovery reason={error} next_callback_mode={callback_mode}"));
                    drop(prepared);
                    xrun_base=telemetry.output_device_xruns.load(Ordering::Relaxed);
                    if started.elapsed()>=Duration::from_secs(1) {failures=0;}
                    let mut replacement=None;
                    while failures<3 && !telemetry.shutdown_requested.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(100));
                        if telemetry.paused.load(Ordering::Acquire) {continue;}
                        failures+=1;
                        match Self::open_stream(callback_mode) {
                            Ok(stream)=>{replacement=Some(stream);break;}
                            Err(error)=>android_log(&format!("output reconnect attempt={failures}: {error}")),
                        }
                    }
                    let Some(stream)=replacement else {
                        if !telemetry.shutdown_requested.load(Ordering::Acquire) {
                            telemetry.output_device_error.store(error,Ordering::Release);
                            android_log("AAudio output reconnect exhausted");
                        }
                        telemetry.output_device_recovering.store(false,Ordering::Release);
                        return;
                    };
                    prepared=stream;
                    telemetry.output_device_restarts.fetch_add(1,Ordering::Relaxed);
                    android_log("AAudio output reconnected; decoder, FIFO and presentation clock retained");
                }
                Err(error)=>{
                    telemetry.output_device_error.store(error,Ordering::Release);
                    telemetry.output_device_recovering.store(false,Ordering::Release);
                    android_log(&format!("AAudio output failed: {error}"));
                    break;
                }
            }
        }
    }
}

fn bind_stream(prepared:&PreparedStream,fifo:&Arc<stereo_fifo::StereoFifo>,telemetry:&Arc<RuntimeTelemetry>) {
    telemetry.output_device_recovering.store(false,Ordering::Release);
    let _=prepared.callback.fifo.set(fifo.clone());
    let _=prepared.callback.telemetry.set(telemetry.clone());
    telemetry.output_device_burst_frames.store(prepared.burst as u64,Ordering::Release);
    telemetry.output_device_mode.store(if prepared.callback_mode {2}else if cfg!(feature="pcm-diagnostic"){1}else{3},Ordering::Release);
    telemetry.output_device_buffer_frames.store(unsafe{aaudio::AAudioStream_getBufferSizeInFrames(prepared.stream.0)}.max(0) as u64,Ordering::Relaxed);
    telemetry.output_device_performance_mode.store(unsafe{aaudio::AAudioStream_getPerformanceMode(prepared.stream.0)},Ordering::Relaxed);
    telemetry.output_device_error.store(0,Ordering::Release);
    // A newly started stream produces silence until both shared Arcs are bound.
    // If it disconnected before binding, keep recovery set for the host.
    if prepared.callback.error.load(Ordering::Acquire)==DISCONNECTED {
        telemetry.output_device_recovering.store(true,Ordering::Release);
    }
    prepared.callback.ready.store(true,Ordering::Release);
}

#[derive(Default)]
struct PendingWrite { block:Vec<f32>, popped:usize, offset:usize }
fn monitor_callback_stream(prepared:&PreparedStream,telemetry:&Arc<RuntimeTelemetry>,xrun_base:u64)->Result<(),i32>{
    let stream=prepared.stream.0;
    let capacity=unsafe{aaudio::AAudioStream_getBufferCapacityInFrames(stream)}.max(1) as usize;
    let current=unsafe{aaudio::AAudioStream_getBufferSizeInFrames(stream)}.max(1) as usize;
    let mut policy=crate::android_output_policy::BufferPolicy::new(prepared.burst,capacity,current);
    telemetry.output_device_buffer_frames.store(current as u64,Ordering::Relaxed);
    telemetry.output_device_performance_mode.store(unsafe{aaudio::AAudioStream_getPerformanceMode(stream)},Ordering::Relaxed);
    let mut last_report=Instant::now();
    let observed=Instant::now();let mut watchdog=crate::android_output_policy::CallbackWatchdog::default();
    while !telemetry.shutdown_requested.load(Ordering::Acquire){
        let error=prepared.callback.error.load(Ordering::Acquire);
        if error<0 {return Err(error);}
        if watchdog.stalled(observed.elapsed().as_millis() as u64,
            prepared.callback.callbacks.load(Ordering::Relaxed),!telemetry.paused.load(Ordering::Acquire)) {
            telemetry.output_device_recovering.store(true,Ordering::Release);
            android_log(&format!("AAudio callback stalled state={} callbacks={} fifo={} consumed={}",
                unsafe{aaudio::AAudioStream_getState(stream)},prepared.callback.callbacks.load(Ordering::Relaxed),
                prepared.callback.fifo.get().map_or(0,|f|f.available_read()),
                telemetry.callback_consumed_sample_pos.load(Ordering::Acquire)));
            return Err(CALLBACK_STALLED);
        }
        let xruns=unsafe{aaudio::AAudioStream_getXRunCount(stream)};
        if xruns>=0 {telemetry.output_device_xruns_supported.store(true,Ordering::Relaxed);telemetry.output_device_xruns.store(xrun_base+xruns as u64,Ordering::Relaxed);}
        if let Some(request)=policy.observe_xruns(xruns){
            let accepted=unsafe{aaudio::AAudioStream_setBufferSizeInFrames(stream,request as i32)};
            if accepted>0 {policy.accepted(accepted as usize);telemetry.output_device_buffer_frames.store(accepted as u64,Ordering::Relaxed);}
        }
        if last_report.elapsed()>=Duration::from_secs(5){
            android_log(&format!("callback_progress audio_frames={} hardware_xruns={xruns} buffer_frames={} fifo_frames={}",
                telemetry.callback_consumed_sample_pos.load(Ordering::Acquire),telemetry.output_device_buffer_frames.load(Ordering::Relaxed),
                prepared.callback.fifo.get().map_or(0,|fifo|fifo.available_read())));
            last_report=Instant::now();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe{aaudio::AAudioStream_requestStop(stream);}
    Ok(())
}

fn writer_loop(
    prepared: &PreparedStream,
    fifo: &Arc<stereo_fifo::StereoFifo>,
    telemetry: &Arc<RuntimeTelemetry>,
    pending: &mut PendingWrite,
    xrun_base: u64,
) -> Result<(), i32> {
    let stream=&prepared.stream;
    let frames_per_burst=prepared.burst;
    let mut total_written = 0_u64;
    #[cfg(feature = "pcm-diagnostic")]
    let mut capture = PcmCapture::requested();
    let mut total_audio = 0_u64;
    let mut partial_writes = 0_u64;
    let mut last_report = Instant::now();
    while !telemetry.shutdown_requested.load(Ordering::Acquire) {
        if fifo.apply_flush_from_consumer() {pending.block.clear();}
        let error=prepared.callback.error.load(Ordering::Acquire);
        if error<0 {return Err(error);}
        let enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
        // A partial write survives a device replacement, including a different burst size.
        if pending.block.is_empty() {
            pending.block.resize(frames_per_burst*CHANNELS,0.);
            pending.popped=if enabled {fifo.pop_into_f32(&mut pending.block,CHANNELS)}else{0};
            pending.offset=0;
            #[cfg(feature = "pcm-diagnostic")]
            if let Some(capture) = &mut capture {capture.push(total_audio,&pending.block[..pending.popped*CHANNELS]);}
        }
        let started = Instant::now();
        let mut last_write=Instant::now();
        while pending.offset < pending.block.len()/CHANNELS {
            if telemetry.shutdown_requested.load(Ordering::Acquire) {return Ok(());}
            if fifo.apply_flush_from_consumer() {break;}
            let requested = pending.block.len()/CHANNELS - pending.offset;
            let written = unsafe {
                aaudio::AAudioStream_write(
                    stream.0, pending.block[pending.offset * CHANNELS..].as_ptr().cast(),
                    requested as i32, WRITE_TIMEOUT_NS,
                )
            };
            if written < 0 || written as usize > requested {
                if written==DISCONNECTED {telemetry.output_device_recovering.store(true,Ordering::Release);}
                return Err(if written<0 {written}else{-1});
            }
            if written == 0 {
                if last_write.elapsed() > Duration::from_secs(2) {
                    telemetry.output_device_recovering.store(true,Ordering::Release);
                    return Err(WRITE_STALLED);
                }
                continue;
            }
            last_write=Instant::now();
            let written = written as usize;
            partial_writes += u64::from(written < requested);
            let consumed = pending.popped.saturating_sub(pending.offset).min(written);
            record_callback(&telemetry, started, written, consumed, enabled);
            total_written += written as u64;
            total_audio += consumed as u64;
            pending.offset += written;
        }
        pending.block.clear();
        if last_report.elapsed() >= Duration::from_secs(5) {
            let xruns = unsafe { aaudio::AAudioStream_getXRunCount(stream.0) };
            if xruns>=0 {telemetry.output_device_xruns_supported.store(true,Ordering::Relaxed);telemetry.output_device_xruns.store(xrun_base+xruns as u64,Ordering::Relaxed);}
            android_log(&format!(
                "write_progress format=PCM_FLOAT frames={total_written} audio_frames={total_audio} partial_writes={partial_writes} xruns={xruns}",
            ));
            last_report = Instant::now();
        }
    }
    Ok(())
}

// Opt-in diagnostic builds only. Keep the actual application's rendered PCM
// before AAudio; never perform file IO in the writer's real-time loop.
#[cfg(feature = "pcm-diagnostic")]
struct PcmCapture { start: u64, end: u64, samples: Option<Vec<f32>> }
#[cfg(feature = "pcm-diagnostic")]
impl PcmCapture {
    fn requested() -> Option<Self> {
        unsafe extern "C" { fn __system_property_get(name: *const std::ffi::c_char, value: *mut std::ffi::c_char) -> i32; }
        let mut bytes = [0 as std::ffi::c_char; 92];
        let len = unsafe { __system_property_get(c"debug.sda.pcm_capture".as_ptr(), bytes.as_mut_ptr()) };
        if len <= 0 { return None; }
        let value = unsafe { std::ffi::CStr::from_ptr(bytes.as_ptr()) }.to_str().ok()?;
        let (start, end) = value.split_once(',')?;
        let start = start.parse::<u64>().ok()?;
        let end = end.parse::<u64>().ok()?;
        if start >= end || end > 3600 || end-start > 60 { return None; }
        android_log(&format!("PCM diagnostic armed seconds={start}..{end}"));
        Some(Self { start: start*48000, end: end*48000, samples: Some(Vec::with_capacity(((end-start)*48000*2) as usize)) })
    }
    fn push(&mut self, at: u64, block: &[f32]) {
        let Some(samples) = &mut self.samples else { return; };
        let end = at + (block.len()/2) as u64;
        let first = self.start.max(at);
        let last = self.end.min(end);
        if first < last { samples.extend_from_slice(&block[((first-at)*2) as usize..((last-at)*2) as usize]); }
        if end >= self.end {
            let samples = self.samples.take().unwrap();
            std::thread::spawn(move || {
                use std::io::Write;
                let result = (|| -> std::io::Result<()> {
                    let file = std::fs::File::create("/sdcard/Android/data/app.sda.mobile/files/render-capture.f32")?;
                    let mut out = std::io::BufWriter::new(file);
                    for sample in &samples { out.write_all(&sample.to_le_bytes())?; }
                    out.flush()
                })();
                android_log(&format!("PCM diagnostic frames={} result={result:?}", samples.len()/2));
            });
        }
    }
}
