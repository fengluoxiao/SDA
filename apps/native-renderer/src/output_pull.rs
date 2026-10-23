//! AAudio's callback only consumes completed PCM. No allocation, locks, logging,
//! decoder, HRTF preparation or DSP dispatch belongs on this path.
use crate::{RuntimeTelemetry, stereo_fifo::StereoFifo};
use std::{sync::atomic::Ordering, time::Instant};
pub(crate) fn pull(fifo: &StereoFifo, telemetry: &RuntimeTelemetry, output: &mut [f32]) -> usize {
    let started = Instant::now();
    let requested = output.len() / 2;
    fifo.apply_flush_from_consumer();
    let enabled = telemetry.callback_output_enabled.load(Ordering::Acquire)
        && !telemetry.output_device_recovering.load(Ordering::Acquire);
    let popped = if enabled {
        fifo.pop_into_f32(output, 2)
    } else {
        output.fill(0.);
        0
    };
    telemetry.callback_count.fetch_add(1, Ordering::Relaxed);
    if enabled {
        telemetry
            .callback_consumed_sample_pos
            .fetch_add(popped as u64, Ordering::Release);
        telemetry
            .callback_fifo_underrun_frames
            .fetch_add((requested - popped) as u64, Ordering::Relaxed);
    }
    RuntimeTelemetry::record_max(
        &telemetry.callback_max_micros,
        started.elapsed().as_micros() as u64,
    );
    popped
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn burst_sizes_and_pause_preserve_pcm_and_presentation_clock() {
        for burst in [96, 192, 240, 256, 384, 1024] {
            let fifo = StereoFifo::new(32768);
            let telemetry = RuntimeTelemetry::default();
            let pcm: Vec<_> = (0..24000)
                .map(|i| i as f32 / 24000.)
                .flat_map(|x| [x, -x])
                .collect();
            assert_eq!(fifo.push(&pcm), 24000);
            let mut out = vec![0.; burst * 2];
            pull(&fifo, &telemetry, &mut out);
            assert!(out.iter().all(|x| *x == 0.));
            assert_eq!(fifo.available_read(), 24000);
            telemetry
                .callback_output_enabled
                .store(true, Ordering::Release);
            let mut result = vec![];
            while fifo.available_read() > 0 {
                let n = pull(&fifo, &telemetry, &mut out);
                result.extend_from_slice(&out[..n * 2]);
            }
            assert_eq!(result, pcm);
            assert_eq!(
                telemetry
                    .callback_consumed_sample_pos
                    .load(Ordering::Acquire),
                24000
            );
        }
    }
    #[test]
    fn output_reconnection_keeps_queued_pcm_and_monotonic_position() {
        let fifo = StereoFifo::new(32768);
        let telemetry = RuntimeTelemetry::default();
        telemetry.callback_output_enabled.store(true, Ordering::Release);
        let pcm: Vec<_> = (0..960).flat_map(|i| [i as f32 / 960., -(i as f32) / 960.]).collect();
        fifo.push(&pcm);
        let mut first = [0.; 384];
        assert_eq!(pull(&fifo, &telemetry, &mut first), 192);
        assert_eq!(first, pcm[..384]);
        telemetry.output_device_recovering.store(true, Ordering::Release);
        let mut changed_burst = [1.; 480];
        for _ in 0..100 {
            assert_eq!(pull(&fifo, &telemetry, &mut changed_burst), 0);
            assert!(changed_burst.iter().all(|v| *v == 0.));
        }
        assert_eq!(fifo.available_read(), 768);
        assert_eq!(telemetry.callback_consumed_sample_pos.load(Ordering::Acquire), 192);
        assert_eq!(telemetry.callback_missing_frames(), 0);
        telemetry.output_device_recovering.store(false, Ordering::Release);
        assert_eq!(pull(&fifo, &telemetry, &mut changed_burst), 240);
        assert_eq!(changed_burst, pcm[384..864]);
        assert_eq!(telemetry.callback_consumed_sample_pos.load(Ordering::Acquire), 432);
        assert_eq!(fifo.available_read(), 528);
    }
    #[test]
    fn a_producer_stall_uses_reserve_before_reporting_real_missing_frames() {
        let fifo = StereoFifo::new(32768);
        let t = RuntimeTelemetry::default();
        t.callback_output_enabled.store(true, Ordering::Release);
        fifo.push(&vec![0.01; 16384]);
        let mut out = [0.; 384];
        for _ in 0..41 {
            assert_eq!(pull(&fifo, &t, &mut out), 192);
        }
        assert_eq!(t.callback_missing_frames(), 0);
        let n = pull(&fifo, &t, &mut out);
        assert_eq!(n, 192);
        let n = pull(&fifo, &t, &mut out);
        assert_eq!(n, 128);
        assert_eq!(t.callback_missing_frames(), 64);
        fifo.push(&[0.02; 384]);
        assert_eq!(pull(&fifo, &t, &mut out), 192);
        assert!(out.iter().all(|x| *x == 0.02));
    }
}
