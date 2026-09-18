use windows::Win32::{Media::Audio::*, System::Com::*};

mod iec_format;
#[cfg(test)]
use iec_format::IecFormat;
use iec_format::format;

struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn main() -> windows::core::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let _com = Com;
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let devices = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)? };
    println!("SDA read-only IEC61937 probe; no stream is initialized or started.");
    println!("S_OK only means this exact descriptor is accepted, not object capture support.");
    let candidates = [
        ("E-AC-3 / 48k / 5.1", format(0x0a, 2, 6, 0x3f)),
        ("E-AC-3 Atmos / 48k / 5.1", format(0x10a, 2, 6, 0x3f)),
        ("E-AC-3 / stereo carrier mask", format(0x0a, 2, 6, 3)),
        ("E-AC-3 / unspecified carrier mask", format(0x0a, 2, 6, 0)),
        ("MLP MAT 1 / 48k / 7.1", format(0x0c, 8, 8, 0x63f)),
    ];
    for index in 0..unsafe { devices.GetCount()? } {
        let device = unsafe { devices.Item(index)? };
        let id = unsafe { device.GetId()? };
        let name = unsafe { id.to_string() };
        unsafe {
            CoTaskMemFree(Some(id.0.cast()));
        }
        println!("\nEndpoint: {}", name?);
        let client: IAudioClient = match unsafe { device.Activate(CLSCTX_ALL, None) } {
            Ok(client) => client,
            Err(err) => {
                println!("  activation failed: {err}");
                continue;
            }
        };
        for (label, descriptor) in &candidates {
            let result = unsafe {
                client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &descriptor.wave.Format, None)
            };
            println!(
                "  {label}: {} (HRESULT 0x{:08X})",
                if result.0 == 0 {
                    "accepted"
                } else {
                    "not accepted / error"
                },
                result.0 as u32
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_sdk_iec_layout() {
        assert_eq!(size_of::<IecFormat>(), 52);
        assert_eq!(std::mem::offset_of!(IecFormat, encoded_rate), 40);
        let f = format(0x0a, 2, 6, 0x3f);
        let base = f.wave.Format;
        let size = base.cbSize;
        let byte_rate = base.nAvgBytesPerSec;
        assert_eq!(usize::from(size) + size_of::<WAVEFORMATEX>(), 52);
        assert_eq!(byte_rate, 768000);
    }
}
