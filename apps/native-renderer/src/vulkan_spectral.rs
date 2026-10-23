//! Vulkan 1.0 compute on the existing audio worker; no vendor runtime bundle.
use super::{Batch,Shape,coordinator::Backend as ComputeBackend};
use std::{ffi::{c_char,c_void,CStr},ptr::NonNull};
#[repr(C)]
struct DeviceInfo{vendor:u32,device:u32,api:u32,driver:u32,queue_family:u32,name:[c_char;256]}
#[repr(C)]
#[derive(Default)]
struct Timings{upload:u64,dispatch_wait:u64,readback:u64,total:u64,host_cpu:u64,input_flags:u64,output_flags:u64,input_allocated:u64,output_allocated:u64}
unsafe extern "C"{
 fn sda_vk_open(code:*const u32,bytes:usize,info:*mut DeviceInfo,error:*mut c_char,capacity:usize)->*mut c_void;
 fn sda_vk_configure(handle:*mut c_void,plane:u32,parts:u32,cpu_bytes:u64,error:*mut c_char,capacity:usize)->i32;
 fn sda_vk_run(handle:*mut c_void,input:*const f32,inputs:usize,output:*mut f32,outputs:usize,timeout:u64,error:*mut c_char,capacity:usize)->i32;
 fn sda_vk_configure_resident(handle:*mut c_void,rows:u32,parts:u32,bins:u32,cpu_bytes:u64,error:*mut c_char,capacity:usize)->i32;
 fn sda_vk_run_resident(handle:*mut c_void,input:*const f32,inputs:usize,output:*mut f32,outputs:usize,dirty:*const u8,timeout:u64,error:*mut c_char,capacity:usize)->i32;
 fn sda_vk_close(handle:*mut c_void);
 fn sda_vk_timings(handle:*mut c_void,timings:*mut Timings);
}
pub(super) struct Backend{handle:NonNull<c_void>,info:DeviceInfo,shape:Option<Shape>}
fn reason(error:&[c_char;256])->String{unsafe{CStr::from_ptr(error.as_ptr())}.to_string_lossy().into_owned()}
impl Backend{
 pub fn open()->Result<Self,String>{
  let bytes=include_bytes!("vulkan_resident.spv");
  let code:Vec<u32>=bytes.chunks_exact(4).map(|x|u32::from_le_bytes(x.try_into().unwrap())).collect();
  if code.first()!=Some(&0x07230203) || bytes.len()%4!=0{return Err("invalid-build-time-Vulkan-shader".into());}
  let mut info=DeviceInfo{vendor:0,device:0,api:0,driver:0,queue_family:0,name:[0;256]};let mut error=[0;256];
  let handle=NonNull::new(unsafe{sda_vk_open(code.as_ptr(),bytes.len(),&mut info,error.as_mut_ptr(),error.len())})
   .ok_or_else(||format!("system-vulkan-unavailable:{}",reason(&error)))?;
  Ok(Self{handle,info,shape:None})
 }
 fn device_name(&self)->String{unsafe{CStr::from_ptr(self.info.name.as_ptr())}.to_string_lossy().into_owned()}
}
impl Drop for Backend{fn drop(&mut self){unsafe{sda_vk_close(self.handle.as_ptr())};}}
impl ComputeBackend for Backend{
 fn configure(&mut self,shape:Shape)->Result<String,String>{
  self.shape=None;let mut error=[0;256];
  let cpu_bytes=shape.plane().checked_mul(6*shape.parts+8).and_then(|n|n.checked_mul(4)).and_then(|n|n.checked_add(shape.rows*(std::mem::size_of::<Option<(u64,u64,bool)>>()+1))).ok_or("Vulkan audio shape overflow")?;
  let result=unsafe{sda_vk_configure_resident(self.handle.as_ptr(),shape.rows as u32,shape.parts as u32,shape.bins as u32,cpu_bytes as u64,error.as_mut_ptr(),error.len())};
  if result!=0{return Err(format!("system-vulkan-shape-unavailable:{}",reason(&error)));}self.shape=Some(shape);Ok(self.device_name())
 }
 fn packet(&self,shape:Shape)->Batch {Batch::resident(shape)}
 fn execute(&mut self,batch:&mut Batch,timeout_ns:u64)->Result<(),String>{
  if self.shape!=Some(batch.shape){return Err("Vulkan packet shape mismatch".into());}let mut error=[0;256];
  let result=unsafe{sda_vk_run_resident(self.handle.as_ptr(),batch.input.as_ptr(),batch.input.len(),batch.output.as_mut_ptr(),batch.output.len(),batch.filter_dirty.as_ptr(),timeout_ns,error.as_mut_ptr(),error.len())};
  if result!=0{return Err(format!("system-vulkan-audio:{}",reason(&error)));}
  let mut t=Timings::default();unsafe{sda_vk_timings(self.handle.as_ptr(),&mut t)};batch.worker_cpu_ns=t.host_cpu;Ok(())
 }
 fn details(&self)->serde_json::Value{
  let mut t=Timings::default();unsafe{sda_vk_timings(self.handle.as_ptr(),&mut t)};
  serde_json::json!({"runtime":"android-system-vulkan-compute-1.0","acceleratorKind":"gpu","cooperativeResident":true,"coefficientTransfer":"changed-rows-only","inputFormat":"interleaved-complex-owned-packet","deviceName":self.device_name(),"vendorId":self.info.vendor,"deviceId":self.info.device,"apiVersion":self.info.api,"driverVersion":self.info.driver,"queueFamily":self.info.queue_family,"computeUnits":"hardware-GPU","precision":"FP32-NoContraction-ordered-partitions","frameworkCpuFallbackAllowed":false,"memory":"persistent-host-visible-storage-buffers","totalAudioBufferBudgetMiB":32,"shaderCompilation":"build-time-SPIR-V-no-phone-compiler-SDK","runtimeLibraryLifetime":"process-pinned-system-loader","modelCache":"none","inputMemoryFlags":t.input_flags,"outputMemoryFlags":t.output_flags,"inputAllocationBytes":t.input_allocated,"outputAllocationBytes":t.output_allocated,"timingNs":{"upload":t.upload,"dispatchAndWait":t.dispatch_wait,"readback":t.readback,"total":t.total,"hostThreadCpu":t.host_cpu}})
 }
}
