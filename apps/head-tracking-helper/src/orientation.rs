use crate::protocol::Orientation;

const HEAD_TRACKING_PREFIX: [u8; 10] = [0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x00, 0x00, 0x10, 0x00];
const CALIBRATION_SAMPLES: usize = 10;
const ORIENTATION_MEDIAN_SAMPLES: usize = 3;
const ORIENTATION_DEAD_ZONE_RADIANS: f64 = 0.6 * std::f64::consts::PI / 180.0;
const STATIONARY_WINDOW_SAMPLES: usize = 12;
const STATIONARY_WINDOW_RADIANS: f64 = 0.25 * std::f64::consts::PI / 180.0;
const STATIONARY_RELEASE_RADIANS: f64 = 1.5 * std::f64::consts::PI / 180.0;
const DISCONTINUITY_RADIANS: f64 = 2.5 * std::f64::consts::PI / 180.0;
const DISCONTINUITY_MAX_CANDIDATE_STEP_RADIANS: f64 = 20.0 * std::f64::consts::PI / 180.0;
const DISCONTINUITY_MIN_PROGRESS_RADIANS: f64 = 0.15 * std::f64::consts::PI / 180.0;
const DISCONTINUITY_MOTION_CONFIRM_SAMPLES: usize = 2;
const DISCONTINUITY_STEADY_CONFIRM_SAMPLES: usize = 12;
const DISCONTINUITY_MAX_OUTPUT_STEP_RADIANS: f64 = 6.0 * std::f64::consts::PI / 180.0;
const STATIONARY_RELEASE_CONFIRM_SAMPLES: usize = 2;
/// One full turn is 64000 counts. A real head cannot move more than ±90° in
/// one frame; a misparsed AACP control packet can claim ±180°. Anything above
/// the gate is dropped instead of re-anchoring the attitude accumulator.
const MAX_FRAME_DELTA_COUNTS: i64 = 16_000;
/// Sustained rejection means the stream re-referenced or moved during a gap:
/// realign the unwrap reference without accumulating, keeping the attitude.
const UNWRAP_RESYNC_REJECTS: usize = 10;
/// Rate of the stationary drift-bias estimate (counts/frame EMA while the
/// stationary lock holds; ~10 frame time constant at 30 Hz). The AirPods
/// report gyro-integrated relative angles, so a fixed PC must estimate and
/// cancel the per-frame bias or the attitude random-walks away while sitting
/// still. The rate must outrun the stationary-release threshold: uncancelled
/// drift reaches the 1.5° release in ~27 frames, so 0.1 converges to <0.3°.
const BIAS_LEARN_RATE: f64 = 0.1;

#[derive(Clone, Copy)]
struct PendingDiscontinuity {
    last: [f64; 2],
    direction: [f64; 2],
    motion_samples: usize,
    steady_samples: usize,
}

#[derive(Default)]
pub struct HeadOrientation {
    samples: Vec<[f64; 3]>,
    calibration_subtypes: Vec<u8>,
    subtype_tally: [u32; 2],
    stream_subtype: Option<u8>,
    neutral: Option<[f64; 3]>,
    previous_raw: Option<[i16; 3]>,
    unwrapped: [f64; 3],
    /// Counts/frame drift estimate per pose axis, learned while locked.
    bias: [f64; 2],
    bias_learning: bool,
    /// Total bias subtracted since the last reset, so a suspected-motion
    /// reset can give the untracked amount back to the accumulator.
    bias_applied_total: [f64; 2],
    orientation_samples: Vec<[f64; 2]>,
    last_orientation: Option<[f64; 2]>,
    stationary_samples: Vec<[f64; 2]>,
    locked_orientation: Option<[f64; 2]>,
    pending_discontinuity: Option<PendingDiscontinuity>,
    pending_stationary_release: Option<PendingDiscontinuity>,
    slewing_discontinuity: bool,
    continuity_origin: [f64; 2],
    unwrap_rejects: usize,
    last_subtype: u8,
}

impl HeadOrientation {
    pub fn begin_transport_session(&mut self) {
        self.continuity_origin = self
            .last_orientation
            .or(self.locked_orientation)
            .unwrap_or(self.continuity_origin);
        self.samples.clear();
        self.calibration_subtypes.clear();
        self.subtype_tally = [0; 2];
        self.stream_subtype = None;
        self.neutral = None;
        self.previous_raw = None;
        self.unwrapped = [0.0; 3];
        self.bias = [0.0; 2];
        self.bias_learning = false;
        self.bias_applied_total = [0.0; 2];
        self.unwrap_rejects = 0;
        self.orientation_samples.clear();
        self.last_orientation = None;
        self.stationary_samples.clear();
        self.locked_orientation = None;
        self.pending_discontinuity = None;
        self.pending_stationary_release = None;
        self.slewing_discontinuity = false;
    }

    pub fn calibrated(&self) -> bool {
        self.neutral.is_some()
    }

    /// One-line diagnostic state for env-gated field logging.
    pub fn debug_state(&self) -> String {
        let (pitch, yaw) = match self.last_orientation {
            Some(values) => (values[0], values[1]),
            None => (0.0, 0.0),
        };
        let to_degrees = |radians: f64| radians * 180.0 / std::f64::consts::PI;
        format!(
            "sub=0x{:02X} raw={:?} unwrapped={:?} yaw={:+.2}deg pitch={:+.2}deg rejects={} lock={} slew={} cal={} bias={:?}",
            self.last_subtype,
            self.previous_raw,
            self.unwrapped,
            to_degrees(yaw),
            to_degrees(pitch),
            self.unwrap_rejects,
            self.locked_orientation.is_some() as u8,
            self.slewing_discontinuity as u8,
            self.neutral.is_some() as u8,
            self.bias,
        )
    }

    pub fn process_packet(&mut self, packet: &[u8]) -> Option<Orientation> {
        let packet = head_tracking_frame(packet)?;
        self.last_subtype = packet[10];

        let raw = [
            read_i16(packet, 43)?,
            read_i16(packet, 45)?,
            read_i16(packet, 47)?,
        ];
        let values = match self.unwrap_values(raw) {
            Some(values) => values,
            None => return None,
        };
        let neutral = match self.neutral {
            Some(neutral) => neutral,
            None => {
                match self.last_subtype {
                    0x44 => self.subtype_tally[0] += 1,
                    0x45 => self.subtype_tally[1] += 1,
                    _ => {}
                }
                self.calibration_subtypes.push(self.last_subtype);
                self.samples.push(values);
                if self.samples.len() == CALIBRATION_SAMPLES {
                    // Both buds notify motion with their own sub-type and
                    // independent reference frames; lock onto whichever
                    // dominated calibration, average only that stream's
                    // samples, and ignore the other stream afterwards.
                    let dominant = if self.subtype_tally[1] > self.subtype_tally[0] { 0x45 } else { 0x44 };
                    self.stream_subtype = Some(dominant);
                    let dominant_samples: Vec<[f64; 3]> = self
                        .samples
                        .iter()
                        .zip(self.calibration_subtypes.iter())
                        .filter(|(_, subtype)| **subtype == dominant)
                        .map(|(values, _)| *values)
                        .collect();
                    let source: &[[f64; 3]] = if dominant_samples.len() >= 3 {
                        &dominant_samples
                    } else {
                        &self.samples
                    };
                    let mut average = [0.0; 3];
                    for sample in source {
                        for index in 0..3 {
                            average[index] += sample[index] / source.len() as f64;
                        }
                    }
                    self.neutral = Some(average);
                    // Seed the median filter with the newly calibrated forward
                    // direction, avoiding an unfiltered first motion frame.
                    self.orientation_samples =
                        vec![self.continuity_origin; ORIENTATION_MEDIAN_SAMPLES - 1];
                    self.last_orientation = Some(self.continuity_origin);
                    self.stationary_samples =
                        vec![self.continuity_origin; STATIONARY_WINDOW_SAMPLES];
                    self.locked_orientation = Some(self.continuity_origin);
                }
                return None;
            }
        };
        // Foreign-bud frames carry a different, independently drifting
        // reference; accepting one would jump the attitude by tens of degrees
        // and back on the next frame of the locked stream.
        if self.stream_subtype != Some(self.last_subtype) {
            return None;
        }

        // LibrePods' measured AirPods sensor mapping. SDA currently renders yaw
        // only, but retaining pitch makes the helper protocol future-proof.
        let pitch = self.continuity_origin[0]
            + ((values[1] - neutral[1]) + (values[2] - neutral[2])) * 0.5 / 32_000.0
                * std::f64::consts::PI;
        // AirPods' sensor difference increases in the opposite direction from
        // SDA/ADM positive yaw. Negate it here so a physical left turn makes a
        // fixed frontal source move toward the listener's right ear.
        let yaw = self.continuity_origin[1]
            + ((values[2] - neutral[2]) - (values[1] - neutral[1])) * 0.5 / 32_000.0
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

    fn unwrap_values(&mut self, raw: [i16; 3]) -> Option<[f64; 3]> {
        let Some(previous) = self.previous_raw else {
            self.unwrapped = raw.map(|value| value as f64);
            self.previous_raw = Some(raw);
            return Some(self.unwrapped);
        };

        // Fold each cyclic delta into (−32000, 32000] so a move across the
        // ±32768 boundary stays a small step. AACP transport recovery and
        // coalesced control notifications can surface samples that are not
        // real motion; accumulating one would permanently re-anchor the
        // attitude by tens of degrees (observed as a sudden ~±100° float
        // while stationary), so implausible steps are dropped outright.
        let mut folded = [0i64; 3];
        let mut plausible = true;
        for index in 1..3 {
            let delta = i64::from(raw[index].wrapping_sub(previous[index]));
            folded[index] = ((delta + 32_000).rem_euclid(64_000)) - 32_000;
            if folded[index].abs() > MAX_FRAME_DELTA_COUNTS {
                plausible = false;
            }
        }
        if !plausible {
            self.unwrap_rejects += 1;
            if self.unwrap_rejects >= UNWRAP_RESYNC_REJECTS {
                // Sustained rejection: the stream re-referenced or the listener
                // moved during a gap. Realign the reference without adding the
                // untrusted delta so the attitude stays continuous.
                self.previous_raw = Some(raw);
                self.unwrap_rejects = 0;
            }
            return None;
        }
        self.unwrap_rejects = 0;
        self.previous_raw = Some(raw);
        // While the stationary lock holds, learn the per-frame gyro bias and
        // cancel it from the accumulated angle so the attitude does not
        // random-walk away with the sensor's integrated drift.
        if self.bias_learning {
            for axis in 0..2 {
                self.bias[axis] += BIAS_LEARN_RATE * (folded[axis + 1] as f64 - self.bias[axis]);
            }
        }
        for index in 0..3 {
            let bias = if (1..3).contains(&index) { self.bias[index - 1] } else { 0.0 };
            // Only what was eaten while the lock firmly held counts as owed:
            // during unlocked motion the subtraction is legitimate drift
            // cancellation, not something a motion confirmation should undo.
            if index >= 1 && self.bias_learning {
                self.bias_applied_total[index - 1] += bias;
            }
            self.unwrapped[index] += folded[index] as f64 - bias;
        }
        Some(self.unwrapped)
    }

    /// Discard the drift estimate and give everything it subtracted back to
    /// the accumulator, so a suspected real motion resumes from where the
    /// attitude truly is.
    fn reset_bias_owing(&mut self) {
        for axis in 0..2 {
            self.unwrapped[axis + 1] += self.bias_applied_total[axis];
        }
        self.bias_applied_total = [0.0; 2];
        self.bias = [0.0; 2];
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

        // AACP transport recovery and occasional malformed sensor bursts can
        // produce several consecutive but impossible attitude samples. Hold a
        // new distant target briefly, but confirm a continuously moving target
        // sooner than a stationary step. Once confirmed, approach it with a
        // bounded per-frame step so neither a fast turn nor recovery can jump.
        if let Some(previous) = self.last_orientation.or(self.locked_orientation) {
            let displacement = (0..2)
                .map(|axis| angle_delta(previous[axis], filtered[axis]).abs())
                .fold(0.0, f64::max);

            if self.slewing_discontinuity {
                filtered = limit_orientation_step(
                    previous,
                    filtered,
                    DISCONTINUITY_MAX_OUTPUT_STEP_RADIANS,
                );
                if displacement <= DISCONTINUITY_MAX_OUTPUT_STEP_RADIANS {
                    self.slewing_discontinuity = false;
                }
            } else if displacement >= DISCONTINUITY_RADIANS {
                if self.pending_discontinuity.is_none() {
                    // Real motion is suspected: give back everything the bias
                    // estimate ate since it was last reset and relearn from
                    // zero once the movement is confirmed.
                    self.reset_bias_owing();
                }
                let pending =
                    update_motion_candidate(previous, filtered, self.pending_discontinuity);
                let confirmed = pending.motion_samples >= DISCONTINUITY_MOTION_CONFIRM_SAMPLES
                    || pending.steady_samples >= DISCONTINUITY_STEADY_CONFIRM_SAMPLES;
                if !confirmed {
                    self.pending_discontinuity = Some(pending);
                    return previous;
                }
                self.pending_discontinuity = None;
                self.pending_stationary_release = None;
                self.locked_orientation = None;
                self.stationary_samples.clear();
                // The attitude just jumped: relearn from zero (the owed bias
                // was already given back when the suspicion first arose).
                self.bias = [0.0; 2];
                self.bias_applied_total = [0.0; 2];
                self.slewing_discontinuity = true;
                filtered = limit_orientation_step(
                    previous,
                    filtered,
                    DISCONTINUITY_MAX_OUTPUT_STEP_RADIANS,
                );
            } else {
                self.pending_discontinuity = None;
            }
        }

        // While the stationary lock holds, learn the per-frame gyro bias; the
        // flag must be set before the lock-hold early returns below. Learning
        // pauses while any motion is pending: until the discontinuity logic
        // confirms the movement is real, chasing it would corrupt the bias.
        self.bias_learning = self.locked_orientation.is_some()
            && self.pending_discontinuity.is_none()
            && self.pending_stationary_release.is_none();

        // A fixed Windows PC has no second IMU to cancel headphone drift. Hold
        // a proven-stationary pose until displacement exceeds a deliberate
        // release threshold. A real turn then remains unlocked until a complete
        // low-excursion window confirms that the head has stopped again.
        if let Some(locked) = self.locked_orientation {
            let displacement = (0..2)
                .map(|axis| angle_delta(locked[axis], filtered[axis]).abs())
                .fold(0.0, f64::max);
            if displacement < STATIONARY_RELEASE_RADIANS {
                self.pending_stationary_release = None;
                return locked;
            }

            let pending =
                update_motion_candidate(locked, filtered, self.pending_stationary_release);
            let confirmed = pending.motion_samples >= STATIONARY_RELEASE_CONFIRM_SAMPLES
                || pending.steady_samples >= DISCONTINUITY_STEADY_CONFIRM_SAMPLES;
            if !confirmed {
                self.pending_stationary_release = Some(pending);
                return locked;
            }
            self.pending_stationary_release = None;
            self.locked_orientation = None;
            self.stationary_samples.clear();
        }

        self.stationary_samples.push(filtered);
        if self.stationary_samples.len() > STATIONARY_WINDOW_SAMPLES {
            self.stationary_samples.remove(0);
        }

        if let Some(last) = self.last_orientation {
            for axis in 0..2 {
                if (filtered[axis] - last[axis]).abs() < ORIENTATION_DEAD_ZONE_RADIANS {
                    filtered[axis] = last[axis];
                }
            }
        }
        self.last_orientation = Some(filtered);
        if self.stationary_samples.len() == STATIONARY_WINDOW_SAMPLES {
            let stable = (0..2).all(|axis| {
                let reference = self.stationary_samples[0][axis];
                let (minimum, maximum) = self
                    .stationary_samples
                    .iter()
                    .map(|sample| angle_delta(reference, sample[axis]))
                    .fold(
                        (f64::INFINITY, f64::NEG_INFINITY),
                        |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
                    );
                maximum - minimum <= STATIONARY_WINDOW_RADIANS
            });
            if stable {
                self.locked_orientation = Some(filtered);
                self.pending_stationary_release = None;
            }
        }
        filtered
    }
}

fn angle_delta(from: f64, to: f64) -> f64 {
    (to - from).sin().atan2((to - from).cos())
}

fn update_motion_candidate(
    origin: [f64; 2],
    target: [f64; 2],
    pending: Option<PendingDiscontinuity>,
) -> PendingDiscontinuity {
    let offset = [
        angle_delta(origin[0], target[0]),
        angle_delta(origin[1], target[1]),
    ];
    let fresh = || PendingDiscontinuity {
        last: target,
        direction: offset,
        motion_samples: 0,
        steady_samples: 1,
    };
    let Some(mut pending) = pending else {
        return fresh();
    };

    let step = [
        angle_delta(pending.last[0], target[0]),
        angle_delta(pending.last[1], target[1]),
    ];
    let step_size = step.iter().map(|value| value.abs()).fold(0.0, f64::max);
    let remains_on_same_side = dot2(pending.direction, offset) > 0.0;
    let advances_in_original_direction = dot2(pending.direction, step) > 0.0;
    if step_size > DISCONTINUITY_MAX_CANDIDATE_STEP_RADIANS || !remains_on_same_side {
        return fresh();
    }

    pending.last = target;
    if step_size < DISCONTINUITY_MIN_PROGRESS_RADIANS {
        pending.steady_samples += 1;
    } else if advances_in_original_direction {
        pending.motion_samples += 1;
        pending.steady_samples = 0;
    } else {
        return fresh();
    }
    pending
}

fn dot2(left: [f64; 2], right: [f64; 2]) -> f64 {
    left[0] * right[0] + left[1] * right[1]
}

fn limit_orientation_step(from: [f64; 2], to: [f64; 2], maximum: f64) -> [f64; 2] {
    let delta = [angle_delta(from[0], to[0]), angle_delta(from[1], to[1])];
    let largest = delta.iter().map(|value| value.abs()).fold(0.0, f64::max);
    if largest <= maximum {
        return to;
    }
    let scale = maximum / largest;
    [from[0] + delta[0] * scale, from[1] + delta[1] * scale]
}

pub(crate) fn is_head_tracking_packet(packet: &[u8]) -> bool {
    head_tracking_frame(packet).is_some()
}

fn head_tracking_frame(packet: &[u8]) -> Option<&[u8]> {
    // A single overlapped driver read can contain a control notification
    // followed by a motion frame. Search the complete L2CAP payload instead of
    // requiring motion to begin at byte zero, but hold motion frames to the
    // full length the reference implementation requires (>= 70 bytes: values
    // at 43..49 plus acceleration at 51..55). A shorter fragment — or a
    // control notification whose payload happens to embed the header — would
    // otherwise parse as a deterministic garbage pose, observed as a sudden
    // ~±100° float while stationary, repeating while playback drives control
    // traffic (ear detection, battery, audio-source ownership).
    packet
        .windows(HEAD_TRACKING_PREFIX.len())
        .enumerate()
        .find_map(|(offset, candidate)| {
            if candidate != HEAD_TRACKING_PREFIX {
                return None;
            }
            let frame = packet.get(offset..)?;
            (frame.len() >= 70 && matches!(frame[10], 0x44 | 0x45) && frame[11] == 0)
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
        packet_with_subtype(0x44, o1, o2, o3)
    }

    fn packet_with_subtype(subtype: u8, o1: i16, o2: i16, o3: i16) -> Vec<u8> {
        let mut packet = vec![0; 72];
        packet[..HEAD_TRACKING_PREFIX.len()].copy_from_slice(&HEAD_TRACKING_PREFIX);
        packet[10] = subtype;
        for (offset, value) in [(43, o1), (45, o2), (47, o3)] {
            packet[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        packet
    }

    fn yaw_packet(degrees: i16) -> Vec<u8> {
        let half_difference = i32::from(degrees) * 32_000 / 180;
        packet(
            19_000,
            (1_000 - half_difference) as i16,
            (1_000 + half_difference) as i16,
        )
    }

    #[test]
    fn calibrates_to_identity_and_maps_yaw() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            assert!(tracker
                .process_packet(&packet(19_000, 1_000, 1_000))
                .is_none());
        }
        assert!(tracker.calibrated());

        let identity = tracker
            .process_packet(&packet(19_000, 1_000, 1_000))
            .unwrap();
        assert!(identity.x.abs() < 1e-12 && identity.y.abs() < 1e-12);
        assert!(identity.z.abs() < 1e-12 && (identity.w - 1.0).abs() < 1e-12);

        let mut turned = None;
        for _ in 0..40 {
            turned = tracker.process_packet(&packet(19_000, 17_000, -15_000));
        }
        let turned = turned.unwrap();
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
        let crossed = tracker
            .process_packet(&packet(19_000, -32_760, 1_000))
            .unwrap();
        assert!(
            crossed.z.abs() < 0.001,
            "boundary crossing became a large yaw: {crossed:?}"
        );
        assert!(crossed.w > 0.999);
    }

    #[test]
    fn suppresses_stationary_jitter_and_a_short_sensor_burst() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&packet(19_000, 1_000, 1_000));
        }
        for _ in 0..2 {
            let jitter = tracker.process_packet(&packet(19_000, 1_080, 920)).unwrap();
            assert!(jitter.z.abs() < 1e-12);
        }
        for _ in 0..3 {
            let spike = tracker
                .process_packet(&packet(19_000, 17_000, -15_000))
                .unwrap();
            assert!(spike.z.abs() < 1e-12);
        }
        for _ in 0..3 {
            let recovered = tracker
                .process_packet(&packet(19_000, 1_000, 1_000))
                .unwrap();
            assert!(recovered.z.abs() < 1e-12);
        }
    }

    #[test]
    fn accepts_a_confirmed_fast_turn_after_rejecting_transient_targets() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&packet(19_000, 1_000, 1_000));
        }

        let mut turned = None;
        for sample in 0..=DISCONTINUITY_STEADY_CONFIRM_SAMPLES {
            turned = tracker.process_packet(&packet(19_000, 17_000, -15_000));
            if sample < DISCONTINUITY_STEADY_CONFIRM_SAMPLES {
                assert!(turned.unwrap().z.abs() < 1e-12);
            }
        }
        assert!(turned.unwrap().z.abs() > 0.04);
    }

    #[test]
    fn accepts_fast_continuous_turns_in_both_directions() {
        for direction in [-1, 1] {
            let mut tracker = HeadOrientation::default();
            for _ in 0..CALIBRATION_SAMPLES {
                tracker.process_packet(&yaw_packet(0));
            }

            let mut turned = None;
            for degrees in [10, 20, 30, 40] {
                turned = tracker.process_packet(&yaw_packet(direction * degrees));
            }
            let turned = turned.unwrap();
            assert!(
                turned.z * f64::from(direction) > 0.04,
                "fast turn remained locked for direction {direction}: {turned:?}"
            );
        }
    }

    #[test]
    fn keeps_stationary_lock_during_an_alternating_small_angle_burst() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&yaw_packet(0));
        }

        for degrees in [2, -2].into_iter().cycle().take(24) {
            let pose = tracker.process_packet(&yaw_packet(degrees)).unwrap();
            assert!(
                pose.z.abs() < 1e-12,
                "alternating small-angle burst escaped the stationary lock: {pose:?}"
            );
        }
    }

    #[test]
    fn rejects_an_alternating_large_discontinuity_burst() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&yaw_packet(0));
        }

        for degrees in [10, -10].into_iter().cycle().take(24) {
            let pose = tracker.process_packet(&yaw_packet(degrees)).unwrap();
            assert!(
                pose.z.abs() < 1e-12,
                "alternating large-angle burst passed discontinuity confirmation: {pose:?}"
            );
        }
    }

    #[test]
    fn preserves_the_last_pose_when_transport_recalibrates_to_a_new_raw_origin() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&packet(19_000, 1_000, 1_000));
        }
        let mut before = None;
        for _ in 0..=DISCONTINUITY_STEADY_CONFIRM_SAMPLES {
            before = tracker.process_packet(&packet(19_000, 9_000, -7_000));
        }
        let before = before.unwrap();

        tracker.begin_transport_session();
        for _ in 0..CALIBRATION_SAMPLES {
            assert!(tracker
                .process_packet(&packet(-4_000, -12_000, 20_000))
                .is_none());
        }
        let after = tracker
            .process_packet(&packet(-4_000, -12_000, 20_000))
            .unwrap();
        assert!((after.x - before.x).abs() < 1e-12);
        assert!((after.y - before.y).abs() < 1e-12);
        assert!((after.z - before.z).abs() < 1e-12);
        assert!((after.w - before.w).abs() < 1e-12);
    }

    #[test]
    fn estimates_and_cancels_slow_stationary_drift() {
        // The AirPods report gyro-integrated relative angles: sitting still,
        // the raw value creeps in one direction. The stationary lock used to
        // follow that drift once it crossed the release threshold, which
        // random-walked the attitude away over minutes. The learned per-frame
        // bias must cancel it instead.
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&packet(19_000, 1_000, 1_000));
        }

        for step in 1..=60 {
            let drift = tracker
                .process_packet(&packet(19_000, 1_000 + step * 10, 1_000 - step * 10))
                .unwrap();
            assert!(
                drift.z.abs() < 0.01,
                "slow drift escaped at step {step}: z={}",
                drift.z
            );
        }
    }

    #[test]
    fn ignores_the_other_bud_stream_after_calibration() {
        // Both buds notify motion with independent reference frames; frames
        // from the non-calibration stream must vanish entirely.
        let mut tracker = HeadOrientation::default();
        for _ in 0..8 {
            tracker.process_packet(&packet_with_subtype(0x45, 19_000, 1_000, 1_000));
        }
        for _ in 0..2 {
            tracker.process_packet(&packet_with_subtype(0x44, 19_000, 9_000, -7_000));
        }
        assert!(tracker.calibrated());

        let baseline = tracker
            .process_packet(&packet_with_subtype(0x45, 19_000, 1_000, 1_000))
            .unwrap();
        assert!(tracker
            .process_packet(&packet_with_subtype(0x44, 19_000, 30_000, -30_000))
            .is_none());
        let after = tracker
            .process_packet(&packet_with_subtype(0x45, 19_000, 1_000, 1_000))
            .unwrap();
        assert!(
            (after.z - baseline.z).abs() < 1e-12 && (after.w - baseline.w).abs() < 1e-12,
            "foreign-bud frame moved the attitude: {after:?}"
        );
    }

    #[test]
    fn rejects_non_tracking_packets() {
        let mut tracker = HeadOrientation::default();
        assert!(tracker.process_packet(&[0; 64]).is_none());
    }

    #[test]
    fn rejects_a_short_fragment_that_embeds_a_valid_header() {
        // 61..69-byte tails (a split read or a control notification whose
        // payload embeds the header) must not parse as motion.
        let mut full = packet(19_000, 1_000, 1_000);
        full.truncate(69);
        assert!(!is_head_tracking_packet(&full));
        full.truncate(64);
        assert!(!is_head_tracking_packet(&full));
    }

    #[test]
    fn drops_an_implausible_sample_instead_of_reanchoring_the_attitude() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&yaw_packet(0));
        }

        // One misparsed coalesced control packet claims a ~+117° step on axis
        // 2 while the head is still. It must vanish, not shift the pose.
        assert!(tracker.process_packet(&packet(19_000, 1_000, 21_000)).is_none());
        let recovered = tracker
            .process_packet(&packet(19_000, 1_000, 1_000))
            .unwrap();
        assert!(
            recovered.z.abs() < 1e-12,
            "garbage sample re-anchored the attitude: {recovered:?}"
        );

        // Tracking must still work afterwards.
        let mut turned = None;
        for _ in 0..=DISCONTINUITY_STEADY_CONFIRM_SAMPLES {
            turned = tracker.process_packet(&yaw_packet(20));
        }
        assert!(
            turned.unwrap().z.abs() > 0.017,
            "tracking went dead after a rejected sample"
        );
    }

    #[test]
    fn realigns_after_sustained_implausible_samples_without_a_pose_jump() {
        let mut tracker = HeadOrientation::default();
        for _ in 0..CALIBRATION_SAMPLES {
            tracker.process_packet(&yaw_packet(0));
        }
        let before = tracker.process_packet(&yaw_packet(0)).unwrap();

        // A device-side re-reference steps the raw branch ~+117° and stays
        // there. After the resync window the stream is adopted while the
        // reported attitude stays where it was.
        for _ in 0..UNWRAP_RESYNC_REJECTS {
            assert!(tracker.process_packet(&packet(19_000, 1_000, 21_000)).is_none());
        }
        let after = tracker
            .process_packet(&packet(19_000, 1_000, 21_000))
            .unwrap();
        assert!(
            (after.z - before.z).abs() < 1e-9 && (after.w - before.w).abs() < 1e-9,
            "re-reference leaked into the attitude: {after:?}"
        );
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
