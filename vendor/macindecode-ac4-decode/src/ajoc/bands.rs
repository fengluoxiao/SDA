//! A-JOC 参数频带到 QMF 子带的映射。
//!
//! `TS103190-2:v1.3.1` 的 `5.7.3.1` 与表 28。`ajoc_data()` 的参数按
//! `ajoc_num_bands` 个参数频带传输，重建时要摊回 64 个 QMF 子带；
//! `Pseudocode 18` 里的 `sb_to_pb()` 就是这张表。
//!
//! # 表 197 是同一张表的子集
//!
//! 表 28 的 **15 / 12 / 9 / 7 四列与 P1 表 197 的 A-CPL 映射逐格相同**（两份
//! PDF 独立排印，由 `scripts/check_ajoc_tables.py` 逐格核对）。表 28 只是多出
//! 23 / 5 / 3 / 1 四列，并把子带区间拆得更细以容纳 23 列。
//!
//! 这条相等**不是巧合，是复用的前提**：`5.7.3.5` 的 transient ducker 借用
//! A-CPL 的 `acpl_max_num_param_bands = 15` 频带划分（P1 `5.7.7.4.3`），本模块
//! 因而不需要另存一张表 197。相等一旦被推翻，ducker 必须改取独立的表。
//!
//! 表值由 `scripts/generate_spec_tables.py` 从用户本地的官方 PDF 生成；运行期
//! 使用生成器展开的 64 项直查列。

use crate::spec_tables::ajoc::{BAND_COUNTS, SB_TO_PB as GENERATED_SB_TO_PB};

/// QMF 子带数。
pub const NUM_QMF_SUBBANDS: usize = 64;

/// 表 28 的列数，等于表 78 的 `ajoc_num_bands` 取值个数。
const NUM_COLUMNS: usize = BAND_COUNTS.len();

/// 表 78 的 `ajoc_num_bands`，按表 28 的列序。
///
/// **单调递减**：码值越大频带越少。与语法层 `ajoc_num_bands()` 的表
/// 同源，此处独立保存一份是为了让列序与表 28 的排版一一对应。
pub const AJOC_BAND_COUNTS: [u8; NUM_COLUMNS] = BAND_COUNTS;

/// 表 28 展开后的逐子带映射，`[列][子带]`。
static SB_TO_PB: [[u8; NUM_QMF_SUBBANDS]; NUM_COLUMNS] = GENERATED_SB_TO_PB;

/// 一个 `ajoc_num_bands` 取值对应的表 28 列。
///
/// 由 [`Self::for_num_bands`] 构造，因此持有的列必然合法；重建的热路径可以
/// 直接借 [`Self::column`] 遍历，不必逐子带反查列号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AjocBandMap {
    column: &'static [u8; NUM_QMF_SUBBANDS],
    num_bands: u8,
}

impl AjocBandMap {
    /// 取 `ajoc_num_bands` 对应的列。
    ///
    /// `num_bands` 不在表 78 的八个取值内时返回 `None`——那不是「频带数超界」
    /// 而是「码流给了表外的值」，调用方应按解析失败处理。
    #[must_use]
    pub fn for_num_bands(num_bands: u8) -> Option<Self> {
        let index = AJOC_BAND_COUNTS
            .iter()
            .position(|&count| count == num_bands)?;
        Some(Self {
            column: SB_TO_PB.get(index)?,
            num_bands,
        })
    }

    /// `Pseudocode 18` 的 `sb_to_pb()`：QMF 子带落在哪个参数频带。
    #[must_use]
    pub fn parameter_band(&self, subband: usize) -> Option<u8> {
        self.column.get(subband).copied()
    }

    /// 整列，供逐子带遍历的热路径直接使用。
    #[must_use]
    pub const fn column(&self) -> &'static [u8; NUM_QMF_SUBBANDS] {
        self.column
    }

    /// 本列的参数频带数。
    #[must_use]
    pub const fn num_bands(&self) -> u8 {
        self.num_bands
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "测试内的下标越界即用例失败，无需再包一层错误处理"
)]
mod tests {
    use super::*;

    /// 表 78 的八个取值都能取到列，表外取值一律拒绝。
    #[test]
    fn only_table_78_band_counts_resolve() {
        for count in AJOC_BAND_COUNTS {
            let map = AjocBandMap::for_num_bands(count).expect("表 78 的取值必须有列");
            assert_eq!(map.num_bands(), count);
        }
        for count in 0..=u8::MAX {
            assert!(
                AJOC_BAND_COUNTS.contains(&count) || AjocBandMap::for_num_bands(count).is_none(),
                "表 78 之外的 {count} 不得解析为列",
            );
        }
    }

    /// 每列都必须单调非降、自 0 起、取遍 `0..num_bands` 且末值为 `num_bands - 1`。
    ///
    /// 「取遍」是这里最强的一条：漏掉一个频带号意味着某个参数频带没有任何 QMF
    /// 子带消费它，重建时那份参数会被整段丢弃。
    #[test]
    fn every_column_is_a_surjective_monotone_map() {
        for count in AJOC_BAND_COUNTS {
            let map = AjocBandMap::for_num_bands(count).expect("列必须存在");
            let column = map.column();
            assert_eq!(column[0], 0, "{count} 列的首个子带必须落在频带 0");
            assert_eq!(
                column[NUM_QMF_SUBBANDS - 1],
                count - 1,
                "{count} 列的末个子带必须落在最后一个频带"
            );
            for sb in 1..NUM_QMF_SUBBANDS {
                let (prev, here) = (column[sb - 1], column[sb]);
                assert!(
                    here >= prev,
                    "{count} 列在子带 {sb} 处回退：{prev} -> {here}"
                );
                assert!(
                    here <= prev + 1,
                    "{count} 列在子带 {sb} 处跳过频带：{prev} -> {here}"
                );
            }
            // 23 是表 78 的最大频带数，足以容纳任何一列的频带号。
            let mut seen = [false; super::super::MAX_AJOC_BANDS];
            for &band in column.iter() {
                seen[usize::from(band)] = true;
            }
            for (band, &hit) in seen.iter().take(usize::from(count)).enumerate() {
                assert!(hit, "{count} 列没有任何子带落在频带 {band}");
            }
        }
    }

    /// 表外子带返回 `None`，不回绕也不饱和。
    #[test]
    fn subbands_outside_the_qmf_range_are_rejected() {
        let map = AjocBandMap::for_num_bands(23).expect("列必须存在");
        assert_eq!(
            map.parameter_band(NUM_QMF_SUBBANDS - 1),
            Some(map.num_bands() - 1)
        );
        assert!(map.parameter_band(NUM_QMF_SUBBANDS).is_none());
        assert!(map.parameter_band(usize::MAX).is_none());
    }
}
