//! Pure VBAP geometry used by the browser renderer through wasm-bindgen.
//!
//! This module intentionally owns no audio graph, decoder state, clock, or
//! scheduling policy. It receives the renderer's active layout as ADM unit
//! vectors and returns normalized logical-layout gain rows.

use js_sys::{Float32Array, Float64Array, Uint8Array};
use wasm_bindgen::prelude::*;

const DET_EPSILON: f64 = 1e-9;
const COPLANAR_EPSILON: f64 = 1e-3;
const HULL_EPSILON: f64 = 1e-7;
const GAIN_EPSILON: f64 = 1e-4;

type Vec3 = [f64; 3];

#[derive(Clone)]
struct Pair {
    speakers: [usize; 2],
    inv2: [f64; 4],
}

#[derive(Clone)]
struct Triplet {
    speakers: [usize; 3],
    inv_basis: [[f64; 3]; 3],
}

fn unit(v: Vec3) -> Vec3 {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n == 0.0 { [0.0, 0.0, 0.0] } else { [v[0] / n, v[1] / n, v[2] / n] }
}

fn det3(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn inv3(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let d = det3(m);
    if d.abs() < DET_EPSILON { return None; }
    let [a, b, c] = m[0];
    let [e, f, g] = m[1];
    let [h, i, j] = m[2];
    Some([
        [(f * j - g * i) / d, -(b * j - c * i) / d, (b * g - c * f) / d],
        [-(e * j - g * h) / d, (a * j - c * h) / d, -(a * g - c * e) / d],
        [(e * i - f * h) / d, -(a * i - b * h) / d, (a * f - b * e) / d],
    ])
}

#[wasm_bindgen]
pub struct VbapSolver {
    dirs: Vec<Vec3>,
    lfe_mask: Vec<bool>,
    pairs: Vec<Pair>,
    triplets: Vec<Triplet>,
}

#[wasm_bindgen]
impl VbapSolver {
    /// `directions_adm` is packed `[x,y,z] × speakerCount`; directions may be
    /// non-unit and are normalized internally. `azimuths` preserves the caller's
    /// logical ordering for coplanar adjacent-pair construction.
    #[wasm_bindgen(constructor)]
    pub fn new(directions_adm: Float64Array, lfe_mask: Uint8Array, azimuths: Float64Array) -> Result<VbapSolver, JsValue> {
        VbapSolver::from_parts(&directions_adm.to_vec(), &lfe_mask.to_vec(), &azimuths.to_vec())
            .map_err(|message| JsValue::from_str(&message))
    }

    #[wasm_bindgen(getter)]
    pub fn speaker_count(&self) -> usize { self.dirs.len() }

    /// Packed input: positions `[x,y,z] × count`, one spread per position.
    /// Returns row-major normalized gain vectors: `[count][speakerCount]`.
    #[wasm_bindgen(js_name = panBatch)]
    pub fn pan_batch(&self, positions_adm: Float64Array, spreads: Float64Array) -> Result<Float32Array, JsValue> {
        let positions = positions_adm.to_vec();
        if positions.len() % 3 != 0 { return Err(JsValue::from_str("VBAP positions must be xyz triples")); }
        let count = positions.len() / 3;
        if spreads.length() as usize != count { return Err(JsValue::from_str("VBAP positions and spreads differ")); }
        let spreads = spreads.to_vec();
        let mut out = Vec::with_capacity(count * self.dirs.len());
        for (index, position) in positions.chunks_exact(3).enumerate() {
            let gains = self.pan([position[0], position[1], position[2]], spreads[index]);
            out.extend(gains.into_iter().map(|gain| gain as f32));
        }
        Ok(Float32Array::from(out.as_slice()))
    }
}

impl VbapSolver {
    fn from_parts(directions_adm: &[f64], lfe_mask: &[u8], azimuths: &[f64]) -> Result<VbapSolver, String> {
        if directions_adm.len() % 3 != 0 { return Err("VBAP directions must be xyz triples".into()); }
        let speaker_count = directions_adm.len() / 3;
        if lfe_mask.len() != speaker_count || azimuths.len() != speaker_count {
            return Err("VBAP layout array lengths differ".into());
        }
        let dirs = directions_adm.chunks_exact(3)
            .map(|v| unit([v[0], v[1], v[2]]))
            .collect();
        let lfe_mask = lfe_mask.iter().map(|value| *value != 0).collect();
        let mut solver = VbapSolver { dirs, lfe_mask, pairs: Vec::new(), triplets: Vec::new() };
        solver.precompute(azimuths);
        Ok(solver)
    }

    fn precompute(&mut self, azimuths: &[f64]) {
        let n = self.dirs.len();
        if self.dirs.iter().all(|d| d[2].abs() < COPLANAR_EPSILON) {
            let mut order: Vec<(f64, usize)> = azimuths.iter().copied().enumerate()
                .filter_map(|(i, az)| (!self.lfe_mask[i]).then_some((az, i)))
                .collect();
            order.sort_by(|a, b| a.0.total_cmp(&b.0));
            for index in 0..order.len() {
                let a = order[index].1;
                let b = order[(index + 1) % order.len()].1;
                let [ax, ay] = [self.dirs[a][0], self.dirs[a][1]];
                let [bx, by] = [self.dirs[b][0], self.dirs[b][1]];
                let det = ax * by - bx * ay;
                if det.abs() >= DET_EPSILON {
                    self.pairs.push(Pair { speakers: [a, b], inv2: [by / det, -bx / det, -ay / det, ax / det] });
                }
            }
            return;
        }
        for i in 0..n {
            if self.lfe_mask[i] { continue; }
            for j in (i + 1)..n {
                if self.lfe_mask[j] { continue; }
                for k in (j + 1)..n {
                    if self.lfe_mask[k] { continue; }
                    let a = self.dirs[i]; let b = self.dirs[j]; let c = self.dirs[k];
                    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                    let normal = [
                        ab[1] * ac[2] - ab[2] * ac[1],
                        ab[2] * ac[0] - ab[0] * ac[2],
                        ab[0] * ac[1] - ab[1] * ac[0],
                    ];
                    let plane = normal[0] * a[0] + normal[1] * a[1] + normal[2] * a[2];
                    if plane.abs() < DET_EPSILON { continue; }
                    let mut positive = false;
                    let mut negative = false;
                    for q in 0..n {
                        if q == i || q == j || q == k || self.lfe_mask[q] { continue; }
                        let d = self.dirs[q];
                        let side = normal[0] * d[0] + normal[1] * d[1] + normal[2] * d[2] - plane;
                        if side > HULL_EPSILON { positive = true; }
                        if side < -HULL_EPSILON { negative = true; }
                    }
                    // Retain only convex-hull faces, matching the renderer's
                    // 3D VBAP dome guard; the booleans are evaluated before
                    // the inverse basis is retained.
                    if positive && negative { continue; }
                    let basis = [[a[0], b[0], c[0]], [a[1], b[1], c[1]], [a[2], b[2], c[2]]];
                    if let Some(inv_basis) = inv3(basis) {
                        self.triplets.push(Triplet { speakers: [i, j, k], inv_basis });
                    }
                }
            }
        }
    }

    fn pan(&self, position: Vec3, spread: f64) -> Vec<f64> {
        let mut gains = vec![0.0; self.dirs.len()];
        let p = unit(position);
        if !self.pairs.is_empty() {
            let pn = (p[0] * p[0] + p[1] * p[1]).sqrt();
            let (px, py) = if pn == 0.0 { (0.0, 0.0) } else { (p[0] / pn, p[1] / pn) };
            let mut best: Option<([f64; 2], &Pair, f64)> = None;
            for pair in &self.pairs {
                let [a, b, c, d] = pair.inv2;
                let g = [a * px + b * py, c * px + d * py];
                let min_gain = g[0].min(g[1]);
                if min_gain >= -GAIN_EPSILON && best.as_ref().map_or(true, |(_, _, value)| min_gain > *value) {
                    best = Some((g, pair, min_gain));
                }
            }
            if let Some((g, pair, _)) = best {
                gains[pair.speakers[0]] = g[0].max(0.0);
                gains[pair.speakers[1]] = g[1].max(0.0);
            } else { self.nearest_into(&mut gains, [px, py, 0.0]); }
        } else {
            let mut best: Option<([f64; 3], &Triplet, f64)> = None;
            for triplet in &self.triplets {
                let m = triplet.inv_basis;
                let g = [
                    m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2],
                    m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2],
                    m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2],
                ];
                let min_gain = g[0].min(g[1]).min(g[2]);
                if min_gain >= -GAIN_EPSILON && best.as_ref().map_or(true, |(_, _, value)| min_gain > *value) {
                    best = Some((g, triplet, min_gain));
                }
            }
            if let Some((g, triplet, _)) = best {
                for index in 0..3 { gains[triplet.speakers[index]] = g[index].max(0.0); }
            } else { self.nearest_into(&mut gains, p); }
        }
        normalize(&mut gains);
        if spread > 0.0 { self.apply_spread(&mut gains, p, spread.min(1.0)); }
        gains
    }

    fn nearest_into(&self, gains: &mut [f64], point: Vec3) {
        let mut best_index = 0;
        let mut best_dot = f64::NEG_INFINITY;
        for i in 0..self.dirs.len() {
            if self.lfe_mask[i] { continue; }
            let d = self.dirs[i];
            let dot = d[0] * point[0] + d[1] * point[1] + d[2] * point[2];
            if dot > best_dot { best_dot = dot; best_index = i; }
        }
        if !gains.is_empty() { gains[best_index] = 1.0; }
    }

    fn apply_spread(&self, gains: &mut [f64], point: Vec3, spread: f64) {
        let mut nearest = [usize::MAX; 4];
        let mut dots = [f64::NEG_INFINITY; 4];
        for i in 0..self.dirs.len() {
            if self.lfe_mask[i] { continue; }
            let d = self.dirs[i];
            let dot = d[0] * point[0] + d[1] * point[1] + d[2] * point[2];
            for rank in 0..dots.len() {
                if dot <= dots[rank] { continue; }
                for move_index in (rank + 1..dots.len()).rev() {
                    dots[move_index] = dots[move_index - 1];
                    nearest[move_index] = nearest[move_index - 1];
                }
                dots[rank] = dot;
                nearest[rank] = i;
                break;
            }
        }
        let count = nearest.iter().filter(|index| **index != usize::MAX).count();
        let diffuse = 1.0 / (count.max(1) as f64).sqrt();
        for i in 0..gains.len() {
            let local = if nearest.contains(&i) { diffuse } else { 0.0 };
            gains[i] = (1.0 - spread) * gains[i] + spread * local;
        }
        normalize(gains);
    }
}

fn normalize(gains: &mut [f64]) {
    let power = gains.iter().map(|gain| gain * gain).sum::<f64>();
    let scale = if power > 0.0 { 1.0 / power.sqrt() } else { 0.0 };
    for gain in gains { *gain *= scale; }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solver(dirs: &[[f64; 3]], lfe: &[u8], azimuths: &[f64]) -> VbapSolver {
        let flat: Vec<f64> = dirs.iter().flat_map(|dir| dir).copied().collect();
        VbapSolver::from_parts(&flat, lfe, azimuths).unwrap()
    }

    #[test]
    fn horizontal_layout_excludes_lfe_and_normalizes() {
        let solver = solver(&[[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]], &[0, 0, 1], &[0.0, 90.0, 45.0]);
        let gains = solver.pan([0.0, 1.0, 0.0], 0.0);
        assert!(gains[0] > 0.999);
        assert_eq!(gains[2], 0.0);
        assert!((gains.iter().map(|gain| gain * gain).sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn batch_preserves_rows_and_spread_normalization() {
        let solver = solver(&[[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]], &[0, 0, 0], &[0.0, 90.0, -90.0]);
        let direct = solver.pan([0.0, 1.0, 0.0], 0.5);
        let other = solver.pan([-1.0, 0.0, 0.0], 0.0);
        assert!((direct.iter().map(|gain| gain * gain).sum::<f64>() - 1.0).abs() < 1e-12);
        assert!(other[1] > 0.999);
    }
}
