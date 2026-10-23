//! Public NNAPI ABI only. Explicit single accelerator compilation prevents the
//! framework from silently falling back to CPU. No vendor model whitelist and
//! no private vendor-partition libraries. Android <29 safely lacks these symbols.
use super::{Batch, Graph, Shape};
use std::{
    ffi::{CStr, c_char, c_void},
    ptr,
    rc::Rc,
};
type H = *mut c_void;
use crate::nnapi_runtime::ProcessLibrary as Library;
#[repr(C)]
struct Operand {
    kind: i32,
    dimensions: u32,
    sizes: *const u32,
    scale: f32,
    zero_point: i32,
}
macro_rules! api {
    ($($field:ident : $name:literal ($($arg:ty),*) -> $ret:ty;)+) => {
        struct Api { _library:Library, $($field:unsafe extern "C" fn($($arg),*) -> $ret,)+
            execution_timeout:Option<unsafe extern "C" fn(H,u64)->i32>,
            compilation_timeout:Option<unsafe extern "C" fn(H,u64)->i32>,
        }
        impl Api {
            fn open()->Result<Rc<Self>,String> {
                let library=Library::open(c"libneuralnetworks.so").ok_or("nnapi-runtime-unavailable")?;
                $(let symbol=library.symbol(CStr::from_bytes_with_nul(concat!($name,"\0").as_bytes()).expect("NNAPI symbol literal"));
                if symbol.is_null(){return Err(concat!("public-api-unavailable:",$name).into());}
                let $field=unsafe { std::mem::transmute::<H,unsafe extern "C" fn($($arg),*)->$ret>(symbol) };)+
                let e=library.symbol(c"ANeuralNetworksExecution_setTimeout");
                let c=library.symbol(c"ANeuralNetworksCompilation_setTimeout");
                let execution_timeout=if e.is_null(){None}else{Some(unsafe { std::mem::transmute::<H,unsafe extern "C" fn(H,u64)->i32>(e) })};
                let compilation_timeout=if c.is_null(){None}else{Some(unsafe { std::mem::transmute::<H,unsafe extern "C" fn(H,u64)->i32>(c) })};
                Ok(Rc::new(Self { _library:library, $($field,)+ execution_timeout, compilation_timeout }))
            }
        }
    }
}
api! {
    count:"ANeuralNetworks_getDeviceCount"(*mut u32)->i32;
    device:"ANeuralNetworks_getDevice"(u32,*mut *const c_void)->i32;
    name:"ANeuralNetworksDevice_getName"(*const c_void,*mut *const c_char)->i32;
    kind:"ANeuralNetworksDevice_getType"(*const c_void,*mut i32)->i32;
    level:"ANeuralNetworksDevice_getFeatureLevel"(*const c_void,*mut i64)->i32;
    create_model:"ANeuralNetworksModel_create"(*mut H)->i32;
    free_model:"ANeuralNetworksModel_free"(H)->();
    add_operand:"ANeuralNetworksModel_addOperand"(H,*const Operand)->i32;
    value:"ANeuralNetworksModel_setOperandValue"(H,i32,*const c_void,usize)->i32;
    add_operation:"ANeuralNetworksModel_addOperation"(H,i32,u32,*const u32,u32,*const u32)->i32;
    identify:"ANeuralNetworksModel_identifyInputsAndOutputs"(H,u32,*const u32,u32,*const u32)->i32;
    relax:"ANeuralNetworksModel_relaxComputationFloat32toFloat16"(H,bool)->i32;
    finish_model:"ANeuralNetworksModel_finish"(H)->i32;
    supported:"ANeuralNetworksModel_getSupportedOperationsForDevices"(H,*const *const c_void,u32,*mut bool)->i32;
    compile:"ANeuralNetworksCompilation_createForDevices"(H,*const *const c_void,u32,*mut H)->i32;
    preference:"ANeuralNetworksCompilation_setPreference"(H,i32)->i32;
    finish_compile:"ANeuralNetworksCompilation_finish"(H)->i32;
    free_compile:"ANeuralNetworksCompilation_free"(H)->();
    create_execution:"ANeuralNetworksExecution_create"(H,*mut H)->i32;
    free_execution:"ANeuralNetworksExecution_free"(H)->();
    input:"ANeuralNetworksExecution_setInput"(H,i32,*const Operand,*const c_void,usize)->i32;
    output:"ANeuralNetworksExecution_setOutput"(H,i32,*const Operand,*mut c_void,usize)->i32;
    compute:"ANeuralNetworksExecution_compute"(H)->i32;
}
fn check(code: i32, operation: &str) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("{operation}:{code}"))
    }
}
struct Compiled {
    api: Rc<Api>,
    model: H,
    compilation: H,
    shape: Shape,
}
impl Drop for Compiled {
    fn drop(&mut self) {
        unsafe {
            if !self.compilation.is_null() {
                (self.api.free_compile)(self.compilation);
            }
            if !self.model.is_null() {
                (self.api.free_model)(self.model);
            }
        }
    }
}
struct Execution {
    api: Rc<Api>,
    handle: H,
}
impl Drop for Execution {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                (self.api.free_execution)(self.handle);
            }
        }
    }
}
struct Device {
    handle: *const c_void,
    name: String,
}
pub(super) struct Backend {
    api: Rc<Api>,
    devices: Vec<Device>,
    compiled: Option<Compiled>,
}
impl Backend {
    pub fn open() -> Result<Self, String> {
        let api = Api::open()?;
        let mut count = 0;
        check(unsafe { (api.count)(&mut count) }, "devices")?;
        let mut devices = Vec::new();
        for i in 0..count.min(32) {
            let mut handle = ptr::null();
            check(unsafe { (api.device)(i, &mut handle) }, "get-device")?;
            if handle.is_null() {
                continue;
            }
            let mut kind = 0;
            let mut level = 0;
            if unsafe { (api.kind)(handle, &mut kind) } != 0 || kind != 4 {
                continue;
            }
            if unsafe { (api.level)(handle, &mut level) } != 0 || level < 3 {
                continue;
            }
            let mut name = ptr::null();
            check(unsafe { (api.name)(handle, &mut name) }, "device-name")?;
            let name = if name.is_null() {
                "unnamed-accelerator".into()
            } else {
                unsafe { CStr::from_ptr(name) }
                    .to_string_lossy()
                    .chars()
                    .take(256)
                    .collect()
            };
            devices.push(Device { handle, name });
        }
        if devices.is_empty() {
            return Err(
                "no-public-fp32-accelerator-device; GPU/CPU/unknown devices excluded".into(),
            );
        }
        Ok(Self {
            api,
            devices,
            compiled: None,
        })
    }
    fn compile(&self, shape: Shape, device: &Device) -> Result<Compiled, String> {
        let api = self.api.clone();
        let graph = Graph::new(shape.parts);
        let mut c = Compiled {
            api: api.clone(),
            model: ptr::null_mut(),
            compilation: ptr::null_mut(),
            shape,
        };
        check(unsafe { (api.create_model)(&mut c.model) }, "create-model")?;
        let sizes = [shape.rows as u32, shape.bins as u32];
        let tensor = Operand {
            kind: 3,
            dimensions: 2,
            sizes: sizes.as_ptr(),
            scale: 0.,
            zero_point: 0,
        };
        let scalar = Operand {
            kind: 1,
            dimensions: 0,
            sizes: ptr::null(),
            scale: 0.,
            zero_point: 0,
        };
        let activation = (shape.parts * 6) as u32;
        for id in 0..graph.operands {
            check(
                unsafe {
                    (api.add_operand)(c.model, if id == activation { &scalar } else { &tensor })
                },
                "operand",
            )?;
        }
        let none = 0i32;
        check(
            unsafe { (api.value)(c.model, activation as i32, (&none as *const i32).cast(), 4) },
            "activation",
        )?;
        for (op, inputs, out) in &graph.operations {
            check(
                unsafe { (api.add_operation)(c.model, *op, 3, inputs.as_ptr(), 1, out) },
                "operation",
            )?;
        }
        check(
            unsafe {
                (api.identify)(
                    c.model,
                    graph.inputs.len() as u32,
                    graph.inputs.as_ptr(),
                    4,
                    graph.outputs.as_ptr(),
                )
            },
            "inputs-outputs",
        )?;
        check(
            unsafe { (api.relax)(c.model, false) },
            "disable-fp16-relaxation",
        )?;
        check(unsafe { (api.finish_model)(c.model) }, "finish-model")?;
        let selected = [device.handle];
        let mut supported = vec![false; graph.operations.len()];
        check(
            unsafe { (api.supported)(c.model, selected.as_ptr(), 1, supported.as_mut_ptr()) },
            "supported-operations",
        )?;
        if supported.iter().any(|x| !*x) {
            return Err("accelerator-does-not-support-entire-fp32-graph".into());
        }
        check(
            unsafe { (api.compile)(c.model, selected.as_ptr(), 1, &mut c.compilation) },
            "compile-explicit-device",
        )?;
        check(
            unsafe { (api.preference)(c.compilation, 2) },
            "sustained-speed",
        )?;
        if let Some(timeout) = api.compilation_timeout {
            check(
                unsafe { timeout(c.compilation, 5_000_000_000) },
                "compilation-timeout",
            )?;
        }
        check(
            unsafe { (api.finish_compile)(c.compilation) },
            "finish-compile",
        )?;
        Ok(c)
    }
}
impl super::coordinator::Backend for Backend {
    fn deactivate(&mut self) { self.compiled = None; }
    fn configure(&mut self, shape: Shape) -> Result<String, String> {
        // Drop the previous shape before compiling: one bounded model, no growth
        // as songs or object counts change. Compilation stays off render threads.
        self.compiled = None;
        let mut failures = Vec::new();
        for device in &self.devices {
            match self.compile(shape, device) {
                Ok(compiled) => {
                    self.compiled = Some(compiled);
                    return Ok(device.name.clone());
                }
                Err(e) => failures.push(format!("{}:{e}", device.name)),
            }
        }
        Err(failures.join(";"))
    }
    fn execute(&mut self, batch: &mut Batch, timeout_ns: u64) -> Result<(), String> {
        let c = self.compiled.as_ref().ok_or("model-not-compiled")?;
        if c.shape != batch.shape {
            return Err("shape-mismatch".into());
        }
        let api = &c.api;
        let mut e = Execution {
            api: api.clone(),
            handle: ptr::null_mut(),
        };
        check(
            unsafe { (api.create_execution)(c.compilation, &mut e.handle) },
            "create-execution",
        )?;
        // A timeout is a driver hint. The independent renderer deadline still
        // handles a driver which ignores it; late packets cannot be reused.
        if let Some(timeout) = api.execution_timeout {
            check(
                unsafe { timeout(e.handle, timeout_ns) },
                "execution-timeout",
            )?;
        }
        let n = batch.shape.plane();
        batch.output.fill(f32::NAN);
        for (i, v) in batch.input.chunks_exact(n).enumerate() {
            check(
                unsafe {
                    (api.input)(
                        e.handle,
                        i as i32,
                        ptr::null(),
                        v.as_ptr().cast(),
                        v.len() * 4,
                    )
                },
                "set-input",
            )?;
        }
        for (i, v) in batch.output.chunks_exact_mut(n).enumerate() {
            check(
                unsafe {
                    (api.output)(
                        e.handle,
                        i as i32,
                        ptr::null(),
                        v.as_mut_ptr().cast(),
                        v.len() * 4,
                    )
                },
                "set-output",
            )?;
        }
        check(unsafe { (api.compute)(e.handle) }, "execute-audio-graph")
    }
}
