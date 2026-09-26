//! Delay-aligned, compact-support interpolation on measured HRTF directions.
use crate::{hrtf::StereoIr, spatial};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Direction {
    pub diffuse: f32,
    pub horizontal_only: bool,
    pub position: [f32; 3],
    pub head: Option<[f32; 4]>,
    pub width: f32,
    pub height: f32,
    pub depth: f32,
}

type Route = (
    Direction,
    crate::vbap::LayoutId,
    [f32; crate::vbap::MAX_BUS_COUNT],
    [f32; crate::vbap::MAX_BUS_COUNT],
);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct DiffuseFieldKey {
    horizontal_only: bool,
    head: Option<[u32; 4]>,
}

struct DiffuseField {
    left: Vec<f32>,
    right: Vec<f32>,
    reference_energy: f64,
}

// The continuous renderer receives authoritative ADM coordinates every 128
// samples, while its HRTF convolver runs in 1024-sample partitions. Rebuilding
// an interpolated KU100 filter for sub-degree updates spends a full extra FFT
// transition without reaching a distinct measured response. Keep the last
// filter until motion has accumulated one degree; explicit spatial properties
// and listener changes still retarget immediately.
const CONTINUOUS_DIRECTION_DEADBAND_COS: f32 = 0.999_847_7;
const CONTINUOUS_GAIN_DEADBAND: f32 = 0.01;

fn equivalent_direction(a: Direction, b: Direction) -> bool {
    if a.diffuse != b.diffuse
        || a.horizontal_only != b.horizontal_only
        || a.head != b.head
        || a.width != b.width
        || a.height != b.height
        || a.depth != b.depth
    {
        return false;
    }
    let length = |position: [f32; 3]| position.iter().map(|axis| axis * axis).sum::<f32>().sqrt();
    let (a_length, b_length) = (length(a.position), length(b.position));
    if a_length <= f32::EPSILON || b_length <= f32::EPSILON {
        return a.position == b.position;
    }
    let dot = a
        .position
        .iter()
        .zip(b.position)
        .map(|(a, b)| a * b)
        .sum::<f32>()
        / (a_length * b_length);
    dot >= CONTINUOUS_DIRECTION_DEADBAND_COS
}

fn equivalent_route(current: &Route, next: &Route) -> bool {
    current.1 == next.1
        && current.3 == next.3
        && current
            .2
            .iter()
            .zip(next.2)
            .all(|(current, next)| (current - next).abs() <= CONTINUOUS_GAIN_DEADBAND)
        && equivalent_direction(current.0, next.0)
}

#[derive(Clone, Copy)]
pub struct Frame {
    pub input: f32,
    pub near: [f32; 2],
    pub output: [f32; 2],
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            input: 0.0,
            near: [1.0; 2],
            output: [0.0; 2],
        }
    }
}
pub struct ContinuousSource {
    pub perf_id: String,
    hardware: crate::hardware::Chain,
    convolver: crate::convolution::StereoPartitionedConvolver,
    route: Option<Route>,
    pending: Option<Route>,
    near: crate::near_field::Filter,
    /// Per-ear occlusion amounts for the current block (1 = open), applied to
    /// the binaural output next to near-field.
    pub occlusion_targets: [f32; 2],
    occlusion: crate::occlusion::Shadow,
    // Sample-major hot storage: the mixer reads the previous output and writes
    // excitation/near targets together, avoiding five distant cache lines per object.
    pub frames: [Frame; crate::convolution::DEFAULT_PARTITION],
    input: [f32; crate::convolution::DEFAULT_PARTITION],
    left: [f32; crate::convolution::DEFAULT_PARTITION],
    right: [f32; crate::convolution::DEFAULT_PARTITION],
}
impl ContinuousSource {
    pub fn new(set: &crate::hrtf::NativeHrtfSet) -> Result<Self, String> {
        let zero = vec![0.0; set.directional_filter_len()];
        Ok(Self {
            perf_id: String::new(),
            hardware: crate::hardware::Chain::new(&set.cinema.monitor.hardware),
            convolver: crate::convolution::StereoPartitionedConvolver::new(
                &zero,
                &zero,
                crate::convolution::DEFAULT_PARTITION,
            )?,
            route: None,
            pending: None,
            near: Default::default(),
            occlusion_targets: [1.0; 2],
            occlusion: Default::default(),
            input: [0.0; crate::convolution::DEFAULT_PARTITION],
            frames: [Frame::default(); crate::convolution::DEFAULT_PARTITION],
            left: [0.0; crate::convolution::DEFAULT_PARTITION],
            right: [0.0; crate::convolution::DEFAULT_PARTITION],
        })
    }
    pub fn schedule(
        &mut self,
        direction: Direction,
        layout: crate::vbap::LayoutId,
        gains: [f32; crate::vbap::MAX_BUS_COUNT],
        amounts: [f32; crate::vbap::MAX_BUS_COUNT],
    ) {
        let route = (direction, layout, gains, amounts);
        let current = self.pending.as_ref().or(self.route.as_ref());
        self.pending = current
            .is_none_or(|current| !equivalent_route(current, &route))
            .then_some(route);
    }
    fn finish(&mut self, set: &crate::hrtf::NativeHrtfSet) -> Result<(), String> {
        let _perf = crate::performance::span(
            "hrtf.object.convolution",
            &self.perf_id,
            crate::convolution::DEFAULT_PARTITION as u64,
        );
        for (input, frame) in self.input.iter_mut().zip(&self.frames) {
            *input = self.hardware.process(frame.input);
        }
        if self.input.iter().any(|x| *x != 0.0) || !self.convolver.tail_is_silent() {
            if let Some((direction, layout, gains, amounts)) = self.pending.take() {
                let _filter_perf = crate::performance::span(
                    "hrtf.object.filter_update",
                    &self.perf_id,
                    (set.directional_filter_len() * 2) as u64,
                );
                let (left, right) =
                    set.directional_dry_compact(direction, layout, gains, amounts)?;
                if self.route.is_none() {
                    let filter = self.convolver.prepare_pair(&left, &right);
                    self.convolver.set_prepared_filter(filter);
                } else {
                    // Two valid HRTFs can have opposing narrow-band phase.
                    // Keep the click-free handoff short so motion does not
                    // dwell in their destructive output sum for a full block.
                    self.convolver.transition_to_pair(
                        &left,
                        &right,
                        crate::convolution::DIRECTIONAL_FILTER_TRANSITION_SAMPLES,
                    );
                }
                self.route = Some((direction, layout, gains, amounts));
            }
        }
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.convolver
            .process_block(&self.input, &mut self.left, &mut self.right)?;
        let occlusion_targets = self.occlusion_targets;
        for (i, frame) in self.frames.iter_mut().enumerate() {
            let output = self.near.process([self.left[i], self.right[i]], frame.near);
            let shaded = self.occlusion.process(output, occlusion_targets);
            frame.output = shaded;
            frame.input = 0.0;
            frame.near = [1.0; 2];
        }
        self.occlusion_targets = [1.0; 2];
        Ok(())
    }
}
pub fn finish_sources<'a>(
    sources: impl Iterator<Item = &'a mut ContinuousSource>,
    set: &crate::hrtf::NativeHrtfSet,
) -> Result<(), String> {
    use rayon::prelude::*;
    let mut sources: Vec<_> = sources.collect();
    let worker_count = crate::direct_renderer::worker_count_for_sources(sources.len());
    if let Some(pool) =
        crate::direct_renderer::workers().filter(|_| sources.len() >= 8 && worker_count > 1)
    {
        let chunk = sources.len().div_ceil(worker_count);
        pool.install(|| {
            sources.par_chunks_mut(chunk).try_for_each(|sources| {
                sources.iter_mut().try_for_each(|source| source.finish(set))
            })
        })
    } else {
        sources.iter_mut().try_for_each(|s| s.finish(set))
    }
}

pub struct Grid {
    directions: Vec<[f64; 3]>,
    arrivals: Vec<[usize; 2]>,
    /// KU100's measured pinna notches can cancel a moving tone when adjacent
    /// directions are added in the time domain. Complete subject sets keep
    /// the normal continuous interpolation path.
    ku100_notch_guard: bool,
    /// Canonical whole-waveform alignment offsets between neighbouring IR
    /// pairs, per ear. Each value aligns the higher index to the lower index.
    /// Peak-position alignment leaves the fine phase structure of the two
    /// measurements misaligned by several samples; mixing then cancels the
    /// common (correlated) part and collapses interaural coherence, which
    /// unfocuses binaural imaging between grid points. The offset stores the
    /// signed sample shift used to bring the higher-index neighbour onto the
    /// lower-index anchor timeline,
    /// computed lazily per pair.
    alignment_lags: std::sync::Mutex<std::collections::HashMap<(usize, usize), [f64; 2]>>,
    // The 12-point diffuse field is object-position independent. Its only
    // variable is listener orientation, so sharing it keeps dense ADM scenes
    // from regenerating the same measured field once per object and block.
    // Horizontal-only and spherical diffuse fields have different measured
    // directions. Keep one lock-free slot for each while static head pose is
    // active; the rendering hot path then only performs an atomic read.
    diffuse_static: [std::sync::OnceLock<std::sync::Arc<DiffuseField>>; 2],
    diffuse_fields:
        std::sync::Mutex<std::collections::HashMap<DiffuseFieldKey, std::sync::Arc<DiffuseField>>>,
}
impl Clone for Grid {
    fn clone(&self) -> Self {
        Self {
            directions: self.directions.clone(),
            arrivals: self.arrivals.clone(),
            ku100_notch_guard: self.ku100_notch_guard,
            alignment_lags: std::sync::Mutex::new(std::collections::HashMap::new()),
            diffuse_static: std::array::from_fn(|_| std::sync::OnceLock::new()),
            diffuse_fields: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}
impl std::fmt::Debug for Grid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grid")
            .field("directions", &self.directions.len())
            .field("ku100_notch_guard", &self.ku100_notch_guard)
            .field(
                "cached_lags",
                &self.alignment_lags.lock().map(|c| c.len()).unwrap_or(0),
            )
            .field(
                "cached_static_diffuse_fields",
                &self
                    .diffuse_static
                    .iter()
                    .filter(|field| field.get().is_some())
                    .count(),
            )
            .field(
                "cached_diffuse_fields",
                &self.diffuse_fields.lock().map(|c| c.len()).unwrap_or(0),
            )
            .finish()
    }
}

/// Best whole-waveform correlation index of `b` relative to `a` for one ear.
/// It is directly the signed sample offset used to align `b` to `a`.
///
/// The discrete correlation peak is refined with a bounded parabolic fit.
/// HRTF arrivals often fall between samples, and keeping that fractional part
/// avoids a small phase jump when a moving object crosses a measurement point.
fn waveform_alignment_lag(a: &[f32], b: &[f32], max_lag: isize) -> f64 {
    let energy_a: f64 = a.iter().map(|v| (*v as f64).powi(2)).sum();
    if energy_a <= 1e-20 {
        return 0.0;
    }
    let score = |lag: isize| -> f64 {
        // After applying `lag` to b, compare the overlapping region.
        // lag >= 0: b[t] aligns with a[t + lag]. lag < 0: b[t - lag] with a[t].
        let (a_start, b_start) = if lag >= 0 {
            (lag as usize, 0usize)
        } else {
            (0usize, (-lag) as usize)
        };
        let overlap = a
            .len()
            .saturating_sub(a_start)
            .min(b.len().saturating_sub(b_start));
        let mut correlation = 0.0_f64;
        let mut energy_b = 0.0_f64;
        for t in 0..overlap {
            let av = a[a_start + t] as f64;
            let bv = b[b_start + t] as f64;
            correlation += av * bv;
            energy_b += bv * bv;
        }
        if energy_b <= 1e-20 {
            f64::NEG_INFINITY
        } else {
            correlation / (energy_a * energy_b).sqrt()
        }
    };
    let mut best = (0_isize, f64::NEG_INFINITY);
    for lag in -max_lag..=max_lag {
        let normalized = score(lag);
        if normalized > best.1 {
            best = (lag, normalized);
        }
    }
    if best.1.is_finite() && best.0 > -max_lag && best.0 < max_lag {
        let left = score(best.0 - 1);
        let right = score(best.0 + 1);
        let denominator = left - 2.0 * best.1 + right;
        if denominator.abs() > 1e-12 && left.is_finite() && right.is_finite() {
            let offset = (0.5 * (left - right) / denominator).clamp(-0.5, 0.5);
            return best.0 as f64 + offset;
        }
    }
    best.0 as f64
}

impl Grid {
    /// Shift `neighbour` onto `anchor`'s timeline.
    ///
    /// The cache is canonical: it stores only the lower-index anchor to
    /// higher-index neighbour offset. When the dominant measurement changes
    /// while a source crosses a cell, the same pair is queried in reverse and
    /// must receive the inverse shift. Reusing the canonical shift unchanged
    /// turns an advance into a delay, producing a narrow-band cancellation.
    fn neighbour_shift(&self, irs: &[StereoIr], anchor: usize, neighbour: usize) -> [f64; 2] {
        if anchor == neighbour {
            return [0.0; 2];
        }
        let key = if anchor < neighbour {
            (anchor, neighbour)
        } else {
            (neighbour, anchor)
        };
        if let Ok(cached) = self.alignment_lags.lock() {
            if let Some(lags) = cached.get(&key) {
                return if anchor < neighbour {
                    *lags
                } else {
                    [-lags[0], -lags[1]]
                };
            }
        }
        let (first, second) = key;
        let mut lags = [0.0_f64; 2];
        for (ear, slot) in lags.iter_mut().enumerate() {
            let n = irs[first].dry.len() / 2;
            let a_slice = &irs[first].dry[ear * n..(ear + 1) * n];
            let b_slice = &irs[second].dry[ear * n..(ear + 1) * n];
            *slot = waveform_alignment_lag(a_slice, b_slice, 40);
        }
        if let Ok(mut cached) = self.alignment_lags.lock() {
            cached.insert(key, lags);
        }
        if anchor < neighbour {
            lags
        } else {
            [-lags[0], -lags[1]]
        }
    }
}
fn unit(az: f64, el: f64) -> [f64; 3] {
    let a = az.to_radians();
    let e = el.to_radians();
    [-a.sin() * e.cos(), a.cos() * e.cos(), e.sin()]
}
fn add_shifted(output: &mut [f32], input: &[f32], offset: isize, gain: f32) {
    if gain == 0.0 {
        return;
    }
    let src = (-offset).max(0) as usize;
    let dst = offset.max(0) as usize;
    if src >= input.len() || dst >= output.len() {
        return;
    }
    for (a, b) in output[dst..].iter_mut().zip(&input[src..]) {
        *a += b * gain;
    }
}
impl Grid {
    pub fn new(irs: &[StereoIr]) -> Self {
        Self::new_with_notch_guard(irs, false)
    }
    pub fn new_with_notch_guard(irs: &[StereoIr], ku100_notch_guard: bool) -> Self {
        Self {
            directions: irs
                .iter()
                .map(|ir| unit(ir.azimuth, ir.elevation))
                .collect(),
            arrivals: irs
                .iter()
                .map(|ir| {
                    let n = ir.dry.len() / 2;
                    std::array::from_fn(|ear| {
                        ir.dry[ear * n..(ear + 1) * n]
                            .iter()
                            .enumerate()
                            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                            .map_or(0, |x| x.0)
                    })
                })
                .collect(),
            ku100_notch_guard,
            alignment_lags: std::sync::Mutex::new(std::collections::HashMap::new()),
            diffuse_static: std::array::from_fn(|_| std::sync::OnceLock::new()),
            diffuse_fields: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn ku100_notch_guard_enabled(&self) -> bool {
        self.ku100_notch_guard
    }

    #[cfg(test)]
    pub(crate) fn diffuse_field_cache_len(&self) -> usize {
        self.diffuse_static
            .iter()
            .filter(|field| field.get().is_some())
            .count()
            + self.diffuse_fields.lock().map_or(0, |cache| cache.len())
    }

    fn build_diffuse_field(
        &self,
        irs: &[StereoIr],
        horizontal_only: bool,
        head: Option<[f32; 4]>,
    ) -> DiffuseField {
        let length = irs.iter().map(|ir| ir.dry.len() / 2).max().unwrap_or(0) + 4 + 127;
        let mut field = (vec![0.0; length], vec![0.0; length]);
        let mut reference_energy = 0.0;
        for i in 0..12 {
            let az = i as f64 * 137.507764;
            let el = if horizontal_only {
                0.0
            } else {
                (1.0 - 2.0 * (i as f64 + 0.5) / 12.0).asin().to_degrees()
            };
            let position = unit(az, el).map(|value| value as f32);
            let relative = spatial::adm_to_spherical(spatial::head_relative_adm(position, head));
            let pair = self.interpolate(irs, relative.azimuth as f64, relative.elevation as f64);
            reference_energy += pair
                .0
                .iter()
                .chain(&pair.1)
                .map(|value| (*value as f64).powi(2))
                .sum::<f64>()
                / 12.0;
            let delay = (i * 37 % 128) as isize;
            add_shifted(&mut field.0, &pair.0, delay, 1.0 / 12.0_f32.sqrt());
            add_shifted(&mut field.1, &pair.1, delay, 1.0 / 12.0_f32.sqrt());
        }
        let field_energy: f64 = field
            .0
            .iter()
            .chain(&field.1)
            .map(|value| (*value as f64).powi(2))
            .sum();
        let scale = if field_energy > 1e-20 {
            (reference_energy / field_energy).sqrt() as f32
        } else {
            0.0
        };
        for value in field.0.iter_mut().chain(&mut field.1) {
            *value *= scale;
        }
        DiffuseField {
            left: field.0,
            right: field.1,
            reference_energy,
        }
    }

    fn diffuse_field(
        &self,
        irs: &[StereoIr],
        horizontal_only: bool,
        head: Option<[f32; 4]>,
    ) -> std::sync::Arc<DiffuseField> {
        if head.is_none() {
            return self.diffuse_static[usize::from(horizontal_only)]
                .get_or_init(|| {
                    std::sync::Arc::new(self.build_diffuse_field(irs, horizontal_only, None))
                })
                .clone();
        }
        let key = DiffuseFieldKey {
            horizontal_only,
            head: head.map(|orientation| orientation.map(f32::to_bits)),
        };
        let mut cache = self
            .diffuse_fields
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(field) = cache.get(&key) {
            return field.clone();
        }
        // Head tracking may report every frame. Exact keys retain precision;
        // the small bound only limits retained historic poses.
        if cache.len() >= 8 {
            cache.clear();
        }
        let field = std::sync::Arc::new(self.build_diffuse_field(irs, horizontal_only, head));
        cache.insert(key, field.clone());
        field
    }

    fn weights(&self, az: f64, el: f64) -> Vec<(usize, f64)> {
        let u = unit(az, el);
        let mut distances: Vec<_> = self
            .directions
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    i,
                    (2.0 - 2.0 * p.iter().zip(u).map(|(a, b)| a * b).sum::<f64>())
                        .max(0.0)
                        .sqrt(),
                )
            })
            .collect();
        // Only the eighth neighbour defines support. Sorting the entire dense
        // measurement grid for every moving object wasted the render deadline.
        // Keep stable-sort tie order explicitly after the partial selection.
        let compare = |a: &(usize, f64), b: &(usize, f64)| {
            a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0))
        };
        if distances.iter().any(|x| x.1 < 1e-7) {
            distances.retain(|x| x.1 < 1e-7);
            distances.sort_by(compare);
            let count = distances.len();
            return distances
                .iter()
                .take(count)
                .map(|x| (x.0, 1.0 / count as f64))
                .collect();
        }
        // Include all tied neighbours. Weights vanish at the support boundary,
        // avoiding discontinuities when nearest-neighbour membership changes.
        let neighbour = (distances.len() - 1).min(7);
        let radius = distances.select_nth_unstable_by(neighbour, compare).1.1 * 1.05 + 1e-6;
        distances.retain(|x| x.1 < radius);
        distances.sort_by(compare);
        let mut weights: Vec<_> = distances
            .into_iter()
            .take_while(|x| x.1 < radius)
            .map(|(i, d)| {
                let t = d / radius;
                (i, (1.0 - t).powi(4) * (1.0 + 4.0 * t) / (d * d))
            })
            .collect();
        let sum: f64 = weights.iter().map(|x| x.1).sum();
        for w in &mut weights {
            w.1 /= sum;
        }
        weights
    }
    pub fn interpolate(&self, irs: &[StereoIr], az: f64, el: f64) -> (Vec<f32>, Vec<f32>) {
        let weights = self.weights(az, el);
        let n = irs.iter().map(|ir| ir.dry.len() / 2).max().unwrap_or(0);
        let mut output = [vec![0.0; n + 4], vec![0.0; n + 4]];
        // The dominant measurement anchors the output position (its ITD and
        // level pattern are physically intact); every other neighbour is
        // whole-waveform aligned to it per ear before mixing. Aligning to a
        // weighted average arrival instead leaves the fine phase structure
        // misaligned, and the mix then cancels the correlated part between
        // the ears, collapsing interaural coherence (unfocused, "wide"
        // imaging). Per-ear alignment keeps the frontal IACC at the measured
        // level; the mix interpolates each ear's fine structure toward the
        // dominant direction, which is the physically expected behaviour of
        // a source between two measurements.
        let dominant = weights
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |x| x.0);
        for ear in 0..2 {
            // Keep the fine waveform locked to the dominant measurement, but
            // move that temporary timeline to the weighted measured arrival.
            // Without this second step, the dominant index changes at a cell
            // boundary and the whole filter can jump by a few samples even
            // though the spatial weights are continuous. The bounded shift is
            // a common timeline adjustment for this ear; it does not change
            // the neighbour-to-dominant alignment or the interaural balance.
            let target_arrival: f64 = weights
                .iter()
                .map(|&(index, weight)| self.arrivals[index][ear] as f64 * weight)
                .sum();
            let timeline_shift =
                (target_arrival - self.arrivals[dominant][ear] as f64).clamp(-16.0, 16.0);
            for &(index, weight) in &weights {
                let len = irs[index].dry.len() / 2;
                let alignment_shift = if index == dominant {
                    0.0
                } else {
                    self.neighbour_shift(irs, dominant, index)[ear]
                };
                let shift = alignment_shift + timeline_shift;
                let base = shift.floor() as isize;
                let fraction = (shift - base as f64) as f32;
                let input = &irs[index].dry[ear * len..(ear + 1) * len];
                add_shifted(
                    &mut output[ear],
                    input,
                    base,
                    weight as f32 * (1.0 - fraction),
                );
                add_shifted(&mut output[ear], input, base + 1, weight as f32 * fraction);
            }
        }
        // Delay alignment prevents duplicated onsets, but interpolation of
        // different waveforms (including fractional shifts) loses energy. Use
        // the weighted measurement energy as a layout-independent reference.
        // Both ears receive the same scalar: preserve ITD and interaural level.
        let target: f64 = weights
            .iter()
            .map(|(i, w)| irs[*i].dry.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() * w)
            .sum();
        let actual: f64 = output.iter().flatten().map(|v| (*v as f64).powi(2)).sum();
        if target > 1e-20 {
            // A pinna notch can make two perfectly valid neighbouring HRIRs
            // cancel at one intermediate angle. Total-energy compensation
            // alone then raises the surviving tail and the direct image still
            // sounds as if it briefly disappears. Keep a small, adaptive
            // contribution from the dominant measured response only when the
            // coherent mix has collapsed; ordinary interpolation remains
            // untouched and does not allocate an anchor buffer.
            let ratio = actual / target;
            if weights.len() > 1 && ratio < if self.ku100_notch_guard { 0.90 } else { 0.72 } {
                // KU100 is the one dense set assembled from a binaural dummy
                // head rather than a complete individual subject. Its
                // adjacent pinna notches are valid measurements, but a time
                // domain sum can erase a narrow-band source between them.
                // Keep the dominant measured response intact in that case;
                // convolver filter transitions still smooth movement between
                // anchors. Other subjects retain the lighter fallback.
                let floor = if self.ku100_notch_guard {
                    1.0
                } else {
                    (1.0 - (ratio / 0.72).sqrt()).clamp(0.0, 1.0) as f32 * 0.32
                };
                if floor > 0.0 {
                    let mut anchor = [vec![0.0; n + 4], vec![0.0; n + 4]];
                    for ear in 0..2 {
                        let target_arrival: f64 = weights
                            .iter()
                            .map(|&(index, weight)| self.arrivals[index][ear] as f64 * weight)
                            .sum();
                        let shift = (target_arrival - self.arrivals[dominant][ear] as f64)
                            .clamp(-16.0, 16.0);
                        let base = shift.floor() as isize;
                        let fraction = (shift - base as f64) as f32;
                        let len = irs[dominant].dry.len() / 2;
                        let input = &irs[dominant].dry[ear * len..(ear + 1) * len];
                        add_shifted(&mut anchor[ear], input, base, 1.0 - fraction);
                        add_shifted(&mut anchor[ear], input, base + 1, fraction);
                    }
                    for ear in 0..2 {
                        for (mixed, stable) in output[ear].iter_mut().zip(&anchor[ear]) {
                            *mixed = *mixed * (1.0 - floor) + *stable * floor;
                        }
                    }
                }
            }
            let actual = output
                .iter()
                .flatten()
                .map(|v| (*v as f64).powi(2))
                .sum::<f64>();
            if actual > 1e-20 {
                let scale = (target / actual).sqrt() as f32;
                for ear in &mut output {
                    for v in ear {
                        *v *= scale;
                    }
                }
            }
        }
        let [left, right] = output;
        (left, right)
    }
    /// Object-local quadrature, independent of the virtual speaker layout.
    /// Staggered arrivals reduce coherent buildup between directions. Normalize
    /// the response energy, not the PCM, so musical dynamics remain untouched.
    pub fn footprint(&self, irs: &[StereoIr], direction: Direction) -> (Vec<f32>, Vec<f32>) {
        let mut direct = self.direct_footprint(irs, direction);
        let diffuse = direction.diffuse.clamp(0.0, 1.0);
        if diffuse == 0.0 {
            return direct;
        }
        let energy = |p: &(Vec<f32>, Vec<f32>)| -> f64 {
            p.0.iter().chain(&p.1).map(|v| (*v as f64).powi(2)).sum()
        };
        let field = self.diffuse_field(irs, direction.horizontal_only, direction.head);
        let target_energy =
            energy(&direct) * (1.0 - diffuse) as f64 + field.reference_energy * diffuse as f64;
        direct.0.resize(field.left.len(), 0.0);
        direct.1.resize(field.right.len(), 0.0);
        for (out, spread) in [(&mut direct.0, &field.left), (&mut direct.1, &field.right)] {
            for (a, b) in out.iter_mut().zip(spread) {
                *a = *a * (1.0 - diffuse).sqrt() + *b * diffuse.sqrt();
            }
        }
        let mixed_energy = energy(&direct);
        if mixed_energy > 1e-20 {
            let scale = (target_energy / mixed_energy).sqrt() as f32;
            for v in direct.0.iter_mut().chain(&mut direct.1) {
                *v *= scale;
            }
        }
        direct
    }

    fn direct_footprint(&self, irs: &[StereoIr], direction: Direction) -> (Vec<f32>, Vec<f32>) {
        let s = spatial::adm_to_spherical(direction.position);
        let at = |az: f64, el: f64| {
            let p = unit(az, el).map(|x| x as f32);
            let relative = spatial::adm_to_spherical(spatial::head_relative_adm(p, direction.head));
            self.interpolate(irs, relative.azimuth as f64, relative.elevation as f64)
        };
        let mut pair = at(s.azimuth as f64, s.elevation as f64);
        if direction.width == 0.0 && direction.height == 0.0 {
            return pair;
        }
        // Zero height/depth often makes several quadrature points identical.
        // Merge their weights before interpolating; retain the same footprint.
        let mut points = vec![(s.azimuth as f64, s.elevation as f64, 1.0_f32 / 3.0)];
        for (da, de, r) in [
            (-0.5, 0.0, 1.0),
            (0.5, 0.0, 1.0),
            (0.0, -0.5, 1.0),
            (0.0, 0.5, 1.0),
            (-0.5, 0.0, 1.0 - direction.depth * 0.5),
            (0.5, 0.0, 1.0 - direction.depth * 0.5),
            (-0.5, 0.0, 1.0 + direction.depth * 0.5),
            (0.5, 0.0, 1.0 + direction.depth * 0.5),
        ] {
            let az = (s.azimuth + da * direction.width / r) as f64;
            let el = (s.elevation + de * direction.height / r).clamp(-89.9, 89.9) as f64;
            if let Some(point) = points.iter_mut().find(|p| p.0 == az && p.1 == el) {
                point.2 += 1.0 / 12.0;
            } else {
                points.push((az, el, 1.0 / 12.0));
            }
        }
        for x in pair.0.iter_mut().chain(&mut pair.1) {
            *x *= points[0].2;
        }
        for &(az, el, weight) in &points[1..] {
            let next = at(az, el);
            for (out, input) in [(&mut pair.0, next.0), (&mut pair.1, next.1)] {
                for (a, b) in out.iter_mut().zip(input) {
                    *a += b * weight;
                }
            }
        }
        pair
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_neighbour_selection_matches_full_sort() {
    fn reference(grid: &Grid, az: f64, el: f64) -> Vec<(usize, f64)> {
        let u = unit(az, el);
        let mut distances: Vec<_> = grid
            .directions
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    i,
                    (2.0 - 2.0 * p.iter().zip(u).map(|(a, b)| a * b).sum::<f64>())
                        .max(0.0)
                        .sqrt(),
                )
            })
            .collect();
        distances.sort_by(|a,b|a.1.total_cmp(&b.1));
        if distances[0].1 < 1e-7 {
            let count=distances.iter().take_while(|x|x.1<1e-7).count();
            return distances.iter().take(count).map(|x|(x.0,1.0/count as f64)).collect();
        }
        // Include all tied neighbours. Weights vanish at the support boundary,
        // avoiding discontinuities when nearest-neighbour membership changes.
        let radius=distances[(distances.len()-1).min(7)].1*1.05+1e-6;
        let mut weights: Vec<_> = distances
            .into_iter()
            .take_while(|x| x.1 < radius)
            .map(|(i, d)| {
                let t = d / radius;
                (i, (1.0 - t).powi(4) * (1.0 + 4.0 * t) / (d * d))
            })
            .collect();
        let sum: f64 = weights.iter().map(|x| x.1).sum();
        for w in &mut weights {
            w.1 /= sum;
        }
        weights
    }
        for n in [1, 2, 7, 8, 9, 360, 1800] {
            let mut grid = Grid::new(&[]);
            grid.directions = (0..n).map(|i| unit((i % 120) as f64 * 3.0, (i / 120) as f64 * 10.0 - 60.0)).collect();
            for az in [-180.0, -90.0, 0.0, 0.001, 0.5, 45.0, 90.0, 179.9] {
                for el in [-90.0, -60.0, 0.0, 45.0, 90.0] {
                    assert_eq!(grid.weights(az, el), reference(&grid, az, el));
                }
            }
        }
        let mut grid = Grid::new(&[]);
        grid.directions = vec![unit(0.0, 0.0); 20];
        assert_eq!(grid.weights(0.0, 0.0), reference(&grid, 0.0, 0.0));
    }



    #[test]
    fn continuous_schedule_accumulates_subdegree_motion_but_keeps_explicit_changes() {
        let set = crate::hrtf::NativeHrtfSet::synthetic(12, 64, 0).unwrap();
        let mut source = ContinuousSource::new(&set).unwrap();
        let direction = |degrees: f32| Direction {
            position: [degrees.to_radians().sin(), degrees.to_radians().cos(), 0.0],
            head: None,
            diffuse: 0.0,
            horizontal_only: false,
            width: 0.0,
            height: 0.0,
            depth: 0.0,
        };
        let gains = [0.0; crate::vbap::MAX_BUS_COUNT];
        let amounts = [0.0; crate::vbap::MAX_BUS_COUNT];
        source.route = Some((
            direction(0.0),
            crate::vbap::LayoutId::Dolby7_1_4,
            gains,
            amounts,
        ));

        source.schedule(
            direction(0.7),
            crate::vbap::LayoutId::Dolby7_1_4,
            gains,
            amounts,
        );
        assert!(
            source.pending.is_none(),
            "sub-degree motion must retain the active filter"
        );

        source.schedule(
            direction(1.1),
            crate::vbap::LayoutId::Dolby7_1_4,
            gains,
            amounts,
        );
        assert!(
            source.pending.is_some(),
            "motion must accumulate against the applied filter"
        );

        let mut focused = amounts;
        focused[0] = 1.0;
        source.pending = None;
        source.schedule(
            direction(0.2),
            crate::vbap::LayoutId::Dolby7_1_4,
            gains,
            focused,
        );
        assert!(
            source.pending.is_some(),
            "speaker focus changes cannot be delayed by direction deadband"
        );

        source.pending = None;
        let mut muted = gains;
        muted[0] = 0.02;
        source.schedule(
            direction(0.2),
            crate::vbap::LayoutId::Dolby7_1_4,
            muted,
            amounts,
        );
        assert!(
            source.pending.is_some(),
            "audible speaker gain changes cannot be delayed by direction deadband"
        );
    }

    #[test]
    #[ignore = "offline diagnostic using the installed KU100 and H13 dense assets"]
    fn diagnose_dense_hrtf_motion_transition() {
        let asset_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../desktop/native-renderer/hrtf-assets");
        let positions = [
            -135.0_f32, -120.0, -105.0, -90.0, -75.0, -60.0, -45.0, -30.0, -15.0, 0.0,
        ];
        let frequencies = [500.0_f32, 1_000.0, 2_000.0, 4_000.0, 8_000.0];
        let to_direction = |azimuth: f32| Direction {
            // ADM x is right, and adm_to_spherical maps it to negative azimuth.
            position: [-azimuth.to_radians().sin(), azimuth.to_radians().cos(), 0.0],
            head: None,
            diffuse: 0.0,
            horizontal_only: true,
            width: 0.0,
            height: 0.0,
            depth: 0.0,
        };
        let gains = std::array::from_fn(|index| if index == 0 { 1.0 } else { 0.0 });
        let silence = [0.0; crate::vbap::MAX_BUS_COUNT];
        let rms = |samples: &[f32]| {
            (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32)
                .sqrt()
        };

        let mut datasets = vec![("KU100", "hrtf-dense"), ("H13", "hrtf-h13-dense")];
        if asset_root.join("hrtf-ku100-clarity-experimental").exists() {
            datasets.push((
                "KU100-clarity-experiment",
                "hrtf-ku100-clarity-experimental",
            ));
        }
        for (name, directory) in datasets {
            let set = crate::hrtf::NativeHrtfSet::load_calibrated(
                &asset_root.join(directory).join("hrtf-set.json"),
            )
            .unwrap();
            for frequency in frequencies {
                for duration in [1024, 512, 256, 128] {
                    let zero = vec![0.0; set.directional_filter_len()];
                    let mut convolver = crate::convolution::StereoPartitionedConvolver::new(
                        &zero,
                        &zero,
                        crate::convolution::DEFAULT_PARTITION,
                    )
                    .unwrap();
                    let mut phase = 0.0_f32;
                    let step = std::f32::consts::TAU * frequency / 48_000.0;
                    let mut render = |azimuth: f32| {
                        let (left, right) = set
                            .directional_dry_compact(
                                to_direction(azimuth),
                                crate::vbap::LayoutId::Dolby7_1_4,
                                gains,
                                silence,
                            )
                            .unwrap();
                        let filter = convolver.prepare_pair(&left, &right);
                        convolver.transition_to(filter, duration);
                        let input: Vec<_> = (0..1024)
                            .map(|_| {
                                let value = phase.sin() * 0.1;
                                phase += step;
                                value
                            })
                            .collect();
                        let mut left = vec![0.0; 1024];
                        let mut right = vec![0.0; 1024];
                        convolver
                            .process_block(&input, &mut left, &mut right)
                            .unwrap();
                        (rms(&left), rms(&right))
                    };
                    // Establish the first target and clear its convolution latency.
                    render(positions[0]);
                    render(positions[0]);
                    let mut worst_ratio = 1.0_f32;
                    let mut worst_at = positions[0];
                    for &position in &positions[1..] {
                        let transition = render(position);
                        let stable = render(position);
                        let ratio = ((transition.0 * transition.0 + transition.1 * transition.1)
                            / (stable.0 * stable.0 + stable.1 * stable.1).max(1e-12))
                        .sqrt();
                        if ratio < worst_ratio {
                            worst_ratio = ratio;
                            worst_at = position;
                        }
                    }
                    eprintln!(
                        "{name} {frequency:.0} Hz, {duration} samples: worst transition/stable ratio {worst_ratio:.3} at {worst_at:.0} degrees"
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "offline diagnostic of the KU100 direct object path against its shared room residual"]
    fn diagnose_ku100_direct_and_reflection_transfer() {
        let asset_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../desktop/native-renderer/hrtf-assets");
        let set = crate::hrtf::NativeHrtfSet::load_calibrated(
            &asset_root.join("hrtf-dense").join("hrtf-set.json"),
        )
        .unwrap();
        let layout = crate::vbap::LayoutId::Dolby7_1_4;
        let solver = crate::vbap::VbapSolver::with_layout(layout);
        let amounts = [0.0; crate::vbap::MAX_BUS_COUNT];
        let frequencies = [
            50.0_f32, 80.0, 120.0, 160.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0,
        ];
        let directions = [(-90.0_f32, 45.0_f32), (-90.0, 0.0), (0.0, 45.0)];
        let response = |ir: &[f32], frequency: f32| {
            let mut real = 0.0_f64;
            let mut imaginary = 0.0_f64;
            let step = std::f64::consts::TAU * frequency as f64 / 48_000.0;
            for (index, sample) in ir.iter().enumerate() {
                let phase = step * index as f64;
                real += *sample as f64 * phase.cos();
                imaginary -= *sample as f64 * phase.sin();
            }
            (real, imaginary)
        };
        let magnitude = |pair: [(f64, f64); 2]| {
            (pair
                .iter()
                .map(|(real, imaginary)| real * real + imaginary * imaginary)
                .sum::<f64>())
            .sqrt()
        };
        let direction = |azimuth: f32, elevation: f32| Direction {
            // ADM x is right, and adm_to_spherical maps it to negative azimuth.
            position: [
                -elevation.to_radians().cos() * azimuth.to_radians().sin(),
                elevation.to_radians().cos() * azimuth.to_radians().cos(),
                elevation.to_radians().sin(),
            ],
            head: None,
            diffuse: 0.0,
            horizontal_only: false,
            width: 0.0,
            height: 0.0,
            depth: 0.0,
        };

        for (azimuth, elevation) in directions {
            let direction = direction(azimuth, elevation);
            let gains = solver.pan(direction.position, 0.0);
            let direct = set
                .directional_dry_compact(direction, layout, gains, amounts)
                .unwrap();
            let mut residual = (
                vec![0.0_f32; set.speaker_filter_len()],
                vec![0.0_f32; set.speaker_filter_len()],
            );
            for (bus, speaker) in crate::vbap::speakers(layout).iter().enumerate() {
                if gains[bus] == 0.0 {
                    continue;
                }
                let (wet_left, wet_right) = set
                    .mixed_speaker(
                        speaker.name,
                        layout.as_str(),
                        speaker.azimuth as f64,
                        speaker.elevation as f64,
                        0.04,
                    )
                    .unwrap();
                let (dry_left, dry_right) = set
                    .mixed_speaker(
                        speaker.name,
                        layout.as_str(),
                        speaker.azimuth as f64,
                        speaker.elevation as f64,
                        0.0,
                    )
                    .unwrap();
                for ((out, wet), dry) in residual.0.iter_mut().zip(wet_left).zip(dry_left) {
                    *out += (wet - dry) * gains[bus];
                }
                for ((out, wet), dry) in residual.1.iter_mut().zip(wet_right).zip(dry_right) {
                    *out += (wet - dry) * gains[bus];
                }
            }
            for frequency in frequencies {
                let direct_response = [
                    response(&direct.0, frequency),
                    response(&direct.1, frequency),
                ];
                let reflection_response = [
                    response(&residual.0, frequency),
                    response(&residual.1, frequency),
                ];
                let summed_response = std::array::from_fn(|ear| {
                    (
                        direct_response[ear].0 + reflection_response[ear].0,
                        direct_response[ear].1 + reflection_response[ear].1,
                    )
                });
                let direct_magnitude = magnitude(direct_response).max(1e-12);
                let residual_db =
                    20.0 * (magnitude(reflection_response) / direct_magnitude).log10();
                let summed_db = 20.0 * (magnitude(summed_response) / direct_magnitude).log10();
                eprintln!(
                    "KU100 reflection diagnostic az={azimuth:>6.1} el={elevation:>5.1} freq={frequency:>7.0}Hz residual={residual_db:>6.2}dB summed/direct={summed_db:>6.2}dB"
                );
            }
        }
    }

    #[test]
    #[ignore = "offline diagnostic of the KU100 and H13 direct-HRTF rear motion response"]
    fn diagnose_rear_motion_direct_hrtf_response() {
        let asset_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../desktop/native-renderer/hrtf-assets");
        let layout = crate::vbap::LayoutId::Dolby7_1_4;
        let solver = crate::vbap::VbapSolver::with_layout(layout);
        let amounts = [0.0; crate::vbap::MAX_BUS_COUNT];
        // The affected program's moving rear synthesizer is concentrated near
        // 1.1 and 1.4 kHz, so retain those bands instead of inferring its
        // behaviour from octave-spaced probe tones alone.
        let frequencies = [
            250.0_f32, 500.0, 1_000.0, 1_100.0, 1_400.0, 2_000.0, 3_000.0, 4_000.0, 6_000.0,
            8_000.0,
        ];
        let azimuths: Vec<_> = (135..=225)
            .map(|angle| {
                if angle > 180 {
                    angle as f32 - 360.0
                } else {
                    angle as f32
                }
            })
            .collect();
        let response_magnitude = |ir: &[f32], frequency: f32| {
            let mut real = 0.0_f64;
            let mut imaginary = 0.0_f64;
            let step = std::f64::consts::TAU * frequency as f64 / 48_000.0;
            for (index, sample) in ir.iter().enumerate() {
                let phase = step * index as f64;
                real += *sample as f64 * phase.cos();
                imaginary -= *sample as f64 * phase.sin();
            }
            real * real + imaginary * imaginary
        };
        let direction = |azimuth: f32| Direction {
            position: [-azimuth.to_radians().sin(), azimuth.to_radians().cos(), 0.0],
            head: None,
            diffuse: 0.0,
            horizontal_only: true,
            width: 0.0,
            height: 0.0,
            depth: 0.0,
        };

        let mut datasets = vec![("KU100", "hrtf-dense"), ("H13", "hrtf-h13-dense")];
        if asset_root.join("hrtf-ku100-clarity-experimental").exists() {
            datasets.push((
                "KU100-clarity-experiment",
                "hrtf-ku100-clarity-experimental",
            ));
        }
        for (name, directory) in datasets {
            let set = crate::hrtf::NativeHrtfSet::load_calibrated(
                &asset_root.join(directory).join("hrtf-set.json"),
            )
            .unwrap();
            for frequency in frequencies {
                let mut levels = Vec::new();
                for &azimuth in &azimuths {
                    let direction = direction(azimuth);
                    let gains = solver.pan(direction.position, 0.0);
                    let pair = set
                        .directional_dry_compact(direction, layout, gains, amounts)
                        .unwrap();
                    let magnitude = (response_magnitude(&pair.0, frequency)
                        + response_magnitude(&pair.1, frequency))
                    .sqrt();
                    levels.push(20.0 * magnitude.max(1e-12).log10());
                }
                let (min_index, min) = levels
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.total_cmp(b.1))
                    .unwrap();
                let (max_index, max) = levels
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .unwrap();
                let right_rear = levels[0];
                let left_rear = levels[azimuths.len() - 1];
                let center = levels[45];
                let adjacent = (levels[44] + levels[46]) * 0.5;
                eprintln!(
                    "{name} rear direct diagnostic freq={frequency:>7.0}Hz range={:.2}dB min={:.2}dB@{:>6.1} max={:.2}dB@{:>6.1} right-135={right_rear:.2}dB left-135={left_rear:.2}dB center-vs-neighbours={:.2}dB",
                    *max - *min,
                    *min,
                    azimuths[min_index],
                    *max,
                    azimuths[max_index],
                    center - adjacent,
                );
            }
            // Compare the exact halfway filters used by the moving rear
            // objects against their two measured anchors. A broad energy
            // check can miss this kind of narrow-band cancellation.
            for (azimuth, anchors) in [(-135.0_f32, [-140.0, -130.0]), (135.0, [140.0, 130.0])] {
                let target = direction(azimuth);
                let gains = solver.pan(target.position, 0.0);
                let pair = set
                    .directional_dry_compact(target, layout, gains, amounts)
                    .unwrap();
                for frequency in [1_100.0_f32, 1_400.0] {
                    let current = (response_magnitude(&pair.0, frequency)
                        + response_magnitude(&pair.1, frequency))
                    .sqrt();
                    let reference = anchors
                        .into_iter()
                        .map(|anchor| {
                            let measured = set.nearest(anchor as f64, 0.0).unwrap();
                            (response_magnitude(&measured.dry[..measured.dry.len() / 2], frequency)
                                + response_magnitude(
                                    &measured.dry[measured.dry.len() / 2..],
                                    frequency,
                                ))
                            .sqrt()
                        })
                        .sum::<f64>()
                        / anchors.len() as f64;
                    eprintln!(
                        "{name} rear interpolation az={azimuth:>6.1} freq={frequency:>7.0}Hz midpoint-vs-anchor={:.2}dB",
                        20.0 * (current / reference.max(1e-12)).log10()
                    );
                }
            }
        }
    }

    #[test]
    fn interpolation_preserves_measured_energy_between_directions() {
        let mut a = vec![0.0; 64];
        let mut b = a.clone();
        a[4] = 1.0;
        a[5] = 0.5;
        a[36] = 0.7;
        a[37] = 0.35;
        b[5] = 1.0;
        b[6] = -0.5;
        b[37] = 0.7;
        b[38] = -0.35;
        let irs = vec![
            StereoIr {
                azimuth: -30.0,
                elevation: 0.0,
                dry: a,
                wet: vec![],
            },
            StereoIr {
                azimuth: 30.0,
                elevation: 0.0,
                dry: b,
                wet: vec![],
            },
        ];
        let grid = Grid::new(&irs);
        let expected: f64 = irs[0].dry.iter().map(|v| (*v as f64).powi(2)).sum();
        for az in [-30.0, -15.0, 0.0, 15.0, 30.0] {
            let (l, r) = grid.interpolate(&irs, az, 0.0);
            let energy: f64 = l.iter().chain(&r).map(|v| (*v as f64).powi(2)).sum();
            assert!((energy / expected - 1.0).abs() < 1e-6, "energy dip at {az}");
            assert!(
                l.iter().zip(&r).all(|(l, r)| (r - l * 0.7).abs() < 1e-6),
                "interaural balance changed"
            );
        }
    }
    #[test]
    fn interpolation_keeps_a_direct_anchor_when_neighbours_cancel() {
        let mut left = vec![0.0; 64];
        for (i, sample) in left[12..52].iter_mut().enumerate() {
            *sample = ((i as f32) * 0.37).sin() * 0.8;
        }
        let right = left.iter().map(|sample| -*sample).collect::<Vec<_>>();
        let irs = vec![
            StereoIr {
                azimuth: -30.0,
                elevation: 0.0,
                dry: left.clone(),
                wet: vec![],
            },
            StereoIr {
                azimuth: 30.0,
                elevation: 0.0,
                dry: right,
                wet: vec![],
            },
        ];
        let grid = Grid::new(&irs);
        let (out_left, out_right) = grid.interpolate(&irs, 0.0, 0.0);
        let target: f64 = irs[0].dry.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() * 2.0;
        let actual: f64 = out_left
            .iter()
            .chain(&out_right)
            .map(|v| (*v as f64).powi(2))
            .sum();
        assert!(
            actual > target * 0.45,
            "direct anchor was lost during cancellation: {actual} / {target}"
        );
        assert!(out_left.iter().chain(&out_right).all(|v| v.is_finite()));
    }

    #[test]
    fn ku100_notch_guard_preserves_the_dominant_measurement_after_full_cancellation() {
        let mut waveform = vec![0.0; 64];
        for (index, sample) in waveform[12..52].iter_mut().enumerate() {
            *sample = ((index as f32) * 0.37).sin() * 0.8;
        }
        let mut anchor = waveform.clone();
        anchor.extend_from_slice(&waveform);
        let mut cancelled = waveform.iter().map(|sample| -*sample).collect::<Vec<_>>();
        cancelled.extend(waveform.iter().map(|sample| -*sample));
        let irs = vec![
            StereoIr {
                azimuth: -30.0,
                elevation: 0.0,
                dry: anchor.clone(),
                wet: vec![],
            },
            StereoIr {
                azimuth: 30.0,
                elevation: 0.0,
                dry: cancelled,
                wet: vec![],
            },
        ];
        let grid = Grid::new_with_notch_guard(&irs, true);
        let (left, right) = grid.interpolate(&irs, 0.0, 0.0);
        let expected_energy: f64 = waveform.iter().map(|sample| (*sample as f64).powi(2)).sum();
        let left_energy: f64 = left.iter().map(|sample| (*sample as f64).powi(2)).sum();
        let right_energy: f64 = right.iter().map(|sample| (*sample as f64).powi(2)).sum();
        assert!(
            (left_energy / expected_energy - 1.0).abs() < 1e-6,
            "expected={expected_energy} left={left_energy} right={right_energy}"
        );
        assert!((right_energy / expected_energy - 1.0).abs() < 1e-6);
        assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
    }
    #[test]
    fn waveform_alignment_refines_subsample_arrival() {
        let sample = |position: f64| {
            if !(0.0..127.0).contains(&position) {
                return 0.0;
            }
            let left = position.floor() as usize;
            let fraction = position - left as f64;
            let shape = |index: usize| {
                let x = index as f64;
                (-((x - 80.0) / 8.0).powi(2)).exp() + 0.3 * (-((x - 119.0) / 3.5).powi(2)).exp()
            };
            (shape(left) * (1.0 - fraction) + shape(left + 1) * fraction) as f32
        };
        let a: Vec<_> = (0..128).map(|i| sample(i as f64)).collect();
        let b: Vec<_> = (0..128).map(|i| sample(i as f64 - 2.35)).collect();
        let lag = waveform_alignment_lag(&a, &b, 20);
        assert!(
            (lag + 2.35).abs() < 0.08,
            "expected fractional lag near -2.35, got {lag}"
        );
    }
    #[test]
    fn cached_neighbour_shift_reverses_when_the_anchor_changes() {
        let make = |left: usize, right: usize, azimuth: f64| {
            let mut dry = vec![0.0; 128];
            dry[left] = 1.0;
            dry[left + 1] = 0.4;
            dry[64 + right] = 1.0;
            dry[64 + right + 1] = 0.4;
            StereoIr {
                azimuth,
                elevation: 0.0,
                dry,
                wet: vec![],
            }
        };
        let irs = vec![make(20, 22, -30.0), make(25, 28, 30.0)];
        let grid = Grid::new(&irs);
        // This first call populates the canonical (0, 1) cache entry.
        let forward = grid.neighbour_shift(&irs, 0, 1);
        let reverse = grid.neighbour_shift(&irs, 1, 0);
        assert!((forward[0] + 5.0).abs() < 0.1 && (forward[1] + 6.0).abs() < 0.1);
        assert!(
            (reverse[0] - 5.0).abs() < 0.1 && (reverse[1] - 6.0).abs() < 0.1,
            "reverse cache lookup must invert the time alignment: {reverse:?}"
        );
    }
    #[test]
    fn interpolation_keeps_the_timeline_continuous_when_dominant_changes() {
        let make = |left: usize, right: usize| {
            let mut dry = vec![0.0; 256];
            for (offset, amplitude) in [(0, 1.0_f32), (1, 0.45), (2, 0.2), (3, 0.08)] {
                dry[left + offset] = amplitude;
                dry[64 + right + offset] = amplitude;
            }
            StereoIr {
                azimuth: 0.0,
                elevation: 0.0,
                dry: dry.clone(),
                wet: dry,
            }
        };
        let mut first = make(32, 36);
        let mut second = make(44, 48);
        first.azimuth = -30.0;
        second.azimuth = 30.0;
        let grid = Grid::new(&[first, second]);
        let left = grid.interpolate(&[make_at(-30.0, 32, 36), make_at(30.0, 44, 48)], -0.01, 0.0);
        let right = grid.interpolate(&[make_at(-30.0, 32, 36), make_at(30.0, 44, 48)], 0.01, 0.0);
        let peak = |p: &[f32]| {
            p.iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|x| x.0)
                .unwrap()
        };
        let (left_peak, right_peak) = (peak(&left.0), peak(&right.0));
        assert!(
            left_peak.abs_diff(right_peak) <= 1,
            "dominant-anchor switch moved the direct arrival: peaks {left_peak} and {right_peak}"
        );

        fn make_at(azimuth: f64, left: usize, right: usize) -> StereoIr {
            let mut dry = vec![0.0; 256];
            for (offset, amplitude) in [(0, 1.0_f32), (1, 0.45), (2, 0.2), (3, 0.08)] {
                dry[left + offset] = amplitude;
                dry[64 + right + offset] = amplitude;
            }
            StereoIr {
                azimuth,
                elevation: 0.0,
                dry: dry.clone(),
                wet: dry,
            }
        }
    }
    #[test]
    fn hardware_objects_keep_direction_and_match_parallel_mixing() {
        let render = |hardware: bool, fast: bool, position: [f32; 3]| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../web/public/hrtf/hrtf-set.json");
            let mut e = crate::Engine::new(48000, 2);
            e.cinema.monitor.hardware.enabled = hardware;
            e.cinema.monitor.hardware.rail_v = 1.0;
            e.directional_hrtf = true;
            e.disable_fast_objects = !fast;
            e.replace_hrtf(
                crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),
                0.0,
            )
            .unwrap();
            e.direct_mix = 1.0;
            e.paused = false;
            e.output_active = true;
            for id in 0..8 {
                let mut source = crate::Source {
                    kind: crate::SourceKind::Object,
                    position,
                    diffuse: if id == 0 { 0.25 } else { 0.0 },
                    gain: 1.0,
                    target_gain: 1.0,
                    availability: 1.0,
                    availability_target: 1.0,
                    ..Default::default()
                };
                let pcm: Vec<_> = (0..16384)
                    .map(|i| 0.08 * (i as f32 * 0.13 + id as f32).sin())
                    .collect();
                source.samples.write(0, 0, &pcm);
                let key = format!("obj:{id}");
                e.sources.insert(key.clone(), source);
                e.route_source_now(&key, 0).unwrap();
            }
            let mut out = vec![0.0; 32768];
            e.render_into(&mut out, 2);
            assert!(
                e.sources
                    .values()
                    .all(|s| s.continuous_active && s.continuous.is_some() && s.direct.is_none())
            );
            if fast {
                assert!(e.fast_object_blocks > 0);
            }
            assert!(out.iter().all(|v| v.is_finite()));
            if hardware && fast {
                for source in e.sources.values_mut() {
                    source.zone_exclusion = vec![crate::adm_zone::Zone::Polar {
                        min: [-180.0, 80.0],
                        max: [180.0, 90.0],
                    }]
                    .into();
                    let pcm: Vec<_> = (0..32768).map(|i| 0.01 * (i as f32 * 0.13).sin()).collect();
                    source.samples.write(16384, 16384, &pcm);
                }
                let mut tail = vec![0.0; 65536];
                e.render_into(&mut tail, 2);
                assert!(
                    tail[49152..].iter().any(|v| v.abs() > 1e-5),
                    "hardware exclusion fallback lost audio"
                );
                assert!(
                    e.sources
                        .values()
                        .all(|s| !s.continuous_active && s.direct.is_none())
                );
            }
            out
        };
        let left = render(true, true, [-0.8, 0.5, 0.0]);
        let slow = render(true, false, [-0.8, 0.5, 0.0]);
        assert!(left.iter().zip(&slow).all(|(a, b)| (a - b).abs() < 2e-6));
        let right = render(true, true, [0.8, 0.5, 0.0]);
        let dry = render(false, true, [-0.8, 0.5, 0.0]);
        let energy = |v: &[f32]| v[8192..].iter().map(|v| v * v).sum::<f32>();
        assert!(energy(&left) > 1e-8);
        assert!(
            energy(&left) < energy(&dry) * 0.5,
            "hardware must affect object PCM"
        );
        let delta: f32 = left.iter().zip(&right).map(|(a, b)| (a - b).abs()).sum();
        assert!(delta > 0.01, "hardware must not disable spatial direction");
    }
    #[test]
    fn diffuse_response_preserves_energy_and_partial_sources_keep_direction() {
        let irs: Vec<_> = (0..12)
            .map(|i| {
                let mut dry = vec![0.0; 64];
                dry[4 + i % 4] = 1.0;
                dry[32 + 7 - i % 4] = 1.0;
                StereoIr {
                    azimuth: i as f64 * 30.0,
                    elevation: 0.0,
                    dry,
                    wet: vec![],
                }
            })
            .collect();
        let grid = Grid::new(&irs);
        let base = Direction {
            position: [0.0, 1.0, 0.0],
            head: None,
            width: 0.0,
            height: 0.0,
            depth: 0.0,
            diffuse: 0.0,
            horizontal_only: true,
        };
        let energy = |p: &(Vec<f32>, Vec<f32>)| p.0.iter().chain(&p.1).map(|v| v * v).sum::<f32>();
        for diffuse in [0.25, 0.5, 1.0] {
            let a = grid.footprint(&irs, Direction { diffuse, ..base });
            let b = grid.footprint(
                &irs,
                Direction {
                    diffuse,
                    position: [1.0, 0.0, 0.0],
                    ..base
                },
            );
            assert!(energy(&a) > 0.5 && energy(&a) < 2.01);
            if diffuse < 1.0 {
                assert_ne!(a, b);
            } else {
                assert_eq!(a, b, "fully diffuse field has no authored point direction");
            }
        }
    }

    #[test]
    fn cached_diffuse_field_matches_an_independent_full_calculation() {
        let irs: Vec<_> = (0..12)
            .map(|index| {
                let mut dry = vec![0.0; 256];
                for sample in 0..128 {
                    dry[sample] = ((sample as f32 + index as f32 * 3.0) * 0.19).sin() * 0.04;
                    dry[128 + sample] = ((sample as f32 + index as f32 * 5.0) * 0.23).cos() * 0.03;
                }
                StereoIr {
                    azimuth: index as f64 * 30.0,
                    elevation: 0.0,
                    dry,
                    wet: vec![],
                }
            })
            .collect();
        let direction = |position| Direction {
            position,
            head: None,
            diffuse: 0.35,
            horizontal_only: false,
            width: 0.2,
            height: 0.0,
            depth: 0.0,
        };
        let cached = Grid::new(&irs);
        let isolated = Grid::new(&irs);
        cached.footprint(&irs, direction([0.0, 1.0, 0.2]));
        let from_cache = cached.footprint(&irs, direction([0.7, 0.5, 0.3]));
        let independent = isolated.footprint(&irs, direction([0.7, 0.5, 0.3]));
        assert_eq!(cached.diffuse_field_cache_len(), 1);
        for (cached, independent) in from_cache
            .0
            .iter()
            .chain(&from_cache.1)
            .zip(independent.0.iter().chain(&independent.1))
        {
            assert!((cached - independent).abs() < 1e-6);
        }

        // A static spherical field must not be reused for a horizontal-only
        // source merely because neither source has head tracking enabled.
        let horizontal = Direction {
            horizontal_only: true,
            ..direction([0.4, -0.6, 0.1])
        };
        let from_second_cache = cached.footprint(&irs, horizontal);
        let second_independent = Grid::new(&irs).footprint(&irs, horizontal);
        assert_eq!(cached.diffuse_field_cache_len(), 2);
        for (cached, independent) in from_second_cache
            .0
            .iter()
            .chain(&from_second_cache.1)
            .zip(second_independent.0.iter().chain(&second_independent.1))
        {
            assert!((cached - independent).abs() < 1e-6);
        }
    }
    #[test]
    #[ignore = "offline full engine realtime budget measurement"]
    fn benchmark_shared_engine_108() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let blocks = 400_usize;
        let total_frames = (blocks * crate::convolution::DEFAULT_PARTITION) as u32;
        let moving = std::env::var_os("SDA_BENCH_STATIC").is_none();
        let mut e = crate::Engine::new(48000, 2);
        e.set_layout(crate::vbap::LayoutId::Dolby9_1_6).unwrap();
        if let Ok(path) = std::env::var("SDA_BENCH_ROOM") {
            let room = crate::cinema::RoomProfile::load(&path).unwrap();
            e.set_layout(crate::vbap::LayoutId::parse(&room.layout).unwrap())
                .unwrap();
            e.cinema.enabled = true;
            e.room_profile = Some(std::sync::Arc::new(room));
        }
        e.replace_hrtf(
            crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),
            0.04,
        )
        .unwrap();
        e.directional_hrtf = true;
        e.near_field.enabled = true;
        e.source_extent = crate::source_extent::Settings {
            enabled: true,
            width: 0.25,
            diffusion: 0.12,
        };
        if std::env::var_os("SDA_BENCH_POINT").is_some() {
            e.source_extent.width = 0.0;
            e.source_extent.diffusion = 0.0;
        }
        eprintln!(
            "benchmark layout={} room={} width={} diffusion={}",
            e.layout.as_str(),
            e.cinema.enabled,
            e.source_extent.width,
            e.source_extent.diffusion
        );
        e.paused = false;
        e.output_active = true;
        for id in 0..118 {
            let angle = id as f32 * 0.13;
            let object = id < 108;
            let mut source = crate::Source {
                kind: if object {
                    crate::SourceKind::Object
                } else {
                    crate::SourceKind::Bed
                },
                object_id: object.then_some(id),
                position: [angle.cos() * 0.7, angle.sin() * 0.7, 0.3],
                bed_label: (!object).then(|| {
                    crate::vbap::speakers(e.layout)[(id - 108) as usize]
                        .name
                        .into()
                }),
                gain: 1.0,
                target_gain: 1.0,
                availability: 1.0,
                availability_target: 1.0,
                ..Default::default()
            };
            let samples: Vec<_> = (0..total_frames)
                .map(|i| ((i + id * 17) as f32 * 0.013).sin() * 0.001)
                .collect();
            source.samples.write(0, 0, &samples);
            if object && moving {
                source.spatial_events.insert(
                    0,
                    crate::SpatialEvent {
                        position: [(angle + 0.6).cos() * 0.7, (angle + 0.6).sin() * 0.7, 0.5],
                        extent: [0.0; 3],
                        zone_exclusion: Default::default(),
                        horizontal_only: false,
                        diffuse: 0.0,
                        spread: 0.0,
                        distance_m: None,
                        ramp: total_frames,
                    },
                );
            }
            if !object {
                let label = source.bed_label.clone().unwrap();
                crate::Engine::set_source_route(&mut source, crate::bed_route(&label, &e.vbap), 0);
            }
            let name = format!("source:{id}");
            e.sources.insert(name.clone(), source);
            e.route_source_now(&name, 0).unwrap();
        }
        let mut times = Vec::new();
        let mut cold = 0.0_f64;
        let mut output = vec![0.0; crate::convolution::DEFAULT_PARTITION * 2];
        for block in 0..blocks {
            if block == 16 {
                e.profile_ms = [0.0; 7];
            }
            let start = std::time::Instant::now();
            e.render_into(&mut output, 2);
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            assert!(output.iter().all(|x| x.is_finite()));
            if block >= 16 {
                times.push(ms)
            } else {
                cold += ms;
            }
        }
        eprintln!(
            "stage ms route / directional / legacy / buses / fast-mix / fast-reduce / output-reduce: {:?}",
            e.profile_ms.map(|v| v / (blocks - 16) as f64)
        );
        eprintln!("source-major object blocks={}", e.fast_object_blocks);
        times.sort_by(f64::total_cmp);
        eprintln!(
            "FULL 108 objects moving={moving} + 10 beds + reflections + near: mean={:.2} p95={:.2} max={:.2}ms budget=21.33ms; first 16 blocks={cold:.2}ms (audio 341.33ms)",
            times.iter().sum::<f64>() / times.len() as f64,
            times[times.len() * 95 / 100],
            times[times.len() - 1]
        );
    }
    #[test]
    fn source_major_matches_sample_major_pcm_events_activity_and_bass_management() {
        let build = |reference| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../web/public/hrtf/hrtf-set.json");
            let mut e = crate::Engine::new(48000, 2);
            e.replace_hrtf(
                crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),
                0.04,
            )
            .unwrap();
            e.disable_fast_objects = reference;
            e.directional_hrtf = true;
            e.cinema.monitor.enabled = true;
            e.cinema.monitor.bass_enabled = true;
            e.cinema.monitor.crossover_hz = 80.0;
            e.cinema.monitor.bass_db = -3.0;
            e.near_field.enabled = true;
            e.source_extent = crate::source_extent::Settings {
                enabled: true,
                width: 0.25,
                diffusion: 0.12,
            };
            e.paused = false;
            e.output_active = true;
            e.speaker_levels.fill(0.7);
            e.speaker_background.fill(0.25);
            for id in 0..18 {
                let object = id < 16;
                let angle = id as f32 * 0.4;
                let mut source = crate::Source {
                    kind: if object {
                        crate::SourceKind::Object
                    } else {
                        crate::SourceKind::Bed
                    },
                    object_id: object.then_some(id),
                    position: [angle.cos() * 0.6, angle.sin() * 0.6, 0.3],
                    gain: 1.0,
                    target_gain: 1.0,
                    availability: 1.0,
                    availability_target: 1.0,
                    ..Default::default()
                };
                let samples: Vec<_> = (0..8192)
                    .map(|i| ((i + id * 13) as f32 * 0.027).sin() * 0.001)
                    .collect();
                source.samples.write(0, 0, &samples[..2048]);
                source.samples.write(0, 4096, &samples[4096..]);
                source.gain_events.insert(
                    57,
                    crate::GainEvent {
                        gain: 0.65,
                        ramp: 123,
                    },
                );
                source.mute_events.insert(111, id % 2 == 0);
                source.mute_events.insert(1293, false);
                if object {
                    source.spatial_events.insert(
                        31,
                        crate::SpatialEvent {
                            position: [-0.3, 0.2, 0.7],
                            extent: [0.2, 0.1, 0.1],
                            zone_exclusion: if id == 1 {
                                std::sync::Arc::from([crate::adm_zone::Zone::Polar {
                                    min: [20.0, -10.0],
                                    max: [40.0, 10.0],
                                }])
                            } else {
                                Default::default()
                            },
                            horizontal_only: id % 2 == 0,
                            diffuse: 0.2,
                            spread: 0.0,
                            distance_m: None,
                            ramp: 15000,
                        },
                    );
                } else {
                    let label = if id == 16 { "FrontLeft" } else { "FrontRight" };
                    source.bed_label = Some(label.into());
                    crate::Engine::set_source_route(
                        &mut source,
                        crate::bed_route(label, &e.vbap),
                        0,
                    );
                }
                if id == 3 {
                    source.remove_at = Some(7003);
                }
                let name = format!("source:{id}");
                e.sources.insert(name.clone(), source);
                e.route_source_now(&name, 0).unwrap();
            }
            e
        };
        let mut actual = build(false);
        let mut reference = build(true);
        let chunks = [31, 993, 17, 1024, 511, 2048];
        let mut step = 0;
        while actual.sample_pos < 8192 {
            let frames = chunks[step % chunks.len()].min((8192 - actual.sample_pos) as usize);
            let mut a = vec![0.0; frames * 2];
            let mut b = a.clone();
            actual.render_into(&mut a, 2);
            reference.render_into(&mut b, 2);
            let delta = a
                .iter()
                .zip(&b)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            assert!(
                a.iter().all(|x| x.is_finite()) && delta < 2e-3,
                "transposed mixer PCM mismatch at {}: {delta}",
                actual.sample_pos
            );
            assert_eq!(actual.underrun_samples, reference.underrun_samples);
            assert_eq!(actual.route_update_count, reference.route_update_count);
            assert_eq!(
                actual.last_queued_activity.active_ids(),
                reference.last_queued_activity.active_ids()
            );
            for (id, source) in &actual.sources {
                let other = &reference.sources[id];
                assert_eq!(source.position, other.position);
                assert_eq!(source.bus_gains, other.bus_gains);
                assert_eq!(source.gain, other.gain);
            }
            step += 1;
        }
        assert!(actual.fast_object_blocks > 0);
        assert_eq!(reference.fast_object_blocks, 0);
    }
    #[test]
    fn cold_start_and_mode_switches_settle_to_the_same_pcm() {
        let build = |enabled| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../web/public/hrtf/hrtf-set.json");
            let mut e = crate::Engine::new(48000, 2);
            e.replace_hrtf(
                crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),
                0.04,
            )
            .unwrap();
            e.directional_hrtf = enabled;
            e.near_field.enabled = true;
            e.paused = false;
            e.output_active = true;
            let mut source = crate::Source {
                kind: crate::SourceKind::Object,
                position: [0.4, 0.3, 0.2],
                gain: 1.0,
                target_gain: 1.0,
                availability: 1.0,
                availability_target: 1.0,
                ..Default::default()
            };
            let samples: Vec<_> = (0..98304)
                .map(|i| (i as f32 * 0.031).sin() * 0.01)
                .collect();
            source.samples.write(0, 0, &samples);
            e.sources.insert("obj:1".into(), source);
            e.route_source_now("obj:1", 0).unwrap();
            e
        };
        let mut actual = build(true);
        let mut reference = build(false);
        let mut a = vec![0.0; 2048];
        let mut b = a.clone();
        for block in 0..96 {
            if block == 24 {
                actual.directional_hrtf = false;
            }
            if block == 48 {
                actual.directional_hrtf = true;
                reference.directional_hrtf = true;
            }
            actual.render_into(&mut a, 2);
            reference.render_into(&mut b, 2);
            assert!(a.iter().chain(&b).all(|x| x.is_finite()));
            if block == 0 {
                assert!(
                    actual.sources["obj:1"].direct.is_none(),
                    "new continuous source allocated a legacy room convolver"
                );
                assert!(actual.sources["obj:1"].continuous.is_some());
            }
            if (44..48).contains(&block) || block >= 88 {
                let delta = a
                    .iter()
                    .zip(&b)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f32, f32::max);
                assert!(
                    delta < 2e-6,
                    "mode switch did not settle at block {block}: {delta}"
                );
                assert!(
                    a.iter().any(|x| x.abs() > 1e-5),
                    "mode switch lost the object"
                );
            }
        }
    }
    #[test]
    fn authored_diffuse_objects_use_independent_convolution_when_directional_is_toggled() {
        for diffuse in [0.25, 1.0] {
            let build = |enabled| {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../web/public/hrtf/hrtf-set.json");
                let mut e = crate::Engine::new(48000, 2);
                e.replace_hrtf(
                    crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),
                    0.04,
                )
                .unwrap();
                e.directional_hrtf = enabled;
                e.direct_objects = true;
                e.direct_mix = 1.0;
                e.paused = false;
                e.output_active = true;
                let mut source = crate::Source {
                    kind: crate::SourceKind::Object,
                    position: [-0.22, 1.0, 0.19],
                    diffuse,
                    gain: 1.0,
                    target_gain: 1.0,
                    availability: 1.0,
                    availability_target: 1.0,
                    ..Default::default()
                };
                let pcm: Vec<_> = (0..65536)
                    .map(|i| (i as f32 * 0.031).sin() * 0.01)
                    .collect();
                source.samples.write(0, 0, &pcm);
                e.sources.insert("obj:11".into(), source);
                e.route_source_now("obj:11", 0).unwrap();
                e
            };
            let mut actual = build(true);
            let mut reference = build(false);
            let mut difference = 0.0_f32;
            for block in 0..64 {
                if block == 16 {
                    actual.directional_hrtf = false;
                }
                if block == 32 {
                    actual.directional_hrtf = true;
                }
                let mut a = [0.0; 2048];
                let mut b = a;
                actual.render_into(&mut a, 2);
                reference.render_into(&mut b, 2);
                if block > 48 {
                    difference += a.iter().zip(b).map(|(a, b)| (a - b).abs()).sum::<f32>();
                }
                assert!(a.iter().all(|x| x.is_finite()));
                if block > 4 {
                    assert!(a.iter().any(|x| x.abs() > 1e-5));
                }
            }
            assert!(
                difference > 0.01,
                "directional must not fall back to layout"
            );
            assert!(actual.sources["obj:11"].continuous_active);
            assert!(actual.sources["obj:11"].direct.is_none());
            assert_eq!(actual.sources["obj:11"].diffusion_mix, 0.0);
        }
    }
    #[test]
    fn shared_reflections_match_independent_objects_with_near_field_and_focus() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let mut set = crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap();
        let solver = crate::vbap::VbapSolver::with_layout(crate::vbap::LayoutId::Dolby9_1_6);
        let mut buses = crate::bus_renderer::BusRenderer::new(&set, &solver, 0.04).unwrap();
        let mut old: Vec<_> = (0..4)
            .map(|_| crate::direct_renderer::DirectSource::new(&set, 0.04).unwrap())
            .collect();
        let mut new: Vec<_> = (0..4)
            .map(|_| ContinuousSource::new(&set).unwrap())
            .collect();
        for source in &mut old {
            source.near_reference = Some(Box::new(
                crate::direct_renderer::DirectSource::new(&set, 0.0).unwrap(),
            ));
        }
        let amounts = std::array::from_fn(|i| if i % 2 == 0 { 0.4 } else { 0.0 });
        let mut difference = 0.0_f32;
        let mut tail = false;
        for block in 0..48 {
            buses.begin_block();
            for (id, (a, b)) in old.iter_mut().zip(&mut new).enumerate() {
                let gains = solver.pan([id as f32 * 0.2 - 0.3, 0.6, 0.4], 0.0);
                let direction = Direction {
                    position: [(block as f32 * 0.04 + id as f32).sin() * 0.6, 0.5, 0.3],
                    head: None,
                    diffuse: 0.0,
                    horizontal_only: false,
                    width: 30.0,
                    height: 20.0,
                    depth: 0.3,
                };
                a.direction = Some(direction);
                a.schedule_focus(solver.layout(), 0.04, gains, amounts);
                b.schedule(direction, solver.layout(), gains, amounts);
                for i in 0..crate::convolution::DEFAULT_PARTITION {
                    let input = if block < 20 {
                        ((block * crate::convolution::DEFAULT_PARTITION + i + id * 17) as f32
                            * 0.19)
                            .sin()
                            * 0.01
                    } else {
                        0.0
                    };
                    a.input[i] = input;
                    b.frames[i].input = input;
                    a.near_targets[i] = [1.2, 0.7];
                    b.frames[i].near = [1.2, 0.7];
                    buses.add_reflections(input, &gains, i);
                }
            }
            for i in 0..crate::convolution::DEFAULT_PARTITION {
                buses.shape_background(i, &amounts);
            }
            crate::direct_renderer::finish_sources(old.iter_mut(), &mut set, &solver, 0.04)
                .unwrap();
            finish_sources(new.iter_mut(), &set).unwrap();
            buses.finish_block().unwrap();
            for i in 0..crate::convolution::DEFAULT_PARTITION {
                let room = buses.output_at(i);
                for ear in 0..2 {
                    let expected: f32 = old
                        .iter()
                        .map(|s| if ear == 0 { s.left[i] } else { s.right[i] })
                        .sum();
                    let actual: f32 =
                        room[ear] + new.iter().map(|s| s.frames[i].output[ear]).sum::<f32>();
                    difference = difference.max((expected - actual).abs());
                    if block > 20 && room[ear].abs() > 1e-6 {
                        tail = true;
                    }
                }
            }
        }
        assert!(tail);
        assert!(difference < 2e-6, "shared-room PCM difference {difference}");
    }

    #[test]
    #[ignore = "offline 108-object shared reflection performance measurement"]
    fn benchmark_shared_directional_objects() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let set = crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap();
        let solver = crate::vbap::VbapSolver::with_layout(crate::vbap::LayoutId::Dolby9_1_6);
        let mut sources: Vec<_> = (0..108)
            .map(|_| ContinuousSource::new(&set).unwrap())
            .collect();
        let mut buses = crate::bus_renderer::BusRenderer::new(&set, &solver, 0.04).unwrap();
        let moving = std::env::var_os("SDA_BENCH_STATIC").is_none();
        for width in [0.0, 30.0] {
            let mut times = Vec::new();
            for block in 0..80 {
                let start = std::time::Instant::now();
                buses.begin_block();
                for (id, source) in sources.iter_mut().enumerate() {
                    let angle = id as f32 * 0.13 + if moving { block as f32 * 0.01 } else { 0.0 };
                    let position = [angle.cos() * 0.7, angle.sin() * 0.7, 0.3];
                    let gains = solver.pan(position, 0.0);
                    source.schedule(
                        Direction {
                            position,
                            head: None,
                            diffuse: 0.0,
                            horizontal_only: false,
                            width,
                            height: 0.0,
                            depth: 0.0,
                        },
                        solver.layout(),
                        gains,
                        [0.0; crate::vbap::MAX_BUS_COUNT],
                    );
                    for frame in &mut source.frames {
                        frame.input = 0.001;
                        frame.near = [1.2, 0.7];
                    }
                    for i in 0..crate::convolution::DEFAULT_PARTITION {
                        buses.add_reflections(0.001, &gains, i);
                    }
                }
                for i in 0..crate::convolution::DEFAULT_PARTITION {
                    buses.shape_background(i, &[0.0; crate::vbap::MAX_BUS_COUNT]);
                }
                finish_sources(sources.iter_mut(), &set).unwrap();
                buses.finish_block().unwrap();
                if block >= 16 {
                    times.push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            times.sort_by(f64::total_cmp);
            eprintln!(
                "108 objects moving={moving} + shared room + near, width={width}: mean={:.2} p95={:.2} max={:.2} ms; budget=21.33ms",
                times.iter().sum::<f64>() / times.len() as f64,
                times[times.len() * 95 / 100],
                times[times.len() - 1]
            );
        }
    }
    #[test]
    #[ignore = "offline directional convolution performance measurement"]
    fn benchmark_directional_objects() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../web/public/hrtf/hrtf-set.json");
        let mut set = crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap();
        let solver = crate::vbap::VbapSolver::with_layout(crate::vbap::LayoutId::Dolby9_1_6);
        let mut sources: Vec<_> = (0..108)
            .map(|_| crate::direct_renderer::DirectSource::new(&set, 0.04).unwrap())
            .collect();
        for width in [0.0, 30.0] {
            let start = std::time::Instant::now();
            for block in 0..30 {
                for (id, source) in sources.iter_mut().enumerate() {
                    let phase = id as f32 * 0.13 + block as f32 * 0.01;
                    let position = [phase.cos() * 0.7, phase.sin() * 0.7, 0.3];
                    source.direction = Some(Direction {
                        position,
                        head: None,
                        diffuse: 0.0,
                        horizontal_only: false,
                        width,
                        height: 0.0,
                        depth: 0.0,
                    });
                    source.schedule_focus(
                        solver.layout(),
                        0.04,
                        solver.pan(position, 0.0),
                        [0.0; crate::vbap::MAX_BUS_COUNT],
                    );
                    source.input.fill(0.001);
                }
                crate::direct_renderer::finish_sources(sources.iter_mut(), &mut set, &solver, 0.04)
                    .unwrap();
            }
            eprintln!(
                "108 moving objects width={width}: {:.2} ms/block (audio {:.2} ms)",
                start.elapsed().as_secs_f64() * 1000.0 / 30.0,
                crate::convolution::DEFAULT_PARTITION as f64 / 48.0
            );
        }
    }
    #[test]
    fn actual_object_pcm_uses_direction_instead_of_layout() {
        let render = |layout: crate::vbap::LayoutId, enabled: bool| {
            let mut e = crate::Engine::new(48000, 2);
            e.set_layout(layout).unwrap();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../web/public/hrtf/hrtf-set.json");
            e.replace_hrtf(
                crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),
                0.0,
            )
            .unwrap();
            e.directional_hrtf = enabled;
            e.paused = false;
            e.output_active = true;
            let mut source = crate::Source {
                kind: crate::SourceKind::Object,
                position: [0.7, -0.5, 0.4],
                gain: 1.0,
                target_gain: 1.0,
                availability: 1.0,
                availability_target: 1.0,
                ..Default::default()
            };
            let samples: Vec<_> = (0..24000).map(|i| (i as f32 * 0.17).sin() * 0.01).collect();
            source.samples.write(0, 0, &samples);
            e.sources.insert("obj:1".into(), source);
            e.route_source_now("obj:1", 0).unwrap();
            let mut out = vec![0.0; 48000];
            e.render_into(&mut out, 2);
            out
        };
        let a = render(crate::vbap::LayoutId::Dolby5_1_2, true);
        let b = render(crate::vbap::LayoutId::Dolby9_1_6, true);
        let old = render(crate::vbap::LayoutId::Dolby5_1_2, false);
        assert!(a.iter().chain(&b).all(|x| x.is_finite()));
        let difference = a[36000..]
            .iter()
            .zip(&b[36000..])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            difference < 2e-6,
            "layout leaked into uncalibrated dry direction: {difference}"
        );
        assert!(
            a[36000..]
                .iter()
                .zip(&old[36000..])
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                > 0.01
        );
    }
    #[test]
    fn delay_alignment_preserves_itd_and_does_not_duplicate_impulses() {
        // Waveform-structured bursts (decaying tail after the onset) so the
        // whole-waveform alignment has fine structure to lock onto, mirroring
        // real measurements. Left burst at `l`, right burst at 64 + `r` (each
        // ear owns one half of the packed dry buffer).
        let mut seed = 12345_u32;
        let mut burst = |position: usize, offset: usize| -> Vec<f32> {
            let mut dry = vec![0.0; 128];
            dry[offset + position] = 1.0;
            for k in 1..24 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if offset + position + k < 128 {
                    dry[offset + position + k] = ((seed >> 9) % 2000) as f32 / 65536.0;
                }
            }
            dry
        };
        let irs: Vec<_> = [(-30.0, 10, 20), (30.0, 20, 10)]
            .into_iter()
            .map(|(az, l, r)| {
                let mut dry = vec![0.0; 128];
                for (i, v) in burst(l, 0).iter().enumerate() {
                    dry[i] += *v;
                }
                for (i, v) in burst(r, 64).iter().enumerate() {
                    dry[i] += *v;
                }
                StereoIr {
                    azimuth: az,
                    elevation: 0.0,
                    wet: dry.clone(),
                    dry,
                }
            })
            .collect();
        let grid = Grid::new(&irs);
        let (l, r) = grid.interpolate(&irs, 0.0, 0.0);
        // Between measurements each ear stays one onset group: the neighbour's
        // burst is aligned onto the dominant's, never left as a separate echo.
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        let l_energy: f32 = l.iter().map(|v| v * v).sum();
        let r_energy: f32 = r.iter().map(|v| v * v).sum();
        assert!(
            (l_energy - r_energy).abs() < 0.35 * l_energy,
            "centred source must stay balanced: {l_energy} vs {r_energy}"
        );
        let late_tail: f32 = l[45..64].iter().map(|v| v * v).sum::<f32>()
            + r[45..64].iter().map(|v| v * v).sum::<f32>();
        assert!(
            late_tail < 0.05 * (l_energy + r_energy),
            "neighbour onset leaked as echo: {late_tail}"
        );
        let (l, r) = grid.interpolate(&irs, -30.0, 0.0);
        assert_eq!(l[10], 1.0);
        assert_eq!(r[20], 1.0);
        let a = grid.interpolate(&irs, 179.99999, 0.0);
        let b = grid.interpolate(&irs, -179.99999, 0.0);
        // The rear-pole mirrors pick different dominant measurements, so the
        // two outputs are aligned to different timelines; both must stay sane
        // and within the same measured-energy envelope.
        assert!(a.0.iter().chain(&b.0).all(|v| v.is_finite()));
        let energy = |p: &(Vec<f32>, Vec<f32>)| -> f64 {
            p.0.iter().chain(&p.1).map(|v| (*v as f64).powi(2)).sum()
        };
        let (ea, eb) = (energy(&a), energy(&b));
        assert!(
            ea > 0.0 && eb > 0.0 && (ea / eb - 1.0).abs() < 0.1,
            "rear pole energy must match: {ea} vs {eb}"
        );
    }
}
