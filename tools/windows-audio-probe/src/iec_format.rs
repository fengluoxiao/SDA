use windows::{Win32::Media::Audio::*, core::GUID};
// ksmedia.h WAVEFORMATEXTENSIBLE_IEC61937. The extra fields describe
// encoded content, while Format describes the IEC carrier.
#[repr(C, packed)]
pub struct IecFormat {
    pub wave: WAVEFORMATEXTENSIBLE,
    pub encoded_rate: u32,
    encoded_channels: u32,
    encoded_bytes_per_second: u32,
}

pub fn format(subtype: u32, channels: u16, encoded_channels: u32, mask: u32) -> IecFormat {
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
