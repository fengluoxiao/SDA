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
}
struct PreparedStream {
    // Drop/close the stream before freeing callback userdata and its Arcs.
    stream: Stream,
    callback: Box<CallbackContext>,
    burst: usize,
    callback_mode: bool,
}
unsafe extern "C" fn data_callback(_stream:*mut aaudio::AAudioStream,user:*mut std::ffi::c_void,audio:*mut std::ffi::c_void,frames:i32)->i32 {
    if user.is_null()||audio.is_null()||frames<=0 {return aaudio::AAUDIO_CALLBACK_RESULT_STOP as i32;}
    let context=unsafe{&*user.cast::<CallbackContext>()};
    let out=unsafe{std::slice::from_raw_parts_mut(audio.cast::<f32>(),frames as usize*CHANNELS)};
    if let (Some(fifo),Some(telemetry))=(context.fifo.get(),context.telemetry.get()) {
        if telemetry.shutdown_requested.load(Ordering::Acquire) {out.fill(0.);return aaudio::AAUDIO_CALLBACK_RESULT_STOP as i32;}
        crate::output_pull::pull(fifo,telemetry,out);
    } else {out.fill(0.);}
    aaudio::AAUDIO_CALLBACK_RESULT_CONTINUE as i32
}
unsafe extern "C" fn error_callback(_stream:*mut aaudio::AAudioStream,user:*mut std::ffi::c_void,error:i32) {
    if let Some(context)=unsafe{user.cast::<CallbackContext>().as_ref()} {
        context.error.store(error,Ordering::Release);
        if let Some(t)=context.telemetry.get(){t.output_device_error.store(error,Ordering::Release);}
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
        let callback_mode = !cfg!(feature="pcm-diagnostic");
        let stream = Self::open_stream(callback_mode).or_else(|first| {
            if !callback_mode {return Err(first);}
            android_log(&format!("callback output unavailable; trying blocking output: {first}"));
            Self::open_stream(false).map_err(|second|format!("AAudio callback failed: {first}; blocking fallback failed: {second}"))
        })?;
        Ok(Self { stream: Mutex::new(Some(stream)) })
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
        let prepared = self.stream.lock().ok().and_then(|mut stream| stream.take());
        let result = prepared
            .ok_or_else(|| "AAudio output stream was not prepared".to_string())
            .and_then(|prepared| {
                let _=prepared.callback.fifo.set(fifo.clone());
                let _=prepared.callback.telemetry.set(telemetry.clone());
                telemetry.output_device_burst_frames.store(prepared.burst as u64,Ordering::Release);
                telemetry.output_device_mode.store(if prepared.callback_mode {2}else if cfg!(feature="pcm-diagnostic"){1}else{3},Ordering::Release);
                telemetry.output_device_buffer_frames.store(unsafe{aaudio::AAudioStream_getBufferSizeInFrames(prepared.stream.0)}.max(0) as u64,Ordering::Relaxed);
                telemetry.output_device_performance_mode.store(unsafe{aaudio::AAudioStream_getPerformanceMode(prepared.stream.0)},Ordering::Relaxed);
                if !prepared.callback_mode {
                    crate::realtime::android_priority(-16);
                    // Diagnostic capture retains the existing blocking path;
                    // no PCM allocation/capture is performed by live callbacks.
                    writer_loop_with_context(prepared,fifo,telemetry)
                } else { monitor_callback_stream(prepared,telemetry) }
            });
        if let Err(error) = result {
            android_log(&error);
        }
    }
}

fn writer_loop_with_context(prepared:PreparedStream,fifo:Arc<stereo_fifo::StereoFifo>,telemetry:Arc<RuntimeTelemetry>)->Result<(),String>{
    writer_loop(&prepared.stream,prepared.burst,fifo,telemetry)
}
fn monitor_callback_stream(prepared:PreparedStream,telemetry:Arc<RuntimeTelemetry>)->Result<(),String>{
    let stream=prepared.stream.0;
    let capacity=unsafe{aaudio::AAudioStream_getBufferCapacityInFrames(stream)}.max(1) as usize;
    let current=unsafe{aaudio::AAudioStream_getBufferSizeInFrames(stream)}.max(1) as usize;
    let mut policy=crate::android_output_policy::BufferPolicy::new(prepared.burst,capacity,current);
    telemetry.output_device_buffer_frames.store(current as u64,Ordering::Relaxed);
    telemetry.output_device_performance_mode.store(unsafe{aaudio::AAudioStream_getPerformanceMode(stream)},Ordering::Relaxed);
    let mut last_report=Instant::now();
    while !telemetry.shutdown_requested.load(Ordering::Acquire){
        let error=prepared.callback.error.load(Ordering::Acquire);
        if error<0 {telemetry.output_device_error.store(error,Ordering::Release);return Err(format!("AAudio callback stream failed: {error}"));}
        let xruns=unsafe{aaudio::AAudioStream_getXRunCount(stream)};
        if xruns>=0 {telemetry.output_device_xruns_supported.store(true,Ordering::Relaxed);telemetry.output_device_xruns.store(xruns as u64,Ordering::Relaxed);}
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
    stream: &Stream,
    frames_per_burst: usize,
    fifo: Arc<stereo_fifo::StereoFifo>,
    telemetry: Arc<RuntimeTelemetry>,
) -> Result<(), String> {
    let mut block = vec![0.0_f32; frames_per_burst * CHANNELS];
    let mut total_written = 0_u64;
    #[cfg(feature = "pcm-diagnostic")]
    let mut capture = PcmCapture::requested();
    let mut total_audio = 0_u64;
    let mut partial_writes = 0_u64;
    let mut last_report = Instant::now();
    while !telemetry.shutdown_requested.load(Ordering::Acquire) {
        fifo.apply_flush_from_consumer();
        let enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
        let popped = if enabled {
            fifo.pop_into_f32(&mut block, CHANNELS)
        } else {
            block.fill(0.0);
            0
        };
        let started = Instant::now();
        #[cfg(feature = "pcm-diagnostic")]
        if let Some(capture) = &mut capture {
            capture.push(total_audio, &block[..popped * CHANNELS]);
        }
        let mut offset = 0;
        // AAudio returns frames, not samples or bytes. Retain unwritten
        // samples across short writes; never fetch a new FIFO block early.
        while offset < frames_per_burst {
            if telemetry.shutdown_requested.load(Ordering::Acquire) {
                return Ok(());
            }
            if fifo.apply_flush_from_consumer() {
                break;
            }
            let requested = frames_per_burst - offset;
            let written = unsafe {
                aaudio::AAudioStream_write(
                    stream.0, block[offset * CHANNELS..].as_ptr().cast(),
                    requested as i32, WRITE_TIMEOUT_NS,
                )
            };
            if written < 0 || written as usize > requested {
                return Err(format!("AAudioStream_write failed: {written}"));
            }
            if written == 0 {
                if started.elapsed() > Duration::from_secs(2) {
                    return Err("AAudioStream_write made no progress for 2 seconds".into());
                }
                continue;
            }
            let written = written as usize;
            partial_writes += u64::from(written < requested);
            let consumed = popped.saturating_sub(offset).min(written);
            record_callback(&telemetry, started, written, consumed, enabled);
            total_written += written as u64;
            total_audio += consumed as u64;
            offset += written;
        }
        if last_report.elapsed() >= Duration::from_secs(5) {
            let xruns = unsafe { aaudio::AAudioStream_getXRunCount(stream.0) };
            if xruns>=0 {telemetry.output_device_xruns_supported.store(true,Ordering::Relaxed);telemetry.output_device_xruns.store(xruns as u64,Ordering::Relaxed);}
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
        let mut bytes = [0i8; 92];
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
