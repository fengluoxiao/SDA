//! Converts the fixed 48 kHz renderer clock to the output device clock.
use crate::{callback_output::CallbackOutput, stereo_fifo::StereoFifo};

const TAPS: usize = 96;
const PHASES: usize = 1024;

pub(super) struct DeviceOutput {
    source: CallbackOutput,
    rate: u32,
    phase: u64,
    history: [[f32; 2]; TAPS],
    cursor: usize,
    filters: Vec<[f32; TAPS]>,
    pub(super) requested_source: usize,
}

impl DeviceOutput {
    pub(super) fn new(rate: u32, refill: usize) -> Self {
        let cutoff = (rate as f64 / 48_000.0).min(1.0) * 0.94;
        let filters = if rate == 48_000 { Vec::new() } else {
            (0..PHASES).map(|phase| {
                let mut taps = [0.0; TAPS];
                let delay = (TAPS / 2) as f64 + phase as f64 / PHASES as f64;
                let mut sum = 0.0;
                for (i, tap) in taps.iter_mut().enumerate() {
                    let x = (i as f64 - delay) * cutoff;
                    let sinc = if x.abs() < 1e-12 { 1.0 } else {
                        (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                    };
                    let window = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (TAPS - 1) as f64).cos()
                        + 0.08 * (4.0 * std::f64::consts::PI * i as f64 / (TAPS - 1) as f64).cos();
                    *tap = (sinc * cutoff * window) as f32;
                    sum += *tap;
                }
                for tap in &mut taps { *tap /= sum; }
                taps
            }).collect()
        };
        Self { source: CallbackOutput::new(48_000, refill), rate, phase: 0,
            history: [[0.0; 2]; TAPS], cursor: 0, filters, requested_source: 0 }
    }

    pub(super) fn reset(&mut self) {
        self.source.reset();
        self.phase = 0;
        self.history.fill([0.0; 2]);
        self.cursor = 0;
        self.requested_source = 0;
    }

    pub(super) fn fill(&mut self, fifo: &StereoFifo, enabled: bool, frames: usize,
        mut write: impl FnMut(usize, [f32; 2])) -> usize {
        self.requested_source = 0;
        if !enabled {
            self.reset();
            for i in 0..frames { write(i, [0.0; 2]); }
            return 0;
        }
        if self.rate == 48_000 {
            self.requested_source = frames;
            return self.source.fill(fifo, true, frames, write);
        }
        let mut consumed = 0;
        for i in 0..frames {
            self.phase += 48_000;
            let count = (self.phase / self.rate as u64) as usize;
            self.phase %= self.rate as u64;
            self.requested_source += count;
            if count > 0 {
                consumed += self.source.fill(fifo, true, count, |_, frame| {
                    self.cursor = (self.cursor + 1) % TAPS;
                    self.history[self.cursor] = frame;
                });
            }
            // Fractional delay is causal; no future FIFO samples are consumed.
            let phase = ((self.rate as u64 - self.phase) * PHASES as u64 / self.rate as u64)
                .min((PHASES - 1) as u64) as usize;
            let mut frame = [0.0; 2];
            for (j, weight) in self.filters[phase].iter().enumerate() {
                let sample = self.history[(self.cursor + TAPS - j) % TAPS];
                frame[0] += sample[0] * weight;
                frame[1] += sample[1] * weight;
            }
            write(i, frame);
        }
        consumed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render(rate: u32, hz: f32, chunk: usize) -> Vec<f32> {
        let fifo = StereoFifo::new(65_536);
        let samples: Vec<f32> = (0..48_000).flat_map(|i| {
            let v = (i as f32 * std::f32::consts::TAU * hz / 48_000.0).sin();
            [v, v]
        }).collect();
        fifo.push(&samples);
        let mut output = DeviceOutput::new(rate, 16);
        let mut result = Vec::new();
        while result.len() < rate as usize {
            let n = chunk.min(rate as usize - result.len());
            output.fill(&fifo, true, n, |_, f| result.push(f[0]));
        }
        result
    }

    #[test]
    fn chunk_boundaries_do_not_change_resampling() {
        assert_eq!(render(44_100, 1000.0, 137), render(44_100, 1000.0, 1024));
    }

    #[test]
    fn downsampling_rejects_above_nyquist_energy() {
        let rms = |data: Vec<f32>| {
            let tail = &data[1600..];
            (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt()
        };
        assert!(rms(render(16_000, 1000.0, 137)) > 0.69);
        assert!(rms(render(16_000, 12_000.0, 137)) < 0.005);
    }

    #[test]
    fn native_rate_is_exact_and_reset_removes_filter_tail() {
        let fifo = StereoFifo::new(1024);
        fifo.push(&[0.25, -0.75, -0.5, 0.125]);
        let mut output = DeviceOutput::new(48_000, 16);
        let mut frames = Vec::new();
        assert_eq!(output.fill(&fifo, true, 2, |_, f| frames.push(f)), 2);
        assert_eq!(frames, [[0.25, -0.75], [-0.5, 0.125]]);
        let mut output = DeviceOutput::new(44_100, 16);
        fifo.push(&vec![0.5; 512]);
        output.fill(&fifo, true, 128, |_, _| {});
        output.reset();
        fifo.clear_from_producer();
        fifo.apply_flush_from_consumer();
        fifo.push(&vec![0.0; 512]);
        output.fill(&fifo, true, 128, |_, f| assert_eq!(f, [0.0; 2]));
    }

    #[test]
    fn device_rates_preserve_codec_clock_and_tone() {
        for rate in [16_000, 22_050, 44_100, 48_000, 96_000] {
            let fifo = StereoFifo::new(65_536);
            let samples: Vec<f32> = (0..48_000).flat_map(|i| {
                let v = (i as f32 * std::f32::consts::TAU * 1000.0 / 48_000.0).sin() * 0.5;
                [v, -v]
            }).collect();
            assert_eq!(fifo.push(&samples), 48_000);
            let mut output = DeviceOutput::new(rate, 16);
            let mut rendered = Vec::new();
            let mut consumed = 0;
            while rendered.len() < rate as usize {
                let count = 137.min(rate as usize - rendered.len());
                consumed += output.fill(&fifo, true, count, |_, frame| rendered.push(frame));
            }
            assert_eq!(consumed, 48_000, "rate={rate}");
            let crossings = rendered[rate as usize / 10..].windows(2)
                .filter(|w| w[0][0] <= 0.0 && w[1][0] > 0.0).count();
            assert!((crossings as i32 - 900).abs() <= 2, "rate={rate} crossings={crossings}");
            assert!(rendered.iter().all(|f| (f[0] + f[1]).abs() < 1e-6));
            output.fill(&fifo, false, 256, |_, f| assert_eq!(f, [0.0; 2]));
            assert_eq!(output.requested_source, 0);
        }
    }
}
