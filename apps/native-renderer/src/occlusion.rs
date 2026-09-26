//! Source-to-source occlusion. The Atmos/ADM model places objects
//! independently, so a nearer object sitting on the same bearing as a farther
//! one must shadow it — the far source reaches the blocked ear mostly by
//! diffraction (low frequencies survive, highs do not) and loses overall
//! level. Without this, front/back object pairs on one bearing collapse into
//! an inseparable image, which reads as a depth-rendering defect.
//!
//! Per-block occlusion amounts come from the engine's pairwise pass; this
//! module only applies the smoothed per-ear shadow.

/// One-pole low shelf per ear. `1.0` = bypass; smaller = more shadow.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shadow {
    state: [f32; 2],
}

impl Shadow {
    pub fn is_bypassed(&self, amount: [f32; 2]) -> bool {
        amount[0] >= 1.0 - 1e-3
            && amount[1] >= 1.0 - 1e-3
            && self.state.iter().all(|s| s.abs() < 1e-20)
    }

    /// `amount[ear]` in `[0, 1]`: how open the ear is (1 = unoccluded).
    /// Below 1 the ear gets a first-order low-pass whose corner drops with
    /// the occlusion depth plus a proportional direct-level trim.
    pub fn process(&mut self, input: [f32; 2], amount: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|ear| {
            let open = amount[ear].clamp(0.0, 1.0);
            if open >= 0.999 {
                self.state[ear] += (input[ear] - self.state[ear]) * 0.087557;
                if self.state[ear].abs() < 1e-20 {
                    self.state[ear] = 0.0;
                }
                return input[ear];
            }
            // Corner frequency from 20 kHz (open) down to ~500 Hz (fully blocked).
            let corner = (500.0 * 40.0_f32.powf(open)).max(500.0).min(20_000.0);
            let alpha = 1.0 - (-std::f32::consts::TAU * corner / 48_000.0).exp();
            self.state[ear] += alpha * (input[ear] - self.state[ear]);
            if self.state[ear].abs() < 1e-20 {
                self.state[ear] = 0.0;
            }
            input[ear] * (0.55 + 0.45 * open) + self.state[ear] * (1.0 - (0.55 + 0.45 * open))
        })
    }
}

fn openness_for_lateral_shadow(lateral: f32, occluded: f32) -> [f32; 2] {
    let mut openness = [1.0_f32; 2];
    if lateral.abs() < 1e-4 {
        let centred = 1.0 - 0.2 * occluded;
        openness = [centred; 2];
    } else {
        let far = if lateral < 0.0 { 1 } else { 0 };
        openness[far] = 1.0 - 0.4 * occluded;
    }
    openness
}

/// Pairwise occlusion amounts for one source against all others.
/// Returns per-ear openness in [0, 1]; 1 = fully open.
pub fn ear_openness(
    position: [f32; 3],
    head: Option<[f32; 4]>,
    others: &[(f32, [f32; 3])],
) -> [f32; 2] {
    let norm = position.iter().map(|a| a * a).sum::<f32>().sqrt();
    if norm < 1e-5 {
        return [1.0; 2];
    }
    let mut occluded = 0.0_f32;
    for &(other_distance, other) in others {
        let other_norm = other_distance;
        if other_norm >= norm * 0.6 || other_norm >= 0.7 || other_norm < 1e-5 {
            continue; // the other must sit clearly in front of this source
        }
        let dot = position.iter().zip(other).map(|(a, b)| a * b).sum::<f32>() / (norm * other_norm);
        if dot <= 0.0 {
            continue;
        }
        let angle = dot.clamp(-1.0, 1.0).acos();
        if angle >= 8.0_f32.to_radians() {
            continue;
        }
        // Deeper block when the occluder is closer to the bearing centre and
        // much nearer to the listener than the occluded source.
        let bearing = 1.0 - angle / 8.0_f32.to_radians();
        let nearness = (1.0 - other_norm / (norm * 0.6)).clamp(0.0, 1.0);
        occluded = occluded.max(bearing * nearness);
    }
    if occluded <= 1e-3 {
        return [1.0; 2];
    }
    // ADM x is negative on the listener's left. The opposite ear is shaded;
    // a centred source has no lateral preference, so both ears get the same
    // attenuation. Treating x == 0 as one side made front objects lean left.
    let relative = crate::spatial::head_relative_adm(position, head);
    openness_for_lateral_shadow(relative[0], occluded)
}

/// Allocation-free variant used by the render loop: the caller pre-rotates
/// every source into head-relative space once per block.
pub fn ear_openness_prepared(
    position: &[f32; 3],
    position_relative: &[f32; 3],
    all_positions: &[[f32; 3]],
    all_relative: &[[f32; 3]],
    self_index: usize,
) -> [f32; 2] {
    let norm = position.iter().map(|a| a * a).sum::<f32>().sqrt();
    if norm < 1e-5 {
        return [1.0; 2];
    }
    let mut occluded = 0.0_f32;
    for (other_index, other) in all_positions.iter().enumerate() {
        if other_index == self_index {
            continue;
        }
        let other_norm = other.iter().map(|a| a * a).sum::<f32>().sqrt();
        if other_norm >= norm * 0.6 || other_norm >= 0.7 || other_norm < 1e-5 {
            continue;
        }
        let dot = position
            .iter()
            .zip(other.iter())
            .map(|(a, b)| a * b)
            .sum::<f32>()
            / (norm * other_norm);
        if dot <= 0.0 {
            continue;
        }
        let angle = dot.clamp(-1.0, 1.0).acos();
        if angle >= 8.0_f32.to_radians() {
            continue;
        }
        let bearing = 1.0 - angle / 8.0_f32.to_radians();
        let nearness = (1.0 - other_norm / (norm * 0.6)).clamp(0.0, 1.0);
        occluded = occluded.max(bearing * nearness);
    }
    if occluded <= 1e-3 {
        return [1.0; 2];
    }
    openness_for_lateral_shadow(position_relative[0], occluded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_same_bearing_object_shadows_far_ear() {
        // A centred source remains centred when a nearer object shadows it.
        let openness = ear_openness([0.0, 1.0, 0.0], None, &[(0.25, [0.0, 0.25, 0.0])]);
        assert!(
            openness[0] < 0.99 && openness[1] < 0.99,
            "occlusion must engage: {openness:?}"
        );
        assert!(
            (openness[0] - openness[1]).abs() < 1e-6,
            "front shadow must remain centred: {openness:?}"
        );
        // ADM x is negative on the left. Shade the ear farther from each
        // lateral source; the two cases must be mirror images.
        let left = ear_openness([-1.0, 0.0, 0.0], None, &[(0.25, [-0.25, 0.0, 0.0])]);
        let right = ear_openness([1.0, 0.0, 0.0], None, &[(0.25, [0.25, 0.0, 0.0])]);
        assert!(
            left[1] < left[0],
            "left source must shade the right ear: {left:?}"
        );
        assert!(
            right[0] < right[1],
            "right source must shade the left ear: {right:?}"
        );
        assert!(
            (left[0] - right[1]).abs() < 1e-6 && (left[1] - right[0]).abs() < 1e-6,
            "lateral shadows must mirror: left={left:?} right={right:?}"
        );
        let positions = [[0.0, 1.0, 0.0], [0.0, 0.25, 0.0]];
        assert_eq!(
            ear_openness_prepared(&positions[0], &positions[0], &positions, &positions, 0),
            openness,
            "parallel object path must match the regular occlusion path"
        );
        // Sideways placement keeps both ears open.
        let sideways = ear_openness([1.0, 0.0, 0.0], None, &[(0.25, [0.0, 0.25, 0.0])]);
        assert_eq!(sideways, [1.0; 2]);
        // No occluders at all = fully open.
        assert_eq!(ear_openness([0.0, 1.0, 0.0], None, &[]), [1.0; 2]);
    }

    #[test]
    fn shadow_filters_low_passes_and_bypasses_cleanly() {
        let mut shadow = Shadow::default();
        // Fully blocked ear: high frequencies must collapse relative to lows.
        let mut low_energy = 0.0;
        let mut high_energy = 0.0;
        for i in 0..4800 {
            let low = (i as f32 * 0.02).sin() * 0.1;
            let high = (i as f32 * 0.9).sin() * 0.1;
            let out = shadow.process([low, high], [0.0, 0.0]);
            low_energy += out[0] * out[0];
            high_energy += out[1] * out[1];
        }
        let open_low = low_energy;
        let open_reference = 2400.0 * 0.005;
        assert!(
            open_low > 0.2 * open_reference,
            "low band must survive diffraction: {low_energy}"
        );
        assert!(
            high_energy < open_low,
            "blocked ear must attenuate highs more: {high_energy} vs {low_energy}"
        );
        // Back to open: exact passthrough after settling.
        let mut shadow = Shadow::default();
        for i in 0..4800 {
            let x = (i as f32 * 0.13).sin() * 0.1;
            let out = shadow.process([x, x * 0.5], [1.0, 1.0]);
            assert!((out[0] - x).abs() < 1e-4);
            assert!((out[1] - x * 0.5).abs() < 1e-4);
        }
    }
}
