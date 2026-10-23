//! NNAPI may register pthread TLS destructors in its dependency libraries.
//! Scoped handles may close, but code must remain mapped until process exit.
//! Keep this policy shared by device discovery and real audio execution.
use std::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
};

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(name: *const c_char, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> i32;
}

// Public Android dlfcn.h constants, available before our minimum API 26.
// Without NODELETE, a failed/unsupported probe can unload code before the
// probing thread runs pthread_key_clean_all, causing a native SIGSEGV.
#[cfg(target_pointer_width = "64")]
const RTLD_NOW: i32 = 2;
#[cfg(target_pointer_width = "32")]
const RTLD_NOW: i32 = 0;
const RTLD_NODELETE: i32 = 0x1000;

pub struct ProcessLibrary(NonNull<c_void>);
impl ProcessLibrary {
    pub fn open(name: &CStr) -> Option<Self> {
        NonNull::new(unsafe { dlopen(name.as_ptr(), RTLD_NOW | RTLD_NODELETE) }).map(Self)
    }
    pub fn symbol(&self, name: &CStr) -> *mut c_void {
        unsafe { dlsym(self.0.as_ptr(), name.as_ptr()) }
    }
}
impl Drop for ProcessLibrary {
    fn drop(&mut self) {
        // Release this handle without unmapping TLS destructor callbacks.
        unsafe {
            dlclose(self.0.as_ptr());
        }
    }
}
