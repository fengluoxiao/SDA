use super::{Batch, Shape, coordinator::Backend as AudioBackend};
use std::ffi::{CStr,c_char,c_void};
use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};
static FOREGROUND:AtomicBool=AtomicBool::new(false);
static EPOCH:AtomicU64=AtomicU64::new(1);
unsafe extern "C" {
 fn sda_metal_set_allowed(allowed:i32);
 fn sda_metal_completed_batches()->u64;
 fn sda_metal_create(rows:usize,bins:usize,parts:usize,error:*mut c_char,capacity:usize)->*mut c_void;
 fn sda_metal_predict(context:*mut c_void,input:*const f32,ni:usize,output:*mut f32,no:usize,timeout:u64,error:*mut c_char,capacity:usize)->i32;
 fn sda_metal_free(context:*mut c_void);
}
pub(super) fn foreground()->bool{FOREGROUND.load(Ordering::Acquire)}
pub(super) fn epoch()->u64{EPOCH.load(Ordering::Acquire)}
pub(super) fn transition(active:bool){
 // Disable submissions before UIKit can suspend GPU access.
 unsafe{sda_metal_set_allowed(active as i32)};
 if FOREGROUND.swap(active,Ordering::AcqRel)!=active{EPOCH.fetch_add(1,Ordering::AcqRel);}
}
pub(super) fn new_session(){EPOCH.fetch_add(1,Ordering::AcqRel);}
pub(super) fn gpu_batches()->u64{unsafe{sda_metal_completed_batches()}}
struct Metal(*mut c_void);
impl Drop for Metal{fn drop(&mut self){unsafe{sda_metal_free(self.0)}}}
fn message(error:&[c_char;512])->String{unsafe{CStr::from_ptr(error.as_ptr())}.to_string_lossy().into_owned()}
pub(super) struct Backend {metal:Option<Metal>,neural:Option<super::coreml::Backend>,shape:Option<Shape>,gpu:bool,epoch:u64}
impl Backend {pub fn open()->Result<Self,String>{Ok(Self{metal:None,neural:None,shape:None,gpu:false,epoch:0})}}
impl AudioBackend for Backend {
 fn deactivate(&mut self){self.metal=None;self.neural=None;self.shape=None;}
 fn configure(&mut self,shape:Shape)->Result<String,String>{
  self.deactivate();self.gpu=foreground();self.epoch=epoch();
  let description=if self.gpu {
   let mut error=[0 as c_char;512];let p=unsafe{sda_metal_create(shape.rows,shape.bins,shape.parts,error.as_mut_ptr(),error.len())};
   if p.is_null(){return Err(format!("metal-unavailable:{}",message(&error)));}
   self.metal=Some(Metal(p));"Metal GPU FP32 spectral MAC".to_string()
  } else {
   let mut backend=super::coreml::Backend::open()?;let result=backend.configure(shape)?;self.neural=Some(backend);result
  };
  self.shape=Some(shape);Ok(description)
 }
 fn execute(&mut self,batch:&mut Batch,timeout:u64)->Result<(),String>{
  if self.epoch!=epoch()||self.gpu!=foreground()||self.shape!=Some(batch.shape){return Err("apple-compute-session-changed".into());}
  if let Some(metal)=&self.metal {
   let mut error=[0 as c_char;512];batch.output.fill(f32::NAN);
   let result=unsafe{sda_metal_predict(metal.0,batch.input.as_ptr(),batch.input.len(),batch.output.as_mut_ptr(),batch.output.len(),timeout,error.as_mut_ptr(),error.len())};
   if result!=0 {self.metal=None;return Err(format!("metal-execution:{result}:{}",message(&error)));}
   Ok(())
  } else {self.neural.as_mut().ok_or("apple-compute-not-configured")?.execute(batch,timeout)}
 }
 fn details(&self)->serde_json::Value{
  let mut v=self.neural.as_ref().map(|n|n.details()).unwrap_or_else(||serde_json::json!({"requestedComputeUnits":"metalGPU","frameworkCpuFallbackAllowed":false,"internalPrecision":"float32-no-fast-math"}));
  v["selectedMode"]=if self.gpu{"foreground-gpu"}else{"background-cpu-and-neural-engine"}.into();v["playbackEpoch"]=self.epoch.into();v
 }
}
