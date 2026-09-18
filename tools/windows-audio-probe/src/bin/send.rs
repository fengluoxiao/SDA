//! Send a known E-AC-3 IEC61937 fixture only to the SDA virtual endpoint.
use std::time::{Duration, Instant};
use windows::{
    Win32::{Devices::Properties::DEVPKEY_Device_FriendlyName, Media::Audio::*, System::Com::*},
    core::PCWSTR,
};
#[path = "../iec_format.rs"]
mod iec_format;
struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}
struct Stream(IAudioClient);
impl Drop for Stream {
    fn drop(&mut self) {
        let _ = unsafe { self.0.Stop() };
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("Usage: send <exact SDA endpoint ID> <fixture.spdif>".into());
    }
    if std::fs::metadata(&args[2])?.len() > 128 * 1024 * 1024 {
        return Err("Fixture exceeds 128 MiB".into());
    }
    let bytes = std::fs::read(&args[2])?;
    if bytes.is_empty() || bytes.len() % 24576 != 0 {
        return Err("Expected complete E-AC-3 IEC bursts".into());
    }
    for burst in bytes.chunks_exact(24576) {
        if burst[..6] != [0x72, 0xf8, 0x1f, 0x4e, 0x15, 0] {
            return Err("Not an E-AC-3 test burst".into());
        }
        let n = u16::from_le_bytes([burst[6], burst[7]]) as usize;
        if n < 6 || n > 24568 || n % 2 != 0 {
            return Err("Invalid E-AC-3 burst length".into());
        }
    }
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let _com = Com;
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let id: Vec<u16> = args[1].encode_utf16().chain(Some(0)).collect();
    let device = unsafe { enumerator.GetDevice(PCWSTR(id.as_ptr()))? };
    let name = unsafe {
        device
            .OpenPropertyStore(STGM_READ)?
            .GetValue(&DEVPKEY_Device_FriendlyName as *const _ as *const _)?
    }
    .to_string();
    if !name.contains("SDA Spatial Bitstream Input") {
        return Err("Refusing to send encoded bytes to a non-SDA device".into());
    }
    let mut client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let f = iec_format::format(0x0a, 2, 6, 0x3f);
    let supported =
        unsafe { client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &f.wave.Format, None) };
    if supported.0 != 0 {
        return Err(format!("Exact IEC descriptor not accepted: {supported:?}").into());
    }
    let initialized = unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_EXCLUSIVE,
            0,
            200000,
            200000,
            &f.wave.Format,
            None,
        )
    };
    if let Err(error) = initialized {
        if error.code() != AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
            return Err(error.into());
        }
        let frames = unsafe { client.GetBufferSize()? };
        let period = (10_000_000 * u64::from(frames) + 96000) / 192000;
        client = unsafe { device.Activate(CLSCTX_ALL, None)? };
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_EXCLUSIVE,
                0,
                period as i64,
                period as i64,
                &f.wave.Format,
                None,
            )?;
        }
    }
    let stream = Stream(client);
    let output: IAudioRenderClient = unsafe { stream.0.GetService()? };
    let capacity = unsafe { stream.0.GetBufferSize()? };
    let mut offset = 0usize;
    let mut started = false;
    let deadline = Instant::now() + Duration::from_secs_f64(bytes.len() as f64 / 768000.0 + 10.0);
    loop {
        if Instant::now() > deadline {
            return Err("Endpoint did not consume fixture before deadline".into());
        }
        let padding = unsafe { stream.0.GetCurrentPadding()? };
        if offset == bytes.len() && padding == 0 {
            break;
        }
        let frames = (capacity.saturating_sub(padding) as usize).min((bytes.len() - offset) / 4);
        if frames != 0 {
            let buffer = unsafe { output.GetBuffer(frames as u32)? };
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr().add(offset), buffer, frames * 4);
                output.ReleaseBuffer(frames as u32, 0)?;
            }
            offset += frames * 4;
        }
        if !started {
            unsafe {
                stream.0.Start()?;
            }
            started = true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    eprintln!(
        "Submitted and consumed {} IEC carrier bytes on {}",
        bytes.len(),
        name
    );
    Ok(())
}
