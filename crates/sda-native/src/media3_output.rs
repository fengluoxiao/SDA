//! Media3 owns the device; the existing renderer supplies float stereo PCM.
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};
use jni::{JavaVM, objects::{GlobalRef, JValue}};
use sda_native_renderer::{AudioOutput, RuntimeTelemetry, record_callback, stereo_fifo::StereoFifo,
    render_command::RenderCommandQueue};

pub struct Media3Output { pub vm: JavaVM, pub output: GlobalRef }

fn publish_clock(env: &mut jni::JNIEnv, object: &jni::objects::JObject,
    telemetry: &RuntimeTelemetry, published: &mut u64) -> Result<(), Box<dyn std::error::Error>> {
    let played = env.call_method(object, "playedFrames", "()J", &[])?.j()?;
    if played < 0 { return Err("Media3 playback clock failed".into()); }
    let played = played as u64;
    let delta = played.saturating_sub(*published) as usize;
    if delta > 0 {
        record_callback(telemetry, Instant::now(), delta, delta, true);
        *published = played;
    }
    Ok(())
}

impl AudioOutput for Media3Output {
    fn run(self: Arc<Self>, fifo: Arc<StereoFifo>, telemetry: Arc<RuntimeTelemetry>, _: Arc<RenderCommandQueue>) {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut env = self.vm.attach_current_thread()?;
            let object = self.output.as_obj();
            if !env.call_method(object, "open", "()Z", &[])?.z()? { return Err("Media3 open failed".into()); }
            let mut block = vec![0.0f32; 2048];
            let mut playing = true;
            let mut published = 0;
            while !telemetry.shutdown_requested.load(Ordering::Acquire) {
                if fifo.apply_flush_from_consumer() {
                    if !env.call_method(object, "flush", "()Z", &[])?.z()? { return Err("Media3 flush failed".into()); }
                    published = 0;
                }
                publish_clock(&mut env, object, &telemetry, &mut published)?;
                let enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
                if playing != enabled {
                    if !env.call_method(object, "setPlaying", "(Z)Z", &[JValue::Bool(enabled as u8)])?.z()? {
                        return Err("Media3 pause/resume failed".into());
                    }
                    playing = enabled;
                }
                if !enabled { std::thread::sleep(Duration::from_millis(2)); continue; }
                let popped = fifo.pop_into_f32(&mut block, 2);
                if popped == 0 { std::thread::sleep(Duration::from_millis(2)); continue; }
                let samples = env.new_float_array((popped * 2) as i32)?;
                env.set_float_array_region(&samples, 0, &block[..popped * 2])?;
                loop {
                    if telemetry.shutdown_requested.load(Ordering::Acquire) { break; }
                    if fifo.apply_flush_from_consumer() {
                        if !env.call_method(object, "flush", "()Z", &[])?.z()? { return Err("Media3 flush failed".into()); }
                        published = 0;
                        break;
                    }
                    publish_clock(&mut env, object, &telemetry, &mut published)?;
                    let enabled = telemetry.callback_output_enabled.load(Ordering::Acquire);
                    if playing != enabled {
                        if !env.call_method(object, "setPlaying", "(Z)Z", &[JValue::Bool(enabled as u8)])?.z()? {
                            return Err("Media3 pause/resume failed".into());
                        }
                        playing = enabled;
                    }
                    if !enabled { std::thread::sleep(Duration::from_millis(2)); continue; }
                    match env.call_method(object, "write", "([F)I", &[JValue::Object(&samples)])?.i()? {
                        1 => break,
                        0 => std::thread::sleep(Duration::from_millis(2)),
                        _ => return Err("Media3 PCM write failed".into()),
                    }
                }
                env.delete_local_ref(samples)?;
            }
            Ok(())
        })();
        if let Err(error) = result { super::jni::android_log(&format!("Media3 output error: {error}")); }
        if let Ok(mut env) = self.vm.attach_current_thread() {
            let _ = env.call_method(self.output.as_obj(), "close", "()V", &[]);
        }
    }
}
