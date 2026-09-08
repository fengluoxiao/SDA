use serde::Deserialize;
use crate::vbap;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum Zone {
    Cartesian { min: [f32; 3], max: [f32; 3] },
    Polar { min: [f32; 2], max: [f32; 2] },
}

const EPS: f32 = 1e-6;
const LAYERS: [[u8; 4]; 4] = [[0, 1, 2, 3], [3, 0, 1, 2], [3, 2, 0, 1], [3, 2, 1, 0]];
fn layer(e: f32) -> usize { if e < -10.0 { 0 } else if e < 10.0 { 1 } else if e < 75.0 { 2 } else { 3 } }
fn sign(v: f32) -> f32 { if v > EPS { 1.0 } else if v < -EPS { -1.0 } else { 0.0 } }
fn position(az: f32, el: f32) -> [f32; 3] {
    let (az, el) = (az.to_radians(), el.to_radians());
    [-az.sin() * el.cos(), az.cos() * el.cos(), el.sin()]
}

impl Zone {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Cartesian { min, max } => (0..3).all(|i| min[i].is_finite() && max[i].is_finite() && min[i] >= -1.0 && max[i] <= 1.0 && min[i] <= max[i]),
            Self::Polar { min, max } => min.iter().chain(max).all(|v| v.is_finite()) && min[0].abs() <= 180.0 && max[0].abs() <= 180.0 && min[1] >= -90.0 && max[1] <= 90.0 && min[1] <= max[1],
        }
    }
    pub(crate) fn contains(&self, az: f32, el: f32) -> bool {
        match self {
            Self::Cartesian { min, max } => {
                let p = position(az, el);
                (0..3).all(|i| p[i] >= min[i] - EPS && p[i] <= max[i] + EPS)
            }
            Self::Polar { min, max } => {
                let span = max[0] - min[0];
                let offset = (az - min[0]).rem_euclid(360.0);
                let in_az = el.abs() >= 90.0 - EPS || span >= 360.0 - EPS || offset <= span.rem_euclid(360.0) + EPS || offset >= 360.0 - EPS;
                in_az && el >= min[1] - EPS && el <= max[1] + EPS
            }
        }
    }
}

/// EBU EAR priority: layer, front/back half, Cartesian distance, front/back distance.
pub(crate) fn apply(gains: &mut [f32; vbap::MAX_BUS_COUNT], solver: &vbap::VbapSolver, zones: &[Zone]) {
    if zones.is_empty() { return; }
    let n = solver.bus_count();
    let mut excluded = [false; vbap::MAX_BUS_COUNT];
    let mut positions = [[0.0; 3]; vbap::MAX_BUS_COUNT];
    let mut elevations = [0.0; vbap::MAX_BUS_COUNT];
    for i in 0..n {
        let (az, el) = solver.speaker_direction(i);
        positions[i] = position(az, el);
        elevations[i] = el;
        excluded[i] = zones.iter().any(|zone| zone.contains(az, el));
    }
    if excluded[..n].iter().all(|v| *v) || excluded[..n].iter().all(|v| !*v) { return; }
    let mut energy = gains.map(|v| v * v);
    for from in 0..n {
        if !excluded[from] { continue; }
        let a = positions[from];
        let mut best: Option<[f32; 4]> = None;
        let mut targets = [0usize; vbap::MAX_BUS_COUNT];
        let mut count = 0;
        for to in 0..n {
            if excluded[to] { continue; }
            let b = positions[to];
            let key = [LAYERS[layer(elevations[from])][layer(elevations[to])] as f32,
                (sign(a[1]) - sign(b[1])).abs(), (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt(), (a[1] - b[1]).abs()];
            let difference = best.and_then(|old| (0..4).find(|i| (key[*i] - old[*i]).abs() >= EPS));
            if best.is_none() || difference.is_some_and(|i| key[i] < best.unwrap()[i]) {
                best = Some(key); count = 1; targets[0] = to;
            } else if difference.is_none() { targets[count] = to; count += 1; }
        }
        let share = gains[from].powi(2) / count as f32;
        energy[from] = 0.0;
        for target in &targets[..count] { energy[*target] += share; }
    }
    for i in 0..n { gains[i] = energy[i].sqrt(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polar_wrap_poles_and_cartesian_bounds() {
        let rear = Zone::Polar { min: [150.0, -90.0], max: [-150.0, 90.0] };
        assert!(rear.contains(180.0, 0.0)); assert!(rear.contains(-160.0, 0.0));
        assert!(!rear.contains(0.0, 0.0)); assert!(rear.contains(0.0, 90.0));
        let left = Zone::Cartesian { min: [-1.0, -1.0, -1.0], max: [-0.1, 1.0, 1.0] };
        assert!(left.contains(90.0, 0.0)); assert!(!left.contains(-90.0, 0.0));
    }
    #[test]
    fn downmix_conserves_energy_and_all_excluded_is_identity() {
        let solver = vbap::VbapSolver::new();
        let mut gains = solver.pan([-1.0, 1.0, 0.5], 0.5);
        let original = gains;
        let zone = Zone::Cartesian { min: [-1.0, -1.0, -1.0], max: [-0.1, 1.0, 1.0] };
        apply(&mut gains, &solver, &[zone.clone()]);
        assert!((gains.iter().map(|v| v*v).sum::<f32>() - original.iter().map(|v| v*v).sum::<f32>()).abs() < 1e-5);
        for (i, gain) in gains.iter().enumerate().take(solver.bus_count()) {
            let (az, el) = solver.speaker_direction(i); if zone.contains(az, el) { assert_eq!(*gain, 0.0); }
        }
        let before = gains;
        apply(&mut gains, &solver, &[Zone::Cartesian { min: [-1.0; 3], max: [1.0; 3] }]);
        assert_eq!(gains, before);
        apply(&mut gains, &solver, &[]); assert_eq!(gains, before);
    }
}
