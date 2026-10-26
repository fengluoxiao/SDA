//! Static front-image calibration and authored stereo bass preservation.
//! Only stereo-codec front direct FIRs change; residual/room and rear sends stay intact.
use crate::{hrtf, vbap};
type Pair = (Vec<f32>, Vec<f32>);
pub(super) struct Responses {
    pub indices: Vec<usize>,
    pub normal: Vec<Pair>,
    pub preserved: Vec<Pair>,
}
const RADIUS: usize = 160;
fn low(ir: &[f32]) -> Option<Vec<f32>> {
    // Dry shared responses are padded to the room bank length. Ignore that
    // silent suffix so correction preparation cannot grow every FFT history.
    let end = ir.iter().rposition(|x| *x != 0.).map_or(0, |i| i + 1);
    let ir = &ir[..end];
    // Shared HRTFHQ has a >=160-sample causal arrival reserve. Refuse another
    // profile rather than truncate a zero-phase correction before time zero.
    if ir.iter().take(RADIUS).any(|x| *x != 0.) {
        return None;
    }
    let mut h: Vec<f64> = (0..=2 * RADIUS)
        .map(|i| {
            let x = i as f64 - RADIUS as f64;
            let f = 140. / 48000.;
            let sinc = if x == 0. {
                2. * f
            } else {
                (std::f64::consts::TAU * f * x).sin() / (std::f64::consts::PI * x)
            };
            sinc * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (2 * RADIUS) as f64).cos())
        })
        .collect();
    let sum = h.iter().sum::<f64>();
    for x in &mut h {
        *x /= sum;
    }
    let mut out = vec![0f64; ir.len() + RADIUS];
    for (i, &x) in ir.iter().enumerate().filter(|(_, x)| **x != 0.) {
        for (k, &g) in h.iter().enumerate() {
            if i + k >= RADIUS {
                out[i + k - RADIUS] += x as f64 * g;
            }
        }
    }
    Some(out.into_iter().map(|v| v as f32).collect())
}
pub(super) fn prepare(
    set: &hrtf::NativeHrtfSet,
    solver: &vbap::VbapSolver,
    wet: f32,
) -> Result<Option<Responses>, String> {
    if !set.uses_shared_measured_response()
        || set.cinema.enabled
        || set.cinema.monitor.hardware.enabled
    {
        return Ok(None);
    }
    let speakers = vbap::speakers(solver.layout());
    let Some(left) = speakers.iter().position(|s| s.name == "FrontLeft") else {
        return Ok(None);
    };
    let Some(right) = speakers.iter().position(|s| s.name == "FrontRight") else {
        return Ok(None);
    };
    let load = |i: usize, w| {
        let s = &speakers[i];
        set.mixed_speaker(
            s.name,
            solver.layout().as_str(),
            s.azimuth as f64,
            s.elevation as f64,
            w,
        )
    };
    // Explicit monitor trims/delays belong to the listener's calibration.
    // Keep the existing low-band preservation there without symmetrizing it.
    let mirror_front = !set.cinema.monitor.enabled;
    let mut indices = vec![left, right];
    if mirror_front {
        if let Some(center) = speakers.iter().position(|s| s.name == "Center") {
            indices.push(center);
        }
    }
    let mut normal: Vec<Pair> = indices
        .iter()
        .map(|&i| load(i, wet))
        .collect::<Result<_, _>>()?;
    let mut dry: Vec<Pair> = indices
        .iter()
        .map(|&i| load(i, 0.))
        .collect::<Result<_, _>>()?;
    let n = normal
        .iter()
        .chain(&dry)
        .flat_map(|p| [p.0.len(), p.1.len()])
        .max()
        .unwrap();
    for p in normal.iter_mut().chain(&mut dry) {
        p.0.resize(n, 0.);
        p.1.resize(n, 0.);
    }
    let mut calibrated = dry.clone();
    // Average corresponding near-ear and far-ear responses of the mirrored
    // front directions, not left/right PCM or each source's two ears. This
    // keeps distinct near/far arrivals and directional crossfeed while removing
    // the generic measurement's incidental left/right spectral/phase mismatch.
    // Their all-ear common sum stays unchanged. No signal-dependent control.
    if mirror_front {
        for i in 0..n {
            let near = (dry[0].0[i] + dry[1].1[i]) * 0.5;
            let far = (dry[0].1[i] + dry[1].0[i]) * 0.5;
            calibrated[0].0[i] = near;
            calibrated[1].1[i] = near;
            calibrated[0].1[i] = far;
            calibrated[1].0[i] = far;
            if calibrated.len() > 2 {
                let center = (dry[2].0[i] + dry[2].1[i]) * 0.5;
                calibrated[2].0[i] = center;
                calibrated[2].1[i] = center;
            }
        }
    }
    let Some(to_left) = low(&calibrated[1].0) else {
        return Ok(None);
    };
    let Some(to_right) = low(&calibrated[0].1) else {
        return Ok(None);
    };
    // A direct-only profile may have no room-bank padding. Retain the full
    // causal correction tail in that case rather than indexing past the FIR.
    let correction_len = n.max(to_left.len()).max(to_right.len());
    if correction_len > n {
        for p in normal.iter_mut().chain(&mut dry).chain(&mut calibrated) {
            p.0.resize(correction_len, 0.);
            p.1.resize(correction_len, 0.);
        }
    }
    // Move only the calibrated low-frequency cross-ear term to its authored
    // ear. This preserves the newly calibrated common component sample by sample.
    for (i, &x) in to_left.iter().enumerate() {
        calibrated[0].0[i] += x;
        calibrated[1].0[i] -= x;
    }
    for (i, &x) in to_right.iter().enumerate() {
        calibrated[0].1[i] -= x;
        calibrated[1].1[i] += x;
    }
    let preserved = normal
        .iter()
        .zip(&dry)
        .zip(&calibrated)
        .map(|((normal, dry), direct)| {
            // Preserve each source/ear's complete original room residual exactly;
            // apply the fixed calibration to the direct response only.
            let apply = |n: &[f32], d: &[f32], c: &[f32]| {
                n.iter()
                    .zip(d)
                    .zip(c)
                    .map(|((&n, &d), &c)| c + (n - d))
                    .collect()
            };
            (
                apply(&normal.0, &dry.0, &direct.0),
                apply(&normal.1, &dry.1, &direct.1),
            )
        })
        .collect();
    Ok(Some(Responses {
        indices,
        normal,
        preserved,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> hrtf::NativeHrtfSet {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        hrtf::NativeHrtfSet::load_calibrated(&root).unwrap()
    }
    #[test]
    fn stereo_front_calibration_preserves_common_average_and_shared_room_residual() {
        let mut set = profile();
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        set.configure_spatial_cues(solver.layout()).unwrap();
        let dry = prepare(&set, &solver, 0.).unwrap().unwrap();
        let wet = prepare(&set, &solver, set.effective_wet(0.))
            .unwrap()
            .unwrap();
        for (i, index) in wet.indices.iter().enumerate() {
            let speaker = &vbap::speakers(solver.layout())[*index];
            let original = set
                .mixed_speaker(
                    speaker.name,
                    solver.layout().as_str(),
                    speaker.azimuth as f64,
                    speaker.elevation as f64,
                    set.effective_wet(0.),
                )
                .unwrap();
            assert_eq!(
                wet.normal[i].0.len(),
                original.0.len(),
                "mobile front FFT histories grew for a zero-padded suffix"
            );
        }
        for ear in 0..2 {
            fn samples(p: &Pair, ear: usize) -> &[f32] {
                if ear == 0 { &p.0 } else { &p.1 }
            }
            for i in 0..wet.normal[0].0.len() {
                let common_average = (dry.normal[0].0[i]
                    + dry.normal[0].1[i]
                    + dry.normal[1].0[i]
                    + dry.normal[1].1[i])
                    * 0.5;
                let residual = samples(&wet.normal[0], ear)[i] + samples(&wet.normal[1], ear)[i]
                    - samples(&dry.normal[0], ear)[i]
                    - samples(&dry.normal[1], ear)[i];
                let before = common_average + residual;
                let after = samples(&wet.preserved[0], ear)[i] + samples(&wet.preserved[1], ear)[i];
                assert!(
                    (before - after).abs() < 2e-7,
                    "common component changed at {i}"
                );
                for source in 0..wet.indices.len() {
                    let old =
                        samples(&wet.normal[source], ear)[i] - samples(&dry.normal[source], ear)[i];
                    let new = samples(&wet.preserved[source], ear)[i]
                        - samples(&dry.preserved[source], ear)[i];
                    assert!((old - new).abs() < 2e-7, "room residual changed at {i}");
                }
            }
        }
        set.configure_cinema(
            crate::cinema::Settings {
                enabled: true,
                ..Default::default()
            },
            None,
        );
        assert!(
            prepare(&set, &solver, 0.).unwrap().is_none(),
            "explicit user room must keep its original calibration"
        );
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    #[test]
    fn stereo_front_direct_is_mirrored_without_erasing_far_ear_arrival() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        let set = hrtf::NativeHrtfSet::load_calibrated(&root).unwrap();
        for layout in [vbap::LayoutId::Stereo2_0, vbap::LayoutId::Dolby7_1_4] {
            let solver = vbap::VbapSolver::with_layout(layout);
            let b = prepare(&set, &solver, 0.).unwrap().unwrap();
            let l = &b.preserved[0];
            let r = &b.preserved[1];
            let max_diff =
                l.0.iter()
                    .zip(&r.1)
                    .chain(l.1.iter().zip(&r.0))
                    .map(|(a, b)| (a - b).abs())
                    .fold(0f32, f32::max);
            assert!(
                max_diff < 2e-7,
                "front direct pairs are not mirrored: {max_diff}"
            );
            let peak = |v: &[f32]| {
                v.iter()
                    .enumerate()
                    .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                    .unwrap()
                    .0
            };
            assert!(
                peak(&l.1) > peak(&l.0),
                "left far-ear arrival lost its delay"
            );
            assert!(
                peak(&r.0) > peak(&r.1),
                "right far-ear arrival lost its delay"
            );
            let speakers = vbap::speakers(layout);
            if let Some(i) = b.indices.iter().position(|i| speakers[*i].name == "Center") {
                assert_eq!(
                    b.preserved[i].0, b.preserved[i].1,
                    "upmix center is not centered"
                );
            } else {
                assert!(
                    !speakers.iter().any(|s| s.name == "Center"),
                    "upmix center calibration missing"
                );
            }
        }
    }
    #[test]
    fn stereo_front_preserves_explicit_monitor_calibration() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        let mut set = hrtf::NativeHrtfSet::load_calibrated(&root).unwrap();
        set.cinema.monitor.enabled = true;
        let solver = vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4);
        let b = prepare(&set, &solver, 0.).unwrap().unwrap();
        assert_eq!(
            b.indices.len(),
            2,
            "user-calibrated center must be untouched"
        );
        for i in 0..b.normal[0].0.len() {
            for ear in 0..2 {
                let p = |s: usize| {
                    if ear == 0 {
                        &b.preserved[s].0
                    } else {
                        &b.preserved[s].1
                    }
                };
                let n = |s: usize| {
                    if ear == 0 {
                        &b.normal[s].0
                    } else {
                        &b.normal[s].1
                    }
                };
                assert!(((p(0)[i] + p(1)[i]) - (n(0)[i] + n(1)[i])).abs() < 2e-7);
            }
        }
    }
}
