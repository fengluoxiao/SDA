//! A-SPX Huffman 码本的选择与偏移。
//!
//! 覆盖 `TS103190-1:v1.4.1` 的 `Pseudocode 79` `get_aspx_hcb()` 与
//! `4.3.10.8.3` `huff_decode_diff()`。
//!
//! 表 A.16–A.33 的十八张码本随 `build.rs` 由规范附带的 C 表生成。**`cb_off`
//! 不转录**：它在十二张 DF／DT 表上恒等于 `(codebook_length - 1) / 2`，而
//! `codebook_length` 已由生成的 trie 给出，故直接推算。该关系与
//! `len_DF == 2 * len_F0 - 1` 一并由单元测试在十八张表上验证。

use super::tables::{AspxCodebook, EnvelopeKind, HcbType, StereoMode};
use crate::huffman::{HuffmanTable, tables};

/// 返回该标识对应的码本。
///
/// 命名按 `Pseudocode 79` 展开：信号包络为
/// `ASPX_HCB_ENV_<stereo>_<qmode>_<hcb_type>`，噪声包络为
/// `ASPX_HCB_NOISE_<stereo>_<hcb_type>`。
#[must_use]
pub fn table_for(book: AspxCodebook) -> &'static HuffmanTable {
    use EnvelopeKind::{Noise, Signal};
    use HcbType::{Df, Dt, F0};
    use StereoMode::{Balance, Level};

    match (book.kind, book.stereo, book.coarse_quant, book.hcb_type) {
        (Signal, Level, Some(false), F0) => &tables::ASPX_HCB_ENV_LEVEL_15_F0,
        (Signal, Level, Some(false), Df) => &tables::ASPX_HCB_ENV_LEVEL_15_DF,
        (Signal, Level, Some(false), Dt) => &tables::ASPX_HCB_ENV_LEVEL_15_DT,
        (Signal, Level, _, F0) => &tables::ASPX_HCB_ENV_LEVEL_30_F0,
        (Signal, Level, _, Df) => &tables::ASPX_HCB_ENV_LEVEL_30_DF,
        (Signal, Level, _, Dt) => &tables::ASPX_HCB_ENV_LEVEL_30_DT,
        (Signal, Balance, Some(false), F0) => &tables::ASPX_HCB_ENV_BALANCE_15_F0,
        (Signal, Balance, Some(false), Df) => &tables::ASPX_HCB_ENV_BALANCE_15_DF,
        (Signal, Balance, Some(false), Dt) => &tables::ASPX_HCB_ENV_BALANCE_15_DT,
        (Signal, Balance, _, F0) => &tables::ASPX_HCB_ENV_BALANCE_30_F0,
        (Signal, Balance, _, Df) => &tables::ASPX_HCB_ENV_BALANCE_30_DF,
        (Signal, Balance, _, Dt) => &tables::ASPX_HCB_ENV_BALANCE_30_DT,
        (Noise, Level, _, F0) => &tables::ASPX_HCB_NOISE_LEVEL_F0,
        (Noise, Level, _, Df) => &tables::ASPX_HCB_NOISE_LEVEL_DF,
        (Noise, Level, _, Dt) => &tables::ASPX_HCB_NOISE_LEVEL_DT,
        (Noise, Balance, _, F0) => &tables::ASPX_HCB_NOISE_BALANCE_F0,
        (Noise, Balance, _, Df) => &tables::ASPX_HCB_NOISE_BALANCE_DF,
        (Noise, Balance, _, Dt) => &tables::ASPX_HCB_NOISE_BALANCE_DT,
    }
}

/// `huff_decode_diff()` 要减去的码本偏移 `cb_off`。
///
/// `F0` 码本编绝对值，无偏移；`DF` 与 `DT` 编差值，取值范围对称于零，故
/// 偏移为符号数的一半。
#[must_use]
pub fn cb_off(book: AspxCodebook) -> i16 {
    match book.hcb_type {
        HcbType::F0 => 0,
        HcbType::Df | HcbType::Dt => {
            let len = table_for(book).len();
            i16::try_from(len.saturating_sub(1) / 2).unwrap_or(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aspx::tables::get_aspx_hcb;

    /// 枚举十八张码本的全部标识。
    fn every_codebook() -> [AspxCodebook; 18] {
        let mut out =
            [get_aspx_hcb(EnvelopeKind::Signal, StereoMode::Level, false, HcbType::F0); 18];
        let mut index = 0usize;
        for kind in [EnvelopeKind::Signal, EnvelopeKind::Noise] {
            for stereo in [StereoMode::Level, StereoMode::Balance] {
                for quant in [false, true] {
                    if kind == EnvelopeKind::Noise && quant {
                        continue;
                    }
                    for hcb in [HcbType::F0, HcbType::Df, HcbType::Dt] {
                        if let Some(slot) = out.get_mut(index) {
                            *slot = get_aspx_hcb(kind, stereo, quant, hcb);
                        }
                        index = index.saturating_add(1);
                    }
                }
            }
        }
        assert_eq!(index, 18);
        out
    }

    /// 由生成代码取出一张码本的名字。
    fn codebook_name(table: &HuffmanTable) -> &'static str {
        for &(name, candidate, _, _) in crate::huffman::tables::ALL_CODEBOOKS {
            if core::ptr::eq(candidate, table) {
                return name;
            }
        }
        panic!("码本不在 ALL_CODEBOOKS 内");
    }

    /// 每个标识必须落到 `Pseudocode 79` 命名规则指出的那张码本上。
    ///
    /// 「互不相同」挡不住整组错配，落点判据也无感——构造侧与解析侧共用
    /// 同一张映射表，映射写错时两边一起错。`DF` 与 `DT` 更是长度相同，
    /// 连长度关系都区分不开，只有码本名能钉死。
    #[test]
    fn identifiers_map_to_the_codebooks_named_in_the_specification() {
        use EnvelopeKind::{Noise, Signal};
        use HcbType::{Df, Dt, F0};
        use StereoMode::{Balance, Level};

        let expected: [(EnvelopeKind, StereoMode, bool, HcbType, &str); 18] = [
            (Signal, Level, false, F0, "ASPX_HCB_ENV_LEVEL_15_F0"),
            (Signal, Level, false, Df, "ASPX_HCB_ENV_LEVEL_15_DF"),
            (Signal, Level, false, Dt, "ASPX_HCB_ENV_LEVEL_15_DT"),
            (Signal, Level, true, F0, "ASPX_HCB_ENV_LEVEL_30_F0"),
            (Signal, Level, true, Df, "ASPX_HCB_ENV_LEVEL_30_DF"),
            (Signal, Level, true, Dt, "ASPX_HCB_ENV_LEVEL_30_DT"),
            (Signal, Balance, false, F0, "ASPX_HCB_ENV_BALANCE_15_F0"),
            (Signal, Balance, false, Df, "ASPX_HCB_ENV_BALANCE_15_DF"),
            (Signal, Balance, false, Dt, "ASPX_HCB_ENV_BALANCE_15_DT"),
            (Signal, Balance, true, F0, "ASPX_HCB_ENV_BALANCE_30_F0"),
            (Signal, Balance, true, Df, "ASPX_HCB_ENV_BALANCE_30_DF"),
            (Signal, Balance, true, Dt, "ASPX_HCB_ENV_BALANCE_30_DT"),
            (Noise, Level, false, F0, "ASPX_HCB_NOISE_LEVEL_F0"),
            (Noise, Level, false, Df, "ASPX_HCB_NOISE_LEVEL_DF"),
            (Noise, Level, false, Dt, "ASPX_HCB_NOISE_LEVEL_DT"),
            (Noise, Balance, false, F0, "ASPX_HCB_NOISE_BALANCE_F0"),
            (Noise, Balance, false, Df, "ASPX_HCB_NOISE_BALANCE_DF"),
            (Noise, Balance, false, Dt, "ASPX_HCB_NOISE_BALANCE_DT"),
        ];
        for (kind, stereo, quant, hcb, name) in expected {
            let book = get_aspx_hcb(kind, stereo, quant, hcb);
            assert_eq!(
                codebook_name(table_for(book)),
                name,
                "{book:?} 应映射到 {name}"
            );
        }
    }

    /// 十八个标识必须映射到十八张互不相同的码本。
    ///
    /// `match` 的通配分支若写错顺序，两个标识会落到同一张表上；比较裸指针
    /// 即可发现。
    #[test]
    fn every_identifier_maps_to_a_distinct_table() {
        let books = every_codebook();
        for (i, &left) in books.iter().enumerate() {
            for &right in books.iter().skip(i.saturating_add(1)) {
                assert!(
                    !core::ptr::eq(table_for(left), table_for(right)),
                    "{left:?} 与 {right:?} 落到同一张码本"
                );
            }
        }
    }

    /// 差分码本的长度恒为对应 F0 码本的两倍减一。
    ///
    /// F0 编绝对值 `[0, N)`，差值范围为 `[-(N-1), N-1]` 共 `2N-1` 个取值。
    /// 该关系在六组（信号四组、噪声两组）上全部成立。
    #[test]
    fn difference_codebooks_are_twice_the_absolute_one_minus_one() {
        for kind in [EnvelopeKind::Signal, EnvelopeKind::Noise] {
            for stereo in [StereoMode::Level, StereoMode::Balance] {
                for quant in [false, true] {
                    if kind == EnvelopeKind::Noise && quant {
                        continue;
                    }
                    let base = table_for(get_aspx_hcb(kind, stereo, quant, HcbType::F0)).len();
                    for hcb in [HcbType::Df, HcbType::Dt] {
                        let diff = table_for(get_aspx_hcb(kind, stereo, quant, hcb)).len();
                        assert_eq!(
                            diff,
                            base.saturating_mul(2).saturating_sub(1),
                            "{kind:?}/{stereo:?}/{quant}/{hcb:?} 的长度不符"
                        );
                    }
                }
            }
        }
    }

    /// `cb_off` 必须与表 A.17–A.33 标注的值一致。
    ///
    /// 这里的期望值是从 PDF 抄来的**第二来源**：实现按
    /// `(codebook_length - 1) / 2` 推算，本用例确认推算与规范标注相同。F0
    /// 码本在规范中不标 `cb_off`，故为 0。
    #[test]
    fn derived_offsets_match_the_specification() {
        let expected: [(EnvelopeKind, StereoMode, bool, i16); 6] = [
            (EnvelopeKind::Signal, StereoMode::Level, false, 70),
            (EnvelopeKind::Signal, StereoMode::Balance, false, 24),
            (EnvelopeKind::Signal, StereoMode::Level, true, 35),
            (EnvelopeKind::Signal, StereoMode::Balance, true, 12),
            (EnvelopeKind::Noise, StereoMode::Level, false, 29),
            (EnvelopeKind::Noise, StereoMode::Balance, false, 12),
        ];
        for (kind, stereo, quant, offset) in expected {
            for hcb in [HcbType::Df, HcbType::Dt] {
                assert_eq!(
                    cb_off(get_aspx_hcb(kind, stereo, quant, hcb)),
                    offset,
                    "{kind:?}/{stereo:?}/{quant}/{hcb:?} 的 cb_off"
                );
            }
            assert_eq!(
                cb_off(get_aspx_hcb(kind, stereo, quant, HcbType::F0)),
                0,
                "F0 码本不带偏移"
            );
        }
    }

    /// 差值范围必须对称于零，这是 `cb_off` 取一半的前提。
    #[test]
    fn difference_range_is_symmetric_around_zero() {
        for kind in [EnvelopeKind::Signal, EnvelopeKind::Noise] {
            for stereo in [StereoMode::Level, StereoMode::Balance] {
                for hcb in [HcbType::Df, HcbType::Dt] {
                    let book = get_aspx_hcb(kind, stereo, false, hcb);
                    let len = i16::try_from(table_for(book).len()).unwrap_or(i16::MAX);
                    let offset = cb_off(book);
                    let low = 0i16.saturating_sub(offset);
                    let high = len.saturating_sub(1).saturating_sub(offset);
                    assert_eq!(low, 0i16.saturating_sub(high), "{book:?} 的差值范围不对称");
                }
            }
        }
    }
}
