use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW,
    SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_IO_PENDING, ERROR_NO_MORE_ITEMS,
    ERROR_OPERATION_ABORTED, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
    WriteFile,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, DeviceIoControl, GetOverlappedResult, OVERLAPPED,
};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};

const RECEIVE_TIMEOUT_MS: u32 = 200;
const SEND_TIMEOUT_MS: u32 = 5_000;
const SDA_L2CAP_INTERFACE: windows_sys::core::GUID =
    windows_sys::core::GUID::from_u128(0x8d09ce09_58c6_4f67_95a7_9824b4d54cb3);
const IOCTL_SDA_QUERY_CHANNEL_INFO: u32 = 0x0041_6000;
const L2CAP_ENHANCED_RETRANSMISSION_MODE: u32 = 0x03;
const SUPPORTED_AIRPODS_PIDS: [&str; 8] = [
    "200E", // AirPods Pro
    "2014", // AirPods Pro 2 (Lightning)
    "2024", // AirPods Pro 2 (USB-C)
    "2013", // AirPods 3
    "2019", // AirPods 4
    "201B", // AirPods 4 (ANC)
    "200A", // AirPods Max
    "201F", // AirPods Max (USB-C)
];

pub struct L2capSocket {
    channel: DriverChannel,
}

impl L2capSocket {
    pub fn connect_airpods() -> Result<Self, String> {
        DriverChannel::connect().map(|channel| Self { channel })
    }

    pub fn send_packet(&self, packet: &[u8]) -> Result<(), String> {
        self.channel.send_packet(packet)
    }

    pub fn receive_packet(&self, buffer: &mut [u8]) -> Result<Option<usize>, String> {
        self.channel.receive_packet(buffer)
    }

    pub fn local_address(&self) -> u64 {
        self.channel.local_address
    }
}

struct DriverChannel {
    handle: HANDLE,
    local_address: u64,
}

#[derive(Default)]
#[repr(C)]
struct DriverChannelInfo {
    size: u32,
    negotiated_mode: u32,
    out_mtu: u32,
    in_mtu: u32,
    device_id_sdp_published: u32,
    device_id_sdp_status: i32,
    local_features_mask: u64,
    local_bth_address: u64,
}

impl DriverChannel {
    fn connect() -> Result<Self, String> {
        let path = driver_device_path()?;
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(last_win32_error("could not open SDA AirPods L2CAP driver"));
        }
        let mut channel = Self {
            handle,
            local_address: 0,
        };
        match channel.channel_info() {
            Ok(info) => {
                channel.local_address = info.local_bth_address;
                let mode = if info.negotiated_mode == L2CAP_ENHANCED_RETRANSMISSION_MODE {
                    "enhanced-retransmission"
                } else {
                    "basic-or-other"
                };
                eprintln!(
                    "AirPods transport: L2CAP mode is {mode} ({}, MTU {}/{}, host-features=0x{:x}, local-address={:012x}, device-id-sdp={}/{:#x})",
                    info.negotiated_mode,
                    info.out_mtu,
                    info.in_mtu,
                    info.local_features_mask,
                    info.local_bth_address,
                    info.device_id_sdp_published != 0,
                    info.device_id_sdp_status
                );
            }
            Err(error) => eprintln!("AirPods transport: channel diagnostics unavailable: {error}"),
        }
        Ok(channel)
    }

    fn channel_info(&self) -> Result<DriverChannelInfo, String> {
        let event = Event::create()?;
        let mut overlapped: OVERLAPPED = unsafe { zeroed() };
        overlapped.hEvent = event.0;
        let mut info = DriverChannelInfo::default();
        let started = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_SDA_QUERY_CHANNEL_INFO,
                std::ptr::null(),
                0,
                &mut info as *mut DriverChannelInfo as *mut std::ffi::c_void,
                size_of::<DriverChannelInfo>() as u32,
                null_mut(),
                &mut overlapped,
            )
        };
        let transferred = self.finish_io(started, &mut overlapped, SEND_TIMEOUT_MS, false)?;
        if transferred != size_of::<DriverChannelInfo>() as u32
            || info.size != size_of::<DriverChannelInfo>() as u32
        {
            return Err("profile driver returned invalid channel diagnostics".into());
        }
        Ok(info)
    }

    fn send_packet(&self, packet: &[u8]) -> Result<(), String> {
        let event = Event::create()?;
        let mut overlapped: OVERLAPPED = unsafe { zeroed() };
        overlapped.hEvent = event.0;
        let started = unsafe {
            WriteFile(
                self.handle,
                packet.as_ptr(),
                packet.len() as u32,
                null_mut(),
                &mut overlapped,
            )
        };
        let transferred = self.finish_io(started, &mut overlapped, SEND_TIMEOUT_MS, false)?;
        if transferred != packet.len() as u32 {
            return Err("profile driver only accepted part of the AirPods packet".into());
        }
        Ok(())
    }

    fn receive_packet(&self, buffer: &mut [u8]) -> Result<Option<usize>, String> {
        let event = Event::create()?;
        let mut overlapped: OVERLAPPED = unsafe { zeroed() };
        overlapped.hEvent = event.0;
        let started = unsafe {
            ReadFile(
                self.handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                null_mut(),
                &mut overlapped,
            )
        };
        self.finish_io(started, &mut overlapped, RECEIVE_TIMEOUT_MS, true)
            .map(|size| if size == 0 { None } else { Some(size as usize) })
    }

    fn finish_io(
        &self,
        started: i32,
        overlapped: &mut OVERLAPPED,
        timeout_ms: u32,
        timeout_is_idle: bool,
    ) -> Result<u32, String> {
        if started == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                return Err(format!("profile driver I/O failed ({error})"));
            }
        }
        let wait = unsafe { WaitForSingleObject(overlapped.hEvent, timeout_ms) };
        if wait == WAIT_TIMEOUT {
            unsafe {
                CancelIoEx(self.handle, overlapped);
                WaitForSingleObject(overlapped.hEvent, INFINITE);
            }
            let mut ignored = 0;
            unsafe { GetOverlappedResult(self.handle, overlapped, &mut ignored, 0) };
            return if timeout_is_idle {
                Ok(0)
            } else {
                Err("profile driver write timed out".into())
            };
        }
        if wait != WAIT_OBJECT_0 {
            unsafe {
                CancelIoEx(self.handle, overlapped);
                WaitForSingleObject(overlapped.hEvent, INFINITE);
            }
            let mut ignored = 0;
            unsafe { GetOverlappedResult(self.handle, overlapped, &mut ignored, 0) };
            return Err(format!("profile driver wait failed ({wait})"));
        }
        let mut transferred = 0;
        if unsafe { GetOverlappedResult(self.handle, overlapped, &mut transferred, 0) } == 0 {
            let error = unsafe { GetLastError() };
            if timeout_is_idle && error == ERROR_OPERATION_ABORTED {
                return Ok(0);
            }
            return Err(format!("profile driver I/O completion failed ({error})"));
        }
        Ok(transferred)
    }
}

impl Drop for DriverChannel {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

struct Event(HANDLE);

impl Event {
    fn create() -> Result<Self, String> {
        let handle = unsafe { CreateEventW(null_mut(), 1, 0, null_mut()) };
        if handle.is_null() {
            return Err(last_win32_error(
                "could not create profile driver I/O event",
            ));
        }
        Ok(Self(handle))
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct DeviceInfoSet(HDEVINFO);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

fn driver_device_path() -> Result<Vec<u16>, String> {
    let raw = unsafe {
        SetupDiGetClassDevsW(
            &SDA_L2CAP_INTERFACE,
            null_mut(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    if raw == -1_isize {
        return Err(last_win32_error(
            "SDA AirPods L2CAP driver is not installed",
        ));
    }
    let devices = DeviceInfoSet(raw);
    let mut index = 0;
    loop {
        let mut interface_data = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if unsafe {
            SetupDiEnumDeviceInterfaces(
                devices.0,
                null_mut(),
                &SDA_L2CAP_INTERFACE,
                index,
                &mut interface_data,
            )
        } == 0
        {
            let error = unsafe { GetLastError() };
            return Err(if error == ERROR_NO_MORE_ITEMS {
                "SDA AirPods L2CAP driver has no active supported AirPods device".into()
            } else {
                format!("could not enumerate SDA AirPods L2CAP driver ({error})")
            });
        }
        index += 1;

        let mut required = 0;
        unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                devices.0,
                &interface_data,
                null_mut(),
                0,
                &mut required,
                null_mut(),
            );
        }
        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER || required == 0 {
            return Err(last_win32_error("could not size SDA driver device path"));
        }
        let mut storage = vec![0_u8; required as usize];
        let detail = storage.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        unsafe { (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 };
        let mut device_info = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                devices.0,
                &interface_data,
                detail,
                required,
                null_mut(),
                &mut device_info,
            )
        } == 0
        {
            return Err(last_win32_error("could not read SDA driver device path"));
        }
        if !is_supported_airpods_instance_id(&device_instance_id(devices.0, &mut device_info)?) {
            continue;
        }

        let path_start = unsafe { (*detail).DevicePath.as_ptr() };
        let capacity = (required as usize - 4) / size_of::<u16>();
        let path_slice = unsafe { std::slice::from_raw_parts(path_start, capacity) };
        let length = path_slice
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(capacity);
        let mut path = path_slice[..length].to_vec();
        path.push(0);
        return Ok(path);
    }
}

fn device_instance_id(
    devices: HDEVINFO,
    device_info: &mut SP_DEVINFO_DATA,
) -> Result<String, String> {
    let mut required = 0;
    unsafe {
        SetupDiGetDeviceInstanceIdW(devices, device_info, null_mut(), 0, &mut required);
    }
    if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER || required == 0 {
        return Err(last_win32_error("could not size driver device identity"));
    }
    let mut buffer = vec![0_u16; required as usize];
    if unsafe {
        SetupDiGetDeviceInstanceIdW(
            devices,
            device_info,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err(last_win32_error("could not read driver device identity"));
    }
    let length = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..length]))
}

fn is_supported_airpods_instance_id(instance_id: &str) -> bool {
    let uppercase = instance_id.to_ascii_uppercase();
    uppercase.contains("_VID&0001004C_")
        && SUPPORTED_AIRPODS_PIDS
            .iter()
            .any(|pid| uppercase.contains(&format!("_PID&{pid}")))
}

fn last_win32_error(context: &str) -> String {
    let error = unsafe { GetLastError() };
    format!("{context} ({error})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_supported_apple_airpods_profile_ids() {
        for pid in SUPPORTED_AIRPODS_PIDS {
            assert!(is_supported_airpods_instance_id(&format!(
                "BTHENUM\\{{AACP}}_VID&0001004C_PID&{pid}\\INSTANCE"
            )));
        }
        assert!(is_supported_airpods_instance_id(
            "bthenum\\{aacp}_vid&0001004c_pid&2014\\instance"
        ));
    }

    #[test]
    fn rejects_unsupported_or_non_apple_profile_ids() {
        for pid in ["2002", "200F", "FFFF"] {
            assert!(!is_supported_airpods_instance_id(&format!(
                "BTHENUM\\{{AACP}}_VID&0001004C_PID&{pid}\\INSTANCE"
            )));
        }
        assert!(!is_supported_airpods_instance_id(
            "BTHENUM\\{AACP}_VID&0002004C_PID&2014\\INSTANCE"
        ));
        assert!(!is_supported_airpods_instance_id(
            "BTHENUM\\{AACP}_VID&0001004C\\INSTANCE"
        ));
    }
}
