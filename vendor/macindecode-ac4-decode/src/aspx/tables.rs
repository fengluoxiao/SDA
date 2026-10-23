//! A-SPX 的静态表与查表函数。
//!
//! 覆盖 `TS103190-1:v1.4.1` 的：
//!
//! - `5.7.6.3.1.1` 的两张模板子带组表；
//! - 表 189 的 `num_qmf_timeslots` 与表 192 的 `num_ts_in_ats`；
//! - 表 126 的 `aspx_int_class` 变长码；
//! - `Pseudocode 79` `get_aspx_hcb()` 的码本选择。

#[cfg(feature = "spec-tables")]
use crate::spec_tables::aspx::{NUM_TS_IN_ATS, SBG_TEMPLATE_HIGHRES, SBG_TEMPLATE_LOWRES};

/// QMF 分析滤波器组的子带数，`5.7.3.2` 规定恒为 64。
pub const NUM_QMF_SUBBANDS: u16 = 64;

/// 噪声子带组数的上界，见 `5.7.6.3.1.3`。
pub const MAX_SBG_NOISE: u8 = 5;

/// 主子带组表的最大组数，取两张模板表中较大的一张。
pub const MAX_SBG_MASTER: usize = 22;

/// A-SPX 时隙数的上界，见表 189 与表 192。
pub const MAX_ASPX_TIMESLOTS: usize = 16;

/// 返回选定模板表的子带边界。
///
/// `master_freq_scale` 取 0 为低频分辨率，取 1 为高频分辨率，见 `aspx_config`。
#[must_use]
#[cfg(feature = "spec-tables")]
pub const fn sbg_template(master_freq_scale: bool) -> &'static [u8] {
    if master_freq_scale {
        &SBG_TEMPLATE_HIGHRES
    } else {
        &SBG_TEMPLATE_LOWRES
    }
}

/// 该模板表在 `aspx_start_freq` 与 `aspx_stop_freq` 均为 0 时的子带组数。
///
/// `Pseudocode 67` 的 `num_sbg_master` 由该值减去 `2 * start + 2 * stop` 得到。
#[must_use]
#[cfg(feature = "spec-tables")]
pub const fn template_group_count(master_freq_scale: bool) -> u8 {
    if master_freq_scale { 22 } else { 20 }
}

/// 表 192 的 `num_ts_in_ats`。
///
/// `frame_len_base` 不在表 192 内时返回 `None`。
#[must_use]
#[cfg(feature = "spec-tables")]
pub fn num_ts_in_ats(frame_len_base: u16) -> Option<u8> {
    NUM_TS_IN_ATS
        .iter()
        .find(|&&(length, _, _)| length == frame_len_base)
        .map(|&(_, factor, _)| factor)
}

/// 表 192 的 `ts_offset_hfgen`，单位为 QMF 时隙。
///
/// `frame_len_base` 不在表 192 内时返回 `None`。
#[must_use]
#[cfg(feature = "spec-tables")]
pub fn ts_offset_hfgen(frame_len_base: u16) -> Option<u8> {
    NUM_TS_IN_ATS
        .iter()
        .find(|&&(length, _, _)| length == frame_len_base)
        .map(|&(_, _, offset)| offset)
}

/// A-SPX 时隙单位下可变边界的最大偏移。
///
/// `5.7.6.3.3.1` 规定边界移位满足 `1 < ts_var_offset <= ts_offset_hfgen`，
/// 单位为 QMF 时隙；换算到 A-SPX 时隙即 `ts_offset_hfgen / num_ts_in_ats`。
/// 该商在表 192 的八行上恒为 3，恰好等于 `aspx_var_bord_left`／
/// `aspx_var_bord_right` 这两个 2 位字段的最大值。
///
/// `frame_len_base` 不在表 192 内时返回 `None`。
#[must_use]
#[cfg(feature = "spec-tables")]
pub fn max_var_border_offset(frame_len_base: u16) -> Option<u8> {
    let offset = ts_offset_hfgen(frame_len_base)?;
    let factor = num_ts_in_ats(frame_len_base)?;
    offset.checked_div(factor)
}

/// `num_qmf_timeslots`，见 `5.7.3.2`：`frame_length / num_qmf_subbands`。
///
/// 只对表 192 收录的 `frame_len_base` 返回值；表 189 列出了全部八个结果。
#[must_use]
#[cfg(feature = "spec-tables")]
pub fn num_qmf_timeslots(frame_len_base: u16) -> Option<u8> {
    num_ts_in_ats(frame_len_base)?;
    let slots = frame_len_base.checked_div(NUM_QMF_SUBBANDS)?;
    u8::try_from(slots).ok()
}

/// `num_aspx_timeslots`，见 `Pseudocode 75a`。
///
/// 结果只有 16、15、12、8、6 五个取值；其中 8 与 6 分别对应 `frame_len_base`
/// 512 与 384，也就是 `aspx_framing` 里若干字段退化为 1 位的两种情形。
#[must_use]
#[cfg(feature = "spec-tables")]
pub fn num_aspx_timeslots(frame_len_base: u16) -> Option<u8> {
    let qmf = num_qmf_timeslots(frame_len_base)?;
    let factor = num_ts_in_ats(frame_len_base)?;
    qmf.checked_div(factor)
}

/// 由表 189/192 的 `(num_aspx_timeslots, num_ts_in_ats)` 配对还原 QMF 时隙数。
///
/// 单独看两个值都不足以证明帧布局合法：例如 A-SPX 时隙数 8 只与倍率 1 配对，
/// 而 12、15、16 各自可与 1 或 2 配对。组装阶段用本函数把成帧来源与调用方的
/// 倍率绑回同一行表，再校验显式传入的 `num_qmf_timeslots`。
#[cfg(any(feature = "audio-decode", all(test, feature = "spec-tables")))]
#[must_use]
pub(super) fn qmf_timeslots_for_aspx_layout(
    num_aspx_timeslots: u8,
    num_ts_in_ats: u8,
) -> Option<u8> {
    NUM_TS_IN_ATS
        .iter()
        .find_map(|&(frame_len_base, table_factor, _)| {
            if table_factor != num_ts_in_ats {
                return None;
            }
            let qmf = frame_len_base.checked_div(NUM_QMF_SUBBANDS)?;
            let qmf = u8::try_from(qmf).ok()?;
            let aspx = qmf.checked_div(table_factor)?;
            (aspx == num_aspx_timeslots).then_some(qmf)
        })
}

/// A-SPX 间隔类别，见表 126。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalClass {
    /// `0b0`：两端固定。
    FixFix,
    /// `0b10`：左端固定，右端可变。
    FixVar,
    /// `0b110`：左端可变，右端固定。
    VarFix,
    /// `0b111`：两端可变。
    VarVar,
}

/// A-SPX 码本的差分方向，见 `Pseudocode 79` 的 `hcb_type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HcbType {
    /// 频率方向的首个值，绝对量化。
    F0,
    /// 频率方向的差值。
    Df,
    /// 时间方向的差值。
    Dt,
}

/// A-SPX 包络数据的类型，见 `aspx_ec_data()` 的 `data_type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeKind {
    /// 信号包络。
    Signal,
    /// 噪声包络。
    Noise,
}

/// 双声道标度因子的配对方式，见表 125。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StereoMode {
    /// 逐声道独立编码。
    Level,
    /// 配对编码。
    Balance,
}

/// A-SPX 码本的标识，对应表 A.16–A.33 的十八张表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspxCodebook {
    /// 包络类型。
    pub kind: EnvelopeKind,
    /// 立体声模式。
    pub stereo: StereoMode,
    /// 量化步长，`Some(false)` 为 1,5 dB，`Some(true)` 为 3 dB；噪声包络无此维度。
    pub coarse_quant: Option<bool>,
    /// 差分方向。
    pub hcb_type: HcbType,
}

/// `get_aspx_hcb()`，见 `Pseudocode 79`。
///
/// 信号包络按 `stereo_mode`、`quant_mode`、`hcb_type` 三维展开成 12 张表，
/// 噪声包络没有量化步长维度，展开成 6 张表。
#[must_use]
pub const fn get_aspx_hcb(
    kind: EnvelopeKind,
    stereo: StereoMode,
    quant_mode: bool,
    hcb_type: HcbType,
) -> AspxCodebook {
    let coarse_quant = match kind {
        EnvelopeKind::Signal => Some(quant_mode),
        EnvelopeKind::Noise => None,
    };
    AspxCodebook {
        kind,
        stereo,
        coarse_quant,
        hcb_type,
    }
}

#[cfg(all(test, feature = "spec-tables"))]
mod tests {
    use super::*;

    /// 可变边界的最大偏移在表 192 的八行上恒为 3。
    ///
    /// `5.7.6.3.3.1` 以 QMF 时隙给出偏移上界 `ts_offset_hfgen`，而
    /// `aspx_var_bord_left`／`aspx_var_bord_right` 以 A-SPX 时隙计量且只有
    /// 2 位。两者本是无关的表述——表 192 第三列与表 53 的字段宽度——换算后
    /// 却处处相等，因此它同时校验了 `num_ts_in_ats` 与 `ts_offset_hfgen`
    /// 两列的转录。
    #[test]
    fn variable_border_offset_matches_the_two_bit_field_width() {
        for &(frame_len_base, _, _) in &NUM_TS_IN_ATS {
            assert_eq!(
                max_var_border_offset(frame_len_base),
                Some(3),
                "frame_len_base {frame_len_base} 的可变边界偏移上界"
            );
        }
        assert_eq!(max_var_border_offset(1000), None);
    }

    /// 模板表严格递增。
    #[test]
    fn templates_are_strictly_increasing() {
        for scale in [false, true] {
            let template = sbg_template(scale);
            for pair in template.windows(2) {
                let (Some(low), Some(high)) = (pair.first(), pair.get(1)) else {
                    unreachable!("windows(2) 必有两项");
                };
                assert!(low < high, "模板表必须严格递增：{low} 之后是 {high}");
            }
        }
    }

    /// 模板表的长度必须恰好容纳 `Pseudocode 67` 在 `stop == 0` 时的最大索引。
    ///
    /// 该式为 `2 * start + num_sbg_master`，代入 `num_sbg_master` 后化简为
    /// `template_group_count`，因此边界数必须是组数加一。
    #[test]
    fn template_length_matches_group_count() {
        for scale in [false, true] {
            let count = usize::from(template_group_count(scale));
            assert_eq!(
                sbg_template(scale).len(),
                count.saturating_add(1),
                "边界数应为组数加一"
            );
        }
    }

    /// 表 189 的转录值必须与 `frame_length / 64` 一致。
    #[test]
    fn qmf_timeslots_agree_with_the_formula() {
        for &(frame_len_base, _, _) in &NUM_TS_IN_ATS {
            let expected = u8::try_from(frame_len_base / NUM_QMF_SUBBANDS).expect("时隙数为 u8");
            assert_eq!(
                num_qmf_timeslots(frame_len_base),
                Some(expected),
                "frame_len_base {frame_len_base} 的 num_qmf_timeslots"
            );
            assert_eq!(
                u16::from(expected).saturating_mul(NUM_QMF_SUBBANDS),
                frame_len_base,
                "表 189 与 frame_length / 64 不符"
            );
        }
    }

    /// 表 192 只收录表 99 的八个合法帧基准，其余一律无值。
    #[test]
    fn only_the_eight_legal_frame_bases_have_timeslots() {
        for frame_len_base in 0u16..=4096 {
            let listed = NUM_TS_IN_ATS
                .iter()
                .any(|&(len, _, _)| len == frame_len_base);
            assert_eq!(
                num_aspx_timeslots(frame_len_base).is_some(),
                listed,
                "frame_len_base {frame_len_base} 的收录情况不符"
            );
        }
    }

    /// 所有生成表行都能换算出不超过公共上界的 A-SPX 时隙数。
    #[test]
    fn aspx_timeslots_fit_the_public_bound() {
        for &(frame_len_base, _, _) in &NUM_TS_IN_ATS {
            let slots = num_aspx_timeslots(frame_len_base).expect("生成表行应可换算");
            assert!(usize::from(slots) <= MAX_ASPX_TIMESLOTS);
        }
    }

    /// A-SPX 时隙数与倍率必须来自表 189/192 的同一行，且能唯一还原 QMF 时隙数。
    #[test]
    fn aspx_layout_uniquely_recovers_qmf_timeslots() {
        for &(frame_len_base, _, _) in &NUM_TS_IN_ATS {
            let expected_qmf =
                u8::try_from(frame_len_base / NUM_QMF_SUBBANDS).expect("时隙数为 u8");
            let aspx = num_aspx_timeslots(frame_len_base).expect("合法帧长应有 A-SPX 时隙数");
            let factor = num_ts_in_ats(frame_len_base).expect("合法帧长应有时隙倍率");
            assert_eq!(
                qmf_timeslots_for_aspx_layout(aspx, factor),
                Some(expected_qmf),
                "frame_len_base {frame_len_base} 的表行应能还原"
            );
        }

        let max_aspx_timeslots = u8::try_from(MAX_ASPX_TIMESLOTS).expect("公共上界为 u8");
        for aspx in 0..=max_aspx_timeslots {
            for factor in 0..=u8::MAX {
                let expected = NUM_TS_IN_ATS.iter().find_map(|&(frame, row_factor, _)| {
                    let qmf = u8::try_from(frame / NUM_QMF_SUBBANDS).ok()?;
                    if row_factor != factor {
                        return None;
                    }
                    (qmf.checked_div(row_factor) == Some(aspx)).then_some(qmf)
                });
                assert_eq!(qmf_timeslots_for_aspx_layout(aspx, factor), expected);
            }
        }
    }

    /// 十八张码本的展开必须两两不同，且噪声包络不带量化步长维度。
    #[test]
    fn codebook_selection_is_injective() {
        let mut seen: [Option<AspxCodebook>; 18] = [None; 18];
        let mut count = 0usize;
        for kind in [EnvelopeKind::Signal, EnvelopeKind::Noise] {
            for stereo in [StereoMode::Level, StereoMode::Balance] {
                for quant in [false, true] {
                    for hcb in [HcbType::F0, HcbType::Df, HcbType::Dt] {
                        let book = get_aspx_hcb(kind, stereo, quant, hcb);
                        if kind == EnvelopeKind::Noise {
                            assert_eq!(book.coarse_quant, None, "噪声包络无量化步长维度");
                            if quant {
                                continue;
                            }
                        } else {
                            assert_eq!(book.coarse_quant, Some(quant));
                        }
                        assert!(
                            !seen.iter().flatten().any(|&other| other == book),
                            "码本展开重复：{book:?}"
                        );
                        if let Some(slot) = seen.get_mut(count) {
                            *slot = Some(book);
                        }
                        count = count.saturating_add(1);
                    }
                }
            }
        }
        assert_eq!(count, 18, "表 A.16–A.33 共十八张码本");
    }
}
