//! Android runtime selection. Hardware/graph checks, not a handset whitelist.
use super::{Batch, Shape, coordinator::Backend as ComputeBackend};
use std::{path::PathBuf, sync::OnceLock};
struct Config {
    directory: PathBuf,
    sdk: u32,
    preparation: String,
}
static CONFIG: OnceLock<Config> = OnceLock::new();
pub(super) fn configured() -> bool {
    CONFIG.get().is_some()
}
pub(super) fn configure(directory: &str, sdk: u32, preparation: &str) -> Result<(), String> {
    if !directory.is_empty() && !std::path::Path::new(directory).is_absolute() {
        return Err("accelerator-runtime-directory-must-be-absolute".into());
    }
    CONFIG
        .set(Config {
            directory: directory.into(),
            sdk,
            preparation: preparation.into(),
        })
        .map_err(|_| "accelerator-runtime-already-configured".into())
}
pub(super) struct Backend {
    active: Option<Box<dyn ComputeBackend>>,
    notes: Vec<String>,
}
impl Backend {
    pub fn open() -> Result<Self, String> {
        if !configured() {
            return Err("waiting-for-host-runtime-preparation".into());
        }
        Ok(Self {
            active: None,
            notes: vec![],
        })
    }
}
impl ComputeBackend for Backend {
    fn deactivate(&mut self) {
        self.active = None;
    }
    fn configure(&mut self, shape: Shape) -> Result<String, String> {
        self.active = None;
        self.notes.clear();
        let c = CONFIG
            .get()
            .ok_or("Android runtime configuration missing")?;
        // System GPU compute is the default; no packaged driver/compiler SDK.
        match super::vulkan::Backend::open() {
            Ok(mut b) => match b.configure(shape) {
                Ok(name) => { self.active=Some(Box::new(b)); return Ok(name); }
                Err(e) => self.notes.push(e),
            },
            Err(e) => self.notes.push(e),
        }
        if !c.preparation.is_empty() {
            self.notes.push(c.preparation.clone());
        }
        if !c.directory.as_os_str().is_empty() && c.sdk >= 31 && cfg!(target_arch = "aarch64") {
            match super::litert::Backend::open(&c.directory.join("common")) {
                Ok(mut b) => match b.configure(shape) {
                    Ok(name) => {
                        self.active = Some(Box::new(b));
                        return Ok(name);
                    }
                    Err(e) => self.notes.push(e),
                },
                Err(e) => self.notes.push(e),
            }
        }
        match super::nnapi::Backend::open() {
            Ok(mut b) => match b.configure(shape) {
                Ok(name) => {
                    self.active = Some(Box::new(b));
                    return Ok(name);
                }
                Err(e) => self.notes.push(e),
            },
            Err(e) => self.notes.push(e),
        }
        if !c.directory.as_os_str().is_empty() {
            match super::hexagon::Backend::open(&c.directory.join("legacy")) {
                Ok(mut b) => match b.configure(shape) {
                    Ok(name) => {
                        self.active = Some(Box::new(b));
                        return Ok(name);
                    }
                    Err(e) => self.notes.push(e),
                },
                Err(e) => self.notes.push(e),
            }
        }
        Err(format!(
            "no-qualified-Android-coprocessor-backend:{}",
            self.notes.join(";")
        ))
    }
    fn packet(&self,shape:Shape)->Batch {self.active.as_ref().map_or_else(||Batch::new(shape),|b|b.packet(shape))}
    fn execute(&mut self, batch: &mut Batch, timeout: u64) -> Result<(), String> {
        self.active
            .as_mut()
            .ok_or("Android audio accelerator unavailable")?
            .execute(batch, timeout)
    }
    fn details(&self) -> serde_json::Value {
        let mut details = self
            .active
            .as_ref()
            .map(|b| b.details())
            .unwrap_or_else(|| serde_json::json!({}));
        if details["runtime"].is_null() {
            details["runtime"] = "nnapi-explicit-accelerator".into();
            details["acceleratorKind"] = "npu".into();
        }
        details["selectionNotes"] = self.notes.clone().into();
        details
    }
}
