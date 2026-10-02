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
    #[test]
    fn source_abi_preserves_pcm_and_subframe_metadata() {
        let bytes = include_bytes!("../../../packages/core/mpegh/fixtures/motion.mhas");
        let expected = {
            let mut decoder = mpegh::MpeghDecoder::new().unwrap();
            decoder.push(bytes).unwrap(); decoder.flush().unwrap();
            let mut frames = vec![];
            while let Some(f) = decoder.next_frame() {
                frames.push(json!({"sampleRate":f.sample_rate,"samplePos":f.sample_pos,
                    "channels":f.channels,"labels":f.labels,"bedLabels":f.raw_bed_labels,
                    "objects":f.object_channels,"events":f.events}));
            }
            // Canonicalize through the same JSON boundary as the C ABI.
            serde_json::from_str::<Vec<Value>>(&serde_json::to_string(&frames).unwrap()).unwrap()
        };
        unsafe fn value(p: *mut c_char) -> Value {
            let v: Value = serde_json::from_str(unsafe { CStr::from_ptr(p) }.to_str().unwrap()).unwrap();
            unsafe { sda_ios_string_free(p); }
            assert_eq!(v["ok"],true,"{v}"); v["value"].clone()
        }
        unsafe {
            let mut error = std::ptr::null_mut();
            let host = sda_ios_sources_create(&mut error);
            assert!(!host.is_null()); assert!(error.is_null());
            struct Close(*mut std::ffi::c_void);
            impl Drop for Close { fn drop(&mut self) { unsafe { sda_ios_sources_close(self.0); } } }
            let _close = Close(host);
            assert!(mpegh::MpeghDecoder::new_7_1_4().is_err());
            let mut actual = vec![];
            for chunk in bytes.chunks(997) {
                value(sda_ios_sources_feed(host,chunk.as_ptr(),chunk.len(),false));
                loop {
                    let frame = value(sda_ios_sources_next(host));
                    if frame.is_null() { break; }
                    actual.push(frame);
                }
            }
            assert_eq!(value(sda_ios_sources_feed(host,std::ptr::null(),0,true))["queuedFrames"],0);
            assert!(actual == expected, "source PCM/OAM differs across chunk boundaries");
            assert!(actual.iter().all(|f| f["objects"].as_array().unwrap().len()==2));
            assert!(actual.iter().map(|f| f["events"].as_array().unwrap().len()).sum::<usize>() > 2);
            let p = sda_ios_sources_feed(host,std::ptr::null(),0,false);
            let rejected: Value = serde_json::from_str(CStr::from_ptr(p).to_str().unwrap()).unwrap();
            sda_ios_string_free(p); assert_eq!(rejected["ok"],false);
        }
    }

    #[test]
    fn speaker_abi_preserves_twelve_channel_interleaving_and_drains() {
        let bytes = include_bytes!("../../../packages/core/mpegh/fixtures/motion.mhas");
        let expected = {
            let mut d = mpegh::MpeghDecoder::new_7_1_4().unwrap();
            d.push(bytes).unwrap(); d.flush().unwrap();
            let mut expected = vec![];
            while let Some(f) = d.next_frame() { for i in 0..f.channels[0].len() { for c in &f.channels { expected.push(c[i]); } } }
            expected
        };
        unsafe fn value(p: *mut c_char) -> Value {
            let v: Value = serde_json::from_str(unsafe { CStr::from_ptr(p) }.to_str().unwrap()).unwrap();
            unsafe { sda_ios_string_free(p); }
            assert_eq!(v["ok"], true, "{v}"); v["value"].clone()
        }
        unsafe {
            let mut error = std::ptr::null_mut();
            let h = sda_ios_speakers_create(&mut error);
            assert!(!h.is_null()); assert!(error.is_null());
            struct Close(*mut std::ffi::c_void);
            impl Drop for Close { fn drop(&mut self) { unsafe { sda_ios_speakers_close(self.0); } } }
            let _close = Close(h);
            let mut pcm = [0.0; 997*12]; let mut actual = vec![];
            for chunk in bytes.chunks(1024) {
                let result = value(sda_ios_speakers_feed(h,chunk.as_ptr(),chunk.len(),false));
                assert_eq!(result["channels"],12);
                loop {
                    let n = sda_ios_speakers_read(h,pcm.as_mut_ptr(),997);
                    if n == 0 { break; }
                    actual.extend_from_slice(&pcm[..n*12]);
                }
            }
            assert_eq!(value(sda_ios_speakers_feed(h,std::ptr::null(),0,true))["queuedFrames"],0);
            assert_eq!(actual,expected); assert!(!actual.is_empty());
            let host = &mut *h.cast::<SpeakerHost>();
            assert!(!host.events.is_empty());
            let first = host.events.front().unwrap().sample_pos;
            if first > 0 { assert_eq!(value(sda_ios_speakers_objects(h,first-1)),json!({})); }
            let objects = value(sda_ios_speakers_objects(h,first));
            assert_eq!(objects.as_object().unwrap().len(),2);
            assert!(objects.as_object().unwrap().values().all(|v| v["hasPos"]==true && v["samplePos"].as_u64().unwrap()<=first));
            let final_objects = value(sda_ios_speakers_objects(h,u64::MAX));
            assert_eq!(final_objects.as_object().unwrap().len(),2);
            assert!((*h.cast::<SpeakerHost>()).events.is_empty());
        }
    }

    #[test]
    fn speaker_balance_is_uniform_smooth_and_toggle_does_not_reset() {
        unsafe {
            let mut error = std::ptr::null_mut();
            let h = sda_ios_speakers_create(&mut error);
            assert!(!h.is_null());
            let host = &mut *h.cast::<SpeakerHost>();
            host.target_gain = 0.5;
            host.pcm.extend(std::iter::repeat_n(0.8, 1024*12));
            sda_ios_speakers_balance(h,true);
            let mut out = [0.0;1024*12];
            assert_eq!(sda_ios_speakers_read(h,out.as_mut_ptr(),1024),1024);
            for frame in out.chunks_exact(12) { assert!(frame.iter().all(|v| *v==frame[0])); }
            assert_eq!(out[0], 0.4);
            assert_eq!(out[1023*12], out[0]); // Balanced from sample zero, no loud startup ramp.
            let before = (*h.cast::<SpeakerHost>()).gain;
            sda_ios_speakers_balance(h,false);
            assert_eq!((*h.cast::<SpeakerHost>()).gain,before);
            (*h.cast::<SpeakerHost>()).pcm.extend(std::iter::repeat_n(0.8,12));
            assert_eq!(sda_ios_speakers_read(h,out.as_mut_ptr(),1),1);
            assert!(out[0] > before*0.8 && out[0] <= 0.8);
            sda_ios_speakers_close(h);
        }
    }

}

// Separate speaker PCM ABI. Never enters MobileEngine, HRTF, room or stereo FIFO.
struct SpeakerHost {
    decoder: mpegh::MpeghDecoder,
    pcm: std::collections::VecDeque<f32>,
    events: std::collections::VecDeque<sda_core::ObjectEvent>,
    active: crate::ObjectSnapshot,
    meter: crate::balance::LoudnessMeter,
    balance_enabled: bool,
    measured_frames: usize,
    target_gain: f32,
    gain: f32,
    read_started: bool,
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_create(error: *mut *mut c_char) -> *mut std::ffi::c_void {
    let result = std::panic::catch_unwind(|| mpegh::MpeghDecoder::new_7_1_4()).unwrap_or_else(|_| Err("speaker decoder panic".into()));
    match result {
        Ok(decoder) => Box::into_raw(Box::new(SpeakerHost { decoder, pcm: Default::default(), events: Default::default(), active: Default::default(),
            meter: crate::balance::LoudnessMeter::speakers_7_1_4(), balance_enabled: false, measured_frames: 0, target_gain: 1.0, gain: 1.0, read_started: false })).cast(),
        Err(e) => { if !error.is_null() { unsafe { *error = reply(Err(e)); } } std::ptr::null_mut() }
    }
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_feed(p: *mut std::ffi::c_void, bytes: *const u8, len: usize, finish: bool) -> *mut c_char {
    guarded(|| {
        if p.is_null() || (len > 0 && bytes.is_null()) || len > 65536 { return Err("invalid speaker input".into()); }
        let host = unsafe { &mut *p.cast::<SpeakerHost>() };
        if finish { host.decoder.flush()?; }
        else { host.decoder.push(if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(bytes, len) } })?; }
        while let Some(frame) = host.decoder.next_frame() {
            let frames = frame.channels[0].len();
            if host.events.len() + frame.events.len() > 262144 { return Err("speaker object timeline backlog exceeded".into()); }
            host.events.extend(frame.events);
            host.events.make_contiguous().sort_by_key(|e| e.sample_pos);
            host.meter.push(&frame.channels);
            host.measured_frames += 1;
            if !host.read_started || host.measured_frames % 8 == 0 {
                let m = host.meter.integrated();
                if m.blocks >= crate::balance::MIN_BLOCKS {
                    if let Some(lufs) = m.integrated_lufs { host.target_gain = 10_f64.powf(crate::balance::master_gain(lufs, m.true_peak_dbtp)/20.0) as f32; }
                }
            }
            if host.pcm.len() + frames*12 > 8*48000*12 { return Err("speaker PCM backlog exceeded".into()); }
            for i in 0..frames { for channel in &frame.channels { host.pcm.push_back(channel[i]); } }
        }
        if finish {
            let m = host.meter.integrated();
            if let Some(lufs) = m.integrated_lufs {
                host.target_gain = 10_f64.powf(crate::balance::master_gain(lufs, m.true_peak_dbtp)/20.0) as f32;
            }
        }
        Ok(json!({"queuedFrames":host.pcm.len()/12,"channels":12,"sampleRate":48000}))
    })
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_read(p: *mut std::ffi::c_void, out: *mut f32, capacity_frames: usize) -> usize {
    if p.is_null() || out.is_null() || capacity_frames > 4096 { return 0; }
    let host = unsafe { &mut *p.cast::<SpeakerHost>() };
    let frames = capacity_frames.min(host.pcm.len()/12);
    if frames > 0 && !host.read_started {
        host.gain = if host.balance_enabled { host.target_gain } else { 1.0 };
        host.read_started = true;
    }
    for i in 0..frames {
        let target = if host.balance_enabled { host.target_gain } else { 1.0 };
        // A single smooth gain for all 12 channels, including LFE. No remapping.
        host.gain += (target - host.gain).clamp(-1.0/12000.0, 1.0/12000.0);
        for ch in 0..12 { unsafe { *out.add(i*12+ch) = host.pcm.pop_front().unwrap_or(0.0) * host.gain; } }
    }
    frames
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_objects(p: *mut std::ffi::c_void, clock: u64) -> *mut c_char {
    guarded(|| {
        if p.is_null() { return Err("invalid speaker handle".into()); }
        let host = unsafe { &mut *p.cast::<SpeakerHost>() };
        serde_json::to_value(crate::snapshot_at_clock(&mut host.events, &mut host.active, clock)).map_err(|e| e.to_string())
    })
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_balance(p: *mut std::ffi::c_void, enabled: bool) {
    if !p.is_null() { unsafe { (*p.cast::<SpeakerHost>()).balance_enabled = enabled; } }
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_close(p: *mut std::ffi::c_void) {
    if !p.is_null() { drop(unsafe { Box::from_raw(p.cast::<SpeakerHost>()) }); }
}


// Experimental source-frame ABI, control thread only. No HRTF or speaker rendering.
// Caller serializes all calls and owns exactly one decoder; JSON is never used on RT.
struct SourceHost {
    decoder: mpegh::MpeghDecoder,
    frames: std::collections::VecDeque<sda_core::FrameData>,
    queued: usize,
    finished: bool,
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_sources_create(error: *mut *mut c_char) -> *mut std::ffi::c_void {
    let result = std::panic::catch_unwind(mpegh::MpeghDecoder::new)
        .unwrap_or_else(|_| Err("source decoder panic".into()));
    match result {
        Ok(decoder) => Box::into_raw(Box::new(SourceHost { decoder, frames: Default::default(), queued: 0, finished: false })).cast(),
        Err(e) => { if !error.is_null() { unsafe { *error = reply(Err(e)); } } std::ptr::null_mut() }
    }
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_sources_feed(p: *mut std::ffi::c_void, bytes: *const u8, len: usize, finish: bool) -> *mut c_char {
    guarded(|| {
        if p.is_null() || (len > 0 && bytes.is_null()) || len > 65536 || (finish && len != 0) {
            return Err("invalid source input".into());
        }
        let host = unsafe { &mut *p.cast::<SourceHost>() };
        if host.finished { return Err("source decoder already finished".into()); }
        if finish { host.decoder.flush()?; host.finished = true; }
        else { host.decoder.push(if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(bytes, len) } })?; }
        while let Some(frame) = host.decoder.next_frame() {
            if frame.sample_rate != 48000 || frame.channels.is_empty() || frame.channels.len() > 64 {
                return Err("invalid source format".into());
            }
            let n = frame.channels[0].len();
            if n == 0 || n > 4096 || frame.channels.iter().any(|c| c.len() != n || c.iter().any(|v| !v.is_finite())) {
                return Err("invalid source PCM".into());
            }
            if host.queued + n > 8*48000 { return Err("source PCM backlog exceeded".into()); }
            host.queued += n;
            host.frames.push_back(frame);
        }
        Ok(json!({"queuedFrames":host.queued,"finished":host.finished}))
    })
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_sources_next(p: *mut std::ffi::c_void) -> *mut c_char {
    guarded(|| {
        let host = unsafe { p.cast::<SourceHost>().as_mut() }.ok_or("source decoder unavailable")?;
        let Some(frame) = host.frames.pop_front() else { return Ok(Value::Null); };
        host.queued -= frame.channels[0].len();
        Ok(json!({"sampleRate":frame.sample_rate,"samplePos":frame.sample_pos,
            "channels":frame.channels,"labels":frame.labels,"bedLabels":frame.raw_bed_labels,
            "objects":frame.object_channels,"events":frame.events}))
    })
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_sources_close(p: *mut std::ffi::c_void) {
    if !p.is_null() { drop(unsafe { Box::from_raw(p.cast::<SourceHost>()) }); }
}
