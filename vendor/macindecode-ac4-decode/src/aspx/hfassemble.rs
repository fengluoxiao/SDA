//! 高频信号组装（`TS103190-1:v1.4.1:5.7.6.4.5`）。
//!
//! `Pseudocode 106`–`108`：把 `5.7.6.4.2.2` 的 `sig_gain_sb_adj` 乘到高频生成器
//! 的 `Q_high` 上，再依次叠加 `5.7.6.4.3` 的 `qmf_noise` 与 `5.7.6.4.4` 的
//! `qmf_sine`，得到本区间的 `Y`。这是 A-SPX 参数侧的最后一步；`Y` 随后在
//! `5.7.6.5.3` 与延迟后的 `Q_in,ASPX` 合并成 `Q_out,ASPX`，再交给下游 QMF 域
//! 工具。
//!
//! # 变长边界要跨帧搬运已组装的结果
//!
//! `Pseudocode 106` 的第一个循环把 `Y_prev[sb][num_qmf_timeslots + ts]` 搬进
//! 本区间 `Y` 的前 `atsg_sig[0] · num_ts_in_ats` 个时隙。正文的 NOTE 说明了
//! 缘由：A-SPX 区间的右边界可以越过帧尾（`aspx_var_bord_right`），那部分在上
//! 一帧就已经组装完毕，本帧不重算而是取回来。
//!
//! 因此这里的跨帧状态与 `5.7.6.3.2` 的低带延迟线不同：低带搬的是**输入**，
//! 按固定偏移取后缀；这里搬的是**已组装的输出**，长度由两帧的边界共同决定，
//! 必须精确对齐。[`HfDelay::carried`] 与本区间的左边界不等时直接报错，而不是
//! 像取后缀那样自动截断——串位在这里不会有任何自证迹象。
//!
//! 越过帧尾的时隙数是 `aspx_var_bord_right · num_ts_in_ats`。
//! `4.3.10.4.5` 定义该边界，语法表 53 给它 2 比特、故取值为 `0…3`；表 192
//! 的时隙倍率至多 2，所以上界恰为 [`MAX_HF_CARRYOVER`] = 6。这个界是紧的，
//! 不是随手取的余量。
//!
//! # `Pseudocode 107`/`108` 的起点未乘倍率
//!
//! 两段叠加循环写的是 `for (ts = atsg_sig[0]; ...)`，而同一节的
//! `Pseudocode 106` 与生成侧的 `Pseudocode 102`/`104` 起点都是
//! `atsg_sig[0] · num_ts_in_ats`。倍率为 2 且 `atsg_sig[0] ≠ 0` 时，字面读法会
//! 多覆盖 `[atsg_sig[0], atsg_sig[0] · num_ts_in_ats)` 这一段。
//!
//! 那一段有两个性质：其一，`Pseudocode 102`/`104` 根本没往那里写过
//! `qmf_noise`/`qmf_sine`；其二，`Pseudocode 106` 的第一个循环恰好覆盖它，写进
//! 去的是**上一帧已经加过噪声与正弦**的成品。所以字面读法要么读到未定义值，
//! 要么构成二次叠加。
//!
//! 本实现按 `atsg_sig[0] · num_ts_in_ats` 起加，与 `106` 及生成侧一致。这在
//! 「`qmf_noise`/`qmf_sine` 在该区间为零」时与字面读法等价，而本仓库的生成器
//! 只产出 `[first · factor, end · factor)`，那段根本不存在，字面读法无从表达。
//! 已登记在规范可追踪性第 7 节。
//!
//! # 子带下标的两套约定
//!
//! `Pseudocode 106` 写 `Y[sb][ts]` 用**相对**子带号，读 `Q_high[sb+sbx][ts]` 用
//! **绝对**号；`107`/`108` 的 `qmf_noise[sb][ts]` 又是相对号。本实现的
//! [`QmfSlot`] 一律是 64 个绝对子带，噪声与音调生成器也已经写在绝对位置上，
//! 故这里全程用绝对号，只写 `[sbx, sbx + num_sb_aspx)`，其余子带保持调用方交
//! 来的值——低带由 `5.7.6.3.2` 负责，不在本步。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "子带下标以 64 为界，时隙范围由函数开头的前置检查给出"
)]

use crate::aspx::bands::AspxBandTables;
use crate::aspx::frames::AspxInterval;
use crate::aspx::hfgain::AdjustedGains;
use crate::aspx::qmf::QmfSlot;
use crate::aspx::tables::{NUM_QMF_SUBBANDS, qmf_timeslots_for_aspx_layout};

/// 子带数，`5.7.3.2` 规定恒为 64。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// 越过帧尾、需要留给下一帧的最大 QMF 时隙数。
///
/// `aspx_var_bord_right` 是 2 比特（取值 `0…3`），`num_ts_in_ats` 至多 2。
pub const MAX_HF_CARRYOVER: usize = 6;

/// 高频组装无法执行的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssembleError {
    /// 区间的信号包络数与 `sig_gain_sb_adj` 携带的不符。
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
    /// 区间来源的 A-SPX 时隙数与倍率不是表 189/192 的合法配对。
    UnsupportedTimeslotLayout { num_aspx_timeslots: u8, factor: u8 },
    /// 显式传入的 QMF 时隙数与区间来源及倍率对应的表行不同。
    QmfTimeslotCountMismatch { expected: u8, provided: u8 },
    /// 包络的时隙区间为空或首尾颠倒。
    EmptyEnvelope { envelope: usize },
    /// `Q_high`、`qmf_noise` 或 `qmf_sine` 的时隙数与区间覆盖的不符。
    SourceLengthMismatch {
        which: SourceBuffer,
        expected: usize,
        provided: usize,
    },
    /// 输出时隙数与 `atsg_sig[num_atsg_sig] · num_ts_in_ats` 不符。
    OutputLengthMismatch { expected: usize, provided: usize },
    /// 延迟缓冲携带的时隙数与本区间左边界不符。
    ///
    /// 上一帧越过帧尾多少，本帧就必须从延迟取回多少；不等即是串位。
    CarryoverMismatch { carried: usize, required: usize },
    /// 本区间读取或保存的越帧时隙数超出当前倍率的两比特边界上限。
    CarryoverOutOfRange { carryover: usize },
    /// `num_qmf_timeslots` 大于本区间的右边界，无法确定越帧部分。
    TimeslotsBeyondInterval { timeslots: usize, stop: usize },
}

/// 出错的输入缓冲。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBuffer {
    /// 高频生成器的 `Q_high`。
    QHigh,
    /// 噪声生成器的 `qmf_noise`。
    Noise,
    /// 音调生成器的 `qmf_sine`。
    Sine,
}

/// 跨帧携带的已组装高频信号，即 `Pseudocode 106` 的 `Y_prev` 尾部。
#[derive(Debug, PartialEq)]
pub struct HfDelay {
    tail: [QmfSlot; MAX_HF_CARRYOVER],
    carried: u8,
}

impl HfDelay {
    /// 建立空状态：没有任何越帧信号。
    ///
    /// 首个区间的左边界若非零（`VARFIX`/`VARVAR` 的 I 帧带
    /// `aspx_var_bord_left`），调用方必须先用 [`Self::prefill_silence`] 显式声明
    /// 前置静音，否则 [`assemble`] 会以 [`AssembleError::CarryoverMismatch`]
    /// 拒绝——解码起点缺多少历史是调用方的判断，不由本函数猜。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tail: [QmfSlot::zero(); MAX_HF_CARRYOVER],
            carried: 0,
        }
    }

    /// 声明 `count` 个静音时隙作为前置历史。
    ///
    /// 超出 [`MAX_HF_CARRYOVER`] 时不改状态并返回 `false`。
    pub fn prefill_silence(&mut self, count: usize) -> bool {
        if count > MAX_HF_CARRYOVER {
            return false;
        }
        self.tail = [QmfSlot::zero(); MAX_HF_CARRYOVER];
        self.carried = u8::try_from(count).unwrap_or(0);
        true
    }

    /// 供其他模块的判据往越帧缓冲里塞已知内容。
    #[cfg(test)]
    pub(crate) fn tail_mut_for_test(&mut self, ts: usize) -> &mut QmfSlot {
        &mut self.tail[ts]
    }

    /// 上一帧越过帧尾、本帧应当取回的时隙数。
    #[must_use]
    pub const fn carried(&self) -> u8 {
        self.carried
    }
}

impl Default for HfDelay {
    fn default() -> Self {
        Self::new()
    }
}

/// `Pseudocode 106`–`108`：组装本区间的高频信号 `Y`。
///
/// `q_high`、`qmf_noise` 与 `qmf_sine` 的第 0 项都对应 QMF 时隙
/// `atsg_sig[0] · num_ts_in_ats`，与 [`crate::aspx::noisegen::generate`] 和
/// [`crate::aspx::tonegen::generate`] 的输出约定一致；`out` 的第 0 项对应时隙
/// 0，因此它比三个输入长 `atsg_sig[0] · num_ts_in_ats` 个时隙，多出的前缀取自
/// `delay`。
///
/// 每个时隙只写 `[sbx, sbx + num_sb_aspx)`，其余子带保持调用方交来的值。
///
/// # Errors
///
/// 见 [`AssembleError`]。任一条不成立时都不改写 `out` 与 `delay`。
#[expect(
    clippy::too_many_arguments,
    reason = "三段伪码的输入合起来就是这几路；q_high、qmf_noise 与 qmf_sine \
              分别来自三个独立的工具，聚成一个结构体反而掩盖它们各自的来源"
)]
pub fn assemble(
    gains: &AdjustedGains,
    bands: &AspxBandTables,
    interval: &AspxInterval,
    num_ts_in_ats: u8,
    num_qmf_timeslots: u8,
    q_high: &[QmfSlot],
    qmf_noise: &[QmfSlot],
    qmf_sine: &[QmfSlot],
    delay: &mut HfDelay,
    out: &mut [QmfSlot],
) -> Result<(), AssembleError> {
    let envelopes = usize::from(gains.envelopes());
    if envelopes == 0 || interval.num_atsg_sig() != gains.envelopes() {
        return Err(AssembleError::EnvelopeCountMismatch {
            interval: interval.num_atsg_sig(),
            gains: gains.envelopes(),
        });
    }
    if !matches!(num_ts_in_ats, 1 | 2) {
        return Err(AssembleError::TimeslotFactorOutOfRange {
            factor: num_ts_in_ats,
        });
    }
    let sbx = usize::from(bands.sbx());
    let num_sb_aspx = usize::from(bands.num_sb_aspx());
    let Some(end_sb) = sbx.checked_add(num_sb_aspx) else {
        return Err(AssembleError::SubbandOutOfRange { sbx, num_sb_aspx });
    };
    if num_sb_aspx == 0 || end_sb > SUBBANDS {
        return Err(AssembleError::SubbandOutOfRange { sbx, num_sb_aspx });
    }
    if !gains.matches_bands(bands) {
        return Err(AssembleError::BandLayoutMismatch);
    }
    if !gains.matches_interval(interval) {
        return Err(AssembleError::IntervalMismatch);
    }
    if gains.source_num_ts_in_ats() != num_ts_in_ats {
        return Err(AssembleError::TimeslotFactorMismatch {
            gains: gains.source_num_ts_in_ats(),
            requested: num_ts_in_ats,
        });
    }
    let num_aspx_timeslots = interval.source_num_aspx_timeslots();
    let Some(expected_qmf_timeslots) =
        qmf_timeslots_for_aspx_layout(num_aspx_timeslots, num_ts_in_ats)
    else {
        return Err(AssembleError::UnsupportedTimeslotLayout {
            num_aspx_timeslots,
            factor: num_ts_in_ats,
        });
    };
    if num_qmf_timeslots != expected_qmf_timeslots {
        return Err(AssembleError::QmfTimeslotCountMismatch {
            expected: expected_qmf_timeslots,
            provided: num_qmf_timeslots,
        });
    }
    let border = |index: usize| i32::from(interval.sig_border(index).unwrap_or(0));
    for atsg in 0..envelopes {
        if border(atsg + 1) <= border(atsg) {
            return Err(AssembleError::EmptyEnvelope { envelope: atsg });
        }
    }

    let factor = i32::from(num_ts_in_ats);
    let first = border(0);
    let stop = border(envelopes);
    // 起点非负由 `AspxInterval::derive` 的 `NegativeStartBorder` 保证，
    // 全零边界被上面的 `EmptyEnvelope` 挡住，见 noisegen 同处。
    let head = usize::try_from(first * factor).unwrap_or(0);
    let total = usize::try_from(stop * factor).unwrap_or(0);
    let body = total.saturating_sub(head);
    let timeslots = usize::from(expected_qmf_timeslots);
    if timeslots > total {
        return Err(AssembleError::TimeslotsBeyondInterval {
            timeslots,
            stop: total,
        });
    }
    let max_carryover = usize::from(num_ts_in_ats) * 3;
    if head > max_carryover {
        return Err(AssembleError::CarryoverOutOfRange { carryover: head });
    }
    let carryover = total - timeslots;
    if carryover > max_carryover {
        return Err(AssembleError::CarryoverOutOfRange { carryover });
    }

    for (which, buffer) in [
        (SourceBuffer::QHigh, q_high),
        (SourceBuffer::Noise, qmf_noise),
        (SourceBuffer::Sine, qmf_sine),
    ] {
        if buffer.len() != body {
            return Err(AssembleError::SourceLengthMismatch {
                which,
                expected: body,
                provided: buffer.len(),
            });
        }
    }
    if out.len() != total {
        return Err(AssembleError::OutputLengthMismatch {
            expected: total,
            provided: out.len(),
        });
    }
    if usize::from(delay.carried) != head {
        return Err(AssembleError::CarryoverMismatch {
            carried: usize::from(delay.carried),
            required: head,
        });
    }
    // `Pseudocode 106` 第一段：取回上一帧越过帧尾的已组装信号。
    for (timeslot, slot) in out.iter_mut().take(head).enumerate() {
        let source = &delay.tail[timeslot];
        for sb in sbx..end_sb {
            slot.re[sb] = source.re[sb];
            slot.im[sb] = source.im[sb];
        }
    }

    // `Pseudocode 106` 第二段与 `107`/`108`：本区间现算，随后叠加噪声与正弦。
    let mut atsg = 0usize;
    for (offset, slot) in out.iter_mut().skip(head).enumerate() {
        let ts = i32::try_from(head + offset).unwrap_or(i32::MAX);
        if ts == border(atsg + 1) * factor {
            atsg += 1;
        }
        let source = &q_high[offset];
        let noise = &qmf_noise[offset];
        let sine = &qmf_sine[offset];
        for sb in sbx..end_sb {
            let relative = sb - sbx;
            let gain = gains.signal_gain(relative, atsg).unwrap_or(0.0);
            // 三段伪码按原顺序分步写：`106` 先乘，`107` 再加噪声，`108` 最后加
            // 正弦。不合并成一条乘加式——`a * b + c` 允许后端融合成 FMA，而
            // ADR-0002 要求舍入次数与有无 FMA 的目标无关。
            let mut re = gain * source.re[sb];
            let mut im = gain * source.im[sb];
            re += noise.re[sb];
            im += noise.im[sb];
            re += sine.re[sb];
            im += sine.im[sb];
            slot.re[sb] = re;
            slot.im[sb] = im;
        }
    }

    // 越过帧尾的部分留给下一帧；不足 `MAX_HF_CARRYOVER` 的位置保持零。
    delay.tail = [QmfSlot::zero(); MAX_HF_CARRYOVER];
    delay.tail[..carryover].copy_from_slice(&out[timeslots..timeslots + carryover]);
    delay.carried = u8::try_from(carryover).unwrap_or(0);
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
    use crate::aspx::dequant::ScaleFactors;
    use crate::aspx::frames::AspxIntervalParams;
    use crate::aspx::hfadjust::{EnvelopeEstimate, SinePlacement, SineState, estimate};
    use crate::aspx::hfgain::{LimiterMode, adjust};
    use crate::aspx::limiter::LimiterTable;
    use crate::aspx::patches::PatchTable;
    use crate::aspx::tables::IntervalClass;
    use std::vec;
    use std::vec::Vec;

    const SLOTS: u8 = 16;
    /// 倍率为 2 时 [`SLOTS`] 个 ATS 对应的 QMF 时隙数。
    const QMF_SLOTS_X2: u8 = 32;

    fn bands() -> AspxBandTables {
        AspxBandTables::derive(false, 0, 0, 0, 0).expect("应能推出频带表")
    }

    fn fixfix(envelopes: u8) -> AspxInterval {
        let params = AspxIntervalParams::fixfix(envelopes);
        AspxInterval::derive(&params, SLOTS, 1, true, i16::from(SLOTS)).expect("应能推导区间")
    }

    /// 起点非零、配合倍率 2 使用：`atsg_sig[0] = 2`，故 head = 4。
    ///
    /// 区间以 ATS 计有 16 个时隙，倍率 2 时对应 [`QMF_SLOTS_X2`] = 32 个 QMF
    /// 时隙——`num_qmf_timeslots` 是 QMF 单位，按 ATS 传会把越帧量算成 16。
    fn varfix() -> AspxInterval {
        let mut params = AspxIntervalParams::fixfix(1);
        params.int_class = IntervalClass::VarFix;
        params.var_bord_left = Some(2);
        AspxInterval::derive(&params, SLOTS, 1, true, 16).expect("应能推导区间")
    }

    fn gains_for(
        bands: &AspxBandTables,
        interval: &AspxInterval,
        num_ts_in_ats: u8,
        placement: SinePlacement,
    ) -> AdjustedGains {
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let envelopes = usize::from(interval.num_atsg_sig());
        let mut q = [QmfSlot::zero(); 64];
        for slot in &mut q {
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
            &q,
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

    /// 每个 (时隙, 子带) 填一个互不相同的值，搬错一格就能认出来。
    fn tagged(count: usize, sbx: usize, span: usize, scale: f32) -> Vec<QmfSlot> {
        let mut buf = vec![QmfSlot::zero(); count];
        for (ts, slot) in buf.iter_mut().enumerate() {
            for sb in sbx..sbx + span {
                let tag = ((ts * 100 + sb) as f32) * scale;
                slot.re[sb] = tag;
                slot.im[sb] = -tag;
            }
        }
        buf
    }

    #[test]
    fn the_signal_gain_multiplies_q_high() {
        // `Pseudocode 106`：Q_high 取全 1、噪声与正弦取零时，输出必须**精确
        // 等于** sig_gain_sb_adj 本身。断言直接取增益比较，不在测试里重抄
        // 一遍乘法公式。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = fixfix(1);
        let gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(-1));
        let slots = usize::from(SLOTS);

        let mut q_high = vec![QmfSlot::zero(); slots];
        for slot in &mut q_high {
            for sb in sbx..sbx + count {
                slot.re[sb] = 1.0;
                slot.im[sb] = 1.0;
            }
        }
        let zero = vec![QmfSlot::zero(); slots];
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = HfDelay::new();
        assemble(
            &gains, &bands, &interval, 1, SLOTS, &q_high, &zero, &zero, &mut delay, &mut out,
        )
        .expect("应能组装");

        for slot in &out {
            for sb in sbx..sbx + count {
                let gain = gains.signal_gain(sb - sbx, 0).expect("范围内");
                assert_eq!(slot.re[sb], gain, "子带 {sb} 的实部应等于信号增益");
                assert_eq!(slot.im[sb], gain, "子带 {sb} 的虚部应等于信号增益");
            }
        }
    }

    #[test]
    fn the_noise_and_the_tone_are_both_added() {
        // `Pseudocode 107`/`108`：Q_high 取零时，输出必须精确等于噪声加正弦。
        // 两个缓冲用不同标度，任一被漏掉或被重复加都会改变结果。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = fixfix(1);
        let gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(-1));
        let slots = usize::from(SLOTS);

        let zero = vec![QmfSlot::zero(); slots];
        let noise = tagged(slots, sbx, count, 1.0);
        let sine = tagged(slots, sbx, count, 0.125);
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = HfDelay::new();
        assemble(
            &gains, &bands, &interval, 1, SLOTS, &zero, &noise, &sine, &mut delay, &mut out,
        )
        .expect("应能组装");

        for (ts, slot) in out.iter().enumerate() {
            for sb in sbx..sbx + count {
                assert_eq!(
                    slot.re[sb],
                    noise[ts].re[sb] + sine[ts].re[sb],
                    "时隙 {ts} 子带 {sb} 应是噪声与正弦之和"
                );
                assert_eq!(slot.im[sb], noise[ts].im[sb] + sine[ts].im[sb]);
            }
        }
    }

    #[test]
    fn multiply_noise_and_tone_keep_the_three_pseudocode_roundings() {
        // 这组数专门让三种合法编译写法得到三个不同的 f32：
        //
        // 1. `(gain * source)`、`+ noise`、`+ sine` 逐步舍入（规范顺序）；
        // 2. `gain.mul_add(source, noise)` 后再加正弦；
        // 3. 先算 `noise + sine`，再与乘积相加。
        //
        // 三路输入在同一个复样本上都非零，避免任一运算被夹具消掉。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let interval = fixfix(1);
        let mut gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(-1));
        let gain = 620.906_4f32;
        let source_value = -173.319_27f32;
        let noise_value = -993.401_25f32;
        let sine_value = 65_536.0f32;
        assert!(
            gains.set_signal_gain_for_test(0, 0, gain),
            "目标增益必须落在有效矩阵内"
        );

        let slots = usize::from(SLOTS);
        let mut q_high = vec![QmfSlot::zero(); slots];
        let mut noise = vec![QmfSlot::zero(); slots];
        let mut sine = vec![QmfSlot::zero(); slots];
        q_high[0].re[sbx] = source_value;
        q_high[0].im[sbx] = source_value;
        noise[0].re[sbx] = noise_value;
        noise[0].im[sbx] = noise_value;
        sine[0].re[sbx] = sine_value;
        sine[0].im[sbx] = sine_value;

        let product = gain * source_value;
        let mut expected = product;
        expected += noise_value;
        expected += sine_value;
        let mut fused = gain.mul_add(source_value, noise_value);
        fused += sine_value;
        let combined_addends = noise_value + sine_value;
        let reassociated = product + combined_addends;
        assert_ne!(
            expected.to_bits(),
            fused.to_bits(),
            "夹具必须能分辨 FMA，否则判据没有鉴别力"
        );
        assert_ne!(
            expected.to_bits(),
            reassociated.to_bits(),
            "夹具必须能分辨加法重结合，否则判据没有鉴别力"
        );

        let mut out = vec![QmfSlot::zero(); slots];
        assemble(
            &gains,
            &bands,
            &interval,
            1,
            SLOTS,
            &q_high,
            &noise,
            &sine,
            &mut HfDelay::new(),
            &mut out,
        )
        .expect("应能组装");

        assert_eq!(out[0].re[sbx].to_bits(), expected.to_bits());
        assert_eq!(out[0].im[sbx].to_bits(), expected.to_bits());
    }

    #[test]
    fn the_gain_follows_the_envelope_borders() {
        // `Pseudocode 106` 的 atsg 推进：两个包络增益不同时，边界两侧换值。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = fixfix(2);
        let gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(0));
        let sb = (0..count)
            .find(|&sb| {
                let a = gains.signal_gain(sb, 0).expect("范围内");
                let b = gains.signal_gain(sb, 1).expect("范围内");
                a != b
            })
            .expect("应有两个包络增益不同的子带");
        let split = usize::try_from(interval.sig_border(1).expect("中间边界")).expect("非负");
        let slots = usize::from(SLOTS);

        let mut q_high = vec![QmfSlot::zero(); slots];
        for slot in &mut q_high {
            for s in sbx..sbx + count {
                slot.re[s] = 1.0;
            }
        }
        let zero = vec![QmfSlot::zero(); slots];
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = HfDelay::new();
        assemble(
            &gains, &bands, &interval, 1, SLOTS, &q_high, &zero, &zero, &mut delay, &mut out,
        )
        .expect("应能组装");

        let early = gains.signal_gain(sb, 0).expect("范围内");
        let late = gains.signal_gain(sb, 1).expect("范围内");
        assert!(split > 0 && split < slots);
        for (ts, slot) in out.iter().enumerate() {
            let expected = if ts < split { early } else { late };
            assert_eq!(slot.re[sb + sbx], expected, "时隙 {ts} 的包络取值不对");
        }
    }

    #[test]
    fn the_carried_head_is_taken_verbatim_and_never_re_synthesised() {
        // `Pseudocode 106` 第一段与 NOTE：越过帧尾的部分在上一帧已经组装完毕，
        // 本帧必须**原样取回**。
        //
        // 这条同时锁住 `107`/`108` 的起点判读：倍率 2、起点 2 时 head = 4，按
        // 字面从 `atsg_sig[0] = 2` 起加噪声会污染时隙 2 与 3，而它们属于取回
        // 的那一段。噪声取非零正是为了让那种写法显形。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = varfix();
        assert_eq!(interval.sig_border(0), Some(2), "本判据需要非零起点");
        let gains = gains_for(&bands, &interval, 2, SinePlacement::from_params(-1));
        let head = 4usize;
        let total = usize::try_from(interval.sig_border(1).expect("右边界")).expect("非负") * 2;
        let body = total - head;

        let mut delay = HfDelay::new();
        assert!(delay.prefill_silence(head), "应能声明前置静音");
        // 把可辨认的值直接放进延迟缓冲，模拟上一帧留下的成品。
        let carried = tagged(head, sbx, count, 4.0);
        delay.tail[..head].clone_from_slice(&carried);

        let zero = vec![QmfSlot::zero(); body];
        let noise = tagged(body, sbx, count, 1.0);
        let mut out = vec![QmfSlot::zero(); total];
        assemble(
            &gains,
            &bands,
            &interval,
            2,
            QMF_SLOTS_X2,
            &zero,
            &noise,
            &zero,
            &mut delay,
            &mut out,
        )
        .expect("应能组装");

        for ts in 0..head {
            for sb in sbx..sbx + count {
                assert_eq!(
                    out[ts].re[sb], carried[ts].re[sb],
                    "时隙 {ts} 子带 {sb} 应原样取自延迟缓冲，不得被叠加"
                );
                assert_eq!(out[ts].im[sb], carried[ts].im[sb]);
            }
        }
        // 反面：本区间内确实加了噪声，否则上面的相等可能来自「什么都没加」。
        assert_ne!(noise[0].re[sbx], 0.0, "噪声必须非零，否则判据无鉴别力");
        assert_eq!(
            out[head].re[sbx], noise[0].re[sbx],
            "本区间首个时隙应含噪声"
        );
    }

    #[test]
    fn the_carryover_is_exactly_the_part_beyond_the_frame() {
        // 越帧部分是 `[num_qmf_timeslots, atsg_sig[num_atsg_sig] · factor)`，
        // 下一帧要按同样的长度取回。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let mut params = AspxIntervalParams::fixfix(1);
        params.int_class = IntervalClass::FixVar;
        params.var_bord_right = Some(3);
        let interval = AspxInterval::derive(&params, SLOTS, 1, true, 16).expect("应能推导区间");
        let total = usize::try_from(interval.sig_border(1).expect("右边界")).expect("非负");
        assert_eq!(total, usize::from(SLOTS) + 3, "右边界应越过帧尾 3 个时隙");
        let gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(-1));

        let zero = vec![QmfSlot::zero(); total];
        let noise = tagged(total, sbx, count, 1.0);
        let mut out = vec![QmfSlot::zero(); total];
        let mut delay = HfDelay::new();
        assemble(
            &gains, &bands, &interval, 1, SLOTS, &zero, &noise, &zero, &mut delay, &mut out,
        )
        .expect("应能组装");

        assert_eq!(usize::from(delay.carried()), 3, "应携带 3 个越帧时隙");
        for position in 0..3 {
            for sb in sbx..sbx + count {
                assert_eq!(
                    delay.tail[position].re[sb],
                    out[usize::from(SLOTS) + position].re[sb],
                    "携带的应是帧尾之后的那几个时隙"
                );
            }
        }
        // 未用满的位置必须是零，不能留上一次的残值。
        for position in 3..MAX_HF_CARRYOVER {
            for sb in 0..SUBBANDS {
                assert_eq!(delay.tail[position].re[sb], 0.0);
                assert_eq!(delay.tail[position].im[sb], 0.0);
            }
        }
    }

    #[test]
    fn factor_two_scales_the_largest_variable_right_border_to_six_slots() {
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let mut params = AspxIntervalParams::fixfix(1);
        params.int_class = IntervalClass::FixVar;
        params.var_bord_right = Some(3);
        let interval = AspxInterval::derive(&params, SLOTS, 1, true, 16).expect("应能推导区间");
        let total = usize::try_from(interval.sig_border(1).expect("右边界")).expect("非负") * 2;
        assert_eq!(total, usize::from(QMF_SLOTS_X2) + MAX_HF_CARRYOVER);
        let gains = gains_for(&bands, &interval, 2, SinePlacement::from_params(-1));

        let zero = vec![QmfSlot::zero(); total];
        let noise = tagged(total, sbx, count, 1.0);
        let mut out = vec![QmfSlot::zero(); total];
        let mut delay = HfDelay::new();
        assemble(
            &gains,
            &bands,
            &interval,
            2,
            QMF_SLOTS_X2,
            &zero,
            &noise,
            &zero,
            &mut delay,
            &mut out,
        )
        .expect("倍率 2 的最大合法右边界应能组装");

        assert_eq!(usize::from(delay.carried()), MAX_HF_CARRYOVER);
        assert_eq!(
            delay.tail.as_slice(),
            &out[usize::from(QMF_SLOTS_X2)..],
            "倍率必须同时作用在帧长与可变右边界上"
        );
    }

    #[test]
    fn a_variable_border_above_the_two_bit_range_is_rejected_per_factor() {
        let bands = bands();
        let mut params = AspxIntervalParams::fixfix(1);
        params.int_class = IntervalClass::FixVar;
        params.var_bord_right = Some(4);
        let interval =
            AspxInterval::derive(&params, SLOTS, 1, true, 16).expect("边界推导本身可完成");
        let total = usize::try_from(interval.sig_border(1).expect("右边界")).expect("非负");
        let gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(-1));
        let zero = vec![QmfSlot::zero(); total];
        let mut out = vec![QmfSlot::zero(); total];

        assert_eq!(
            assemble(
                &gains,
                &bands,
                &interval,
                1,
                SLOTS,
                &zero,
                &zero,
                &zero,
                &mut HfDelay::new(),
                &mut out,
            ),
            Err(AssembleError::CarryoverOutOfRange { carryover: 4 })
        );
    }

    #[test]
    fn the_carryover_buffer_is_cleared_before_it_is_refilled() {
        // 携带量逐帧变化：上一帧留下 6 个时隙、本帧只留 1 个时，位置 1…5 必须
        // 被清掉，否则下一帧会读到更早那一帧的残值。
        //
        // **判据必须从非零状态出发。** `HfDelay::new()` 的 tail 本来就是全零，
        // 从它出发断言「未用位置为零」对「根本不清零」这个缺陷恒真——那是空
        // 断言，注入实测确实一条都不响。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let mut params = AspxIntervalParams::fixfix(1);
        params.int_class = IntervalClass::FixVar;
        params.var_bord_right = Some(1);
        let interval = AspxInterval::derive(&params, SLOTS, 1, true, 16).expect("应能推导区间");
        let total = usize::try_from(interval.sig_border(1).expect("右边界")).expect("非负");
        assert_eq!(total, usize::from(SLOTS) + 1, "本判据要求只越帧 1 个时隙");
        let gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(-1));

        let mut delay = HfDelay::new();
        // 先把整个缓冲填成可辨认的非零残值，模拟上一帧携带了 6 个时隙。
        let stale = tagged(MAX_HF_CARRYOVER, 0, SUBBANDS, 9.0);
        delay.tail.clone_from_slice(&stale);
        assert_ne!(delay.tail[1].re[sbx], 0.0, "残值必须非零，否则判据无鉴别力");

        let zero = vec![QmfSlot::zero(); total];
        let noise = tagged(total, sbx, count, 1.0);
        let mut out = vec![QmfSlot::zero(); total];
        assemble(
            &gains, &bands, &interval, 1, SLOTS, &zero, &noise, &zero, &mut delay, &mut out,
        )
        .expect("应能组装");

        assert_eq!(usize::from(delay.carried()), 1);
        for sb in sbx..sbx + count {
            assert_eq!(
                delay.tail[0].re[sb],
                out[usize::from(SLOTS)].re[sb],
                "第一个位置应写入本帧的越帧时隙"
            );
        }
        for position in 1..MAX_HF_CARRYOVER {
            for sb in 0..SUBBANDS {
                assert_eq!(
                    delay.tail[position].re[sb], 0.0,
                    "位置 {position} 子带 {sb} 的旧残值必须被清掉"
                );
                assert_eq!(delay.tail[position].im[sb], 0.0);
            }
        }
    }

    #[test]
    fn a_mismatched_carryover_is_rejected_rather_than_truncated() {
        // 延迟缓冲与本区间左边界必须精确对齐。取后缀式的自动截断会让串位
        // 悄悄通过，这里要求报错。
        let bands = bands();
        let interval = varfix();
        let gains = gains_for(&bands, &interval, 2, SinePlacement::from_params(-1));
        let head = 4usize;
        let total = usize::try_from(interval.sig_border(1).expect("右边界")).expect("非负") * 2;
        let body = total - head;
        let zero_body = vec![QmfSlot::zero(); body];
        let mut out = vec![QmfSlot::zero(); total];

        for carried in [0usize, head - 1, head + 1] {
            let mut delay = HfDelay::new();
            assert!(delay.prefill_silence(carried));
            assert_eq!(
                assemble(
                    &gains,
                    &bands,
                    &interval,
                    2,
                    QMF_SLOTS_X2,
                    &zero_body,
                    &zero_body,
                    &zero_body,
                    &mut delay,
                    &mut out,
                ),
                Err(AssembleError::CarryoverMismatch {
                    carried,
                    required: head
                }),
                "携带 {carried} 个时隙时应被拒绝"
            );
        }
    }

    #[test]
    fn only_the_aspx_range_is_written() {
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = fixfix(1);
        let gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(-1));
        let slots = usize::from(SLOTS);
        let noise = tagged(slots, sbx, count, 1.0);
        let zero = vec![QmfSlot::zero(); slots];
        let mut out = vec![QmfSlot::zero(); slots];
        for slot in &mut out {
            for sb in 0..SUBBANDS {
                slot.re[sb] = 7.0;
                slot.im[sb] = -7.0;
            }
        }
        let mut delay = HfDelay::new();
        assemble(
            &gains, &bands, &interval, 1, SLOTS, &zero, &noise, &zero, &mut delay, &mut out,
        )
        .expect("应能组装");

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
                    assert_eq!(slot.im[sb], -7.0);
                }
            }
        }
    }

    #[test]
    fn a_qmf_count_from_another_table_row_or_zero_is_rejected() {
        let bands = bands();
        for (aspx_slots, provided) in [(16u8, 15u8), (6, 0)] {
            let params = AspxIntervalParams::fixfix(1);
            let interval =
                AspxInterval::derive(&params, aspx_slots, 1, true, i16::from(aspx_slots))
                    .expect("应能推导区间");
            let gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(-1));
            let slots = usize::from(aspx_slots);
            let zero = vec![QmfSlot::zero(); slots];
            let mut out = vec![QmfSlot::zero(); slots];

            assert_eq!(
                assemble(
                    &gains,
                    &bands,
                    &interval,
                    1,
                    provided,
                    &zero,
                    &zero,
                    &zero,
                    &mut HfDelay::new(),
                    &mut out,
                ),
                Err(AssembleError::QmfTimeslotCountMismatch {
                    expected: aspx_slots,
                    provided,
                }),
                "A-SPX 时隙数 {aspx_slots} 不应接受 QMF 时隙数 {provided}"
            );
        }
    }

    #[test]
    fn rejected_input_leaves_the_output_and_delay_untouched() {
        let bands = bands();
        let interval = fixfix(1);
        let gains = gains_for(&bands, &interval, 1, SinePlacement::from_params(-1));
        let slots = usize::from(SLOTS);
        let zero = vec![QmfSlot::zero(); slots];
        let noise = tagged(slots, usize::from(bands.sbx()), 4, 1.0);
        let mut out = vec![QmfSlot::zero(); slots];
        let mut delay = HfDelay::new();
        assemble(
            &gains, &bands, &interval, 1, SLOTS, &zero, &noise, &zero, &mut delay, &mut out,
        )
        .expect("哨兵结果");
        let snapshot = out.clone();
        let stale = tagged(MAX_HF_CARRYOVER, 0, SUBBANDS, 11.0);
        delay.tail.clone_from_slice(&stale);
        assert_ne!(delay.tail[1].re[1], 0.0, "延迟哨兵必须非零");
        let tail_snapshot = delay.tail;
        let carried_snapshot = delay.carried();

        for factor in [0u8, 3] {
            assert_eq!(
                assemble(
                    &gains, &bands, &interval, factor, SLOTS, &zero, &noise, &zero, &mut delay,
                    &mut out,
                ),
                Err(AssembleError::TimeslotFactorOutOfRange { factor })
            );
        }
        let foreign = AspxBandTables::derive(false, 1, 0, 0, 0).expect("应能推出频带表");
        assert_eq!(
            assemble(
                &gains, &foreign, &interval, 1, SLOTS, &zero, &noise, &zero, &mut delay, &mut out,
            ),
            Err(AssembleError::BandLayoutMismatch)
        );
        let short = vec![QmfSlot::zero(); 4];
        assert_eq!(
            assemble(
                &gains, &bands, &interval, 1, SLOTS, &short, &noise, &zero, &mut delay, &mut out,
            ),
            Err(AssembleError::SourceLengthMismatch {
                which: SourceBuffer::QHigh,
                expected: slots,
                provided: 4
            })
        );
        assert_eq!(
            assemble(
                &gains, &bands, &interval, 1, SLOTS, &zero, &short, &zero, &mut delay, &mut out,
            ),
            Err(AssembleError::SourceLengthMismatch {
                which: SourceBuffer::Noise,
                expected: slots,
                provided: 4
            })
        );
        assert_eq!(
            assemble(
                &gains, &bands, &interval, 1, 15, &zero, &noise, &zero, &mut delay, &mut out,
            ),
            Err(AssembleError::QmfTimeslotCountMismatch {
                expected: SLOTS,
                provided: 15
            })
        );

        assert_eq!(out, snapshot, "被拒绝的输入不应改写输出");
        assert_eq!(
            delay.carried(),
            carried_snapshot,
            "被拒绝的输入不应改写延迟"
        );
        assert_eq!(delay.tail, tail_snapshot, "被拒绝的输入不应改写延迟内容");
    }
}
