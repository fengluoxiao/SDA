//! 限幅器子带组表（`TS103190-1:v1.4.1:5.7.6.3.1.5`）。
//!
//! `Pseudocode 72`–`74` 把低分辨率信号包络表与 patch 边界并成一张表，再把靠得
//! 太近的边界合掉，使每个八度大约留两个限幅器子带组。`5.7.6.4.2.2` 的
//! `Pseudocode 96` 与 `99` 在 `aspx_limiter == 1` 时用它算增益上限与 boost
//! 因子；关闭 limiter 时只执行初始增益 `Pseudocode 95`，不消费这张表。
//!
//! # 伪码的一处分号
//!
//! 原文第二个复制循环写作
//!
//! ```text
//! for (sbg = 1; sbg < num_sbg_patches; sbg++);
//! {
//!     sbg_lim[sbg+num_sbg_sig_lowres] = sbg_patches[sbg];
//! }
//! ```
//!
//! 行尾那个分号让循环体成为空语句，随后的块只在 `sbg == num_sbg_patches` 时执行
//! 一次。两条证据表明它是排印错误而非本意：一是只复制一个边界与「把 patch 边界
//! 并进限幅器表」的意图矛盾；二是 `sbg == num_sbg_patches` 时写入的下标
//! `num_sbg_patches + num_sbg_sig_lowres` 恰好比边界数上界大一格，按字面实现会
//! 越界写。这里按意图逐个复制。
//!
//! # 八度判据用 `log2`
//!
//! `num_octaves = log2(sbg_lim[sbg] / sbg_lim[sbg-1])` 与阈值 `0,245` 比较，是
//! 频带表推导里第一次需要 ADR-0005 的实数函数。等价的乘法形式
//! `sbg_lim[sbg] < sbg_lim[sbg-1] · 2^0,245` 少一次求值，但 `2^0,245` 是无理数，
//! 写成十进制常量会在边界比值上与规范分歧；这里照原文取 `log2`。
//!
//! # 终止边界 `sbz`
//!
//! 原文合并循环会在 32 组合法配置上删掉 `sbz`，与 `5.7.6.3.1` 「每张子带组
//! 表都包含最高组的上边界」冲突。`Pseudocode 96` 与 `100` 又要把全部
//! `num_sb_aspx` 个子带映射到该表；末项低于 `sbz` 会使映射越过已计算的组。
//! 因此实现把 `sbz` 与 patch 接缝一样视为不可删除的锚点：它与普通边界过近时
//! 删普通边界，与 patch 接缝过近时两者都保留。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "下标由两张已推导的表长度派生，越界即是判据要报告的失败"
)]

use crate::aspx::bands::{AspxBandTables, MAX_SBG_SIG_LOWRES};
use crate::aspx::patches::{MAX_SBG_PATCHES, PatchTable};
use macindecode_ac4_bitstream::math::log2;

/// 限幅器边界数的上界：低分辨率组数加 patch 段数。
pub const MAX_SBG_LIM: usize = MAX_SBG_SIG_LOWRES + MAX_SBG_PATCHES;

/// `Pseudocode 72` 的八度阈值。
const OCTAVE_THRESHOLD: f64 = 0.245;

/// 限幅器表无法推出的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimiterError {
    /// 合并前的边界数超出 [`MAX_SBG_LIM`]。
    TooManyBorders { borders: usize },
    /// 低分辨率表或 patch 表为空，并不出任何限幅器组。
    EmptySource {
        num_sbg_sig_lowres: u8,
        num_sbg_patches: u8,
    },
    /// 合并循环没有在与边界数成正比的轮数内收敛。
    ///
    /// 原文的 `continue` 不推进游标，只靠 `num_sbg_lim` 递减保证终止。
    LoopStuck { borders: usize },
}

/// `Pseudocode 72` 的限幅器子带组表。
#[derive(Debug, Clone, Copy)]
pub struct LimiterTable {
    borders: [u8; MAX_SBG_LIM + 1],
    count: u8,
    /// 推导时使用的完整频带布局，不参与有效表的相等性。
    source_bands: AspxBandTables,
    /// 推导时使用的 patch 表，不参与有效表的相等性。
    source_patches: PatchTable,
}

impl PartialEq for LimiterTable {
    fn eq(&self, other: &Self) -> bool {
        self.borders == other.borders && self.count == other.count
    }
}

impl Eq for LimiterTable {}

impl LimiterTable {
    /// 由低分辨率信号包络表与 patch 表推出限幅器表。
    ///
    /// # Errors
    ///
    /// 见 [`LimiterError`]。
    pub fn derive(tables: &AspxBandTables, patches: &PatchTable) -> Result<Self, LimiterError> {
        let lowres_groups = usize::from(tables.num_sbg_sig_lowres());
        let patch_count = usize::from(patches.count());
        if lowres_groups == 0 || patch_count == 0 {
            return Err(LimiterError::EmptySource {
                num_sbg_sig_lowres: tables.num_sbg_sig_lowres(),
                num_sbg_patches: patches.count(),
            });
        }

        // 边界数 = (lowres 组数 + 1) + (patch 段数 − 1)。
        let total = lowres_groups + patch_count;
        if total > MAX_SBG_LIM {
            return Err(LimiterError::TooManyBorders { borders: total });
        }

        let mut borders = [0u8; MAX_SBG_LIM + 1];
        for (sbg, slot) in borders.iter_mut().take(lowres_groups + 1).enumerate() {
            *slot = tables.sig_lowres_border(sbg).unwrap_or(0);
        }
        // 原文这里的分号让循环体落空，见模块文档；按意图逐个复制。
        for sbg in 1..patch_count {
            borders[sbg + lowres_groups] = patches.border(sbg).unwrap_or(0);
        }

        let mut count = total - 1;
        borders[..total].sort_unstable();
        let terminal = tables.sbz();

        // `continue` 不推进游标，每次删除让 `count` 减一；给与边界数成正比的预算。
        let mut budget = total * 2 + 4;
        let mut sbg = 1usize;
        while sbg <= count {
            let low = f64::from(borders[sbg - 1]);
            let high = f64::from(borders[sbg]);
            let octaves = log2(high / low);
            if octaves >= OCTAVE_THRESHOLD {
                sbg += 1;
                continue;
            }
            // 靠得太近：删掉其中一个。patch 边界与终止边界优先保留——
            // 前者标记搬运接缝，后者保证后续映射覆盖整个 A-SPX 范围。
            let drop = if borders[sbg] == borders[sbg - 1] {
                sbg
            } else if is_required_border(patches, terminal, borders[sbg]) {
                if is_required_border(patches, terminal, borders[sbg - 1]) {
                    sbg += 1;
                    continue;
                }
                sbg - 1
            } else {
                sbg
            };
            remove_element(&mut borders, count, drop);
            count -= 1;
            budget = budget
                .checked_sub(1)
                .ok_or(LimiterError::LoopStuck { borders: total })?;
        }

        Ok(Self {
            borders,
            count: count as u8,
            source_bands: *tables,
            source_patches: *patches,
        })
    }

    /// `num_sbg_lim`。
    #[must_use]
    pub const fn count(&self) -> u8 {
        self.count
    }

    /// `sbg_lim[index]`；长度为组数加一。
    #[must_use]
    pub fn border(&self, index: usize) -> Option<u8> {
        if index > usize::from(self.count) {
            return None;
        }
        self.borders.get(index).copied()
    }

    /// 这张表是否由给定的频带与 patch 表共同推出。
    ///
    /// 完整频带比较是可观察的来源契约：只改变 `aspx_noise_sbg` 时，频带表不同，
    /// 但 patch 与有效 limiter 表可以完全相同；此处仍拒绝这种错接，而不把相等的
    /// 输出表当成同源。见规范可追踪性 5.33。
    pub(super) fn matches_sources(&self, tables: &AspxBandTables, patches: &PatchTable) -> bool {
        self.source_bands == *tables && self.source_patches == *patches
    }
}

/// `Pseudocode 73` 的 `is_element_of_sbg_patches()`。
///
/// 循环取 `i <= num_sbg_patches`，因此包含最后一个边界。
fn is_patch_border(patches: &PatchTable, value: u8) -> bool {
    (0..=usize::from(patches.count())).any(|index| patches.border(index) == Some(value))
}

/// patch 接缝与 A-SPX 终止边界都不能被合并掉。
fn is_required_border(patches: &PatchTable, terminal: u8, value: u8) -> bool {
    value == terminal || is_patch_border(patches, value)
}

/// `Pseudocode 74` 的 `remove_element()`。
fn remove_element(borders: &mut [u8; MAX_SBG_LIM + 1], count: usize, index: usize) {
    for i in index..count {
        borders[i] = borders[i + 1];
    }
    // `LimiterTable` 派生 `Eq`，无效后缀也会参与比较；每次删除后都要
    // 规范化刚失效的末槽，使相同的有效表有相同的表示。
    borders[count] = 0;
}

/// 全部 904 组合法配置的输入与限幅器表按固定顺序串联后的 FNV-1a 摘要。
///
/// 与 patch 表分开算：混成一个摘要时，不符只说明「两张表之一变了」，定位不到
/// 是哪张。字面量由 `scripts/check_patch_tables.py --sweep` 的独立参考实现复算。
#[cfg(test)]
const LIMITER_SWEEP_FNV64: u64 = 0xb967_383b_3bb7_e5c5;

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_CONFIGURATION_COUNT: usize = 904;
    const FNV64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

    fn hash_byte(digest: &mut u64, value: u8) {
        *digest ^= u64::from(value);
        *digest = digest.wrapping_mul(FNV64_PRIME);
    }

    #[test]
    fn the_full_sweep_matches_the_independent_reference_digest() {
        let mut digest = FNV64_OFFSET_BASIS;
        let seen = for_every_configuration_keyed(|scale, sf, pf, xo, b48, limits| {
            for value in [u8::from(scale), sf, pf, xo, u8::from(b48), limits.count()] {
                hash_byte(&mut digest, value);
            }
            for index in 0..=usize::from(limits.count()) {
                hash_byte(&mut digest, limits.border(index).expect("边界应存在"));
            }
            // 合法字段都小于 255，用不可混淆的分隔符结束一组配置。
            hash_byte(&mut digest, u8::MAX);
        });
        assert_eq!(seen, SPEC_CONFIGURATION_COUNT, "扫描到的配置数变了");
        assert_eq!(
            digest, LIMITER_SWEEP_FNV64,
            "限幅器表全配置摘要不符（实际 0x{digest:016x}）"
        );
    }

    /// 与 [`for_every_configuration`] 同一趟扫描，但把配置键也交给回调。
    fn for_every_configuration_keyed(
        mut check: impl FnMut(bool, u8, u8, u8, bool, LimiterTable),
    ) -> usize {
        let mut seen = 0usize;
        for master_freq_scale in [false, true] {
            for start_freq in 0..8 {
                for stop_freq in 0..4 {
                    for xover in 0..8 {
                        let Ok(tables) = AspxBandTables::derive(
                            master_freq_scale,
                            start_freq,
                            stop_freq,
                            0,
                            xover,
                        ) else {
                            continue;
                        };
                        for base_48 in [false, true] {
                            let Ok(patches) =
                                PatchTable::derive(&tables, master_freq_scale, base_48)
                            else {
                                continue;
                            };
                            let limits = LimiterTable::derive(&tables, &patches)
                                .expect("合法频带表应能推出限幅器表");
                            check(
                                master_freq_scale,
                                start_freq,
                                stop_freq,
                                xover,
                                base_48,
                                limits,
                            );
                            seen += 1;
                        }
                    }
                }
            }
        }
        seen
    }

    /// 遍历全部合法配置。`derive` 拒绝的组合由各自的模块判据管。
    fn for_every_configuration(
        mut check: impl FnMut(&AspxBandTables, &PatchTable, LimiterTable),
    ) -> usize {
        let mut seen = 0usize;
        for master_freq_scale in [false, true] {
            for start_freq in 0..8 {
                for stop_freq in 0..4 {
                    for xover in 0..8 {
                        let Ok(tables) = AspxBandTables::derive(
                            master_freq_scale,
                            start_freq,
                            stop_freq,
                            0,
                            xover,
                        ) else {
                            continue;
                        };
                        for base_48 in [false, true] {
                            let Ok(patches) =
                                PatchTable::derive(&tables, master_freq_scale, base_48)
                            else {
                                continue;
                            };
                            let limits = LimiterTable::derive(&tables, &patches)
                                .expect("合法频带表应能推出限幅器表");
                            check(&tables, &patches, limits);
                            seen += 1;
                        }
                    }
                }
            }
        }
        assert!(seen > 100, "扫描到的配置太少（{seen}）");
        seen
    }

    #[test]
    fn the_borders_are_strictly_increasing() {
        // 合并的第一件事就是消掉重复边界；剩下的必须严格递增。
        for_every_configuration(|_, _, limits| {
            for index in 0..usize::from(limits.count()) {
                let low = limits.border(index).expect("边界应存在");
                let high = limits.border(index + 1).expect("边界应存在");
                assert!(low < high, "限幅器边界必须严格递增：{low} → {high}");
            }
        });
    }

    #[test]
    fn the_table_spans_the_whole_aspx_range() {
        // `5.7.6.3.1` 要求子带组表包含最高组的上边界；这也是
        // `Pseudocode 96`/`100` 把所有 A-SPX 子带映射到限幅器组的前提。
        for_every_configuration(|tables, _, limits| {
            assert_eq!(
                limits.border(0),
                tables.sig_lowres_border(0),
                "首项应与低分辨率表一致，即 sbx"
            );
            let last = limits.border(usize::from(limits.count())).unwrap_or(0);
            assert_eq!(last, tables.sbz(), "末项应为 sbz");
        });
    }

    #[test]
    fn every_aspx_subband_maps_to_an_existing_limiter_group() {
        for_every_configuration(|tables, _, limits| {
            for relative in 0..tables.num_sb_aspx() {
                let absolute = tables.sbx().saturating_add(relative);
                let mapped = (0..usize::from(limits.count())).any(|group| {
                    let low = limits.border(group).unwrap_or(0);
                    let high = limits.border(group + 1).unwrap_or(0);
                    low <= absolute && absolute < high
                });
                assert!(mapped, "A-SPX 子带 {absolute} 没有可用的限幅器组");
            }
        });
    }

    #[test]
    fn every_border_comes_from_one_of_the_two_sources() {
        // 合并只删不造：每个留下的边界都必须在低分辨率表或 patch 表里。
        for_every_configuration(|tables, patches, limits| {
            for index in 0..=usize::from(limits.count()) {
                let value = limits.border(index).expect("边界应存在");
                let from_lowres = (0..=usize::from(tables.num_sbg_sig_lowres()))
                    .any(|i| tables.sig_lowres_border(i) == Some(value));
                let from_patch = is_patch_border(patches, value);
                assert!(from_lowres || from_patch, "边界 {value} 不来自任何源表");
            }
        });
    }

    #[test]
    fn adjacent_borders_are_far_enough_apart_unless_both_are_required() {
        // 阈值写字面 0,245，不引用实现的常量。合并的出口只有两个：间隔够宽，
        // 或者两侧都是不可删除的 patch 接缝/`sbz` 锚点。
        const SPEC_THRESHOLD: f64 = 0.245;
        let mut both_required = 0usize;
        for_every_configuration(|tables, patches, limits| {
            for index in 0..usize::from(limits.count()) {
                let low = f64::from(limits.border(index).expect("边界应存在"));
                let high = f64::from(limits.border(index + 1).expect("边界应存在"));
                if log2(high / low) >= SPEC_THRESHOLD {
                    continue;
                }
                let terminal = tables.sbz();
                let low_required =
                    is_required_border(patches, terminal, limits.border(index).unwrap_or(0));
                let high_required =
                    is_required_border(patches, terminal, limits.border(index + 1).unwrap_or(0));
                assert!(
                    low_required && high_required,
                    "间隔不足 0,245 个八度的一对边界，两侧必须都是必留锚点"
                );
                both_required += 1;
            }
        });
        assert!(
            both_required > 0,
            "应有走到「两侧都是必留锚点」的配置，否则该分支未被检验"
        );
    }

    #[test]
    fn a_patch_seam_survives_a_close_lowres_border() {
        // 靠得太近而一侧是 patch 接缝时，删掉的必须是另一侧。逐配置核对：
        // 凡是被删掉的低分辨率边界，其近邻里应当有 patch 接缝留下。
        let mut checked = 0usize;
        for_every_configuration(|tables, patches, limits| {
            let kept: [bool; 64] = {
                let mut kept = [false; 64];
                for index in 0..=usize::from(limits.count()) {
                    kept[usize::from(limits.border(index).unwrap_or(0))] = true;
                }
                kept
            };
            for i in 0..=usize::from(tables.num_sbg_sig_lowres()) {
                let value = tables.sig_lowres_border(i).unwrap_or(0);
                if kept[usize::from(value)] || is_patch_border(patches, value) {
                    continue;
                }
                // 被删的低分辨率边界：它一定与某个留下的边界靠得太近。
                let close = (0..=usize::from(limits.count())).any(|index| {
                    let other = f64::from(limits.border(index).unwrap_or(0));
                    let v = f64::from(value);
                    other > 0.0 && v > 0.0 && log2(v.max(other) / v.min(other)) < 0.245
                });
                assert!(close, "被删的边界 {value} 与留下的边界都不靠近");
                checked += 1;
            }
        });
        assert!(checked > 0, "应有边界确实被删掉的配置");
    }

    #[test]
    fn accessors_reject_indices_past_the_end() {
        for_every_configuration(|_, _, limits| {
            let count = usize::from(limits.count());
            assert!(limits.border(count).is_some());
            assert_eq!(limits.border(count + 1), None);
        });
    }

    #[test]
    fn equality_ignores_the_derivation_history() {
        fn derive(scale: bool, start: u8, xover: u8, base_48: bool) -> LimiterTable {
            let tables = AspxBandTables::derive(scale, start, 0, 0, xover).expect("应能推出频带表");
            let patches = PatchTable::derive(&tables, scale, base_48).expect("应能推出 patch 表");
            LimiterTable::derive(&tables, &patches).expect("应能推出限幅器表")
        }

        // 两条推导路径的初始边界数不同，但有效输出相同。无效后缀
        // 若没有清零，派生的 `Eq` 会把它们错判为不等。
        let a = derive(false, 0, 4, true);
        let b = derive(false, 2, 0, false);
        assert_eq!(a.count(), b.count());
        for index in 0..=usize::from(a.count()) {
            assert_eq!(a.border(index), b.border(index));
        }
        assert_eq!(a, b, "相等性不应受无效后缀影响");
    }

    #[test]
    fn the_group_count_stays_within_the_declared_bound() {
        const SPEC_MAX: usize = MAX_SBG_LIM;
        for_every_configuration(|_, _, limits| {
            assert!(usize::from(limits.count()) <= SPEC_MAX, "组数超出上界");
            assert!(limits.count() > 0, "至少要有一个限幅器组");
        });
    }
}
