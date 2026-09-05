//! Background-only spectral attenuation for speaker focus.
pub const BACKGROUND_GAIN: f32 = 0.063095734; // -24 dB

#[derive(Default)]
pub struct BackgroundFilter {
    low: f32,
}

impl BackgroundFilter {
    pub fn process(&mut self, input: f32) -> f32 {
        // Native HRTF playback is 48 kHz. Keep some high-frequency ambience.
        let alpha = 1.0 - (-std::f32::consts::TAU * 1500.0 / 48000.0).exp();
        self.low += alpha * (input - self.low);
        0.25 * input + 0.75 * self.low
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_preserves_dc_and_reduces_high_frequency_energy() {
        let mut filter = BackgroundFilter::default();
        for _ in 0..4096 { filter.process(1.0); }
        assert!((filter.process(1.0) - 1.0).abs() < 1e-6);
        let mut energy = 0.0;
        for i in 0..8192 {
            let y = filter.process(if i % 2 == 0 { 1.0 } else { -1.0 });
            if i >= 4096 { energy += y * y; }
        }
        assert!(energy / 4096.0 < 0.12);
    }
}
