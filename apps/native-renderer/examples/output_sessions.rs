use windows::{core::Interface,Win32::{Media::Audio::*,System::Com::*}};
fn main()->windows::core::Result<()> {unsafe{
 CoInitializeEx(None,COINIT_MULTITHREADED).ok()?;
 let e:IMMDeviceEnumerator=CoCreateInstance(&MMDeviceEnumerator,None,CLSCTX_ALL)?;
 let devices=e.EnumAudioEndpoints(eRender,DEVICE_STATE_ACTIVE)?;
 for n in 0..devices.GetCount()? {
  let d=devices.Item(n)?;let p=d.GetId()?;let id=p.to_string()?;CoTaskMemFree(Some(p.0.cast()));
  let manager:IAudioSessionManager2=d.Activate(CLSCTX_ALL,None)?;
  let sessions=manager.GetSessionEnumerator()?;
  for i in 0..sessions.GetCount()? {
   let s=sessions.GetSession(i)?;
   let c:IAudioSessionControl2=s.cast()?;let v:ISimpleAudioVolume=s.cast()?;
   println!("{}",serde_json::json!({"endpoint":id,"pid":c.GetProcessId()?,"state":s.GetState()?.0,"volume":v.GetMasterVolume()?,"muted":v.GetMute()?.as_bool()}));
  }
 }
 Ok(())
}}
