//! 一条 A-SPX 声道的全部跨帧状态。
//!
//! `5.7.6` 的各工具各自持有跨帧状态，且生命周期规则并不一致。编排一条通路时
//! 它们必须一起创建、一起重置，否则某一个漏掉就会在 seek 之后串入上一段的
//! 信号——而那种错误不会报错，只会让输出听上去"差一点"。
//!
//! # 十一个状态，五类生命周期
//!
//! - **滤波器组历史**：[`QmfAnalysisState`]、[`QmfSynthesisState`]。`5.7.3` 的
//!   分析与 `5.7.4` 的合成各自持有窗口长度的样本历史；停在 `Q_out,ASPX` 的
//!   QMF 域入口只推进前者，终端 PCM 包装器才推进后者。
//! - **包络历史**：[`EnvelopeHistory`]（`5.7.6.3.4`）。时间方向差分会引用上一
//!   区间最后一个信号与噪声包络，必须按声道延续。
//! - **延迟线**：[`LowBandDelay`]（`5.7.6.3.2`，喂高频生成器）、
//!   [`AspxOutputDelay`]（`5.7.6.5.3`，喂输出）、[`HfDelay`]（`5.7.6.4.5`，越过
//!   帧尾的已组装信号）。三条长度不同、作用范围不同，见各自模块。
//! - **预测器历史**：[`TnaDelay`]、[`ChirpState`]（`5.7.6.4.1`）。
//! - **序列游标**：[`SineState`]（`5.7.6.4.2.1` 的正弦延续）、[`NoiseCursor`]
//!   （`5.7.6.4.3`）、[`ToneCursor`]（`5.7.6.4.4`）。
//!
//! # 重置整体替换，不逐字段清
//!
//! [`AspxChannelState::reset`] 写作 `*self = Self::new()`。逐字段清理看起来更
//! 精细，实际上引入了一个无法用判据兜住的风险：**新增一个状态字段却忘了在
//! 重置里清它**。整体替换让这种遗漏在语法上不可能——加字段就必然进
//! `new()`，也就必然被重置覆盖。
//!
//! 这条约束值得写下来，因为它正是本模块存在的理由：把十一个状态收拢到一处，
//! 就是为了让"全部重置"成为一次操作而不是十一次。
//!
//! # 游标的重置条件不由本结构决定
//!
//! [`NoiseCursor`] 受 `master_reset`（配置相对上一个 I 帧变化）控制，
//! [`ToneCursor`] 受 `first_frame`（只在编解码初始化）控制——两者语义不同，
//! 且都是**逐次调用的参数**，由 `noisegen::generate` 与 `tonegen::generate`
//! 各自接收。本结构只管"整条声道重新开始"这一件事，不替它们判断某一帧该不
//! 该重置。普通 A-SPX 配置变化不是重新起解，不调用整组 [`AspxChannelState::reset`]。

use crate::aspx::envelope::EnvelopeHistory;
use crate::aspx::hfadjust::SineState;
use crate::aspx::hfassemble::HfDelay;
use crate::aspx::interleave::AspxOutputDelay;
use crate::aspx::lowband::LowBandDelay;
use crate::aspx::noisegen::NoiseCursor;
use crate::aspx::qmf::{QmfAnalysisState, QmfSynthesisState};
use crate::aspx::tna::{ChirpState, TnaDelay};
use crate::aspx::tonegen::ToneCursor;

/// 一条 A-SPX 声道从 QMF 分析到输出合并的全部跨帧状态。
#[derive(Debug, PartialEq)]
pub struct AspxChannelState {
    /// `5.7.3.1` 分析滤波器组的样本历史。
    pub analysis: QmfAnalysisState,
    /// `5.7.4` 终端合成滤波器组的样本历史；QMF 域出口不会推进它。
    pub synthesis: QmfSynthesisState,
    /// `5.7.6.3.2` 低带滤波的延迟线。
    pub low_band: LowBandDelay,
    /// `5.7.6.3.4` 包络差分解码的跨区间历史。
    pub envelope: EnvelopeHistory,
    /// `5.7.6.4.1` 的预测器输入历史。
    pub tna: TnaDelay,
    /// `5.7.6.4.1.3` 的 chirp 因子延续。
    pub chirp: ChirpState,
    /// `5.7.6.4.2.1` 的正弦延续标志。
    pub sine: SineState,
    /// `5.7.6.4.3` 的噪声表游标。
    pub noise: NoiseCursor,
    /// `5.7.6.4.4` 的音调表游标。
    pub tone: ToneCursor,
    /// `5.7.6.4.5` 越过帧尾的已组装高频信号。
    pub hf: HfDelay,
    /// `5.7.6.5.3` 输出合并的全子带延迟线。
    pub output: AspxOutputDelay,
}

impl AspxChannelState {
    /// 建立全新状态，等价于解码起点。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            analysis: QmfAnalysisState::new(),
            synthesis: QmfSynthesisState::new(),
            low_band: LowBandDelay::new(),
            envelope: EnvelopeHistory::new(),
            tna: TnaDelay::new(),
            chirp: ChirpState::new(),
            sine: SineState::new(),
            noise: NoiseCursor::new(),
            tone: ToneCursor::new(),
            hf: HfDelay::new(),
            output: AspxOutputDelay::new(),
        }
    }

    /// 丢弃全部历史，回到解码起点。
    ///
    /// 用于解码器初始化、seek 后在新随机访问点重新起解，以及外部不连续。
    /// 普通 A-SPX 配置变化仍由各工具自己的 `master_reset`/`first_frame` 条件处理，
    /// 不调用本方法。**整体替换而非逐字段清理**，见模块文档：逐字段写法会让
    /// "新增字段忘了清"成为可能，而那种遗漏没有判据兜得住。
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for AspxChannelState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::aspx::bands::AspxBandTables;
    use crate::aspx::envelope::{
        EnvelopeDeltas, EnvelopeScaleFactors, SbgIndexMap, decode as decode_envelopes,
    };
    use crate::aspx::interleave::combine;
    use crate::aspx::lowband::low_band;
    use crate::aspx::qmf::{QmfSlot, analyse, synthesise};
    use crate::aspx::tna::chirp_factors;
    use std::vec;

    /// 把全部十一个子状态分别推离初值，并逐字段证明夹具不是空断言。
    fn dirtied() -> AspxChannelState {
        let mut state = AspxChannelState::new();
        let pcm: std::vec::Vec<f32> = (0..64 * 8).map(|i| (i as f32) * 0.001).collect();
        let mut slots = vec![QmfSlot::zero(); 8];
        analyse(&pcm, &mut state.analysis, &mut slots).expect("分析");
        let mut back = vec![0.0f32; 64 * 8];
        synthesise(&slots, &mut state.synthesis, &mut back).expect("合成");

        let mut ext = vec![QmfSlot::zero(); 8 + 3];
        low_band(&slots, &mut state.low_band, 10, 3, &mut ext).expect("低带");

        let bands = AspxBandTables::derive(false, 0, 0, 0, 0).expect("频带表");
        let map = SbgIndexMap::derive(&bands).expect("包络映射");
        let sig_data = vec![1i16; usize::from(bands.num_sbg_sig_lowres())];
        let noise_data = vec![1i16; usize::from(bands.num_sbg_noise())];
        let sig = [EnvelopeDeltas {
            data: &sig_data,
            time_direction: false,
            high_resolution: false,
        }];
        let noise = [EnvelopeDeltas {
            data: &noise_data,
            time_direction: false,
            high_resolution: false,
        }];
        let mut factors = EnvelopeScaleFactors::new();
        decode_envelopes(&sig, &noise, &map, false, &mut state.envelope, &mut factors)
            .expect("包络");

        state.tna.advance(&slots, slots.len()).expect("预测器历史");
        let mut chirp = [0.0f32; 1];
        chirp_factors(&[1], &mut state.chirp, &mut chirp).expect("chirp");
        state.sine.mark_non_default_for_test();
        state.noise.mark_non_default_for_test();
        state.tone.mark_non_default_for_test();

        let y = vec![QmfSlot::zero(); 8];
        let mut out = vec![QmfSlot::zero(); 8];
        combine(&slots, &y, 3, &mut state.output, &mut out).expect("合并");

        assert!(state.hf.prefill_silence(4), "越帧缓冲应能声明前置静音");

        let fresh = AspxChannelState::new();
        macro_rules! assert_dirtied {
            ($field:ident) => {
                assert_ne!(
                    state.$field, fresh.$field,
                    concat!(stringify!($field), " 必须离开初值")
                );
            };
        }
        assert_dirtied!(analysis);
        assert_dirtied!(synthesis);
        assert_dirtied!(low_band);
        assert_dirtied!(envelope);
        assert_dirtied!(tna);
        assert_dirtied!(chirp);
        assert_dirtied!(sine);
        assert_dirtied!(noise);
        assert_dirtied!(tone);
        assert_dirtied!(hf);
        assert_dirtied!(output);
        state
    }

    #[test]
    fn a_dirtied_state_differs_from_a_fresh_one() {
        // 先证明「弄脏」确实有效，否则下一条判据是空断言。
        assert_ne!(
            dirtied(),
            AspxChannelState::new(),
            "跑过数据的状态必须与全新状态不同"
        );
    }

    #[test]
    fn reset_returns_every_field_to_the_starting_point() {
        let mut state = dirtied();
        state.reset();
        assert_eq!(
            state,
            AspxChannelState::new(),
            "重置后必须与全新状态逐字段一致"
        );
    }

    #[test]
    fn the_individual_states_stay_independent() {
        // 十一个状态各有各的生命周期，编排层会分别取用；这里确认它们不共享存储
        // ——改一个不应牵动另一个。
        let mut state = AspxChannelState::new();
        assert!(state.hf.prefill_silence(2));
        assert_eq!(state.hf.carried(), 2);
        assert_eq!(state.low_band.history(), 0, "低带延迟不应被越帧缓冲牵动");
        assert_eq!(state.output.history(), 0, "输出延迟不应被越帧缓冲牵动");
        assert_eq!(state.tone.previous(), 0);

        let mut other = AspxChannelState::new();
        assert!(other.hf.prefill_silence(5));
        assert_eq!(state.hf.carried(), 2, "另一条声道的状态不应泄漏进来");
    }
}
