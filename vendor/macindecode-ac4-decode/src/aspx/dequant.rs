//! A-SPX 标度因子的反量化与立体声解码（`TS103190-1:v1.4.1:5.7.6.3.5`）。
//!
//! `Pseudocode 82`（信号）、`Pseudocode 83`（噪声）与 `Pseudocode 84`（平衡式
//! 立体声）把 `5.7.6.3.4` 解出的量化标度因子 `qscf` 变成线性标度因子 `scf`。
//!
//! # 标度因子是能量，不是幅度
//!
//! 规范 `3.1` 把 signal scale factor 定义为「average energy of the signal within
//! the region in a QMF matrix」。这一条决定了两档量化步长怎么对上：
//! `scf = 2^(qscf/a)` 在能量域取 `10·log10`，`a = 2` 得 1,505 dB/步，`a = 1` 得
//! 3,010 dB/步，正是 `aspx_qmode_env` 的两档。若按幅度域取 `20·log10`，两档会
//! 变成 3 dB 与 6 dB，与明文不符。
//!
//! 由此还能定死一件伪码没写明的事：**`qscf/a` 是实数除法**。整数除法会让
//! `a = 2` 时相邻的奇偶 `qscf` 撞在一起，1,5 dB 档退化成 3 dB 档，两档步长就不
//! 再有区别。
//!
//! # 不需要通用的 `pow`
//!
//! 指数只可能是整数或半整数，因此 `2^n` 直接由 f32 的指数域构造，半整数部分乘
//! 一个 `√2`。这既避免了目标侧的数学库依赖，也比通用幂函数精确：整数部分完全
//! 无误差，半整数部分只带 `√2` 一次舍入。
//!
//! 指数落到 f32 正规数之外一律报错。上溢会变成 `inf` 并顺着 HF 生成污染整个
//! QMF 域，下溢到次正规则悄悄丢精度；`5.7.6.3.5` 两者都没有定义，不该由本层
//! 静默产生。

#![allow(
    clippy::indexing_slicing,
    reason = "下标由已核对过的包络数与子带组数派生，两条伪码用显式下标比迭代器\
              更贴近原文"
)]

use core::f32::consts::SQRT_2;

use crate::aspx::envelope::EnvelopeScaleFactors;
use crate::aspx::frames::{MAX_ATSG_NOISE, MAX_ATSG_SIG};
use crate::aspx::syntax::MAX_SBG_PER_ENVELOPE;
use crate::aspx::tables::{EnvelopeKind, MAX_SBG_NOISE};

/// 噪声子带组数的上限。
const NOISE_GROUPS: usize = MAX_SBG_NOISE as usize;

/// `NOISE_FLOOR_OFFSET`，见 `Pseudocode 83`。
const NOISE_FLOOR_OFFSET: i32 = 6;

/// `PAN_OFFSET`，见 `Pseudocode 84`。
const PAN_OFFSET: i32 = 12;

/// `num_qmf_subbands` 恒为 64 = 2^6，见 `5.7.3`。
///
/// 伪码写成 `num_qmf_subbands * pow(2, ...)`，这里并进指数一起算：乘 2^6 只改
/// 指数，两种写法在浮点下逐位相同，但并进去能少一次中间上溢。
const QMF_SUBBANDS_LOG2: i32 = 6;

/// f32 正规数的指数范围。
const MIN_EXPONENT: i32 = -126;
/// f32 正规数的指数上界。
const MAX_EXPONENT: i32 = 127;

/// 反量化无法完成的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DequantError {
    /// 指数超出 f32 正规数范围。
    ExponentOutOfRange {
        kind: EnvelopeKind,
        envelope: usize,
        group: usize,
    },
    /// 平衡式立体声的两个声道时频网格不一致。
    ///
    /// `5.7.6.3.5` 要求 `aspx_balance = 1` 时两声道的 `atsg_sig` 与 `atsg_noise`
    /// 相同，逐组配对才有意义。
    GridMismatch { kind: EnvelopeKind, envelope: usize },
}

/// 一个声道反量化后的线性标度因子。
#[derive(Debug, Clone, Copy)]
pub struct ScaleFactors {
    sig: [[f32; MAX_SBG_PER_ENVELOPE]; MAX_ATSG_SIG],
    sig_groups: [u8; MAX_ATSG_SIG],
    num_sig: u8,
    noise: [[f32; NOISE_GROUPS]; MAX_ATSG_NOISE],
    noise_groups: u8,
    num_noise: u8,
}

impl ScaleFactors {
    /// 铺一组常量标度因子，供 `hfadjust` 的判据隔离包络映射。
    ///
    /// 不跑 `Pseudocode 82`–`84`——那条路径由本模块自己的判据覆盖。
    #[cfg(test)]
    pub(crate) fn fill_for_test(
        &mut self,
        envelopes: usize,
        sig_groups: usize,
        noise_groups: usize,
        sig: f32,
        noise: f32,
    ) {
        for env in 0..envelopes.min(MAX_ATSG_SIG) {
            for group in 0..sig_groups.min(MAX_SBG_PER_ENVELOPE) {
                self.sig[env][group] = sig;
            }
            self.sig_groups[env] = sig_groups as u8;
        }
        self.num_sig = envelopes as u8;
        for env in 0..MAX_ATSG_NOISE {
            for group in 0..noise_groups.min(NOISE_GROUPS) {
                self.noise[env][group] = noise;
            }
        }
        self.noise_groups = noise_groups as u8;
        self.num_noise = MAX_ATSG_NOISE as u8;
    }

    /// 全零结果。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sig: [[0.0; MAX_SBG_PER_ENVELOPE]; MAX_ATSG_SIG],
            sig_groups: [0; MAX_ATSG_SIG],
            num_sig: 0,
            noise: [[0.0; NOISE_GROUPS]; MAX_ATSG_NOISE],
            noise_groups: 0,
            num_noise: 0,
        }
    }

    /// 第 `env` 个信号包络第 `sbg` 组的 `scf_sig_sbg`。
    #[must_use]
    pub fn sig(&self, env: usize, sbg: usize) -> Option<f32> {
        if env >= usize::from(self.num_sig) || sbg >= usize::from(*self.sig_groups.get(env)?) {
            return None;
        }
        self.sig.get(env)?.get(sbg).copied()
    }

    /// 第 `env` 个噪声包络第 `sbg` 组的 `scf_noise_sbg`。
    #[must_use]
    pub fn noise(&self, env: usize, sbg: usize) -> Option<f32> {
        if env >= usize::from(self.num_noise) || sbg >= usize::from(self.noise_groups) {
            return None;
        }
        self.noise.get(env)?.get(sbg).copied()
    }

    /// 信号与噪声包络数。
    #[must_use]
    pub const fn counts(&self) -> (u8, u8) {
        (self.num_sig, self.num_noise)
    }
}

impl Default for ScaleFactors {
    fn default() -> Self {
        Self::new()
    }
}

/// `pow(2, n)`，`n` 为整数。
///
/// 结果不是正规数时返回 `None`。指数域直接构造，无舍入。
fn exp2i(n: i32) -> Option<f32> {
    if !(MIN_EXPONENT..=MAX_EXPONENT).contains(&n) {
        return None;
    }
    let biased = u32::try_from(n.checked_add(MAX_EXPONENT)?).ok()?;
    Some(f32::from_bits(biased << 23))
}

/// `pow(2, shift + qscf/a)`，`a` 由 `coarse_quant` 选出。
///
/// `coarse_quant` 为真时 `a = 1`（3 dB 档），指数是整数；为假时 `a = 2`
/// （1,5 dB 档），指数是半整数。算术右移即向下取整除，余数只可能是 0 或 1，
/// 因此半整数部分恒为一个 `√2`。
fn exp2_quantised(qscf: i16, coarse_quant: bool, shift: i32) -> Option<f32> {
    let qscf = i32::from(qscf);
    if coarse_quant {
        return exp2i(shift.checked_add(qscf)?);
    }
    let floor = qscf >> 1;
    let base = exp2i(shift.checked_add(floor)?)?;
    if qscf == floor.checked_mul(2)? {
        return Some(base);
    }
    let value = base * SQRT_2;
    value.is_finite().then_some(value)
}

/// `1 + pow(2, exponent / 2)`，用于 `Pseudocode 84` 的两个分母。
///
/// 用两倍指数表示整数与半整数，避免可表示的 `2^127,5` 先构造不可表示的
/// `2^128` 再乘 `1/√2`。指数很负时幂项被 1 吸收，分母恰好是 1；只有上溢
/// 才拒绝。
fn one_plus_exp2_half(twice_exponent: i32) -> Option<f32> {
    if twice_exponent < MIN_EXPONENT.checked_mul(2)? {
        return Some(1.0);
    }
    let floor = twice_exponent.div_euclid(2);
    let mut power = exp2i(floor)?;
    if twice_exponent.rem_euclid(2) != 0 {
        power *= SQRT_2;
    }
    let value = 1.0 + power;
    value.is_finite().then_some(value)
}

/// 做除法并只接受正正规结果。
fn divide_normal(numerator: f32, denominator: f32) -> Option<f32> {
    let value = numerator / denominator;
    (value.is_sign_positive() && value.is_normal()).then_some(value)
}

/// `Pseudocode 82` 与 `Pseudocode 83`：单声道或 `aspx_balance = 0` 的反量化。
///
/// `coarse_quant` 是 `aspx_qmode_env[ch]`：假为 1,5 dB，真为 3 dB。
///
/// # Errors
///
/// 指数超出 f32 正规数范围时返回 [`DequantError::ExponentOutOfRange`]，此时
/// 不改写 `out`。
pub fn dequantise(
    qscf: &EnvelopeScaleFactors,
    coarse_quant: bool,
    out: &mut ScaleFactors,
) -> Result<(), DequantError> {
    let (num_sig, num_noise) = qscf.counts();
    let mut result = ScaleFactors::new();
    result.num_sig = num_sig;
    result.num_noise = num_noise;
    result.noise_groups = qscf.noise_group_count();

    for envelope in 0..usize::from(num_sig) {
        let groups = qscf.sig_group_count(envelope).unwrap_or(0);
        result.sig_groups[envelope] = groups;
        for group in 0..usize::from(groups) {
            let quantised = qscf.sig(envelope, group).unwrap_or(0);
            // scf_sig_sbg = num_qmf_subbands * pow(2, qscf/a)
            result.sig[envelope][group] =
                exp2_quantised(quantised, coarse_quant, QMF_SUBBANDS_LOG2).ok_or(
                    DequantError::ExponentOutOfRange {
                        kind: EnvelopeKind::Signal,
                        envelope,
                        group,
                    },
                )?;
        }
    }

    for envelope in 0..usize::from(num_noise) {
        for group in 0..usize::from(result.noise_groups) {
            let quantised = i32::from(qscf.noise(envelope, group).unwrap_or(0));
            // scf_noise_sbg = pow(2, NOISE_FLOOR_OFFSET - qscf)
            let exponent = NOISE_FLOOR_OFFSET.checked_sub(quantised);
            result.noise[envelope][group] =
                exponent
                    .and_then(exp2i)
                    .ok_or(DequantError::ExponentOutOfRange {
                        kind: EnvelopeKind::Noise,
                        envelope,
                        group,
                    })?;
        }
    }

    *out = result;
    Ok(())
}

/// `Pseudocode 84`：`aspx_balance = 1` 时两个声道联合反量化。
///
/// `sum` 是和声道 A 的量化标度因子，`balance` 是平衡声道 B 的。两者的时频网格
/// 必须一致——`5.7.6.3.5` 明文要求 `atsg_sig` 与 `atsg_noise` 在两声道相同。
///
/// 分母满足 `1/denom_a + 1/denom_b = 1`，因此 `scf_a + scf_b = nom` 恒成立：
/// 两声道的能量之和只由和声道的 `qscf` 决定，平衡声道只决定怎么分。
///
/// # Errors
///
/// 网格不一致返回 [`DequantError::GridMismatch`]，指数越界返回
/// [`DequantError::ExponentOutOfRange`]；两种情况都不改写输出。
pub fn dequantise_pair(
    sum: &EnvelopeScaleFactors,
    balance: &EnvelopeScaleFactors,
    coarse_quant: bool,
    out_sum: &mut ScaleFactors,
    out_balance: &mut ScaleFactors,
) -> Result<(), DequantError> {
    let (num_sig, num_noise) = sum.counts();
    let (balance_sig, balance_noise) = balance.counts();
    if num_sig != balance_sig {
        return Err(DequantError::GridMismatch {
            kind: EnvelopeKind::Signal,
            envelope: 0,
        });
    }
    if num_noise != balance_noise {
        return Err(DequantError::GridMismatch {
            kind: EnvelopeKind::Noise,
            envelope: 0,
        });
    }
    if sum.noise_group_count() != balance.noise_group_count() {
        return Err(DequantError::GridMismatch {
            kind: EnvelopeKind::Noise,
            envelope: 0,
        });
    }

    let mut result_sum = ScaleFactors::new();
    let mut result_balance = ScaleFactors::new();
    for target in [&mut result_sum, &mut result_balance] {
        target.num_sig = num_sig;
        target.num_noise = num_noise;
        target.noise_groups = sum.noise_group_count();
    }

    for envelope in 0..usize::from(num_sig) {
        let groups = sum.sig_group_count(envelope).unwrap_or(0);
        if groups != balance.sig_group_count(envelope).unwrap_or(0) {
            return Err(DequantError::GridMismatch {
                kind: EnvelopeKind::Signal,
                envelope,
            });
        }
        result_sum.sig_groups[envelope] = groups;
        result_balance.sig_groups[envelope] = groups;
        for group in 0..usize::from(groups) {
            let out_of_range = DequantError::ExponentOutOfRange {
                kind: EnvelopeKind::Signal,
                envelope,
                group,
            };
            let quantised_sum = sum.sig(envelope, group).unwrap_or(0);
            let quantised_balance = balance.sig(envelope, group).unwrap_or(0);
            // nom = pow(2, qscf_a/a + 1) * num_qmf_subbands
            let nom = exp2_quantised(quantised_sum, coarse_quant, QMF_SUBBANDS_LOG2 + 1)
                .ok_or(out_of_range)?;
            let (denom_sum, denom_balance) =
                pan_denominators(quantised_balance, coarse_quant, out_of_range)?;
            result_sum.sig[envelope][group] = divide_normal(nom, denom_sum).ok_or(out_of_range)?;
            result_balance.sig[envelope][group] =
                divide_normal(nom, denom_balance).ok_or(out_of_range)?;
        }
    }

    let noise_groups = usize::from(sum.noise_group_count());
    for envelope in 0..usize::from(num_noise) {
        for group in 0..noise_groups {
            let out_of_range = DequantError::ExponentOutOfRange {
                kind: EnvelopeKind::Noise,
                envelope,
                group,
            };
            let quantised_sum = i32::from(sum.noise(envelope, group).unwrap_or(0));
            let quantised_balance = balance.noise(envelope, group).unwrap_or(0);
            // nom = pow(2, NOISE_FLOOR_OFFSET - qscf_noise_a + 1)
            let exponent = NOISE_FLOOR_OFFSET
                .checked_add(1)
                .and_then(|value| value.checked_sub(quantised_sum));
            let nom = exponent.and_then(exp2i).ok_or(out_of_range)?;
            // 噪声侧没有量化步长这一维，分母不除以 a。
            let (denom_sum, denom_balance) =
                pan_denominators(quantised_balance, true, out_of_range)?;
            result_sum.noise[envelope][group] =
                divide_normal(nom, denom_sum).ok_or(out_of_range)?;
            result_balance.noise[envelope][group] =
                divide_normal(nom, denom_balance).ok_or(out_of_range)?;
        }
    }

    *out_sum = result_sum;
    *out_balance = result_balance;
    Ok(())
}

/// `Pseudocode 84` 的两个分母。
///
/// `denom_a = 1 + pow(2, PAN_OFFSET - qscf_b/a)`，
/// `denom_b = 1 + pow(2, qscf_b/a - PAN_OFFSET)`。
///
/// 半整数指数用两倍指数表示，既保留正负方向，也不引入超出 `f32` 的整数幂中间值。
fn pan_denominators(
    quantised: i16,
    coarse_quant: bool,
    out_of_range: DequantError,
) -> Result<(f32, f32), DequantError> {
    let quantised = i32::from(quantised);
    let twice_quantised = if coarse_quant {
        quantised.checked_mul(2).ok_or(out_of_range)?
    } else {
        quantised
    };
    let twice_pan = PAN_OFFSET.checked_mul(2).ok_or(out_of_range)?;
    let sum_exponent = twice_pan.checked_sub(twice_quantised).ok_or(out_of_range)?;
    let balance_exponent = twice_quantised.checked_sub(twice_pan).ok_or(out_of_range)?;
    let denom_sum = one_plus_exp2_half(sum_exponent).ok_or(out_of_range)?;
    let denom_balance = one_plus_exp2_half(balance_exponent).ok_or(out_of_range)?;
    Ok((denom_sum, denom_balance))
}

#[cfg(test)]
#[expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "下标由同一用例构造的包络数与组数派生，越界即是该用例要报告的失败；\
              构造 qscf 的差分要精确相减，饱和或回绕都会让用例失去意义"
)]
mod tests {
    extern crate std;

    use super::*;
    use crate::aspx::bands::AspxBandTables;
    use crate::aspx::envelope::{EnvelopeDeltas, EnvelopeHistory, SbgIndexMap, decode};
    use std::vec;
    use std::vec::Vec;

    fn tables() -> AspxBandTables {
        AspxBandTables::derive(true, 0, 0, 1, 0).expect("测试频带表应可推导")
    }

    /// 用频率方向的前缀和造出一份指定的 `qscf`。
    ///
    /// 频率方向是前缀和，因此逐组差分取相邻目标值之差即可精确命中。
    fn quantised(target: &[i16], noise_target: &[i16]) -> EnvelopeScaleFactors {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let mut sig_deltas = Vec::new();
        let mut previous = 0i16;
        for &value in target {
            sig_deltas.push(value - previous);
            previous = value;
        }
        let mut noise_deltas = Vec::new();
        previous = 0;
        for &value in noise_target {
            noise_deltas.push(value - previous);
            previous = value;
        }
        let mut history = EnvelopeHistory::new();
        let mut out = EnvelopeScaleFactors::new();
        decode(
            &[EnvelopeDeltas {
                data: &sig_deltas,
                time_direction: false,
                high_resolution: true,
            }],
            &[EnvelopeDeltas {
                data: &noise_deltas,
                time_direction: false,
                high_resolution: false,
            }],
            &map,
            false,
            &mut history,
            &mut out,
        )
        .expect("构造应成功");
        out
    }

    fn high_groups() -> usize {
        usize::from(tables().num_sbg_sig_highres())
    }

    fn noise_groups() -> usize {
        usize::from(tables().num_sbg_noise())
    }

    /// `2^n` 必须与逐次乘二得到的结果逐位相同。
    ///
    /// 指数域构造是本模块唯一的幂运算来源；把它与一个不依赖位模式的朴素累乘
    /// 对照，能抓住偏置常数写错、移位位数写错这类问题。
    #[test]
    fn the_power_of_two_matches_repeated_multiplication() {
        for n in -60..=60i32 {
            let mut expected = 1.0f32;
            for _ in 0..n.abs() {
                if n > 0 {
                    expected *= 2.0;
                } else {
                    expected /= 2.0;
                }
            }
            assert_eq!(exp2i(n), Some(expected), "2^{n} 应与累乘一致");
        }
        assert_eq!(exp2i(-126), Some(f32::MIN_POSITIVE), "最小正规数");
        assert_eq!(exp2i(-127), None, "次正规应被拒绝");
        assert_eq!(exp2i(128), None, "上溢应被拒绝");
        assert!(exp2i(127).is_some_and(f32::is_finite), "最大正规指数应可用");
    }

    /// 两档量化步长必须真的是 1,5 dB 与 3,0 dB。
    ///
    /// 这一条同时钉住三件事：标度因子在能量域（否则是 3 dB 与 6 dB）、
    /// `qscf/a` 是实数除法（整数除法会让 `a = 2` 时相邻奇偶值撞在一起）、
    /// 以及 `a` 的两档没有取反。
    #[test]
    fn the_two_quantisation_steps_are_one_and_a_half_and_three_decibels() {
        let ratio_in_decibels = |from: f32, to: f32| 10.0 * (f64::from(to / from)).log10();

        for (coarse_quant, expected) in [(false, 1.505_15f64), (true, 3.010_3)] {
            for quantised in -20..20i16 {
                let low = exp2_quantised(quantised, coarse_quant, 0).expect("应在范围内");
                let high = exp2_quantised(quantised + 1, coarse_quant, 0).expect("应在范围内");
                let step = ratio_in_decibels(low, high);
                assert!(
                    (step - expected).abs() < 1e-3,
                    "qscf {quantised} → {} 的步长应是 {expected} dB，实得 {step}",
                    quantised + 1
                );
            }
        }
    }

    /// 1,5 dB 档的相邻量化值不得相等——整数除法会让它们相等。
    #[test]
    fn the_fine_step_separates_odd_and_even_quantised_values() {
        for quantised in -30..30i16 {
            let low = exp2_quantised(quantised, false, 0).expect("范围内");
            let high = exp2_quantised(quantised + 1, false, 0).expect("范围内");
            assert!(low < high, "qscf {quantised} 与其后继应严格递增");
        }
        // 半整数指数确实是 √2 倍，而不是被截断成同一档。
        let unit = exp2_quantised(0, false, 0).expect("范围内");
        let half = exp2_quantised(1, false, 0).expect("范围内");
        assert_eq!(half, unit * SQRT_2, "奇数 qscf 应恰好差一个 √2");
    }

    /// `Pseudocode 82`/`83` 的两条公式。
    #[test]
    fn the_dequantised_factors_follow_the_two_formulas() {
        let high = high_groups();
        let target: Vec<i16> = (0..high).map(|index| (index as i16) % 7 - 3).collect();
        let noise_target: Vec<i16> = vec![2, -1, 0, 3, 1];
        let noise_groups = noise_groups();
        let quantised = quantised(&target, &noise_target[..noise_groups]);

        let mut out = ScaleFactors::new();
        dequantise(&quantised, true, &mut out).expect("反量化应成功");
        for group in 0..high {
            let expected = exp2i(6 + i32::from(target[group])).expect("范围内");
            assert_eq!(
                out.sig(0, group),
                Some(expected),
                "信号第 {group} 组应是 64 · 2^qscf"
            );
        }
        for group in 0..noise_groups {
            let expected = exp2i(6 - i32::from(noise_target[group])).expect("范围内");
            assert_eq!(
                out.noise(0, group),
                Some(expected),
                "噪声第 {group} 组应是 2^(6 − qscf)"
            );
        }
    }

    /// 标度因子恒为正，因此 `Pseudocode 82` 的第三个条件恒假。
    ///
    /// 伪码里 `scf_sig_sbg[1][atsg] < 0` 这一支永远进不去：
    /// `scf = 64 · 2^x` 中两个因子都是正的。本用例把这个恒等式钉住——它一旦
    /// 失败，就说明标度因子换了表示（例如改用有符号的对数域），那条分支需要
    /// 重新审视，而不是继续省略。
    #[test]
    fn every_dequantised_factor_is_strictly_positive() {
        let high = high_groups();
        let noise_groups = noise_groups();
        for base in [-40i16, -7, 0, 5, 41] {
            let target: Vec<i16> = (0..high).map(|index| base + (index as i16)).collect();
            let noise_target: Vec<i16> =
                (0..noise_groups).map(|index| base + index as i16).collect();
            let quantised = quantised(&target, &noise_target);
            for coarse_quant in [false, true] {
                let mut out = ScaleFactors::new();
                dequantise(&quantised, coarse_quant, &mut out).expect("反量化应成功");
                for group in 0..high {
                    let value = out.sig(0, group).expect("值");
                    assert!(value > 0.0 && value.is_finite(), "信号标度因子应为正有限值");
                }
                for group in 0..noise_groups {
                    let value = out.noise(0, group).expect("值");
                    assert!(value > 0.0 && value.is_finite(), "噪声标度因子应为正有限值");
                }
            }
        }
    }

    /// 平衡式立体声把能量分成两份，总和只由和声道决定。
    ///
    /// `1/denom_a + 1/denom_b = 1` 是 `Pseudocode 84` 的代数恒等式，于是
    /// `scf_a + scf_b = nom = 2 · scf_mono(qscf_a)`。这条判据跨两段伪码：右端
    /// 完全由 `Pseudocode 82` 算出，与平衡声道的取值无关。
    #[test]
    fn the_stereo_pair_conserves_the_sum_channel_energy() {
        let high = high_groups();
        let noise_groups = noise_groups();
        let sum_target: Vec<i16> = (0..high).map(|index| (index as i16) % 5 - 2).collect();
        let noise_sum: Vec<i16> = (0..noise_groups).map(|index| index as i16 - 1).collect();
        let sum = quantised(&sum_target, &noise_sum);

        for offset in [-9i16, -1, 0, 1, 12, 25] {
            let balance_target: Vec<i16> = (0..high).map(|_| offset).collect();
            let noise_balance: Vec<i16> = (0..noise_groups).map(|_| offset).collect();
            let balance = quantised(&balance_target, &noise_balance);

            let mut mono = ScaleFactors::new();
            dequantise(&sum, false, &mut mono).expect("单声道反量化");
            let mut out_sum = ScaleFactors::new();
            let mut out_balance = ScaleFactors::new();
            dequantise_pair(&sum, &balance, false, &mut out_sum, &mut out_balance)
                .expect("立体声反量化");

            for group in 0..high {
                let total =
                    out_sum.sig(0, group).expect("和") + out_balance.sig(0, group).expect("平衡");
                let expected = 2.0 * mono.sig(0, group).expect("单声道");
                let error = (f64::from(total) - f64::from(expected)).abs() / f64::from(expected);
                assert!(
                    error < 1e-6,
                    "偏移 {offset} 第 {group} 组：信号两声道之和 {total} 应等于 {expected}"
                );
            }
            // 噪声侧同一条恒等式：nom = 2^(NOISE_FLOOR_OFFSET − qscf_a + 1)。
            for group in 0..noise_groups {
                let total = out_sum.noise(0, group).expect("和")
                    + out_balance.noise(0, group).expect("平衡");
                let expected = 2.0 * mono.noise(0, group).expect("单声道");
                let error = (f64::from(total) - f64::from(expected)).abs() / f64::from(expected);
                assert!(
                    error < 1e-6,
                    "偏移 {offset} 第 {group} 组：噪声两声道之和 {total} 应等于 {expected}"
                );
            }
        }
    }

    /// 平衡参数取 12 时两声道恰好均分。
    ///
    /// 居中点直接抄 `Pseudocode 84` 的 `PAN_OFFSET = 12`，**不引用实现里的
    /// 常量**：两边共用一个常量的话，常量改错时期望值会跟着一起改，判据就成了
    /// 自证。
    #[test]
    fn the_pan_centre_splits_the_energy_evenly() {
        const SPEC_PAN_OFFSET: i16 = 12;
        let high = high_groups();
        let noise_groups = noise_groups();
        let sum_target: Vec<i16> = vec![3; high];
        let noise_sum: Vec<i16> = vec![1; noise_groups];
        let sum = quantised(&sum_target, &noise_sum);

        // 3 dB 档：qscf_b = 12 即居中；1,5 dB 档要除以 a = 2，故需 24。
        for (coarse_quant, centre) in [(true, SPEC_PAN_OFFSET), (false, 2 * SPEC_PAN_OFFSET)] {
            let balance_target: Vec<i16> = vec![centre; high];
            let noise_balance: Vec<i16> = vec![SPEC_PAN_OFFSET; noise_groups];
            let balance = quantised(&balance_target, &noise_balance);
            let mut out_sum = ScaleFactors::new();
            let mut out_balance = ScaleFactors::new();
            dequantise_pair(&sum, &balance, coarse_quant, &mut out_sum, &mut out_balance)
                .expect("立体声反量化");
            for group in 0..high {
                assert_eq!(
                    out_sum.sig(0, group),
                    out_balance.sig(0, group),
                    "居中时第 {group} 组两声道应相等"
                );
            }
            for group in 0..noise_groups {
                assert_eq!(
                    out_sum.noise(0, group),
                    out_balance.noise(0, group),
                    "噪声第 {group} 组居中时两声道应相等"
                );
            }
        }
    }

    /// 平衡参数增大把能量从平衡声道移向和声道，且移动是单调的。
    #[test]
    fn the_balance_parameter_moves_energy_monotonically() {
        let high = high_groups();
        let noise_groups = noise_groups();
        let sum = quantised(&vec![0; high], &vec![0; noise_groups]);

        let mut previous: Option<(f32, f32)> = None;
        for offset in -6..=6i16 {
            // 起点同样抄规范的 12，理由见 the_pan_centre_splits_the_energy_evenly。
            let balance = quantised(&vec![24 + offset; high], &vec![12 + offset; noise_groups]);
            let mut out_sum = ScaleFactors::new();
            let mut out_balance = ScaleFactors::new();
            dequantise_pair(&sum, &balance, false, &mut out_sum, &mut out_balance)
                .expect("立体声反量化");
            let current = (
                out_sum.sig(0, 0).expect("和"),
                out_balance.sig(0, 0).expect("平衡"),
            );
            if let Some((last_sum, last_balance)) = previous {
                assert!(current.0 > last_sum, "偏移 {offset}：和声道应递增");
                assert!(current.1 < last_balance, "偏移 {offset}：平衡声道应递减");
            }
            previous = Some(current);
        }
    }

    /// 指数越界必须拒绝，且不改写输出。
    #[test]
    fn an_out_of_range_exponent_is_rejected_without_touching_the_output() {
        let high = high_groups();
        let noise_groups = noise_groups();
        // 3 dB 档下 qscf = 200 让指数到 206，远超 f32 正规数上界。
        let mut target: Vec<i16> = vec![0; high];
        target[2] = 200;
        let quantised_signal = quantised(&target, &vec![0; noise_groups]);
        let mut out = ScaleFactors::new();
        assert_eq!(
            dequantise(&quantised_signal, true, &mut out),
            Err(DequantError::ExponentOutOfRange {
                kind: EnvelopeKind::Signal,
                envelope: 0,
                group: 2
            })
        );
        assert_eq!(out.counts(), (0, 0), "越界不应提交部分输出");

        // 噪声侧指数是 6 − qscf，因此很负的 qscf 才会上溢。
        let mut noise_target: Vec<i16> = vec![0; noise_groups];
        noise_target[1] = -200;
        let quantised_noise = quantised(&vec![0; high], &noise_target);
        let mut out = ScaleFactors::new();
        assert_eq!(
            dequantise(&quantised_noise, true, &mut out),
            Err(DequantError::ExponentOutOfRange {
                kind: EnvelopeKind::Noise,
                envelope: 0,
                group: 1
            })
        );
        assert_eq!(out.counts(), (0, 0), "越界不应提交部分输出");
    }

    /// 分子和分母各自可表示时，最终商仍可能下溢；这种结果同样必须拒绝。
    #[test]
    fn a_stereo_quotient_underflow_is_rejected_without_touching_the_outputs() {
        let high = high_groups();
        let noise_groups = noise_groups();

        // 信号侧：nom = 2^-126，denom = 1 + 2^127，最终商下溢。
        let sum = quantised(&vec![-133; high], &vec![0; noise_groups]);
        let balance = quantised(&vec![-115; high], &vec![12; noise_groups]);
        let mut out_sum = ScaleFactors::new();
        let mut out_balance = ScaleFactors::new();
        assert_eq!(
            dequantise_pair(&sum, &balance, true, &mut out_sum, &mut out_balance),
            Err(DequantError::ExponentOutOfRange {
                kind: EnvelopeKind::Signal,
                envelope: 0,
                group: 0
            })
        );
        assert_eq!(out_sum.counts(), (0, 0), "下溢不应提交和声道输出");
        assert_eq!(out_balance.counts(), (0, 0), "下溢不应提交平衡声道输出");

        // 噪声侧同样构造 nom = 2^-126；信号侧先成功，仍不得提交部分输出。
        let sum = quantised(&vec![0; high], &vec![133; noise_groups]);
        let balance = quantised(&vec![12; high], &vec![-115; noise_groups]);
        assert_eq!(
            dequantise_pair(&sum, &balance, true, &mut out_sum, &mut out_balance),
            Err(DequantError::ExponentOutOfRange {
                kind: EnvelopeKind::Noise,
                envelope: 0,
                group: 0
            })
        );
        assert_eq!(out_sum.counts(), (0, 0), "下溢不应提交和声道输出");
        assert_eq!(out_balance.counts(), (0, 0), "下溢不应提交平衡声道输出");
    }

    /// 可表示的最大半整数指数不得因不可表示的整数幂中间值而被误拒绝。
    #[test]
    fn the_largest_finite_half_exponent_avoids_an_overflowing_intermediate() {
        let high = high_groups();
        let noise_groups = noise_groups();
        // nom 与和声道分母中的幂项都是 2^127,5，仍是有限正规数。
        let sum = quantised(&vec![241; high], &vec![0; noise_groups]);
        let balance = quantised(&vec![-231; high], &vec![12; noise_groups]);
        let mut out_sum = ScaleFactors::new();
        let mut out_balance = ScaleFactors::new();
        dequantise_pair(&sum, &balance, false, &mut out_sum, &mut out_balance)
            .expect("127,5 次幂应可解码");

        for group in 0..high {
            assert!(
                out_sum.sig(0, group).is_some_and(f32::is_normal),
                "和声道第 {group} 组应为正规数"
            );
            assert!(
                out_balance.sig(0, group).is_some_and(f32::is_normal),
                "平衡声道第 {group} 组应为正规数"
            );
        }
    }

    /// 平衡式立体声要求两声道网格一致。
    #[test]
    fn a_mismatched_stereo_grid_is_rejected() {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let high = high_groups();
        let low = usize::from(tables().num_sbg_sig_lowres());
        let noise_groups = usize::from(map.noise_groups());
        let sum = quantised(&vec![0; high], &vec![0; noise_groups]);

        // 第二个声道用低分辨率，组数不同。
        let mut history = EnvelopeHistory::new();
        let mut balance = EnvelopeScaleFactors::new();
        decode(
            &[EnvelopeDeltas {
                data: &vec![0; low],
                time_direction: false,
                high_resolution: false,
            }],
            &[EnvelopeDeltas {
                data: &vec![0; noise_groups],
                time_direction: false,
                high_resolution: false,
            }],
            &map,
            false,
            &mut history,
            &mut balance,
        )
        .expect("构造");

        let mut out_sum = ScaleFactors::new();
        let mut out_balance = ScaleFactors::new();
        assert_eq!(
            dequantise_pair(&sum, &balance, false, &mut out_sum, &mut out_balance),
            Err(DequantError::GridMismatch {
                kind: EnvelopeKind::Signal,
                envelope: 0
            })
        );
        assert_eq!(out_sum.counts(), (0, 0), "拒绝不应提交输出");
        assert_eq!(out_balance.counts(), (0, 0), "拒绝不应提交输出");

        // 信号网格一致、但第二个声道多一个噪声包络时，应准确报告噪声域。
        let sig = vec![0; high];
        let noise = vec![0; noise_groups];
        let mut history = EnvelopeHistory::new();
        let mut balance = EnvelopeScaleFactors::new();
        decode(
            &[EnvelopeDeltas {
                data: &sig,
                time_direction: false,
                high_resolution: true,
            }],
            &[
                EnvelopeDeltas {
                    data: &noise,
                    time_direction: false,
                    high_resolution: false,
                },
                EnvelopeDeltas {
                    data: &noise,
                    time_direction: false,
                    high_resolution: false,
                },
            ],
            &map,
            false,
            &mut history,
            &mut balance,
        )
        .expect("构造");
        assert_eq!(
            dequantise_pair(&sum, &balance, false, &mut out_sum, &mut out_balance),
            Err(DequantError::GridMismatch {
                kind: EnvelopeKind::Noise,
                envelope: 0
            })
        );
        assert_eq!(out_sum.counts(), (0, 0), "拒绝不应提交输出");
        assert_eq!(out_balance.counts(), (0, 0), "拒绝不应提交输出");
    }
}
