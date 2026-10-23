//! ASF 的规范表格。
//!
//! 来源 `TS103190-1:v1.4.1` 附录 B（尺度因子频带）、附录 A 表 A.2–A.15
//! （码本元数据）及 `4.3.6.1`–`4.3.6.2` 的表 99–110。表值由
//! `scripts/generate_spec_tables.py` 从用户本地的官方 PDF 确定性生成，不进入
//! 版本控制或 crate 包。

use crate::spec_tables::asf::{
    FRAME_LEN_BASES_48, N_GRP_BITS_LONG_BASE, N_GRP_BITS_SHORT_BASE, N_MSFB_BITS_48,
    N_MSFBL_BITS_48, N_SIDE_BITS_48, PARTIAL_TRANSFORM_48, SFB_OFFSETS_48, SHORT_BASE_TRANSFORM_48,
    SPECTRUM_CODEBOOK_ROWS, SpectrumCodebookRow,
};

/// 44,1 kHz 或 48 kHz 下受支持的变换长度，由长到短。
///
/// 与表 B.1 的行序一致，`NUM_SFB_48` 按同一顺序给出频带数。
pub const TRANSFORM_LENGTHS_48: [u16; 15] = crate::spec_tables::asf::TRANSFORM_LENGTHS_48;

/// 表 B.1：44,1 kHz 或 48 kHz 各变换长度的尺度因子频带数。
pub const NUM_SFB_48: [u8; 15] = crate::spec_tables::asf::NUM_SFB_48;

/// 变换长度在表内的行号。
fn row_of(transform_length: u16) -> Option<usize> {
    let mut index = 0;
    while index < TRANSFORM_LENGTHS_48.len() {
        match TRANSFORM_LENGTHS_48.get(index) {
            Some(&length) if length == transform_length => return Some(index),
            Some(_) => index = index.saturating_add(1),
            None => return None,
        }
    }
    None
}

/// 表 B.1 的 `num_sfb`：该变换长度下的尺度因子频带数。
///
/// 44,1 kHz 与 48 kHz 共用一张表。变换长度不在表内时返回 `None`。
#[must_use]
pub fn num_sfb_48(transform_length: u16) -> Option<u8> {
    NUM_SFB_48.get(row_of(transform_length)?).copied()
}

/// 附录 B 的 `sfb_offset[]`：长度为 `num_sfb + 1`，末项即变换长度。
#[must_use]
pub fn sfb_offsets_48(transform_length: u16) -> Option<&'static [u16]> {
    SFB_OFFSETS_48.get(row_of(transform_length)?).copied()
}

/// ASF 谱线码本的元数据，见附录 A 表 A.2–A.12、A.14、A.15。
///
/// `sect_cb` 取 0 表示该区段无谱线数据；1 至 11 对应下表；12 至 15 由
/// `4.3.6.3.1` 规定不得使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpectrumCodebook {
    /// 一个码字承载的谱线数，表 A.14 的 `CB_DIM`。
    pub dimension: u8,
    /// 表 A.15 的 `UNSIGNED_CB`；为真时码字后跟随符号位。
    pub unsigned: bool,
    /// `cb_mod`，即每条谱线的取值个数。
    pub modulus: u16,
    /// `cb_off`，由码本下标还原谱线时减去的偏置。
    pub offset: i16,
    /// `codebook_length`，恒等于 `modulus.pow(dimension)`。
    pub length: u16,
}

impl SpectrumCodebook {
    /// 基数分解得到的单条谱线基础值区间，闭区间。
    ///
    /// 这是应用无符号码本的符号位、以及码本 11 的转义扩展**之前**的范围。
    /// 因此无符号码本返回非负幅度，不能把它当作最终 `quant_spec` 的范围。
    #[must_use]
    pub const fn base_value_range(&self) -> (i16, i16) {
        let modulus = self.modulus as i16;
        let high = modulus.saturating_sub(1).saturating_sub(self.offset);
        (0i16.saturating_sub(self.offset), high)
    }
}

/// 下标 1 至 11 对应 `ASF_HCB_1` 至 `ASF_HCB_11`；下标 0 为占位，不使用。
const SPECTRUM_CODEBOOKS: [SpectrumCodebook; 12] = build_spectrum_codebooks();

const fn build_spectrum_codebooks() -> [SpectrumCodebook; 12] {
    let [
        row_0,
        row_1,
        row_2,
        row_3,
        row_4,
        row_5,
        row_6,
        row_7,
        row_8,
        row_9,
        row_10,
        row_11,
    ] = SPECTRUM_CODEBOOK_ROWS;
    [
        spectrum_codebook_from_row(row_0),
        spectrum_codebook_from_row(row_1),
        spectrum_codebook_from_row(row_2),
        spectrum_codebook_from_row(row_3),
        spectrum_codebook_from_row(row_4),
        spectrum_codebook_from_row(row_5),
        spectrum_codebook_from_row(row_6),
        spectrum_codebook_from_row(row_7),
        spectrum_codebook_from_row(row_8),
        spectrum_codebook_from_row(row_9),
        spectrum_codebook_from_row(row_10),
        spectrum_codebook_from_row(row_11),
    ]
}

const fn spectrum_codebook_from_row(row: SpectrumCodebookRow) -> SpectrumCodebook {
    let (dimension, unsigned, modulus, offset, length) = row;
    SpectrumCodebook {
        dimension,
        unsigned,
        modulus,
        offset,
        length,
    }
}

/// 取 `sect_cb` 对应的谱线码本元数据。
///
/// `sect_cb == 0` 表示该区段不含谱线数据，返回 `None`；12 至 15 为规范禁止
/// 使用的取值，同样返回 `None`。
#[must_use]
pub fn spectrum_codebook(sect_cb: u8) -> Option<&'static SpectrumCodebook> {
    if sect_cb == 0 {
        return None;
    }
    SPECTRUM_CODEBOOKS.get(usize::from(sect_cb))
}

/// `max_sfb[i]` 的比特数，见 `4.3.6.2.1` 表 106。
#[must_use]
pub fn n_msfb_bits_48(transform_length: u16) -> Option<u8> {
    N_MSFB_BITS_48.get(row_of(transform_length)?).copied()
}

/// `max_sfb_side[i]` 或 `max_sfb_master` 的比特数，见表 106。
#[must_use]
pub fn n_side_bits_48(transform_length: u16) -> Option<u8> {
    N_SIDE_BITS_48.get(row_of(transform_length)?).copied()
}

/// `sf_info_lfe()` 中 `max_sfb[0]` 的比特数，见表 106。
#[must_use]
pub fn n_msfbl_bits_48(transform_length: u16) -> Option<u8> {
    N_MSFBL_BITS_48
        .get(row_of(transform_length)?)
        .copied()
        .flatten()
}

/// 由 `transf_length` 索引取实际变换长度，44,1 kHz 或 48 kHz。
///
/// `index` 取 0 至 3 对应 `asf_transform_info()` 传输的两比特值；`index == 4`
/// 表示长帧，此时变换长度等于 `frame_len_base`（表 99）。
///
/// 组合不合法时返回 `None`：`frame_len_base` 不在表内，或该组合在表 103 中
/// 标为 `×`。
#[must_use]
pub fn transform_length_48(frame_len_base: u16, index: u8) -> Option<u16> {
    if index == 4 {
        // 表 99：44,1 kHz 与 48 kHz 下长帧的变换长度即 frame_len_base。
        return FRAME_LEN_BASES_48
            .contains(&frame_len_base)
            .then_some(frame_len_base);
    }
    let slot = usize::from(index);
    if frame_len_base >= 1536 {
        for (base, lengths) in PARTIAL_TRANSFORM_48 {
            if base == frame_len_base {
                return lengths.get(slot).copied();
            }
        }
        return None;
    }
    for (base, lengths) in SHORT_BASE_TRANSFORM_48 {
        if base == frame_len_base {
            return lengths.get(slot).copied().flatten();
        }
    }
    None
}

/// `frame_len_base >= 1536` 且非长帧时的 `n_grp_bits`，见表 109。
#[must_use]
pub fn n_grp_bits_long_base(first: u8, second: u8) -> Option<u8> {
    N_GRP_BITS_LONG_BASE
        .get(usize::from(first))?
        .get(usize::from(second))
        .copied()
}

/// `frame_len_base < 1536` 时的 `n_grp_bits`，见表 110。
#[must_use]
pub fn n_grp_bits_short_base(frame_len_base: u16, index: u8) -> Option<u8> {
    let row = match frame_len_base {
        1024 | 960 | 768 => 0,
        512 | 384 => 1,
        _ => return None,
    };
    N_GRP_BITS_SHORT_BASE
        .get(row)?
        .get(usize::from(index))
        .copied()
        .flatten()
}

/// 第一半帧内的窗口数，`Pseudocode 2` 与 `Pseudocode 3` 的 `num_windows_0`。
#[must_use]
pub const fn num_windows_first_half(transf_length_index: u8) -> Option<u8> {
    match transf_length_index {
        0 => Some(8),
        1 => Some(4),
        2 => Some(2),
        3 => Some(1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三张按变换长度索引的表必须行数一致，否则 `row_of` 会取错行。
    #[test]
    fn tables_share_row_order() {
        assert_eq!(TRANSFORM_LENGTHS_48.len(), NUM_SFB_48.len());
        assert_eq!(TRANSFORM_LENGTHS_48.len(), SFB_OFFSETS_48.len());
        assert_eq!(TRANSFORM_LENGTHS_48.len(), N_MSFB_BITS_48.len());
        assert_eq!(TRANSFORM_LENGTHS_48.len(), N_SIDE_BITS_48.len());
        assert_eq!(TRANSFORM_LENGTHS_48.len(), N_MSFBL_BITS_48.len());
    }

    /// 变换长度严格递减，`row_of` 才不会有二义。
    #[test]
    fn transform_lengths_strictly_decrease() {
        for pair in TRANSFORM_LENGTHS_48.windows(2) {
            let [long, short] = pair else { unreachable!() };
            assert!(long > short, "{long} 未大于 {short}");
        }
    }

    /// 附录 B 每一列的四条形状约束。
    ///
    /// 末项等于变换长度这一条尤其关键：它把整列的累加结果钉死，中间任何一
    /// 项被写大或写小都会同时破坏严格递增或末项等式。
    #[test]
    fn sfb_offsets_have_declared_shape() {
        for &length in &TRANSFORM_LENGTHS_48 {
            let offsets = sfb_offsets_48(length).expect("表内变换长度应有偏移列");
            let num_sfb = usize::from(num_sfb_48(length).expect("表内变换长度应有 num_sfb"));
            let expected_len = num_sfb.saturating_add(1);

            assert_eq!(offsets.first(), Some(&0), "{length}：首项应为 0");
            assert_eq!(
                offsets.len(),
                expected_len,
                "{length}：应有 num_sfb + 1 = {expected_len} 项"
            );
            assert_eq!(
                offsets.last(),
                Some(&length),
                "{length}：末项应等于变换长度"
            );
            for pair in offsets.windows(2) {
                let [low, high] = pair else { unreachable!() };
                assert!(low < high, "{length}：偏移 {low} 未小于 {high}");
            }
        }
    }

    /// 频带宽度单调不减，末带除外。
    ///
    /// 末带允许变窄：变换长度不是 2 的幂时（960、480 等）最后一带被截短。
    /// 这条排除项本身也是判据——若在**非末带**处出现下降，说明表有错。
    #[test]
    fn band_widths_increase_except_final() {
        for &length in &TRANSFORM_LENGTHS_48 {
            let offsets = sfb_offsets_48(length).expect("表内变换长度应有偏移列");
            // 相邻两带由三个偏移决定，末带对应最后一个三元窗口，跳过它。
            let triples = offsets.windows(3).count();
            for (index, triple) in offsets.windows(3).enumerate() {
                if index.saturating_add(1) == triples {
                    continue;
                }
                let [low, mid, high] = triple else {
                    unreachable!()
                };
                let current = mid.saturating_sub(*low);
                let next = high.saturating_sub(*mid);
                assert!(
                    current <= next,
                    "{length}：第 {index} 带宽 {current} 大于第 {} 带宽 {next}",
                    index.saturating_add(1)
                );
            }
        }
    }

    /// 同一张附录 B 表内，各列在公共前缀上完全一致，只有末项不同。
    ///
    /// 这是最强的一条：任一处笔误都会被同组邻列揭穿，无需外部数据。
    #[test]
    fn columns_within_a_table_share_their_prefix() {
        // 表 B.4、B.5、B.6 各一组；B.7 虽排成六列，实际是两组独立的三列。
        for group in TRANSFORM_LENGTHS_48.as_chunks::<3>().0 {
            let Some((&longest, rest)) = group.split_first() else {
                unreachable!()
            };
            let reference = sfb_offsets_48(longest).expect("组内最长列应存在");
            for &length in rest {
                let column = sfb_offsets_48(length).expect("组内列应存在");
                let shared = column.len().saturating_sub(1);
                assert_eq!(
                    column.get(..shared),
                    reference.get(..shared),
                    "{length} 与 {longest} 在前 {shared} 项上不一致"
                );
                assert_eq!(column.last(), Some(&length));
            }
        }
    }

    /// `n_msfb_bits` 必须能表示该变换长度的 `num_sfb`。
    ///
    /// `max_sfb` 的取值上界即 `num_sfb`（`4.3.6.2.2`），因此
    /// `num_sfb < 2^n_msfb_bits` 是表 106 与表 B.1 之间的硬约束。
    #[test]
    fn msfb_bits_can_represent_num_sfb() {
        for &length in &TRANSFORM_LENGTHS_48 {
            let num_sfb = u32::from(num_sfb_48(length).expect("应有 num_sfb"));
            let bits = u32::from(n_msfb_bits_48(length).expect("应有 n_msfb_bits"));
            assert!(
                num_sfb < (1u32 << bits),
                "{length}：num_sfb {num_sfb} 无法用 {bits} 比特表示"
            );
        }
    }

    /// 表 109 关于对角线对称——两个半帧的角色在窗口计数上是可交换的。
    #[test]
    fn grouping_bits_table_is_symmetric() {
        for first in 0..4u8 {
            for second in 0..4u8 {
                assert_eq!(
                    n_grp_bits_long_base(first, second),
                    n_grp_bits_long_base(second, first),
                    "表 109 在 ({first}, {second}) 处不对称"
                );
            }
        }
    }

    /// 表 109 的每一项都等于两个半帧的窗口数之和减去分组基数。
    ///
    /// `Pseudocode 3` 给出 `num_windows = n_grp_bits + 1`，两半帧变换长度不同
    /// 时再加一。把它反解出来就得到本式，与表 109 是同一事实的两种独立表述。
    #[test]
    fn grouping_bits_match_window_counts() {
        for first in 0..4u8 {
            for second in 0..4u8 {
                let windows_0 = u16::from(num_windows_first_half(first).expect("索引合法"));
                let windows_1 = u16::from(num_windows_first_half(second).expect("索引合法"));
                let base = if first == second { 1 } else { 2 };
                let derived = windows_0.saturating_add(windows_1).saturating_sub(base);
                assert_eq!(
                    n_grp_bits_long_base(first, second).map(u16::from),
                    Some(derived),
                    "表 109 在 ({first}, {second}) 处与窗口数推导不符"
                );
            }
        }
    }

    /// 部分块变换长度乘以其窗口数应恰好铺满半帧。
    #[test]
    fn partial_transforms_tile_the_frame() {
        for (frame_len_base, lengths) in PARTIAL_TRANSFORM_48 {
            for (index, length) in lengths.iter().enumerate() {
                let index = u8::try_from(index).expect("索引不超过 3");
                let windows = u32::from(num_windows_first_half(index).expect("索引合法"));
                assert_eq!(
                    u32::from(*length) * windows,
                    u32::from(frame_len_base) / 2,
                    "frame_len_base {frame_len_base} 索引 {index} 未铺满半帧"
                );
                assert_eq!(transform_length_48(frame_len_base, index), Some(*length));
            }
            // 长帧的变换长度即 frame_len_base，见表 99。
            assert_eq!(transform_length_48(frame_len_base, 4), Some(frame_len_base));
        }
    }

    /// 谱线码本的三个常量互相约束：`modulus^dimension == length`。
    #[test]
    fn spectrum_codebook_constants_agree() {
        for sect_cb in 1..=11u8 {
            let cb = spectrum_codebook(sect_cb).expect("1 至 11 应有码本");
            let expected = u32::from(cb.modulus).pow(u32::from(cb.dimension));
            assert_eq!(
                expected,
                u32::from(cb.length),
                "码本 {sect_cb}：{}^{} 不等于 codebook_length {}",
                cb.modulus,
                cb.dimension,
                cb.length
            );
            assert!(matches!(cb.dimension, 2 | 4), "码本维度只能是 2 或 4");
        }
    }

    /// 有符号码本以 0 为中心，无符号码本从 0 起算。
    ///
    /// `cb_off` 把码本下标平移成谱线值，因此它同时决定了取值区间是否对称，
    /// 以及是否需要额外的符号位（表 A.15）。
    #[test]
    fn spectrum_codebook_base_ranges_match_signedness() {
        for sect_cb in 1..=11u8 {
            let cb = spectrum_codebook(sect_cb).expect("1 至 11 应有码本");
            let (low, high) = cb.base_value_range();
            if cb.unsigned {
                assert_eq!(low, 0, "码本 {sect_cb} 为无符号，下界应为 0");
            } else {
                assert_eq!(low, -high, "码本 {sect_cb} 为有符号，区间应对称");
            }
        }
    }

    /// 码本 11 的上界 16 即 `ext_decode` 的转义值，见 `5.1.2.2`。
    #[test]
    fn escape_codebook_uses_sixteen_sentinel() {
        let cb = spectrum_codebook(11).expect("码本 11 应存在");
        assert_eq!(cb.base_value_range(), (0, 16));
    }

    /// `sect_cb` 的非法取值不得返回码本。
    #[test]
    fn rejects_reserved_codebook_numbers() {
        assert!(spectrum_codebook(0).is_none(), "0 表示无谱线数据");
        for sect_cb in 12..=15u8 {
            assert!(
                spectrum_codebook(sect_cb).is_none(),
                "{sect_cb} 由 4.3.6.3.1 规定不得使用"
            );
        }
    }

    /// 表 106 中记为 N/A 的行不得返回 LFE 比特数。
    #[test]
    fn lfe_bits_absent_where_table_says_na() {
        assert_eq!(n_msfbl_bits_48(2048), Some(3));
        assert_eq!(n_msfbl_bits_48(1024), Some(2));
        assert_eq!(n_msfbl_bits_48(480), None);
        assert_eq!(n_msfbl_bits_48(256), None);
    }

    /// 表外的变换长度一律返回 `None`，不得回退到相邻行。
    #[test]
    fn unknown_transform_lengths_are_rejected() {
        for length in [0, 1, 100, 2049, 4096, u16::MAX] {
            assert_eq!(num_sfb_48(length), None, "{length} 不在表 B.1 内");
            assert_eq!(sfb_offsets_48(length), None);
            assert_eq!(n_msfb_bits_48(length), None);
        }
    }

    /// 表 103 中标为 `×` 的组合不得返回变换长度。
    #[test]
    fn short_base_rejects_absent_combinations() {
        assert_eq!(transform_length_48(512, 2), Some(512));
        assert_eq!(transform_length_48(512, 3), None, "表 103 该格为 ×");
        assert_eq!(transform_length_48(384, 3), None, "表 103 该格为 ×");
        assert_eq!(transform_length_48(1024, 3), Some(1024));
    }

    /// 附录 B 中存在的部分块长度不一定是表 99 允许的帧基准。
    #[test]
    fn long_frames_reject_partial_transform_lengths_as_frame_bases() {
        for frame_len_base in [480, 256, 240, 192, 128, 120, 96] {
            assert_eq!(
                transform_length_48(frame_len_base, 4),
                None,
                "{frame_len_base} 只是部分块长度，不是合法 frame_len_base"
            );
        }
        for frame_len_base in FRAME_LEN_BASES_48 {
            assert_eq!(transform_length_48(frame_len_base, 4), Some(frame_len_base));
        }
    }
}
