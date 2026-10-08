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
    alac: Mutex<crate::alac_pcm::AlacPcm>,
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
                alac: Mutex::new(Default::default()),
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
            "masterPreamp" => e.set_master_preamp_db(number("gainDb")?)?,
            "spatialLayerGain" => e.set_spatial_layer_gain_db(number("gainDb")?)?,
            "spatialEnhancement" => e.set_spatial_enhancement(boolean("enabled")?)?,
            "balance" => e.set_volume_balance(boolean("enabled")?)?,
            "measured" => e.set_measured_loudness(str_arg("json")?)?,
            "loudness" => return Ok(json!(e.complete_loudness_json())),
            "yaw" => e.set_head_yaw_degrees(number("degrees")?)?,
            "resetPose" => e.reset_head_pose()?,
            "near" => e.set_near_field(boolean("enabled")?, number("scale")?)?,
            "room" => e.set_room(str_arg("path")?)?,
            "rendering" => e.set_object_rendering(boolean("direct")?, boolean("directional")?)?,
            "spatialCueGain" => e.set_spatial_cue_gain(str_arg("path")?, number("gain")?)?,
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
                let cue_path = root.join("apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json");
                let cue_path = CString::new(cue_path.to_str().unwrap()).unwrap();
                unsafe { command(h, "preset", json!({"path":cue_path.to_str().unwrap(),"wet":0.0,"direct":true,"directional":true})) };
                for gain in [1.0, 0.5011872, 0.25118864] {
                    let layout = CString::new("7.1.4").unwrap();
                    let mut error = std::ptr::null_mut();
                    let update = unsafe { sda_ios_prepare_cues(cue_path.as_ptr(), layout.as_ptr(), gain, &mut error) };
                    assert!(!update.is_null(), "cue preparation failed");
                    unsafe { value(sda_ios_apply_cues(h, update)); sda_ios_free_cues(update); }
                }
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
    fn speaker_full_track_balance_handles_silent_intro_from_first_audible_sample() {
        let mut meter = crate::balance::LoudnessMeter::speakers_7_1_4();
        let silence = vec![vec![0.0; 4800]; 12];
        for _ in 0..60 { meter.push(&silence); }
        // The old six-second queued-PCM gate releases here without a valid gain.
        assert_eq!(meter.integrated().blocks, 0);
        let loud: Vec<Vec<f32>> = (0..12).map(|_| (0..4800).map(|i| 0.8*(i as f32*std::f32::consts::TAU*1000.0/48000.0).sin()).collect()).collect();
        for _ in 0..40 { meter.push(&loud); }
        let m = meter.integrated();
        assert!(m.blocks < crate::balance::MIN_BLOCKS); // Complete short loud part is still valid.
        let gain = 10_f64.powf(crate::balance::master_gain(m.integrated_lufs.unwrap(),m.true_peak_dbtp)/20.0) as f32;
        assert!(gain < 0.5);
        unsafe {
            let mut error = std::ptr::null_mut();
            let h = sda_ios_speakers_create(&mut error);
            assert!(!h.is_null());
            let text = CString::new(serde_json::to_string(&m).unwrap()).unwrap();
            let reply = sda_ios_speakers_measured(h,text.as_ptr());
            let response: Value = serde_json::from_str(CStr::from_ptr(reply).to_str().unwrap()).unwrap();
            sda_ios_string_free(reply); assert_eq!(response["ok"],true);
            sda_ios_speakers_balance(h,true);
            let host = &mut *h.cast::<SpeakerHost>();
            host.pcm.extend(std::iter::repeat_n(0.0, 6*48000*12));
            host.pcm.extend(std::iter::repeat_n(0.8, 1024*12));
            let mut out = [0.0;1024*12];
            for _ in 0..(6*48000/1024) { assert_eq!(sda_ios_speakers_read(h,out.as_mut_ptr(),1024),1024); assert!(out.iter().all(|v| *v==0.0)); }
            let remaining_silence = 6*48000%1024;
            assert_eq!(sda_ios_speakers_read(h,out.as_mut_ptr(),1024),1024);
            assert_eq!(out[remaining_silence*12],0.8*gain);
            assert_eq!(out[1023*12],0.8*gain); // No loud first frame or startup ramp.
            // Repeated live/offline meter updates must not overwrite the whole-track gain.
            sda_ios_string_free(sda_ios_speakers_feed(h,std::ptr::null(),0,true));
            assert_eq!((*h.cast::<SpeakerHost>()).target_gain,gain);
            sda_ios_speakers_close(h);
        }
    }

    #[test]
    fn speaker_analysis_discards_pcm_and_objects_beyond_playback_backlog() {
        let bytes = include_bytes!("../../../packages/core/mpegh/fixtures/motion.mhas");
        unsafe {
            let mut error = std::ptr::null_mut();
            let h = sda_ios_speakers_create(&mut error);
            assert!(!h.is_null());
            sda_ios_speakers_measure_only(h);
            for _ in 0..10 {
                for chunk in bytes.chunks(1024) {
                    let p = sda_ios_speakers_feed(h,chunk.as_ptr(),chunk.len(),false);
                    let v: Value = serde_json::from_str(CStr::from_ptr(p).to_str().unwrap()).unwrap();
                    sda_ios_string_free(p); assert_eq!(v["ok"],true,"{v}");
                    assert_eq!(v["value"]["queuedFrames"],0);
                }
            }
            let host = &*h.cast::<SpeakerHost>();
            assert!(host.pcm.is_empty()); assert!(host.events.is_empty());
            assert!(host.meter.integrated().blocks > 80);
            sda_ios_speakers_close(h);
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

// Bounded non-realtime interleaved Float32 input; Swift owns decoding/resampling.
unsafe fn alac_samples(bytes: *const u8, len: usize) -> EngineResult<Vec<f32>> {
    if bytes.is_null() || len == 0 || len % 8 != 0 || len > 65536*4 { return Err("invalid ALAC PCM buffer".into()); }
    let bytes = unsafe { std::slice::from_raw_parts(bytes,len) };
    let pcm: Vec<f32> = bytes.chunks_exact(4).map(|v| f32::from_le_bytes(v.try_into().unwrap())).collect();
    if pcm.iter().any(|v| !v.is_finite()) { return Err("nonfinite ALAC PCM".into()); }
    Ok(pcm)
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_alac_feed(p: *mut std::ffi::c_void, bytes: *const u8, len: usize, upmix: bool) -> *mut c_char {
    guarded(|| {
        let host = unsafe { (p as *mut Host).as_ref() }.ok_or("engine unavailable")?;
        let pcm = unsafe { alac_samples(bytes,len)? };
        let mut e = host.engine.lock().map_err(|_| "engine poisoned")?;
        let mut adapter = host.alac.lock().map_err(|_| "ALAC adapter poisoned")?;
        for pcm in pcm.chunks(8192) {
            let frame = adapter.live_frame(pcm,upmix,e.decoded_sample_pos())?;
            let reference = vec![pcm.iter().step_by(2).copied().collect(),pcm.iter().skip(1).step_by(2).copied().collect()];
            e.feed_alac_pcm(frame,reference)?;
        }
        Ok(json!(true))
    })
}

// Separate speaker PCM ABI. Never enters MobileEngine, HRTF, room or stereo FIFO.
struct SpeakerHost {
    decoder: Option<mpegh::MpeghDecoder>,
    alac: crate::alac_pcm::AlacPcm,
    channels: usize,
    upmix: bool,
    pcm: std::collections::VecDeque<f32>,
    events: std::collections::VecDeque<sda_core::ObjectEvent>,
    active: crate::ObjectSnapshot,
    meter: crate::balance::LoudnessMeter,
    balance_enabled: bool,
    measured_frames: usize,
    target_gain: f32,
    gain: f32,
    read_started: bool,
    measure_only: bool,
    fixed_measurement: Option<crate::balance::Measurement>,
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_create(error: *mut *mut c_char) -> *mut std::ffi::c_void {
    let result = std::panic::catch_unwind(|| mpegh::MpeghDecoder::new_7_1_4()).unwrap_or_else(|_| Err("speaker decoder panic".into()));
    match result {
        Ok(decoder) => Box::into_raw(Box::new(SpeakerHost { decoder: Some(decoder), alac: Default::default(), channels: 12, upmix: false, pcm: Default::default(), events: Default::default(), active: Default::default(),
            meter: crate::balance::LoudnessMeter::speakers_7_1_4(), balance_enabled: false, measured_frames: 0, target_gain: 1.0, gain: 1.0, read_started: false, measure_only: false, fixed_measurement: None })).cast(),
        Err(e) => { if !error.is_null() { unsafe { *error = reply(Err(e)); } } std::ptr::null_mut() }
    }
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_feed(p: *mut std::ffi::c_void, bytes: *const u8, len: usize, finish: bool) -> *mut c_char {
    guarded(|| {
        if p.is_null() || (len > 0 && bytes.is_null()) || len > 65536 { return Err("invalid speaker input".into()); }
        let host = unsafe { &mut *p.cast::<SpeakerHost>() };
        let decoder = host.decoder.as_mut().ok_or("not an MPEG-H source")?;
        if finish { decoder.flush()?; }
        else { decoder.push(if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(bytes, len) } })?; }
        while let Some(frame) = decoder.next_frame() {
            let frames = frame.channels[0].len();
            if host.events.len() + frame.events.len() > 262144 { return Err("speaker object timeline backlog exceeded".into()); }
            if !host.measure_only { host.events.extend(frame.events); }
            host.events.make_contiguous().sort_by_key(|e| e.sample_pos);
            if host.fixed_measurement.is_none() { host.meter.push(&frame.channels); }
            host.measured_frames += 1;
            if !host.measure_only && host.fixed_measurement.is_none() && (!host.read_started || host.measured_frames % 8 == 0) {
                let m = host.meter.integrated();
                if m.blocks >= crate::balance::MIN_BLOCKS {
                    if let Some(lufs) = m.integrated_lufs { host.target_gain = 10_f64.powf(crate::balance::master_gain(lufs, m.true_peak_dbtp)/20.0) as f32; }
                }
            }
            if host.measure_only { continue; }
            if host.pcm.len() + frames*12 > 8*48000*12 { return Err("speaker PCM backlog exceeded".into()); }
            for i in 0..frames { for channel in &frame.channels { host.pcm.push_back(channel[i]); } }
        }
        if finish && host.fixed_measurement.is_none() {
            let m = host.meter.integrated();
            if let Some(lufs) = m.integrated_lufs {
                host.target_gain = 10_f64.powf(crate::balance::master_gain(lufs, m.true_peak_dbtp)/20.0) as f32;
            }
        }
        Ok(json!({"queuedFrames":host.pcm.len()/12,"channels":12,"sampleRate":48000}))
    })
}
// PCM-only speaker host: never acquires the process-global MPEG-H decoder.
#[no_mangle]
pub unsafe extern "C" fn sda_ios_alac_speakers_create(upmix: bool) -> *mut std::ffi::c_void {
    Box::into_raw(Box::new(SpeakerHost { decoder:None, alac:Default::default(), channels:if upmix {12} else {2}, upmix,
        pcm:Default::default(), events:Default::default(), active:Default::default(), meter:crate::balance::LoudnessMeter::new(48000),
        balance_enabled:false, measured_frames:0, target_gain:1.0, gain:1.0, read_started:false, measure_only:false, fixed_measurement:None })).cast()
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_alac_speakers_feed(p: *mut std::ffi::c_void, bytes: *const u8, len: usize, finish: bool) -> *mut c_char {
    guarded(|| {
        let host = unsafe { p.cast::<SpeakerHost>().as_mut() }.ok_or("speaker host unavailable")?;
        if host.decoder.is_some() || (finish && len != 0) { return Err("invalid ALAC source".into()); }
        if !finish {
            let pcm = unsafe { alac_samples(bytes,len)? };
            if !host.measure_only && host.pcm.len() + pcm.len() > 8*48000*2 { return Err("speaker PCM backlog exceeded".into()); }
            let reference = vec![pcm.iter().step_by(2).copied().collect(),pcm.iter().skip(1).step_by(2).copied().collect()];
            if host.fixed_measurement.is_none() { host.meter.push(&reference); }
            host.measured_frames += 1;
            if !host.measure_only && host.fixed_measurement.is_none() && (!host.read_started || host.measured_frames % 8 == 0) {
                let m = host.meter.integrated();
                if m.blocks >= crate::balance::MIN_BLOCKS {
                    if let Some(lufs) = m.integrated_lufs { host.target_gain = 10_f64.powf(crate::balance::master_gain(lufs,m.true_peak_dbtp)/20.0) as f32; }
                }
            }
            if !host.measure_only {
                // Keep original stereo until read: queued audio can change mode
                // without dropping/redecoding samples or mixing queue strides.
                host.pcm.extend(pcm);
            }
        }
        if finish && host.fixed_measurement.is_none() {
            let m = host.meter.integrated();
            host.target_gain = m.integrated_lufs.map_or(1.0,|lufs| 10_f64.powf(crate::balance::master_gain(lufs,m.true_peak_dbtp)/20.0) as f32);
        }
        Ok(json!({"queuedFrames":host.pcm.len()/2,"channels":host.channels,"sampleRate":48000}))
    })
}
// Analysis uses a separate decoder and keeps no PCM/object backlog. Never audible.
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_measure_only(p: *mut std::ffi::c_void) {
    if !p.is_null() { unsafe { (*p.cast::<SpeakerHost>()).measure_only = true; } }
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_measurement(p: *mut std::ffi::c_void) -> *mut c_char {
    guarded(|| {
        if p.is_null() { return Err("invalid speaker handle".into()); }
        let host = unsafe { &*p.cast::<SpeakerHost>() };
        serde_json::to_value(host.fixed_measurement.clone().unwrap_or_else(|| host.meter.integrated())).map_err(|e| e.to_string())
    })
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_measured(p: *mut std::ffi::c_void, measurement: *const c_char) -> *mut c_char {
    guarded(|| {
        if p.is_null() || measurement.is_null() { return Err("invalid speaker measurement".into()); }
        let host = unsafe { &mut *p.cast::<SpeakerHost>() };
        let m: crate::balance::Measurement = serde_json::from_str(unsafe { CStr::from_ptr(measurement) }.to_str().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if m.integrated_lufs.is_some_and(|v| !v.is_finite()) || m.true_peak_dbtp.is_some_and(|v| !v.is_finite()) {
            return Err("nonfinite speaker measurement".into());
        }
        host.target_gain = m.integrated_lufs.map_or(1.0, |lufs| 10_f64.powf(crate::balance::master_gain(lufs, m.true_peak_dbtp)/20.0) as f32);
        host.fixed_measurement = Some(m);
        Ok(json!(true))
    })
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_read(p: *mut std::ffi::c_void, out: *mut f32, capacity_frames: usize) -> usize {
    if p.is_null() || out.is_null() || capacity_frames > 4096 { return 0; }
    let host = unsafe { &mut *p.cast::<SpeakerHost>() };
    let alac = host.decoder.is_none();
    let frames = capacity_frames.min(host.pcm.len()/if alac { 2 } else { host.channels });
    if frames == 0 { return 0; }
    let converted = if alac {
        host.channels = host.alac.live_channels(host.upmix);
        let pcm: Vec<f32> = host.pcm.drain(..frames*2).collect();
        match host.alac.live_frame(&pcm,host.upmix,0) {
            Ok(frame) => Some(frame), Err(_) => return 0,
        }
    } else { None };
    if frames > 0 && !host.read_started {
        host.gain = if host.balance_enabled { host.target_gain } else { 1.0 };
        host.read_started = true;
    }
    for i in 0..frames {
        let target = if host.balance_enabled { host.target_gain } else { 1.0 };
        // A single smooth gain for all 12 channels, including LFE. No remapping.
        host.gain += (target - host.gain).clamp(-1.0/12000.0, 1.0/12000.0);
        for ch in 0..host.channels {
            let sample = if let Some(frame) = &converted {
                frame.channels[[0,1,2,3,6,7,4,5,8,9,10,11][ch]][i]
            } else { host.pcm.pop_front().unwrap_or(0.0) };
            unsafe { *out.add(i*host.channels+ch) = sample * host.gain; }
        }
    }
    frames
}
// Control-thread only. Callers provide space for 12 channels when live mode is used.
#[no_mangle]
pub unsafe extern "C" fn sda_ios_alac_speakers_upmix(p: *mut std::ffi::c_void, enabled: bool) {
    if let Some(host) = unsafe { p.cast::<SpeakerHost>().as_mut() } {
        if host.decoder.is_none() { host.upmix = enabled; }
    }
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_speakers_channels(p: *mut std::ffi::c_void) -> usize {
    unsafe { p.cast::<SpeakerHost>().as_ref() }.map_or(0, |h| h.channels)
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

// Asset IO and FFT preparation intentionally require no playback handle/lock.
#[no_mangle]
pub unsafe extern "C" fn sda_ios_prepare_cues(path: *const c_char, layout: *const c_char, gain: f32, error: *mut *mut c_char) -> *mut std::ffi::c_void {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sda_native_renderer::live_spatial_cues::PreparedCueUpdate::load(unsafe { text(path)? }, unsafe { text(layout)? }, gain)
    })).unwrap_or_else(|_| Err("cue preparation panic".into()));
    match result {
        Ok(update) => Box::into_raw(Box::new(Some(update))).cast(),
        Err(e) => { if !error.is_null() { unsafe { *error = reply(Err(e)); } } std::ptr::null_mut() }
    }
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_apply_cues(p: *mut std::ffi::c_void, prepared: *mut std::ffi::c_void) -> *mut c_char {
    guarded(|| {
        if p.is_null() || prepared.is_null() { return Err("null cue handle".into()); }
        let host = unsafe { &*(p as *mut Host) };
        let update = unsafe { &mut *(prepared as *mut Option<sda_native_renderer::live_spatial_cues::PreparedCueUpdate>) };
        host.engine.lock().map_err(|_| "engine lock poisoned")?.apply_spatial_cue_update(update.take().ok_or("cue update already consumed")?)?;
        Ok(Value::Null)
    })
}
#[no_mangle]
pub unsafe extern "C" fn sda_ios_free_cues(p: *mut std::ffi::c_void) {
    if !p.is_null() { drop(unsafe { Box::from_raw(p as *mut Option<sda_native_renderer::live_spatial_cues::PreparedCueUpdate>) }); }
}

#[cfg(test)]
mod alac_tests {
    use super::*;
    unsafe fn value(p: *mut c_char) -> Value {
        let result: Value = serde_json::from_str(unsafe { CStr::from_ptr(p) }.to_str().unwrap()).unwrap();
        unsafe { sda_ios_string_free(p) };
        assert_eq!(result["ok"],true,"{result}"); result["value"].clone()
    }
    struct Close(*mut std::ffi::c_void);
    impl Drop for Close { fn drop(&mut self) { unsafe { sda_ios_speakers_close(self.0) }; } }
    #[test]
    fn stereo_and_upmix_speaker_pcm_have_correct_order_and_no_fake_objects() {
        for upmix in [false,true] {
            let h = unsafe { sda_ios_alac_speakers_create(upmix) }; let _close = Close(h);
            let pcm: [f32;4] = [0.6,0.2,-0.2,0.4];
            let status = unsafe { value(sda_ios_alac_speakers_feed(h,pcm.as_ptr().cast(),16,false)) };
            let channels = if upmix {12} else {2};
            assert_eq!(status["queuedFrames"],2); assert_eq!(status["channels"],channels);
            let mut result=vec![99.0;2*channels+1];
            assert_eq!(unsafe { sda_ios_speakers_read(h,result.as_mut_ptr(),2) },2);
            assert_eq!(result[2*channels],99.0); // no writes beyond actual channel count
            if upmix {
                let expected=crate::alac_pcm::AlacPcm::default().frame(&pcm,true,0).unwrap();
                for (ch,source) in [0,1,2,3,6,7,4,5,8,9,10,11].into_iter().enumerate() {
                    assert_eq!(result[ch],expected.channels[source][0]);
                    assert_eq!(result[12+ch],expected.channels[source][1]);
                }
            } else { assert_eq!(&result[..4],&pcm); }
            assert_eq!(unsafe { value(sda_ios_speakers_objects(h,2)) },json!({}));
            assert_eq!(unsafe { value(sda_ios_alac_speakers_feed(h,std::ptr::null(),0,true)) }["queuedFrames"],0);
        }
    }
    #[test]
    fn alac_speaker_live_switch_preserves_queued_stereo_and_balance() {
        let h = unsafe { sda_ios_alac_speakers_create(false) }; let _close=Close(h);
        let pcm: Vec<f32> = (0..20000).flat_map(|i| [0.1+i as f32*0.000001,-0.2]).collect();
        unsafe { value(sda_ios_alac_speakers_feed(h,pcm.as_ptr().cast(),pcm.len()*4,false)); }
        let cached=CString::new(r#"{"integratedLufs":-12,"truePeakDbtp":-3,"blocks":100}"#).unwrap();
        unsafe { value(sda_ios_speakers_measured(h,cached.as_ptr())); sda_ios_speakers_balance(h,true); }
        let gain=10f32.powf(-6.0/20.0);
        let mut consumed=0;
        for enabled in [false,true,false,true,false] {
            unsafe { sda_ios_alac_speakers_upmix(h,enabled); }
            for _ in 0..4 {
                let mut out=vec![99.0;1000*12+1];
                assert_eq!(unsafe { sda_ios_speakers_read(h,out.as_mut_ptr(),1000) },1000);
                let channels=unsafe { sda_ios_speakers_channels(h) };
                assert!(channels==2 || channels==12);
                assert_eq!(out[1000*channels],99.0);
                if channels==2 {
                    for i in 0..1000 {
                        assert!((out[2*i]-pcm[2*(consumed+i)]*gain).abs()<1e-6);
                        assert!((out[2*i+1]-pcm[2*(consumed+i)+1]*gain).abs()<1e-6);
                    }
                }
                consumed+=1000;
            }
            let host=unsafe { &*h.cast::<SpeakerHost>() };
            assert_eq!(host.pcm.len(),(20000-consumed)*2);
            assert!(host.balance_enabled && host.fixed_measurement.is_some());
            assert_eq!(host.alac.live_channels(enabled),if enabled {12} else {2});
        }
        assert_eq!(consumed,20000);
        assert_eq!(unsafe { value(sda_ios_alac_speakers_feed(h,std::ptr::null(),0,true)) }["queuedFrames"],0);
    }
    #[test]
    fn balance_is_one_linked_gain_and_analysis_keeps_no_pcm() {
        let pcm: Vec<f32> = (0..8192).map(|i| 0.5*(i as f32*0.03).sin()).collect();
        let a=unsafe { sda_ios_alac_speakers_create(false) }; let _a=Close(a);
        let b=unsafe { sda_ios_alac_speakers_create(true) }; let _b=Close(b);
        for h in [a,b] { unsafe { sda_ios_speakers_measure_only(h); } }
        for _ in 0..80 { for h in [a,b] { unsafe { value(sda_ios_alac_speakers_feed(h,pcm.as_ptr().cast(),pcm.len()*4,false)); } } }
        let measured=unsafe { value(sda_ios_speakers_measurement(a)) };
        assert_eq!(measured,unsafe { value(sda_ios_speakers_measurement(b)) });
        assert!(measured["integratedLufs"].is_number());
        let mut out=[0.0;12];
        assert_eq!(unsafe { sda_ios_speakers_read(b,out.as_mut_ptr(),1) },0);
        let h=unsafe { sda_ios_alac_speakers_create(true) }; let _close=Close(h);
        let cached=CString::new(json!({"integratedLufs":-12.0,"truePeakDbtp":-3.0,"blocks":100}).to_string()).unwrap();
        unsafe { value(sda_ios_speakers_measured(h,cached.as_ptr())); sda_ios_speakers_balance(h,true); }
        unsafe { value(sda_ios_alac_speakers_feed(h,pcm.as_ptr().cast(),pcm.len()*4,false)); }
        assert_eq!(unsafe { sda_ios_speakers_read(h,out.as_mut_ptr(),1) },1);
        let expected=crate::alac_pcm::AlacPcm::default().frame(&pcm,true,0).unwrap();
        for (ch,source) in [0,1,2,3,6,7,4,5,8,9,10,11].into_iter().enumerate() {
            assert!((out[ch]-expected.channels[source][0]*10f32.powf(-6.0/20.0)).abs()<1e-6);
        }
    }
    #[test]
    fn alac_upmix_uncached_balance_measures_original_stereo() {
        let mut stereo = crate::balance::VolumeBalance::default();
        let mut upmixed = crate::balance::VolumeBalance::default();
        stereo.enabled = true; upmixed.enabled = true;
        stereo.enable_alac_startup(); upmixed.enable_alac_startup();
        let mut adapter = crate::alac_pcm::AlacPcm::default();
        for start in (0..48000*8).step_by(4096) {
            let pcm: Vec<f32> = (start..start+4096).flat_map(|i| {
                [0.5*(i as f32*0.13).sin(),0.25*(i as f32*0.17).sin()]
            }).collect();
            let dry=adapter.frame(&pcm,false,start as u64).unwrap();
            let reference=dry.channels.clone();
            let wet=adapter.frame(&pcm,true,start as u64).unwrap();
            let a=stereo.measure(dry,None); let b=upmixed.measure(wet,Some(reference));
            stereo.route(&a); upmixed.route(&b);
            assert_eq!(stereo.gain_db,upmixed.gain_db);
            assert!(stereo.eligible && upmixed.eligible);
        }
        assert!(upmixed.gain_db < -3.0,"uncached ALAC must attenuate a loud master: {}",upmixed.gain_db);
        stereo.finish(); upmixed.finish();
        assert_eq!(serde_json::to_value(stereo.complete_measurement()).unwrap(),
            serde_json::to_value(upmixed.complete_measurement()).unwrap());
    }
    // Exercise the real native FIFO, not only the eligibility/status flags.
    #[test]
    fn alac_native_balance_changes_output_pcm_in_stereo_and_upmix() {
        fn render(enabled: bool, upmix: bool, cached: bool, toggle: bool, live: bool) -> Vec<f32> {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let path = CString::new(root.join("apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json").to_str().unwrap()).unwrap();
            let config = CString::new(json!({"sampleRate":48000,"outputChannels":2,"layout":if upmix {"7.1.4"} else {"2.0"},"directObjectHrtf":true,"directionalHrtf":true}).to_string()).unwrap();
            let mut error = std::ptr::null_mut();
            let h = unsafe { sda_ios_create(config.as_ptr(),path.as_ptr(),&mut error) };
            if h.is_null() { unsafe { value(error); } panic!("create failed"); }
            struct NativeClose(*mut std::ffi::c_void);
            impl Drop for NativeClose { fn drop(&mut self) { unsafe { sda_ios_close(self.0); } } }
            let _close = NativeClose(h);
            for (op,args) in [("balance",json!({"enabled":enabled})),
                ("measured",json!({"json":r#"{"integratedLufs":-12,"blocks":100,"truePeakDbtp":-3}"#}))] {
                if op == "measured" && !cached { continue; }
                let op=CString::new(op).unwrap(); let args=CString::new(args.to_string()).unwrap();
                unsafe { value(sda_ios_command(h,op.as_ptr(),args.as_ptr())); }
            }
            let frames = 48000*4;
            let amplitude = if cached { 0.002 } else { 0.5 };
            for start in (0..frames).step_by(4096) {
                let pcm: Vec<f32> = (start..(start+4096).min(frames)).flat_map(|i| {
                    [amplitude*(i as f32*0.13).sin(),amplitude*0.5*(i as f32*0.17).sin()]
                }).collect();
                unsafe { value(sda_ios_alac_feed(h,pcm.as_ptr().cast(),pcm.len()*4,if live { start >= 48000 && start < 96000 } else { upmix })); }
            }
            unsafe { value(sda_ios_command(h,c"finish".as_ptr(),c"{}".as_ptr())); }
            let host = unsafe { &*h.cast::<Host>() };
            let deadline=Instant::now()+Duration::from_secs(30);
            let mut output=Vec::new();
            let mut toggles=0;
            while output.len()<frames*2 {
                let mut l=[0.0;512]; let mut r=[0.0;512];
                let n=pull(&host.output,&mut l,&mut r);
                for i in 0..n { output.extend_from_slice(&[l[i],r[i]]); }
                if toggle && toggles < 2 && output.len() >= (toggles+1)*96000 {
                    let args = CString::new(json!({"enabled":toggles == 1}).to_string()).unwrap();
                    unsafe { value(sda_ios_command(h,c"balance".as_ptr(),args.as_ptr())); }
                    toggles+=1;
                }
                assert!(Instant::now()<deadline,"native ALAC output stalled");
                if n==0 { std::thread::sleep(Duration::from_millis(1)); }
            }
            output.truncate(frames*2); output
        }
        for upmix in [false,true] {
            let bypass=render(false,upmix,true,false,false); let balanced=render(true,upmix,true,false,false);
            let gain=10f32.powf(-6.0/20.0);
            assert!(bypass.iter().any(|v|v.abs()>1e-5));
            for (a,b) in bypass.iter().zip(&balanced).skip(8192) {
                assert!((a*gain-b).abs()<2e-6,"expected linked -6 dB: upmix={upmix}, bypass={a}, balanced={b}");
            }
        }
        // Real output must attenuate a cold, short ALAC stream before six
        // seconds; the old MIN_BLOCKS policy left this entire clip unchanged.
        let bypass=render(false,true,false,false,false);
        let balanced=render(true,true,false,false,false);
        let energy = |pcm: &[f32]| pcm.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
        let ratio=energy(&balanced[48000*4..])/energy(&bypass[48000*4..]);
        assert!(ratio.is_finite() && ratio < 0.5,"cold ALAC not attenuated: energy ratio={ratio}");
        // Toggle off at one second and on at two, without restarting/pausing.
        let switched=render(true,true,true,true,false);
        let stable=render(true,true,true,false,false);
        let energy_slice = |pcm: &[f32], from: usize, to: usize| energy(&pcm[from..to]) / (to-from) as f64;
        let baseline=energy_slice(&stable,48000,96000);
        let off=energy_slice(&switched,144000,180000);
        let on=energy_slice(&switched,288000,360000);
        assert!((off/baseline-10f64.powf(6.0/10.0)).abs()<0.05,"toggle off did not restore unity: {}",off/baseline);
        assert!((on/baseline-1.0).abs()<0.02,"toggle on did not restore balance: {}",on/baseline);
        // Same native handle and continuously increasing PCM timestamps, dry ->
        // wet -> dry. Settled regions must match the corresponding static path.
        let dry=render(false,false,true,false,false);
        let wet=render(false,true,true,false,false);
        let live=render(false,false,true,false,true);
        assert_eq!(live.len(),48000*4*2);
        for (range,reference) in [(64000..88000,&wet),(160000..185000,&dry)] {
            for i in range { for ch in 0..2 {
                assert!((live[i*2+ch]-reference[i*2+ch]).abs()<2e-5,
                    "live mode lost continuity at {i}:{ch}: {} vs {}",live[i*2+ch],reference[i*2+ch]);
            } }
        }

    }
    #[test]
    fn ku100_adapter_preserves_source_clock_and_balance_eligibility() {
        let mut engine=MobileEngine::new(EngineConfig::default(),None).unwrap();
        let pcm=vec![0.2f32;8192];
        let frame=crate::alac_pcm::AlacPcm::default().frame(&pcm,true,0).unwrap();
        engine.feed_alac_pcm(frame,vec![vec![0.2;4096];2]).unwrap();
        assert_eq!(engine.decoded_sample_pos(),4096);
        let mut pending=engine.pending.lock().unwrap(); let frame=pending.pop_front().unwrap(); drop(pending);
        engine.balance.route(&frame);
        assert!(engine.balance.eligible);
        assert_eq!(frame.frame.raw_bed_labels,vec!["L","R"]);
    }
}
