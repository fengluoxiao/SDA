//! 音调生成器（`TS103190-1:v1.4.1:5.7.6.4.4`）。
//!
//! `Pseudocode 104`–`105` 与表 196。把 `5.7.6.4.2.2` 的 `sine_lev_sb_adj` 逐时隙
//! 铺开，乘上表 196 的四个复数，产出与 `Q_high` 同形的 `qmf_sine`，供
//! `5.7.6.4.5` 叠加。
//!
//! # 与噪声发生器形似而不同的四处
//!
//! 两节的外层循环逐字相同，容易顺手照抄，但内部有四处实质差别：
//!
//! - **下标与子带无关。** `Pseudocode 103` 的偏移是
//!   `num_sb_aspx·Δts + sb + 1`，`Pseudocode 105` 只有 `Δts`——同一时隙的全部
//!   子带取同一张表项。照噪声那样补上 `sb` 会让每个子带各转各的相位。
//! - **虚部另带一个逐子带的符号。** `pow(-1, sb+sbx)` 只乘在虚部上，且用的是
//!   **绝对**子带号 `sb + sbx`，不是 A-SPX 内的相对号。实部没有这一项。
//! - **重置条件不同。** 噪声用 `master_reset`（`5.7.6.3.1.1`：三个配置字段相对
//!   上一个 I 帧变化），音调用 `first_frame`（正文：**只**在编解码初始化时为 1）。
//!   两者不可互换：配置变化不重置音调相位。
//! - **表长与推进量不同。** 噪声表 512 项、每个 `(子带, 时隙)` 走一格；音调表
//!   4 项、每个时隙走一格，非首帧基址取上一区间末项加一。
//!
//! # `sine_idx_prev` 是标量，不是矩阵
//!
//! 与 `Pseudocode 103` 同样的排印问题：写作 `sine_idx_prev[sb][ts]`，正文却说
//! 它是「上一个 A-SPX 区间的**最后一个** `sine_idx`」——单数。这里多一条噪声那
//! 边没有的旁证：`sine_idx(sb, ts)` 声明了 `sb` 形参，而函数体除这个下标外**再
//! 没用过它**；按标量读，`sb` 恰好成为一个未使用的形参，与「同一时隙全部子带
//! 同相位」自洽。按矩阵读则每个子带各带一个基址，`pow(-1, sb+sbx)` 那套逐子带
//! 符号就失去意义——相位已经各不相同，不需要再用符号区分。
//!
//! # `ts − atsg_sig[0]` 的单位不一致，按字面保留
//!
//! 与 `Pseudocode 103` 完全相同的问题：`ts` 以 QMF 时隙计，减去的却是未乘
//! `num_ts_in_ats` 的 `atsg_sig[0]`。`% 4` 使字面读法不越界，无旁证说它是笔误，
//! 故按字面执行。已登记在规范可追踪性第 7 节。
//!
//! # 表 196 就是 `i^k`
//!
//! 四项 `(1,0)`、`(0,1)`、`(−1,0)`、`(0,−1)` 恰是 `i` 的前四个幂。这里仍按表的
//! 四行写出而不写成幂运算：表是规范的呈现形式，逐行对照比论证「这确实是 i^k」
//! 更直接。该表在 PDF 正文内，不属于随附 C 文件。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "子带下标以 64 为界、表下标已对 4 取模，时隙范围由函数开头的\
              前置检查给出"
)]

use crate::aspx::bands::AspxBandTables;
use crate::aspx::frames::AspxInterval;
use crate::aspx::hfgain::AdjustedGains;
use crate::aspx::qmf::QmfSlot;
use crate::aspx::tables::NUM_QMF_SUBBANDS;

/// 子带数，`5.7.3.2` 规定恒为 64。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// 表 196 的 `SineTable`，逐行 `(实部, 虚部)`。
const SINE_TABLE: [(f32, f32); 4] = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)];

/// 音调无法生成的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneError {
    /// 区间的信号包络数与 `sine_lev_sb_adj` 携带的不符。
    EnvelopeCountMismatch { interval: u8, gains: u8 },
    /// 调整后增益不是由本次传入的完整频带布局生成。
    BandLayoutMismatch,
    /// 调整后增益不是由本次传入的完整时间布局生成。
    IntervalMismatch,
    /// A-SPX 范围超出 64 个 QMF 子带。
    SubbandOutOfRange { sbx: usize, num_sb_aspx: usize },
    /// `num_ts_in_ats` 不是表 192 定义的 1 或 2。
    TimeslotFactorOutOfRange { factor: u8 },
    /// 调整后增益采用的时隙倍率与本次请求不同。
    TimeslotFactorMismatch { gains: u8, requested: u8 },
    /// 包络的时隙区间为空或首尾颠倒。
    EmptyEnvelope { envelope: usize },
    /// 输出时隙数与区间覆盖的时隙数不符。
    OutputLengthMismatch { expected: usize, provided: usize },
}

/// 跨 A-SPX 区间携带的音调表游标。
///
/// 保存 `Pseudocode 105` 返回的最后一个下标，即下一区间的 `sine_idx_prev`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToneCursor {
    previous: u8,
}

impl ToneCursor {
    /// 建立空游标。首个区间由 `first_frame` 接管，这里的初值只在调用方从非
    /// 初始化点起解时生效。
    #[must_use]
    pub const fn new() -> Self {
        Self { previous: 0 }
    }

    /// 上一区间留下的下标，取值 `0..4`。
    #[must_use]
    pub const fn previous(&self) -> u8 {
        self.previous
    }

    /// 构造一个非初值游标，供聚合状态的重置判据使用。
    #[cfg(test)]
    pub(crate) fn mark_non_default_for_test(&mut self) {
        self.previous = 1;
    }
}

impl Default for ToneCursor {
    fn default() -> Self {
        Self::new()
    }
}

/// `Pseudocode 104`/`105`：生成本区间的 `qmf_sine`。
///
/// `out` 按 QMF 时隙索引，第 0 项对应 `atsg_sig[0] · num_ts_in_ats`；每个时隙
/// 只写 `bands` 定义的 `[sbx, sbx + num_sb_aspx)`，其余子带保持调用方交来的值。
///
/// `first_frame` 按正文只在**编解码初始化**时为真，与噪声发生器的
/// `master_reset` 不是同一个条件。
///
/// # Errors
///
/// 见 [`ToneError`]。任一条不成立时都不改写 `out` 与 `cursor`。
pub fn generate(
    gains: &AdjustedGains,
    bands: &AspxBandTables,
    interval: &AspxInterval,
    num_ts_in_ats: u8,
    first_frame: bool,
    cursor: &mut ToneCursor,
    out: &mut [QmfSlot],
) -> Result<(), ToneError> {
    let envelopes = usize::from(gains.envelopes());
    if envelopes == 0 || interval.num_atsg_sig() != gains.envelopes() {
        return Err(ToneError::EnvelopeCountMismatch {
            interval: interval.num_atsg_sig(),
            gains: gains.envelopes(),
        });
    }
    if !matches!(num_ts_in_ats, 1 | 2) {
        return Err(ToneError::TimeslotFactorOutOfRange {
            factor: num_ts_in_ats,
        });
    }
    let sbx = usize::from(bands.sbx());
    let num_sb_aspx = usize::from(bands.num_sb_aspx());
    let Some(end) = sbx.checked_add(num_sb_aspx) else {
        return Err(ToneError::SubbandOutOfRange { sbx, num_sb_aspx });
    };
    if num_sb_aspx == 0 || end > SUBBANDS {
        return Err(ToneError::SubbandOutOfRange { sbx, num_sb_aspx });
    }
    if !gains.matches_bands(bands) {
        return Err(ToneError::BandLayoutMismatch);
    }
    if !gains.matches_interval(interval) {
        return Err(ToneError::IntervalMismatch);
    }
    if gains.source_num_ts_in_ats() != num_ts_in_ats {
        return Err(ToneError::TimeslotFactorMismatch {
            gains: gains.source_num_ts_in_ats(),
            requested: num_ts_in_ats,
        });
    }
    let border = |index: usize| i32::from(interval.sig_border(index).unwrap_or(0));
    for atsg in 0..envelopes {
        if border(atsg + 1) <= border(atsg) {
            return Err(ToneError::EmptyEnvelope { envelope: atsg });
        }
    }
    // 起点非负由 `AspxInterval::derive` 的 `FrameError::NegativeStartBorder`
    // 保证，`empty()` 的全零边界被上面的 `EmptyEnvelope` 挡住，见 noisegen 同处。
    let first = border(0);

    let factor = i32::from(num_ts_in_ats);
    let slots = usize::try_from((border(envelopes) - first) * factor).unwrap_or(0);
    if out.len() != slots {
        return Err(ToneError::OutputLengthMismatch {
            expected: slots,
            provided: out.len(),
        });
    }

    // `Pseudocode 105` 的基址：初始化时取 1，否则取上一区间末项加一。
    // 与噪声不同，这里的「加一」是跨区间的推进量，不是逐子带的偏移。
    let base = if first_frame {
        1i32
    } else {
        i32::from(cursor.previous) + 1
    };

    let mut atsg = 0usize;
    let mut index = 0usize;
    for (ts, slot) in (first * factor..).zip(out.iter_mut()) {
        if ts == border(atsg + 1) * factor {
            atsg += 1;
        }
        // `ts − atsg_sig[0]` 按伪码字面：减未乘倍率的边界，见模块文档。
        // 下标与 `sb` 无关，整条时隙共用一项。
        index = usize::try_from((base + (ts - first)).rem_euclid(4)).unwrap_or(0);
        let (table_re, table_im) = SINE_TABLE[index];
        for sb in 0..num_sb_aspx {
            let level = gains.sine_level(sb, atsg).unwrap_or(0.0);
            // `pow(-1, sb+sbx)` 只作用于虚部，且按绝对子带号取奇偶。
            let sign = if (sb + sbx) % 2 == 0 { 1.0 } else { -1.0 };
            slot.re[sb + sbx] = level * table_re;
            slot.im[sb + sbx] = level * sign * table_im;
        }
    }

    cursor.previous = u8::try_from(index).unwrap_or(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::aspx::dequant::ScaleFactors;
    use crate::aspx::frames::AspxIntervalParams;
    use crate::aspx::hfadjust::{EnvelopeEstimate, SinePlacement, SineState, estimate};
    use crate::aspx::hfgain::{LimiterMode, adjust};
    use crate::aspx::limiter::LimiterTable;
    use crate::aspx::patches::PatchTable;
    use crate::aspx::tables::IntervalClass;
    use std::vec;

    const SLOTS: u8 = 16;

    fn bands() -> AspxBandTables {
        AspxBandTables::derive(false, 0, 0, 0, 0).expect("应能推出频带表")
    }

    /// 跑完 `5.7.6.4.2`，得到 `sine_lev_sb_adj` 非零的补偿增益。
    ///
    /// `aspx_add_harmonic` 全开：`sine_idx` 在每个高分辨率组正中命中，
    /// `scf_noise` 取 3 使 `Pseudocode 94` 的正弦电平非零。
    fn gains_with_tones(
        bands: &AspxBandTables,
        interval: &AspxInterval,
        num_ts_in_ats: u8,
        placement: SinePlacement,
    ) -> AdjustedGains {
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let envelopes = usize::from(interval.num_atsg_sig());
        let mut q_high = [QmfSlot::zero(); 64];
        for slot in &mut q_high {
            for sb in sbx..sbx + count {
                slot.re[sb] = 1.0;
                slot.im[sb] = 1.0;
            }
        }
        let mut sf = ScaleFactors::new();
        sf.fill_for_test(
            envelopes,
            usize::from(bands.num_sbg_sig_highres()),
            usize::from(bands.num_sbg_noise()),
            1.0,
            3.0,
        );
        let mut estimated = EnvelopeEstimate::new();
        estimate(
            &q_high,
            bands,
            interval,
            &sf,
            &[true; 32],
            placement,
            true,
            num_ts_in_ats,
            &mut SineState::new(),
            &mut estimated,
        )
        .expect("应能估计包络");
        let patches = PatchTable::derive(bands, false, false).expect("应能推出 patch 表");
        let table = LimiterTable::derive(bands, &patches).expect("应能推出 limiter 表");
        let mut out = AdjustedGains::new();
        adjust(
            &estimated,
            bands,
            LimiterMode::On {
                table: &table,
                patches: &patches,
            },
            &mut out,
        )
        .expect("应能算出补偿增益");
        out
    }

    fn fixfix(envelopes: u8) -> AspxInterval {
        fixfix_slots(SLOTS, envelopes)
    }

    /// 表 194 允许的时隙数：6、8、12、15、16。
    fn fixfix_slots(slots: u8, envelopes: u8) -> AspxInterval {
        let params = AspxIntervalParams::fixfix(envelopes);
        AspxInterval::derive(&params, slots, 1, true, i16::from(slots)).expect("应能推导区间")
    }

    /// 从写出的复样本反查用到的表下标。
    ///
    /// 表只有四项且两两不同，乘上同一非零电平后仍两两不同；虚部另带
    /// `pow(-1, sb+sbx)`，因此反查要把该符号一并还原。比较逐位精确。
    #[track_caller]
    fn recover_index(slot: &QmfSlot, sb: usize, sbx: usize, level: f32) -> usize {
        assert!(level != 0.0, "电平为零时无法反查下标");
        let sign = if (sb + sbx).is_multiple_of(2) {
            1.0f32
        } else {
            -1.0
        };
        let re = slot.re[sb + sbx];
        let im = slot.im[sb + sbx];
        let mut found = None;
        for (index, (tr, ti)) in SINE_TABLE.iter().enumerate() {
            if re == level * tr && im == level * sign * ti {
                assert!(found.is_none(), "下标 {index} 与 {found:?} 同时匹配");
                found = Some(index);
            }
        }
        found.unwrap_or_else(|| panic!("子带 {sb} 的样本不匹配任何表项：({re}, {im})"))
    }

    #[test]
    fn the_index_advances_one_step_per_timeslot_and_is_shared_by_all_subbands() {
        // `Pseudocode 105` 的偏移只有 `ts − atsg_sig[0]`，没有 `sb`：同一时隙的
        // 全部子带取同一项，逐时隙走一格并按 4 回绕。照噪声那样补 `sb` 会让
        // 每个子带各转各的相位，这条同时挡住那种写法。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = fixfix(1);
        let gains = gains_with_tones(&bands, &interval, 1, SinePlacement::from_params(-1));
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = ToneCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("应能生成音调");

        let marked: std::vec::Vec<usize> = (0..count)
            .filter(|&sb| gains.sine_level(sb, 0).expect("范围内") != 0.0)
            .collect();
        assert!(marked.len() >= 3, "本用例需要至少三个被标记的子带");

        let mut previous = None;
        for (offset, slot) in out.iter().enumerate() {
            let mut shared = None;
            for &sb in &marked {
                let level = gains.sine_level(sb, 0).expect("范围内");
                let index = recover_index(slot, sb, sbx, level);
                match shared {
                    None => shared = Some(index),
                    Some(expected) => {
                        assert_eq!(index, expected, "时隙 {offset} 的子带间下标不一致")
                    }
                }
            }
            let index = shared.expect("至少一个被标记子带");
            if let Some(prev) = previous {
                assert_eq!(index, (prev + 1) % 4, "时隙 {offset} 没有前进一格");
            }
            previous = Some(index);
        }
        // 16 个时隙走 4 项，必定回绕多次。
        assert_eq!(usize::from(cursor.previous()), previous.expect("非空"));
    }

    #[test]
    fn the_first_frame_starts_at_index_one() {
        // `Pseudocode 105` 的 `first_frame` 分支取 1，不是 0——首个时隙的表项
        // 因此是 `(0, 1)` 而非 `(1, 0)`。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let interval = fixfix(1);
        let gains = gains_with_tones(&bands, &interval, 1, SinePlacement::from_params(-1));
        let sb = (0..usize::from(bands.num_sb_aspx()))
            .find(|&sb| gains.sine_level(sb, 0).expect("范围内") != 0.0)
            .expect("应有被标记的子带");
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = ToneCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("应能生成音调");
        let level = gains.sine_level(sb, 0).expect("范围内");
        assert_eq!(
            recover_index(&out[0], sb, sbx, level),
            1,
            "首帧首项应是下标 1"
        );
    }

    #[test]
    fn the_cursor_continues_from_previous_plus_one() {
        // 非首帧时基址是「上一区间末项**加一**」。本用例的 FIXFIX 起点为 0、
        // 时隙倍率为 1，因此首个时隙的区间内偏移为 0：末项 k 接到首项 k+1。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let interval = fixfix(1);
        // 首个时隙的区间内偏移是 `atsg_sig[0] · (num_ts_in_ats − 1)`，任一因子为零
        // 它就归零，「末项接首项」才成立。倍率在下方调用处是字面 1，一眼可见；
        // 起点由夹具推导、调用处看不见，钉在这里。换成非零起点且倍率为 2 的区间时
        // 本判据会失败，失败的原因是前提不满足，不是「加一」这条判读错了。
        assert_eq!(interval.sig_border(0), Some(0), "本判据要求零起点");
        let gains = gains_with_tones(&bands, &interval, 1, SinePlacement::from_params(-1));
        let sb = (0..usize::from(bands.num_sb_aspx()))
            .find(|&sb| gains.sine_level(sb, 0).expect("范围内") != 0.0)
            .expect("应有被标记的子带");
        let level = gains.sine_level(sb, 0).expect("范围内");

        let mut cursor = ToneCursor::new();
        let mut first = vec![QmfSlot::zero(); usize::from(SLOTS)];
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut first).expect("第一区间");
        let carried = usize::from(cursor.previous());

        let mut second = vec![QmfSlot::zero(); usize::from(SLOTS)];
        generate(
            &gains,
            &bands,
            &interval,
            1,
            false,
            &mut cursor,
            &mut second,
        )
        .expect("第二区间");
        assert_eq!(
            recover_index(&second[0], sb, sbx, level),
            (carried + 1) % 4,
            "跨区间应在末项基础上再加一"
        );
    }

    #[test]
    fn the_first_frame_flag_is_not_the_master_reset_flag() {
        // 噪声发生器用 `master_reset`，音调用 `first_frame`；两者语义不同，
        // 后者只在编解码初始化时为真。这里固定同一份增益连跑两个区间：
        // 第二区间取 first_frame 为假时接着走，为真时回到下标 1。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        // 时隙数必须不是 4 的倍数：16 个时隙下末项恒回到 0，`(carried+1)%4`
        // 恰好等于 `first_frame` 分支的 1，两条分支重合、判据失去鉴别力。
        let interval = fixfix_slots(15, 1);
        let gains = gains_with_tones(&bands, &interval, 1, SinePlacement::from_params(-1));
        let sb = (0..usize::from(bands.num_sb_aspx()))
            .find(|&sb| gains.sine_level(sb, 0).expect("范围内") != 0.0)
            .expect("应有被标记的子带");
        let level = gains.sine_level(sb, 0).expect("范围内");

        let mut cursor = ToneCursor::new();
        let mut out = vec![QmfSlot::zero(); 15];
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("第一区间");
        let carried = usize::from(cursor.previous());
        assert_ne!(carried, 0, "携带值应非零，否则两条分支看不出区别");

        let mut resumed = cursor;
        let mut a = vec![QmfSlot::zero(); 15];
        generate(&gains, &bands, &interval, 1, false, &mut resumed, &mut a).expect("续跑");
        let mut restarted = cursor;
        let mut b = vec![QmfSlot::zero(); 15];
        generate(&gains, &bands, &interval, 1, true, &mut restarted, &mut b).expect("重新初始化");

        assert_eq!(recover_index(&a[0], sb, sbx, level), (carried + 1) % 4);
        assert_eq!(recover_index(&b[0], sb, sbx, level), 1);
        assert_ne!(
            recover_index(&a[0], sb, sbx, level),
            recover_index(&b[0], sb, sbx, level),
            "两条分支必须给出不同结果，否则本判据无鉴别力"
        );
    }

    #[test]
    fn only_the_imaginary_part_carries_the_absolute_subband_sign() {
        // `pow(-1, sb+sbx)` 只乘在虚部，且按**绝对**子带号取奇偶。取两个相邻
        // 且都被标记的子带，它们的绝对号奇偶相反：实部同号，虚部反号。
        //
        // **`sbx` 必须是奇数。** 默认夹具的 `sbx = 10`，此时
        // `(sb + sbx) % 2 == sb % 2`，「绝对号」与「相对号」两种写法给出相同
        // 结果，把 `sb + sbx` 改成 `sb` 的注入一条判据都不会响。改用
        // `xover = 1` 的配置（`sbx = 11`）后两者才分得开。
        let bands = AspxBandTables::derive(false, 0, 0, 0, 1).expect("应能推出频带表");
        let sbx = usize::from(bands.sbx());
        assert_eq!(sbx % 2, 1, "本判据要求奇数 sbx，否则绝对与相对号不可分辨");
        let count = usize::from(bands.num_sb_aspx());
        let interval = fixfix(1);
        let gains = gains_with_tones(&bands, &interval, 1, SinePlacement::from_params(-1));
        let pair = (0..count - 1)
            .find(|&sb| {
                gains.sine_level(sb, 0).expect("范围内") != 0.0
                    && gains.sine_level(sb + 1, 0).expect("范围内") != 0.0
            })
            .expect("应有相邻两个被标记的子带");
        assert_eq!(
            (pair + sbx) % 2,
            1 - (pair + 1 + sbx) % 2,
            "相邻子带的绝对号奇偶必然相反"
        );

        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = ToneCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("应能生成音调");

        // 首帧首项是表 196 的第 1 行 `(0, 1)`：实部为零、虚部非零，符号可见。
        let slot = &out[0];
        let lo = gains.sine_level(pair, 0).expect("范围内");
        let hi = gains.sine_level(pair + 1, 0).expect("范围内");
        assert_eq!(lo, hi, "同组两个子带的电平应相同，否则符号判据被幅度污染");
        assert_eq!(slot.re[pair + sbx], 0.0, "该表项实部为零");
        assert_eq!(slot.re[pair + 1 + sbx], 0.0);
        assert_eq!(
            slot.im[pair + sbx],
            -slot.im[pair + 1 + sbx],
            "相邻子带的虚部必须反号"
        );
        assert_ne!(slot.im[pair + sbx], 0.0, "虚部应非零，否则反号无从判断");

        // 绝对号为偶的那个取正号。`sbx` 为奇数，因此这一对里取正号的是**相对
        // 号为奇**的那个——按相对号写的实现会把符号整体反过来。
        let even_absolute = if (pair + sbx) % 2 == 0 {
            pair
        } else {
            pair + 1
        };
        assert_eq!(
            even_absolute % 2,
            1,
            "奇数 sbx 下绝对号为偶对应相对号为奇，两种写法就此分开"
        );
        assert!(
            slot.im[even_absolute + sbx] > 0.0,
            "绝对子带号为偶时 pow(-1, sb+sbx) 取 +1"
        );
    }

    #[test]
    fn the_level_follows_the_envelope_borders() {
        // 两个包络的 `sine_lev_sb_adj` 不同时，边界两侧的幅度必须换值。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let interval = fixfix(2);
        // 指针 0：两个包络都有正弦标记（`atsg >= 0` 恒真），但只有包络 0 是
        // 正弦起点，`Pseudocode 99` 的 boost 因此逐包络不同。
        let gains = gains_with_tones(&bands, &interval, 1, SinePlacement::from_params(0));
        let sb = (0..usize::from(bands.num_sb_aspx()))
            .find(|&sb| {
                let a = gains.sine_level(sb, 0).expect("范围内");
                let b = gains.sine_level(sb, 1).expect("范围内");
                a != 0.0 && b != 0.0 && a != b
            })
            .expect("应有两个包络电平不同的被标记子带");
        let early = gains.sine_level(sb, 0).expect("范围内");
        let late = gains.sine_level(sb, 1).expect("范围内");
        let split = usize::try_from(interval.sig_border(1).expect("应有中间边界")).expect("非负");

        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = ToneCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("应能生成音调");
        for (offset, slot) in out.iter().enumerate() {
            let level = if offset < split { early } else { late };
            recover_index(slot, sb, sbx, level);
        }
        assert!(split > 0 && split < usize::from(SLOTS));
    }

    #[test]
    fn the_start_border_enters_the_offset_without_the_timeslot_factor() {
        // 与 `Pseudocode 103` 同款的量纲问题：倍率 2 且起点非零时，首个时隙的
        // 偏移是 `atsg_sig[0]` 而不是 0。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let mut params = AspxIntervalParams::fixfix(1);
        params.int_class = IntervalClass::VarFix;
        params.var_bord_left = Some(2);
        let interval = AspxInterval::derive(&params, SLOTS, 1, true, 16).expect("应能推导区间");
        assert_eq!(interval.sig_border(0), Some(2), "本用例需要非零起点");
        let gains = gains_with_tones(&bands, &interval, 2, SinePlacement::from_params(-1));
        let sb = (0..usize::from(bands.num_sb_aspx()))
            .find(|&sb| gains.sine_level(sb, 0).expect("范围内") != 0.0)
            .expect("应有被标记的子带");
        let level = gains.sine_level(sb, 0).expect("范围内");

        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS - 2) * 2];
        let mut cursor = ToneCursor::new();
        generate(&gains, &bands, &interval, 2, true, &mut cursor, &mut out).expect("应能生成音调");
        assert_eq!(
            recover_index(&out[0], sb, sbx, level),
            // `first_frame` 的 1 加上未乘倍率的 `atsg_sig[0] = 2`；和小于 4，
            // 取模是恒等的，故不写 `% 4`。
            1 + 2,
            "首项偏移应是 atsg_sig[0] 加上 first_frame 的 1"
        );
    }

    #[test]
    fn only_the_aspx_range_is_written() {
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = fixfix(1);
        let gains = gains_with_tones(&bands, &interval, 1, SinePlacement::from_params(-1));
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        for slot in &mut out {
            for sb in 0..SUBBANDS {
                slot.re[sb] = 7.0;
                slot.im[sb] = -7.0;
            }
        }
        let mut cursor = ToneCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("应能生成音调");
        for slot in &out {
            for sb in 0..SUBBANDS {
                if sb >= sbx && sb < sbx + count {
                    assert_ne!(
                        (slot.re[sb], slot.im[sb]),
                        (7.0, -7.0),
                        "子带 {sb} 应被写过"
                    );
                } else {
                    assert_eq!(slot.re[sb], 7.0, "子带 {sb} 在 A-SPX 之外");
                    assert_eq!(slot.im[sb], -7.0, "子带 {sb} 在 A-SPX 之外");
                }
            }
        }
    }

    #[test]
    fn rejected_input_leaves_the_output_and_cursor_untouched() {
        let bands = bands();
        let interval = fixfix(1);
        let gains = gains_with_tones(&bands, &interval, 1, SinePlacement::from_params(-1));
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = ToneCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("哨兵结果");
        let snapshot = out.clone();
        let cursor_snapshot = cursor;

        for factor in [0, 3] {
            assert_eq!(
                generate(
                    &gains,
                    &bands,
                    &interval,
                    factor,
                    false,
                    &mut cursor,
                    &mut out
                ),
                Err(ToneError::TimeslotFactorOutOfRange { factor })
            );
        }
        assert_eq!(
            generate(&gains, &bands, &interval, 2, false, &mut cursor, &mut out),
            Err(ToneError::TimeslotFactorMismatch {
                gains: 1,
                requested: 2
            })
        );
        let two = fixfix(2);
        assert_eq!(
            generate(&gains, &bands, &two, 1, false, &mut cursor, &mut out),
            Err(ToneError::EnvelopeCountMismatch {
                interval: 2,
                gains: 1
            })
        );
        // 另一套合法频带布局：子带数与 sbx 都不同，来源核对先于范围命中。
        let foreign = AspxBandTables::derive(false, 1, 0, 0, 0).expect("应能推出频带表");
        assert_eq!(
            generate(&gains, &foreign, &interval, 1, false, &mut cursor, &mut out),
            Err(ToneError::BandLayoutMismatch)
        );
        let mut short = vec![QmfSlot::zero(); 4];
        let short_snapshot = short.clone();
        assert_eq!(
            generate(&gains, &bands, &interval, 1, false, &mut cursor, &mut short),
            Err(ToneError::OutputLengthMismatch {
                expected: usize::from(SLOTS),
                provided: 4
            })
        );
        assert_eq!(short, short_snapshot, "长度错误不应改写实际传入的输出");

        assert_eq!(out, snapshot, "被拒绝的输入不应改写输出");
        assert_eq!(cursor, cursor_snapshot, "被拒绝的输入不应推进游标");
    }
}
