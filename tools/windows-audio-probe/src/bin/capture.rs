//! Local privileged driver reader. stdout is SDAC binary snapshots, not audio.
use std::{
    io::{self, Read, Write},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE},
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
    let return_audio = std::env::args().any(|s| s == "--return");
    let device = Device(unsafe {
        CreateFileW(
            w!("\\\\.\\SdaSystemAudio"),
            GENERIC_READ.0 | if return_audio { GENERIC_WRITE.0 } else { 0 },
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?
    });
    if return_audio {
        let handle = device.0.0;
        // Fail immediately with an old driver, rather than silently accepting a
        // return channel that the endpoint cannot expose.
        unsafe {
            DeviceIoControl(device.0, 0x0022a008, None, 0, None, 0, None, None)?;
        }
        std::thread::spawn(move || {
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let mut input = io::stdin().lock();
                let mut bytes = [0u8; 3840];
                loop {
                    let mut length = [0u8; 4];
                    input.read_exact(&mut length)?;
                    let n = u32::from_le_bytes(length) as usize;
                    if n > bytes.len() || n % 8 != 0 {
                        return Err("invalid return packet".into());
                    }
                    input.read_exact(&mut bytes[..n])?;
                    unsafe {
                        DeviceIoControl(
                            HANDLE(handle),
                            0x0022a008,
                            Some(bytes.as_ptr().cast()),
                            n as u32,
                            None,
                            0,
                            None,
                            None,
                        )?;
                    }
                }
            })();
            if let Err(e) = result {
                eprintln!("System return closed: {e}");
                std::process::exit(1);
            }
        });
    }
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
