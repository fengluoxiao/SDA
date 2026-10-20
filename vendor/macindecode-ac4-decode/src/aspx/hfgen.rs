//! HF 信号创建（`TS103190-1:v1.4.1:5.7.6.4.1.4`）。
//!
//! `Pseudocode 89` 把低带按 patch 表搬到 A-SPX 范围，同时施加音调噪声比调整与
//! 预平坦化。它是 `5.7.6.4.1` 的汇合点，四路输入在这里第一次同时用上：
//!
//! - [`super::patches`] 的 patch 表说明搬哪几段、源在低带的哪里；
//! - [`super::tna`] 的 `alpha0`/`alpha1` 与 chirp 因子构成二阶预测滤波；
//! - [`super::preflatten`] 的增益向量按 `aspx_preflat` 取倒数施加；
//! - [`super::bands`] 的噪声子带组表决定每个高带子带用哪个 chirp。
//!
//! # 三处下标各走各的
//!
//! 同一次内层循环里有三个不同的子带下标，混用任意两个都会静默地搬错频段：
//!
//! - `sb_high = sbx + sum_sb_patches + sb` 是写入的高带子带，跨 patch 连续累加；
//! - `p = sbg_patch_start_sb[i] + sb` 是读取的低带子带，每段各自从源起点数起；
//! - `g` 是噪声包络下标，只在 `sbg_noise[g+1] == sb_high` 时前进一格。
//!
//! `alpha` 与增益向量按 **`p`** 取，chirp 按 **`g`** 取。前者是源子带的性质
//! （预测系数是在低带上算的），后者是目标频段的性质。
//!
//! # 时间轴
//!
//! 输入是 [`super::tna::ExtendedLowBand`]，与 `Pseudocode 86` 的协方差同一条
//! 时间轴。`n = ts + ts_offset_hfadj` 把 `Q_high` 的时隙映到 `Q_low_ext` 上，
//! 最远回看 `n − 4`；`ts` 从 `atsg_sig[0] * num_ts_in_ats` 起，而
//! `5.7.6.3.3.1` 已拒绝为负的起始边界，因此 `n − 4 ≥ 0` 恒成立。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "下标由 patch 表、64 个子带与已校验的时隙范围派生；\
              `Pseudocode 89` 的三个下标用显式算式比迭代器更贴近原文"
)]

use crate::aspx::patches::PatchTable;
use crate::aspx::preflatten::PreFlattenGains;
use crate::aspx::qmf::QmfSlot;
use crate::aspx::tables::{MAX_SBG_NOISE, NUM_QMF_SUBBANDS};
use crate::aspx::tna::{Coefficient, ExtendedLowBand, TS_OFFSET_HFADJ, TnaFilters};

/// 子带数，`5.7.3.2` 规定恒为 64。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// HF 信号无法生成的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HfGenError {
    /// 时隙区间为空或首尾颠倒。
    EmptyInterval { first: usize, last: usize },
    /// 时隙区间超出 `Q_low_ext` 能提供的范围。
    ///
    /// 最后一个时隙要读到 `Q_low_ext[last − 1 + ts_offset_hfadj]`。
    IntervalOutOfRange { last: usize, available: usize },
    /// patch 段要写到 64 个 QMF 子带之外。
    SubbandOutOfRange { sb_high: usize },
    /// patch 段的低带源超出 `alpha` 已填充的子带数。
    SourceWithoutFilter { source: usize, filters: u8 },
    /// 噪声子带组数与 chirp 因子个数不符。
    ChirpCountMismatch { groups: usize, provided: usize },
    /// 噪声子带组数不在规范的 `1..=MAX_SBG_NOISE` 内。
    NoiseGroupCountOutOfRange { groups: usize },
    /// 噪声子带组边界没有严格递增。
    NoiseBordersNotIncreasing {
        index: usize,
        previous: u8,
        current: u8,
    },
    /// 启用预平坦化但增益向量没有覆盖到源子带。
    MissingGain { source: usize },
    /// 输出缓冲的时隙数与请求的区间不符。
    OutputLengthMismatch { expected: usize, provided: usize },
    /// patch 表与噪声子带组表给出的 `sbx` 不一致。
    ///
    /// 两张表由不同的推导产出，`Pseudocode 89` 默认它们的第 0 项都是交叉子带。
    CrossoverMismatch { patches: usize, noise: usize },
}

/// `Pseudocode 89` 的 HF 信号创建。
///
/// `out` 按时隙索引，长度必须是 `last − first`；每个时隙只写
/// `[sbx, sbx + Σ patch_num_sb)` 这一段，低带留给调用方。`noise_borders` 是
/// `sbg_noise`，长度为噪声子带组数加一。`gains` 只在 `preflat` 为真时读取。
///
/// # Errors
///
/// 见 [`HfGenError`]。任一条不成立时都不改写 `out`。
#[expect(
    clippy::too_many_arguments,
    reason = "`Pseudocode 89` 的输入就是这七路，聚成结构体只会把同一组参数换个地方写"
)]
pub fn hf_generate(
    ext: ExtendedLowBand<'_>,
    patches: &PatchTable,
    filters: &TnaFilters,
    chirp: &[f32],
    noise_borders: &[u8],
    gains: Option<&PreFlattenGains>,
    first: usize,
    last: usize,
    out: &mut [QmfSlot],
) -> Result<(), HfGenError> {
    if last <= first {
        return Err(HfGenError::EmptyInterval { first, last });
    }
    if out.len() != last - first {
        return Err(HfGenError::OutputLengthMismatch {
            expected: last - first,
            provided: out.len(),
        });
    }
    // 最后一个时隙读到 `Q_low_ext[last − 1 + ts_offset_hfadj]`。
    let Some(needed) = last.checked_add(TS_OFFSET_HFADJ) else {
        return Err(HfGenError::IntervalOutOfRange {
            last,
            available: ext.timeslots(),
        });
    };
    if needed > ext.timeslots() {
        return Err(HfGenError::IntervalOutOfRange {
            last,
            available: ext.timeslots(),
        });
    }
    if noise_borders.len() < 2 || noise_borders.len() > MAX_SBG_NOISE as usize + 1 {
        return Err(HfGenError::NoiseGroupCountOutOfRange {
            groups: noise_borders.len().saturating_sub(1),
        });
    }
    if chirp.len() != noise_borders.len() - 1 {
        return Err(HfGenError::ChirpCountMismatch {
            groups: noise_borders.len() - 1,
            provided: chirp.len(),
        });
    }
    for (index, pair) in noise_borders.windows(2).enumerate() {
        if pair[0] >= pair[1] {
            return Err(HfGenError::NoiseBordersNotIncreasing {
                index: index + 1,
                previous: pair[0],
                current: pair[1],
            });
        }
    }

    // `sbg_patches[0]` 与 `sbg_noise[0]` 都定义为 `sbx`（前者见 `Pseudocode 71`，
    // 后者经 `sbg_sig_lowres[0] = sbg_sig_highres[0]` 回到同一个值）。两个表由不同
    // 的推导给出，取其一而不核对另一个，等于把这条隐含前提留给运气。
    let sbx = usize::from(patches.border(0).unwrap_or(0));
    if usize::from(noise_borders[0]) != sbx {
        return Err(HfGenError::CrossoverMismatch {
            patches: sbx,
            noise: usize::from(noise_borders[0]),
        });
    }

    // 先把全部下标核对一遍，越界不能等到写了一半才发现。
    let mut probe = 0usize;
    for index in 0..usize::from(patches.count()) {
        let num_sb = usize::from(patches.num_sb(index).unwrap_or(0));
        let start = usize::from(patches.start_sb(index).unwrap_or(0));
        for sb in 0..num_sb {
            let sb_high = sbx + probe + sb;
            if sb_high >= SUBBANDS {
                return Err(HfGenError::SubbandOutOfRange { sb_high });
            }
            let source = start + sb;
            if source >= usize::from(filters.subbands()) {
                return Err(HfGenError::SourceWithoutFilter {
                    source,
                    filters: filters.subbands(),
                });
            }
            if let Some(gains) = gains
                && gains.gain(source).is_none()
            {
                return Err(HfGenError::MissingGain { source });
            }
        }
        probe += num_sb;
    }

    for (slot_index, slot) in out.iter_mut().enumerate() {
        let ts = first + slot_index;
        let n = ts + TS_OFFSET_HFADJ;
        let mut sum_sb_patches = 0usize;
        let mut g = 0usize;
        for index in 0..usize::from(patches.count()) {
            let num_sb = usize::from(patches.num_sb(index).unwrap_or(0));
            let start = usize::from(patches.start_sb(index).unwrap_or(0));
            for sb in 0..num_sb {
                let sb_high = sbx + sum_sb_patches + sb;
                // 噪声包络只前进不后退，且逐时隙从 0 重新数起。
                if g + 1 < noise_borders.len() && usize::from(noise_borders[g + 1]) == sb_high {
                    g += 1;
                }
                let source = start + sb;
                let (a0, a1) = filters.coefficients(source).unwrap_or_default();
                let chirp_g = chirp.get(g).copied().unwrap_or(0.0);

                let (re0, im0) = ext.sample(source, n).unwrap_or((0.0, 0.0));
                let (re2, im2) = ext.sample(source, n - 2).unwrap_or((0.0, 0.0));
                let (re4, im4) = ext.sample(source, n - 4).unwrap_or((0.0, 0.0));

                // chirp 是实数，alpha 是复数：先把 chirp 折进 alpha 再做复乘。
                let g0 = scale(a0, chirp_g);
                let g1 = scale(a1, chirp_g * chirp_g);
                let mut re = re0 + g0.re * re2 - g0.im * im2 + g1.re * re4 - g1.im * im4;
                let mut im = im0 + g0.re * im2 + g0.im * re2 + g1.re * im4 + g1.im * re4;

                if let Some(gains) = gains {
                    // `*= 1/gain_vec[p]`：增益向量描述低带斜率，取倒数把它抹平。
                    let gain = gains.gain(source).unwrap_or(1.0);
                    re /= gain;
                    im /= gain;
                }
                slot.re[sb_high] = re;
                slot.im[sb_high] = im;
            }
            sum_sb_patches += num_sb;
        }
    }
    Ok(())
}

/// 复系数乘一个实数。
fn scale(value: Coefficient, factor: f32) -> Coefficient {
    Coefficient {
        re: value.re * factor,
        im: value.im * factor,
    }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "判据取的都是在 f32 上精确可表示的值，容差会让它们变松"
)]
mod tests {
    use super::*;
    use crate::aspx::tna::TnaDelay;

    const N: usize = 8;
    const HFGEN: usize = 3;

    /// 造一条 `Q_low`，让 `Q_low_ext[sb][ts]` 的实部为 `sb * 100 + ts`。
    ///
    /// 每个 `(子带, 时隙)` 都有唯一值，因此搬错源、错位一个时隙都直接看得出来。
    fn ramp(delay: &mut TnaDelay, q_low: &mut [QmfSlot]) {
        for ts in 0..TS_OFFSET_HFADJ {
            for sb in 0..SUBBANDS {
                delay.tail_mut(ts).re[sb] = (sb * 100 + ts) as f32;
            }
        }
        for (k, slot) in q_low.iter_mut().enumerate() {
            for sb in 0..SUBBANDS {
                slot.re[sb] = (sb * 100 + k + TS_OFFSET_HFADJ) as f32;
            }
        }
    }

    /// 单段 patch：源从低带 `start` 起 `num` 个子带，高带从 `sbx` 起。
    fn single_patch(start: u8, num: u8, sbx: u8) -> PatchTable {
        PatchTable::from_parts(&[num], &[start], sbx)
    }

    /// alpha 全零、chirp 全零的滤波器：HF 生成退化成纯搬运。
    fn passthrough_filters(subbands: u8) -> TnaFilters {
        let mut filters = TnaFilters::new();
        filters.fill_for_test(subbands, Coefficient::ZERO, Coefficient::ZERO);
        filters
    }

    #[test]
    fn with_zero_coefficients_the_generator_is_a_pure_patch_copy() {
        // alpha 与 chirp 全零时 Pseudocode 89 只剩第一行 Q_high = Q_low_ext[p][n]。
        // 源值编码成 sb*100 + ts，因此逐格核对就能钉住三个下标各自的算式。
        let mut delay = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        ramp(&mut delay, &mut q_low);
        let ext = ExtendedLowBand::new(&delay, &q_low);

        let sbx = 20u8;
        let patches = single_patch(4, 3, sbx);
        let filters = passthrough_filters(8);
        let mut out = [QmfSlot::zero(); 2];
        hf_generate(
            ext,
            &patches,
            &filters,
            &[0.0],
            &[sbx, 63],
            None,
            1,
            3,
            &mut out,
        )
        .expect("应能生成");

        for (slot_index, slot) in out.iter().enumerate() {
            let ts = 1 + slot_index;
            for sb in 0..3usize {
                let source = 4 + sb;
                let expected = (source * 100 + ts + TS_OFFSET_HFADJ) as f32;
                assert_eq!(
                    slot.re[usize::from(sbx) + sb],
                    expected,
                    "高带 {} 应取低带 {source} 的第 {} 个扩展时隙",
                    usize::from(sbx) + sb,
                    ts + TS_OFFSET_HFADJ
                );
            }
        }
    }

    #[test]
    fn the_two_filter_taps_reach_back_two_and_four_timeslots() {
        // alpha0 只作用于 n−2、alpha1 只作用于 n−4：分别单独置 1 看落点。
        let mut delay = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        ramp(&mut delay, &mut q_low);
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let sbx = 20u8;
        let patches = single_patch(4, 1, sbx);
        let mut out = [QmfSlot::zero(); 1];

        for (tap, offset) in [(0usize, 2usize), (1, 4)] {
            let mut filters = TnaFilters::new();
            let one = Coefficient { re: 1.0, im: 0.0 };
            let (a0, a1) = if tap == 0 {
                (one, Coefficient::ZERO)
            } else {
                (Coefficient::ZERO, one)
            };
            filters.fill_for_test(8, a0, a1);
            // chirp = 1 时两个抽头的系数分别是 chirp 与 chirp²，都等于 1。
            hf_generate(
                ext,
                &patches,
                &filters,
                &[1.0],
                &[sbx, 63],
                None,
                2,
                3,
                &mut out,
            )
            .expect("应能生成");
            let n = 2 + TS_OFFSET_HFADJ;
            let expected = (4 * 100 + n) as f32 + (4 * 100 + n - offset) as f32;
            assert_eq!(
                out[0].re[usize::from(sbx)],
                expected,
                "抽头 {tap} 应回看 {offset} 个时隙"
            );
        }
    }

    #[test]
    fn the_second_tap_is_weighted_by_the_squared_chirp() {
        // alpha1 的系数是 chirp²，alpha0 的是 chirp。取 chirp = 0,5 让两者可区分。
        const SPEC_CHIRP: f32 = 0.5;
        let mut delay = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        ramp(&mut delay, &mut q_low);
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let sbx = 20u8;
        let patches = single_patch(4, 1, sbx);
        let mut filters = TnaFilters::new();
        let one = Coefficient { re: 1.0, im: 0.0 };
        filters.fill_for_test(8, one, one);
        let mut out = [QmfSlot::zero(); 1];
        hf_generate(
            ext,
            &patches,
            &filters,
            &[SPEC_CHIRP],
            &[sbx, 63],
            None,
            2,
            3,
            &mut out,
        )
        .expect("应能生成");

        let n = 2 + TS_OFFSET_HFADJ;
        let base = (4 * 100 + n) as f32;
        let tap0 = (4 * 100 + n - 2) as f32;
        let tap1 = (4 * 100 + n - 4) as f32;
        assert_eq!(
            out[0].re[usize::from(sbx)],
            base + SPEC_CHIRP * tap0 + SPEC_CHIRP * SPEC_CHIRP * tap1
        );
    }

    #[test]
    fn the_noise_envelope_index_follows_the_high_subband_not_the_source() {
        // chirp 按 g 取而不是按 p：把两个噪声组的 chirp 设成不同值，边界落在
        // patch 中间，于是同一段里前后两半用不同的 chirp。
        let mut delay = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        ramp(&mut delay, &mut q_low);
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let sbx = 20u8;
        let patches = single_patch(4, 4, sbx);
        let mut filters = TnaFilters::new();
        let one = Coefficient { re: 1.0, im: 0.0 };
        filters.fill_for_test(8, one, Coefficient::ZERO);
        let mut out = [QmfSlot::zero(); 1];
        // 噪声边界 22 落在高带 20…24 的正中。
        hf_generate(
            ext,
            &patches,
            &filters,
            &[0.0, 1.0],
            &[sbx, 22, 63],
            None,
            2,
            3,
            &mut out,
        )
        .expect("应能生成");

        let n = 2 + TS_OFFSET_HFADJ;
        for sb in 0..4usize {
            let source = 4 + sb;
            let sb_high = usize::from(sbx) + sb;
            let base = (source * 100 + n) as f32;
            let tap = (source * 100 + n - 2) as f32;
            // 高带 < 22 用 chirp[0] = 0，≥ 22 用 chirp[1] = 1。
            let expected = if sb_high < 22 { base } else { base + tap };
            assert_eq!(out[0].re[sb_high], expected, "高带 {sb_high} 的 chirp 选错");
        }
    }

    #[test]
    fn preflattening_divides_by_the_gain_of_the_source_subband() {
        // `*= 1/gain_vec[p]`：增益按源子带 p 取，不是按高带 sb_high。
        let mut delay = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        ramp(&mut delay, &mut q_low);
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let sbx = 20u8;
        let patches = single_patch(4, 2, sbx);
        let filters = passthrough_filters(8);
        let mut gains = PreFlattenGains::new();
        // 源子带 4 与 5 给不同的增益；若按 sb_high 取会落到 20、21 上。
        gains.fill_for_test(&[1.0, 1.0, 1.0, 1.0, 2.0, 4.0]);
        let mut out = [QmfSlot::zero(); 1];
        hf_generate(
            ext,
            &patches,
            &filters,
            &[0.0],
            &[sbx, 63],
            Some(&gains),
            2,
            3,
            &mut out,
        )
        .expect("应能生成");

        let n = 2 + TS_OFFSET_HFADJ;
        assert_eq!(out[0].re[usize::from(sbx)], (4 * 100 + n) as f32 / 2.0);
        assert_eq!(out[0].re[usize::from(sbx) + 1], (5 * 100 + n) as f32 / 4.0);
    }

    #[test]
    fn consecutive_patches_stack_without_gaps_or_overlap() {
        // sum_sb_patches 的累加：第二段的高带紧接第一段末尾，源却各自从
        // patch_start_sb 数起。两段源区间故意不连续。
        let mut delay = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        ramp(&mut delay, &mut q_low);
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let sbx = 20u8;
        let patches = PatchTable::from_parts(&[2, 3], &[6, 1], sbx);
        let filters = passthrough_filters(10);
        let mut out = [QmfSlot::zero(); 1];
        hf_generate(
            ext,
            &patches,
            &filters,
            &[0.0],
            &[sbx, 63],
            None,
            2,
            3,
            &mut out,
        )
        .expect("应能生成");

        let n = 2 + TS_OFFSET_HFADJ;
        let expected = [
            (6 * 100 + n) as f32,
            (7 * 100 + n) as f32,
            (100 + n) as f32,
            (2 * 100 + n) as f32,
            (3 * 100 + n) as f32,
        ];
        for (offset, &want) in expected.iter().enumerate() {
            assert_eq!(
                out[0].re[usize::from(sbx) + offset],
                want,
                "高带 {} 取错源",
                usize::from(sbx) + offset
            );
        }
        // 段外不写。
        assert_eq!(out[0].re[usize::from(sbx) + 5], 0.0, "patch 之外不应被写");
    }

    #[test]
    fn the_imaginary_part_uses_the_full_complex_product() {
        // alpha 取纯虚数：实部只能来自 −alpha.im * im，虚部只能来自 alpha.im * re。
        let mut delay = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        for ts in 0..TS_OFFSET_HFADJ {
            delay.tail_mut(ts).re[4] = 1.0;
            delay.tail_mut(ts).im[4] = 2.0;
        }
        for slot in &mut q_low {
            slot.re[4] = 1.0;
            slot.im[4] = 2.0;
        }
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let sbx = 20u8;
        let patches = single_patch(4, 1, sbx);
        let mut filters = TnaFilters::new();
        filters.fill_for_test(8, Coefficient { re: 0.0, im: 1.0 }, Coefficient::ZERO);
        let mut out = [QmfSlot::zero(); 1];
        hf_generate(
            ext,
            &patches,
            &filters,
            &[1.0],
            &[sbx, 63],
            None,
            2,
            3,
            &mut out,
        )
        .expect("应能生成");

        // (1 + 2i) + i·(1 + 2i) = 1 + 2i + i − 2 = −1 + 3i
        assert_eq!(out[0].re[usize::from(sbx)], -1.0);
        assert_eq!(out[0].im[usize::from(sbx)], 3.0);
    }

    #[test]
    fn rejected_input_leaves_the_output_untouched() {
        let mut delay = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        ramp(&mut delay, &mut q_low);
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let sbx = 20u8;
        let patches = single_patch(4, 2, sbx);
        let filters = passthrough_filters(8);
        let mut out = [QmfSlot::zero(); 1];
        out[0].re[usize::from(sbx)] = 42.0;
        out[0].im[usize::from(sbx) + 1] = -7.0;
        let snapshot = out;

        for error in [
            // 区间为空
            hf_generate(
                ext,
                &patches,
                &filters,
                &[0.0],
                &[sbx, 63],
                None,
                3,
                3,
                &mut out,
            ),
            // 输出长度不符
            hf_generate(
                ext,
                &patches,
                &filters,
                &[0.0],
                &[sbx, 63],
                None,
                0,
                4,
                &mut out,
            ),
            // 源超出已解出的 alpha
            hf_generate(
                ext,
                &patches,
                &passthrough_filters(4),
                &[0.0],
                &[sbx, 63],
                None,
                2,
                3,
                &mut out,
            ),
            // 时隙上界加 ts_offset_hfadj 溢出
            hf_generate(
                ext,
                &patches,
                &filters,
                &[0.0],
                &[sbx, 63],
                None,
                usize::MAX - 1,
                usize::MAX,
                &mut out,
            ),
        ] {
            assert!(error.is_err(), "该输入应被拒绝");
        }
        assert_eq!(out, snapshot, "被拒绝的输入不应改写输出");
    }

    #[test]
    fn noise_table_errors_are_precise_and_atomic() {
        let delay = TnaDelay::new();
        let q_low = [QmfSlot::zero(); N + HFGEN];
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let sbx = 20u8;
        let patches = single_patch(4, 2, sbx);
        let filters = passthrough_filters(8);
        let mut out = [QmfSlot::zero(); 1];
        out[0].re[usize::from(sbx)] = 42.0;
        out[0].im[usize::from(sbx) + 1] = -7.0;
        let snapshot = out;

        let mut expect = |chirp: &[f32], borders: &[u8], expected: HfGenError| {
            assert_eq!(
                hf_generate(
                    ext, &patches, &filters, chirp, borders, None, 2, 3, &mut out
                ),
                Err(expected)
            );
            assert_eq!(out, snapshot, "拒绝噪声表时不应改写输出");
        };

        // 空表与只有交叉边界的表都表示零个噪声组。
        expect(
            &[],
            &[],
            HfGenError::NoiseGroupCountOutOfRange { groups: 0 },
        );
        expect(
            &[],
            &[sbx],
            HfGenError::NoiseGroupCountOutOfRange { groups: 0 },
        );

        let mut too_many_borders = [0u8; MAX_SBG_NOISE as usize + 2];
        for (offset, border) in too_many_borders.iter_mut().enumerate() {
            *border = sbx + u8::try_from(offset).expect("噪声组上限可装入 u8");
        }
        let too_many_chirp = [0.0; MAX_SBG_NOISE as usize + 1];
        expect(
            &too_many_chirp,
            &too_many_borders,
            HfGenError::NoiseGroupCountOutOfRange {
                groups: MAX_SBG_NOISE as usize + 1,
            },
        );

        expect(
            &[0.0, 0.0],
            &[sbx, 63],
            HfGenError::ChirpCountMismatch {
                groups: 1,
                provided: 2,
            },
        );
        expect(
            &[0.0, 0.0],
            &[sbx, sbx, 63],
            HfGenError::NoiseBordersNotIncreasing {
                index: 1,
                previous: sbx,
                current: sbx,
            },
        );
    }

    #[test]
    fn a_crossover_disagreement_between_the_two_tables_is_rejected() {
        // patch 表与噪声表的第 0 项都定义为 sbx；不一致时宁可报错也不挑一个用。
        let delay = TnaDelay::new();
        let q_low = [QmfSlot::zero(); N + HFGEN];
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let patches = single_patch(4, 2, 20);
        let filters = passthrough_filters(8);
        let mut out = [QmfSlot::zero(); 1];
        assert_eq!(
            hf_generate(
                ext,
                &patches,
                &filters,
                &[0.0],
                &[21, 63],
                None,
                2,
                3,
                &mut out
            ),
            Err(HfGenError::CrossoverMismatch {
                patches: 20,
                noise: 21
            })
        );
        assert_eq!(out[0].re[20], 0.0, "被拒绝的输入不应改写输出");
    }

    #[test]
    fn a_source_beyond_the_available_filters_is_rejected() {
        let delay = TnaDelay::new();
        let q_low = [QmfSlot::zero(); N + HFGEN];
        let ext = ExtendedLowBand::new(&delay, &q_low);
        let sbx = 20u8;
        let patches = single_patch(6, 4, sbx);
        let filters = passthrough_filters(8);
        let mut out = [QmfSlot::zero(); 1];
        assert_eq!(
            hf_generate(
                ext,
                &patches,
                &filters,
                &[0.0],
                &[sbx, 63],
                None,
                2,
                3,
                &mut out
            ),
            Err(HfGenError::SourceWithoutFilter {
                source: 8,
                filters: 8
            })
        );
    }
}
