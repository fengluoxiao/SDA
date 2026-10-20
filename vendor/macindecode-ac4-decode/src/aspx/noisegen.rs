//! 噪声发生器（`TS103190-1:v1.4.1:5.7.6.4.3`）。
//!
//! `Pseudocode 102` 把 `5.7.6.4.2.2` 的 `noise_lev_sb_adj` 逐时隙铺开，乘上表
//! D.2 的 512 个复数；`Pseudocode 103` 给出取表的下标。输出 `qmf_noise` 与
//! `Q_high` 同形，供 `5.7.6.4.5` 叠加。
//!
//! # `noise_idx_prev` 是标量，不是矩阵
//!
//! `Pseudocode 103` 写 `indexNoise = noise_idx_prev[sb][ts]`，紧跟其后的正文却
//! 说「`noise_idx_prev` 是上一个 A-SPX 区间的**最后一个** `noise_idx`」——单数。
//! 三条证据支持标量读法：
//!
//! - 下标 `[sb][ts]` 里的 `ts` 属于**当前**区间，用它去索引上一区间的矩阵没有
//!   意义（两个区间的时隙范围不同）；
//! - 矩阵读法下每个 `(sb, ts)` 各带一个不同的基址，`+ num_sb_aspx·Δts + sb + 1`
//!   这个光栅偏移就失去意义；
//! - 标量读法下整个区间被同一个基址平移，区间内的偏移恰好走 `1 … num_sb_aspx·N`，
//!   末项接上下一区间的基址，512 项的表被连续走过——这正是「上一个区间的最后
//!   一个」这句话描述的行为。
//!
//! 与 `Pseudocode 91` 把循环变量 `atsg_noise` 当数组用是同一类排印错误。
//!
//! # `ts − atsg_sig[0]` 的单位不一致，按字面保留
//!
//! `Pseudocode 102` 的 `ts` 以 QMF 时隙计（循环从 `atsg_sig[0]·num_ts_in_ats`
//! 起），而 `Pseudocode 103` 减去的是未乘倍率的 `atsg_sig[0]`。倍率为 2 时两者
//! 不同量纲，区间内第一个时隙的偏移不是 0 而是 `atsg_sig[0]`。
//!
//! 「本意应是区间内的时隙序号」只是推测：`% 512` 使字面读法不会越界，也不破坏
//! 任何规范明写的性质，因此**没有旁证**说它是笔误，按字面执行。它不同于
//! `Pseudocode 90` 的归一化——后者与同节定义、正文及相邻能量公式直接冲突；也
//! 不同于分号那处——那里有语义矛盾加下标越界的双重证据。
//!
//! # 表的列序无法判定
//!
//! 表 D.2 只给 `num_columns 2`，正文只说「512 个复数」。「随机相位」与「平均
//! 能量为 1」在实虚互换下都成立，故数据也判不出来。取 C 的常规写法
//! `{re, im}`，见 `build_support/noise.rs`。
//!
//! # 增益、频带与时间布局必须同源
//!
//! `AdjustedGains` 的列由 `AspxInterval` 的边界划分，行则相对 `AspxBandTables::sbx`
//! 编号；只比较包络数与子带数会接受同形但不同源的结果。来源因此从包络估计一路
//! 携带到调整后增益，本模块在写任何输出前精确核对频带、区间与时隙倍率。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "子带下标以 64 为界、表下标已对 512 取模，时隙范围由函数开头的\
              前置检查给出"
)]

use crate::aspx::bands::AspxBandTables;
use crate::aspx::frames::AspxInterval;
use crate::aspx::hfgain::AdjustedGains;
use crate::aspx::qmf::QmfSlot;
use crate::aspx::tables::NUM_QMF_SUBBANDS;

mod table {
    include!(concat!(env!("OUT_DIR"), "/aspx_noise.rs"));
}

/// 子带数，`5.7.3.2` 规定恒为 64。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// 表 D.2 的行数。
const NOISE_TABLE_LEN: usize = 512;

/// 噪声无法生成的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseError {
    /// 区间的信号包络数与 `noise_lev_sb_adj` 携带的不符。
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

/// 跨 A-SPX 区间携带的噪声表游标。
///
/// 保存 `Pseudocode 103` 返回的最后一个下标，即下一区间的 `noise_idx_prev`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseCursor {
    previous: u16,
}

impl NoiseCursor {
    /// 建立空游标。首个区间的 `master_reset` 会把它归零，这里的初值只在
    /// 调用方从非 I 帧起解时生效。
    #[must_use]
    pub const fn new() -> Self {
        Self { previous: 0 }
    }

    /// 上一区间留下的下标。
    #[must_use]
    pub const fn previous(&self) -> u16 {
        self.previous
    }

    /// 构造一个非初值游标，供聚合状态的重置判据使用。
    #[cfg(test)]
    pub(crate) fn mark_non_default_for_test(&mut self) {
        self.previous = 1;
    }
}

impl Default for NoiseCursor {
    fn default() -> Self {
        Self::new()
    }
}

/// `Pseudocode 102`/`103`：生成本区间的 `qmf_noise`。
///
/// `out` 按 QMF 时隙索引，第 0 项对应 `atsg_sig[0] · num_ts_in_ats`；每个时隙
/// 只写 `bands` 定义的 `[sbx, sbx + num_sb_aspx)`，其余子带保持调用方交来的值。
///
/// `master_reset` 见 `5.7.6.3.1.1`：`aspx_master_freq_scale`、`aspx_start_freq`
/// 或 `aspx_stop_freq` 相对上一个 I 帧发生变化时为真。
///
/// # Errors
///
/// 见 [`NoiseError`]。任一条不成立时都不改写 `out` 与 `cursor`。
pub fn generate(
    gains: &AdjustedGains,
    bands: &AspxBandTables,
    interval: &AspxInterval,
    num_ts_in_ats: u8,
    master_reset: bool,
    cursor: &mut NoiseCursor,
    out: &mut [QmfSlot],
) -> Result<(), NoiseError> {
    let envelopes = usize::from(gains.envelopes());
    if envelopes == 0 || interval.num_atsg_sig() != gains.envelopes() {
        return Err(NoiseError::EnvelopeCountMismatch {
            interval: interval.num_atsg_sig(),
            gains: gains.envelopes(),
        });
    }
    if !matches!(num_ts_in_ats, 1 | 2) {
        return Err(NoiseError::TimeslotFactorOutOfRange {
            factor: num_ts_in_ats,
        });
    }
    let sbx = usize::from(bands.sbx());
    let num_sb_aspx = usize::from(bands.num_sb_aspx());
    let Some(end) = sbx.checked_add(num_sb_aspx) else {
        return Err(NoiseError::SubbandOutOfRange { sbx, num_sb_aspx });
    };
    if num_sb_aspx == 0 || end > SUBBANDS {
        return Err(NoiseError::SubbandOutOfRange { sbx, num_sb_aspx });
    }
    if !gains.matches_bands(bands) {
        return Err(NoiseError::BandLayoutMismatch);
    }
    if !gains.matches_interval(interval) {
        return Err(NoiseError::IntervalMismatch);
    }
    if gains.source_num_ts_in_ats() != num_ts_in_ats {
        return Err(NoiseError::TimeslotFactorMismatch {
            gains: gains.source_num_ts_in_ats(),
            requested: num_ts_in_ats,
        });
    }
    let border = |index: usize| i32::from(interval.sig_border(index).unwrap_or(0));
    for atsg in 0..envelopes {
        if border(atsg + 1) <= border(atsg) {
            return Err(NoiseError::EmptyEnvelope { envelope: atsg });
        }
    }
    // `Pseudocode 103` 的 `ts − atsg_sig[0]` 只在起始边界非负时恒非负，而 C 的
    // `%` 对负数向零取整会给出负下标。这里不重复检查：`AspxInterval` 只能由
    // `derive` 构造，而它已用 `FrameError::NegativeStartBorder` 拒绝了负起点，
    // `empty()` 给出的全零边界则被上面的 `EmptyEnvelope` 挡住。
    let first = border(0);

    let factor = i32::from(num_ts_in_ats);
    // 起始边界非负且边界严格递增，故跨度与全部时隙下标都非负。
    let slots = usize::try_from((border(envelopes) - first) * factor).unwrap_or(0);
    if out.len() != slots {
        return Err(NoiseError::OutputLengthMismatch {
            expected: slots,
            provided: out.len(),
        });
    }

    // `Pseudocode 103` 的基址：`master_reset` 时归零，否则接上一区间的末项。
    // 它对整个区间是常量，区间内的变化全在下面的光栅偏移里。
    let base = if master_reset {
        0i64
    } else {
        i64::from(cursor.previous)
    };

    let mut atsg = 0usize;
    let mut index = 0i64;
    // `ts` 与伪码同为绝对 QMF 时隙号，从 `atsg_sig[0] · num_ts_in_ats` 起。
    for (ts, slot) in (first * factor..).zip(out.iter_mut()) {
        if ts == border(atsg + 1) * factor {
            atsg += 1;
        }
        // `ts − atsg_sig[0]` 按伪码字面：减的是未乘倍率的边界，见模块文档。
        let raster = i64::from(num_sb_aspx as i32) * i64::from(ts - first);
        for sb in 0..num_sb_aspx {
            index = (base + raster + sb as i64 + 1) % NOISE_TABLE_LEN as i64;
            let [re, im] = table::ASPX_NOISE[index as usize];
            let level = gains.noise_level(sb, atsg).unwrap_or(0.0);
            slot.re[sb + sbx] = level * re;
            slot.im[sb + sbx] = level * im;
        }
    }

    // `Pseudocode 103` 返回取模后的值，正文说携带的是「最后一个 noise_idx」。
    cursor.previous = u16::try_from(index).unwrap_or(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::aspx::bands::AspxBandTables;
    use crate::aspx::dequant::ScaleFactors;
    use crate::aspx::frames::AspxIntervalParams;
    use crate::aspx::hfadjust::{EnvelopeEstimate, SinePlacement, SineState, estimate};
    use crate::aspx::hfgain::{LimiterMode, adjust};
    use crate::aspx::limiter::LimiterTable;
    use crate::aspx::patches::PatchTable;
    use crate::aspx::tables::IntervalClass;
    use std::vec;
    use std::vec::Vec;

    /// A-SPX 时隙数。
    const SLOTS: u8 = 16;

    fn bands() -> AspxBandTables {
        AspxBandTables::derive(false, 0, 0, 0, 0).expect("应能推出频带表")
    }

    /// 跑完 `5.7.6.4.2`，得到一份 `noise_lev_sb_adj` 非零的补偿增益。
    fn gains_for(
        bands: &AspxBandTables,
        interval: &AspxInterval,
        placement: SinePlacement,
        num_ts_in_ats: u8,
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
            1.0,
        );
        let mut estimated = EnvelopeEstimate::new();
        estimate(
            &q_high,
            bands,
            interval,
            &sf,
            &[false; 32],
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
        let params = AspxIntervalParams::fixfix(envelopes);
        AspxInterval::derive(&params, SLOTS, 1, true, 16).expect("应能推导区间")
    }

    /// 从写出的复样本反查用到的表下标。
    ///
    /// 表的 512 项两两不同（由
    /// [`the_table_entries_are_distinct_so_indices_can_be_recovered`] 断言），
    /// 乘上同一个非零电平后仍两两不同，因此匹配唯一。比较是逐位精确的：生产
    /// 代码算的就是 `level * 表项`，不引入额外舍入。
    #[track_caller]
    fn recover_index(slot: &QmfSlot, sb: usize, sbx: usize, level: f32) -> usize {
        assert!(level != 0.0, "电平为零时无法反查下标");
        let re = slot.re[sb + sbx];
        let im = slot.im[sb + sbx];
        let mut found = None;
        for (index, [tr, ti]) in table::ASPX_NOISE.iter().enumerate() {
            if re == level * tr && im == level * ti {
                assert!(found.is_none(), "下标 {index} 与 {found:?} 同时匹配");
                found = Some(index);
            }
        }
        found.unwrap_or_else(|| panic!("子带 {sb} 的样本不是任何表项的 {level} 倍"))
    }

    /// 按光栅顺序取出整段区间用到的下标。
    fn indices(out: &[QmfSlot], gains: &AdjustedGains, sbx: usize, borders: &[i32]) -> Vec<usize> {
        let count = usize::from(gains.subbands());
        let mut atsg = 0usize;
        let mut result = Vec::new();
        for (offset, slot) in out.iter().enumerate() {
            let ts = borders[0] + offset as i32;
            if atsg + 1 < borders.len() - 1 && ts == borders[atsg + 1] {
                atsg += 1;
            }
            for sb in 0..count {
                let level = gains.noise_level(sb, atsg).expect("电平应在范围内");
                result.push(recover_index(slot, sb, sbx, level));
            }
        }
        result
    }

    #[test]
    fn the_table_entries_are_distinct_so_indices_can_be_recovered() {
        // 反查下标的前提。规范只声称「随机相位、平均能量为 1」，两两不同是实测
        // 性质；它一旦不成立，下面每条判据的反查都会先报「同时匹配」。
        let mut seen = Vec::with_capacity(NOISE_TABLE_LEN);
        for [re, im] in &table::ASPX_NOISE {
            seen.push((re.to_bits(), im.to_bits()));
        }
        seen.sort_unstable();
        let total = seen.len();
        seen.dedup();
        assert_eq!(total, NOISE_TABLE_LEN);
        assert_eq!(seen.len(), NOISE_TABLE_LEN, "表项应两两不同");
    }

    #[test]
    fn the_index_advances_by_one_per_subband_and_wraps_at_the_table_length() {
        // `Pseudocode 103` 的偏移是 `num_sb_aspx·(ts − atsg_sig[0]) + sb + 1`：
        // 同一时隙内逐子带加一，跨时隙时上一时隙末项与下一时隙首项也只差一。
        // 整段因此是一条连续的光栅扫描，且 `+1` 使首项落在下标 1 而非 0。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = fixfix(1);
        let gains = gains_for(&bands, &interval, SinePlacement::from_params(-1), 1);
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = NoiseCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("应能生成噪声");

        let seen = indices(&out, &gains, sbx, &[0, i32::from(SLOTS)]);
        assert_eq!(seen.len(), usize::from(SLOTS) * count);
        assert_eq!(seen[0], 1, "master_reset 后首项是下标 1，不是 0");
        for (step, pair) in seen.windows(2).enumerate() {
            assert_eq!(
                pair[1],
                (pair[0] + 1) % NOISE_TABLE_LEN,
                "第 {step} 步没有前进一格"
            );
        }
        // 16 个时隙 × 36 子带 = 576 > 512，因此这一段必定绕过表尾一次。
        assert!(seen.contains(&0), "本段应绕过表尾并取到下标 0");
        assert_eq!(
            usize::from(cursor.previous()),
            *seen.last().expect("非空"),
            "游标应停在本区间的最后一个下标"
        );
    }

    #[test]
    fn the_cursor_continues_into_the_next_interval() {
        // `noise_idx_prev` 是标量：整个下一区间被同一个基址平移，因此两个区间
        // 首尾相接。若按矩阵读法逐 `(sb, ts)` 取不同基址，这条会断。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let interval = fixfix(1);
        let gains = gains_for(&bands, &interval, SinePlacement::from_params(-1), 1);
        let mut cursor = NoiseCursor::new();
        let mut first = vec![QmfSlot::zero(); usize::from(SLOTS)];
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut first).expect("第一区间");
        let carried = cursor.previous();

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
        let seen = indices(&second, &gains, sbx, &[0, i32::from(SLOTS)]);
        assert_eq!(
            seen[0],
            (usize::from(carried) + 1) % NOISE_TABLE_LEN,
            "第二区间应接着上一区间的末项走"
        );
        assert_ne!(carried, 0, "本用例需要一个非零的携带值才有区分力");
    }

    #[test]
    fn a_master_reset_restarts_the_walk_at_one() {
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let interval = fixfix(1);
        let gains = gains_for(&bands, &interval, SinePlacement::from_params(-1), 1);
        let mut cursor = NoiseCursor::new();
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("第一区间");
        assert_ne!(cursor.previous(), 0, "携带值应非零，否则重置无从区分");

        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("第二区间");
        let seen = indices(&out, &gains, sbx, &[0, i32::from(SLOTS)]);
        assert_eq!(seen[0], 1, "master_reset 应丢弃携带值");
    }

    #[test]
    fn the_start_border_enters_the_offset_without_the_timeslot_factor() {
        // `Pseudocode 102` 的 `ts` 以 QMF 时隙计，`Pseudocode 103` 却减去未乘
        // `num_ts_in_ats` 的 `atsg_sig[0]`。倍率为 2 且起点非零时，区间内第一个
        // 时隙的偏移不是 0 而是 `num_sb_aspx · atsg_sig[0]`，首项下标随之变化。
        // 这条锁死的是规范字面，不是「本意」——见模块文档。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let mut params = AspxIntervalParams::fixfix(1);
        params.int_class = IntervalClass::VarFix;
        params.var_bord_left = Some(2);
        let interval = AspxInterval::derive(&params, SLOTS, 1, true, 16).expect("应能推导区间");
        assert_eq!(interval.sig_border(0), Some(2), "本用例需要非零起点");

        let gains = gains_for(&bands, &interval, SinePlacement::from_params(-1), 2);
        let slots = usize::from(SLOTS - 2) * 2;
        let mut out = vec![QmfSlot::zero(); slots];
        let mut cursor = NoiseCursor::new();
        generate(&gains, &bands, &interval, 2, true, &mut cursor, &mut out).expect("应能生成噪声");

        let level = gains.noise_level(0, 0).expect("电平应在范围内");
        let first = recover_index(&out[0], 0, sbx, level);
        assert_eq!(
            first,
            (count * 2 + 1) % NOISE_TABLE_LEN,
            "首项偏移应是 num_sb_aspx · atsg_sig[0] 加一"
        );
        assert_ne!(first, 1, "若按乘过倍率的起点相减，首项会退回下标 1");
    }

    #[test]
    fn the_level_follows_the_envelope_borders() {
        // 指针指向第 1 个包络：`5.7.6.4.2.2` 让两个包络的 noise_lev_sb_adj 不同
        // （sqrt(0,6) 与 sqrt(0,75)），因此边界两侧的缩放必须换值。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let interval = fixfix(2);
        let gains = gains_for(&bands, &interval, SinePlacement::from_params(1), 1);
        let early = gains.noise_level(0, 0).expect("电平应在范围内");
        let late = gains.noise_level(0, 1).expect("电平应在范围内");
        assert_ne!(early, late, "两个包络的电平必须不同，否则判据看不出边界");
        let split = usize::try_from(interval.sig_border(1).expect("应有中间边界")).expect("非负");

        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = NoiseCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("应能生成噪声");

        // 逐时隙反查：用错电平会让反查找不到任何匹配项而 panic。
        for (offset, slot) in out.iter().enumerate() {
            let level = if offset < split { early } else { late };
            recover_index(slot, 0, sbx, level);
        }
        assert!(
            split > 0 && split < usize::from(SLOTS),
            "边界应落在区间内部"
        );
    }

    #[test]
    fn only_the_aspx_range_is_written() {
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = fixfix(1);
        let gains = gains_for(&bands, &interval, SinePlacement::from_params(-1), 1);
        // 哨兵铺满整个时隙；只有 A-SPX 范围内的才该被改写。
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        for slot in &mut out {
            for sb in 0..SUBBANDS {
                slot.re[sb] = 7.0;
                slot.im[sb] = -7.0;
            }
        }
        let mut cursor = NoiseCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out).expect("应能生成噪声");

        for slot in &out {
            for sb in 0..SUBBANDS {
                if sb >= sbx && sb < sbx + count {
                    assert_ne!(slot.re[sb], 7.0, "A-SPX 内的子带 {sb} 应被写过");
                } else {
                    assert_eq!(slot.re[sb], 7.0, "子带 {sb} 在 A-SPX 之外");
                    assert_eq!(slot.im[sb], -7.0, "子带 {sb} 在 A-SPX 之外");
                }
            }
        }
    }

    #[test]
    fn a_same_shape_foreign_interval_is_rejected() {
        // 两个合法区间都是双包络、总跨度 16；只有中间边界不同。只比较包络数与
        // 输出长度会把源区间 [0, 8, 16] 的两列增益按 [0, 12, 16] 错时铺开。
        let bands = bands();
        let source = fixfix(2);
        let mut params = AspxIntervalParams::fixfix(2);
        params.int_class = IntervalClass::FixVar;
        params.num_rel_right = 1;
        params.rel_bord_right[0] = 4;
        let foreign = AspxInterval::derive(&params, SLOTS, 1, true, 16).expect("应能推导外来区间");
        assert_eq!(source.sig_border(0), foreign.sig_border(0));
        assert_eq!(source.sig_border(2), foreign.sig_border(2));
        assert_eq!(source.sig_border(1), Some(8));
        assert_eq!(foreign.sig_border(1), Some(12));

        let gains = gains_for(&bands, &source, SinePlacement::from_params(1), 1);
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = NoiseCursor::new();
        generate(&gains, &bands, &source, 1, true, &mut cursor, &mut out).expect("哨兵结果");
        let snapshot = out.clone();
        let cursor_snapshot = cursor;

        assert_eq!(
            generate(&gains, &bands, &foreign, 1, false, &mut cursor, &mut out),
            Err(NoiseError::IntervalMismatch)
        );
        assert_eq!(out, snapshot, "外来时间布局不应改写输出");
        assert_eq!(cursor, cursor_snapshot, "外来时间布局不应推进游标");
    }

    #[test]
    fn a_same_width_foreign_band_layout_is_rejected() {
        // 两套合法配置都覆盖 22 个 A-SPX 子带，但 sbx 与内部频带布局不同。
        let source = AspxBandTables::derive(false, 0, 1, 0, 6).expect("应能推出来源频带表");
        let foreign = AspxBandTables::derive(false, 0, 2, 0, 0).expect("应能推出外来频带表");
        assert_eq!(source.num_sb_aspx(), 22);
        assert_eq!(foreign.num_sb_aspx(), 22);
        assert_eq!(source.sbx(), 16);
        assert_eq!(foreign.sbx(), 10);

        let interval = fixfix(1);
        let gains = gains_for(&source, &interval, SinePlacement::from_params(-1), 1);
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = NoiseCursor::new();
        generate(&gains, &source, &interval, 1, true, &mut cursor, &mut out).expect("哨兵结果");
        let snapshot = out.clone();
        let cursor_snapshot = cursor;

        assert_eq!(
            generate(&gains, &foreign, &interval, 1, false, &mut cursor, &mut out),
            Err(NoiseError::BandLayoutMismatch)
        );
        assert_eq!(out, snapshot, "外来频带布局不应改写输出");
        assert_eq!(cursor, cursor_snapshot, "外来频带布局不应推进游标");
    }

    #[test]
    fn rejected_input_leaves_the_output_and_cursor_untouched() {
        let bands = bands();
        let interval = fixfix(1);
        let gains = gains_for(&bands, &interval, SinePlacement::from_params(-1), 1);
        let mut out = vec![QmfSlot::zero(); usize::from(SLOTS)];
        let mut cursor = NoiseCursor::new();
        generate(&gains, &bands, &interval, 1, true, &mut cursor, &mut out)
            .expect("哨兵结果应可生成");
        let snapshot = out.clone();
        let cursor_snapshot = cursor;

        // 表 192 之外的倍率必须在任何乘法之前被拒绝。
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
                Err(NoiseError::TimeslotFactorOutOfRange { factor })
            );
        }
        // 合法但与增益来源不同的倍率。
        assert_eq!(
            generate(&gains, &bands, &interval, 2, false, &mut cursor, &mut out),
            Err(NoiseError::TimeslotFactorMismatch {
                gains: 1,
                requested: 2
            })
        );
        // 包络数不符。
        let two = fixfix(2);
        assert_eq!(
            generate(&gains, &bands, &two, 1, false, &mut cursor, &mut out),
            Err(NoiseError::EnvelopeCountMismatch {
                interval: 2,
                gains: 1
            })
        );
        // 空频带表。
        let empty_bands = AspxBandTables::empty();
        assert_eq!(
            generate(
                &gains,
                &empty_bands,
                &interval,
                1,
                false,
                &mut cursor,
                &mut out
            ),
            Err(NoiseError::SubbandOutOfRange {
                sbx: 0,
                num_sb_aspx: 0
            })
        );
        // 输出长度不符。
        let mut short = vec![QmfSlot::zero(); 4];
        let short_snapshot = short.clone();
        assert_eq!(
            generate(&gains, &bands, &interval, 1, false, &mut cursor, &mut short),
            Err(NoiseError::OutputLengthMismatch {
                expected: usize::from(SLOTS),
                provided: 4
            })
        );
        assert_eq!(short, short_snapshot, "长度错误不应改写实际传入的输出");
        // 空区间。
        let empty = AspxInterval::empty();
        assert_eq!(
            generate(&gains, &bands, &empty, 1, false, &mut cursor, &mut out),
            Err(NoiseError::EnvelopeCountMismatch {
                interval: 0,
                gains: 1
            })
        );

        assert_eq!(out, snapshot, "被拒绝的输入不应改写输出");
        assert_eq!(cursor, cursor_snapshot, "被拒绝的输入不应推进游标");
    }
}
