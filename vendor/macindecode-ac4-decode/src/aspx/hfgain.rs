//! HF 包络调整：补偿增益（`TS103190-1:v1.4.1:5.7.6.4.2.2`）。
//!
//! `Pseudocode 95`–`101` 接过 `5.7.6.4.2.1` 的七张矩阵，产出三张调整后的矩阵：
//! `sig_gain_sb_adj` 缩放 HF 生成信号，`noise_lev_sb_adj` 交给 `5.7.6.4.3` 的
//! 噪声发生器，`sine_lev_sb_adj` 是要叠加的正弦幅度。
//!
//! # `aspx_limiter` 开关
//!
//! `Pseudocode 95` 的初始增益始终计算。`aspx_limiter == 0` 时不进入
//! `Pseudocode 96`–`101` 的 limiter 链：信号使用初始增益，噪声与正弦电平保持
//! `Pseudocode 94` 的值，诊断用 boost 为中性值 `1`。打开时才执行下面的两级钳制。
//! [`LimiterMode`] 把开关与开启路径所需的两张表绑定在同一个参数里。
//!
//! # 两级钳制
//!
//! 增益先被限幅器子带组的上限压住（`Pseudocode 96`–`98`），再按限幅损失的能量
//! 整组抬回来（`Pseudocode 99`–`101`）。两级各有硬上限：`MAX_SIG_GAIN` 压住上限
//! 本身，`MAX_BOOST_FACT` 压住抬升倍数。抬升按限幅器组统一施加，因此组内一个
//! 被压住的子带最终可以高过它自己未限幅时的增益。
//!
//! # 三个常量互不相同
//!
//! - `Pseudocode 95` 的 `EPSILON = 1`，加在实际包络上。它顺带解释了限幅为何在
//!   均匀子带组内永远不生效：`sig_gain² = scf/((1+est)(1+noise))` 恒小于
//!   `LIM_GAIN² · scf/est`。限幅只在组内 `est` 起伏够大时才咬得住。
//! - `Pseudocode 96` 与 `Pseudocode 99` 的 `EPSILON0 = 1e-12`，防止除零。
//! - 两处的 `nom` 初值**不同**：`Pseudocode 96` 从 `0` 起，`Pseudocode 99` 从
//!   `EPSILON0` 起。没有旁证说明这是笔误，按字面各自保留。
//!
//! # 限幅器表的末项必须是 `sbz`
//!
//! `Pseudocode 96` 与 `Pseudocode 100` 的映射循环写作
//! `if (sb == sbg_lim[sbg+1]-sbx) sbg++;`，没有上界守卫。`sbg` 不会越过表尾，只
//! 因为 `sbg_lim[num_sbg_lim] - sbx` 恰是 `num_sb_aspx`，循环变量永远够不到它。
//! 这正是 `5.7.6.3.1.5` 把 `sbz` 定为不可合并边界的理由，这里把它变成前置检查。
//!
//! # 输入必须来自同一布局
//!
//! 子带数相同不足以说明两份频带表相同，首末边界相同也不足以说明 limiter 的内部
//! 分组相同。[`EnvelopeEstimate`] 与 [`LimiterTable`] 因此各自携带推导来源；开启
//! limiter 时，这里同时核对 estimate、频带表、patch 表与 limiter 表，拒绝同形但
//! 不同源的组合。
//!
//! # 七层外循环合成一层
//!
//! `Pseudocode 95`–`101` 各有一层 `for (atsg ...)`，之间没有任何跨包络的携带
//! 量，合成一层与原文等价。`Pseudocode 95` 循环外的 `b_sine_at_end` 属于**下一**
//! 区间，由 `5.7.6.4.2.1` 固化在 [`EnvelopeEstimate::sine_onset`] 里，见那里的
//! 顺序说明。
//!
//! # `Pseudocode 101` 的括号
//!
//! 原文写 `noise_lev_sb_lim[sb]atsg]`，少一个左括号；同一行的另外两个赋值都是
//! `[sb][atsg]`，按之补齐。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "子带下标以 64 为界、限幅器组下标以 MAX_SBG_LIM 为界，边界减 sbx \
              的合法性由函数开头的前置检查给出；伪码在同一层循环里索引多张\
              「子带 × 包络」矩阵，换成迭代器要先 zip 起来，比原文更难核对"
)]

use crate::aspx::bands::AspxBandTables;
use crate::aspx::frames::{AspxInterval, MAX_ATSG_SIG};
use crate::aspx::hfadjust::EnvelopeEstimate;
use crate::aspx::limiter::{LimiterTable, MAX_SBG_LIM};
use crate::aspx::patches::PatchTable;
use crate::aspx::tables::NUM_QMF_SUBBANDS;
use macindecode_ac4_bitstream::math::sqrt;

/// 子带数，`5.7.3.2` 规定恒为 64。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// `Pseudocode 95` 的 `EPSILON`。
const EPSILON: f64 = 1.0;

/// `Pseudocode 96`/`99` 的 `EPSILON0 = pow(10, -12)`。
const EPSILON0: f64 = 1e-12;

/// `Pseudocode 96` 的 `LIM_GAIN`。
const LIM_GAIN: f64 = 1.41254;

/// `Pseudocode 96` 的 `MAX_SIG_GAIN = pow(10, 5)`。
const MAX_SIG_GAIN: f64 = 1e5;

/// `Pseudocode 100` 的 `MAX_BOOST_FACT`。
const MAX_BOOST_FACT: f64 = 1.584_893_192;

/// 补偿增益无法计算的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GainError {
    /// 包络数为零或超出表 128 的上界。
    EnvelopeCountOutOfRange { envelopes: usize },
    /// 包络估计覆盖的子带数与频带表的 `num_sb_aspx` 不符。
    SubbandCountMismatch { estimate: u8, bands: u8 },
    /// 包络估计与频带表虽然同宽，但完整的频带布局不同。
    BandLayoutMismatch,
    /// 限幅器组数为零或超出 [`MAX_SBG_LIM`]。
    LimiterGroupsOutOfRange { groups: usize },
    /// 限幅器表的首末边界与 A-SPX 范围不符。
    ///
    /// `Pseudocode 96`/`100` 的映射循环靠 `sbg_lim[num_sbg_lim] - sbx` 恰是
    /// `num_sb_aspx` 才不会越出表尾，见模块文档。
    LimiterRangeMismatch {
        first: u8,
        last: u8,
        sbx: u8,
        num_sb_aspx: u8,
    },
    /// 限幅器边界未严格递增。
    ///
    /// 宽度为零的组会让 `Pseudocode 96` 的 `nom` 停在 `0`，把整组增益压成零。
    LimiterBordersNotIncreasing { index: usize },
    /// limiter 表不是由本次传入的频带表与 patch 表共同推出。
    LimiterSourceMismatch,
    /// `sig_gain_sb` 为零，而 `Pseudocode 97` 要除以它。
    ///
    /// 只有 `scf_sig_sb` 为零才会这样，而 `5.7.6.3.5` 的反量化只产出正规数，
    /// 故它意味着上游已经坏了。照字面算下去会得到 `0 · ∞ = NaN`。
    DegenerateSignalGain { subband: usize, envelope: usize },
}

/// `5.7.6.4.2.2` 产出的三张调整后矩阵与所用的 boost 因子。
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustedGains {
    sig_gain: [[f32; MAX_ATSG_SIG]; SUBBANDS],
    noise_lev: [[f32; MAX_ATSG_SIG]; SUBBANDS],
    sine_lev: [[f32; MAX_ATSG_SIG]; SUBBANDS],
    boost_fact: [[f32; MAX_ATSG_SIG]; SUBBANDS],
    /// 生成这些矩阵的完整频带布局。
    source_bands: AspxBandTables,
    /// 生成这些矩阵的完整时间布局。
    source_interval: AspxInterval,
    /// 包络估计时采用的每 ATS QMF 时隙数。
    source_num_ts_in_ats: u8,
    subbands: u8,
    envelopes: u8,
}

impl AdjustedGains {
    /// 建立空结果。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sig_gain: [[0.0; MAX_ATSG_SIG]; SUBBANDS],
            noise_lev: [[0.0; MAX_ATSG_SIG]; SUBBANDS],
            sine_lev: [[0.0; MAX_ATSG_SIG]; SUBBANDS],
            boost_fact: [[0.0; MAX_ATSG_SIG]; SUBBANDS],
            source_bands: AspxBandTables::empty(),
            source_interval: AspxInterval::empty(),
            source_num_ts_in_ats: 0,
            subbands: 0,
            envelopes: 0,
        }
    }

    /// A-SPX 范围内的子带数，即 `num_sb_aspx`。
    #[must_use]
    pub const fn subbands(&self) -> u8 {
        self.subbands
    }

    /// 信号包络数，即 `num_atsg_sig`。
    #[must_use]
    pub const fn envelopes(&self) -> u8 {
        self.envelopes
    }

    fn in_range(&self, sb: usize, atsg: usize) -> bool {
        sb < usize::from(self.subbands) && atsg < usize::from(self.envelopes)
    }

    /// `sig_gain_sb_adj[sb][atsg]`：施加在 HF 生成信号上的补偿增益。
    #[must_use]
    pub fn signal_gain(&self, sb: usize, atsg: usize) -> Option<f32> {
        self.in_range(sb, atsg).then(|| self.sig_gain[sb][atsg])
    }

    /// 为跨模块浮点运算顺序判据设置一个可精确控制的信号增益。
    #[cfg(test)]
    pub(crate) fn set_signal_gain_for_test(&mut self, sb: usize, atsg: usize, value: f32) -> bool {
        if !self.in_range(sb, atsg) {
            return false;
        }
        self.sig_gain[sb][atsg] = value;
        true
    }

    /// `noise_lev_sb_adj[sb][atsg]`：`5.7.6.4.3` 噪声发生器的幅度。
    #[must_use]
    pub fn noise_level(&self, sb: usize, atsg: usize) -> Option<f32> {
        self.in_range(sb, atsg).then(|| self.noise_lev[sb][atsg])
    }

    /// `sine_lev_sb_adj[sb][atsg]`：要叠加的正弦幅度。
    #[must_use]
    pub fn sine_level(&self, sb: usize, atsg: usize) -> Option<f32> {
        self.in_range(sb, atsg).then(|| self.sine_lev[sb][atsg])
    }

    /// `boost_fact_sb[sb][atsg]`：限幅损失的能量被抬回多少。
    ///
    /// 三张输出矩阵都乘了它，因此它同时是判断限幅是否咬住的诊断量；limiter
    /// 关闭时恒为中性值 `1`。
    #[must_use]
    pub fn boost_factor(&self, sb: usize, atsg: usize) -> Option<f32> {
        self.in_range(sb, atsg).then(|| self.boost_fact[sb][atsg])
    }

    /// 这些矩阵是否由给定的完整频带布局生成。
    pub(super) fn matches_bands(&self, bands: &AspxBandTables) -> bool {
        self.source_bands == *bands
    }

    /// 这些矩阵是否由给定的完整时间布局生成。
    pub(super) fn matches_interval(&self, interval: &AspxInterval) -> bool {
        self.source_interval == *interval
    }

    /// 包络估计时采用的每 ATS QMF 时隙数。
    pub(super) const fn source_num_ts_in_ats(&self) -> u8 {
        self.source_num_ts_in_ats
    }
}

impl Default for AdjustedGains {
    fn default() -> Self {
        Self::new()
    }
}

/// 当前区间是否启用 `aspx_limiter`。
#[derive(Debug, Clone, Copy)]
pub enum LimiterMode<'a> {
    /// `aspx_limiter == 0`：只执行 `Pseudocode 95`，不做限幅与 boost。
    Off,
    /// `aspx_limiter == 1`：执行 `Pseudocode 96`–`101`。
    On {
        /// 由同一频带与 patch 表推出的 limiter 表。
        table: &'a LimiterTable,
        /// 创建 `Q_high` 时所用的 patch 表，用于核对 limiter 来源。
        patches: &'a PatchTable,
    },
}

/// `Pseudocode 95`–`101`：算出当前区间的补偿增益。
///
/// `estimate` 是 [`super::hfadjust::estimate`] 的输出。`mode` 直接对应
/// `aspx_limiter`：关闭时不需要 limiter 表，打开时同时提供 `5.7.6.3.1.5` 的表与
/// 创建 `Q_high` 时所用的 patch 表。
///
/// # Errors
///
/// 见 [`GainError`]。任一条不成立时都不改写 `out`。
pub fn adjust(
    estimate: &EnvelopeEstimate,
    bands: &AspxBandTables,
    mode: LimiterMode<'_>,
    out: &mut AdjustedGains,
) -> Result<(), GainError> {
    let envelopes = usize::from(estimate.envelopes());
    if envelopes == 0 || envelopes > MAX_ATSG_SIG {
        return Err(GainError::EnvelopeCountOutOfRange { envelopes });
    }
    if estimate.subbands() != bands.num_sb_aspx() {
        return Err(GainError::SubbandCountMismatch {
            estimate: estimate.subbands(),
            bands: bands.num_sb_aspx(),
        });
    }
    if !estimate.matches_bands(bands) {
        return Err(GainError::BandLayoutMismatch);
    }
    let sbx = usize::from(bands.sbx());
    let num_sb_aspx = usize::from(estimate.subbands());
    let limiter = match mode {
        LimiterMode::Off => None,
        LimiterMode::On { table, patches } => {
            let groups = usize::from(table.count());
            if groups == 0 || groups > MAX_SBG_LIM {
                return Err(GainError::LimiterGroupsOutOfRange { groups });
            }

            // 映射循环没有上界守卫，靠 limiter 表铺满
            // `[sbx, sbx + num_sb_aspx)`。
            let first = table.border(0).unwrap_or(0);
            let last = table.border(groups).unwrap_or(0);
            if usize::from(first) != sbx || usize::from(last) != sbx + num_sb_aspx {
                return Err(GainError::LimiterRangeMismatch {
                    first,
                    last,
                    sbx: bands.sbx(),
                    num_sb_aspx: estimate.subbands(),
                });
            }
            for index in 1..=groups {
                if table.border(index) <= table.border(index - 1) {
                    return Err(GainError::LimiterBordersNotIncreasing { index });
                }
            }
            if !table.matches_sources(bands, patches) {
                return Err(GainError::LimiterSourceMismatch);
            }
            Some(table)
        }
    };

    let mut sig_gain_adj = [[0.0f32; MAX_ATSG_SIG]; SUBBANDS];
    let mut noise_lev_adj = [[0.0f32; MAX_ATSG_SIG]; SUBBANDS];
    let mut sine_lev_adj = [[0.0f32; MAX_ATSG_SIG]; SUBBANDS];
    let mut boost_fact = [[0.0f32; MAX_ATSG_SIG]; SUBBANDS];

    for atsg in 0..envelopes {
        // 前置检查已保证以下取值全部落在估计矩阵内，`unwrap_or` 不可达。
        let onset = estimate.sine_onset(atsg).unwrap_or(false);
        let est = |sb: usize| f64::from(estimate.estimated(sb, atsg).unwrap_or(0.0));
        let scf_sig = |sb: usize| f64::from(estimate.signal_scale(sb, atsg).unwrap_or(0.0));
        let scf_noise = |sb: usize| f64::from(estimate.noise_scale(sb, atsg).unwrap_or(0.0));

        // `Pseudocode 95`：初始增益。`sine_area` 为真时噪声进分子，为假时只在
        // 本包络不是正弦起点的情况下进分母。
        let mut sig_gain = [0.0f32; SUBBANDS];
        for sb in 0..num_sb_aspx {
            let mut denom = EPSILON + est(sb);
            let nom = if estimate.sine_area(sb, atsg).unwrap_or(false) {
                denom *= 1.0 + scf_noise(sb);
                scf_sig(sb) * scf_noise(sb)
            } else {
                if !onset {
                    denom *= 1.0 + scf_noise(sb);
                }
                scf_sig(sb)
            };
            sig_gain[sb] = sqrt(nom / denom) as f32;
        }

        let Some(limiter) = limiter else {
            // limiter 关闭：`Pseudocode 95` 的增益与 `94` 的两张电平表直接成为
            // 输出。boost 记为乘法中性元，便于诊断与后续统一消费。
            for sb in 0..num_sb_aspx {
                sig_gain_adj[sb][atsg] = sig_gain[sb];
                noise_lev_adj[sb][atsg] = estimate.noise_level(sb, atsg).unwrap_or(0.0);
                sine_lev_adj[sb][atsg] = estimate.sine_level(sb, atsg).unwrap_or(0.0);
                boost_fact[sb][atsg] = 1.0;
            }
            continue;
        };
        let groups = usize::from(limiter.count());
        // 前置检查给出 `sbx <= sbg_lim[i] <= sbx + num_sb_aspx`，下面的
        // `- sbx` 与 `[sb]` 因此都在范围内。
        let group_span = |sbg: usize| {
            let low = usize::from(limiter.border(sbg).unwrap_or(0)) - sbx;
            let high = usize::from(limiter.border(sbg + 1).unwrap_or(0)) - sbx;
            low..high
        };

        // `Pseudocode 96`：限幅器组的增益上限。`nom` 从 0 起，与 `Pseudocode 99`
        // 不同。整组的量留在 f64，只在映射到子带时收窄一次。
        let mut group_gain = [0.0f64; MAX_SBG_LIM];
        for sbg in 0..groups {
            let mut nom = 0.0f64;
            let mut denom = EPSILON0;
            for sb in group_span(sbg) {
                nom += scf_sig(sb);
                denom += est(sb);
            }
            group_gain[sbg] = sqrt(nom / denom) * LIM_GAIN;
        }
        let mut max_gain = [0.0f32; SUBBANDS];
        let mut sbg = 0usize;
        for sb in 0..num_sb_aspx {
            if sb + sbx == usize::from(limiter.border(sbg + 1).unwrap_or(0)) {
                sbg += 1;
            }
            max_gain[sb] = group_gain[sbg].min(MAX_SIG_GAIN) as f32;
        }

        // `Pseudocode 97`/`98`：噪声按增益的损失比例下调，增益本身钳到上限。
        let mut noise_lim = [0.0f32; SUBBANDS];
        let mut gain_lim = [0.0f32; SUBBANDS];
        for sb in 0..num_sb_aspx {
            let gain = sig_gain[sb];
            if gain == 0.0 {
                return Err(GainError::DegenerateSignalGain {
                    subband: sb,
                    envelope: atsg,
                });
            }
            let noise = estimate.noise_level(sb, atsg).unwrap_or(0.0);
            let scaled = (f64::from(noise) * f64::from(max_gain[sb]) / f64::from(gain)) as f32;
            noise_lim[sb] = noise.min(scaled);
            gain_lim[sb] = gain.min(max_gain[sb]);
        }

        // `Pseudocode 99`：按限幅损失的能量算 boost。`nom` 从 EPSILON0 起。
        // 噪声能量只在该子带既无正弦、本包络又不是正弦起点时才计入。
        let mut group_boost = [0.0f64; MAX_SBG_LIM];
        for sbg in 0..groups {
            let mut nom = EPSILON0;
            let mut denom = EPSILON0;
            for sb in group_span(sbg) {
                nom += scf_sig(sb);
                let gain = f64::from(gain_lim[sb]);
                denom += est(sb) * gain * gain;
                let sine = f64::from(estimate.sine_level(sb, atsg).unwrap_or(0.0));
                denom += sine * sine;
                if sine == 0.0 && !onset {
                    let noise = f64::from(noise_lim[sb]);
                    denom += noise * noise;
                }
            }
            group_boost[sbg] = sqrt(nom / denom);
        }

        // `Pseudocode 100`：映射到子带并钳到 MAX_BOOST_FACT。
        let mut sbg = 0usize;
        for sb in 0..num_sb_aspx {
            if sb + sbx == usize::from(limiter.border(sbg + 1).unwrap_or(0)) {
                sbg += 1;
            }
            boost_fact[sb][atsg] = group_boost[sbg].min(MAX_BOOST_FACT) as f32;
        }

        // `Pseudocode 101`：boost 施加到三张矩阵。正弦电平用未限幅的原值。
        for sb in 0..num_sb_aspx {
            let boost = boost_fact[sb][atsg];
            sig_gain_adj[sb][atsg] = gain_lim[sb] * boost;
            noise_lev_adj[sb][atsg] = noise_lim[sb] * boost;
            sine_lev_adj[sb][atsg] = estimate.sine_level(sb, atsg).unwrap_or(0.0) * boost;
        }
    }

    out.sig_gain = sig_gain_adj;
    out.noise_lev = noise_lev_adj;
    out.sine_lev = sine_lev_adj;
    out.boost_fact = boost_fact;
    out.source_bands = *bands;
    out.source_interval = estimate.source_interval();
    out.source_num_ts_in_ats = estimate.source_num_ts_in_ats();
    out.subbands = estimate.subbands();
    out.envelopes = estimate.envelopes();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aspx::dequant::ScaleFactors;
    use crate::aspx::frames::{AspxInterval, AspxIntervalParams};
    use crate::aspx::hfadjust::{SinePlacement, SineState, estimate};
    use crate::aspx::patches::PatchTable;
    use crate::aspx::qmf::QmfSlot;
    use core::f32::consts::FRAC_1_SQRT_2;
    use macindecode_ac4_bitstream::math::sqrt_f32;

    /// A-SPX 时隙数。单包络时它同时是包络的 ATS 跨度。
    const SLOTS: usize = 16;

    /// 夹具配置的 `num_sb_aspx`，见 [`the_fixture_has_the_layout_the_other_criteria_assume`]。
    const ASPX_SUBBANDS: usize = 36;

    /// 全部期望值都由独立转写的 `Pseudocode 95`–`101` 在 f64 下算出。实现按
    /// ADR-0002 用 f32 存中间矩阵，故留 f32 机器精度约二十倍的相对余量。
    const TOLERANCE: f32 = 2e-6;

    fn bands() -> AspxBandTables {
        AspxBandTables::derive(false, 0, 0, 0, 0).expect("应能推出频带表")
    }

    fn limits(bands: &AspxBandTables) -> (PatchTable, LimiterTable) {
        let patches = PatchTable::derive(bands, false, false).expect("应能推出 patch 表");
        let table = LimiterTable::derive(bands, &patches).expect("应能推出限幅器表");
        (patches, table)
    }

    /// 按每个 A-SPX 子带的目标 `est_sig` 铺一段 `Q_high`。
    ///
    /// 插值打开时 `est_sig[sb]` 是该子带模平方在包络内的时间平均，而这里每个
    /// 时隙取同一个值，故令实部与虚部同取 `sqrt(target/2)` 即可。判据只用
    /// `target/2` 为完全平方的取值，换算不引入舍入。
    fn high_band(targets: &[f32], sbx: usize) -> [QmfSlot; SLOTS] {
        let mut out = [QmfSlot::zero(); SLOTS];
        for slot in &mut out {
            for (sb, &target) in targets.iter().enumerate() {
                let value = sqrt_f32(target / 2.0);
                slot.re[sb + sbx] = value;
                slot.im[sb + sbx] = value;
            }
        }
        out
    }

    /// 为任意合法频带布局造一份单包络、全子带 `est = 2` 的估计结果。
    fn flat_estimate(bands: &AspxBandTables) -> EnvelopeEstimate {
        let targets = [2.0f32; SUBBANDS];
        let count = usize::from(bands.num_sb_aspx());
        let q_high = high_band(&targets[..count], usize::from(bands.sbx()));
        let params = AspxIntervalParams::fixfix(1);
        let interval =
            AspxInterval::derive(&params, SLOTS as u8, 1, true, 16).expect("应能推导区间");
        let mut sf = ScaleFactors::new();
        sf.fill_for_test(
            1,
            usize::from(bands.num_sbg_sig_highres()),
            usize::from(bands.num_sbg_noise()),
            1.0,
            1.0,
        );
        let mut estimated = EnvelopeEstimate::new();
        estimate(
            &q_high,
            bands,
            &interval,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            true,
            1,
            &mut SineState::new(),
            &mut estimated,
        )
        .expect("应能估计包络");
        estimated
    }

    /// 跑完 `5.7.6.4.2.1` 与 `5.7.6.4.2.2`，并显式选择 limiter 分支。
    #[expect(
        clippy::too_many_arguments,
        reason = "比通用判据多出的最后一个布尔值只用于覆盖 aspx_limiter 两个分支"
    )]
    fn run_with_limiter(
        est: &[f32; ASPX_SUBBANDS],
        scf_sig: f32,
        scf_noise: f32,
        harmonic: bool,
        placement: SinePlacement,
        envelopes: u8,
        sines: &mut SineState,
        limiter_enabled: bool,
    ) -> AdjustedGains {
        let bands = bands();
        let (patches, table) = limits(&bands);
        let q_high = high_band(est, usize::from(bands.sbx()));
        let params = AspxIntervalParams::fixfix(envelopes);
        let interval =
            AspxInterval::derive(&params, SLOTS as u8, 1, true, 16).expect("应能推导区间");
        let mut sf = ScaleFactors::new();
        sf.fill_for_test(
            usize::from(envelopes),
            usize::from(bands.num_sbg_sig_highres()),
            usize::from(bands.num_sbg_noise()),
            scf_sig,
            scf_noise,
        );
        let mut estimated = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &[harmonic; 32],
            placement,
            true,
            1,
            sines,
            &mut estimated,
        )
        .expect("应能估计包络");
        let mut out = AdjustedGains::new();
        let mode = if limiter_enabled {
            LimiterMode::On {
                table: &table,
                patches: &patches,
            }
        } else {
            LimiterMode::Off
        };
        adjust(&estimated, &bands, mode, &mut out).expect("应能算出补偿增益");
        out
    }

    /// 跑完默认开启 limiter 的完整链。
    fn run(
        est: &[f32; ASPX_SUBBANDS],
        scf_sig: f32,
        scf_noise: f32,
        harmonic: bool,
        placement: SinePlacement,
        envelopes: u8,
        sines: &mut SineState,
    ) -> AdjustedGains {
        run_with_limiter(
            est, scf_sig, scf_noise, harmonic, placement, envelopes, sines, true,
        )
    }

    /// 单包络、无正弦、指针不生效的常见情形。
    fn run_flat(est: &[f32; ASPX_SUBBANDS], scf_sig: f32, scf_noise: f32) -> AdjustedGains {
        run(
            est,
            scf_sig,
            scf_noise,
            false,
            SinePlacement::from_params(-1),
            1,
            &mut SineState::new(),
        )
    }

    #[track_caller]
    fn close(actual: Option<f32>, expected: f32, what: &str) {
        let actual = actual.expect("取值应在矩阵范围内");
        let tolerance = expected.abs() * TOLERANCE + f32::MIN_POSITIVE;
        assert!(
            (actual - expected).abs() <= tolerance,
            "{what}：期望 {expected}，实际 {actual}"
        );
    }

    #[test]
    fn the_fixture_has_the_layout_the_other_criteria_assume() {
        // 下面每条判据的期望值都是就这张限幅器表手算的：第 0 组恰好宽 2，
        // 因此能只靠两个子带构造出「组内 est 起伏大到让限幅咬住」的输入。
        // 频带表推导一旦改动，这些期望值就不再成立——先在这里断掉。
        let bands = bands();
        assert_eq!(bands.sbx(), 10);
        assert_eq!(usize::from(bands.num_sb_aspx()), ASPX_SUBBANDS);
        let (_, table) = limits(&bands);
        assert_eq!(table.count(), 6);
        let borders: [u8; 7] = core::array::from_fn(|i| table.border(i).expect("边界应存在"));
        assert_eq!(borders, [10, 12, 18, 26, 32, 38, 46]);
        // 高分辨率组的前两组各宽 1，故 `sine_area` 判据里 rel 0 与 rel 1 都会
        // 落在各自组的正中而被标记。
        assert_eq!(bands.sig_highres_border(0), Some(10));
        assert_eq!(bands.sig_highres_border(1), Some(11));
        assert_eq!(bands.sig_highres_border(2), Some(12));
    }

    #[test]
    fn a_uniform_interval_runs_the_whole_chain_without_either_clamp() {
        // est = 2、scf_sig = 1、scf_noise = 1、无正弦、非正弦起点：
        //   sig_gain = sqrt(1/((1+2)·2)) = sqrt(1/6)
        //   boost    = sqrt(1/(2·(1/6) + 1/2)) = sqrt(1,2)
        //   sig_adj  = sqrt(1/6)·sqrt(1,2) = sqrt(0,2)
        //   noise_adj= sqrt(1/2)·sqrt(1,2) = sqrt(0,6)
        let out = run_flat(&[2.0; ASPX_SUBBANDS], 1.0, 1.0);
        for sb in 0..ASPX_SUBBANDS {
            close(out.signal_gain(sb, 0), 0.447_213_6, "sqrt(0,2)");
            close(out.noise_level(sb, 0), 0.774_596_7, "sqrt(0,6)");
            close(out.sine_level(sb, 0), 0.0, "无正弦");
            close(out.boost_factor(sb, 0), 1.095_445_1, "sqrt(1,2)");
        }
        assert_eq!(out.subbands(), 36);
        assert_eq!(out.envelopes(), 1);
    }

    #[test]
    fn limiter_off_keeps_the_initial_gain_and_unadjusted_levels() {
        // 第 0 limiter 组内造出足够大的 est 起伏；开启 limiter 时 rel 0 会被钳住，
        // 关闭时则必须停在 Pseudocode 95/94，不能执行 96–101。
        let mut est = [32.0f32; ASPX_SUBBANDS];
        est[0] = 2.0;
        let out = run_with_limiter(
            &est,
            1.0,
            1.0,
            false,
            SinePlacement::from_params(-1),
            1,
            &mut SineState::new(),
            false,
        );

        close(out.signal_gain(0, 0), 0.408_248_3, "P95 的 sqrt(1/6)");
        close(out.signal_gain(1, 0), 0.123_091_49, "P95 的 sqrt(1/66)");
        for sb in 0..ASPX_SUBBANDS {
            close(
                out.noise_level(sb, 0),
                FRAC_1_SQRT_2,
                "P94 的噪声电平保持不变",
            );
            close(out.sine_level(sb, 0), 0.0, "P94 无正弦");
            close(out.boost_factor(sb, 0), 1.0, "关闭时的中性 boost");
        }

        // 上面的 `sine_level == 0` 在 `aspx_add_harmonic` 全假时恒真，证不了
        // 关闭路径确实透传了 `Pseudocode 94` 的正弦电平。再跑一次带正弦的：
        // `scf_noise = 1` 使 `Pseudocode 95` 两支相等，信号增益不变，只有正弦动。
        // 高分辨率组前两组各宽 1，rel 0/1 落在各自组的正中而被标记；rel 10
        // （绝对子带 20）所在的组是 `[20, 22)`，正中在 rel 11，故它不被标记。
        let tonal = run_with_limiter(
            &est,
            1.0,
            1.0,
            true,
            SinePlacement::from_params(-1),
            1,
            &mut SineState::new(),
            false,
        );
        close(
            tonal.signal_gain(0, 0),
            0.408_248_3,
            "两支相等，增益不随正弦变",
        );
        close(
            tonal.sine_level(0, 0),
            FRAC_1_SQRT_2,
            "关闭时透传 P94 的正弦电平",
        );
        close(
            tonal.sine_level(1, 0),
            FRAC_1_SQRT_2,
            "同上，另一个被标记的子带",
        );
        close(tonal.sine_level(10, 0), 0.0, "未被标记的子带仍是零");
        close(tonal.boost_factor(0, 0), 1.0, "关闭时的中性 boost");
    }

    #[test]
    fn the_sine_onset_envelope_drops_the_noise_term_in_both_places() {
        // 指针指向第 1 个包络：包络 0 不是起点，包络 1 是。
        // `Pseudocode 95` 的分母少乘 (1+scf_noise)，`Pseudocode 99` 的分母少加
        // noise_lev²，两处一起变。
        //   包络 0：sig_adj = sqrt(0,2)   noise_adj = sqrt(0,6)   boost = sqrt(1,2)
        //   包络 1：sig_adj = sqrt(0,5)   noise_adj = sqrt(0,75)  boost = sqrt(1,5)
        let out = run(
            &[2.0; ASPX_SUBBANDS],
            1.0,
            1.0,
            false,
            SinePlacement::from_params(1),
            2,
            &mut SineState::new(),
        );
        for sb in 0..ASPX_SUBBANDS {
            close(out.signal_gain(sb, 0), 0.447_213_6, "非起点包络");
            close(out.noise_level(sb, 0), 0.774_596_7, "非起点包络的噪声");
            close(out.boost_factor(sb, 0), 1.095_445_1, "非起点包络的 boost");
            close(out.signal_gain(sb, 1), FRAC_1_SQRT_2, "起点包络");
            close(out.noise_level(sb, 1), 0.866_025_4, "起点包络的噪声");
            close(out.boost_factor(sb, 1), 1.224_744_9, "起点包络的 boost");
        }
    }

    #[test]
    fn the_previous_intervals_end_pointer_makes_envelope_zero_the_onset() {
        // `p_sine_at_end` 属于**上一**区间。第一区间的指针落在其末尾（1 个包络、
        // 指针 1），第二区间的指针是 −1，永不等于任何包络下标；但包络 0 仍要按
        // 正弦起点处理，而包络 1 不能。
        //
        // 若 `5.7.6.4.2.2` 读的是被 `estimate` 推进过的状态，第二区间自己的指针
        // −1 不在末尾，包络 0 就会退回非起点分支。
        let mut sines = SineState::new();
        run(
            &[2.0; ASPX_SUBBANDS],
            1.0,
            1.0,
            false,
            SinePlacement::from_params(1),
            1,
            &mut sines,
        );
        let out = run(
            &[2.0; ASPX_SUBBANDS],
            1.0,
            1.0,
            false,
            SinePlacement::from_params(-1),
            2,
            &mut sines,
        );
        for sb in 0..ASPX_SUBBANDS {
            close(
                out.signal_gain(sb, 0),
                FRAC_1_SQRT_2,
                "包络 0 继承上一区间的起点",
            );
            close(out.signal_gain(sb, 1), 0.447_213_6, "包络 1 不是起点");
        }
    }

    #[test]
    fn the_limiter_ceiling_carries_lim_gain_and_stays_inside_its_group() {
        // 第 0 限幅器组（rel 0..2）的 est 取 2 与 32，组外全取 32；
        // scf_sig = scf_noise = 1：
        //   max_sbg[0]  = sqrt(2/34)·LIM_GAIN = 0,342591
        //   sig_gain[0] = sqrt(1/((1+2)·2))  = sqrt(1/6) = 0,408248 > 上限 → 被钳
        //   sig_gain[1] = sqrt(1/((1+32)·2)) = sqrt(1/66)          < 上限 → 不钳
        //
        // 被钳的子带 est 与 scf_noise 都非零，`Pseudocode 99` 的
        // `est·sig_gain_lim²` 与 `Pseudocode 97` 的噪声下调因此都可观察——
        // 若把 est[0] 取成 0（限幅更容易咬住），前者恒为零，用未限幅的增益算
        // boost 的缺陷就整个消失了。
        let mut est = [32.0f32; ASPX_SUBBANDS];
        est[0] = 2.0;
        let out = run_flat(&est, 1.0, 1.0);
        close(
            out.signal_gain(0, 0),
            0.386_462_64,
            "被钳到 sqrt(2/34)·LIM_GAIN 再乘 boost",
        );
        close(
            out.signal_gain(1, 0),
            0.138_854_28,
            "未被钳住的子带只乘 boost",
        );

        // `Pseudocode 97`：增益被压掉多少，噪声就跟着压掉多少。rel 0 被钳而
        // rel 1 没有，同一组内两者的噪声因此不同。
        close(
            out.noise_level(0, 0),
            0.669_372_9,
            "噪声按 max/sig_gain 下调",
        );
        close(out.noise_level(1, 0), 0.797_657_1, "未被钳的子带噪声不下调");

        // 组边界落在 rel 2（绝对子带 12）。组 1 起 est 均匀，boost 随之变化；
        // 把它和组 0 的值一起断言，映射循环挪一格就会失配。
        close(out.boost_factor(0, 0), 1.128_057_5, "组 0 的 boost");
        close(out.boost_factor(1, 0), 1.128_057_5, "rel 1 仍属组 0");
        close(out.boost_factor(2, 0), 1.007_663, "rel 2 已属组 1");
    }

    #[test]
    fn the_boost_factor_saturates_at_max_boost_fact() {
        // est 全零：sig_gain = sqrt(1/(1+0)) = 1，而 boost 的分母只剩 EPSILON0，
        // 分子是整组的 scf_sig，比值约 1e12 → 被 MAX_BOOST_FACT 钳住。
        let out = run_flat(&[0.0; ASPX_SUBBANDS], 1.0, 0.0);
        for sb in 0..ASPX_SUBBANDS {
            close(out.boost_factor(sb, 0), 1.584_893_2, "MAX_BOOST_FACT");
            close(
                out.signal_gain(sb, 0),
                1.584_893_2,
                "增益 1 只被 boost 抬起",
            );
            close(out.noise_level(sb, 0), 0.0, "scf_noise 为零时无噪声");
        }
    }

    #[test]
    fn the_limiter_ceiling_saturates_at_max_sig_gain() {
        // scf_sig = 1e12 时 sig_gain = 1e6，而组上限约 2e12；两者都要被
        // MAX_SIG_GAIN = 1e5 压住，最终只剩 1e5 · MAX_BOOST_FACT。
        let out = run_flat(&[0.0; ASPX_SUBBANDS], 1e12, 0.0);
        for sb in 0..ASPX_SUBBANDS {
            close(out.signal_gain(sb, 0), 158_489.31, "1e5 · MAX_BOOST_FACT");
        }
    }

    #[test]
    fn a_sine_in_the_subband_group_moves_the_noise_scale_into_the_numerator() {
        // scf_noise = 3，两次运行只差 aspx_add_harmonic：
        //   无正弦：sig_gain = sqrt(1/((1+2)·4)) = sqrt(1/12)
        //   有正弦：sig_gain = sqrt(1·3/((1+2)·4)) = 0,5
        // scf_noise 取 3 是为了让两个分支分开——取 1 时 scf_sig·scf_noise 与
        // scf_sig 相等，这条判据会恒真。
        let quiet = run_flat(&[2.0; ASPX_SUBBANDS], 1.0, 3.0);
        let tonal = run(
            &[2.0; ASPX_SUBBANDS],
            1.0,
            3.0,
            true,
            SinePlacement::from_params(-1),
            1,
            &mut SineState::new(),
        );
        for sb in 0..2 {
            close(quiet.signal_gain(sb, 0), 0.301_511_34, "sqrt(1/11)");
            close(quiet.noise_level(sb, 0), 0.904_534_04, "sqrt(9/11)");
            close(quiet.sine_level(sb, 0), 0.0, "无正弦");
            close(quiet.boost_factor(sb, 0), 1.044_465_9, "sqrt(12/11)");

            close(tonal.signal_gain(sb, 0), 0.577_350_27, "sqrt(1/3)");
            close(tonal.noise_level(sb, 0), 1.0, "sqrt(3/4)·sqrt(4/3)");
            close(tonal.sine_level(sb, 0), 0.577_350_27, "0,5·sqrt(4/3)");
            close(tonal.boost_factor(sb, 0), 1.154_700_5, "sqrt(4/3)");
        }
    }

    #[test]
    fn the_two_nominator_seeds_are_not_interchangeable() {
        // `Pseudocode 96` 的 nom 从 0 起，`Pseudocode 99` 的从 EPSILON0 起。
        // 取远小于 EPSILON0 的 scf_sig，两处的差别就变成可观察量：
        //   96 从 EPSILON0 起 → 上限 5,0e-7，不再钳住 sig_gain[0] = 1e-7；
        //   99 从 0 起        → boost 掉到 0,14。
        // 规范的量化器不会产出这么小的标度因子，这里只为把两个初值分开。
        let mut est = [8.0f32; ASPX_SUBBANDS];
        est[0] = 0.0;
        let out = run_flat(&est, 1e-14, 0.0);
        close(
            out.boost_factor(0, 0),
            1.005_491_5,
            "99 的 nom 从 EPSILON0 起",
        );
        close(out.signal_gain(0, 0), 7.101_485e-8, "96 的 nom 从 0 起");
        close(out.signal_gain(1, 0), 3.351_638_4e-8, "同组未被钳的子带");
    }

    #[test]
    fn a_zero_signal_scale_factor_is_reported_instead_of_dividing_by_zero() {
        // `Pseudocode 97` 除以 sig_gain。scf_sig 为零时它也是零，照字面算下去
        // 得到 0 · ∞ = NaN；`5.7.6.3.5` 的反量化只产出正规数，故这是上游故障。
        let bands = bands();
        let (patches, table) = limits(&bands);
        let q_high = high_band(&[2.0; ASPX_SUBBANDS], usize::from(bands.sbx()));
        let params = AspxIntervalParams::fixfix(1);
        let interval =
            AspxInterval::derive(&params, SLOTS as u8, 1, true, 16).expect("应能推导区间");
        let mut sf = ScaleFactors::new();
        sf.fill_for_test(
            1,
            usize::from(bands.num_sbg_sig_highres()),
            usize::from(bands.num_sbg_noise()),
            0.0,
            1.0,
        );
        let mut estimated = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            true,
            1,
            &mut SineState::new(),
            &mut estimated,
        )
        .expect("包络估计本身不拒绝零标度因子");
        let mut out = AdjustedGains::new();
        assert_eq!(
            adjust(
                &estimated,
                &bands,
                LimiterMode::On {
                    table: &table,
                    patches: &patches,
                },
                &mut out,
            ),
            Err(GainError::DegenerateSignalGain {
                subband: 0,
                envelope: 0
            })
        );
    }

    #[test]
    fn a_same_width_foreign_band_layout_is_rejected() {
        // 两套合法配置同为 22 个 A-SPX 子带，但交叉子带与内部 limiter 分组不同：
        // A 的相对边界是 [0,2,10,16,22]，B 是 [0,2,8,16,22]。只比较宽度会把
        // A 的包络估计按 B 的分组静默重算。
        let a = AspxBandTables::derive(false, 0, 1, 0, 6).expect("应能推出频带表 A");
        let b = AspxBandTables::derive(false, 0, 2, 0, 0).expect("应能推出频带表 B");
        assert_eq!(a.num_sb_aspx(), 22);
        assert_eq!(b.num_sb_aspx(), 22);
        assert_eq!(a.sbx(), 16);
        assert_eq!(b.sbx(), 10);

        let estimated = flat_estimate(&a);
        let patches = PatchTable::derive(&b, false, false).expect("应能推出 B 的 patch 表");
        let table = LimiterTable::derive(&b, &patches).expect("应能推出 B 的 limiter 表");
        let mut out = run_flat(&[2.0; ASPX_SUBBANDS], 1.0, 1.0);
        let snapshot = out.clone();
        assert_eq!(
            adjust(
                &estimated,
                &b,
                LimiterMode::On {
                    table: &table,
                    patches: &patches,
                },
                &mut out,
            ),
            Err(GainError::BandLayoutMismatch)
        );
        assert_eq!(out, snapshot, "外来频带布局不应改写输出");
    }

    #[test]
    fn a_limiter_from_another_patch_layout_is_rejected() {
        // 同一频带在两档基准采样率下得到不同 patch/limiter 表，但首末边界完全相同。
        // 仅检查范围与单调性无法分辨二者。
        let bands = AspxBandTables::derive(true, 0, 0, 0, 0).expect("应能推出频带表");
        let patches_44 =
            PatchTable::derive(&bands, true, false).expect("应能推出 44,1 kHz patch 表");
        let patches_48 = PatchTable::derive(&bands, true, true).expect("应能推出 48 kHz patch 表");
        let table_44 =
            LimiterTable::derive(&bands, &patches_44).expect("应能推出 44,1 kHz limiter 表");
        let table_48 =
            LimiterTable::derive(&bands, &patches_48).expect("应能推出 48 kHz limiter 表");
        assert_ne!(patches_44, patches_48);
        assert_ne!(table_44, table_48);
        assert_eq!(table_44.border(0), table_48.border(0));
        assert_eq!(
            table_44.border(usize::from(table_44.count())),
            table_48.border(usize::from(table_48.count()))
        );

        let estimated = flat_estimate(&bands);
        let mut out = run_flat(&[2.0; ASPX_SUBBANDS], 1.0, 1.0);
        let snapshot = out.clone();
        assert_eq!(
            adjust(
                &estimated,
                &bands,
                LimiterMode::On {
                    table: &table_44,
                    patches: &patches_48,
                },
                &mut out,
            ),
            Err(GainError::LimiterSourceMismatch)
        );
        assert_eq!(out, snapshot, "外来 limiter 来源不应改写输出");
    }

    #[test]
    fn a_limiter_from_a_noise_only_foreign_band_layout_is_rejected() {
        // `aspx_noise_sbg` 不参与 patch/limiter 推导，因此两套合法配置可得到完全
        // 相同的有效表，却仍是不同的完整频带来源。这个反例只会让
        // `LimiterTable::matches_sources` 的 `source_bands` 一半拒绝。
        let source = AspxBandTables::derive(false, 0, 0, 0, 0).expect("应能推出来源频带表");
        let current = AspxBandTables::derive(false, 0, 0, 1, 0).expect("应能推出当前频带表");
        assert_ne!(source, current);
        assert_eq!(source.num_sbg_noise(), 1);
        assert_eq!(current.num_sbg_noise(), 2);

        let source_patches =
            PatchTable::derive(&source, false, false).expect("应能推出来源 patch 表");
        let current_patches =
            PatchTable::derive(&current, false, false).expect("应能推出当前 patch 表");
        assert_eq!(source_patches, current_patches);
        let source_table =
            LimiterTable::derive(&source, &source_patches).expect("应能推出来源 limiter 表");
        let current_table =
            LimiterTable::derive(&current, &current_patches).expect("应能推出当前 limiter 表");
        assert_eq!(source_table, current_table);

        let estimated = flat_estimate(&current);
        let mut out = run_flat(&[2.0; ASPX_SUBBANDS], 1.0, 1.0);
        let snapshot = out.clone();
        assert_eq!(
            adjust(
                &estimated,
                &current,
                LimiterMode::On {
                    table: &source_table,
                    patches: &current_patches,
                },
                &mut out,
            ),
            Err(GainError::LimiterSourceMismatch)
        );
        assert_eq!(out, snapshot, "噪声布局不同的 limiter 来源不应改写输出");
    }

    #[test]
    fn rejected_input_leaves_the_output_untouched() {
        let bands = bands();
        let (patches, table) = limits(&bands);
        // 另一套配置：sbx = 12、num_sb_aspx = 34，限幅器表也从 12 起。
        let other = AspxBandTables::derive(false, 1, 0, 0, 0).expect("应能推出频带表");
        let (other_patches, other_table) = limits(&other);
        assert_eq!(other.num_sb_aspx(), 34);
        assert_eq!(other_table.border(0), Some(12));

        let q_high = high_band(&[2.0; ASPX_SUBBANDS], usize::from(bands.sbx()));
        let params = AspxIntervalParams::fixfix(1);
        let interval =
            AspxInterval::derive(&params, SLOTS as u8, 1, true, 16).expect("应能推导区间");
        let mut sf = ScaleFactors::new();
        sf.fill_for_test(
            1,
            usize::from(bands.num_sbg_sig_highres()),
            usize::from(bands.num_sbg_noise()),
            1.0,
            1.0,
        );
        let mut estimated = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            true,
            1,
            &mut SineState::new(),
            &mut estimated,
        )
        .expect("应能估计包络");

        let mut out = AdjustedGains::new();
        adjust(
            &estimated,
            &bands,
            LimiterMode::On {
                table: &table,
                patches: &patches,
            },
            &mut out,
        )
        .expect("哨兵结果应可生成");
        let snapshot = out.clone();

        // 限幅器表铺的不是本区间的频率范围：映射循环会读到表尾之外。
        assert_eq!(
            adjust(
                &estimated,
                &bands,
                LimiterMode::On {
                    table: &other_table,
                    patches: &other_patches,
                },
                &mut out,
            ),
            Err(GainError::LimiterRangeMismatch {
                first: 12,
                last: 46,
                sbx: 10,
                num_sb_aspx: 36
            })
        );

        // 频带表与包络估计的子带数不符。
        assert_eq!(
            adjust(
                &estimated,
                &other,
                LimiterMode::On {
                    table: &other_table,
                    patches: &other_patches,
                },
                &mut out,
            ),
            Err(GainError::SubbandCountMismatch {
                estimate: 36,
                bands: 34
            })
        );

        // 空的包络估计。
        let empty = EnvelopeEstimate::new();
        assert_eq!(
            adjust(
                &empty,
                &bands,
                LimiterMode::On {
                    table: &table,
                    patches: &patches,
                },
                &mut out,
            ),
            Err(GainError::EnvelopeCountOutOfRange { envelopes: 0 })
        );

        assert_eq!(out, snapshot, "被拒绝的输入不应改写任一矩阵");
    }
}
