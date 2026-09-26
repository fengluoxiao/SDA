//! Windows MMCSS registration for threads that feed or render live audio.

#[cfg(windows)]
pub(super) struct ProAudio(
    Option<windows::Win32::Foundation::HANDLE>,
);

#[cfg(windows)]
impl ProAudio {
    pub(super) fn enter() -> Self {
        use windows::{
            Win32::System::Threading::{
                AvSetMmThreadCharacteristicsW, AvSetMmThreadPriority, AVRT_PRIORITY_HIGH,
            },
            core::PCWSTR,
        };

        // MMCSS has a registered "Pro Audio" task on supported Windows builds.
        // Failure is deliberately non-fatal: ordinary process priority remains
        // the fallback for stripped-down Windows installations.
        let task_name = [80_u16, 114, 111, 32, 65, 117, 100, 105, 111, 0];
        let mut task_index = 0;
        let handle = unsafe {
            AvSetMmThreadCharacteristicsW(PCWSTR(task_name.as_ptr()), &mut task_index)
        }
        .ok();
        if let Some(handle) = handle {
            let _ = unsafe { AvSetMmThreadPriority(handle, AVRT_PRIORITY_HIGH) };
        }
        Self(handle)
    }
}

#[cfg(windows)]
impl Drop for ProAudio {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = unsafe {
                windows::Win32::System::Threading::AvRevertMmThreadCharacteristics(handle)
            };
        }
    }
}

#[cfg(not(windows))]
pub(super) struct ProAudio;

#[cfg(not(windows))]
impl ProAudio {
    pub(super) fn enter() -> Self {
        Self
    }
}
