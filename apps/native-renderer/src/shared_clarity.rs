//! User-approved common-ear midband presentation for shared HRTFHQ profile v3.
//! Raw HRTF files stay unchanged; this explicit layer transforms preparation copies.
use crate::hrtf::StereoIr;
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use serde::Deserialize;
use std::sync::{Arc, OnceLock};
pub(crate) const PROFILE_MARKER: &str = "Shared HRTFHQ clarity profile v1";
const FREQUENCIES: [f64; 7] = [500., 630., 800., 1000., 1250., 1600., 2000.];
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Point {
    azimuth: f64,
    elevation: f64,
    gains_db: [f64; 7],
}
#[derive(Deserialize)]
struct Recipe {
    points: Vec<Point>,
}
fn recipe() -> &'static Recipe {
    static R: OnceLock<Recipe> = OnceLock::new();
    R.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/shared-clarity/recipe.json")).unwrap()
    })
}
fn vector(a: f64, e: f64) -> [f64; 3] {
    let (a, e) = (a.to_radians(), e.to_radians());
    [a.sin() * e.cos(), a.cos() * e.cos(), e.sin()]
}
fn curve(az: f64, el: f64) -> [f64; 7] {
    if az.abs() <= 45.0 {
        return [0.; 7];
    }
    let x = ((az.abs() - 45.) / 30.).clamp(0., 1.);
    let gate = x * x * (3. - 2. * x);
    let v = vector(az, el);
    let mut sum = [0.; 7];
    let mut total = 0.;
    for p in &recipe().points {
        let q = vector(p.azimuth, p.elevation);
        let d = v.iter().zip(q).map(|(a, b)| (a - b).powi(2)).sum::<f64>();
        if d < 1e-16 {
            return p.gains_db.map(|v| v * gate);
        }
        let w = 1. / d.powi(2);
        total += w;
        for i in 0..7 {
            sum[i] += w * p.gains_db[i];
        }
    }
    // Continuous front-sector bypass. All neighbours participate, avoiding
    // abrupt nearest-neighbour changes on source movement or head rotation.
    sum.map(|v| (v / total * gate).clamp(-3., 3.))
}
fn interpolate(g: &[f64; 7], f: f64) -> f64 {
    for i in 1..7 {
        if f <= FREQUENCIES[i] {
            let t = (f / FREQUENCIES[i - 1]).ln() / (FREQUENCIES[i] / FREQUENCIES[i - 1]).ln();
            return g[i - 1] + t * (g[i] - g[i - 1]);
        }
    }
    g[6]
}
fn filter(g: &[f64; 7]) -> Vec<f64> {
    static FFT: OnceLock<Arc<dyn Fft<f64>>> = OnceLock::new();
    let fft = FFT.get_or_init(|| FftPlanner::new().plan_fft_inverse(4096));
    let mut spectrum = vec![Complex::new(0., 0.); 4096];
    for k in 0..=2048 {
        let f = k as f64 * 48000. / 4096.;
        let db = if f <= 500. || f >= 2000. {
            0.
        } else {
            interpolate(g, f)
        };
        let a = 10f64.powf(db / 20.);
        spectrum[k].re = a;
        if k > 0 && k < 2048 {
            spectrum[4096 - k].re = a;
        }
    }
    fft.process(&mut spectrum);
    let mut h: Vec<_> = (0..257)
        .map(|i| {
            let j = (i + 4096 - 128) % 4096;
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 256.).cos();
            spectrum[j].re / 4096. * window
        })
        .collect();
    let dc = h.iter().sum::<f64>();
    for x in &mut h {
        *x /= dc;
    }
    h
}
fn process(input: &[f32], h: &[f64]) -> Result<Vec<f32>, String> {
    let mut full = vec![0f64; input.len() + h.len() - 1];
    for (i, &x) in input.iter().enumerate() {
        if x != 0. {
            for (k, &g) in h.iter().enumerate() {
                full[i + k] += x as f64 * g;
            }
        }
    }
    if full[..128].iter().any(|v| v.abs() > 1e-12) {
        return Err(format!(
            "{PROFILE_MARKER}: unsupported early response; refusing to truncate leading samples"
        ));
    }
    Ok(full[128..].iter().map(|x| *x as f32).collect())
}
pub(crate) fn apply_bank(bank: &mut [StereoIr]) -> Result<(), String> {
    for ir in bank {
        let gains = curve(ir.azimuth, ir.elevation);
        let h = if gains.iter().all(|x| x.abs() < 1e-12) {
            let mut h = vec![0.; 257];
            h[128] = 1.;
            h
        } else {
            filter(&gains)
        };
        let n = ir.dry.len() / 2;
        let left = process(&ir.dry[..n], &h)?;
        let right = process(&ir.dry[n..], &h)?;
        ir.dry = left;
        ir.dry.extend(right);
        ir.wet = ir.dry.clone();
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_clarity_geometry_symmetry_front_and_bounds() {
        for az in -180i32..=180 {
            for el in [-90., -45., 0., 21., 45., 80., 90.] {
                let a = curve(az as f64, el);
                let b = curve(-(az as f64), el);
                for i in 0..7 {
                    assert!(a[i].is_finite() && a[i].abs() <= 3. + 1e-8);
                    assert!((a[i] - b[i]).abs() < 1e-8);
                    if az.abs() <= 45 {
                        assert_eq!(a[i], 0.);
                    }
                }
            }
        }
    }
    #[test]
    fn legacy_clarity_has_no_anchor_or_front_gate_jump() {
        for p in &recipe().points {
            let a = curve(p.azimuth - 0.00001, p.elevation);
            let b = curve(p.azimuth, p.elevation);
            let c = curve(p.azimuth + 0.00001, p.elevation);
            for i in 0..7 {
                assert!((a[i] - b[i]).abs() < 1e-4);
                assert!((c[i] - b[i]).abs() < 1e-4);
            }
        }
    }
    #[test]
    fn legacy_clarity_filter_is_common_symmetric_unity_dc_and_keeps_tail() {
        let h = filter(&[1., 1., 2., 3., 2., 1., 0.]);
        assert!((h.iter().sum::<f64>() - 1.).abs() < 1e-12);
        for i in 0..257 {
            assert!((h[i] - h[256 - i]).abs() < 1e-12);
        }
        let mut x = vec![0.; 512];
        x[490] = 1.;
        let y = process(&x, &h).unwrap();
        assert_eq!(y.len(), 640);
        assert!(y[512..].iter().any(|v| v.abs() > 1e-6));
        let z = process(&x.iter().map(|v| v * 0.5).collect::<Vec<_>>(), &h).unwrap();
        assert!(y.iter().zip(z).all(|(a, b)| (*a * 0.5 - b).abs() < 1e-7));
    }
    #[test]
    fn legacy_clarity_rejects_unsupported_early_response() {
        let mut x = vec![0.; 512];
        x[0] = 1.;
        assert!(process(&x, &filter(&[3.; 7])).is_err());
    }
}
