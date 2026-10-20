//! 低带滤波与 QMF 延迟线（`TS103190-1:v1.4.1:5.7.6.3.2`）。
//!
//! `Pseudocode 75` 做两件事：把分析滤波器组的 `Q_in,ASPX` 在交叉子带 `sbx`
//! 处截断，并整体延迟 `ts_offset_hfgen` 个 QMF 时隙后交给高频生成器。延迟量
//! 取自表 192：`frame_length ≥ 1 536` 为 6 个时隙，其余为 3 个。
//!
//! 延迟的前 `ts_offset_hfgen` 个时隙来自**上一个 A-SPX 区间的尾部**，因此这
//! 里持有跨区间状态。固定保留表 192 上限的最后 6 个时隙，当次延迟为 3 时取
//! 其后缀；这样两档合法帧长切换也不会把历史的前半段错当成末尾。表 189 的八档
//! `num_qmf_timeslots` 下限恰为 6，所以「一次填满」对合法输入永远成立。
//!
//! 截断只是「不写」而非「写零」：`sb ≥ sbx` 的子带留给高频生成器填，调用方
//! 传进来的输出缓冲本就应是它要填的那一份。本模块因此不碰那些子带，由
//! [`LowBandError::OutputNotCleared`] 在调试期挡住「拿了脏缓冲进来」。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "下标由 64 个子带、表 192 的两档延迟与已核对过的时隙数派生；\
              `Pseudocode 75` 的三段偏移用显式下标比迭代器更贴近原文"
)]

use crate::aspx::qmf::QmfSlot;
use crate::aspx::tables::NUM_QMF_SUBBANDS;

/// 表 192 允许的最大延迟，即 `frame_length ≥ 1 536` 那一档。
pub const MAX_TS_OFFSET_HFGEN: usize = 6;

/// 子带数，`5.7.3.2` 规定恒为 64。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// 跨 A-SPX 区间的延迟线状态。
///
/// 保存最近最多 [`MAX_TS_OFFSET_HFGEN`] 个输入时隙。首个区间之前没有信号，
/// 全零即等价于前置静音；延迟为 3 时读取该历史的最后 3 项。
#[derive(Debug, PartialEq)]
pub struct LowBandDelay {
    tail: [QmfSlot; MAX_TS_OFFSET_HFGEN],
    /// 已写入的真实历史时隙数，不含前置静音。
    filled: u8,
}

impl LowBandDelay {
    /// 建立空状态。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tail: [QmfSlot::zero(); MAX_TS_OFFSET_HFGEN],
            filled: 0,
        }
    }

    /// 已保存的历史时隙数；首个区间之前为 0。
    #[must_use]
    pub const fn history(&self) -> u8 {
        self.filled
    }
}

impl Default for LowBandDelay {
    fn default() -> Self {
        Self::new()
    }
}

/// 低带滤波无法执行的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowBandError {
    /// `ts_offset_hfgen` 不是表 192 规定的 3 或 6。
    UnsupportedOffset { offset: usize },
    /// 交叉子带超出 64 个 QMF 子带。
    CrossoverOutOfRange { sbx: usize },
    /// 输出容量不等于 `num_qmf_timeslots + ts_offset_hfgen`。
    OutputLengthMismatch { expected: usize, provided: usize },
    /// 区间短于表 192 的最大延迟，滚动历史无法一次填满。
    ///
    /// 表 189 的八档 `num_qmf_timeslots` 是 32、30、24、16、15、12、8、6，
    /// 下限恰为 6，与 [`MAX_TS_OFFSET_HFGEN`] 相等。因此合法输入永远够长，
    /// 这条只挡住调用方切错了片；界限是紧的，不是随手取的余量。
    IntervalTooShort { timeslots: usize },
    /// 输出缓冲在 `sb < sbx` 的位置带有残留。
    ///
    /// 本步只写低带；若调用方递进来的缓冲那里非零，说明它复用了上一次的结果，
    /// 而高频生成器随后只会覆盖 `sb ≥ sbx`，残留会一路混进输出。
    OutputNotCleared { timeslot: usize, subband: usize },
}

/// `Pseudocode 75` 的低带滤波与延迟。
///
/// `current` 是本区间的 `Q_in,ASPX`，`out` 的长度必须是
/// `current.len() + offset`。只写 `sb < sbx` 的子带，其余留给高频生成器。
///
/// # Errors
///
/// 见 [`LowBandError`]。任一条不成立时都不改写状态。
pub fn low_band(
    current: &[QmfSlot],
    state: &mut LowBandDelay,
    sbx: u8,
    offset: u8,
    out: &mut [QmfSlot],
) -> Result<(), LowBandError> {
    if !matches!(offset, 3 | 6) {
        return Err(LowBandError::UnsupportedOffset {
            offset: usize::from(offset),
        });
    }
    let offset = usize::from(offset);
    let sbx = usize::from(sbx);
    if sbx > SUBBANDS {
        return Err(LowBandError::CrossoverOutOfRange { sbx });
    }
    if current.len() < MAX_TS_OFFSET_HFGEN {
        return Err(LowBandError::IntervalTooShort {
            timeslots: current.len(),
        });
    }
    let expected = current.len().saturating_add(offset);
    if out.len() != expected {
        return Err(LowBandError::OutputLengthMismatch {
            expected,
            provided: out.len(),
        });
    }
    for (timeslot, slot) in out.iter().enumerate() {
        for subband in 0..sbx {
            if slot.re[subband] != 0.0 || slot.im[subband] != 0.0 {
                return Err(LowBandError::OutputNotCleared { timeslot, subband });
            }
        }
    }

    // 前 offset 个时隙取上一区间的尾部；首个区间没有历史，留零。
    let history_start = MAX_TS_OFFSET_HFGEN.saturating_sub(offset);
    for timeslot in 0..offset {
        let source = &state.tail[history_start + timeslot];
        let target = &mut out[timeslot];
        for subband in 0..sbx {
            target.re[subband] = source.re[subband];
            target.im[subband] = source.im[subband];
        }
    }
    // 其余是本区间整体后移 offset。
    for (index, source) in current.iter().enumerate() {
        let target = &mut out[offset + index];
        for subband in 0..sbx {
            target.re[subband] = source.re[subband];
            target.im[subband] = source.im[subband];
        }
    }

    // 保存本区间最后 6 个时隙；下一次取 3 时读后缀，取 6 时读全部。
    // 区间恒不短于 6（见 `IntervalTooShort`），故不需要「攒不满就滚动」的分支
    // ——那条对任何合法输入都不可达，注入实测也无人报警。
    for position in 0..MAX_TS_OFFSET_HFGEN {
        state.tail[position] = current[current.len() - MAX_TS_OFFSET_HFGEN + position];
    }
    state.filled = u8::try_from(MAX_TS_OFFSET_HFGEN).unwrap_or(u8::MAX);
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "下标由同一用例构造的时隙数与交叉子带派生，越界即是该用例要报告的失败"
)]
mod tests {
    extern crate std;

    use super::*;
    use crate::aspx::tables::{num_qmf_timeslots, ts_offset_hfgen};
    use std::vec;
    use std::vec::Vec;

    /// 把「全局时隙号 + 子带号」编成可辨认的样本值。
    ///
    /// 每个 `(sb, ts)` 的值互不相同，因此搬错一格就能从数值上看出来，
    /// 而不是只知道「不相等」。
    fn marker(global_timeslot: usize, subband: usize) -> f32 {
        (global_timeslot * 100 + subband) as f32
    }

    fn interval(start: usize, timeslots: usize) -> Vec<QmfSlot> {
        (0..timeslots)
            .map(|index| {
                let mut slot = QmfSlot::zero();
                for subband in 0..SUBBANDS {
                    slot.re[subband] = marker(start + index, subband);
                    slot.im[subband] = -marker(start + index, subband);
                }
                slot
            })
            .collect()
    }

    /// 表 192 的两档延迟都必须能派生出来。
    #[test]
    fn table_192_offsets_are_three_or_six() {
        for (frame_length, expected) in [
            (2048u16, 6u8),
            (1920, 6),
            (1536, 6),
            (1024, 3),
            (960, 3),
            (768, 3),
            (512, 3),
            (384, 3),
        ] {
            assert_eq!(ts_offset_hfgen(frame_length), Some(expected));
            assert!(usize::from(expected) <= MAX_TS_OFFSET_HFGEN);
        }
    }

    /// 连续区间拼起来后，低带恰好是输入整体延迟 `ts_offset_hfgen` 的副本。
    ///
    /// 这是本模块最强的判据：它同时钉住延迟量、跨区间尾部的取用位置，以及区
    /// 间边界处不丢不重。样本值编了全局时隙号，因此搬错一格会给出具体是哪一
    /// 格，而不是只报「不相等」。
    #[test]
    fn consecutive_intervals_form_one_delayed_copy() {
        const FRAME: u16 = 2048;
        let timeslots = usize::from(num_qmf_timeslots(FRAME).expect("时隙数"));
        let offset = ts_offset_hfgen(FRAME).expect("延迟");
        let sbx = 36u8;

        let mut state = LowBandDelay::new();
        assert_eq!(state.history(), 0, "首个区间之前没有历史");

        for round in 0..4usize {
            let current = interval(round * timeslots, timeslots);
            let mut out = vec![QmfSlot::zero(); timeslots + usize::from(offset)];
            low_band(&current, &mut state, sbx, offset, &mut out).expect("低带滤波应成功");

            for (index, slot) in out.iter().enumerate() {
                // out 的第 index 个时隙对应全局时隙 round*timeslots + index − offset。
                let global = (round * timeslots + index) as isize - isize::from(offset);
                for subband in 0..usize::from(sbx) {
                    let expected = if global < 0 {
                        // 首个区间之前是静音。
                        0.0
                    } else {
                        marker(global as usize, subband)
                    };
                    assert_eq!(
                        slot.re[subband], expected,
                        "第 {round} 区间、时隙 {index}、子带 {subband} 的实部"
                    );
                    assert_eq!(slot.im[subband], -expected, "虚部应是实部取负");
                }
            }
            assert_eq!(state.history(), offset, "每个区间之后都应存满尾部");
        }
    }

    /// 表 192 的两档延迟切换时，必须始终取最近的历史时隙。
    #[test]
    fn changing_between_table_192_offsets_uses_the_latest_history() {
        const LONG_FRAME: u16 = 1536;
        const SHORT_FRAME: u16 = 512;
        let long_timeslots = usize::from(num_qmf_timeslots(LONG_FRAME).expect("长帧时隙数"));
        let short_timeslots = usize::from(num_qmf_timeslots(SHORT_FRAME).expect("短帧时隙数"));
        let long_offset = ts_offset_hfgen(LONG_FRAME).expect("长帧延迟");
        let short_offset = ts_offset_hfgen(SHORT_FRAME).expect("短帧延迟");
        let sbx = 4u8;
        let mut state = LowBandDelay::new();

        let first = interval(0, long_timeslots);
        let mut first_out = vec![QmfSlot::zero(); long_timeslots + usize::from(long_offset)];
        low_band(&first, &mut state, sbx, long_offset, &mut first_out).expect("首个长帧应成功");

        let second = interval(long_timeslots, short_timeslots);
        let mut second_out = vec![QmfSlot::zero(); short_timeslots + usize::from(short_offset)];
        low_band(&second, &mut state, sbx, short_offset, &mut second_out).expect("切到短帧应成功");
        for index in 0..usize::from(short_offset) {
            for subband in 0..usize::from(sbx) {
                assert_eq!(
                    second_out[index].re[subband],
                    marker(long_timeslots - usize::from(short_offset) + index, subband),
                    "6 → 3 后仍应取长帧最后 3 个时隙"
                );
            }
        }

        let third_start = long_timeslots + short_timeslots;
        let third = interval(third_start, long_timeslots);
        let mut third_out = vec![QmfSlot::zero(); long_timeslots + usize::from(long_offset)];
        low_band(&third, &mut state, sbx, long_offset, &mut third_out).expect("切回长帧应成功");
        for index in 0..usize::from(long_offset) {
            for subband in 0..usize::from(sbx) {
                assert_eq!(
                    third_out[index].re[subband],
                    marker(third_start - usize::from(long_offset) + index, subband),
                    "3 → 6 后应恢复短帧最后 6 个时隙"
                );
            }
        }
        assert_eq!(
            state.history(),
            u8::try_from(MAX_TS_OFFSET_HFGEN).expect("最大延迟可表示为 u8")
        );
    }

    /// 交叉子带以上一律不碰，留给高频生成器。
    #[test]
    fn subbands_at_and_above_the_crossover_are_left_untouched() {
        let timeslots = 8usize;
        let offset = 3u8;
        let sbx = 20u8;
        let current = interval(0, timeslots);
        let mut state = LowBandDelay::new();
        let mut out = vec![QmfSlot::zero(); timeslots + usize::from(offset)];
        // 高频侧预置一个哨兵，低带滤波不得改动它。
        for slot in &mut out {
            for subband in usize::from(sbx)..SUBBANDS {
                slot.re[subband] = 7.5;
                slot.im[subband] = -7.5;
            }
        }

        low_band(&current, &mut state, sbx, offset, &mut out).expect("低带滤波应成功");
        for (index, slot) in out.iter().enumerate() {
            for subband in usize::from(sbx)..SUBBANDS {
                assert_eq!(slot.re[subband], 7.5, "时隙 {index} 子带 {subband} 被改动");
                assert_eq!(slot.im[subband], -7.5);
            }
        }
    }

    /// `sbx = 0` 时什么都不写，但状态照样推进。
    ///
    /// 交叉子带为 0 意味着整帧都交给高频生成；延迟线仍要记住尾部，否则下一个
    /// `sbx > 0` 的区间会从零开始。
    #[test]
    fn a_zero_crossover_writes_nothing_but_still_advances_state() {
        let timeslots = 8usize;
        let offset = 3u8;
        let current = interval(0, timeslots);
        let mut state = LowBandDelay::new();
        let mut out = vec![QmfSlot::zero(); timeslots + usize::from(offset)];
        low_band(&current, &mut state, 0, offset, &mut out).expect("sbx = 0 应合法");
        assert!(
            out.iter()
                .all(|slot| slot.re.iter().chain(slot.im.iter()).all(|&v| v == 0.0)),
            "sbx = 0 时不应写入任何子带"
        );
        assert_eq!(
            state.history(),
            u8::try_from(MAX_TS_OFFSET_HFGEN).expect("最大延迟可表示为 u8"),
            "状态仍须推进并保留两档延迟所需的历史"
        );

        let mut next = vec![QmfSlot::zero(); timeslots + usize::from(offset)];
        low_band(
            &interval(timeslots, timeslots),
            &mut state,
            4,
            offset,
            &mut next,
        )
        .expect("下一区间应成功");
        for index in 0..usize::from(offset) {
            for subband in 0..4usize {
                assert_eq!(
                    next[index].re[subband],
                    marker(timeslots - usize::from(offset) + index, subband),
                    "前 {offset} 个时隙应取自上一区间的尾部"
                );
            }
        }
    }

    /// 非法输入一律拒绝，且不推进状态。
    #[test]
    fn invalid_input_is_rejected_without_touching_state() {
        let current = interval(0, 8);

        for offset in [0u8, 1, 2, 4, 5, 7, u8::MAX] {
            let mut state = LowBandDelay::new();
            let mut out = vec![QmfSlot::zero(); current.len().saturating_add(usize::from(offset))];
            assert_eq!(
                low_band(&current, &mut state, 36, offset, &mut out),
                Err(LowBandError::UnsupportedOffset {
                    offset: usize::from(offset)
                }),
                "表 192 只允许延迟 3 或 6"
            );
            assert_eq!(state.history(), 0, "拒绝不应推进状态");
        }

        // 表 189 的下限恰为 6（frame_length 384），因此 5 个时隙不是合法区间。
        let short_interval = interval(0, 5);
        let mut state = LowBandDelay::new();
        let mut out = vec![QmfSlot::zero(); 8];
        assert_eq!(
            low_band(&short_interval, &mut state, 4, 3, &mut out),
            Err(LowBandError::IntervalTooShort { timeslots: 5 })
        );
        assert_eq!(state.history(), 0, "拒绝不应推进状态");

        let mut state = LowBandDelay::new();
        let mut out = vec![QmfSlot::zero(); 11];
        assert_eq!(
            low_band(&current, &mut state, 65, 3, &mut out),
            Err(LowBandError::CrossoverOutOfRange { sbx: 65 })
        );
        let mut short = vec![QmfSlot::zero(); 10];
        assert_eq!(
            low_band(&current, &mut state, 36, 3, &mut short),
            Err(LowBandError::OutputLengthMismatch {
                expected: 11,
                provided: 10
            })
        );
        out[2].re[5] = 1.0;
        assert_eq!(
            low_band(&current, &mut state, 36, 3, &mut out),
            Err(LowBandError::OutputNotCleared {
                timeslot: 2,
                subband: 5
            })
        );
        assert_eq!(state.history(), 0, "拒绝不应推进状态");
    }
}
