use windows::{
    Win32::{Media::Audio::*, System::Com::*},
    core::GUID,
};

// ksmedia.h WAVEFORMATEXTENSIBLE_IEC61937. The extra fields describe
// encoded content, while Format describes the IEC carrier.
#[repr(C, packed)]
struct IecFormat {
    wave: WAVEFORMATEXTENSIBLE,
    encoded_rate: u32,
    encoded_channels: u32,
    encoded_bytes_per_second: u32,
}

fn format(subtype: u32, channels: u16, encoded_channels: u32, mask: u32) -> IecFormat {
    IecFormat {
        wave: WAVEFORMATEXTENSIBLE {
            Format: WAVEFORMATEX {
                wFormatTag: 0xfffe,
                nChannels: channels,
                nSamplesPerSec: 192000,
                nAvgBytesPerSec: 192000 * u32::from(channels) * 2,
                nBlockAlign: channels * 2,
                wBitsPerSample: 16,
                cbSize: 34,
            },
            Samples: WAVEFORMATEXTENSIBLE_0 {
                wValidBitsPerSample: 16,
            },
            dwChannelMask: mask,
            SubFormat: GUID::from_values(
                subtype,
                0x0cea,
                0x0010,
                [0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71],
            ),
        },
        encoded_rate: 48000,
        encoded_channels,
        encoded_bytes_per_second: 0,
    }
}

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
