//! A-SPX 预平坦化控制数据（`TS103190-1:v1.4.1:5.7.6.4.1.2`）。
//!
//! `Pseudocode 85` 把低带 `Q_low` 的谱包络在 dB 域拟合成一条三阶多项式，用它
//! 代表整体谱斜率，再把斜率翻成一个增益向量；HF 生成在搬移子带时乘上它的倒数。
//!
//! 三步：逐子带求区间内的平均功率并取 `10·log10(· + 1)`；对 `sbx` 个点做三阶
//! 最小二乘拟合；`gain_vec[sb] = 10^((mean_energy − slope[sb])/20)`。
//!
//! # 拟合在中心化坐标里做
//!
//! 伪码把 `x[i]` 直接取成子带号 `i`，于是正规方程 `AᵀA` 的元素是幂和
//! `S_k = Σ i^k`。实测几何下 `sbx` 在 28…40 之间，`S_6/S_0` 跨 `6,1×10⁷` 到
//! `5,4×10⁸`——矩阵横跨八九个数量级，f64 的 16 位有效数字会被条件数吃掉一大半。
//!
//! 本实现改在 `u_i = (2i − (sbx−1))/(sbx−1) ∈ [−1, 1]` 上拟合，同样的跨度降到
//! 0,17。**这不改变结果**：最小二乘解在数学上唯一，换一组基只换系数的表示，
//! 拟合值 `slope[sb]` 不变。伪码引用的 `polynomial_fit()` 本身是信息性引用
//! （Numerical Recipes，[i.13]），规范没有规定求解路径，只规定它是最小二乘意义
//! 下的拟合。
//!
//! 因此本模块**不公开 `poly_array`**——那四个系数依赖基的选择，公开它等于把一个
//! 实现细节写进接口。规范后续只用 `slope`，它是良定义的。
//!
//! # 判据不依赖求解算法
//!
//! 两条结构性质合起来唯一刻画最小二乘的三阶拟合，且都与怎么解方程无关：
//!
//! - **残差正交**：`Σ_i r_i · u_i^k = 0`，`k = 0…3`。这就是正规方程本身。
//! - **四阶差分为零**：等距点上三阶多项式的四阶差分恒为 0，说明 `slope` 确实
//!   是一条三次曲线的采样，而不是别的什么恰好正交的东西。
//!
//! 少了前者，任意三次曲线都能过关；少了后者，残差正交只说明投影方向对，不说明
//! 投影到了三次多项式空间。

#![expect(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "下标由已核对过的子带数派生，四阶方阵的下标是常量；子带数与时隙数\
              都不超过 64，转 f64 与相减都不会越界；逐子带循环用显式下标比迭代器\
              更贴近伪码的 sb 语义"
)]

use crate::aspx::qmf::{QmfSlot, SUBBANDS};
use macindecode_ac4_bitstream::math::{exp2, log2};

/// 拟合阶数，`Pseudocode 85` 的 `polynomial_order`。
const ORDER: usize = 3;
/// 系数个数。
const TERMS: usize = ORDER + 1;
/// dB 与 2 的幂之间的换算：`10·log10(y) = 10·log2(y)/log2(10)`。
const LOG2_10: f64 = core::f64::consts::LOG2_10;

/// 预平坦化无法完成的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreFlattenError {
    /// 交叉子带数超出 QMF 子带数，或不足以定出三阶多项式。
    UnsupportedCrossover { sbx: usize },
    /// 时隙区间为空或越出输入。
    EmptyInterval { first: usize, last: usize },
    /// 输入时隙不足以覆盖区间。
    IntervalTooShort { needed: usize, provided: usize },
    /// 正规方程的数值分解失败。
    ///
    /// 等距点上的 Vandermonde 满秩，且正规矩阵只由 `sbx` 决定，与输入
    /// 功率无关。因此这是防御内部数值失效的兜底；非有限输入会在出口落到
    /// [`PreFlattenError::NonFiniteGain`]。
    FitFailed { pivot: usize },
    /// 输出的增益不是有限正数。
    NonFiniteGain { subband: usize },
}

/// 逐子带的预平坦化增益，`Pseudocode 85` 的 `gain_vec`。
#[derive(Debug, Clone, Copy)]
pub struct PreFlattenGains {
    gains: [f32; SUBBANDS],
    subbands: u8,
}

impl PreFlattenGains {
    /// 全零结果。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gains: [0.0; SUBBANDS],
            subbands: 0,
        }
    }

    /// 第 `sb` 个子带的增益。
    #[must_use]
    pub fn gain(&self, sb: usize) -> Option<f32> {
        if sb >= usize::from(self.subbands) {
            return None;
        }
        self.gains.get(sb).copied()
    }

    /// 直接铺一组增益，供 `hfgen` 的判据区分「按源子带取」与「按高带取」。
    #[cfg(test)]
    pub(crate) fn fill_for_test(&mut self, values: &[f32]) {
        for (sb, &value) in values.iter().enumerate() {
            self.gains[sb] = value;
        }
        self.subbands = values.len() as u8;
    }

    /// 已填充的子带数，即 `sbx`。
    #[must_use]
    pub const fn subbands(&self) -> u8 {
        self.subbands
    }
}

impl Default for PreFlattenGains {
    fn default() -> Self {
        Self::new()
    }
}

/// 按 `Pseudocode 85` 算出预平坦化增益。
///
/// `low` 是低带 QMF 时隙，`first`/`last` 是本 A-SPX 区间在其中的时隙范围
/// （`atsg_sig[0]·num_ts_in_ats` 到 `atsg_sig[num_atsg_sig]·num_ts_in_ats`），
/// 左闭右开。
///
/// # Errors
///
/// 见 [`PreFlattenError`]。任一条不成立时都不改写 `out`。
pub fn pre_flatten(
    low: &[QmfSlot],
    sbx: usize,
    first: usize,
    last: usize,
    out: &mut PreFlattenGains,
) -> Result<(), PreFlattenError> {
    // 三阶拟合至少要四个点；再少的话正规方程奇异。
    if sbx <= ORDER || sbx > SUBBANDS {
        return Err(PreFlattenError::UnsupportedCrossover { sbx });
    }
    if last <= first {
        return Err(PreFlattenError::EmptyInterval { first, last });
    }
    if last > low.len() {
        return Err(PreFlattenError::IntervalTooShort {
            needed: last,
            provided: low.len(),
        });
    }

    let slots = last - first;
    let mut envelope = [0.0f64; SUBBANDS];
    let mut mean_energy = 0.0f64;
    for sb in 0..sbx {
        let mut power = 0.0f64;
        for slot in &low[first..last] {
            let re = f64::from(slot.re[sb]);
            let im = f64::from(slot.im[sb]);
            power += re * re + im * im;
        }
        // pow_env[sb] = 10·log10(平均功率 + 1)
        let decibels = 10.0 * log2(power / (slots as f64) + 1.0) / LOG2_10;
        envelope[sb] = decibels;
        mean_energy += decibels;
    }
    mean_energy /= sbx as f64;

    let slope = fit_cubic(&envelope[..sbx])?;

    let mut result = PreFlattenGains::new();
    result.subbands = u8::try_from(sbx).unwrap_or(u8::MAX);
    for sb in 0..sbx {
        // gain_vec[sb] = 10^((mean_energy − slope[sb])/20)
        let gain = exp2((mean_energy - slope[sb]) / 20.0 * LOG2_10);
        let narrowed = gain as f32;
        if !narrowed.is_finite() || narrowed <= 0.0 {
            return Err(PreFlattenError::NonFiniteGain { subband: sb });
        }
        result.gains[sb] = narrowed;
    }

    *out = result;
    Ok(())
}

/// 三阶最小二乘拟合，返回逐点的拟合值。
///
/// 在中心化坐标 `u ∈ [−1, 1]` 上建正规方程，用 Cholesky 分解求解——`AᵀA` 对称
/// 正定，Cholesky 比通用消元少一半运算，且主元恒为正，非正即说明输入非有限。
fn fit_cubic(values: &[f64]) -> Result<[f64; SUBBANDS], PreFlattenError> {
    let count = values.len();
    let span = (count - 1) as f64;

    // 正规方程：normal[m][n] = Σ u^(m+n)，right[m] = Σ u^m · y
    let mut normal = [[0.0f64; TERMS]; TERMS];
    let mut right = [0.0f64; TERMS];
    for (index, &value) in values.iter().enumerate() {
        let u = (2.0 * index as f64 - span) / span;
        let mut powers = [1.0f64; TERMS];
        for term in 1..TERMS {
            powers[term] = powers[term - 1] * u;
        }
        for m in 0..TERMS {
            for n in 0..TERMS {
                normal[m][n] += powers[m] * powers[n];
            }
            right[m] += powers[m] * value;
        }
    }

    let coefficients = solve_cholesky(&normal, &right)?;

    let mut fitted = [0.0f64; SUBBANDS];
    for index in 0..count {
        let u = (2.0 * index as f64 - span) / span;
        // Horner，升幂系数倒着来。
        let mut value = coefficients[TERMS - 1];
        let mut term = TERMS - 1;
        while term > 0 {
            term -= 1;
            value = value * u + coefficients[term];
        }
        fitted[index] = value;
    }
    Ok(fitted)
}

/// 解对称正定的 4×4 方程组。
fn solve_cholesky(
    normal: &[[f64; TERMS]; TERMS],
    right: &[f64; TERMS],
) -> Result<[f64; TERMS], PreFlattenError> {
    let mut lower = [[0.0f64; TERMS]; TERMS];
    for row in 0..TERMS {
        for column in 0..=row {
            let mut sum = normal[row][column];
            for back in 0..column {
                sum -= lower[row][back] * lower[column][back];
            }
            if row == column {
                // NaN 时前一半为假、后一半为真，同样落进这里。
                if sum <= 0.0 || !sum.is_finite() {
                    return Err(PreFlattenError::FitFailed { pivot: row });
                }
                lower[row][column] = macindecode_ac4_bitstream::math::sqrt(sum);
            } else {
                lower[row][column] = sum / lower[column][column];
            }
        }
    }
    // 前代
    let mut intermediate = [0.0f64; TERMS];
    for row in 0..TERMS {
        let mut sum = right[row];
        for back in 0..row {
            sum -= lower[row][back] * intermediate[back];
        }
        intermediate[row] = sum / lower[row][row];
    }
    // 回代
    let mut solution = [0.0f64; TERMS];
    let mut row = TERMS;
    while row > 0 {
        row -= 1;
        let mut sum = intermediate[row];
        for forward in (row + 1)..TERMS {
            sum -= lower[forward][row] * solution[forward];
        }
        solution[row] = sum / lower[row][row];
    }
    Ok(solution)
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "下标由同一用例构造的子带数派生，越界即是该用例要报告的失败"
)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec;
    use std::vec::Vec;

    /// 造一组低带时隙，使第 `sb` 个子带的每时隙功率恰为 `power[sb]`。
    ///
    /// **功率在实虚两部之间分摊**（各一半）。最初把它全放在实部、虚部留零，
    /// 结果「漏掉虚部功率」这条注入一条判据都不失败——虚部路径根本没被执行到。
    fn slots(power: &[f64], count: usize) -> Vec<QmfSlot> {
        let mut out = vec![QmfSlot::default(); count];
        for slot in &mut out {
            for (sb, &value) in power.iter().enumerate() {
                let half = macindecode_ac4_bitstream::math::sqrt(value * 0.5) as f32;
                slot.re[sb] = half;
                slot.im[sb] = half;
            }
        }
        out
    }

    /// 只放在实部与只放在虚部的输入必须给出同一份增益。
    ///
    /// `pow_env` 是 `re² + im²`，两部的地位完全对称。这条判据独立于 [`slots`]
    /// 怎么分摊，因此即使那个辅助函数再退化成单边，它也会失败。
    #[test]
    fn the_two_quadratures_contribute_equally() {
        let sbx = 20usize;
        let shape: Vec<f64> = (0..sbx).map(|sb| 1.0e5 * 0.9f64.powi(sb as i32)).collect();
        let mut real_only = vec![QmfSlot::default(); 6];
        let mut imaginary_only = vec![QmfSlot::default(); 6];
        for index in 0..6 {
            for (sb, &value) in shape.iter().enumerate() {
                let amplitude = macindecode_ac4_bitstream::math::sqrt(value) as f32;
                real_only[index].re[sb] = amplitude;
                imaginary_only[index].im[sb] = amplitude;
            }
        }
        let mut from_real = PreFlattenGains::new();
        pre_flatten(&real_only, sbx, 0, 6, &mut from_real).expect("应成功");
        let mut from_imaginary = PreFlattenGains::new();
        pre_flatten(&imaginary_only, sbx, 0, 6, &mut from_imaginary).expect("应成功");
        for sb in 0..sbx {
            assert_eq!(
                from_real.gain(sb),
                from_imaginary.gain(sb),
                "第 {sb} 个子带：实部与虚部应等价"
            );
        }
    }

    fn centred(index: usize, count: usize) -> f64 {
        (2.0 * index as f64 - (count - 1) as f64) / (count - 1) as f64
    }

    /// 残差与设计矩阵的每一列正交——这就是正规方程本身。
    ///
    /// 不依赖求解算法：换 Cholesky 为别的分解、换基、换坐标，这一条都必须成立。
    ///
    /// 幂次上界写成字面量 3，**不引用实现的 `ORDER`/`TERMS`**：两边共用一个常量
    /// 的话，把阶数改成 2 会让检查范围跟着缩到 `u²`，`u³` 那一列的失配就看不见了
    /// ——实测正是如此。3 来自 `Pseudocode 85` 的 `polynomial_order = 3`。
    #[test]
    fn the_residual_is_orthogonal_to_every_column() {
        const SPEC_POLYNOMIAL_ORDER: i32 = 3;
        for count in [8usize, 28, 36, 40] {
            let values: Vec<f64> = (0..count)
                .map(|index| {
                    let x = index as f64;
                    // 一条带噪的曲线，保证不是三次多项式本身。
                    30.0 - 0.4 * x + 0.01 * x * x + 6.0 * (x * 0.7).sin()
                })
                .collect();
            let fitted = fit_cubic(&values).expect("拟合应成功");
            let scale: f64 = values.iter().map(|v| v.abs()).sum();
            for power in 0..=SPEC_POLYNOMIAL_ORDER {
                let mut projection = 0.0f64;
                for index in 0..count {
                    let u = centred(index, count);
                    projection += (values[index] - fitted[index]) * u.powi(power);
                }
                assert!(
                    projection.abs() < 1e-12 * scale,
                    "count={count} 的残差在 u^{power} 上的投影为 {projection}"
                );
            }
        }
    }

    /// 拟合值的四阶差分为零，即它确实落在三次多项式空间里。
    #[test]
    fn the_fitted_curve_is_a_cubic() {
        for count in [8usize, 28, 40] {
            let values: Vec<f64> = (0..count)
                .map(|index| 12.0 + (index as f64 * 0.9).cos() * 20.0)
                .collect();
            let fitted = fit_cubic(&values).expect("拟合应成功");
            let scale = fitted[..count].iter().fold(0.0f64, |a, v| a.max(v.abs()));
            for index in 0..count.saturating_sub(4) {
                let fourth = fitted[index + 4] - 4.0 * fitted[index + 3] + 6.0 * fitted[index + 2]
                    - 4.0 * fitted[index + 1]
                    + fitted[index];
                assert!(
                    fourth.abs() < 1e-9 * scale.max(1.0),
                    "count={count} 第 {index} 处的四阶差分为 {fourth}"
                );
            }
        }
    }

    /// 输入本身是三次多项式时必须精确还原。
    #[test]
    fn a_cubic_input_is_reproduced() {
        let count = 32usize;
        let values: Vec<f64> = (0..count)
            .map(|index| {
                let u = centred(index, count);
                7.5 - 3.25 * u + 1.75 * u * u - 0.5 * u * u * u
            })
            .collect();
        let fitted = fit_cubic(&values).expect("拟合应成功");
        for index in 0..count {
            assert!(
                (fitted[index] - values[index]).abs() < 1e-11,
                "第 {index} 点：拟合 {} 与输入 {} 不符",
                fitted[index],
                values[index]
            );
        }
    }

    /// 谱包络平坦时增益恒为 1。
    #[test]
    fn a_flat_envelope_gives_unit_gain() {
        let sbx = 32usize;
        let power = vec![4.0f64; sbx];
        let low = slots(&power, 12);
        let mut out = PreFlattenGains::new();
        pre_flatten(&low, sbx, 0, 12, &mut out).expect("应成功");
        for sb in 0..sbx {
            let gain = out.gain(sb).expect("增益");
            assert!(
                (f64::from(gain) - 1.0).abs() < 1e-6,
                "第 {sb} 个子带的增益为 {gain}，应为 1"
            );
        }
    }

    /// 谱包络整体抬高不改变增益。
    ///
    /// `mean_energy` 与 `slope` 一起平移，两者之差不变——这是"应当无关"型判据：
    /// 它不检查数值本身，而是检查结果对一整类输入变化不敏感。若误把 `mean` 写成
    /// 常数、或漏掉 `mean − slope` 里的任一项，这一条立刻失败，而逐值比对要先
    /// 有一份正确的期望值才行。
    ///
    /// **只在功率远大于 1 时成立**：`10·log10(· + 1)` 里的 `+1` 是规范给的地板，
    /// 功率落到个位数时它把 dB 曲线压平，平移就不再是平移。最初用最小功率约 6
    /// 的包络写这条判据，实测偏差 2,1 %——那是规范的行为，不是实现的缺陷。地板
    /// 本身由 [`the_floor_keeps_a_silent_band_finite`] 单独覆盖。
    #[test]
    fn a_uniform_level_shift_leaves_the_gain_unchanged() {
        let sbx = 36usize;
        // 一条有斜率的包络，保证 slope 不是常数；最小值取 10⁴，使 +1 可忽略。
        let shape: Vec<f64> = (0..sbx)
            .map(|sb| 1.0e6 * 0.82f64.powi(sb as i32) + 1.0e4)
            .collect();
        let mut baseline = PreFlattenGains::new();
        pre_flatten(&slots(&shape, 10), sbx, 0, 10, &mut baseline).expect("应成功");

        // 功率整体乘 100，dB 域即整体加 20。
        let lifted: Vec<f64> = shape.iter().map(|value| value * 100.0).collect();
        let mut shifted = PreFlattenGains::new();
        pre_flatten(&slots(&lifted, 10), sbx, 0, 10, &mut shifted).expect("应成功");

        for sb in 0..sbx {
            let before = f64::from(baseline.gain(sb).expect("增益"));
            let after = f64::from(shifted.gain(sb).expect("增益"));
            let relative = ((after - before) / before).abs();
            assert!(
                relative < 1e-4,
                "第 {sb} 个子带：抬高后增益从 {before} 变成 {after}"
            );
        }
    }

    /// `+1` 地板让静音子带给出有限增益，而不是除零或无穷。
    ///
    /// 这是伪码里 `10·log10(pow_env + 1)` 那个加一的用途：没有它，全静音的低带会
    /// 让 `log10(0)` 变成 `−∞`，增益随即溢出。有了它，整段静音退化成平坦包络，
    /// 增益恰为 1。
    #[test]
    fn the_floor_keeps_a_silent_band_finite() {
        let sbx = 24usize;
        let silent = vec![0.0f64; sbx];
        let mut out = PreFlattenGains::new();
        pre_flatten(&slots(&silent, 6), sbx, 0, 6, &mut out).expect("静音应可处理");
        for sb in 0..sbx {
            let gain = out.gain(sb).expect("增益");
            assert!(gain.is_finite(), "第 {sb} 个子带的增益应有限");
            assert!(
                (f64::from(gain) - 1.0).abs() < 1e-9,
                "全静音应退化成单位增益，实得 {gain}"
            );
        }

        // 只有一个子带静音时，地板把它的 dB 钉在 0 而不是 −∞。
        let mut mixed = vec![1.0e5f64; sbx];
        mixed[7] = 0.0;
        let mut out = PreFlattenGains::new();
        pre_flatten(&slots(&mixed, 6), sbx, 0, 6, &mut out).expect("应成功");
        for sb in 0..sbx {
            assert!(
                out.gain(sb).expect("增益").is_finite(),
                "第 {sb} 个子带的增益应有限"
            );
        }
    }

    /// 增益随谱斜率反向：能量高的子带得到小增益。
    #[test]
    fn a_falling_spectrum_gives_a_rising_gain() {
        let sbx = 32usize;
        let shape: Vec<f64> = (0..sbx)
            .map(|sb| 10_000.0 * 0.75f64.powi(sb as i32))
            .collect();
        let mut out = PreFlattenGains::new();
        pre_flatten(&slots(&shape, 8), sbx, 0, 8, &mut out).expect("应成功");
        let first = out.gain(0).expect("增益");
        let last = out.gain(sbx - 1).expect("增益");
        assert!(
            first < last,
            "低子带能量高，增益 {first} 应小于高子带的 {last}"
        );
        // 增益是斜率的倒数，因此应当单调。
        for sb in 1..sbx {
            let previous = out.gain(sb - 1).expect("增益");
            let current = out.gain(sb).expect("增益");
            assert!(current > previous, "第 {sb} 个子带的增益应继续上升");
        }
    }

    /// 时隙区间只统计给定范围。
    ///
    /// 判据是**逐位相同**：取后半段的结果，必须与把后半段单独作为输入算出的结果
    /// 完全一致。先前写成「后半段应给出非单位增益」，注入「区间从 0 起而非
    /// `first`」时一条判据都不失败——混进前半段之后仍然有斜率，那个弱断言照样
    /// 成立。
    #[test]
    fn only_the_requested_slots_contribute() {
        let sbx = 16usize;
        let quiet = vec![1.0e4f64; sbx];
        let loud: Vec<f64> = (0..sbx).map(|sb| 1.0e6 * (sb as f64 + 1.0)).collect();
        let mut combined = slots(&quiet, 4);
        combined.extend(slots(&loud, 4));

        let mut windowed = PreFlattenGains::new();
        pre_flatten(&combined, sbx, 4, 8, &mut windowed).expect("应成功");
        let mut isolated = PreFlattenGains::new();
        pre_flatten(&slots(&loud, 4), sbx, 0, 4, &mut isolated).expect("应成功");

        for sb in 0..sbx {
            assert_eq!(
                windowed.gain(sb),
                isolated.gain(sb),
                "第 {sb} 个子带：窗口内的结果应与单独输入逐位相同"
            );
        }

        // 前半段平坦，两段的结果必须不同，否则上面的相等是平凡的。
        let mut early = PreFlattenGains::new();
        pre_flatten(&combined, sbx, 0, 4, &mut early).expect("应成功");
        assert_ne!(
            early.gain(0),
            windowed.gain(0),
            "两段的增益应当不同，否则区间判据是平凡的"
        );
    }

    /// 入口拒绝越界与退化的输入。
    #[test]
    fn invalid_input_is_rejected_without_touching_the_output() {
        let sbx = 20usize;
        let power: Vec<f64> = (0..sbx).map(|sb| 8.0 + sb as f64 * 3.0).collect();
        let low = slots(&power, 6);
        let mut out = PreFlattenGains::new();
        pre_flatten(&low, sbx, 0, 6, &mut out).expect("哨兵结果应可生成");
        let snapshot = out;

        let assert_unchanged = |actual: &PreFlattenGains| {
            assert_eq!(actual.subbands, snapshot.subbands, "已填充子带数被改写");
            for sb in 0..SUBBANDS {
                assert_eq!(
                    actual.gains[sb].to_bits(),
                    snapshot.gains[sb].to_bits(),
                    "第 {sb} 个子带的哨兵增益被改写"
                );
            }
        };

        assert_eq!(
            pre_flatten(&low, ORDER, 0, 6, &mut out),
            Err(PreFlattenError::UnsupportedCrossover { sbx: ORDER }),
            "三阶拟合至少要四个点"
        );
        assert_unchanged(&out);
        assert_eq!(
            pre_flatten(&low, SUBBANDS + 1, 0, 6, &mut out),
            Err(PreFlattenError::UnsupportedCrossover { sbx: SUBBANDS + 1 })
        );
        assert_unchanged(&out);
        assert_eq!(
            pre_flatten(&low, sbx, 3, 3, &mut out),
            Err(PreFlattenError::EmptyInterval { first: 3, last: 3 })
        );
        assert_unchanged(&out);
        assert_eq!(
            pre_flatten(&low, sbx, 0, 9, &mut out),
            Err(PreFlattenError::IntervalTooShort {
                needed: 9,
                provided: 6
            })
        );
        assert_unchanged(&out);

        let mut non_finite = low;
        non_finite[0].re[0] = f32::NAN;
        assert_eq!(
            pre_flatten(&non_finite, sbx, 0, 6, &mut out),
            Err(PreFlattenError::NonFiniteGain { subband: 0 })
        );
        assert_unchanged(&out);
    }

    /// 四个点恰好定出三阶多项式，是最小的合法输入。
    #[test]
    fn the_smallest_admissible_crossover_is_four() {
        let sbx = ORDER + 1;
        let power: Vec<f64> = vec![1.0, 8.0, 3.0, 20.0];
        let low = slots(&power, 5);
        let mut out = PreFlattenGains::new();
        pre_flatten(&low, sbx, 0, 5, &mut out).expect("四个点应可拟合");
        // 点数等于自由度，拟合必然过每一点，故残差为零、增益由 mean 决定。
        let envelope: Vec<f64> = power
            .iter()
            .map(|value| 10.0 * log2(value + 1.0) / LOG2_10)
            .collect();
        let mean = envelope.iter().sum::<f64>() / sbx as f64;
        for sb in 0..sbx {
            let expected = exp2((mean - envelope[sb]) / 20.0 * LOG2_10);
            let actual = f64::from(out.gain(sb).expect("增益"));
            assert!(
                ((actual - expected) / expected).abs() < 1e-6,
                "第 {sb} 个子带：{actual} 应为 {expected}"
            );
        }
    }
}
