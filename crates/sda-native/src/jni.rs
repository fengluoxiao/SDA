//! JNI boundary for the Android host (plan T2.4 first slice). Exported for
//! `com.sda.nativebridge.SdaEngine`; keeps the handle as a raw `MobileEngine`
//! pointer (long). PCM never crosses this boundary — hosts only see metadata
//! (`DecodeStatus`) and the playback watermark.

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;

use crate::{EngineConfig, MobileEngine};

pub(crate) fn android_log(message: &str) {
    extern "C" {
        fn __android_log_write(prio: i32, tag: *const u8, text: *const u8) -> i32;
    }
    let tag = b"SdaEngine\0".to_vec();
    let mut text = message.as_bytes().to_vec();
    text.push(0);
    unsafe {
        __android_log_write(4, tag.as_ptr(), text.as_ptr());
    }
}

fn take_engine(ptr: jlong) -> Option<&'static mut MobileEngine> {
    if ptr == 0 {
        None
    } else {
        Some(unsafe { &mut *(ptr as *mut MobileEngine) })
    }
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeSetRoom(
    mut env: JNIEnv, _class: JClass, ptr: jlong, path: JString,
) -> jni::sys::jstring {
    let result = (|| -> Result<(), String> {
        let path: String = env.get_string(&path).map_err(|e| e.to_string())?.into();
        take_engine(ptr).ok_or("engine unavailable")?.set_room(&path)
    })();
    env.new_string(result.err().unwrap_or_default()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeSetNearField(
    env: JNIEnv, _class: JClass, ptr: jlong, enabled: jni::sys::jboolean, scale: jni::sys::jfloat,
) -> jni::sys::jstring {
    let result = take_engine(ptr).ok_or("engine unavailable".to_string())
        .and_then(|engine| engine.set_near_field(enabled != 0, scale));
    env.new_string(result.err().unwrap_or_default()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeSetObjectRendering(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    direct: jni::sys::jboolean,
    directional: jni::sys::jboolean,
) -> jint {
    match take_engine(ptr) {
        Some(engine) => engine.set_object_rendering(direct != 0, directional != 0).map(|_| 0).unwrap_or(-1),
        None => -2,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeSetHrtfPreset(
    mut env: JNIEnv, _class: JClass, ptr: jlong, path: JString,
    wet: jni::sys::jfloat, direct: jni::sys::jboolean, directional: jni::sys::jboolean,
) -> jni::sys::jstring {
    let result = (|| -> Result<(), String> {
        let path: String = env.get_string(&path).map_err(|e| e.to_string())?.into();
        take_engine(ptr).ok_or("engine unavailable")?
            .set_hrtf_preset(&path, wet, direct != 0, directional != 0)
    })();
    env.new_string(result.err().unwrap_or_default()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// `nativeInit(configJson: String, hrtfPath: String): Long` — engine handle,
/// or 0 on failure. Empty `hrtfPath` skips HRTF (renders without
/// spatialization rather than failing).
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeInit(
    mut env: JNIEnv,
    _class: JClass,
    config_json: JString,
    hrtf_path: JString,
) -> jlong {
    let config: String = match env.get_string(&config_json) {
        Ok(value) => value.into(),
        Err(_) => return 0,
    };
    let hrtf: String = match env.get_string(&hrtf_path) {
        Ok(value) => value.into(),
        Err(_) => return 0,
    };
    let config: EngineConfig = match serde_json::from_str(&config) {
        Ok(value) => value,
        Err(error) => {
            android_log(&format!("config parse failed: {error}"));
            return 0;
        }
    };
    match MobileEngine::new(config, None) {
        Ok(mut engine) => {
            if !hrtf.is_empty() {
                if let Err(error) = engine.load_hrtf(&hrtf) {
                    android_log(&format!("hrtf load failed: {error}"));
                    set_init_error(&error);
                    return 0;
                }
            }
            set_init_error("");
            Box::into_raw(Box::new(engine)) as jlong
        }
        Err(error) => {
            set_init_error(&error);
            0
        }
    }
}

fn init_error() -> &'static std::sync::Mutex<String> {
    static ERROR: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
    ERROR.get_or_init(|| std::sync::Mutex::new(String::new()))
}

fn set_init_error(message: &str) {
    *init_error().lock().unwrap() = message.to_string();
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeInitError(
    mut env: JNIEnv,
    _class: JClass,
) -> jni::sys::jstring {
    let message = init_error().lock().unwrap().clone();
    env.new_string(message).map(|value| value.into_raw()).unwrap_or_else(|_| std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeHrtfLoaded(
    _env: JNIEnv, _class: JClass, ptr: jlong,
) -> jni::sys::jboolean {
    take_engine(ptr).map(|engine| engine.hrtf_loaded() as u8).unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeOpenMpegh(
    _env: JNIEnv, _class: JClass, ptr: jlong,
) -> jint {
    match take_engine(ptr).ok_or_else(|| "invalid engine".to_string()).and_then(|engine| engine.open_mpegh()) {
        Ok(()) => { set_last_error(""); 0 },
        Err(error) => { set_last_error(&error); -1 },
    }
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeOpenMp3(
    mut env: JNIEnv, _class: JClass, ptr: jlong, path: JString,
) -> jint {
    let path: String = match env.get_string(&path) { Ok(value) => value.into(), Err(_) => return -1 };
    match take_engine(ptr).ok_or_else(|| "invalid engine".to_string()).and_then(|engine| engine.open_mp3(&path)) {
        Ok(rate) => { set_last_error(""); rate as jint },
        Err(error) => { set_last_error(&error); -1 },
    }
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativePullMp3(
    _env: JNIEnv, _class: JClass, ptr: jlong, max_frames: jint,
) -> jint {
    match take_engine(ptr).ok_or_else(|| "invalid engine".to_string()).and_then(|engine| engine.pull_mp3(max_frames.max(0) as usize)) {
        Ok((frames, eof)) => {
            set_last_error("");
            if eof && frames == 0 { -4 } else { frames.min(i32::MAX as usize) as jint }
        },
        Err(error) => { set_last_error(&error); -1 },
    }
}

fn last_error() -> &'static std::sync::Mutex<String> {
    static ERROR: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
    ERROR.get_or_init(|| std::sync::Mutex::new(String::new()))
}

fn set_last_error(message: &str) {
    *last_error().lock().unwrap() = message.to_string();
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeLastError(
    mut env: JNIEnv, _class: JClass,
) -> jni::sys::jstring {
    let message = last_error().lock().unwrap().clone();
    env.new_string(message).map(|value| value.into_raw()).unwrap_or_else(|_| std::ptr::null_mut())
}

/// `nativeObjects(ptr: Long): String` — presentation-clock object metadata.
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeObjects(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jni::sys::jstring {
    let json = take_engine(ptr)
        .and_then(|engine| serde_json::to_string(&engine.object_snapshot()).ok())
        .unwrap_or_else(|| "{}".to_string());
    env.new_string(json).map(|value| value.into_raw()).unwrap_or_else(|_| std::ptr::null_mut())
}

/// `nativeSetHeadYaw(ptr: Long, degrees: Float): Int`
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeSetHeadYaw(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    degrees: jni::sys::jfloat,
) -> jint {
    match take_engine(ptr) {
        Some(engine) => engine.set_head_yaw_degrees(degrees).map(|_| 0).unwrap_or(-1),
        None => -2,
    }
}

/// `nativeResetHeadPose(ptr: Long): Int`
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeResetHeadPose(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    match take_engine(ptr) {
        Some(engine) => engine.reset_head_pose().map(|_| 0).unwrap_or(-1),
        None => -2,
    }
}

/// `nativeStart(ptr: Long): Int` — 0 on success.
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeStart(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    match take_engine(ptr) {
        Some(engine) => match engine.start_android() {
            Ok(()) => {
                set_last_error("");
                0
            }
            Err(error) => {
                android_log(&format!("AAudio startup failed: {error}"));
                set_last_error(&error);
                -1
            }
        },
        None => -2,
    }
}

/// `nativeFeed(ptr: Long, bytes: ByteArray): Int` — decoded frame count, or
/// negative on error.
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeFeed(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    bytes: jbyteArray,
) -> jint {
    let Some(engine) = take_engine(ptr) else {
        return -2;
    };
    let array = unsafe { JByteArray::from_raw(bytes) };
    let data = match env.convert_byte_array(&array) {
        Ok(value) => value,
        Err(_) => return -3,
    };
    match engine.feed(&data) {
        Ok(status) => status.frames_pushed as jint,
        Err(error) => {
            set_last_error(&error);
            android_log(&format!("feed failed: {error}"));
            -1
        },
    }
}

/// `nativeStatus(ptr: Long): String` — PlaybackStatus JSON for host-side
/// feed pacing (fifo watermark).
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeStatus(
    mut env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jni::sys::jstring {
    let fallback = "".to_string();
    let json = match take_engine(ptr) {
        Some(engine) => serde_json::to_string(&engine.playback_status())
            .unwrap_or_else(|_| fallback.clone()),
        None => fallback.clone(),
    };
    env.new_string(json)
        .map(|value| value.into_raw())
        .unwrap_or_else(|_| std::ptr::null_mut())
}

/// `nativeFinish(ptr: Long): Int` — drains the decoder's final frame.
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeFinish(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jint {
    match take_engine(ptr) {
        Some(engine) => match engine.finish() { Ok(status) => status.frames_pushed as jint, Err(error) => { set_last_error(&error); -1 } },
        None => -2,
    }
}

/// `nativePause(ptr: Long, paused: Boolean): Int`
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativePause(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    paused: jni::sys::jboolean,
) -> jint {
    match take_engine(ptr) {
        Some(engine) => engine.set_paused(paused != 0).map(|_| 0).unwrap_or(-1),
        None => -2,
    }
}

/// `nativeSetVolume(ptr: Long, volume: Float): Int`
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeSetVolume(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    volume: jni::sys::jfloat,
) -> jint {
    match take_engine(ptr) {
        Some(engine) => engine.set_volume(volume).map(|_| 0).unwrap_or(-1),
        None => -2,
    }
}

/// `nativeClose(ptr: Long)`
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeClose(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr != 0 {
        drop(unsafe { Box::from_raw(ptr as *mut MobileEngine) });
    }
}

#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeSetVolumeBalance(
    env: JNIEnv, _class: JClass, ptr: jlong, enabled: jni::sys::jboolean,
) -> jni::sys::jstring {
    let result=take_engine(ptr).ok_or("engine unavailable".to_string()).and_then(|engine|engine.set_volume_balance(enabled!=0));
    env.new_string(result.err().unwrap_or_default()).map(|s|s.into_raw()).unwrap_or(std::ptr::null_mut())
}
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeSetMeasuredLoudness(
    mut env: JNIEnv, _class: JClass, ptr: jlong, json: JString,
) -> jni::sys::jstring {
    let result=(|| -> Result<(),String> {
        let json: String=env.get_string(&json).map_err(|e|e.to_string())?.into();
        take_engine(ptr).ok_or("engine unavailable")?.set_measured_loudness(&json)
    })();
    env.new_string(result.err().unwrap_or_default()).map(|s|s.into_raw()).unwrap_or(std::ptr::null_mut())
}
#[no_mangle]
pub extern "system" fn Java_com_sda_nativebridge_SdaEngine_nativeCompleteLoudness(
    env: JNIEnv, _class: JClass, ptr: jlong,
) -> jni::sys::jstring {
    let json=take_engine(ptr).map_or_else(||"null".into(),|engine|engine.complete_loudness_json());
    env.new_string(json).map(|s|s.into_raw()).unwrap_or(std::ptr::null_mut())
}
