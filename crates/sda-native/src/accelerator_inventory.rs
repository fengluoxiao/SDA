//! Read-only accelerator inventory through public Android APIs. Discovery is
//! distinct from audio backend enablement. The nested audio state reports real
//! HRTF spectral work, independent of the device inventory.
use std::sync::{atomic::{AtomicBool,Ordering},Mutex,OnceLock};
use std::ffi::{c_char,c_void,CStr};
static STARTED: AtomicBool=AtomicBool::new(false);
static STATUS: OnceLock<Mutex<String>>=OnceLock::new();
fn state()->&'static Mutex<String>{STATUS.get_or_init(||Mutex::new("{\"status\":\"not-started\",\"npuAudioEnabled\":false,\"audioBackend\":\"cpu-native\"}".into()))}
pub fn status()->String {
    if !STARTED.swap(true,Ordering::AcqRel) {
        if let Ok(mut s)=state().lock(){*s="{\"status\":\"probing\",\"npuAudioEnabled\":false,\"audioBackend\":\"cpu-native\"}".into();}
        let started=std::thread::Builder::new().name("sda-accelerators".into()).spawn(||{
            let result=inventory();if let Ok(mut s)=state().lock(){*s=result.to_string();}
        });
        if started.is_err(){if let Ok(mut s)=state().lock(){*s="{\"status\":\"unavailable\",\"npuAudioEnabled\":false}".into();}}
    }
    sda_native_renderer::spectral_accelerator::start();
    let snapshot=state().lock().map(|s|s.clone()).unwrap_or_else(|_|"{}".into());
    let mut result:serde_json::Value=serde_json::from_str(&snapshot).unwrap_or(serde_json::json!({}));
    let audio=sda_native_renderer::spectral_accelerator::status();
    let enabled=audio["npuAudioEnabled"].as_bool().unwrap_or(false);
    result["npuAudioEnabled"]=enabled.into();
    result["audioBackend"]=if enabled {"cpu-fft-and-npu-spectral-mac"}else{"cpu-native"}.into();
    result["audioOperationSupport"]=audio["state"].clone();
    if let Some(devices)=result["devices"].as_array_mut() {
        for device in devices {
            let selected=enabled && device["name"]==audio["deviceName"];
            device["audioValidated"]=selected.into();
            device["audioSelected"]=selected.into();
        }
    }
    result["audio"]=audio;
    result.to_string()
}
use sda_native_renderer::nnapi_runtime::ProcessLibrary as Library;
fn inventory()->serde_json::Value {
    let mut result=serde_json::json!({"status":"complete","source":"android-nnapi-public-device-enumeration","audioBackend":"cpu-native","npuAudioEnabled":false,"audioOperationSupport":"unverified","devices":[],"visibleAccelerators":0,"nativeArchitecture":std::env::consts::ARCH});
    let Some(lib)=Library::open(c"libneuralnetworks.so") else {result["status"]="runtime-unavailable".into();return result;};
    type Count=unsafe extern "C" fn(*mut u32)->i32;
    type Get=unsafe extern "C" fn(u32,*mut *const c_void)->i32;
    type Name=unsafe extern "C" fn(*const c_void,*mut *const c_char)->i32;
    type Kind=unsafe extern "C" fn(*const c_void,*mut i32)->i32;
    type Level=unsafe extern "C" fn(*const c_void,*mut i64)->i32;
    macro_rules! load {($name:expr,$ty:ty)=>{{let symbol=lib.symbol($name);if symbol.is_null(){result["status"]="device-api-unavailable".into();return result;}unsafe{std::mem::transmute::<*mut c_void,$ty>(symbol)}}};}
    let count=load!(c"ANeuralNetworks_getDeviceCount",Count);
    let get=load!(c"ANeuralNetworks_getDevice",Get);
    let name=load!(c"ANeuralNetworksDevice_getName",Name);
    let version=load!(c"ANeuralNetworksDevice_getVersion",Name);
    let kind=load!(c"ANeuralNetworksDevice_getType",Kind);
    let level=load!(c"ANeuralNetworksDevice_getFeatureLevel",Level);
    let mut n=0;let error=unsafe{count(&mut n)};
    if error!=0 {result["status"]="enumeration-failed".into();result["errorCode"]=error.into();return result;}
    let mut devices=Vec::new();let mut accelerators=0;
    for i in 0..n.min(32) {
        let mut device=std::ptr::null();if unsafe{get(i,&mut device)}!=0||device.is_null(){continue;}
        let mut k=0;let mut l=0;let mut a=std::ptr::null();let mut b=std::ptr::null();
        let _=unsafe{kind(device,&mut k)};let _=unsafe{level(device,&mut l)};
        let _=unsafe{name(device,&mut a)};let _=unsafe{version(device,&mut b)};
        let read=|p:*const c_char|if p.is_null(){String::new()}else{unsafe{CStr::from_ptr(p)}.to_string_lossy().chars().take(256).collect::<String>()};
        if k==4 {accelerators+=1;}
        devices.push(serde_json::json!({"name":read(a),"version":read(b),"type":match k{2=>"cpu",3=>"gpu",4=>"accelerator",1=>"other",_=>"unknown"},"featureLevel":l,"audioValidated":false}));
    }
    result["devices"]=devices.into();result["visibleAccelerators"]=accelerators.into();result
}
