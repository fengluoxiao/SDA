//! Allocation-free output diagnostics. A large sample step is evidence to
//! inspect, not proof of a click: legitimate high-frequency audio can have it.

use std::sync::atomic::{AtomicU64, Ordering};

const LARGE_STEP: f32 = 0.7;
const SCALE: f32 = 1_000_000.0;

#[derive(Default)]
pub(super) struct OutputTelemetry {
    peak: AtomicU64,
    max_step: AtomicU64,
    max_step_sample: AtomicU64,
    large_steps: AtomicU64,
    last_large_step_sample: AtomicU64,
}

impl OutputTelemetry {
    pub(super) fn peak(&self) -> f32 {
        self.peak.load(Ordering::Relaxed) as f32 / SCALE
    }

    pub(super) fn max_step(&self) -> f32 {
        self.max_step.load(Ordering::Relaxed) as f32 / SCALE
    }

    pub(super) fn max_step_sample(&self) -> u64 {
        self.max_step_sample.load(Ordering::Relaxed)
    }

    pub(super) fn large_steps(&self) -> u64 {
        self.large_steps.load(Ordering::Relaxed)
    }

    pub(super) fn last_large_step_sample(&self) -> u64 {
        self.last_large_step_sample.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
pub(super) struct OutputMonitor {
    previous: [f32; 2],
}

impl OutputMonitor {
    pub(super) fn reset(&mut self) {
        self.previous = [0.0; 2];
    }

    pub(super) fn observe(
        &mut self,
        frames: impl Iterator<Item = [f32; 2]>,
        sample_pos: u64,
        telemetry: &OutputTelemetry,
    ) {
        let mut peak = 0.0_f32;
        let mut max_step = 0.0_f32;
        let mut max_step_sample = sample_pos;
        let mut large_steps = 0;
        let mut last_large_step_sample = 0;
        for (offset, frame) in frames.enumerate() {
            for (channel, sample) in frame.into_iter().enumerate() {
                let step = (sample - self.previous[channel]).abs();
                peak = peak.max(sample.abs());
                if step > max_step {
                    max_step = step;
                    max_step_sample = sample_pos + offset as u64;
                }
                if step > LARGE_STEP {
                    large_steps += 1;
                    last_large_step_sample = sample_pos + offset as u64;
                }
                self.previous[channel] = sample;
            }
        }
        telemetry.peak.fetch_max((peak * SCALE) as u64, Ordering::Relaxed);
        let scaled_step = (max_step * SCALE) as u64;
        if scaled_step > telemetry.max_step.fetch_max(scaled_step, Ordering::Relaxed) {
            telemetry.max_step_sample.store(max_step_sample, Ordering::Relaxed);
        }
        if large_steps > 0 {
            telemetry.large_steps.fetch_add(large_steps, Ordering::Relaxed);
            telemetry.last_large_step_sample.store(last_large_step_sample, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_channels_do_not_count_as_temporal_steps() {
        let mut monitor = OutputMonitor::default();
        let telemetry = OutputTelemetry::default();
        monitor.observe([[0.5, -0.5]; 4].into_iter(), 0, &telemetry);
        assert_eq!(telemetry.large_steps(), 0);
        assert_eq!(telemetry.max_step(), 0.5);
        assert_eq!(telemetry.peak(), 0.5);
    }

    #[test]
    fn detects_same_channel_steps_across_callback_boundaries() {
        let mut monitor = OutputMonitor::default();
        let telemetry = OutputTelemetry::default();
        monitor.observe([[0.5, 0.0]; 4].into_iter(), 100, &telemetry);
        monitor.observe([[-0.5, 0.0], [-0.5, 0.0]].into_iter(), 104, &telemetry);
        assert_eq!(telemetry.large_steps(), 1);
        assert_eq!(telemetry.last_large_step_sample(), 104);
        assert_eq!(telemetry.max_step(), 1.0);
        assert_eq!(telemetry.max_step_sample(), 104);
        monitor.reset();
        monitor.observe([[0.5, 0.0]].into_iter(), 0, &telemetry);
        assert_eq!(telemetry.large_steps(), 1);
    }
}
