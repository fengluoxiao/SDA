//! 一条 A-SPX 声道的中间缓冲。
//!
//! 编排 `5.7.6` 那条通路要在各工具之间递七块 QMF 域缓冲。`no_std` 下不能按帧
//! 长动态分配，因此按合法配置的上界一次性留足，由调用方持有。这与
//! [`AspxChannelState`](crate::aspx::state::AspxChannelState) 分工：那边是**跨
//! 帧**的历史，这边是**帧内**的中转，两者的生命周期完全不同。工作区不携带
//! 信息，但并非每块都由生产工具完整覆写；每帧使用前必须先调用
//! [`AspxWorkspace::prepare_frame`]，见下文。
//!
//! # 两个上界都是推出来的
//!
//! **[`MAX_QMF_TIMESLOTS`] = 32**：表 189 的八档 `num_qmf_timeslots` 是 32、
//! 30、24、16、15、12、8、6，最大一档对应 `frame_len_base = 2048`。
//!
//! **[`MAX_EXTENDED_TIMESLOTS`] = 38**：两条独立的路径给出同一个数，取其大者
//! 即可，而它们恰好相等。
//!
//! - 低带滤波（`5.7.6.3.2`）把本帧整体后移 `ts_offset_hfgen`，输出长
//!   `num_qmf_timeslots + ts_offset_hfgen`；表 192 的该字段至多 6，故上界 38。
//! - A-SPX 区间的右边界可越过帧尾 `aspx_var_bord_right · num_ts_in_ats`
//!   个时隙。`aspx_var_bord_right` 是 2 比特（`4.3.10.4.5`，语法表 53），倍率
//!   至多 2，故越帧至多 6，组装输出 `Y` 的上界同样是 38。
//!
//! 两者相等是巧合而非同一条约束：前者来自滤波器延迟，后者来自成帧语法。写成
//! 一个常量是因为缓冲可以共用上界，不是因为它们是同一个量。
//!
//! # 这个结构很大，不要放栈上
//!
//! 七块缓冲共 127 KiB（[`QmfSlot`] 是 64 个子带的实虚两组 `f32`，512 字节）。
//! `no_std` 目标上应当作静态或调用方自持的长生命周期对象，不要在每帧的调用里
//! 按值构造。[`AspxWorkspace::new`] 是 `const fn`，可以直接落在 `static` 里。
//!
//! [`AspxWorkspace::clear`] 与 [`AspxWorkspace::prepare_frame`] 都逐字段原地清零，
//! 不会为整个工作区建立栈临时对象。

use crate::aspx::qmf::QmfSlot;

/// 表 189 最大一档的 `num_qmf_timeslots`。
pub const MAX_QMF_TIMESLOTS: usize = 32;

/// 带上低带延迟或越帧部分之后的时隙上界，见模块文档。
pub const MAX_EXTENDED_TIMESLOTS: usize = 38;

/// 一条声道在一帧内的全部 QMF 域中转缓冲。
///
/// 每块的前若干项有效，有效长度由当帧的成帧参数决定；各工具的入口都会核对
/// 传入切片的长度，因此这里只负责提供足够大的存储。
#[derive(Debug)]
pub struct AspxWorkspace {
    /// `Q_in,ASPX`：QMF 分析的输出，也是 `5.7.6.5.3` 合并的被延迟项。
    pub q_in: [QmfSlot; MAX_QMF_TIMESLOTS],
    /// `Q_low`：`5.7.6.3.2` 低带滤波并延迟后的结果，喂高频生成器。
    pub q_low: [QmfSlot; MAX_EXTENDED_TIMESLOTS],
    /// `Q_high`：`5.7.6.3.3` 高频生成的估计谱。
    pub q_high: [QmfSlot; MAX_EXTENDED_TIMESLOTS],
    /// `qmf_noise`：`5.7.6.4.3` 的噪声。
    pub noise: [QmfSlot; MAX_EXTENDED_TIMESLOTS],
    /// `qmf_sine`：`5.7.6.4.4` 的音调。
    pub sine: [QmfSlot; MAX_EXTENDED_TIMESLOTS],
    /// `Y`：`5.7.6.4.5` 组装出的高频信号，可越过帧尾。
    pub y: [QmfSlot; MAX_EXTENDED_TIMESLOTS],
    /// `Q_out,ASPX`：`5.7.6.5.3` 合并的输出，随后交给 A-JOC 等 QMF 域工具；
    /// 只有链路终点才交给 QMF 合成。
    pub q_out: [QmfSlot; MAX_QMF_TIMESLOTS],
}

impl AspxWorkspace {
    /// 全零工作区。`const` 以便直接落在 `static` 里，见模块文档。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            q_in: [QmfSlot::zero(); MAX_QMF_TIMESLOTS],
            q_low: [QmfSlot::zero(); MAX_EXTENDED_TIMESLOTS],
            q_high: [QmfSlot::zero(); MAX_EXTENDED_TIMESLOTS],
            noise: [QmfSlot::zero(); MAX_EXTENDED_TIMESLOTS],
            sine: [QmfSlot::zero(); MAX_EXTENDED_TIMESLOTS],
            y: [QmfSlot::zero(); MAX_EXTENDED_TIMESLOTS],
            q_out: [QmfSlot::zero(); MAX_QMF_TIMESLOTS],
        }
    }

    /// 为新一帧清理不会由生产工具完整覆写的缓冲。
    ///
    /// 每帧进入 A-SPX 通路前必须调用一次，包括新建工作区后的首帧。`q_low` 要
    /// 满足低带滤波的全零前置条件；`y` 在 A-SPX 范围外必须为零，因为最后的
    /// 输出合并会读取全部 64 个子带。其余五块会在各自的有效切片内完整覆写，
    /// 无需逐帧清理。
    pub fn prepare_frame(&mut self) {
        self.q_low.fill(QmfSlot::zero());
        self.y.fill(QmfSlot::zero());
    }

    /// 原地把全部七块缓冲清零。
    ///
    /// 用于初始化之外的诊断与显式擦除。正常逐帧编排使用 [`Self::prepare_frame`]
    /// 即可，避免清理随后必然完整覆写的缓冲。
    pub fn clear(&mut self) {
        self.q_in.fill(QmfSlot::zero());
        self.q_high.fill(QmfSlot::zero());
        self.noise.fill(QmfSlot::zero());
        self.sine.fill(QmfSlot::zero());
        self.q_out.fill(QmfSlot::zero());
        self.prepare_frame();
    }
}

impl Default for AspxWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "下标是固定的容量常量与 63，越界即是该用例要报告的失败"
)]
mod tests {
    use super::*;
    use crate::aspx::lowband::{LowBandDelay, MAX_TS_OFFSET_HFGEN, low_band};
    use crate::aspx::tables::{NUM_QMF_SUBBANDS, num_qmf_timeslots};

    /// 表 189 的八个合法帧长，与 `tables.rs` 的同名表各写一份。
    const FRAME_LEN_BASE: [u16; 8] = [2048, 1920, 1536, 1024, 960, 768, 512, 384];

    #[test]
    fn the_plain_bound_covers_every_row_of_table_189() {
        // 期望值不引用 MAX_QMF_TIMESLOTS，改用字面量另写一份：引用实现自己的
        // 常量会让判据随实现一起改，永远成立。
        const EXPECTED: usize = 32;
        let mut largest = 0usize;
        for base in FRAME_LEN_BASE {
            let slots = usize::from(num_qmf_timeslots(base).expect("合法帧长应有时隙数"));
            assert!(slots <= EXPECTED, "帧长 {base} 的 {slots} 超出上界");
            largest = largest.max(slots);
        }
        assert_eq!(largest, EXPECTED, "上界必须是紧的，不是随手取的余量");
        assert_eq!(MAX_QMF_TIMESLOTS, EXPECTED);
    }

    #[test]
    fn the_extended_bound_is_reached_by_both_paths() {
        // 38 由两条**互相独立**的路径给出，都必须恰好达到：
        //   低带滤波   = num_qmf_timeslots + ts_offset_hfgen
        //   组装的 Y   = num_qmf_timeslots + var_bord_right · num_ts_in_ats
        // 只验其中一条会让另一条的上界失去保护。
        const EXPECTED: usize = 38;
        const MAX_VAR_BORD_RIGHT: usize = 3; // 2 比特，见 4.3.10.4.5 与语法表 53
        const MAX_TS_IN_ATS: usize = 2; // 表 192

        let longest = FRAME_LEN_BASE
            .iter()
            .map(|&b| usize::from(num_qmf_timeslots(b).expect("合法帧长")))
            .max()
            .expect("表非空");
        assert_eq!(
            longest + MAX_TS_OFFSET_HFGEN,
            EXPECTED,
            "低带路径应恰好触及"
        );
        assert_eq!(
            longest + MAX_VAR_BORD_RIGHT * MAX_TS_IN_ATS,
            EXPECTED,
            "组装路径应恰好触及"
        );
        assert_eq!(MAX_EXTENDED_TIMESLOTS, EXPECTED);
    }

    #[test]
    fn every_buffer_is_long_enough_for_its_own_role() {
        let ws = AspxWorkspace::new();
        // q_in 与 q_out 按帧长；其余五块可能带上延迟或越帧部分。
        assert!(ws.q_in.len() >= MAX_QMF_TIMESLOTS);
        assert!(ws.q_out.len() >= MAX_QMF_TIMESLOTS);
        for (name, len) in [
            ("q_low", ws.q_low.len()),
            ("q_high", ws.q_high.len()),
            ("noise", ws.noise.len()),
            ("sine", ws.sine.len()),
            ("y", ws.y.len()),
        ] {
            assert!(
                len >= MAX_EXTENDED_TIMESLOTS,
                "{name} 只有 {len} 个时隙，不足以容纳延迟或越帧部分"
            );
        }
    }

    fn dirty(slots: &mut [QmfSlot], marker: f32) {
        let subbands = usize::from(NUM_QMF_SUBBANDS);
        for slot in slots {
            for subband in 0..subbands {
                slot.re[subband] = marker;
                slot.im[subband] = marker;
            }
        }
    }

    fn assert_zero(name: &str, slots: &[QmfSlot]) {
        let subbands = usize::from(NUM_QMF_SUBBANDS);
        for (timeslot, slot) in slots.iter().enumerate() {
            for subband in 0..subbands {
                assert_eq!(slot.re[subband], 0.0, "{name}[{timeslot}].re[{subband}]");
                assert_eq!(slot.im[subband], 0.0, "{name}[{timeslot}].im[{subband}]");
            }
        }
    }

    fn dirty_every_buffer(ws: &mut AspxWorkspace) {
        dirty(&mut ws.q_in, 1.0);
        dirty(&mut ws.q_low, 2.0);
        dirty(&mut ws.q_high, 3.0);
        dirty(&mut ws.noise, 4.0);
        dirty(&mut ws.sine, 5.0);
        dirty(&mut ws.y, 6.0);
        dirty(&mut ws.q_out, 7.0);
    }

    #[test]
    fn prepare_frame_clears_only_buffers_with_zero_fill_preconditions() {
        let mut ws = AspxWorkspace::new();
        dirty_every_buffer(&mut ws);

        ws.prepare_frame();

        assert_zero("q_low", &ws.q_low);
        assert_zero("y", &ws.y);
        for (name, slots) in [
            ("q_in", ws.q_in.as_slice()),
            ("q_high", ws.q_high.as_slice()),
            ("noise", ws.noise.as_slice()),
            ("sine", ws.sine.as_slice()),
            ("q_out", ws.q_out.as_slice()),
        ] {
            assert_ne!(slots[0], QmfSlot::zero(), "{name} 不应由逐帧准备清理");
        }
    }

    #[test]
    fn prepare_frame_allows_low_band_to_reuse_its_buffer() {
        let mut ws = AspxWorkspace::new();
        let mut delay = LowBandDelay::new();
        dirty(&mut ws.q_in, 1.0);

        ws.prepare_frame();
        low_band(
            &ws.q_in[..MAX_QMF_TIMESLOTS],
            &mut delay,
            10,
            6,
            &mut ws.q_low[..MAX_EXTENDED_TIMESLOTS],
        )
        .expect("首帧低带滤波");
        assert_ne!(ws.q_low[6].re[0], 0.0, "首帧应弄脏复用缓冲");

        ws.prepare_frame();
        low_band(
            &ws.q_in[..MAX_QMF_TIMESLOTS],
            &mut delay,
            10,
            6,
            &mut ws.q_low[..MAX_EXTENDED_TIMESLOTS],
        )
        .expect("逐帧准备后应能再次使用低带缓冲");
    }

    #[test]
    fn clear_returns_every_buffer_to_zero() {
        let mut ws = AspxWorkspace::new();
        dirty_every_buffer(&mut ws);

        ws.clear();
        for (name, slots) in [
            ("q_in", ws.q_in.as_slice()),
            ("q_low", ws.q_low.as_slice()),
            ("q_high", ws.q_high.as_slice()),
            ("noise", ws.noise.as_slice()),
            ("sine", ws.sine.as_slice()),
            ("y", ws.y.as_slice()),
            ("q_out", ws.q_out.as_slice()),
        ] {
            assert_zero(name, slots);
        }
    }
}
