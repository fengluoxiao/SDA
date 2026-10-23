//! WCC 与 SEC 的合并（`TS103190-1:v1.4.1:5.7.6.5.3`）。
//!
//! A-SPX 的最后一步：把延迟后的 `Q_in,ASPX` 与 `5.7.6.4.5` 组装出的 `Y` 合成
//! `Q_out,ASPX`，交给下游 QMF 域处理（`5.7.6.1` 图 6、A-CPL 图 7）。
//!
//! # 公式遍历全部子带，不只是高带
//!
//! ```text
//! Re{Qout(m,i)} = Re{Qin(m, i − δ)} + Re{Y(m,i)}
//! Im{Qout(m,i)} = Im{Qin(m, i − δ)} + Im{Y(m,i)}
//! ```
//!
//! `m` 是全部 64 个 QMF 子带。低带之所以出现在输出里，正是靠这条加法——`Y`
//! 在 `sb < sbx` 处为零，于是低带等于延迟后的 `Q_in`。
//!
//! **因此这里需要一条独立的全子带延迟线，不能复用 `5.7.6.3.2` 的低带延迟。**
//! 那一步只搬 `sb < sbx`，把它的输出直接当作 `Q_out` 的低带，等于默认
//! `Q_in` 在 `sb ≥ sbx` 处为零。核心带的频谱确实只编到交叉频率，但 QMF 分析
//! 滤波器组有过渡带，交叉子带附近的泄漏不是严格零；按公式字面，那部分泄漏
//! 应当与 `Y` 相加后一并输出。
//!
//! # 延迟量与低带滤波同源
//!
//! `δ_ASPX = ts_offset_hfgen`，正文称其为「A-SPX 处理引入的总延迟」，与
//! `Pseudocode 75` 用的是同一个量，取值由表 192 给出的 3 或 6。两条延迟线的
//! 长度因此相同，但作用范围不同：低带那条喂高频生成器，这条喂输出。
//! 状态固定保留最近 6 个输入时隙，当次延迟为 3 时只读其后缀；这样合法帧长
//! 从短档切到长档时，下一帧仍能取得完整的 6 时隙历史。
//!
//! # 「两种交织都不存在」时走哪条分支，是判读
//!
//! `5.7.6.5.3` 只给了两个分支：频率交织**相加**，时间交织**替换**（丢弃
//! `Y`）。`aspx_fic_present` 与 `aspx_tic_present` 都为 0 时该用哪条，正文没
//! 有明写。
//!
//! 本实现取相加式。依据是相加式在 WCC 为零时退化成「延迟低带 + SEC」，而替
//! 换式会把 `Y` 整个丢掉——A-SPX 的全部产出都在 `Y` 里，那样高频就没有了，
//! 与本工具的存在意义矛盾。相加式还能让 `Q_in` 高带的过渡带泄漏一并保留。
//! 已登记在规范可追踪性第 7 节。
//!
//! # 时间交织的替换分支未实现
//!
//! `aspx_tic_present` 在实测材料上恒为 0（见测试向量策略 9.2），与
//! `aspx_balance` 同属不可达。替换分支本身只有两行，但**没有任何真实码流能
//! 验证它选对了时隙**——`aspx_tic_used_in_slot` 是逐时隙掩码，写出来只能靠
//! 自造夹具自证。故与交织编码整体一并推迟。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "子带下标以 64 为界，时隙范围由函数开头的前置检查给出"
)]

use crate::aspx::lowband::MAX_TS_OFFSET_HFGEN;
use crate::aspx::qmf::QmfSlot;
use crate::aspx::tables::NUM_QMF_SUBBANDS;

/// 子带数，`5.7.3.2` 规定恒为 64。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// `δ_ASPX` 的上界，与 `5.7.6.3.2` 的低带延迟同源（表 192 的 3 与 6）。
pub const MAX_ASPX_DELAY: usize = MAX_TS_OFFSET_HFGEN;

/// 合并无法执行的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterleaveError {
    /// `ts_offset_hfgen` 不是表 192 规定的 3 或 6。
    UnsupportedDelay { delay: u8 },
    /// `Q_in` 短于表 192 的最大延迟，无法保存完整的跨帧历史。
    InputTooShort { expected: usize, provided: usize },
    /// `Y` 的时隙数少于本帧的 QMF 时隙数。
    ///
    /// `Y` 允许更长——`5.7.6.4.5` 的区间右边界可以越过帧尾，多出的部分由
    /// `HfDelay` 携带到下一帧，不参与本帧输出。
    SpectralExtensionTooShort { expected: usize, provided: usize },
    /// 输出时隙数与 `Q_in` 不同。
    OutputLengthMismatch { expected: usize, provided: usize },
    /// 本帧时隙数为零。
    EmptyFrame,
}

/// `Q_in,ASPX` 的全子带延迟线。
///
/// 固定保存最近 [`MAX_ASPX_DELAY`] 个输入时隙。首帧之前没有信号，全零即等价
/// 于前置静音；延迟为 3 时读取该历史的最后 3 项。
#[derive(Debug, PartialEq)]
pub struct AspxOutputDelay {
    tail: [QmfSlot; MAX_ASPX_DELAY],
    filled: u8,
}

impl AspxOutputDelay {
    /// 建立空状态。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tail: [QmfSlot::zero(); MAX_ASPX_DELAY],
            filled: 0,
        }
    }

    /// 已保存的历史时隙数；首帧之前为 0。
    #[must_use]
    pub const fn history(&self) -> u8 {
        self.filled
    }
}

impl Default for AspxOutputDelay {
    fn default() -> Self {
        Self::new()
    }
}

/// `5.7.6.5.3`：合并延迟后的 `Q_in` 与 `Y`，产出 `Q_out`。
///
/// `q_in` 是本帧的 `Q_in,ASPX`，`out` 与它等长；`y` 是 `5.7.6.4.5` 的输出，第
/// 0 项对应时隙 0，允许比本帧长（越帧部分不参与本帧）。逐子带相加覆盖全部
/// 64 个子带，因此 `y` 在 A-SPX 范围之外必须已经是零。
///
/// # Errors
///
/// 见 [`InterleaveError`]。任一条不成立时都不改写 `out` 与 `delay`。
pub fn combine(
    q_in: &[QmfSlot],
    y: &[QmfSlot],
    ts_offset_hfgen: u8,
    delay: &mut AspxOutputDelay,
    out: &mut [QmfSlot],
) -> Result<(), InterleaveError> {
    if !matches!(ts_offset_hfgen, 3 | 6) {
        return Err(InterleaveError::UnsupportedDelay {
            delay: ts_offset_hfgen,
        });
    }
    let timeslots = q_in.len();
    if timeslots == 0 {
        return Err(InterleaveError::EmptyFrame);
    }
    if timeslots < MAX_ASPX_DELAY {
        return Err(InterleaveError::InputTooShort {
            expected: MAX_ASPX_DELAY,
            provided: timeslots,
        });
    }
    if y.len() < timeslots {
        return Err(InterleaveError::SpectralExtensionTooShort {
            expected: timeslots,
            provided: y.len(),
        });
    }
    if out.len() != timeslots {
        return Err(InterleaveError::OutputLengthMismatch {
            expected: timeslots,
            provided: out.len(),
        });
    }
    let offset = usize::from(ts_offset_hfgen);
    // 固定保存本帧最后 6 个时隙；下一帧取 3 时读后缀，取 6 时读全部。
    // 区间恒不短于 6，已由上面的 `InputTooShort` 检查钉住。
    let mut next_tail = [QmfSlot::zero(); MAX_ASPX_DELAY];
    let history_start = MAX_ASPX_DELAY - offset;
    next_tail.copy_from_slice(&q_in[timeslots - MAX_ASPX_DELAY..]);

    for (i, slot) in out.iter_mut().enumerate() {
        let delayed = if i < offset {
            &delay.tail[history_start + i]
        } else {
            &q_in[i - offset]
        };
        let sec = &y[i];
        for sb in 0..SUBBANDS {
            slot.re[sb] = delayed.re[sb] + sec.re[sb];
            slot.im[sb] = delayed.im[sb] + sec.im[sb];
        }
    }

    delay.tail = next_tail;
    delay.filled = u8::try_from(MAX_ASPX_DELAY).unwrap_or(u8::MAX);
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "下标由同一用例构造的时隙数与子带范围派生，越界即是该用例要报告的失败"
)]
mod tests {
    extern crate std;

    use super::*;
    use crate::aspx::tables::{num_qmf_timeslots, ts_offset_hfgen};
    use std::vec;
    use std::vec::Vec;

    /// 每个 (时隙, 子带) 一个互不相同的值，搬错一格就能认出来。
    fn tagged(count: usize, base: f32, scale: f32) -> Vec<QmfSlot> {
        let mut buf = vec![QmfSlot::zero(); count];
        for (ts, slot) in buf.iter_mut().enumerate() {
            for sb in 0..SUBBANDS {
                let tag = base + ((ts * 100 + sb) as f32) * scale;
                slot.re[sb] = tag;
                slot.im[sb] = -tag;
            }
        }
        buf
    }

    #[test]
    fn the_sum_covers_every_subband_not_just_the_aspx_range() {
        // `5.7.6.5.3` 的 m 遍历全部 64 个子带。低带出现在输出里靠的就是这条
        // 加法——把它限制在 A-SPX 范围内会让低带整个丢失。
        let slots = 16usize;
        let q_in = tagged(slots, 0.0, 1.0);
        let y = tagged(slots, 5.0, 0.25);
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = AspxOutputDelay::new();
        combine(&q_in, &y, 3, &mut delay, &mut out).expect("应能合并");

        for (i, slot) in out.iter().enumerate() {
            for sb in 0..SUBBANDS {
                // 前 3 个时隙的延迟项来自空历史，即零。
                let delayed = if i < 3 { 0.0 } else { q_in[i - 3].re[sb] };
                assert_eq!(slot.re[sb], delayed + y[i].re[sb], "时隙 {i} 子带 {sb}");
                let delayed_im = if i < 3 { 0.0 } else { q_in[i - 3].im[sb] };
                assert_eq!(slot.im[sb], delayed_im + y[i].im[sb]);
            }
        }
        // 反面：低带确实非零，否则「覆盖全子带」无从判断。
        assert_ne!(out[slots - 1].re[0], 0.0, "低带应有内容");
    }

    #[test]
    fn the_delay_carries_across_frames() {
        // 第二帧的前 offset 个时隙必须取自第一帧的尾部，而不是零或本帧数据。
        let slots = 16usize;
        let first = tagged(slots, 0.0, 1.0);
        let second = tagged(slots, 10_000.0, 1.0);
        let zero_y = vec![QmfSlot::zero(); slots];
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = AspxOutputDelay::new();
        combine(&first, &zero_y, 6, &mut delay, &mut out).expect("第一帧");
        assert_eq!(delay.history(), 6);
        combine(&second, &zero_y, 6, &mut delay, &mut out).expect("第二帧");

        for i in 0..6 {
            for sb in 0..SUBBANDS {
                assert_eq!(
                    out[i].re[sb],
                    first[slots - 6 + i].re[sb],
                    "第二帧时隙 {i} 应取自上一帧尾部"
                );
            }
        }
        for i in 6..slots {
            assert_eq!(out[i].re[0], second[i - 6].re[0], "其余取自本帧");
        }
    }

    #[test]
    fn a_short_delay_reads_the_latest_three_from_full_history() {
        // 历史固定保存 6 项；短延迟只读其后缀（`MAX_ASPX_DELAY − offset` 起）。
        //
        // **本判据要求 offset < MAX_ASPX_DELAY。** 取 6 时 `history_start` 为
        // 0，「从后缀读」与「从 0 读」完全同义，把下标写成 `tail[i]` 的注入
        // 一条判据都不会响——上一条跨帧判据正是这样漏掉它的。
        const OFFSET: u8 = 3;
        assert!(
            usize::from(OFFSET) < MAX_ASPX_DELAY,
            "本判据要求短延迟，否则前缀与后缀不可分辨"
        );
        let slots = 16usize;
        let first = tagged(slots, 0.0, 1.0);
        let second = tagged(slots, 10_000.0, 1.0);
        let zero_y = vec![QmfSlot::zero(); slots];
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = AspxOutputDelay::new();
        combine(&first, &zero_y, OFFSET, &mut delay, &mut out).expect("第一帧");
        assert_eq!(delay.history(), MAX_ASPX_DELAY as u8);
        for position in 0..MAX_ASPX_DELAY {
            for sb in 0..SUBBANDS {
                assert_eq!(
                    delay.tail[position].re[sb],
                    first[slots - MAX_ASPX_DELAY + position].re[sb],
                    "完整历史的时隙 {position} 子带 {sb}"
                );
            }
        }
        combine(&second, &zero_y, OFFSET, &mut delay, &mut out).expect("第二帧");

        for i in 0..usize::from(OFFSET) {
            for sb in 0..SUBBANDS {
                assert_eq!(
                    out[i].re[sb],
                    first[slots - usize::from(OFFSET) + i].re[sb],
                    "第二帧时隙 {i} 应取自上一帧尾部，而非 tail 的前缀"
                );
            }
        }
        assert_ne!(first[slots - 1].re[0], 0.0, "上一帧尾部必须非零");
    }

    #[test]
    fn changing_from_three_to_six_keeps_the_full_previous_tail() {
        // 表 192 的短档只延迟 3 个时隙，但状态仍须保存 6 个；否则下一帧切到
        // 长档时，前 3 个历史时隙会被错误地补零。
        const SHORT_FRAME: u16 = 512;
        const LONG_FRAME: u16 = 1536;
        let short_slots = usize::from(num_qmf_timeslots(SHORT_FRAME).expect("短帧时隙数"));
        let long_slots = usize::from(num_qmf_timeslots(LONG_FRAME).expect("长帧时隙数"));
        let short_offset = ts_offset_hfgen(SHORT_FRAME).expect("短帧延迟");
        let long_offset = ts_offset_hfgen(LONG_FRAME).expect("长帧延迟");
        assert_eq!((short_slots, short_offset), (8, 3));
        assert_eq!((long_slots, long_offset), (24, 6));

        let first = tagged(short_slots, 0.0, 1.0);
        let second = tagged(long_slots, 10_000.0, 1.0);
        let mut first_out = vec![QmfSlot::zero(); short_slots];
        let mut second_out = vec![QmfSlot::zero(); long_slots];
        let mut delay = AspxOutputDelay::new();
        combine(
            &first,
            &vec![QmfSlot::zero(); short_slots],
            short_offset,
            &mut delay,
            &mut first_out,
        )
        .expect("短帧");
        assert_eq!(delay.history(), MAX_ASPX_DELAY as u8);
        combine(
            &second,
            &vec![QmfSlot::zero(); long_slots],
            long_offset,
            &mut delay,
            &mut second_out,
        )
        .expect("切到长帧");

        for i in 0..usize::from(long_offset) {
            for sb in 0..SUBBANDS {
                assert_eq!(
                    second_out[i].re[sb],
                    first[short_slots - usize::from(long_offset) + i].re[sb],
                    "切档后的时隙 {i} 子带 {sb} 应取短帧完整尾部"
                );
            }
        }
    }

    #[test]
    fn the_first_frame_reads_silence_before_the_start() {
        // 首帧没有历史，前 offset 个时隙的延迟项是零；此时输出就等于 Y。
        let slots = 16usize;
        let q_in = tagged(slots, 1.0, 1.0);
        let y = tagged(slots, 7.0, 0.5);
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = AspxOutputDelay::new();
        assert_eq!(delay.history(), 0, "首帧之前没有历史");
        combine(&q_in, &y, 6, &mut delay, &mut out).expect("应能合并");
        for i in 0..6 {
            for sb in 0..SUBBANDS {
                assert_eq!(out[i].re[sb], y[i].re[sb], "首帧前 6 个时隙应只有 Y");
            }
        }
        assert_ne!(y[0].re[0], 0.0, "Y 必须非零，否则判据无鉴别力");
    }

    #[test]
    fn a_longer_spectral_extension_contributes_only_its_first_frame_worth() {
        // `5.7.6.4.5` 的 Y 可以越过帧尾，多出的部分由 HfDelay 携带到下一帧，
        // 不参与本帧输出。
        let slots = 16usize;
        let q_in = vec![QmfSlot::zero(); slots];
        let y = tagged(slots + 3, 0.0, 1.0);
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = AspxOutputDelay::new();
        combine(&q_in, &y, 3, &mut delay, &mut out).expect("应能合并");
        for i in 0..slots {
            assert_eq!(out[i].re[0], y[i].re[0]);
        }
        // 越帧那几项的值不得出现在本帧任何位置。
        for (extra, tail_slot) in y.iter().enumerate().skip(slots) {
            let value = tail_slot.re[0];
            assert!(
                out.iter().all(|slot| slot.re[0] != value),
                "越帧时隙 {extra} 的值不该进入本帧"
            );
        }
    }

    #[test]
    fn rejected_input_leaves_the_output_and_delay_untouched() {
        let slots = 16usize;
        let q_in = tagged(slots, 1.0, 1.0);
        let y = vec![QmfSlot::zero(); slots];
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = AspxOutputDelay::new();
        combine(&q_in, &y, 3, &mut delay, &mut out).expect("哨兵结果");
        let snapshot = out.clone();
        let history = delay.history();
        let tail = delay.tail;
        assert!(
            tail.iter()
                .any(|slot| slot.re.iter().any(|&sample| sample != 0.0)),
            "延迟快照必须非零，否则清空缺陷无从判断"
        );

        for bad in [0u8, 1, 2, 4, 5, 7] {
            assert_eq!(
                combine(&q_in, &y, bad, &mut delay, &mut out),
                Err(InterleaveError::UnsupportedDelay { delay: bad })
            );
        }
        let short_y = vec![QmfSlot::zero(); slots - 1];
        assert_eq!(
            combine(&q_in, &short_y, 3, &mut delay, &mut out),
            Err(InterleaveError::SpectralExtensionTooShort {
                expected: slots,
                provided: slots - 1
            })
        );
        let mut short_out = vec![QmfSlot::zero(); 4];
        assert_eq!(
            combine(&q_in, &y, 3, &mut delay, &mut short_out),
            Err(InterleaveError::OutputLengthMismatch {
                expected: slots,
                provided: 4
            })
        );
        assert_eq!(
            combine(&[], &y, 3, &mut delay, &mut []),
            Err(InterleaveError::EmptyFrame)
        );
        let short_q_in = vec![QmfSlot::zero(); MAX_ASPX_DELAY - 1];
        let mut matching_out = vec![QmfSlot::zero(); short_q_in.len()];
        assert_eq!(
            combine(&short_q_in, &y, 3, &mut delay, &mut matching_out),
            Err(InterleaveError::InputTooShort {
                expected: MAX_ASPX_DELAY,
                provided: MAX_ASPX_DELAY - 1
            })
        );

        assert_eq!(out, snapshot, "被拒绝的输入不应改写输出");
        assert_eq!(delay.history(), history, "被拒绝的输入不应改写延迟");
        assert_eq!(delay.tail, tail, "被拒绝的输入不应改写延迟内容");
    }
}
