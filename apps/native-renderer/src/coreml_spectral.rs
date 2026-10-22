//! Core ML computes real current/transition HRTF spectra on its serial worker.
//! CPUAndNeuralEngine permits framework CPU fallback. Public APIs do not expose
//! per-prediction ANE counters; availability/requested units are not execution proof.
use super::{Batch, Shape};
use std::ffi::{CStr, c_char, c_void};
unsafe extern "C" {
    fn sda_coreml_neural_engine_available() -> i32;
    fn sda_coreml_create(
        model: *const u8,
        length: usize,
        rows: usize,
        bins: usize,
        inputs: usize,
        cpu_only: i32,
        error: *mut c_char,
        capacity: usize,
    ) -> *mut c_void;
    fn sda_coreml_predict(
        context: *mut c_void,
        input: *const f32,
        input_length: usize,
        output: *mut f32,
        output_length: usize,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn sda_coreml_free(context: *mut c_void);
}
struct Model(*mut c_void);
impl Drop for Model {
    fn drop(&mut self) {
        unsafe {
            sda_coreml_free(self.0);
        }
    }
}
pub(super) struct Backend {
    model: Option<Model>,
    shape: Option<Shape>,
    visible: i32,
}
fn message(bytes: &[c_char; 512]) -> String {
    unsafe { CStr::from_ptr(bytes.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}
impl Backend {
    pub fn open() -> Result<Self, String> {
        let visible = unsafe { sda_coreml_neural_engine_available() };
        if visible == 0 {
            return Err(
                "neural-engine-not-exposed-by-Core-ML; preserving-native-CPU-renderer".into(),
            );
        }
        Ok(Self {
            model: None,
            shape: None,
            visible,
        })
    }
}
impl super::coordinator::Backend for Backend {
    fn configure(&mut self, shape: Shape) -> Result<String, String> {
        self.model = None;
        self.shape = None;
        let bytes = super::coreml_model::encode(shape);
        let mut error = [0 as c_char; 512];
        let handle = unsafe {
            sda_coreml_create(
                bytes.as_ptr(),
                bytes.len(),
                shape.rows,
                shape.bins,
                shape.parts * 6,
                0,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if handle.is_null() {
            return Err(format!("coreml-model-unavailable:{}", message(&error)));
        }
        self.model = Some(Model(handle));
        self.shape = Some(shape);
        Ok("Core ML CPUAndNeuralEngine (actual device not observable)".into())
    }
    fn execute(&mut self, batch: &mut Batch, _timeout_ns: u64) -> Result<(), String> {
        if self.shape != Some(batch.shape) {
            return Err("coreml-batch-shape-mismatch".into());
        }
        let model = self.model.as_ref().ok_or("coreml-model-not-loaded")?;
        let mut error = [0 as c_char; 512];
        batch.output.fill(f32::NAN);
        let result = unsafe {
            sda_coreml_predict(
                model.0,
                batch.input.as_ptr(),
                batch.input.len(),
                batch.output.as_mut_ptr(),
                batch.output.len(),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if result != 0 {
            return Err(format!(
                "coreml-prediction-failed:{result}:{}",
                message(&error)
            ));
        }
        Ok(())
    }
    fn details(&self) -> serde_json::Value {
        serde_json::json!({"requestedComputeUnits":"cpuAndNeuralEngine","gpuAllowed":false,"frameworkCpuFallbackAllowed":true,
            "neuralEngineAvailable":match self.visible{1=>Some(true),0=>Some(false),_=>None},
            "hardwareExecutionVerified":false,"executionPlacement":"not-observable-per-prediction-through-public-API",
            "outputContract":"float32-with-unchanged-PCM-and-latency-qualification","internalPrecision":"Core-ML-managed",
            "modelCache":"one-private-graph-slot-no-PCM-no-input-weights"})
    }
}
