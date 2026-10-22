//! Public LiteRT 2.3 C API; NPU-only compilation must have zero CPU partitions.
//! Copied tensor IO, compilation and dispatch are included in qualification time.
use super::{Batch, Shape};
use crate::nnapi_runtime::ProcessLibrary;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
    rc::Rc,
};
type H = *mut c_void;
#[repr(C)]
union Value {
    integer: i64,
    text: *const c_char,
}
#[repr(C)]
struct Any {
    kind: i32,
    value: Value,
}
#[repr(C)]
struct OptionValue {
    tag: i32,
    value: Any,
}
#[repr(C)]
#[derive(Default)]
struct TensorType {
    element: i32,
    rank_and_stride: u32,
    dimensions: [i32; 8],
    strides: [u32; 8],
}
macro_rules! api {
    ($($field:ident:$name:literal($($arg:ty),*)->$ret:ty;)+)=>{
        struct Api {_library:ProcessLibrary,$($field:unsafe extern "C" fn($($arg),*)->$ret,)+}
        impl Api {fn open(path:&CStr)->Result<Rc<Self>,String>{
            let library=ProcessLibrary::open(path).ok_or("litert-runtime-not-loadable")?;
            $(let s=library.symbol(CStr::from_bytes_with_nul(concat!($name,"\0").as_bytes()).unwrap());
            if s.is_null(){return Err(concat!("litert-public-api-missing:",$name).into());}
            let $field=unsafe{std::mem::transmute::<H,unsafe extern "C" fn($($arg),*)->$ret>(s)};)+
            Ok(Rc::new(Self{_library:library,$($field,)+}))
        }}
    }
}
api! {
    environment:"LiteRtCreateEnvironment"(i32,*const OptionValue,*mut H)->i32;
    free_environment:"LiteRtDestroyEnvironment"(H)->();
    model:"LiteRtCreateModelFromBuffer"(H,*const c_void,usize,*mut H)->i32;
    free_model:"LiteRtDestroyModel"(H)->();
    subgraph:"LiteRtGetModelSubgraph"(H,usize,*mut H)->i32;
    input_tensor:"LiteRtGetSubgraphInput"(H,usize,*mut H)->i32;
    output_tensor:"LiteRtGetSubgraphOutput"(H,usize,*mut H)->i32;
    tensor_type:"LiteRtGetRankedTensorType"(H,*mut TensorType)->i32;
    options:"LiteRtCreateOptions"(*mut H)->i32;
    accelerators:"LiteRtSetOptionsHardwareAccelerators"(H,i32)->i32;
    free_options:"LiteRtDestroyOptions"(H)->();
    compile:"LiteRtCreateCompiledModel"(H,H,H,*mut H)->i32;
    free_compiled:"LiteRtDestroyCompiledModel"(H)->();
    fully_accelerated:"LiteRtCompiledModelIsFullyAccelerated"(H,*mut bool)->i32;
    non_cpu:"LiteRtCompiledModelIsNonCpuFullyAccelerated"(H,*mut bool)->i32;
    input_requirements:"LiteRtGetCompiledModelInputBufferRequirements"(H,usize,usize,*mut H)->i32;
    output_requirements:"LiteRtGetCompiledModelOutputBufferRequirements"(H,usize,usize,*mut H)->i32;
    requirement_size:"LiteRtGetTensorBufferRequirementsBufferSize"(H,*mut usize)->i32;
    buffer:"LiteRtCreateManagedTensorBufferFromRequirements"(H,*const TensorType,H,*mut H)->i32;
    buffer_size:"LiteRtGetTensorBufferSize"(H,*mut usize)->i32;
    free_buffer:"LiteRtDestroyTensorBuffer"(H)->();
    lock:"LiteRtLockTensorBuffer"(H,*mut H,i32)->i32;
    unlock:"LiteRtUnlockTensorBuffer"(H)->i32;
    run:"LiteRtRunCompiledModel"(H,usize,usize,*mut H,usize,*mut H)->i32;
}
fn check(code: i32, what: &str) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("litert-{what}:{code}"))
    }
}
struct Model {
    api: Rc<Api>,
    env: H,
    model: H,
    options: H,
    compiled: H,
    input: Vec<H>,
    output: Vec<H>,
    _bytes: Vec<u8>,
    _directory: CString,
    shape: Shape,
}
impl Drop for Model {
    fn drop(&mut self) {
        unsafe {
            for b in self.input.drain(..).chain(self.output.drain(..)) {
                (self.api.free_buffer)(b);
            }
            if !self.compiled.is_null() {
                (self.api.free_compiled)(self.compiled);
            }
            if !self.options.is_null() {
                (self.api.free_options)(self.options);
            }
            if !self.model.is_null() {
                (self.api.free_model)(self.model);
            }
            if !self.env.is_null() {
                (self.api.free_environment)(self.env);
            }
        }
    }
}
struct Locked<'a> {
    api: &'a Api,
    buffer: H,
    pointer: H,
    locked: bool,
}
impl<'a> Locked<'a> {
    fn new(api: &'a Api, buffer: H, mode: i32) -> Result<Self, String> {
        let mut pointer = ptr::null_mut();
        check(
            unsafe { (api.lock)(buffer, &mut pointer, mode) },
            "buffer-lock",
        )?;
        let result = Self {
            api,
            buffer,
            pointer,
            locked: true,
        };
        if pointer.is_null() {
            return Err("litert-null-locked-buffer".into());
        }
        Ok(result)
    }
    fn finish(mut self) -> Result<(), String> {
        self.locked = false;
        check(unsafe { (self.api.unlock)(self.buffer) }, "buffer-unlock")
    }
}
impl Drop for Locked<'_> {
    fn drop(&mut self) {
        if self.locked {
            unsafe {
                (self.api.unlock)(self.buffer);
            }
        }
    }
}
pub(super) struct Backend {
    model: Option<Model>,
    api: Rc<Api>,
    directory: CString,
    _pinned: Vec<ProcessLibrary>,
}
impl Backend {
    pub fn open(directory: &std::path::Path) -> Result<Self, String> {
        let cpath = |name: &str| {
            CString::new(directory.join(name).to_string_lossy().as_bytes())
                .map_err(|_| "invalid-litert-path".to_string())
        };
        let api = Api::open(&cpath("libLiteRt.so")?)?;
        let mut pinned = Vec::new();
        // Driver/plugin TLS callbacks must remain mapped through worker exit.
        for name in [
            "libLiteRtCompilerPlugin_Qualcomm.so",
            "libLiteRtDispatch_Qualcomm.so",
            "libLiteRtCompilerPlugin_MediaTek.so",
            "libLiteRtDispatch_MediaTek.so",
        ] {
            if let Some(lib) = ProcessLibrary::open(&cpath(name)?) {
                pinned.push(lib);
            }
        }
        if pinned.is_empty() {
            return Err("litert-vendor-plugins-unavailable".into());
        }
        let directory = CString::new(directory.to_string_lossy().as_bytes())
            .map_err(|_| "invalid-litert-directory")?;
        Ok(Self {
            model: None,
            api,
            directory,
            _pinned: pinned,
        })
    }
}
impl super::coordinator::Backend for Backend {
    fn configure(&mut self, shape: Shape) -> Result<String, String> {
        self.model = None;
        let mut m = Model {
            api: self.api.clone(),
            env: ptr::null_mut(),
            model: ptr::null_mut(),
            options: ptr::null_mut(),
            compiled: ptr::null_mut(),
            input: vec![],
            output: vec![],
            _bytes: super::litert_model::encode(shape),
            _directory: self.directory.clone(),
            shape,
        };
        let text = |tag| OptionValue {
            tag,
            value: Any {
                kind: 8,
                value: Value {
                    text: m._directory.as_ptr(),
                },
            },
        };
        let options = [
            text(0),
            text(1),
            text(22),
            OptionValue {
                tag: 24,
                value: Any {
                    kind: 2,
                    value: Value { integer: 4 },
                },
            },
        ];
        check(
            unsafe { (m.api.environment)(options.len() as i32, options.as_ptr(), &mut m.env) },
            "create-environment",
        )?;
        check(
            unsafe {
                (m.api.model)(
                    m.env,
                    m._bytes.as_ptr().cast(),
                    m._bytes.len(),
                    &mut m.model,
                )
            },
            "create-model",
        )?;
        check(unsafe { (m.api.options)(&mut m.options) }, "create-options")?;
        check(
            unsafe { (m.api.accelerators)(m.options, 4) },
            "NPU-only-options",
        )?;
        check(
            unsafe { (m.api.compile)(m.env, m.model, m.options, &mut m.compiled) },
            "compile-NPU-graph",
        )?;
        let mut full = false;
        let mut non_cpu = false;
        check(
            unsafe { (m.api.fully_accelerated)(m.compiled, &mut full) },
            "full-delegation-check",
        )?;
        check(
            unsafe { (m.api.non_cpu)(m.compiled, &mut non_cpu) },
            "non-CPU-delegation-check",
        )?;
        if !full || !non_cpu {
            return Err("litert-rejected-CPU-partitions-or-unaccelerated-graph".into());
        }
        let mut sub = ptr::null_mut();
        check(
            unsafe { (m.api.subgraph)(m.model, 0, &mut sub) },
            "main-subgraph",
        )?;
        let mut owned_bytes = shape.plane() * (6 * shape.parts + 8) * 4;
        for (is_input, count) in [(true, shape.parts * 6), (false, 4)] {
            for i in 0..count {
                let mut tensor = ptr::null_mut();
                let mut ty = TensorType::default();
                let mut requirements = ptr::null_mut();
                let mut bytes = 0;
                check(
                    unsafe {
                        (if is_input {
                            m.api.input_tensor
                        } else {
                            m.api.output_tensor
                        })(sub, i, &mut tensor)
                    },
                    "get-tensor",
                )?;
                check(
                    unsafe { (m.api.tensor_type)(tensor, &mut ty) },
                    "tensor-type",
                )?;
                if ty.element != 1
                    || ty.rank_and_stride & 127 != 2
                    || ty.dimensions[..2] != [shape.rows as i32, shape.bins as i32]
                {
                    return Err("litert-FP32-shape-contract-mismatch".into());
                }
                check(
                    unsafe {
                        (if is_input {
                            m.api.input_requirements
                        } else {
                            m.api.output_requirements
                        })(m.compiled, 0, i, &mut requirements)
                    },
                    "buffer-requirements",
                )?;
                check(
                    unsafe { (m.api.requirement_size)(requirements, &mut bytes) },
                    "buffer-requirement-size",
                )?;
                owned_bytes = owned_bytes
                    .checked_add(bytes)
                    .ok_or("litert-buffer-size-overflow")?;
                if bytes < shape.plane() * 4 || owned_bytes > 32 * 1024 * 1024 {
                    return Err("litert-bounded-buffer-budget-exceeded".into());
                }
                let mut buffer = ptr::null_mut();
                check(
                    unsafe { (m.api.buffer)(m.env, &ty, requirements, &mut buffer) },
                    "allocate-buffer",
                )?;
                if is_input {
                    m.input.push(buffer);
                } else {
                    m.output.push(buffer);
                }
                let mut actual_size = 0;
                check(
                    unsafe { (m.api.buffer_size)(buffer, &mut actual_size) },
                    "allocated-buffer-size",
                )?;
                if actual_size < shape.plane() * 4 {
                    return Err("litert-undersized-buffer".into());
                }
            }
        }
        self.model = Some(m);
        Ok("LiteRT vendor NPU (QNN/NeuroPilot; full non-CPU delegation)".into())
    }
    fn execute(&mut self, batch: &mut Batch, _timeout_ns: u64) -> Result<(), String> {
        let m = self.model.as_mut().ok_or("litert-model-unavailable")?;
        if batch.shape != m.shape {
            return Err("litert-batch-shape-mismatch".into());
        }
        let n = batch.shape.plane();
        for (&buffer, values) in m.input.iter().zip(batch.input.chunks_exact(n)) {
            let lock = Locked::new(&m.api, buffer, 1)?;
            unsafe {
                ptr::copy_nonoverlapping(values.as_ptr(), lock.pointer.cast::<f32>(), n);
            }
            lock.finish()?;
        }
        check(
            unsafe {
                (m.api.run)(
                    m.compiled,
                    0,
                    m.input.len(),
                    m.input.as_mut_ptr(),
                    m.output.len(),
                    m.output.as_mut_ptr(),
                )
            },
            "run",
        )?;
        batch.output.fill(f32::NAN);
        for (&buffer, values) in m.output.iter().zip(batch.output.chunks_exact_mut(n)) {
            let lock = Locked::new(&m.api, buffer, 0)?;
            unsafe {
                ptr::copy_nonoverlapping(lock.pointer.cast::<f32>(), values.as_mut_ptr(), n);
            }
            lock.finish()?;
        }
        Ok(())
    }
    fn details(&self) -> serde_json::Value {
        serde_json::json!({"runtime":"litert-vendor-npu-2.3.0","acceleratorKind":"npu","requestedComputeUnits":"NPU-only","fullyAccelerated":self.model.is_some(),"nonCpuFullyAccelerated":self.model.is_some(),"frameworkCpuFallbackAllowed":false,"outputContract":"float32-with-unchanged-audio-qualification","internalPrecision":"vendor-managed-and-output-qualified","modelCache":"one-in-memory-shape-no-PCM-no-persistent-models"})
    }
}
