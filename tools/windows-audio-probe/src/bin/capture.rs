//! Local privileged driver reader. stdout is SDAC binary snapshots, not audio.
use std::{
    io::{self, Write},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, GENERIC_READ, HANDLE},
        Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE, OPEN_EXISTING},
        System::IO::DeviceIoControl,
    },
    core::w,
};
struct Device(HANDLE);
impl Drop for Device {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seconds = std::env::args()
        .nth(1)
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(3600);
    if !(1..=86400).contains(&seconds) {
        return Err("duration must be 1..86400 seconds".into());
    }
    let device = Device(unsafe {
        CreateFileW(
            w!("\\\\.\\SdaSystemAudio"),
            GENERIC_READ.0,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?
    });
    let mut output = io::stdout().lock();
    let mut data = vec![0u8; 65536];
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut last_state = [0u8; 0x30];
    while Instant::now() < deadline {
        let mut returned = 0;
        unsafe {
            DeviceIoControl(
                device.0,
                0x00226004,
                None,
                0,
                Some(data.as_mut_ptr().cast()),
                data.len() as u32,
                Some(&mut returned),
                None,
            )?;
        }
        let n = returned as usize;
        if n < 120 || n > data.len() || &data[..4] != b"SDAC" || data[4..8] != 1u32.to_le_bytes() {
            return Err("invalid driver capture header".into());
        }
        let payload = u32::from_le_bytes(data[44..48].try_into()?) as usize;
        let format = u32::from_le_bytes(data[48..52].try_into()?) as usize;
        if payload != n - 120 || format > 64 {
            return Err("invalid driver capture lengths".into());
        }
        if payload != 0 || last_state != data[..0x30] {
            output.write_all(&data[..n])?;
            output.flush()?;
            last_state.copy_from_slice(&data[..0x30]);
        }
        if payload == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}
