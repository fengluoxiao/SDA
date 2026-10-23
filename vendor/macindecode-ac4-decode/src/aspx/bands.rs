//! A-SPX 子带组表的推导。
//!
//! 覆盖 `TS103190-1:v1.4.1` 的 `Pseudocode 67`–`Pseudocode 70`：由
//! `aspx_config` 的三个参数与 `aspx_xover_subband_offset` 推出主表、信号包络
//! 高低分辨率表与噪声表，以及后续语法元素的循环次数
//! `num_sbg_sig_highres`、`num_sbg_sig_lowres` 与 `num_sbg_noise`。
//!
//! 本模块不读比特，全部输入来自已解析的配置。

use super::tables::{MAX_SBG_MASTER, MAX_SBG_NOISE, sbg_template, template_group_count};
use core::fmt;

/// 信号包络低分辨率表的最大组数，见 `Pseudocode 69` 的二分之一抽取。
pub const MAX_SBG_SIG_LOWRES: usize = MAX_SBG_MASTER.div_ceil(2);

/// 子带组表推导失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandError {
    /// `aspx_start_freq` 超出 3 位范围。
    StartFrequencyOutOfRange {
        /// 码流给出的值。
        start_freq: u8,
    },
    /// `aspx_stop_freq` 超出 2 位范围。
    StopFrequencyOutOfRange {
        /// 码流给出的值。
        stop_freq: u8,
    },
    /// 起止频率组合把主子带组表压成了空表。
    ///
    /// `Pseudocode 67` 的 `num_sbg_master` 为零时没有任何频带可供扩展，低分辨
    /// 率模板配合 `aspx_start_freq == 7` 与 `aspx_stop_freq == 3` 即触发。
    EmptyMasterTable {
        /// `aspx_start_freq`。
        start_freq: u8,
        /// `aspx_stop_freq`。
        stop_freq: u8,
    },
    /// `aspx_xover_subband_offset` 不小于主子带组数，高分辨率表会成为空表。
    CrossoverOutOfRange {
        /// 码流给出的偏移。
        xover: u8,
        /// 主子带组数。
        num_sbg_master: u8,
    },
    /// `aspx_xover_subband_offset` 超出 3 位范围。
    CrossoverOffsetOutOfRange {
        /// 码流给出的偏移。
        xover: u8,
    },
    /// `aspx_noise_sbg` 超出 2 位范围。
    NoiseSbgOutOfRange {
        /// 码流给出的值。
        noise_sbg: u8,
    },
    /// 推出的噪声子带组数超过 `5.7.6.3.1.3` 规定的上界。
    TooManyNoiseGroups {
        /// 推导结果。
        num_sbg_noise: u8,
        /// 规范上界。
        limit: u8,
    },
}

impl fmt::Display for BandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BandError::StartFrequencyOutOfRange { start_freq } => {
                write!(f, "aspx_start_freq {start_freq} is outside 0..7")
            }
            BandError::StopFrequencyOutOfRange { stop_freq } => {
                write!(f, "aspx_stop_freq {stop_freq} is outside 0..3")
            }
            BandError::EmptyMasterTable {
                start_freq,
                stop_freq,
            } => write!(
                f,
                "aspx_start_freq {start_freq} and aspx_stop_freq {stop_freq} produce an empty master subband-group table"
            ),
            BandError::CrossoverOutOfRange {
                xover,
                num_sbg_master,
            } => write!(
                f,
                "aspx_xover_subband_offset {xover} is not less than master subband-group count {num_sbg_master}"
            ),
            BandError::CrossoverOffsetOutOfRange { xover } => {
                write!(f, "aspx_xover_subband_offset {xover} is outside 0..7")
            }
            BandError::NoiseSbgOutOfRange { noise_sbg } => {
                write!(f, "aspx_noise_sbg {noise_sbg} is outside 0..3")
            }
            BandError::TooManyNoiseGroups {
                num_sbg_noise,
                limit,
            } => write!(
                f,
                "Noise subband-group count {num_sbg_noise} exceeds limit {limit}"
            ),
        }
    }
}

impl core::error::Error for BandError {}

/// 主子带组表与由它派生的信号包络、噪声子带组表。
///
/// 字段定长，推导不分配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspxBandTables {
    master: [u8; MAX_SBG_MASTER + 1],
    num_sbg_master: u8,
    sig_highres: [u8; MAX_SBG_MASTER + 1],
    num_sbg_sig_highres: u8,
    sig_lowres: [u8; MAX_SBG_SIG_LOWRES + 1],
    num_sbg_sig_lowres: u8,
    noise: [u8; MAX_SBG_NOISE as usize + 1],
    num_sbg_noise: u8,
    sba: u8,
    sbx: u8,
    sbz: u8,
    num_sb_aspx: u8,
}

impl AspxBandTables {
    /// 一个未填充的实例。
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            master: [0; MAX_SBG_MASTER + 1],
            num_sbg_master: 0,
            sig_highres: [0; MAX_SBG_MASTER + 1],
            num_sbg_sig_highres: 0,
            sig_lowres: [0; MAX_SBG_SIG_LOWRES + 1],
            num_sbg_sig_lowres: 0,
            noise: [0; MAX_SBG_NOISE as usize + 1],
            num_sbg_noise: 0,
            sba: 0,
            sbx: 0,
            sbz: 0,
            num_sb_aspx: 0,
        }
    }

    /// 由配置参数与交叉子带偏移推出全部子带组表。
    ///
    /// `master_freq_scale`、`start_freq`、`stop_freq` 与 `noise_sbg` 来自
    /// `aspx_config()`，`xover` 来自 `aspx_data_1ch()` 或 `aspx_data_2ch()`。
    ///
    /// # Errors
    ///
    /// 见 [`BandError`]。
    pub fn derive(
        master_freq_scale: bool,
        start_freq: u8,
        stop_freq: u8,
        noise_sbg: u8,
        xover: u8,
    ) -> Result<Self, BandError> {
        if start_freq > 7 {
            return Err(BandError::StartFrequencyOutOfRange { start_freq });
        }
        if stop_freq > 3 {
            return Err(BandError::StopFrequencyOutOfRange { stop_freq });
        }
        if noise_sbg > 3 {
            return Err(BandError::NoiseSbgOutOfRange { noise_sbg });
        }
        if xover > 7 {
            return Err(BandError::CrossoverOffsetOutOfRange { xover });
        }

        let mut out = Self::empty();
        out.derive_master(master_freq_scale, start_freq, stop_freq)?;
        out.derive_sig_highres(xover)?;
        out.derive_sig_lowres();
        out.derive_noise(noise_sbg)?;
        Ok(out)
    }

    /// `Pseudocode 67`：从模板表截出主子带组表。
    fn derive_master(
        &mut self,
        master_freq_scale: bool,
        start_freq: u8,
        stop_freq: u8,
    ) -> Result<(), BandError> {
        let template = sbg_template(master_freq_scale);
        let trimmed = u16::from(start_freq)
            .saturating_add(u16::from(stop_freq))
            .saturating_mul(2);
        let count = u16::from(template_group_count(master_freq_scale)).saturating_sub(trimmed);
        if count == 0 {
            return Err(BandError::EmptyMasterTable {
                start_freq,
                stop_freq,
            });
        }
        let num_sbg_master = u8::try_from(count).unwrap_or(0);
        let base = usize::from(start_freq).saturating_mul(2);

        for sbg in 0..=usize::from(num_sbg_master) {
            let source = base.saturating_add(sbg);
            // 模板长度即组数加一，起止范围已由上面的 count 保证不越界。
            let Some(&border) = template.get(source) else {
                return Err(BandError::EmptyMasterTable {
                    start_freq,
                    stop_freq,
                });
            };
            if let Some(slot) = self.master.get_mut(sbg) {
                *slot = border;
            }
        }
        self.num_sbg_master = num_sbg_master;
        self.sba = self.master.first().copied().unwrap_or(0);
        self.sbz = self
            .master
            .get(usize::from(num_sbg_master))
            .copied()
            .unwrap_or(0);
        Ok(())
    }

    /// `Pseudocode 68`：高分辨率信号包络表是主表自交叉子带起的后缀。
    fn derive_sig_highres(&mut self, xover: u8) -> Result<(), BandError> {
        if xover >= self.num_sbg_master {
            return Err(BandError::CrossoverOutOfRange {
                xover,
                num_sbg_master: self.num_sbg_master,
            });
        }
        let count = self.num_sbg_master.saturating_sub(xover);
        for sbg in 0..=usize::from(count) {
            let source = usize::from(xover).saturating_add(sbg);
            let border = self.master.get(source).copied().unwrap_or(0);
            if let Some(slot) = self.sig_highres.get_mut(sbg) {
                *slot = border;
            }
        }
        self.num_sbg_sig_highres = count;
        self.sbx = self.sig_highres.first().copied().unwrap_or(0);
        let top = self
            .sig_highres
            .get(usize::from(count))
            .copied()
            .unwrap_or(0);
        self.num_sb_aspx = top.saturating_sub(self.sbx);
        Ok(())
    }

    /// `Pseudocode 69`：低分辨率表是高分辨率表的二分之一抽取。
    ///
    /// 高分辨率组数为偶数时取偶数下标，为奇数时取奇数下标；两种情形都保证
    /// 末项落在高分辨率表的末项上。
    fn derive_sig_lowres(&mut self) {
        let high = self.num_sbg_sig_highres;
        let count = high.saturating_sub(high / 2);
        self.sig_lowres = [0; MAX_SBG_SIG_LOWRES + 1];
        if let (Some(dst), Some(&src)) = (self.sig_lowres.first_mut(), self.sig_highres.first()) {
            *dst = src;
        }
        let odd = high % 2 == 1;
        for sbg in 1..=usize::from(count) {
            let doubled = sbg.saturating_mul(2);
            let source = if odd {
                doubled.saturating_sub(1)
            } else {
                doubled
            };
            let border = self.sig_highres.get(source).copied().unwrap_or(0);
            if let Some(slot) = self.sig_lowres.get_mut(sbg) {
                *slot = border;
            }
        }
        self.num_sbg_sig_lowres = count;
    }

    /// `Pseudocode 70`：噪声表由低分辨率表按组数均分得到。
    fn derive_noise(&mut self, noise_sbg: u8) -> Result<(), BandError> {
        let count = num_sbg_noise(noise_sbg, self.sbx, self.sbz)?;
        let mut idx = 0usize;
        if let (Some(dst), Some(&src)) = (self.noise.first_mut(), self.sig_lowres.first()) {
            *dst = src;
        }
        for sbg in 1..=usize::from(count) {
            let remaining = usize::from(self.num_sbg_sig_lowres).saturating_sub(idx);
            let divisor = usize::from(count)
                .saturating_add(1)
                .saturating_sub(sbg)
                .max(1);
            let step = remaining.checked_div(divisor).unwrap_or(0);
            idx = idx.saturating_add(step);
            let border = self.sig_lowres.get(idx).copied().unwrap_or(0);
            if let Some(slot) = self.noise.get_mut(sbg) {
                *slot = border;
            }
        }
        self.num_sbg_noise = count;
        Ok(())
    }

    /// `num_sbg_master`。
    #[must_use]
    pub const fn num_sbg_master(&self) -> u8 {
        self.num_sbg_master
    }

    /// `num_sbg_sig_highres`，即 `aspx_ec_data()` 高分辨率包络的循环次数。
    #[must_use]
    pub const fn num_sbg_sig_highres(&self) -> u8 {
        self.num_sbg_sig_highres
    }

    /// `num_sbg_sig_lowres`，即 `aspx_ec_data()` 低分辨率包络的循环次数。
    #[must_use]
    pub const fn num_sbg_sig_lowres(&self) -> u8 {
        self.num_sbg_sig_lowres
    }

    /// `num_sbg_noise`，即噪声包络与 `aspx_tna_mode` 的循环次数。
    #[must_use]
    pub const fn num_sbg_noise(&self) -> u8 {
        self.num_sbg_noise
    }

    /// `sba`，主子带组表的下边界。
    #[must_use]
    pub const fn sba(&self) -> u8 {
        self.sba
    }

    /// `sbx`，交叉子带。
    #[must_use]
    pub const fn sbx(&self) -> u8 {
        self.sbx
    }

    /// `sbz`，主子带组表的上边界。
    #[must_use]
    pub const fn sbz(&self) -> u8 {
        self.sbz
    }

    /// `num_sb_aspx`，A-SPX 频率范围内的子带数。
    #[must_use]
    pub const fn num_sb_aspx(&self) -> u8 {
        self.num_sb_aspx
    }

    /// 主子带组表的第 `index` 个边界。
    #[must_use]
    pub fn master_border(&self, index: usize) -> Option<u8> {
        if self.num_sbg_master == 0 || index > usize::from(self.num_sbg_master) {
            return None;
        }
        self.master.get(index).copied()
    }

    /// 高分辨率信号包络表的第 `index` 个边界。
    #[must_use]
    pub fn sig_highres_border(&self, index: usize) -> Option<u8> {
        if self.num_sbg_sig_highres == 0 || index > usize::from(self.num_sbg_sig_highres) {
            return None;
        }
        self.sig_highres.get(index).copied()
    }

    /// 低分辨率信号包络表的第 `index` 个边界。
    #[must_use]
    pub fn sig_lowres_border(&self, index: usize) -> Option<u8> {
        if self.num_sbg_sig_lowres == 0 || index > usize::from(self.num_sbg_sig_lowres) {
            return None;
        }
        self.sig_lowres.get(index).copied()
    }

    /// 噪声表的第 `index` 个边界。
    #[must_use]
    pub fn noise_border(&self, index: usize) -> Option<u8> {
        if self.num_sbg_noise == 0 || index > usize::from(self.num_sbg_noise) {
            return None;
        }
        self.noise.get(index).copied()
    }
}

/// `Pseudocode 70` 的 `num_sbg_noise = max(1, floor(n * log2(sbz/sbx) + 0.5))`。
///
/// 规范用浮点对数，本实现改用等价的整数判据。取 `n = aspx_noise_sbg`，则
/// 结果为 `k` 当且仅当 `k` 是使 `2 * sbz^(2n) >= 4^k * sbx^(2n)` 成立的最大
/// 整数：把 `n * log2(sbz/sbx) >= k - 0.5` 两边乘 2 再取 2 的幂即可消去半整
/// 数与对数。`n` 至多为 3、子带号至多为 62，故 `62^6` 的中间量在 `u64` 内。
///
/// # Errors
///
/// 结果超过 [`MAX_SBG_NOISE`] 时返回 [`BandError::TooManyNoiseGroups`]。
fn num_sbg_noise(noise_sbg: u8, sbx: u8, sbz: u8) -> Result<u8, BandError> {
    let exponent = u32::from(noise_sbg).saturating_mul(2);
    let Some(high) = u64::from(sbz).checked_pow(exponent) else {
        return Err(BandError::NoiseSbgOutOfRange { noise_sbg });
    };
    let Some(low) = u64::from(sbx).checked_pow(exponent) else {
        return Err(BandError::NoiseSbgOutOfRange { noise_sbg });
    };
    let Some(bound) = high.checked_mul(2) else {
        return Err(BandError::NoiseSbgOutOfRange { noise_sbg });
    };

    let limit = MAX_SBG_NOISE.saturating_add(1);
    let mut groups = 0u8;
    while groups < limit {
        let next = groups.saturating_add(1);
        let Some(scale) = 4u64.checked_pow(u32::from(next)) else {
            break;
        };
        let Some(threshold) = low.checked_mul(scale) else {
            break;
        };
        if bound < threshold {
            break;
        }
        groups = next;
    }

    let result = groups.max(1);
    if result > MAX_SBG_NOISE {
        return Err(BandError::TooManyNoiseGroups {
            num_sbg_noise: result,
            limit: MAX_SBG_NOISE,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aspx::tables::sbg_template;

    /// 枚举全部合法配置组合，对每一个执行 `body`。
    fn for_each_config(mut body: impl FnMut(bool, u8, u8, u8, u8, &AspxBandTables)) {
        for scale in [false, true] {
            for start in 0u8..=7 {
                for stop in 0u8..=3 {
                    for noise in 0u8..=3 {
                        for xover in 0u8..=7 {
                            if let Ok(tables) =
                                AspxBandTables::derive(scale, start, stop, noise, xover)
                            {
                                body(scale, start, stop, noise, xover, &tables);
                            }
                        }
                    }
                }
            }
        }
    }

    /// 主表是模板表的连续切片，且首尾即 `sba` 与 `sbz`。
    #[test]
    fn master_table_is_a_slice_of_the_template() {
        for_each_config(|scale, start, _stop, _noise, _xover, tables| {
            let template = sbg_template(scale);
            let base = usize::from(start).saturating_mul(2);
            for sbg in 0..=usize::from(tables.num_sbg_master()) {
                assert_eq!(
                    tables.master_border(sbg),
                    template.get(base.saturating_add(sbg)).copied(),
                    "主表第 {sbg} 项应取自模板表"
                );
            }
            assert_eq!(tables.master_border(0), Some(tables.sba()));
            assert_eq!(
                tables.master_border(usize::from(tables.num_sbg_master())),
                Some(tables.sbz())
            );
        });
    }

    /// 三张派生表都严格递增，且下界依次抬高。
    #[test]
    fn derived_tables_are_strictly_increasing() {
        for_each_config(|_scale, _start, _stop, _noise, _xover, tables| {
            let check = |count: u8, get: &dyn Fn(usize) -> Option<u8>, name: &str| {
                for sbg in 0..usize::from(count) {
                    let (Some(low), Some(high)) = (get(sbg), get(sbg.saturating_add(1))) else {
                        panic!("{name} 第 {sbg} 项缺失");
                    };
                    assert!(low < high, "{name} 必须严格递增：{low} 之后是 {high}");
                }
            };
            check(
                tables.num_sbg_master(),
                &|i| tables.master_border(i),
                "主表",
            );
            check(
                tables.num_sbg_sig_highres(),
                &|i| tables.sig_highres_border(i),
                "高分辨率表",
            );
            check(
                tables.num_sbg_sig_lowres(),
                &|i| tables.sig_lowres_border(i),
                "低分辨率表",
            );
            check(
                tables.num_sbg_noise(),
                &|i| tables.noise_border(i),
                "噪声表",
            );
            assert!(tables.sba() <= tables.sbx(), "交叉子带不低于主表下界");
            assert!(tables.sbx() < tables.sbz(), "A-SPX 范围必须非空");
        });
    }

    /// 三张派生表的边界都必须是主表边界的子集，且末项一致。
    ///
    /// 这是最强的一条结构判据：任何抽取下标写错都会让某一项落在主表之外。
    #[test]
    fn derived_borders_come_from_the_master_table() {
        for_each_config(|_scale, _start, _stop, _noise, _xover, tables| {
            let in_master = |value: u8| {
                (0..=usize::from(tables.num_sbg_master()))
                    .any(|i| tables.master_border(i) == Some(value))
            };
            for sbg in 0..=usize::from(tables.num_sbg_sig_highres()) {
                let Some(border) = tables.sig_highres_border(sbg) else {
                    panic!("高分辨率表第 {sbg} 项缺失");
                };
                assert!(in_master(border), "高分辨率边界 {border} 不在主表内");
            }
            for sbg in 0..=usize::from(tables.num_sbg_sig_lowres()) {
                let Some(border) = tables.sig_lowres_border(sbg) else {
                    panic!("低分辨率表第 {sbg} 项缺失");
                };
                assert!(in_master(border), "低分辨率边界 {border} 不在主表内");
            }
            for sbg in 0..=usize::from(tables.num_sbg_noise()) {
                let Some(border) = tables.noise_border(sbg) else {
                    panic!("噪声表第 {sbg} 项缺失");
                };
                assert!(in_master(border), "噪声边界 {border} 不在主表内");
            }
            let top = tables.sig_highres_border(usize::from(tables.num_sbg_sig_highres()));
            assert_eq!(top, Some(tables.sbz()), "高分辨率表末项应为 sbz");
            let low_top = tables.sig_lowres_border(usize::from(tables.num_sbg_sig_lowres()));
            assert_eq!(low_top, Some(tables.sbz()), "低分辨率表末项应为 sbz");
            let noise_top = tables.noise_border(usize::from(tables.num_sbg_noise()));
            assert_eq!(noise_top, Some(tables.sbz()), "噪声表末项应为 sbz");
        });
    }

    /// 低分辨率表恰好是高分辨率表的二分之一抽取。
    #[test]
    fn lowres_is_a_halving_of_highres() {
        for_each_config(|_scale, _start, _stop, _noise, _xover, tables| {
            let high = tables.num_sbg_sig_highres();
            let low = tables.num_sbg_sig_lowres();
            assert_eq!(
                usize::from(low),
                usize::from(high).div_ceil(2),
                "低分辨率组数应为高分辨率的一半向上取整"
            );
            assert!(low <= MAX_SBG_SIG_LOWRES as u8);
        });
    }

    /// `num_sb_aspx` 等于高分辨率表首尾之差。
    #[test]
    fn aspx_range_width_matches_the_borders() {
        for_each_config(|_scale, _start, _stop, _noise, _xover, tables| {
            assert_eq!(
                tables.num_sb_aspx(),
                tables.sbz().saturating_sub(tables.sbx()),
                "num_sb_aspx 应为 sbz 与 sbx 之差"
            );
            assert!(tables.num_sb_aspx() > 0);
        });
    }

    /// 噪声组数落在 1 到 5 之间，且 `aspx_noise_sbg` 为 0 时恒为 1。
    #[test]
    fn noise_group_count_stays_within_the_specified_bound() {
        for_each_config(|_scale, _start, _stop, noise, _xover, tables| {
            let count = tables.num_sbg_noise();
            assert!(
                (1..=MAX_SBG_NOISE).contains(&count),
                "噪声组数 {count} 越界"
            );
            if noise == 0 {
                assert_eq!(count, 1, "aspx_noise_sbg 为 0 时对数项为零");
            }
            assert!(
                count <= tables.num_sbg_sig_lowres().max(1),
                "噪声组不能多于低分辨率组"
            );
        });
    }

    /// 整数判据必须与 `floor(n * log2(sbz/sbx) + 0.5)` 的浮点定义逐点一致。
    ///
    /// 浮点只出现在测试里；实现侧不引入任何浮点运算。
    #[test]
    fn integer_criterion_matches_the_floating_point_definition() {
        extern crate std;
        let mut checked = 0usize;
        for noise in 0u8..=3 {
            for sbx in 1u8..=62 {
                for sbz in sbx.saturating_add(1)..=62 {
                    let ratio = f64::from(sbz) / f64::from(sbx);
                    let exact = f64::from(noise) * ratio.log2() + 0.5;
                    let expected = {
                        let floored = exact.floor();
                        if floored < 1.0 { 1u32 } else { floored as u32 }
                    };
                    let actual = num_sbg_noise(noise, sbx, sbz);
                    if expected > u32::from(MAX_SBG_NOISE) {
                        assert!(actual.is_err(), "noise={noise} sbx={sbx} sbz={sbz} 应超界");
                    } else {
                        assert_eq!(
                            actual.map(u32::from),
                            Ok(expected),
                            "noise={noise} sbx={sbx} sbz={sbz} 的噪声组数不符"
                        );
                    }
                    checked = checked.saturating_add(1);
                }
            }
        }
        assert!(checked > 7_000, "覆盖面不足：只比对了 {checked} 组");
    }

    /// 低分辨率模板配合最大起止频率会把主表压空，必须拒绝而非给出空表。
    #[test]
    fn rejects_the_degenerate_start_stop_combination() {
        assert_eq!(
            AspxBandTables::derive(false, 7, 3, 0, 0),
            Err(BandError::EmptyMasterTable {
                start_freq: 7,
                stop_freq: 3,
            })
        );
        // 高分辨率模板多两个组，同样的起止组合仍然合法。
        assert!(AspxBandTables::derive(true, 7, 3, 0, 0).is_ok());
    }

    /// 交叉子带偏移不得吃掉全部主子带组。
    #[test]
    fn rejects_crossover_at_or_past_the_master_count() {
        let Ok(tables) = AspxBandTables::derive(true, 7, 3, 0, 0) else {
            panic!("该组合应当合法");
        };
        let count = tables.num_sbg_master();
        assert_eq!(
            AspxBandTables::derive(true, 7, 3, 0, count),
            Err(BandError::CrossoverOutOfRange {
                xover: count,
                num_sbg_master: count,
            })
        );
    }

    /// 超出位宽的配置值必须在推导前拒绝。
    #[test]
    fn rejects_out_of_range_configuration_values() {
        assert_eq!(
            AspxBandTables::derive(true, 8, 0, 0, 0),
            Err(BandError::StartFrequencyOutOfRange { start_freq: 8 })
        );
        assert_eq!(
            AspxBandTables::derive(true, 0, 4, 0, 0),
            Err(BandError::StopFrequencyOutOfRange { stop_freq: 4 })
        );
        assert_eq!(
            AspxBandTables::derive(true, 0, 0, 4, 0),
            Err(BandError::NoiseSbgOutOfRange { noise_sbg: 4 })
        );
        assert_eq!(
            AspxBandTables::derive(true, 0, 0, 0, 8),
            Err(BandError::CrossoverOffsetOutOfRange { xover: 8 })
        );
    }

    /// 未填充实例的公开查询一律为空，避免暴露零值当作真实边界。
    #[test]
    fn empty_tables_expose_no_borders() {
        let tables = AspxBandTables::empty();
        assert_eq!(tables.num_sbg_master(), 0);
        assert_eq!(tables.num_sbg_sig_highres(), 0);
        assert_eq!(tables.num_sbg_sig_lowres(), 0);
        assert_eq!(tables.num_sbg_noise(), 0);
        for index in [0, 1] {
            assert_eq!(tables.master_border(index), None);
            assert_eq!(tables.sig_highres_border(index), None);
            assert_eq!(tables.sig_lowres_border(index), None);
            assert_eq!(tables.noise_border(index), None);
        }
    }
}
