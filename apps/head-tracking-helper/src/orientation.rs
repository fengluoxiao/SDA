use crate::protocol::Orientation;

const HEAD_TRACKING_PREFIX: [u8; 10] = [0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x00, 0x00, 0x10, 0x00];
const CALIBRATION_SAMPLES: usize = 10;
const ORIENTATION_MEDIAN_SAMPLES: usize = 3;
const ORIENTATION_DEAD_ZONE_RADIANS: f64 = 0.6 * std::f64::consts::PI / 180.0;

#[derive(Default)]
pub struct HeadOrientation {
    samples: Vec<[f64; 3]>,
    neutral: Option<[f64; 3]>,
    previous_raw: Option<[i16; 3]>,
    unwrapped: [i64; 3],
    orientation_samples: Vec<[f64; 2]>,
    last_orientation: Option<[f64; 2]>,
}

impl HeadOrientation {
    pub fn reset(&mut self) {
        self.samples.clear();
        self.neutral = None;
        self.previous_raw = None;
        self.unwrapped = [0; 3];
        self.orientation_samples.clear();
        self.last_orientation = None;
    }

    pub fn calibrated(&self) -> bool {
        self.neutral.is_some()
    }

    pub fn process_packet(&mut self, packet: &[u8]) -> Option<Orientation> {
        let packet = head_tracking_frame(packet)?;

        let raw = [
            read_i16(packet, 43)?,
            read_i16(packet, 45)?,
            read_i16(packet, 47)?,
        ];
        let values = self.unwrap_values(raw);
        let neutral = match self.neutral {
            Some(neutral) => neutral,
            None => {
                self.samples.push(values);
                if self.samples.len() == CALIBRATION_SAMPLES {
                    let mut average = [0.0; 3];
                    for sample in &self.samples {
                        for index in 0..3 {
                            average[index] += sample[index] / CALIBRATION_SAMPLES as f64;
                        }
                    }
                    self.neutral = Some(average);
                    // Seed the median filter with the newly calibrated forward
                    // direction, avoiding an unfiltered first motion frame.
                    self.orientation_samples = vec![[0.0, 0.0]; ORIENTATION_MEDIAN_SAMPLES - 1];
                    self.last_orientation = Some([0.0, 0.0]);
                }
                return None;
            }
        };

        // LibrePods' measured AirPods sensor mapping. SDA currently renders yaw
        // only, but retaining pitch makes the helper protocol future-proof.
        let pitch = ((values[1] - neutral[1]) + (values[2] - neutral[2])) * 0.5 / 32_000.0
            * std::f64::consts::PI;
        // AirPods' sensor difference increases in the opposite direction from
        // SDA/ADM positive yaw. Negate it here so a physical left turn makes a
        // fixed frontal source move toward the listener's right ear.
        let yaw = ((values[2] - neutral[2]) - (values[1] - neutral[1])) * 0.5 / 32_000.0
            * std::f64::consts::PI;
        let [pitch, yaw] = self.filter_orientation([pitch, yaw]);

        // Head-local -> ADM world: yaw around +Z, then pitch around +X.
        let (sin_pitch, cos_pitch) = (pitch * 0.5).sin_cos();
        let (sin_yaw, cos_yaw) = (yaw * 0.5).sin_cos();
        Some(Orientation {
            x: cos_yaw * sin_pitch,
            y: sin_yaw * sin_pitch,
            z: sin_yaw * cos_pitch,
            w: cos_yaw * cos_pitch,
        })
    }

    fn unwrap_values(&mut self, raw: [i16; 3]) -> [f64; 3] {
        if let Some(previous) = self.previous_raw {
            for index in 0..3 {
                // AirPods exposes cyclic signed 16-bit angles. A normal move
                // across +32767/-32768 must be a small delta, not a 180-degree
                // pose jump.
                self.unwrapped[index] += raw[index].wrapping_sub(previous[index]) as i64;
            }
        } else {
            self.unwrapped = raw.map(i64::from);
        }
        self.previous_raw = Some(raw);
        self.unwrapped.map(|value| value as f64)
    }

    fn filter_orientation(&mut self, orientation: [f64; 2]) -> [f64; 2] {
        self.orientation_samples.push(orientation);
        if self.orientation_samples.len() > ORIENTATION_MEDIAN_SAMPLES {
            self.orientation_samples.remove(0);
        }

        let mut filtered = [0.0; 2];
        for axis in 0..2 {
            let mut values: Vec<f64> = self
                .orientation_samples
                .iter()
                .map(|sample| sample[axis])
                .collect();
            values.sort_by(f64::total_cmp);
            filtered[axis] = values[values.len() / 2];
        }

        if let Some(last) = self.last_orientation {
            for axis in 0..2 {
                if (filtered[axis] - last[axis]).abs() < ORIENTATION_DEAD_ZONE_RADIANS {
                    filtered[axis] = last[axis];
                }
            }
        }
        self.last_orientation = Some(filtered);
        filtered
    }
}

pub(crate) fn is_head_tracking_packet(packet: &[u8]) -> bool {
    head_tracking_frame(packet).is_some()
}

fn head_tracking_frame(packet: &[u8]) -> Option<&[u8]> {
    // A single overlapped driver read can contain a control notification
    // followed by a motion frame. Search the complete L2CAP payload instead of
    // requiring motion to begin at byte zero.
    packet
        .windows(HEAD_TRACKING_PREFIX.len())
        .enumerate()
        .find_map(|(offset, candidate)| {
            if candidate != HEAD_TRACKING_PREFIX {
                return None;
            }
            let frame = packet.get(offset..)?;
            (frame.len() > 60 && matches!(frame[10], 0x44 | 0x45) && frame[11] == 0)
                .then_some(frame)
        })
}

fn read_i16(packet: &[u8], offset: usize) -> Option<i16> {
    let bytes = packet.get(offset..offset + 2)?;
    Some(i16::from_le_bytes([bytes[0], bytes[1]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(o1: i16, o2: i16, o3: i16) -> Vec<u8> {
        let mut packet = vec![0; 64];
        packet[..HEAD_TRACKING_PREFIX.len()].copy_from_slice(&HEAD_TRACKING_PREFIX);
        packet[10] = 0x44;
        for (offset, value) in [(43, o1), (45, o2), (47, o3)] {
            packet[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        packet
    }

    #[test]
    fn calibrates_to_identity_and_maps_yaw() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            assert!(
                tracker
                    .process_packet(&packet(19_000, 1_000, 1_000))
                    .is_none()
            );
        }
        assert!(tracker.calibrated());

        let identity = tracker
            .process_packet(&packet(19_000, 1_000, 1_000))
            .unwrap();
        assert!(identity.x.abs() < 1e-12 && identity.y.abs() < 1e-12);
        assert!(identity.z.abs() < 1e-12 && (identity.w - 1.0).abs() < 1e-12);

        tracker.process_packet(&packet(19_000, 17_000, -15_000));
        let turned = tracker.process_packet(&packet(19_000, 17_000, -15_000)).unwrap();
        let expected = (std::f64::consts::FRAC_PI_2 * 0.5).sin();
        assert!((turned.z + expected).abs() < 1e-12);
    }

    #[test]
    fn unwraps_signed_sensor_boundary_without_a_pose_jump() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&packet(19_000, 32_760, 1_000));
        }
        tracker.process_packet(&packet(19_000, -32_760, 1_000));
        let crossed = tracker.process_packet(&packet(19_000, -32_760, 1_000)).unwrap();
        assert!(crossed.z.abs() < 0.001, "boundary crossing became a large yaw: {crossed:?}");
        assert!(crossed.w > 0.999);
    }

    #[test]
    fn suppresses_stationary_jitter_and_an_isolated_spike() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&packet(19_000, 1_000, 1_000));
        }
        for _ in 0..2 {
            let jitter = tracker.process_packet(&packet(19_000, 1_080, 920)).unwrap();
            assert!(jitter.z.abs() < 1e-12);
        }
        let spike = tracker.process_packet(&packet(19_000, 17_000, -15_000)).unwrap();
        assert!(spike.z.abs() < 1e-12);
        let recovered = tracker.process_packet(&packet(19_000, 1_000, 1_000)).unwrap();
        assert!(recovered.z.abs() < 1e-12);
    }

    #[test]
    fn rejects_non_tracking_packets() {
        let mut tracker = HeadOrientation::default();
        assert!(tracker.process_packet(&[0; 64]).is_none());
    }

    #[test]
    fn accepts_motion_frame_after_a_coalesced_control_packet() {
        let mut tracker = HeadOrientation::default();
        let mut coalesced = vec![0x04, 0x00, 0x04, 0x00, 0x04, 0xaa, 0xbb];
        coalesced.extend(packet(19_000, 1_000, 1_000));
        assert!(is_head_tracking_packet(&coalesced));
        for _ in 0..CALIBRATION_SAMPLES {
            assert!(tracker.process_packet(&coalesced).is_none());
        }
        assert!(tracker.calibrated());
        assert!(tracker.process_packet(&coalesced).is_some());
    }
}
