use crate::protocol::Orientation;

const HEAD_TRACKING_PREFIX: [u8; 10] = [0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x00, 0x00, 0x10, 0x00];
const CALIBRATION_SAMPLES: usize = 10;

#[derive(Default)]
pub struct HeadOrientation {
    samples: Vec<[f64; 3]>,
    neutral: Option<[f64; 3]>,
}

impl HeadOrientation {
    pub fn reset(&mut self) {
        self.samples.clear();
        self.neutral = None;
    }

    pub fn calibrated(&self) -> bool {
        self.neutral.is_some()
    }

    pub fn process_packet(&mut self, packet: &[u8]) -> Option<Orientation> {
        let packet = head_tracking_frame(packet)?;

        let values = [
            read_i16(packet, 43)? as f64,
            read_i16(packet, 45)? as f64,
            read_i16(packet, 47)? as f64,
        ];
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

        let turned = tracker
            .process_packet(&packet(19_000, 17_000, -15_000))
            .unwrap();
        let expected = (std::f64::consts::FRAC_PI_2 * 0.5).sin();
        assert!((turned.z + expected).abs() < 1e-12);
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
