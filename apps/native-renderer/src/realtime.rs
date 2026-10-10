//! Platform scheduling for threads that feed or render live audio.

#[cfg(all(windows, feature = "cpal-output"))]
pub(super) struct ProAudio(
    Option<windows::Win32::Foundation::HANDLE>,
);

#[cfg(all(windows, feature = "cpal-output"))]
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

#[cfg(all(windows, feature = "cpal-output"))]
impl Drop for ProAudio {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = unsafe {
                windows::Win32::System::Threading::AvRevertMmThreadCharacteristics(handle)
            };
        }
    }
}

#[cfg(not(all(windows, feature = "cpal-output")))]
pub(super) struct ProAudio;

#[cfg(not(all(windows, feature = "cpal-output")))]
impl ProAudio {
    pub(super) fn enter() -> Self {
        promote_current_thread();
        Self
    }
}

/// Explicit QoS on every iOS HRTF worker, not just the Swift decoder queue.
/// Best effort; never opt into hard realtime scheduling or run DSP on the callback.
pub(super) fn promote_current_thread() {
    #[cfg(target_os = "ios")]
    {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
        }
        const QOS_CLASS_USER_INITIATED: u32 = 0x19;
        let _ = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED, 0) };
    }
}

pub(super) fn render_buffer_scale(synchronized: bool, dense: bool) -> usize {
    buffer_scale_for_platform(cfg!(target_os = "ios"), synchronized, dense)
}

fn buffer_scale_for_platform(ios: bool, synchronized: bool, dense: bool) -> usize {
    if synchronized { 1 } else if ios { 4 } else if dense { 2 } else { 1 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ios_reserve_handles_other_app_bursts_without_changing_remote_sync() {
        use super::buffer_scale_for_platform as scale;
        assert_eq!(scale(true, false, false), 4);
        assert_eq!(scale(true, false, true), 4);
        assert_eq!(scale(true, true, true), 1);
        assert_eq!(scale(false, false, false), 1);
        assert_eq!(scale(false, false, true), 2);
    }
}
