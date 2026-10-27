//! General causal linear prediction; coefficients are carried by each packet.
//! No song, speaker preset or specific upmix matrix is built into this codec.
use super::*;
const ORDER: usize = 8;
const TAPS: usize = 5;
const D: usize = ORDER + 2 * TAPS;
const Q: i64 = 1 << 23;
const DESC: usize = 8 + D * 4 + 4;
pub const FRAMES: usize = 4096;
#[derive(Clone)]
struct Model {
    a: u8,
    b: u8,
    c: [i32; D],
}
fn features(pcm: &[Vec<i32>], ch: usize, i: usize, a: u8, b: u8) -> [f64; D] {
    let mut x = [0.; D];
    for k in 1..=ORDER {
        if i >= k {
            x[k - 1] = pcm[ch][i - k] as f64 / 8388608.
        }
    }
    for (r, off) in [(a, ORDER), (b, ORDER + TAPS)] {
        if r != 255 {
            for k in 0..TAPS {
                if i >= k {
                    x[off + k] = pcm[r as usize][i - k] as f64 / 8388608.
                }
            }
        }
    }
    x
}
fn prediction(decoded: &[Vec<i32>], own: &[i32], i: usize, m: &Model) -> i64 {
    let mut sum = 0i64;
    for k in 1..=ORDER {
        if i >= k {
            sum += own[i - k] as i64 * m.c[k - 1] as i64
        }
    }
    for (r, off) in [(m.a, ORDER), (m.b, ORDER + TAPS)] {
        if r != 255 {
            for k in 0..TAPS {
                if i >= k {
                    sum += decoded[r as usize][i - k] as i64 * m.c[off + k] as i64
                }
            }
        }
    }
    sum >> 23
}
fn fit(pcm: &[Vec<i32>], ch: usize, a: u8, b: u8, stabilize: bool) -> Model {
    let n = pcm[0].len();
    let mut matrix = [[0.; D + 1]; D];
    for i in (ORDER..n).step_by((n / 256).max(1)) {
        let x = features(pcm, ch, i, a, b);
        let y = pcm[ch][i] as f64 / 8388608.;
        for r in 0..D {
            matrix[r][D] += x[r] * y;
            for c in 0..=r {
                matrix[r][c] += x[r] * x[c];
            }
        }
    }
    let scale = (0..D).map(|r| matrix[r][r]).fold(0f64, f64::max);
    for r in 0..D {
        for c in r + 1..D {
            matrix[r][c] = matrix[c][r]
        }
        matrix[r][r] += scale * if stabilize { 1e-7 } else { 1e-12 } + 1e-14;
    }
    for col in 0..D {
        let pivot = (col..D)
            .max_by(|&a, &b| matrix[a][col].abs().total_cmp(&matrix[b][col].abs()))
            .unwrap();
        matrix.swap(col, pivot);
        let v = matrix[col][col];
        if v.abs() < 1e-20 {
            continue;
        }
        for k in col..=D {
            matrix[col][k] /= v
        }
        for row in 0..D {
            if row == col {
                continue;
            }
            let v = matrix[row][col];
            for k in col..=D {
                matrix[row][k] -= v * matrix[col][k]
            }
        }
    }
    if stabilize {
        // Stabilize the recursive part through bounded reflection coefficients.
        // Refit the feed-forward references after stabilization; they never recurse.
        let mut recursive: Vec<f64> = (0..ORDER).map(|i| matrix[i][D]).collect();
        let mut reflection = [0.; ORDER];
        for m in (1..=ORDER).rev() {
            let k = recursive[m - 1].clamp(-0.98, 0.98);
            reflection[m - 1] = k;
            let prev = recursive.clone();
            for j in 0..m - 1 {
                recursive[j] = (prev[j] + k * prev[m - 2 - j]) / (1. - k * k);
            }
        }
        let mut stable: Vec<f64> = vec![];
        for &k in &reflection {
            let old = stable.clone();
            stable = (0..old.len())
                .map(|j| old[j] - k * old[old.len() - 1 - j])
                .collect();
            stable.push(k);
        }
        for (i, v) in stable.iter_mut().enumerate() {
            *v *= 0.985f64.powi(i as i32 + 1);
            matrix[i][D] = *v;
        }
        if a != 255 || b != 255 {
            let mut feed = [[0.; D + 1]; D];
            for i in (ORDER..n).step_by((n / 256).max(1)) {
                let x = features(pcm, ch, i, a, b);
                let y = pcm[ch][i] as f64 / 8388608.
                    - (0..ORDER).map(|k| stable[k] * x[k]).sum::<f64>();
                for r in ORDER..D {
                    feed[r][D] += x[r] * y;
                    for c in ORDER..D {
                        feed[r][c] += x[r] * x[c];
                    }
                }
            }
            let scale = (ORDER..D).map(|r| feed[r][r]).fold(0f64, f64::max);
            for r in ORDER..D {
                feed[r][r] += scale * 1e-7 + 1e-12;
            }
            for col in ORDER..D {
                let pivot = (col..D)
                    .max_by(|&a, &b| feed[a][col].abs().total_cmp(&feed[b][col].abs()))
                    .unwrap();
                feed.swap(col, pivot);
                let v = feed[col][col];
                if v.abs() < 1e-20 {
                    continue;
                }
                for k in col..=D {
                    feed[col][k] /= v
                }
                for row in ORDER..D {
                    if row == col {
                        continue;
                    }
                    let v = feed[row][col];
                    for k in col..=D {
                        feed[row][k] -= v * feed[col][k]
                    }
                }
            }
            for r in ORDER..D {
                matrix[r][D] = feed[r][D];
            }
        }
    }
    let mut c = [0; D];
    for i in 0..D {
        c[i] = (matrix[i][D].clamp(-128., 128.) * Q as f64).round() as i32;
    }
    Model { a, b, c }
}
fn rice_parameter(v: &[u64]) -> u8 {
    if v.is_empty() {
        return 0;
    }
    let mean = v.iter().sum::<u64>() / v.len() as u64;
    let base = if mean == 0 {
        0
    } else {
        (63 - mean.leading_zeros() as u8).min(30)
    };
    (base.saturating_sub(1)..=(base + 1).min(30))
        .min_by_key(|&k| v.iter().map(|v| (v >> k) + 1 + k as u64).sum::<u64>())
        .unwrap()
}
fn entropy(values: &[u64]) -> Vec<u8> {
    let mut header = [0u8; 8];
    if values.iter().all(|&v| v == 0) {
        header[0] = 255;
        return header.to_vec();
    }
    let k = rice_parameter(values);
    let mut plain = Bits::new();
    for &v in values {
        plain.rice(v, k)
    }
    let (mut runs, mut nonzero, mut i) = (vec![], vec![], 0);
    while i < values.len() {
        if values[i] == 0 {
            let start = i;
            while i < values.len() && values[i] == 0 {
                i += 1
            }
            runs.push((i - start - 1) as u64)
        } else {
            nonzero.push(values[i] - 1);
            i += 1
        }
    }
    let rk = rice_parameter(&runs);
    let nk = rice_parameter(&nonzero);
    let mut sparse = Bits::new();
    let mut i = 0;
    while i < values.len() {
        if values[i] == 0 {
            let start = i;
            while i < values.len() && values[i] == 0 {
                i += 1
            }
            sparse.bit(false);
            sparse.rice((i - start - 1) as u64, rk);
        } else {
            sparse.bit(true);
            sparse.rice(values[i] - 1, nk);
            i += 1
        }
    }
    let bytes = if sparse.bytes.len() < plain.bytes.len() {
        header[0] = nk;
        header[1] = rk;
        header[2] = 1;
        sparse.bytes
    } else {
        header[0] = k;
        plain.bytes
    };
    header[4..8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    [header.to_vec(), bytes].concat()
}
fn decode_entropy(bytes: &[u8], count: usize) -> Result<(Vec<i64>, usize), String> {
    if bytes.len() < 8 {
        return Err("Truncated SDAR entropy header".into());
    }
    let k = bytes[0];
    let rk = bytes[1];
    let kind = bytes[2];
    let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if bytes[3] != 0
        || (k > 30 && k != 255)
        || rk > 12
        || kind > 1
        || len > bytes.len() - 8
        || (k == 255 && (len != 0 || kind != 0))
    {
        return Err("Invalid SDAR entropy coding".into());
    }
    if k == 255 {
        return Ok((vec![0; count], 8));
    }
    let mut bits = ReadBits {
        b: &bytes[8..8 + len],
        at: 0,
    };
    let mut values = vec![];
    while values.len() < count {
        if kind == 0 {
            values.push(unzig(bits.rice(k)?))
        } else if bits.bit()? {
            values.push(unzig(bits.rice(k)? + 1))
        } else {
            let n = bits.rice(rk)? as usize + 1;
            if n > count - values.len() {
                return Err("SDAR residual run past block".into());
            }
            values.resize(values.len() + n, 0)
        }
    }
    bits.finish()?;
    Ok((values, 8 + len))
}
fn temporal(history: &[i64], i: usize, m: &Model) -> Result<i64, String> {
    let mut sum = 0i128;
    for k in 1..=ORDER {
        if i >= k {
            sum += history[i - k] as i128 * m.c[k - 1] as i128
        }
    }
    let v = sum >> 23;
    if v.abs() > 1i128 << 40 {
        return Err("SDAR predictor state exceeds limit".into());
    }
    Ok(v as i64)
}
fn feed(pcm: &[Vec<i32>], i: usize, m: &Model) -> i64 {
    let mut sum = 0i128;
    for (r, off) in [(m.a, ORDER), (m.b, ORDER + TAPS)] {
        if r != 255 {
            for k in 0..TAPS {
                if i >= k {
                    sum += pcm[r as usize][i - k] as i128 * m.c[off + k] as i128
                }
            }
        }
    }
    (sum >> 23) as i64
}
// Bound reconstructed error independently for every source lane. A target
// bitrate is never permission to turn quiet stems into audible quantizer noise.
fn compact_shift_limit(samples: &[i32]) -> u8 {
    let rms = (samples.iter().map(|&v| (v as f64).powi(2)).sum::<f64>()
        / samples.len().max(1) as f64)
        .sqrt();
    ((rms * 0.004).max(2.0).log2().floor() as u8).min(23)
}

type EncodedLane = (Vec<i32>, Vec<u8>, u8);
fn encode_lane(
    pcm: &[Vec<i32>],
    ch: usize,
    decoded: &[Vec<i32>],
    m: &Model,
    requested_shift: u8,
    lossless: bool,
) -> Result<EncodedLane, String> {
    let n = pcm[ch].len();
    let mut shift = if lossless {
        0
    } else {
        requested_shift.min(compact_shift_limit(&pcm[ch]))
    };
    let signal = pcm[ch].iter().map(|&v| (v as f64).powi(2)).sum::<f64>();
    loop {
        let (mut cross_history, mut history) = (vec![], vec![]);
        let mut own = Vec::with_capacity(n);
        let mut residual = Vec::with_capacity(n);
        let mut error_square = 0.;
        let step = 1i64 << shift;
        for i in 0..n {
            if lossless {
                let pred = prediction(decoded, &own, i, m);
                own.push(pcm[ch][i]);
                residual.push(zig(pcm[ch][i] as i64 - pred));
                continue;
            }
            // Encoder and decoder must use identical reconstructed references.
            let cross = feed(decoded, i, m) + temporal(&cross_history, i, m)?;
            if cross.abs() > 1i64 << 40 {
                return Err("SDAR cross predictor state exceeds limit".into());
            }
            cross_history.push(cross);
            let pred = temporal(&history, i, m)?;
            let error = pcm[ch][i] as i64 - cross - pred;
            let q = if error >= 0 {
                (error + step / 2) / step
            } else {
                -((-error + step / 2) / step)
            };
            let min = (-8388608i64 - cross - pred + step - 1).div_euclid(step);
            let max = (8388607i64 - cross - pred).div_euclid(step);
            let value = pred + q.clamp(min, max) * step;
            history.push(value);
            let sample = (cross + value) as i32;
            error_square += (sample as f64 - pcm[ch][i] as f64).powi(2);
            own.push(sample);
            residual.push(zig(q.clamp(min, max)));
        }
        // Measured per-lane RMS protection, plus a two-PCM24-unit allowance
        // for signals below the 24-bit floor. Shift also bounds isolated errors.
        if !lossless && shift > 0 && error_square > (signal * 1e-6).max(4. * n as f64) {
            shift -= 1;
            continue;
        }
        let mut data = entropy(&residual[..n.min(ORDER)]);
        if n > ORDER {
            data.extend(entropy(&residual[ORDER..]));
        }
        return Ok((own, data, shift));
    }
}
fn assemble_packet(
    start: u64,
    n: usize,
    models: &[Model],
    lanes: &[(Vec<u8>, u8)],
    lossless: bool,
) -> Vec<u8> {
    let mut header = Vec::from(if lossless {
        b"SBL3".as_slice()
    } else {
        b"SBC2".as_slice()
    });
    header.extend(start.to_le_bytes());
    header.extend((n as u16).to_le_bytes());
    header.extend([models.len() as u8, 24]);
    for (m, (data, shift)) in models.iter().zip(lanes) {
        header.extend([m.a, m.b, *shift, 0, 0, 0, 0, 0]);
        for c in m.c {
            header.extend(c.to_le_bytes());
        }
        header.extend((data.len() as u32).to_le_bytes());
    }
    for (data, _) in lanes {
        header.extend(data);
    }
    seal(header)
}
fn make_packet(
    pcm: &[Vec<i32>],
    start: u64,
    models: &[Model],
    shift: u8,
    lossless: bool,
) -> Result<Vec<u8>, String> {
    let mut decoded = vec![];
    let mut lanes = vec![];
    for (ch, m) in models.iter().enumerate() {
        let (own, data, actual) = encode_lane(pcm, ch, &decoded, m, shift, lossless)?;
        decoded.push(own);
        lanes.push((data, actual));
    }
    Ok(assemble_packet(
        start,
        pcm[0].len(),
        models,
        &lanes,
        lossless,
    ))
}

pub fn encode(
    pcm: &[Vec<i32>],
    start: u64,
    bitrate: Option<u32>,
    rate: u32,
) -> Result<Vec<u8>, String> {
    let n = pcm.first().map_or(0, Vec::len);
    if n == 0
        || n > FRAMES
        || !(1..=128).contains(&pcm.len())
        || pcm
            .iter()
            .any(|c| c.len() != n || c.iter().any(|v| !(-8388608..=8388607).contains(v)))
    {
        return Err("Invalid SDAR linear block".into());
    }
    let mut models = vec![];
    let mut quality_decoded = vec![];
    let mut quality_lanes = vec![];
    for ch in 0..pcm.len() {
        let mut candidates = vec![(255, 255)];
        if pcm.len() > 16 {
            // Large authored scenes contain independent stems. Bound predictor
            // search instead of scanning every prior object for every packet.
            if ch > 0 {
                candidates.push(((ch - 1) as u8, 255));
            }
        } else if ch > 0 {
            let best = (0..ch)
                .max_by(|&a, &b| {
                    let score = |r: usize| {
                        let (dot, xx) =
                            pcm[r]
                                .iter()
                                .zip(&pcm[ch])
                                .fold((0., 0.), |(dot, xx), (&x, &y)| {
                                    (dot + x as f64 * y as f64, xx + (x as f64).powi(2))
                                });
                        dot.abs() / xx.sqrt().max(1.)
                    };
                    score(a).total_cmp(&score(b))
                })
                .unwrap();
            candidates.push((best as u8, 255));
        }
        if ch > 1 && pcm.len() <= 16 {
            candidates.push((0, 1))
        }
        let mut best: Option<(usize, Model, Option<EncodedLane>)> = None;
        for (a, b) in candidates {
            let m = fit(pcm, ch, a, b, bitrate.is_some());
            let encoded = if bitrate.is_some() {
                match encode_lane(
                    pcm,
                    ch,
                    &quality_decoded,
                    &m,
                    compact_shift_limit(&pcm[ch]),
                    false,
                ) {
                    Ok(lane) => Some(lane),
                    Err(_) => continue,
                }
            } else {
                None
            };
            let cost = if let Some((_, data, _)) = &encoded {
                data.len()
            } else {
                let values: Vec<u64> = (0..n)
                    .map(|i| zig(pcm[ch][i] as i64 - prediction(pcm, &pcm[ch], i, &m)))
                    .collect();
                entropy(&values[..n.min(ORDER)]).len()
                    + if n > ORDER {
                        entropy(&values[ORDER..]).len()
                    } else {
                        0
                    }
            };
            if best.as_ref().is_none_or(|(old, _, _)| cost < *old) {
                best = Some((cost, m, encoded));
            }
        }
        let (_, m, lane) = best.ok_or("No safe SDA linear predictor")?;
        models.push(m);
        if let Some((own, data, shift)) = lane {
            quality_decoded.push(own);
            quality_lanes.push((data, shift));
        }
    }
    let Some(kbps) = bitrate else {
        return make_packet(pcm, start, &models, 0, true);
    };

    let budget = ((kbps as u64 * 1000 * n as u64 / (rate as u64 * 8)) as usize)
        .max(16 + DESC * pcm.len() + pcm.len() * 32 + 128);
    // Select predictors by actual closed-loop coding cost at the quality
    // floor. This avoids reference models whose apparent compression relies
    // on amplifying quantization error in another channel.
    let mut best = assemble_packet(start, n, &models, &quality_lanes, false);
    if best.len() > budget {
        return Ok(best);
    }
    let mut low = 0;
    let mut high = quality_lanes
        .iter()
        .map(|(_, shift)| *shift)
        .max()
        .unwrap_or(0);
    while low < high {
        let mid = (low + high) / 2;
        match make_packet(pcm, start, &models, mid, false) {
            Ok(packet) if packet.len() <= budget => {
                high = mid;
                best = packet;
            }
            _ => low = mid + 1,
        }
    }
    Ok(best)
}
pub fn decode(b: &[u8], start: u64, channels: usize, n: usize) -> Result<Vec<Vec<i32>>, String> {
    if b.len() < 16 + DESC * channels
        || ![b"SBL3".as_slice(), b"SBC2".as_slice()].contains(&&b[..4])
        || u64::from_le_bytes(b[4..12].try_into().unwrap()) != start
        || u16::from_le_bytes(b[12..14].try_into().unwrap()) as usize != n
        || b[14] as usize != channels
        || b[15] != 24
        || n == 0
        || n > FRAMES
        || !(1..=128).contains(&channels)
    {
        return Err("Invalid SDAR linear header".into());
    }
    let lossless = &b[..4] == b"SBL3";
    let mut out: Vec<Vec<i32>> = vec![];
    let mut at = 16 + DESC * channels;
    for ch in 0..channels {
        let d = &b[16 + DESC * ch..16 + DESC * (ch + 1)];
        let (a, rb, shift) = (d[0], d[1], d[2]);
        if [a, rb].iter().any(|&r| r != 255 && r as usize >= ch)
            || shift > 23
            || (lossless && shift != 0)
            || d[3..8] != [0, 0, 0, 0, 0]
        {
            return Err("Invalid SDAR linear model".into());
        }
        let mut c = [0; D];
        for i in 0..D {
            c[i] = i32::from_le_bytes(d[8 + i * 4..12 + i * 4].try_into().unwrap());
            if c[i].unsigned_abs() > 128 * Q as u32 {
                return Err("SDAR linear coefficient out of range".into());
            }
        }
        if (a == 255 && c[ORDER..ORDER + TAPS].iter().any(|&v| v != 0))
            || (rb == 255 && c[ORDER + TAPS..].iter().any(|&v| v != 0))
        {
            return Err("SDAR unavailable predictor reference".into());
        }
        let m = Model { a, b: rb, c };
        let len = u32::from_le_bytes(d[DESC - 4..].try_into().unwrap()) as usize;
        let end = at
            .checked_add(len)
            .filter(|&v| v <= b.len())
            .ok_or("Truncated SDAR linear residuals")?;
        let (first, used) = decode_entropy(&b[at..end], n.min(ORDER))?;
        let mut residual = first;
        let mut total = used;
        if n > ORDER {
            let (rest, used) = decode_entropy(&b[at + used..end], n - ORDER)?;
            residual.extend(rest);
            total += used;
        }
        if total != len {
            return Err("Extra SDAR linear residual bytes".into());
        }
        let mut own = vec![];
        let mut cross_history = vec![];
        let mut residual_history = vec![];
        for (i, r) in residual.into_iter().enumerate() {
            if lossless {
                let sample = prediction(&out, &own, i, &m) + r;
                if !(-8388608..=8388607).contains(&sample) {
                    return Err("SDAR lossless PCM out of range".into());
                }
                own.push(sample as i32);
                continue;
            }
            let cross = feed(&out, i, &m) + temporal(&cross_history, i, &m)?;
            if cross.abs() > 1i64 << 40 {
                return Err("SDAR cross predictor state exceeds limit".into());
            }
            cross_history.push(cross);
            let value = r
                .checked_mul(1i64 << shift)
                .and_then(|r| {
                    temporal(&residual_history, i, &m)
                        .ok()
                        .and_then(|p| p.checked_add(r))
                })
                .ok_or("SDAR residual arithmetic overflow")?;
            residual_history.push(value);
            let sample = cross + value;
            if !(-8388608..=8388607).contains(&sample) {
                return Err("SDAR linear reconstruction out of range".into());
            }
            own.push(sample as i32)
        }
        out.push(own);
        at = end;
    }
    if at != b.len() {
        return Err("Extra SDAR linear packet bytes".into());
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossy_reference_quantization_cannot_amplify_into_other_channels() {
        let n = FRAMES;
        let pcm = vec![
            (0..n)
                .map(|i| ((i as i32 * 17) % 32768 - 16384) * 10)
                .collect(),
            (0..n).map(|i| ((i as i32 * 31) % 97 - 48) * 2).collect(),
        ];
        let first = Model {
            a: 255,
            b: 255,
            c: [0; D],
        };
        let mut second = Model {
            a: 0,
            b: 255,
            c: [0; D],
        };
        second.c[0] = (0.95 * Q as f64) as i32;
        second.c[ORDER] = (64 * Q) as i32;
        let shift = 14;
        let packet = make_packet(&pcm, 0, &[first, second], shift, false).unwrap();
        let decoded = decode(&unseal(&packet).unwrap(), 0, 2, n).unwrap();
        for ch in 0..2 {
            for i in 0..n {
                assert!(
                    (decoded[ch][i] as i64 - pcm[ch][i] as i64).abs()
                        <= 1i64 << shift.min(compact_shift_limit(&pcm[ch])),
                    "channel {ch} sample {i}: cross prediction amplified quantization noise"
                );
            }
        }
    }
    #[test]
    fn compact_quality_floor_protects_quiet_independent_lanes() {
        let pcm: Vec<Vec<i32>> = (0..24)
            .map(|ch| {
                (0..FRAMES)
                    .map(|i| {
                        let amplitude = if ch % 3 == 0 { 300000 } else { 3000 };
                        ((i as f64 * (ch + 1) as f64 * 0.017).sin() * amplitude as f64) as i32
                    })
                    .collect()
            })
            .collect();
        let packet = encode(&pcm, 0, Some(768), 48000).unwrap();
        let decoded = decode(&unseal(&packet).unwrap(), 0, pcm.len(), FRAMES).unwrap();
        for ch in 0..pcm.len() {
            let signal = pcm[ch].iter().map(|&v| (v as f64).powi(2)).sum::<f64>();
            let noise = decoded[ch]
                .iter()
                .zip(&pcm[ch])
                .map(|(&y, &x)| (y as f64 - x as f64).powi(2))
                .sum::<f64>();
            assert!(noise <= (signal * 1e-6).max(4. * FRAMES as f64));
            let bound = 1i64 << compact_shift_limit(&pcm[ch]);
            assert!(decoded[ch]
                .iter()
                .zip(&pcm[ch])
                .all(|(&y, &x)| (y as i64 - x as i64).abs() <= bound));
        }
    }
    #[test]
    fn roundtrip() {
        for n in [1, 17, 1024, 4096] {
            let pcm: Vec<Vec<i32>> = (0..12)
                .map(|ch| {
                    (0..n)
                        .map(|i| {
                            ((i as i32 * 997) % 900000 - 450000) / (ch as i32 + 1)
                                * (if ch % 2 == 0 { 1 } else { -1 })
                        })
                        .collect()
                })
                .collect();
            let p = encode(&pcm, 0, None, 48000).unwrap();
            assert_eq!(decode(&unseal(&p).unwrap(), 0, 12, n).unwrap(), pcm);
            let p = encode(&pcm, 0, Some(768), 48000).unwrap();
            assert_eq!(decode(&unseal(&p).unwrap(), 0, 12, n).unwrap().len(), 12);
        }
    }
}
