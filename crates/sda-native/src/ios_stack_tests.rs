//! Exercise the real iOS factory on a GCD-sized stack, in a crash-isolated child.
use crate::ios::*;
use std::ffi::{CStr, CString};

#[test]
fn ios_factory_fits_dispatch_stack_across_repeated_track_opens() {
    const CHILD: &str = "SDA_TEST_IOS_DISPATCH_STACK";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ios_stack_tests::ios_factory_fits_dispatch_stack_across_repeated_track_opens", "--nocapture"])
            .env(CHILD, "1").output().unwrap();
        assert!(result.status.success(), "512 KiB factory process failed: {}\n{}\n{}", result.status,
                String::from_utf8_lossy(&result.stdout),String::from_utf8_lossy(&result.stderr));
        eprintln!("{}",String::from_utf8_lossy(&result.stdout));
        return;
    }
    std::thread::Builder::new().name("expo-dispatch-stack-probe".into())
        .stack_size(512 * 1024).spawn(|| {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let path = root.join("assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
            let path = CString::new(path.to_str().unwrap()).unwrap();
            for layout in ["2.0","7.1.4","9.1.6","7.1.4"] {
                let config = CString::new(serde_json::json!({"sampleRate":48000,"outputChannels":2,
                    "layout":layout,"sourceCodec":"eac3","directObjectHrtf":true,"directionalHrtf":true,
                    "objectRendering":true,"referenceMonitor":"genelec-8351b"}).to_string()).unwrap();
                let mut error=std::ptr::null_mut();
                let handle=unsafe{sda_ios_create(config.as_ptr(),path.as_ptr(),&mut error)};
                let message=if error.is_null(){String::new()}else{
                    let text=unsafe{CStr::from_ptr(error)}.to_string_lossy().into_owned();
                    unsafe{sda_ios_string_free(error)};text
                };
                assert!(!handle.is_null(),"{layout}: {message}");
                struct Close(*mut std::ffi::c_void);
                impl Drop for Close {fn drop(&mut self){unsafe{sda_ios_close(self.0)}}}
                let _close=Close(handle);
                for (command,args) in [("status","{}"),("balance",r#"{"enabled":true}"#),("spatialEnhancement",r#"{"enabled":true}"#),("pause",r#"{"paused":true}"#)] {
                    let (command,args)=(CString::new(command).unwrap(),CString::new(args).unwrap());
                    let reply=unsafe{sda_ios_command(handle,command.as_ptr(),args.as_ptr())};
                    assert!(!reply.is_null());
                    let value:serde_json::Value=serde_json::from_str(unsafe{CStr::from_ptr(reply)}.to_str().unwrap()).unwrap();
                    unsafe{sda_ios_string_free(reply)};
                    assert_eq!(value["ok"],true,"{value}");
                }
            }
            eprintln!("Created/configured/closed four native tracks, 512 KiB caller stack");
        }).unwrap().join().unwrap();
}
