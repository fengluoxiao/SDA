//! iOS C boundary. Control calls serialized; callback uses only the SPSC FIFO.
//! Host must stop/detach AVAudioSourceNode before closing its opaque handle.
use crate::*;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::sync::{atomic::Ordering, OnceLock};
#[derive(Default)]
struct PullOutput {
    state: OnceLock<(Arc<stereo_fifo::StereoFifo>, Arc<RuntimeTelemetry>)>,
}
impl AudioOutput for PullOutput {
    fn run(
        self: Arc<Self>,
        fifo: Arc<stereo_fifo::StereoFifo>,
        telemetry: Arc<RuntimeTelemetry>,
        _: Arc<render_command::RenderCommandQueue>,
    ) {
        let _ = self.state.set((fifo, telemetry.clone()));
        while !telemetry.shutdown_requested.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
struct Host {
    engine: Mutex<MobileEngine>,
    output: Arc<PullOutput>,
}
fn reply(result: EngineResult<Value>) -> *mut c_char {
    let value = match result {
        Ok(v) => json!({"ok":true,"value":v}),
        Err(e) => json!({"ok":false,"error":e}),
    };
    CString::new(value.to_string()).unwrap().into_raw()
}
unsafe fn text<'a>(p: *const c_char) -> EngineResult<&'a str> {
    if p.is_null() {
        return Err("null string".into());
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|e| e.to_string())
}
fn guarded(f: impl FnOnce() -> EngineResult<Value>) -> *mut c_char {
    reply(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
            .unwrap_or_else(|_| Err("native engine panic".into())),
    )
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_string_free(p: *mut c_char) {
    if !p.is_null() {
        drop(unsafe { CString::from_raw(p) });
    }
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_create(
    config: *const c_char,
    hrtf: *const c_char,
    error: *mut *mut c_char,
) -> *mut std::ffi::c_void {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> EngineResult<Host> {
            let config: EngineConfig =
                serde_json::from_str(unsafe { text(config)? }).map_err(|e| e.to_string())?;
            if config.sample_rate != 48000 || config.output_channels != 2 {
                return Err("iOS requires 48 kHz stereo".into());
            }
            let mut engine = MobileEngine::new(config, None)?;
            engine.load_hrtf(unsafe { text(hrtf)? })?;
            let output = Arc::new(PullOutput::default());
            engine.start(output.clone())?;
            let start = Instant::now();
            while output.state.get().is_none() {
                if start.elapsed() > Duration::from_secs(5) {
                    engine.stop();
                    return Err("output attachment timed out".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(Host {
                engine: Mutex::new(engine),
                output,
            })
        }))
        .unwrap_or_else(|_| Err("native initialization panic".into()));
    match result {
        Ok(host) => Box::into_raw(Box::new(host)).cast(),
        Err(e) => {
            if !error.is_null() {
                unsafe {
                    *error = reply(Err(e));
                }
            }
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_command(
    p: *mut std::ffi::c_void,
    op: *const c_char,
    args: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let host = unsafe { (p as *mut Host).as_ref() }.ok_or("engine unavailable")?;
        let mut e = host.engine.lock().map_err(|_| "engine poisoned")?;
        let a: Value = serde_json::from_str(unsafe { text(args)? }).map_err(|e| e.to_string())?;
        let str_arg =
            |k: &str| -> EngineResult<&str> { a[k].as_str().ok_or_else(|| format!("missing {k}")) };
        let number = |k: &str| -> EngineResult<f32> {
            a[k].as_f64()
                .filter(|v| v.is_finite())
                .map(|v| v as f32)
                .ok_or_else(|| format!("invalid {k}"))
        };
        let boolean = |k: &str| -> EngineResult<bool> {
            a[k].as_bool().ok_or_else(|| format!("missing {k}"))
        };
        match unsafe { text(op)? } {
            "status" => {
                return serde_json::to_value(e.playback_status()).map_err(|e| e.to_string())
            }
            "objects" => {
                return serde_json::to_value(e.object_snapshot()).map_err(|e| e.to_string())
            }
            "pause" => e.set_paused(boolean("paused")?)?,
            "volume" => e.set_volume(number("volume")?)?,
            "balance" => e.set_volume_balance(boolean("enabled")?)?,
            "measured" => e.set_measured_loudness(str_arg("json")?)?,
            "loudness" => return Ok(json!(e.complete_loudness_json())),
            "yaw" => e.set_head_yaw_degrees(number("degrees")?)?,
            "resetPose" => e.reset_head_pose()?,
            "near" => e.set_near_field(boolean("enabled")?, number("scale")?)?,
            "room" => e.set_room(str_arg("path")?)?,
            "rendering" => e.set_object_rendering(boolean("direct")?, boolean("directional")?)?,
            "preset" => e.set_hrtf_preset(
                str_arg("path")?,
                number("wet")?,
                boolean("direct")?,
                boolean("directional")?,
            )?,
            "mp3" => {
                e.open_mp3(str_arg("path")?)?;
            }
            "pullMp3" => {
                let (frames, eof) = e.pull_mp3(4096)?;
                return Ok(json!({"frames":frames,"eof":eof}));
            }
            "mpegh" => e.open_mpegh()?,
            "finish" => return serde_json::to_value(e.finish()?).map_err(|e| e.to_string()),
            other => return Err(format!("unknown command {other}")),
        }
        Ok(Value::Null)
    })
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_feed(
    p: *mut std::ffi::c_void,
    bytes: *const u8,
    len: usize,
) -> *mut c_char {
    guarded(|| {
        if bytes.is_null() || len > 1024 * 1024 {
            return Err("invalid feed buffer".into());
        }
        let host = unsafe { (p as *mut Host).as_ref() }.ok_or("engine unavailable")?;
        let mut e = host.engine.lock().map_err(|_| "engine poisoned")?;
        serde_json::to_value(e.feed(unsafe { std::slice::from_raw_parts(bytes, len) })?)
            .map_err(|e| e.to_string())
    })
}
fn pull(output: &PullOutput, left: &mut [f32], right: &mut [f32]) -> usize {
    left.fill(0.0);
    right.fill(0.0);
    let Some((fifo, t)) = output.state.get() else {
        return 0;
    };
    fifo.apply_flush_from_consumer();
    let enabled = t.callback_output_enabled.load(Ordering::Acquire)
        && !t.shutdown_requested.load(Ordering::Acquire);
    let started = Instant::now();
    let mut popped = 0;
    if enabled {
        let mut scratch = [0.0_f32; 1024];
        for offset in (0..left.len()).step_by(512) {
            let n = (left.len() - offset).min(512);
            let count = fifo.pop_into_f32(&mut scratch[..n * 2], 2);
            for i in 0..n {
                left[offset + i] = scratch[i * 2];
                right[offset + i] = scratch[i * 2 + 1];
            }
            popped += count;
        }
    }
    sda_native_renderer::record_callback(t, started, left.len(), popped, enabled);
    popped
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_render(
    p: *mut std::ffi::c_void,
    left: *mut f32,
    right: *mut f32,
    frames: usize,
) {
    if p.is_null() || left.is_null() || right.is_null() || frames > 131072 {
        return;
    }
    let host = unsafe { &*(p as *const Host) };
    pull(
        &host.output,
        unsafe { std::slice::from_raw_parts_mut(left, frames) },
        unsafe { std::slice::from_raw_parts_mut(right, frames) },
    );
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_close(p: *mut std::ffi::c_void) {
    if !p.is_null() {
        let host = unsafe { Box::from_raw(p as *mut Host) };
        if let Ok(mut engine) = host.engine.lock() {
            engine.stop();
        };
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_preserves_stereo_pause_flush_and_clock() {
        let out = PullOutput::default();
        let fifo = Arc::new(stereo_fifo::StereoFifo::new(2048));
        let t = Arc::new(RuntimeTelemetry::default());
        out.state.set((fifo.clone(), t.clone())).ok().unwrap();
        let mut l = [9.; 700];
        let mut r = [9.; 700];
        fifo.push(&[0.2, -0.3, 0.4, -0.5]);
        assert_eq!(pull(&out, &mut l, &mut r), 0);
        assert_eq!(fifo.available_read(), 2);
        assert_eq!(l, [0.; 700]);
        t.callback_output_enabled.store(true, Ordering::Release);
        assert_eq!(pull(&out, &mut l, &mut r), 2);
        assert_eq!(&l[..3], &[0.2, 0.4, 0.]);
        assert_eq!(&r[..3], &[-0.3, -0.5, 0.]);
        assert_eq!(t.callback_consumed_sample_pos.load(Ordering::Acquire), 2);
        fifo.push(&[0.9, 0.8]);
        let epoch = fifo.clear_from_producer();
        assert_eq!(pull(&out, &mut l, &mut r), 0);
        assert!(fifo.flush_acknowledged(epoch));
        assert_eq!(l, [0.; 700]);
    }
    #[test]
    fn null_commands_return_owned_error_json() {
        let p =
            unsafe { sda_ios_command(std::ptr::null_mut(), c"status".as_ptr(), c"{}".as_ptr()) };
        let s = unsafe { CStr::from_ptr(p) }.to_str().unwrap();
        assert!(s.contains("engine unavailable"));
        unsafe {
            sda_ios_string_free(p);
        }
    }
    #[test]
    fn compressed_tones_reach_stereo_callback_and_preset_keeps_clock() {
        unsafe fn value(p: *mut c_char) -> Value {
            let result: Value =
                serde_json::from_str(unsafe { CStr::from_ptr(p) }.to_str().unwrap()).unwrap();
            unsafe { sda_ios_string_free(p) };
            assert_eq!(result["ok"], true, "{result}");
            result["value"].clone()
        }
        unsafe fn command(h: *mut std::ffi::c_void, op: &str, args: Value) -> Value {
            let op = CString::new(op).unwrap();
            let args = CString::new(args.to_string()).unwrap();
            unsafe { value(sda_ios_command(h, op.as_ptr(), args.as_ptr())) }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("apps/web/public/hrtf-dense/hrtf-set.json");
        let path = CString::new(path.to_str().unwrap()).unwrap();
        let config = CString::new(json!({"sampleRate":48000,"outputChannels":2,"layout":"7.1.4","directObjectHrtf":true,"directionalHrtf":true}).to_string()).unwrap();
        let mut error = std::ptr::null_mut();
        let h = unsafe { sda_ios_create(config.as_ptr(), path.as_ptr(), &mut error) };
        if h.is_null() {
            unsafe { value(error) };
            panic!("create failed");
        }
        // RAII closes only after the simulated callback has stopped.
        struct Close(*mut std::ffi::c_void);
        impl Drop for Close {
            fn drop(&mut self) {
                unsafe { sda_ios_close(self.0) };
            }
        }
        let _close = Close(h);
        let bytes = include_bytes!("../tests/fixtures/ios-stereo-tones.eac3");
        for bytes in bytes.chunks(4096) {
            let result = unsafe { value(sda_ios_feed(h, bytes.as_ptr(), bytes.len())) };
            assert_eq!(result["errors"], json!([]));
        }
        unsafe { command(h, "finish", json!({})) };
        let decoded = unsafe { command(h, "status", json!({})) }["decodedSamplePos"]
            .as_u64()
            .unwrap();
        assert!(decoded >= 46080, "decoded={decoded}");
        let mut left = [0f32; 1024];
        let mut right = [0f32; 1024];
        let mut energy = 0f64;
        let mut difference = 0f64;
        let mut switched = false;
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut previous = 0;
        loop {
            unsafe { sda_ios_render(h, left.as_mut_ptr(), right.as_mut_ptr(), left.len()) };
            for (l, r) in left.iter().zip(&right) {
                assert!(l.is_finite() && r.is_finite());
                energy += (*l as f64).powi(2) + (*r as f64).powi(2);
                difference += (*l as f64 - *r as f64).powi(2);
            }
            let status = unsafe { command(h, "status", json!({})) };
            let clock = status["consumedSamplePos"].as_u64().unwrap();
            assert!(clock >= previous);
            previous = clock;
            if !switched && clock > 0 {
                unsafe {
                    command(
                        h,
                        "preset",
                        json!({"path":path.to_str().unwrap(),"wet":0.0,"direct":true,"directional":true}),
                    )
                };
                let after = unsafe { command(h, "status", json!({})) };
                assert_eq!(after["decodedSamplePos"], decoded);
                assert_eq!(after["consumedSamplePos"], clock);
                switched = true;
            }
            if clock >= decoded {
                break;
            }
            assert!(Instant::now() < deadline, "callback stalled: {status}");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(switched && energy > 0.001 && difference > 0.001);
    }
}
