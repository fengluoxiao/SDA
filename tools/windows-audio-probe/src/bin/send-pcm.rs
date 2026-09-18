//! Send isolated 48 kHz PCM channel tones only to the SDA virtual endpoint.
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
    if args.len() < 3 {
        return Err("Usage: send-pcm <SDA endpoint> <channels> [--shared] [--discrete] [--check]".into());
    }
    let shared = args.iter().any(|s| s == "--shared");
    let discrete = args.iter().any(|s| s == "--discrete");
    let mode = if shared {
        AUDCLNT_SHAREMODE_SHARED
    } else {
        AUDCLNT_SHAREMODE_EXCLUSIVE
    };
    let channels: u16 = args[2].parse()?;
    let mask = if discrete {
        if ![2,3,6,8,10,12,13,14,16,20,24].contains(&channels) { return Err("unsupported discrete count".into()); }
        0
    } else { match channels {
        2 => 3,
        6 => 0x3f,
        8 => 0x63f,
        12 => 0x2d63f,
        _ => return Err("unsupported channel count".into()),
    }};
    let block = usize::from(channels) * 2;
    let mut bytes = Vec::new();
    for frame in 0..(48000 * usize::from(channels)) {
        for channel in 0..usize::from(channels) {
            let sample: i16 = if frame / 48000 == channel {
                ((frame as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 1638.0) as i16
            } else {
                0
            };
            bytes.extend_from_slice(&sample.to_le_bytes());
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
        return Err("Refusing to send test tones to a non-SDA device".into());
    }
    let mut client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let mut f = iec_format::format(0x0a, channels, 6, mask);
    f.wave.Format.nSamplesPerSec = 48000;
    f.wave.Format.nAvgBytesPerSec = 48000 * block as u32;
    f.wave.Format.cbSize = 22;
    f.wave.SubFormat =
        windows::core::GUID::from_values(1, 0, 0x0010, [0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71]);
    let supported = unsafe { client.IsFormatSupported(mode, &f.wave.Format, None) };
    if !shared && supported.0 != 0 {
        return Err(format!("Exact PCM descriptor not accepted: {supported:?}").into());
    }
    if args.iter().any(|s| s == "--check") {
        println!("channels={channels} mask={mask:#x} accepted={}", supported.0==0);
        return Ok(());
    }
    let initialized = unsafe {
        client.Initialize(
            mode,
            if shared {
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY
            } else {
                0
            },
            200000,
            if shared { 0 } else { 200000 },
            &f.wave.Format,
            None,
        )
    };
    if let Err(error) = initialized {
        if error.code() != AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
            return Err(error.into());
        }
        let frames = unsafe { client.GetBufferSize()? };
        let period = (10_000_000 * u64::from(frames) + 24000) / 48000;
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
    let deadline = Instant::now()
        + Duration::from_secs_f64(bytes.len() as f64 / (48000 * block) as f64 + 10.0);
    loop {
        if Instant::now() > deadline {
            return Err("Endpoint did not consume fixture before deadline".into());
        }
        let padding = unsafe { stream.0.GetCurrentPadding()? };
        if offset == bytes.len() && padding == 0 {
            break;
        }
        let frames =
            (capacity.saturating_sub(padding) as usize).min((bytes.len() - offset) / block);
        if frames != 0 {
            let buffer = unsafe { output.GetBuffer(frames as u32)? };
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr().add(offset), buffer, frames * block);
                output.ReleaseBuffer(frames as u32, 0)?;
            }
            offset += frames * block;
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
        "Submitted and consumed {} PCM bytes on {}",
        bytes.len(),
        name
    );
    Ok(())
}
