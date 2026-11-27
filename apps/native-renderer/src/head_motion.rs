//! Bounded yaw anticipation for live tracking. Raw pose is always authoritative;
//! only its presentation-time lead is estimated, with no recentering or drift learning.
use std::time::{Duration, Instant};

pub(crate) struct Predictor {
    origin: Instant,
    last: Option<(f64, f64)>,
    velocity: f64,
    raw: Option<[f32; 4]>,
    pub lead_degrees: f32,
}
impl Default for Predictor {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            last: None,
            velocity: 0.0,
            raw: None,
            lead_degrees: 0.0,
        }
    }
}
impl Predictor {
    pub fn reset(&mut self) {
        self.last = None;
        self.velocity = 0.0;
        self.raw = None;
        self.lead_degrees = 0.0;
    }
    pub fn raw(&self) -> Option<[f32; 4]> {
        self.raw
    }
    pub fn predict(
        &mut self,
        q: [f32; 4],
        sampled: Instant,
        horizon: f64,
        enabled: bool,
    ) -> Option<[f32; 4]> {
        let Some(age) = Instant::now().checked_duration_since(sampled) else {
            return None;
        };
        if age > Duration::from_millis(250) {
            return None;
        }
        // Samples can predate renderer creation during tracking warmup. Keep
        // signed time rather than collapsing all such samples onto zero.
        let time = if sampled >= self.origin {
            sampled.duration_since(self.origin).as_secs_f64()
        } else {
            -self.origin.duration_since(sampled).as_secs_f64()
        };
        Some(self.at(q, time, horizon, enabled))
    }
    pub fn at(&mut self, q: [f32; 4], time: f64, horizon: f64, enabled: bool) -> [f32; 4] {
        let Some(q) = crate::spatial::normalize_quaternion(q) else {
            return self.raw.unwrap_or([0.0, 0.0, 0.0, 1.0]);
        };
        if !time.is_finite() || !horizon.is_finite() {
            return q;
        }
        let [x, y, z, w] = q.map(f64::from);
        let yaw = (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z));
        let mut rate = 0.0;
        if let Some((previous_time, previous_yaw)) = self.last {
            let dt = time - previous_time;
            if dt <= 0.0 {
                return self.raw.unwrap_or(q);
            }
            if (0.004..=0.25).contains(&dt) {
                let delta = (yaw - previous_yaw + std::f64::consts::PI)
                    .rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                rate = (delta / dt).clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
                if rate * self.velocity < 0.0 {
                    self.velocity = 0.0;
                }
                let alpha = 1.0 - (-dt / 0.020).exp();
                self.velocity += (rate - self.velocity) * alpha;
            } else {
                self.velocity = 0.0;
            }
        }
        self.last = Some((time, yaw));
        self.raw = Some(q);
        self.lead_degrees = 0.0;
        if !enabled {
            self.velocity = 0.0;
            return q;
        }
        // No head motion is ever accumulated into a new origin. On a stop,
        // decay the lead promptly rather than holding the last moving pose.
        if rate.abs() < 1e-6 {
            self.velocity *= 0.2;
        }
        let lead = (self.velocity * horizon.clamp(0.0, 0.080))
            .clamp(-6f64.to_radians(), 6f64.to_radians());
        if lead.abs() < 1e-7 {
            return q;
        }
        self.lead_degrees = lead.to_degrees() as f32;
        let (s, c) = (lead * 0.5).sin_cos();
        crate::spatial::normalize_quaternion([
            (c * x - s * y) as f32,
            (c * y + s * x) as f32,
            (c * z + s * w) as f32,
            (c * w - s * z) as f32,
        ])
        .unwrap_or(q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn q(deg: f64) -> [f32; 4] {
        let a = deg.to_radians() / 2.0;
        [0.0, 0.0, a.sin() as f32, a.cos() as f32]
    }
    fn yaw(q: [f32; 4]) -> f64 {
        2.0 * f64::from(q[2]).atan2(f64::from(q[3])).to_degrees()
    }
    #[test]
    fn micro_turns_gain_presentation_lead_and_settle_without_recentering() {
        let mut p = Predictor::default();
        p.at(q(0.0), 0.0, 0.08, true);
        let predicted = p.at(q(0.1), 1.0 / 30.0, 0.08, true);
        assert!(yaw(predicted) > 0.15);
        for i in 2..12 {
            p.at(q(0.1), i as f64 / 30.0, 0.08, true);
        }
        assert!((yaw(p.at(q(0.1), 0.5, 0.08, true)) - 0.1).abs() < 0.001);
    }
    #[test]
    fn lead_is_bounded_across_wrap_reversal_and_transport_reset() {
        let mut p = Predictor::default();
        p.at(q(179.0), 0.0, 0.08, true);
        let v = p.at(q(-179.0), 0.033, 2.0, true);
        assert!(p.lead_degrees > 0.0 && p.lead_degrees <= 6.0);
        assert!(v.iter().all(|x| x.is_finite()));
        p.at(q(175.0), 0.066, 0.08, true);
        assert!(p.lead_degrees >= -6.0);
        p.reset();
        assert!((yaw(p.at(q(30.0), 1.0, 0.08, true)) - 30.0).abs() < 1e-4);
    }
    #[test]
    fn disabled_prediction_and_stale_packets_preserve_authoritative_pose() {
        let mut p = Predictor::default();
        p.at(q(0.0), 0.0, 0.08, false);
        assert!((yaw(p.at(q(0.2), 0.033, 0.08, false)) - 0.2).abs() < 1e-4);
        assert!(p
            .predict(q(90.0), Instant::now() - Duration::from_secs(1), 0.08, true)
            .is_none());
    }
}
