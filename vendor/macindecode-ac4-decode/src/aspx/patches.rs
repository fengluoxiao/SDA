//! HF patch 子带组表（`TS103190-1:v1.4.1:5.7.6.3.1.4`）。
//!
//! `Pseudocode 71` 说明 HF 生成把低带的哪几段搬到 A-SPX 范围里：`patch_num_sb`
//! 给出每段的子带数，`patch_start_sb` 给出该段在低带里的源起点，`patches` 给出
//! 段边界在 A-SPX 范围内的位置。
//!
//! 本表**不随 [`AspxBandTables`] 一起推导**。它多要两个输入：`base_samp_freq`
//! 是 TOC 级的采样率，`aspx_master_freq_scale` 是 `aspx_config()` 的字段而频带
//! 表推完就不再持有它。把采样率塞进语法层的配置结构会让那个结构不再只是「码流
//! 里读到的东西」，因此 patch 表单独作为一次派生。
//!
//! # 源区间恒落在低带内
//!
//! 每段的源是 `[start_sb, start_sb + num_sb)`，而
//! `start_sb + num_sb = sba − odd`，因此上端恒不超过 `sba`。下端非负则由内层
//! `while` 保证：它退出时 `num_sb ≤ sba − source_band_low + msb − odd − usb`，
//! 首轮 `msb = sba` 且 `usb = sbx ≥ sba`，其后 `msb = usb`，两种情形都给出
//! `start_sb ≥ source_band_low > 0`。这条不变量是 patch 表有意义的前提——源必须
//! 是已经解出来的低带。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "下标由主子带组表长度与 5 段上限派生，越界即是判据要报告的失败"
)]

use crate::aspx::bands::AspxBandTables;
use crate::aspx::tables::MAX_SBG_MASTER;

/// `5.7.6.3.1.4` 规定 `num_sbg_patches ≤ 5`。
pub const MAX_SBG_PATCHES: usize = 5;

/// patch 表无法推出的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchError {
    /// patch 的源要低于第 0 个主子带组，配置不可满足。
    ///
    /// `Pseudocode 71` 的内层 `while` 没有下界，C 里会读到 `sbg_master[-1]`。
    /// 触发条件是 `sba < source_band_low + odd`，即主表下界比 patch 要求的源
    /// 深度还浅。
    SourceExhausted {
        /// 主子带组表的下界。
        sba: u8,
        /// 由 `aspx_master_freq_scale` 决定的 4 或 2。
        source_band_low: u8,
    },
    /// 推出的段数超过 `5.7.6.3.1.4` 规定的上界。
    TooManyPatches {
        /// 规范上界。
        limit: u8,
    },
    /// `do`-`while` 没有在与表长成正比的轮数内落到上界。
    ///
    /// 原文的唯一出口是 `sb` 恰好等于 `sbx + num_sb_aspx`，没有别的终止条件。
    LoopStuck {
        /// 主子带组数。
        num_sbg_master: u8,
    },
}

/// `Pseudocode 71` 的 patch 子带组表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchTable {
    num_sb: [u8; MAX_SBG_PATCHES],
    start_sb: [u8; MAX_SBG_PATCHES],
    borders: [u8; MAX_SBG_PATCHES + 1],
    count: u8,
}

impl PatchTable {
    /// 由频带表与两个外部输入推出 patch 表。
    ///
    /// `master_freq_scale` 来自 `aspx_config()`，`base_samp_freq_48` 由 TOC 的
    /// `fs_index` 给出（`1` 为 48 kHz）。
    ///
    /// # Errors
    ///
    /// 见 [`PatchError`]。
    pub fn derive(
        tables: &AspxBandTables,
        master_freq_scale: bool,
        base_samp_freq_48: bool,
    ) -> Result<Self, PatchError> {
        let sba = i32::from(tables.sba());
        let sbx = i32::from(tables.sbx());
        let sbz = i32::from(tables.sbz());
        let goal_sb: i32 = if base_samp_freq_48 { 43 } else { 46 };
        let source_band_low: i32 = if master_freq_scale { 4 } else { 2 };
        let num_master = usize::from(tables.num_sbg_master());
        let master = |index: usize| -> i32 { tables.master_border(index).map_or(sbz, i32::from) };

        // 起点：`goal_sb` 落在 A-SPX 范围内时从跨过它的那一组开始，否则从表尾。
        let mut sbg = if goal_sb < sbz {
            let mut found = 0usize;
            let mut index = 0usize;
            while index <= num_master && master(index) < goal_sb {
                found = index + 1;
                index += 1;
            }
            found
        } else {
            num_master
        };

        let mut num_sb_out = [0u8; MAX_SBG_PATCHES];
        let mut start_sb_out = [0u8; MAX_SBG_PATCHES];
        let mut msb = sba;
        let mut usb = sbx;
        let mut count = 0usize;
        // 每轮至多消费一个主子带组，或把 `sbg` 弹回表尾一次；取两倍余量。
        let mut budget = 2 * (num_master + MAX_SBG_PATCHES) + 4;

        loop {
            let mut j = sbg;
            let mut sb = master(j);
            let mut odd = (sb - 2 + sba).rem_euclid(2);
            while sb > sba - source_band_low + msb - odd {
                if j == 0 {
                    return Err(PatchError::SourceExhausted {
                        sba: tables.sba(),
                        source_band_low: source_band_low as u8,
                    });
                }
                j -= 1;
                sb = master(j);
                odd = (sb - 2 + sba).rem_euclid(2);
            }

            let num_sb = (sb - usb).max(0);
            if num_sb > 0 {
                if count >= MAX_SBG_PATCHES {
                    return Err(PatchError::TooManyPatches {
                        limit: MAX_SBG_PATCHES as u8,
                    });
                }
                let start_sb = sba - odd - num_sb;
                debug_assert!(start_sb >= 0, "patch source start falls below the low band");
                debug_assert!(start_sb + num_sb <= sba, "patch source range crosses sba");
                num_sb_out[count] = num_sb as u8;
                start_sb_out[count] = start_sb as u8;
                usb = sb;
                msb = sb;
                count += 1;
            } else {
                msb = sbx;
            }

            if master(sbg) - sb < 3 {
                sbg = num_master;
            }
            if sb == sbz {
                break;
            }
            budget = budget.checked_sub(1).ok_or(PatchError::LoopStuck {
                num_sbg_master: tables.num_sbg_master(),
            })?;
        }

        // 末段太短就并掉。原文把 `num_sbg_patches > 1` 写在 `&&` 右边，而左边
        // 已经读了 `[num_sbg_patches-1]`；`count == 0` 时那是 `[-1]`。
        if count > 1 && num_sb_out[count - 1] < 3 {
            count -= 1;
        }

        let mut borders = [0u8; MAX_SBG_PATCHES + 1];
        borders[0] = tables.sbx();
        for index in 1..=count {
            borders[index] = borders[index - 1].saturating_add(num_sb_out[index - 1]);
        }

        Ok(Self {
            num_sb: num_sb_out,
            start_sb: start_sb_out,
            borders,
            count: count as u8,
        })
    }

    /// 直接装配一张 patch 表，供 `hfgen` 的判据构造已知的搬运布局。
    ///
    /// 只做前缀和，不跑 `Pseudocode 71`——那条路径由本模块自己的判据覆盖。
    #[cfg(test)]
    pub(crate) fn from_parts(num_sb: &[u8], start_sb: &[u8], sbx: u8) -> Self {
        assert_eq!(
            num_sb.len(),
            start_sb.len(),
            "arrays must have equal length"
        );
        assert!(
            num_sb.len() <= MAX_SBG_PATCHES,
            "segment count exceeds the specification limit"
        );
        let mut out = Self {
            num_sb: [0; MAX_SBG_PATCHES],
            start_sb: [0; MAX_SBG_PATCHES],
            borders: [0; MAX_SBG_PATCHES + 1],
            count: num_sb.len() as u8,
        };
        out.borders[0] = sbx;
        for (index, (&num, &start)) in num_sb.iter().zip(start_sb).enumerate() {
            out.num_sb[index] = num;
            out.start_sb[index] = start;
            out.borders[index + 1] = out.borders[index].saturating_add(num);
        }
        out
    }

    /// `num_sbg_patches`。
    #[must_use]
    pub const fn count(&self) -> u8 {
        self.count
    }

    /// `sbg_patch_num_sb[index]`。
    #[must_use]
    pub fn num_sb(&self, index: usize) -> Option<u8> {
        if index >= usize::from(self.count) {
            return None;
        }
        self.num_sb.get(index).copied()
    }

    /// `sbg_patch_start_sb[index]`，该段在低带里的源起点。
    #[must_use]
    pub fn start_sb(&self, index: usize) -> Option<u8> {
        if index >= usize::from(self.count) {
            return None;
        }
        self.start_sb.get(index).copied()
    }

    /// `sbg_patches[index]`，段边界在 A-SPX 范围内的位置；长度为段数加一。
    #[must_use]
    pub fn border(&self, index: usize) -> Option<u8> {
        if index > usize::from(self.count) {
            return None;
        }
        self.borders.get(index).copied()
    }
}

const _: () = assert!(MAX_SBG_PATCHES <= MAX_SBG_MASTER);

/// `Pseudocode 71` 的锚点：`(master_freq_scale, start_freq, stop_freq, xover,
/// base_samp_freq_48, patch_num_sb, patch_start_sb, sbg_patches)`。
///
/// 覆盖 1…5 段的每一档、两个 `aspx_master_freq_scale`、采样率确实改变结果的
/// 配置，以及只有一个子带的段。`scripts/check_patch_tables.py` 用独立照抄的
/// 伪码复算这张表，并通过下面的全配置摘要核对全部 904 组合法配置；两边都改
/// 才能一起漂移。
///
/// 末尾六行是补上来的：`num_sb == 0` 的 `else` 分支（904 组里有 47 组走到，
/// 其中 `sbx != sba` 才看得出 `msb` 取 `sbx` 还是 `sba`）与「恰好一段且不足
/// 3 个子带」（24 组，决定并段条件写 `count > 1` 还是 `count > 0`）。这两个
/// 分支起初都没有锚点覆盖，对应的注入因此照常通过。
#[cfg(test)]
type PatchAnchor = (
    // aspx_master_freq_scale
    bool,
    // aspx_start_freq、aspx_stop_freq、aspx_xover_subband_offset
    u8,
    u8,
    u8,
    // base_samp_freq 为 48 kHz
    bool,
    // sbg_patch_num_sb、sbg_patch_start_sb、sbg_patches
    &'static [u8],
    &'static [u8],
    &'static [u8],
);

#[cfg(test)]
const PATCH_ANCHORS: &[PatchAnchor] = &[
    (
        false,
        0,
        0,
        0,
        false,
        &[8, 8, 6, 6, 8],
        &[2, 2, 4, 4, 2],
        &[10, 18, 26, 32, 38, 46],
    ),
    (
        false,
        0,
        0,
        7,
        false,
        &[1, 8, 6, 6, 8],
        &[9, 2, 4, 4, 2],
        &[17, 18, 26, 32, 38, 46],
    ),
    (
        false,
        0,
        0,
        7,
        true,
        &[1, 8, 6, 6, 8],
        &[9, 2, 4, 4, 2],
        &[17, 18, 26, 32, 38, 46],
    ),
    (
        false,
        0,
        1,
        0,
        false,
        &[8, 8, 6, 6],
        &[2, 2, 4, 4],
        &[10, 18, 26, 32, 38],
    ),
    (
        false,
        0,
        1,
        7,
        false,
        &[1, 8, 6, 6],
        &[9, 2, 4, 4],
        &[17, 18, 26, 32, 38],
    ),
    (
        false,
        0,
        2,
        0,
        false,
        &[8, 8, 6],
        &[2, 2, 4],
        &[10, 18, 26, 32],
    ),
    (false, 0, 3, 0, false, &[8, 8], &[2, 2], &[10, 18, 26]),
    (false, 2, 3, 0, false, &[12], &[2], &[14, 26]),
    (
        true,
        0,
        0,
        0,
        false,
        &[14, 12, 3, 12, 3],
        &[4, 6, 14, 5, 15],
        &[18, 32, 44, 47, 59, 62],
    ),
    (
        true,
        0,
        0,
        0,
        true,
        &[14, 12, 12, 6],
        &[4, 6, 6, 12],
        &[18, 32, 44, 56, 62],
    ),
    (
        true,
        0,
        0,
        1,
        false,
        &[13, 12, 3, 12, 3],
        &[5, 6, 14, 5, 15],
        &[19, 32, 44, 47, 59, 62],
    ),
    (
        true,
        0,
        0,
        1,
        true,
        &[13, 12, 12, 6],
        &[5, 6, 6, 12],
        &[19, 32, 44, 56, 62],
    ),
    (
        true,
        0,
        0,
        2,
        false,
        &[12, 12, 3, 12, 3],
        &[6, 6, 14, 5, 15],
        &[20, 32, 44, 47, 59, 62],
    ),
    (
        true,
        0,
        0,
        2,
        true,
        &[12, 12, 12, 6],
        &[6, 6, 6, 12],
        &[20, 32, 44, 56, 62],
    ),
    (
        true,
        0,
        0,
        3,
        false,
        &[11, 12, 3, 12, 3],
        &[7, 6, 14, 5, 15],
        &[21, 32, 44, 47, 59, 62],
    ),
    (
        true,
        0,
        0,
        3,
        true,
        &[11, 12, 12, 6],
        &[7, 6, 6, 12],
        &[21, 32, 44, 56, 62],
    ),
    (
        true,
        0,
        1,
        0,
        true,
        &[14, 12, 12],
        &[4, 6, 6],
        &[18, 32, 44, 56],
    ),
    (true, 0, 3, 0, false, &[14, 12], &[4, 6], &[18, 32, 44]),
    (true, 3, 3, 0, false, &[20], &[4], &[24, 44]),
    (true, 5, 0, 6, true, &[18], &[14], &[44, 62]),
    (true, 5, 0, 7, false, &[15], &[17], &[47, 62]),
    (true, 5, 0, 7, true, &[15], &[17], &[47, 62]),
    (false, 3, 3, 7, false, &[2], &[14], &[26, 28]),
    (false, 3, 3, 7, true, &[2], &[14], &[26, 28]),
    (false, 4, 2, 7, false, &[2], &[16], &[30, 32]),
];

/// 全部 904 组合法配置的输入与 patch 输出按固定顺序串联后的 FNV-1a 摘要。
///
/// 这个字面量由 `scripts/check_patch_tables.py --sweep` 的独立参考实现复算；Rust
/// 测试则从 [`PatchTable::derive`] 的真实输出计算。摘要不是安全用途，只把完整参考
/// 向量压成一个不需要分配或提交 904 行夹具的回归判据。
#[cfg(test)]
const PATCH_SWEEP_FNV64: u64 = 0x8f35_1fb5_db6a_0de2;

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

    fn hash_configuration(
        digest: &mut u64,
        master_freq_scale: bool,
        start_freq: u8,
        stop_freq: u8,
        xover: u8,
        base_samp_freq_48: bool,
        patches: PatchTable,
    ) {
        for value in [
            u8::from(master_freq_scale),
            start_freq,
            stop_freq,
            xover,
            u8::from(base_samp_freq_48),
            patches.count(),
        ] {
            hash_byte(digest, value);
        }
        for index in 0..usize::from(patches.count()) {
            hash_byte(digest, patches.num_sb(index).expect("段内应有子带数"));
            hash_byte(digest, patches.start_sb(index).expect("段内应有源起点"));
        }
        for index in 0..=usize::from(patches.count()) {
            hash_byte(digest, patches.border(index).expect("段内应有边界"));
        }
        // 所有合法字段都小于 255，用不可混淆的行分隔符结束一组配置。
        hash_byte(digest, u8::MAX);
    }

    /// 遍历 `aspx_config()` 与 `aspx_data()` 能给出的全部合法频带表。
    ///
    /// `derive` 拒绝的组合直接跳过——那是频带表自己的判据管的事。
    fn for_every_configuration(mut check: impl FnMut(&AspxBandTables, bool, bool, PatchTable)) {
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
                        for base_samp_freq_48 in [false, true] {
                            let patches =
                                PatchTable::derive(&tables, master_freq_scale, base_samp_freq_48)
                                    .expect("合法频带表应能推出 patch 表");
                            check(&tables, master_freq_scale, base_samp_freq_48, patches);
                            seen += 1;
                        }
                    }
                }
            }
        }
        assert!(seen > 100, "扫描到的配置太少（{seen}），判据可能没覆盖到");
    }

    #[test]
    fn the_anchor_table_is_reproduced_exactly() {
        for &(scale, start_freq, stop_freq, xover, base_48, nums, starts, borders) in PATCH_ANCHORS
        {
            let tables = AspxBandTables::derive(scale, start_freq, stop_freq, 0, xover)
                .expect("锚点配置应能推出频带表");
            let patches =
                PatchTable::derive(&tables, scale, base_48).expect("锚点应能推出 patch 表");
            // `no_std` 下没有 `format!`，配置直接摊进断言消息。
            assert_eq!(
                usize::from(patches.count()),
                nums.len(),
                "段数不符：scale={scale} start={start_freq} stop={stop_freq} xover={xover} f48={base_48}"
            );
            for (index, &want) in nums.iter().enumerate() {
                assert_eq!(
                    patches.num_sb(index),
                    Some(want),
                    "num_sb[{index}]：scale={scale} start={start_freq} stop={stop_freq} xover={xover} f48={base_48}"
                );
            }
            for (index, &want) in starts.iter().enumerate() {
                assert_eq!(
                    patches.start_sb(index),
                    Some(want),
                    "start_sb[{index}]：scale={scale} start={start_freq} stop={stop_freq} xover={xover} f48={base_48}"
                );
            }
            for (index, &want) in borders.iter().enumerate() {
                assert_eq!(
                    patches.border(index),
                    Some(want),
                    "border[{index}]：scale={scale} start={start_freq} stop={stop_freq} xover={xover} f48={base_48}"
                );
            }
        }
    }

    #[test]
    fn the_full_sweep_matches_the_independent_reference_digest() {
        let mut digest = FNV64_OFFSET_BASIS;
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
                        for base_samp_freq_48 in [false, true] {
                            let patches =
                                PatchTable::derive(&tables, master_freq_scale, base_samp_freq_48)
                                    .expect("合法频带表应能推出 patch 表");
                            hash_configuration(
                                &mut digest,
                                master_freq_scale,
                                start_freq,
                                stop_freq,
                                xover,
                                base_samp_freq_48,
                                patches,
                            );
                            seen += 1;
                        }
                    }
                }
            }
        }

        assert_eq!(seen, SPEC_CONFIGURATION_COUNT, "合法配置数漂移");
        assert_eq!(digest, PATCH_SWEEP_FNV64, "全配置输出与独立参考实现不一致");
    }

    #[test]
    fn every_patch_source_stays_inside_the_low_band() {
        // patch 表存在的前提：源必须是已经解出来的低带。上端由
        // `start_sb + num_sb = sba − odd` 保证，下端由内层 while 保证。
        for_every_configuration(|tables, _, _, patches| {
            for index in 0..usize::from(patches.count()) {
                let start = u16::from(patches.start_sb(index).expect("段内应有源起点"));
                let num = u16::from(patches.num_sb(index).expect("段内应有子带数"));
                let sba = u16::from(tables.sba());
                assert!(
                    start + num <= sba,
                    "第 {index} 段源区间 [{start}, {}) 越过 sba = {sba}",
                    start + num
                );
            }
        });
    }

    #[test]
    fn every_patch_is_nonempty_and_within_the_count_limit() {
        // 上界写字面 5，不引用实现的常量。
        const SPEC_MAX_PATCHES: u8 = 5;
        for_every_configuration(|_, _, _, patches| {
            assert!(patches.count() <= SPEC_MAX_PATCHES, "段数超过规范上界");
            for index in 0..usize::from(patches.count()) {
                assert!(
                    patches.num_sb(index).expect("段内应有子带数") > 0,
                    "第 {index} 段为空，`num_sb > 0` 才会计入 num_sbg_patches"
                );
            }
        });
    }

    #[test]
    fn the_borders_start_at_the_crossover_and_accumulate_the_segments() {
        for_every_configuration(|tables, _, _, patches| {
            assert_eq!(
                patches.border(0),
                Some(tables.sbx()),
                "sbg_patches[0] 应为 sbx"
            );
            let mut running = tables.sbx();
            for index in 0..usize::from(patches.count()) {
                running = running.saturating_add(patches.num_sb(index).expect("段内应有子带数"));
                assert_eq!(
                    patches.border(index + 1),
                    Some(running),
                    "sbg_patches[{}] 应为前缀和",
                    index + 1
                );
            }
            // 边界不越过 A-SPX 上界；末段被并掉时严格小于。
            assert!(running <= tables.sbz(), "patch 边界越过 sbz");
        });
    }

    #[test]
    fn the_borders_are_strictly_increasing() {
        for_every_configuration(|_, _, _, patches| {
            for index in 0..usize::from(patches.count()) {
                let low = patches.border(index).expect("边界应存在");
                let high = patches.border(index + 1).expect("边界应存在");
                assert!(low < high, "sbg_patches 必须严格递增：{low} → {high}");
            }
        });
    }

    #[test]
    fn accessors_reject_indices_past_the_end() {
        for_every_configuration(|_, _, _, patches| {
            let count = usize::from(patches.count());
            assert_eq!(patches.num_sb(count), None);
            assert_eq!(patches.start_sb(count), None);
            // 边界表比段数多一项。
            assert!(patches.border(count).is_some());
            assert_eq!(patches.border(count + 1), None);
        });
    }

    #[test]
    fn the_sampling_rate_only_matters_when_its_goal_falls_inside_the_range() {
        // goal_sb 是 43（48 kHz）或 46；两者都不小于 sbz 时走同一个分支，
        // patch 表必须逐字段相同。这条同时说明该参数不是随手加的。
        const SPEC_GOAL_48: u8 = 43;
        const SPEC_GOAL_441: u8 = 46;
        let mut differing = 0usize;
        let mut identical_above = 0usize;
        for_every_configuration(|tables, scale, base_48, patches| {
            if !base_48 {
                return;
            }
            let other = PatchTable::derive(tables, scale, false).expect("应能推出");
            if tables.sbz() <= SPEC_GOAL_48.min(SPEC_GOAL_441) {
                assert_eq!(patches, other, "两个 goal_sb 都在范围外时结果应相同");
                identical_above += 1;
            } else if patches != other {
                differing += 1;
            }
        });
        assert!(identical_above > 0, "应有 sbz 低于两个 goal_sb 的配置");
        assert!(
            differing > 0,
            "应有采样率确实改变结果的配置，否则该参数无从检验"
        );
    }
}
