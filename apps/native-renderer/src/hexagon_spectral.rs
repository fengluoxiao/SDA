//! Legacy Snapdragon floating-point HRTF MAC through public Hexagon NN/FastRPC.
//! DSP offload is reported as DSP, never as a dedicated NPU. No quantization.
use super::hexagon_graph::{self, Input};
use super::{Batch, Shape};
use crate::nnapi_runtime::ProcessLibrary;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    rc::Rc,
};
#[repr(C)]
#[derive(Clone, Copy)]
struct Output {
    rank: u32,
    sizes: [u32; 8],
    element_size: u32,
    zero: i32,
    step: f32,
}
#[repr(C)]
struct Tensor {
    batches: u32,
    height: u32,
    width: u32,
    depth: u32,
    data: *mut u8,
    len: i32,
    valid: u32,
    unused: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PerfInfo {
    node_id: u32,
    executions: u32,
    low: u32,
    high: u32,
}
macro_rules! api {
    ($($field:ident:$name:literal($($arg:ty),*)->$ret:ty;)+)=>{
        struct Api {_library:ProcessLibrary,$($field:unsafe extern "C" fn($($arg),*)->$ret,)+}
        impl Api {
            fn open(path:&CStr)->Result<Rc<Self>,String>{
                let library=ProcessLibrary::open(path).ok_or("legacy-hexagon-interface-not-loadable")?;
                $(let sym=library.symbol(CStr::from_bytes_with_nul(concat!($name,"\0").as_bytes()).unwrap());
                if sym.is_null(){return Err(concat!("legacy-hexagon-symbol-missing:",$name).into());}
                let $field=unsafe{std::mem::transmute::<*mut c_void,unsafe extern "C" fn($($arg),*)->$ret>(sym)};)+
                Ok(Rc::new(Self{_library:library,$($field,)+}))
            }
        }
    }
}
api! {
    global_init:"hexagon_nn_global_init"()->();
    global_free:"hexagon_nn_global_teardown"()->();
    supported:"hexagon_nn_is_device_supported"()->bool;
    config:"hexagon_nn_config"()->i32;
    init:"hexagon_nn_init"(*mut i32)->i32;
    teardown:"hexagon_nn_teardown"(i32)->i32;
    append:"hexagon_nn_append_node"(i32,u32,u32,i32,*const Input,i32,*const Output,i32)->i32;
    prepare:"hexagon_nn_prepare"(i32)->i32;
    execute:"hexagon_nn_execute_new"(i32,*const Tensor,i32,*mut Tensor,i32)->i32;
    op_name:"hexagon_nn_op_id_to_name"(u32,*mut c_char,i32)->i32;
    version:"hexagon_nn_version"(*mut i32)->i32;
    log:"hexagon_nn_getlog"(i32,*mut u8,i32)->i32;
    profile:"hexagon_nn_get_perfinfo"(i32,*mut PerfInfo,i32,*mut u32)->i32;
}
fn check(code: i32, what: &str) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("legacy-hexagon-{what}:{code}"))
    }
}
struct Model {
    api: Rc<Api>,
    id: i32,
    shape: Shape,
}
impl Drop for Model {
    fn drop(&mut self) {
        unsafe {
            (self.api.teardown)(self.id);
        }
    }
}
pub(super) struct Backend {
    model: Option<Model>,
    api: Rc<Api>,
    version: i32,
}
impl Backend {
    pub fn open(directory: &std::path::Path) -> Result<Self, String> {
        for name in ["libhexagon_interface.so", "libhexagon_nn_skel.so"] {
            if !directory.join(name).is_file() {
                return Err(format!("legacy-hexagon-runtime-not-installed:{name}"));
            }
        }
        let path = CString::new(
            directory
                .join("libhexagon_interface.so")
                .to_string_lossy()
                .as_bytes(),
        )
        .map_err(|_| "invalid-legacy-runtime-path")?;
        let api = Api::open(&path)?;
        if !unsafe { (api.supported)() } {
            return Err("legacy-hexagon-device-not-supported-by-public-interface".into());
        }
        unsafe {
            (api.global_init)();
        }
        let mut result = Self {
            model: None,
            api,
            version: 0,
        };
        check(unsafe { (result.api.config)() }, "RPC-config-unavailable")?;
        check(
            unsafe { (result.api.version)(&mut result.version) },
            "DSP-version-unavailable",
        )?;
        // Never blindly use op IDs across incompatible interface/skel versions.
        for (id, expected) in [
            (0, "INPUT"),
            (1, "OUTPUT"),
            (66, "Add_f"),
            (67, "Mul_f"),
            (75, "Sub_f"),
        ] {
            let mut name = [0 as c_char; 128];
            check(
                unsafe { (result.api.op_name)(id, name.as_mut_ptr(), name.len() as i32) },
                "op-name",
            )?;
            let actual = unsafe { CStr::from_ptr(name.as_ptr()) }.to_string_lossy();
            if actual != expected {
                return Err(format!("legacy-hexagon-float-op-unavailable:{id}:{actual}"));
            }
        }
        Ok(result)
    }
    #[cfg(feature = "pcm-diagnostic")]
    pub fn diagnostic_cycles(&self) -> Option<u64> {
        let model = self.model.as_ref()?;
        let mut nodes = [PerfInfo::default(); 128];
        let mut count = 0;
        let rc = unsafe {
            (self.api.profile)(model.id, nodes.as_mut_ptr(), nodes.len() as i32, &mut count)
        };
        if rc != 0 || count as usize > nodes.len() {
            return None;
        }
        Some(
            nodes[..count as usize]
                .iter()
                .filter(|p| p.executions > 0)
                .map(|p| (((p.high as u64) << 32) | p.low as u64) / p.executions as u64)
                .sum(),
        )
    }
    fn error(&self, id: i32, error: String) -> String {
        let mut bytes = [0u8; 1024];
        unsafe {
            (self.api.log)(id, bytes.as_mut_ptr(), bytes.len() as i32);
        }
        let end = bytes.iter().position(|&x| x == 0).unwrap_or(bytes.len());
        format!("{error}:{}", String::from_utf8_lossy(&bytes[..end]))
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.model = None;
        unsafe {
            (self.api.global_free)();
        }
    }
}
impl super::coordinator::Backend for Backend {
    fn configure(&mut self, shape: Shape) -> Result<String, String> {
        self.model = None;
        let mut id = 0;
        check(unsafe { (self.api.init)(&mut id) }, "graph-init")?;
        let model = Model {
            api: self.api.clone(),
            id,
            shape,
        };
        let desc = Output {
            rank: 4,
            sizes: [1, 1, shape.rows as u32, shape.bins as u32, 0, 0, 0, 0],
            element_size: 4,
            zero: 0,
            step: 0.,
        };
        for node in hexagon_graph::nodes(shape) {
            let outputs = vec![desc; node.outputs];
            let rc = unsafe {
                (self.api.append)(
                    id,
                    node.id,
                    node.operation,
                    0,
                    node.inputs.as_ptr(),
                    node.inputs.len() as i32,
                    outputs.as_ptr(),
                    outputs.len() as i32,
                )
            };
            if rc != 0 {
                return Err(self.error(id, format!("legacy-hexagon-append:{}:{rc}", node.id)));
            }
        }
        let rc = unsafe { (self.api.prepare)(id) };
        if rc != 0 {
            return Err(self.error(id, format!("legacy-hexagon-prepare:{rc}")));
        }
        self.model = Some(model);
        Ok(format!(
            "Hexagon legacy FP32 DSP (runtime {})",
            self.version
        ))
    }
    fn execute(&mut self, batch: &mut Batch, _timeout_ns: u64) -> Result<(), String> {
        let model = self
            .model
            .as_ref()
            .ok_or("legacy-hexagon-graph-unavailable")?;
        if model.shape != batch.shape {
            return Err("legacy-hexagon-shape-mismatch".into());
        }
        let n = batch.shape.plane();
        let tensor = |data: *mut f32| Tensor {
            batches: 1,
            height: 1,
            width: batch.shape.rows as u32,
            depth: batch.shape.bins as u32,
            data: data.cast(),
            len: (n * 4) as i32,
            valid: (n * 4) as u32,
            unused: 0,
        };
        let inputs: Vec<_> = batch
            .input
            .chunks_exact(n)
            .map(|v| tensor(v.as_ptr().cast_mut()))
            .collect();
        batch.output.fill(f32::NAN);
        let mut outputs: Vec<_> = batch
            .output
            .chunks_exact_mut(n)
            .map(|v| {
                let mut t = tensor(v.as_mut_ptr());
                t.valid = 0;
                t
            })
            .collect();
        let rc = unsafe {
            (self.api.execute)(
                model.id,
                inputs.as_ptr(),
                inputs.len() as i32,
                outputs.as_mut_ptr(),
                outputs.len() as i32,
            )
        };
        if rc != 0 {
            return Err(self.error(model.id, format!("legacy-hexagon-execute:{rc}")));
        }
        if outputs.iter().any(|v| {
            v.batches != 1
                || v.height != 1
                || v.width != batch.shape.rows as u32
                || v.depth != batch.shape.bins as u32
                || v.valid != n as u32 * 4
        }) {
            return Err("legacy-hexagon-output-contract-mismatch".into());
        }
        Ok(())
    }
    fn details(&self) -> serde_json::Value {
        serde_json::json!({"runtime":"legacy-hexagon-fp32-dsp","acceleratorKind":"dsp","requestedPrecision":"float32","quantization":false,"driverVersion":self.version,"frameworkCpuFallbackAllowed":false,"hardwarePlacement":"public-Hexagon-NN-RPC-DSP-interface","driverInternalPlacement":"not-observable-through-public-api"})
    }
}
