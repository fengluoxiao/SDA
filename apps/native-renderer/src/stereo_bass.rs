//! Preserve authored stereo bass without changing common/front or rear content.
//! Only prepared front-speaker direct FIRs change; residual/room sends stay intact.
use crate::{hrtf, vbap};
type Pair = (Vec<f32>, Vec<f32>);
pub(super) struct Responses {
    pub indices: [usize; 2],
    pub normal: [Pair; 2],
    pub preserved: [Pair; 2],
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
    let mut normal = [load(left, wet)?, load(right, wet)?];
    let dry = [load(left, 0.)?, load(right, 0.)?];
    let Some(to_left) = low(&dry[1].0) else {
        return Ok(None);
    };
    let Some(to_right) = low(&dry[0].1) else {
        return Ok(None);
    };
    let n = normal
        .iter()
        .flat_map(|p| [p.0.len(), p.1.len()])
        .chain([to_left.len(), to_right.len()])
        .max()
        .unwrap();
    for p in &mut normal {
        p.0.resize(n, 0.);
        p.1.resize(n, 0.);
    }
    let mut preserved = normal.clone();
    // Move only the low-frequency cross-ear term back to its authored ear.
    // For L==R, every sample in the two-source sum remains unchanged.
    for (i, &x) in to_left.iter().enumerate() {
        preserved[0].0[i] += x;
        preserved[1].0[i] -= x;
    }
    for (i, &x) in to_right.iter().enumerate() {
        preserved[0].1[i] -= x;
        preserved[1].1[i] += x;
    }
    Ok(Some(Responses {
        indices: [left, right],
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
    fn stereo_bass_preserves_every_common_sample_and_shared_room_residual() {
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
                let before = samples(&wet.normal[0], ear)[i] + samples(&wet.normal[1], ear)[i];
                let after = samples(&wet.preserved[0], ear)[i] + samples(&wet.preserved[1], ear)[i];
                assert!(
                    (before - after).abs() < 2e-7,
                    "common component changed at {i}"
                );
                for source in 0..2 {
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
