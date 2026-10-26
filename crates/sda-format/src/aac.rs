use std::{ffi::c_void, ptr::NonNull};
unsafe extern "C" {
    fn sda_aac_open(asc: *const u8, len: usize, rate: *mut u32, channels: *mut u32) -> *mut c_void;
    fn sda_aac_decode(
        handle: *mut c_void,
        packet: *const u8,
        len: usize,
        pcm: *mut *const f32,
        frames: *mut u32,
        channels: *mut u32,
        rate: *mut u32,
        positions: *mut u8,
    ) -> i32;
    fn sda_aac_close(handle: *mut c_void);
}
pub struct AacDecoder {
    handle: NonNull<c_void>,
    config: Vec<u8>,
    pub sample_rate: u32,
    pub channels: usize,
}
// Each decoder is owned exclusively by a non-realtime file feeder.
unsafe impl Send for AacDecoder {}
pub struct AacFrame {
    pub channels: Vec<Vec<f32>>,
    pub sample_rate: u32,
    pub positions: Vec<u8>,
}
impl AacDecoder {
    pub fn new(config: &[u8]) -> Result<Self, String> {
        let (mut rate, mut channels) = (0, 0);
        let handle =
            unsafe { sda_aac_open(config.as_ptr(), config.len(), &mut rate, &mut channels) };
        Ok(Self {
            handle: NonNull::new(handle).ok_or("Unsupported SDAR AAC configuration")?,
            sample_rate: rate,
            channels: channels as usize,
            config: config.to_vec(),
        })
    }
    pub fn restart(&mut self) -> Result<(), String> {
        let next = Self::new(&self.config)?;
        *self = next;
        Ok(())
    }
    pub fn decode(&mut self, packet: &[u8]) -> Result<AacFrame, String> {
        if packet.is_empty() || packet.len() > 1024 * 1024 {
            return Err("Invalid SDAR AAC packet".into());
        }
        let mut padded = packet.to_vec();
        padded.resize(packet.len() + 16, 0);
        let (mut pcm, mut frames, mut channels, mut rate) = (std::ptr::null(), 0, 0, 0);
        let mut positions = [0; 8];
        let code = unsafe {
            sda_aac_decode(
                self.handle.as_ptr(),
                padded.as_ptr(),
                packet.len(),
                &mut pcm,
                &mut frames,
                &mut channels,
                &mut rate,
                positions.as_mut_ptr(),
            )
        };
        if code != 0 {
            return Err(format!("SDAR AAC decode failed: {code}"));
        }
        if frames == 0 {
            return Ok(AacFrame {
                channels: vec![vec![]; self.channels],
                sample_rate: self.sample_rate,
                positions: vec![],
            });
        }
        if rate != self.sample_rate || channels as usize != self.channels {
            return Err("SDAR AAC configuration changed".into());
        }
        let samples = unsafe { std::slice::from_raw_parts(pcm, frames as usize * self.channels) };
        if samples.iter().any(|v| !v.is_finite() || v.abs() > 128.) {
            return Err("Invalid decoded SDAR AAC sample".into());
        }
        Ok(AacFrame {
            channels: (0..self.channels)
                .map(|ch| {
                    samples
                        .iter()
                        .skip(ch)
                        .step_by(self.channels)
                        .copied()
                        .collect()
                })
                .collect(),
            sample_rate: rate,
            positions: positions[..self.channels].to_vec(),
        })
    }
}
impl Drop for AacDecoder {
    fn drop(&mut self) {
        unsafe {
            sda_aac_close(self.handle.as_ptr());
        }
    }
}
