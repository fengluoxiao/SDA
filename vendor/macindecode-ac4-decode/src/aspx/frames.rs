//! A-SPX 时频矩阵的时间轴推导。
//!
//! 覆盖 `TS103190-1:v1.4.1` 的 `5.7.6.3.3.1`：表 194 的 `tab_border`、
//! 表 193 的 `noise_mid_border`、`Pseudocode 76` 的包络边界与
//! `Pseudocode 77` 的 `freq_res()`。
//!
//! 这一层是**解析所必需**而非仅供重建：`aspx_freq_res_mode` 取 2 时，每个
//! 包络的频率分辨率由该包络的时隙跨度决定，而分辨率又决定 `aspx_ec_data()`
//! 取 `num_sbg_sig_highres` 还是 `num_sbg_sig_lowres`，从而改变 Huffman 解
//! 码的次数。边界算错，整段熵编码数据就会错位。
//!
//! 本模块不读比特，输入取自已解析的 `aspx_framing()` 字段。

use super::tables::IntervalClass;
use core::fmt;

/// 一个 A-SPX 间隔内的最大信号包络数，见表 128。
pub const MAX_ATSG_SIG: usize = 5;

/// 一个 A-SPX 间隔内的最大噪声包络数，见 `4.3.10.4.11`。
pub const MAX_ATSG_NOISE: usize = 2;

/// `aspx_num_rel_left` 与 `aspx_num_rel_right` 的上界。
///
/// 两者各占 1 或 2 位，故取值不超过 3。
pub const MAX_REL_BORDERS: usize = 3;

/// 时频成帧推导失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// `num_aspx_timeslots` 不在表 194 收录的五个取值内。
    UnsupportedTimeslots {
        /// 传入的时隙数。
        num_aspx_timeslots: u8,
    },
    /// FIXFIX 的包络数不在表 194 的三列内。
    UnsupportedFixfixEnvelopes {
        /// 传入的包络数。
        num_env: u8,
    },
    /// 包络数超过表 128 对该间隔类别的上界。
    TooManyEnvelopes {
        /// 传入的包络数。
        num_env: u8,
        /// 表 128 给出的上界。
        limit: u8,
    },
    /// 噪声包络数与信号包络数不匹配。
    NoiseEnvelopeCountMismatch {
        /// 信号包络数。
        num_env: u8,
        /// 传入的噪声包络数。
        num_noise: u8,
        /// 由 `num_env` 推出的噪声包络数。
        expected: u8,
    },
    /// 相对边界数超过 [`MAX_REL_BORDERS`]。
    TooManyRelativeBorders {
        /// 传入的相对边界数。
        count: u8,
    },
    /// 推出的包络边界没有严格递增。
    ///
    /// `Pseudocode 76` 由两端向中间填充，相对边界之和过大时会越过对侧。
    BordersNotIncreasing {
        /// 出问题的边界下标。
        index: u8,
    },
    /// 推出的起始边界为负。
    NegativeStartBorder {
        /// 推出的值。
        border: i16,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::UnsupportedTimeslots { num_aspx_timeslots } => write!(
                f,
                "num_aspx_timeslots {num_aspx_timeslots} is not one of the five values in Table 194"
            ),
            FrameError::UnsupportedFixfixEnvelopes { num_env } => {
                write!(
                    f,
                    "FIXFIX envelope count {num_env} is not 1, 2, or 4 as required by Table 194"
                )
            }
            FrameError::TooManyEnvelopes { num_env, limit } => {
                write!(
                    f,
                    "Envelope count {num_env} exceeds Table 128 limit {limit}"
                )
            }
            FrameError::NoiseEnvelopeCountMismatch {
                num_env,
                num_noise,
                expected,
            } => write!(
                f,
                "Signal envelope count {num_env} requires {expected} noise envelopes, got {num_noise}"
            ),
            FrameError::TooManyRelativeBorders { count } => {
                write!(f, "Relative-border count {count} exceeds {MAX_REL_BORDERS}")
            }
            FrameError::BordersNotIncreasing { index } => {
                write!(
                    f,
                    "Envelope borders are not strictly increasing at index {index}"
                )
            }
            FrameError::NegativeStartBorder { border } => {
                write!(f, "Initial border {border} is negative")
            }
        }
    }
}

impl core::error::Error for FrameError {}

#[cfg(test)]
use crate::spec_tables::aspx::NUM_TS_IN_ATS;
/// 表 194 `tab_border`：FIXFIX 的时隙组边界。
///
/// 五行的 `num_aspx_timeslots` 恰好是 [`super::num_aspx_timeslots`] 的全部
/// 取值；三列对应 `aspx_num_env` 取 1、2、4——上界 4 即表 128 对 FIXFIX 的
/// 限制。部分四分割并非均分，故整表必须从本地 PDF 生成而不能由公式替代。
use crate::spec_tables::aspx::TAB_BORDER;

/// `aspx_framing()` 解析出的、`Pseudocode 76` 所需的全部字段。
///
/// 由语法层填充；本模块只做推导。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspxIntervalParams {
    /// `aspx_int_class`。
    pub int_class: IntervalClass,
    /// `aspx_num_env`。
    pub num_env: u8,
    /// `aspx_num_noise`。
    pub num_noise: u8,
    /// `aspx_var_bord_left`，仅 VARFIX 与 VARVAR 的 I 帧传输。
    pub var_bord_left: Option<u8>,
    /// `aspx_var_bord_right`，仅 FIXVAR 与 VARVAR 传输。
    pub var_bord_right: Option<u8>,
    /// `aspx_num_rel_left`。
    pub num_rel_left: u8,
    /// `aspx_num_rel_right`。
    pub num_rel_right: u8,
    /// `aspx_rel_bord_left`，已按 `2 * tmp + 2` 还原。
    pub rel_bord_left: [u8; MAX_REL_BORDERS],
    /// `aspx_rel_bord_right`，已按 `2 * tmp + 2` 还原。
    pub rel_bord_right: [u8; MAX_REL_BORDERS],
    /// `aspx_tsg_ptr`，已减一，故 −1 表示未指向任何边界。
    pub tsg_ptr: i8,
    /// `aspx_freq_res`。`aspx_freq_res_mode` 取 0 时，FIXFIX 仅第 0 项有效并
    /// 复制到其余包络，其他间隔类别的前 [`Self::num_env`] 项逐包络传输。
    pub freq_res: [bool; MAX_ATSG_SIG],
}

impl AspxIntervalParams {
    /// 一个 FIXFIX、单包络的最小实例，供测试与占位使用。
    #[must_use]
    pub const fn fixfix(num_env: u8) -> Self {
        Self {
            int_class: IntervalClass::FixFix,
            num_env,
            num_noise: if num_env > 1 { 2 } else { 1 },
            var_bord_left: None,
            var_bord_right: None,
            num_rel_left: 0,
            num_rel_right: 0,
            rel_bord_left: [0; MAX_REL_BORDERS],
            rel_bord_right: [0; MAX_REL_BORDERS],
            tsg_ptr: -1,
            freq_res: [false; MAX_ATSG_SIG],
        }
    }
}

/// 一个 A-SPX 间隔的时隙组边界与逐包络频率分辨率。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspxInterval {
    sig: [i16; MAX_ATSG_SIG + 1],
    num_atsg_sig: u8,
    noise: [i16; MAX_ATSG_NOISE + 1],
    num_atsg_noise: u8,
    freq_res: [bool; MAX_ATSG_SIG],
    /// 推导本区间时采用的名义 A-SPX 时隙数。
    source_num_aspx_timeslots: u8,
}

impl AspxInterval {
    /// 一个未填充的实例。
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            sig: [0; MAX_ATSG_SIG + 1],
            num_atsg_sig: 0,
            noise: [0; MAX_ATSG_NOISE + 1],
            num_atsg_noise: 0,
            freq_res: [false; MAX_ATSG_SIG],
            source_num_aspx_timeslots: 0,
        }
    }

    /// `Pseudocode 76`：推出信号与噪声包络的时隙组边界。
    ///
    /// `previous_stop_pos` 是上一个 A-SPX 间隔的终止边界，`5.7.6.3.3.1` 规定
    /// 初值为 `num_aspx_timeslots`；VARFIX 与 VARVAR 在非 I 帧用它替代
    /// `aspx_var_bord_left`。
    ///
    /// # Errors
    ///
    /// 见 [`FrameError`]。
    pub fn derive(
        params: &AspxIntervalParams,
        num_aspx_timeslots: u8,
        freq_res_mode: u8,
        b_iframe: bool,
        previous_stop_pos: i16,
    ) -> Result<Self, FrameError> {
        ensure_supported_timeslots(num_aspx_timeslots)?;
        ensure_envelope_count(params)?;

        let mut out = Self::empty();
        out.num_atsg_sig = params.num_env;
        out.num_atsg_noise = params.num_noise;
        out.source_num_aspx_timeslots = num_aspx_timeslots;

        if params.int_class == IntervalClass::FixFix {
            out.derive_fixfix(params, num_aspx_timeslots)?;
        } else {
            out.derive_variable(params, num_aspx_timeslots, b_iframe, previous_stop_pos)?;
        }

        out.check_monotonic()?;
        out.derive_freq_res(params, num_aspx_timeslots, freq_res_mode);
        Ok(out)
    }

    /// FIXFIX：信号与噪声边界都直接查表 194。
    fn derive_fixfix(
        &mut self,
        params: &AspxIntervalParams,
        num_aspx_timeslots: u8,
    ) -> Result<(), FrameError> {
        let sig = tab_border(num_aspx_timeslots, params.num_env)?;
        for (slot, &value) in self.sig.iter_mut().zip(sig.iter()) {
            *slot = i16::from(value);
        }
        let noise = tab_border(num_aspx_timeslots, params.num_noise)?;
        for (slot, &value) in self.noise.iter_mut().zip(noise.iter()) {
            *slot = i16::from(value);
        }
        Ok(())
    }

    /// FIXVAR／VARFIX／VARVAR：两端定界后由相对边界向中间填充。
    fn derive_variable(
        &mut self,
        params: &AspxIntervalParams,
        num_aspx_timeslots: u8,
        b_iframe: bool,
        previous_stop_pos: i16,
    ) -> Result<(), FrameError> {
        if params.num_rel_left as usize > MAX_REL_BORDERS {
            return Err(FrameError::TooManyRelativeBorders {
                count: params.num_rel_left,
            });
        }
        if params.num_rel_right as usize > MAX_REL_BORDERS {
            return Err(FrameError::TooManyRelativeBorders {
                count: params.num_rel_right,
            });
        }

        let slots = i16::from(num_aspx_timeslots);
        let last = usize::from(params.num_env);

        // 左端：FIXVAR 固定为 0；VARFIX 与 VARVAR 在 I 帧取 aspx_var_bord_left，
        // 否则续用上一间隔的终止边界。
        let start = match params.int_class {
            IntervalClass::FixVar => 0,
            _ => {
                if b_iframe {
                    i16::from(params.var_bord_left.unwrap_or(0))
                } else {
                    previous_stop_pos.saturating_sub(slots)
                }
            }
        };
        if start < 0 {
            return Err(FrameError::NegativeStartBorder { border: start });
        }

        // 右端：VARFIX 固定为 num_aspx_timeslots；其余为其加上 var_bord_right。
        let stop = match params.int_class {
            IntervalClass::VarFix => slots,
            _ => slots.saturating_add(i16::from(params.var_bord_right.unwrap_or(0))),
        };

        if let Some(slot) = self.sig.first_mut() {
            *slot = start;
        }
        if let Some(slot) = self.sig.get_mut(last) {
            *slot = stop;
        }

        // 左侧相对边界自起点向右累加。
        for tsg in 0..usize::from(params.num_rel_left) {
            let previous = self.sig.get(tsg).copied().unwrap_or(0);
            let step = params.rel_bord_left.get(tsg).copied().unwrap_or(0);
            if let Some(slot) = self.sig.get_mut(tsg.saturating_add(1)) {
                *slot = previous.saturating_add(i16::from(step));
            }
        }
        // 右侧相对边界自终点向左递减。
        for tsg in 0..usize::from(params.num_rel_right) {
            let upper = last.saturating_sub(tsg);
            let previous = self.sig.get(upper).copied().unwrap_or(0);
            let step = params.rel_bord_right.get(tsg).copied().unwrap_or(0);
            if let Some(slot) = self.sig.get_mut(upper.saturating_sub(1)) {
                *slot = previous.saturating_sub(i16::from(step));
            }
        }

        // 噪声包络与信号包络共用两端；中间那条由表 193 定位。
        if let Some(slot) = self.noise.first_mut() {
            *slot = start;
        }
        let noise_last = usize::from(params.num_noise);
        if let Some(slot) = self.noise.get_mut(noise_last) {
            *slot = stop;
        }
        if params.num_noise > 1 {
            let index = noise_mid_border(params.int_class, params.tsg_ptr, params.num_env);
            let border = self.sig.get(usize::from(index)).copied().unwrap_or(0);
            if let Some(slot) = self.noise.get_mut(1) {
                *slot = border;
            }
        }
        Ok(())
    }

    /// 两组边界都必须严格递增，否则相对边界越过了对侧。
    fn check_monotonic(&self) -> Result<(), FrameError> {
        if let Some(&first) = self.sig.first()
            && first < 0
        {
            return Err(FrameError::NegativeStartBorder { border: first });
        }
        for index in 0..usize::from(self.num_atsg_sig) {
            let low = self.sig.get(index).copied().unwrap_or(0);
            let high = self.sig.get(index.saturating_add(1)).copied().unwrap_or(0);
            if low >= high {
                return Err(FrameError::BordersNotIncreasing {
                    index: u8::try_from(index).unwrap_or(u8::MAX),
                });
            }
        }
        for index in 0..usize::from(self.num_atsg_noise) {
            let low = self.noise.get(index).copied().unwrap_or(0);
            let high = self
                .noise
                .get(index.saturating_add(1))
                .copied()
                .unwrap_or(0);
            if low >= high {
                return Err(FrameError::BordersNotIncreasing {
                    index: u8::try_from(index).unwrap_or(u8::MAX),
                });
            }
        }
        Ok(())
    }

    /// `Pseudocode 77`：逐包络定出频率分辨率。
    fn derive_freq_res(
        &mut self,
        params: &AspxIntervalParams,
        num_aspx_timeslots: u8,
        freq_res_mode: u8,
    ) {
        if params.int_class == IntervalClass::FixFix {
            let value = match freq_res_mode {
                0 => params.freq_res.first().copied().unwrap_or(false),
                1 => false,
                2 => self.freq_res_by_duration(0, 0, num_aspx_timeslots),
                _ => true,
            };
            for slot in self
                .freq_res
                .iter_mut()
                .take(usize::from(self.num_atsg_sig))
            {
                *slot = value;
            }
            return;
        }

        for atsg in 0..usize::from(self.num_atsg_sig) {
            let value = match freq_res_mode {
                0 => params.freq_res.get(atsg).copied().unwrap_or(false),
                1 => false,
                2 => self.freq_res_by_duration(params.tsg_ptr, atsg, num_aspx_timeslots),
                _ => true,
            };
            if let Some(slot) = self.freq_res.get_mut(atsg) {
                *slot = value;
            }
        }
    }

    /// `Pseudocode 77` 的 `case 2`，用整数比较代替浮点。
    ///
    /// 原式为 `span > num_aspx_timeslots / 6.0 + 3.25`；两边乘 12 得
    /// `12 * span > 2 * num_aspx_timeslots + 39`，右端恒为整数，故无需浮点。
    fn freq_res_by_duration(&self, tsg_ptr: i8, atsg: usize, num_aspx_timeslots: u8) -> bool {
        if tsg_ptr > 0
            && num_aspx_timeslots > 8
            && i16::try_from(atsg).unwrap_or(i16::MAX) < i16::from(tsg_ptr)
        {
            return true;
        }
        let low = self.sig.get(atsg).copied().unwrap_or(0);
        let high = self.sig.get(atsg.saturating_add(1)).copied().unwrap_or(0);
        let span = i32::from(high.saturating_sub(low));
        let threshold = i32::from(num_aspx_timeslots)
            .saturating_mul(2)
            .saturating_add(39);
        span.saturating_mul(12) > threshold
    }

    /// 信号包络数 `num_atsg_sig`。
    #[must_use]
    pub const fn num_atsg_sig(&self) -> u8 {
        self.num_atsg_sig
    }

    /// 噪声包络数 `num_atsg_noise`。
    #[must_use]
    pub const fn num_atsg_noise(&self) -> u8 {
        self.num_atsg_noise
    }

    /// 推导本区间时采用的名义 `num_aspx_timeslots`。
    ///
    /// 可变右边界会让 [`Self::stop_pos`] 大于该值，故下游不能从终止边界反推
    /// 帧长；需要用这个来源值与表 189/192 的 QMF 时隙数重新配对。
    #[cfg(feature = "audio-decode")]
    #[must_use]
    pub(super) const fn source_num_aspx_timeslots(&self) -> u8 {
        self.source_num_aspx_timeslots
    }

    /// `atsg_sig[index]`，`index` 取 0 至 `num_atsg_sig`（含）。
    #[must_use]
    pub fn sig_border(&self, index: usize) -> Option<i16> {
        if self.num_atsg_sig == 0 || index > usize::from(self.num_atsg_sig) {
            return None;
        }
        self.sig.get(index).copied()
    }

    /// `atsg_noise[index]`，`index` 取 0 至 `num_atsg_noise`（含）。
    #[must_use]
    pub fn noise_border(&self, index: usize) -> Option<i16> {
        if self.num_atsg_noise == 0 || index > usize::from(self.num_atsg_noise) {
            return None;
        }
        self.noise.get(index).copied()
    }

    /// `atsg_freqres[atsg]`：真为高频分辨率。
    ///
    /// `aspx_ec_data()` 据此在 `num_sbg_sig_highres` 与 `num_sbg_sig_lowres`
    /// 之间选择，见 `Pseudocode 78`。
    #[must_use]
    pub fn freq_res(&self, atsg: usize) -> Option<bool> {
        if atsg >= usize::from(self.num_atsg_sig) {
            return None;
        }
        self.freq_res.get(atsg).copied()
    }

    /// 本间隔的终止边界，供下一个间隔作 `previous_stop_pos`。
    #[must_use]
    pub fn stop_pos(&self) -> i16 {
        self.sig
            .get(usize::from(self.num_atsg_sig))
            .copied()
            .unwrap_or(0)
    }
}

/// 表 194 `tab_border` 的查表。
fn tab_border(num_aspx_timeslots: u8, num_env: u8) -> Result<&'static [u8], FrameError> {
    let row = TAB_BORDER
        .iter()
        .find(|&&(slots, _, _, _)| slots == num_aspx_timeslots)
        .ok_or(FrameError::UnsupportedTimeslots { num_aspx_timeslots })?;
    let (_, one, two, four) = row;
    match num_env {
        1 => Ok(one),
        2 => Ok(two),
        4 => Ok(four),
        _ => Err(FrameError::UnsupportedFixfixEnvelopes { num_env }),
    }
}

/// 表 193 `noise_mid_border`：中间那条噪声边界落在哪个信号边界上。
fn noise_mid_border(int_class: IntervalClass, tsg_ptr: i8, num_atsg_sig: u8) -> u8 {
    let last = num_atsg_sig.saturating_sub(1);
    match (int_class, tsg_ptr) {
        (IntervalClass::VarFix, ptr) if ptr < 0 => 1,
        (IntervalClass::VarFix, _) => last,
        (_, ptr) if ptr < 0 => last,
        (_, ptr) => {
            let value = u8::try_from(ptr).unwrap_or(0);
            value.min(last).max(1)
        }
    }
}

/// 表 128：各间隔类别的包络数上界。
const fn envelope_limit(int_class: IntervalClass) -> u8 {
    match int_class {
        IntervalClass::FixFix => 4,
        IntervalClass::FixVar | IntervalClass::VarFix | IntervalClass::VarVar => 5,
    }
}

fn ensure_supported_timeslots(num_aspx_timeslots: u8) -> Result<(), FrameError> {
    if TAB_BORDER
        .iter()
        .any(|&(slots, _, _, _)| slots == num_aspx_timeslots)
    {
        return Ok(());
    }
    Err(FrameError::UnsupportedTimeslots { num_aspx_timeslots })
}

fn ensure_envelope_count(params: &AspxIntervalParams) -> Result<(), FrameError> {
    let limit = envelope_limit(params.int_class);
    if params.num_env == 0 || params.num_env > limit {
        return Err(FrameError::TooManyEnvelopes {
            num_env: params.num_env,
            limit,
        });
    }
    if params.num_noise == 0 || usize::from(params.num_noise) > MAX_ATSG_NOISE {
        return Err(FrameError::TooManyEnvelopes {
            num_env: params.num_noise,
            limit: MAX_ATSG_NOISE as u8,
        });
    }
    let expected = if params.num_env > 1 { 2 } else { 1 };
    if params.num_noise != expected {
        return Err(FrameError::NoiseEnvelopeCountMismatch {
            num_env: params.num_env,
            num_noise: params.num_noise,
            expected,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aspx::num_aspx_timeslots;

    /// 表 194 的五行恰好覆盖 `num_aspx_timeslots` 的全部取值。
    ///
    /// 两张表本无关联——一个来自表 189/192 的换算，一个是 FIXFIX 的边界表
    /// ——取值集合却完全重合，因此彼此都是对方的转录判据。
    #[test]
    fn tab_border_rows_cover_every_reachable_timeslot_count() {
        let mut seen = [false; 5];
        for &(frame_len_base, _, _) in &NUM_TS_IN_ATS {
            let Some(slots) = num_aspx_timeslots(frame_len_base) else {
                panic!("frame_len_base {frame_len_base} 应有时隙数");
            };
            let Some(index) = TAB_BORDER
                .iter()
                .position(|&(value, _, _, _)| value == slots)
            else {
                panic!("时隙数 {slots} 不在表 194 内");
            };
            if let Some(slot) = seen.get_mut(index) {
                *slot = true;
            }
        }
        assert!(seen.iter().all(|&hit| hit), "表 194 有行不可达");
    }

    /// 表 194 每行每列都以 0 起、以时隙数止，且严格递增。
    #[test]
    fn tab_border_rows_span_the_whole_interval() {
        for &(slots, ref one, ref two, ref four) in &TAB_BORDER {
            for (num_env, borders) in [
                (1u8, one.as_slice()),
                (2, two.as_slice()),
                (4, four.as_slice()),
            ] {
                assert_eq!(
                    borders.len(),
                    usize::from(num_env).saturating_add(1),
                    "时隙 {slots} 的 {num_env} 包络应有 num_env+1 条边界"
                );
                assert_eq!(borders.first(), Some(&0), "首项应为 0");
                assert_eq!(borders.last(), Some(&slots), "末项应为 num_aspx_timeslots");
                for pair in borders.windows(2) {
                    let (Some(low), Some(high)) = (pair.first(), pair.get(1)) else {
                        unreachable!("windows(2) 必有两项");
                    };
                    assert!(low < high, "时隙 {slots} 的边界未严格递增");
                }
            }
        }
    }

    /// FIXFIX 的全部合法组合都能推出边界，且噪声边界是信号边界的子集。
    #[test]
    fn fixfix_noise_borders_are_a_subset_of_the_signal_borders() {
        for &(slots, _, _, _) in &TAB_BORDER {
            for num_env in [1u8, 2, 4] {
                let params = AspxIntervalParams::fixfix(num_env);
                let Ok(interval) = AspxInterval::derive(&params, slots, 1, true, i16::from(slots))
                else {
                    panic!("时隙 {slots} 的 {num_env} 包络应可推导");
                };
                assert_eq!(interval.num_atsg_sig(), num_env);
                assert_eq!(interval.sig_border(0), Some(0));
                assert_eq!(
                    interval.sig_border(usize::from(num_env)),
                    Some(i16::from(slots))
                );
                for index in 0..=usize::from(interval.num_atsg_noise()) {
                    let Some(border) = interval.noise_border(index) else {
                        panic!("噪声边界 {index} 缺失");
                    };
                    assert!(
                        (0..=usize::from(num_env)).any(|i| interval.sig_border(i) == Some(border)),
                        "噪声边界 {border} 不在信号边界内"
                    );
                }
            }
        }
    }

    /// 表 128：FIXFIX 至多 4 个包络，其余类别至多 5 个。
    #[test]
    fn envelope_limits_follow_table_128() {
        let params = AspxIntervalParams::fixfix(8);
        assert_eq!(
            AspxInterval::derive(&params, 16, 1, true, 16),
            Err(FrameError::TooManyEnvelopes {
                num_env: 8,
                limit: 4
            })
        );

        let mut varvar = AspxIntervalParams::fixfix(6);
        varvar.int_class = IntervalClass::VarVar;
        assert_eq!(
            AspxInterval::derive(&varvar, 16, 1, true, 16),
            Err(FrameError::TooManyEnvelopes {
                num_env: 6,
                limit: 5
            })
        );
    }

    /// VARVAR：左右相对边界从两端向中间填充，结果必须严格递增。
    #[test]
    fn varvar_fills_from_both_ends() {
        let mut params = AspxIntervalParams::fixfix(3);
        params.int_class = IntervalClass::VarVar;
        params.var_bord_left = Some(2);
        params.var_bord_right = Some(1);
        params.num_rel_left = 1;
        params.num_rel_right = 1;
        params.rel_bord_left = [4, 0, 0];
        params.rel_bord_right = [6, 0, 0];
        params.tsg_ptr = 1;

        let Ok(interval) = AspxInterval::derive(&params, 16, 1, true, 16) else {
            panic!("该组合应可推导");
        };
        // 起点 2；左侧加 4 得 6；终点 16+1=17；右侧减 6 得 11。
        assert_eq!(interval.sig_border(0), Some(2));
        assert_eq!(interval.sig_border(1), Some(6));
        assert_eq!(interval.sig_border(2), Some(11));
        assert_eq!(interval.sig_border(3), Some(17));
        assert_eq!(interval.stop_pos(), 17);
    }

    /// 非 I 帧的 VARFIX／VARVAR 用 `previous_stop_pos` 替代左边界。
    #[test]
    fn non_iframe_takes_the_start_from_the_previous_interval() {
        let mut params = AspxIntervalParams::fixfix(1);
        params.int_class = IntervalClass::VarFix;
        params.var_bord_left = Some(3);

        // I 帧读 aspx_var_bord_left。
        let Ok(iframe) = AspxInterval::derive(&params, 16, 1, true, 16) else {
            panic!("I 帧应可推导");
        };
        assert_eq!(iframe.sig_border(0), Some(3));

        // 非 I 帧改用 previous_stop_pos - num_aspx_timeslots，此处 19-16=3。
        let Ok(inter) = AspxInterval::derive(&params, 16, 1, false, 19) else {
            panic!("非 I 帧应可推导");
        };
        assert_eq!(inter.sig_border(0), Some(3));
        assert_eq!(inter.sig_border(1), Some(16), "VARFIX 右端固定");
    }

    /// 相对边界之和越过对侧时必须拒绝，而不是给出倒序的边界。
    #[test]
    fn rejects_borders_that_cross_over() {
        let mut params = AspxIntervalParams::fixfix(2);
        params.int_class = IntervalClass::VarVar;
        params.var_bord_left = Some(0);
        params.var_bord_right = Some(0);
        params.num_rel_left = 1;
        params.num_rel_right = 0;
        params.rel_bord_left = [8, 0, 0];
        params.tsg_ptr = 0;

        // 时隙 6：左边界 0+8=8 越过右端 6。
        assert!(matches!(
            AspxInterval::derive(&params, 6, 1, true, 6),
            Err(FrameError::BordersNotIncreasing { .. })
        ));
    }

    /// FIXFIX 只传输第 0 个频率分辨率，并将其复制到后续包络。
    #[test]
    fn frequency_resolution_modes_zero_one_and_three() {
        let mut params = AspxIntervalParams::fixfix(2);
        params.freq_res = [true, false, false, false, false];

        let Ok(explicit) = AspxInterval::derive(&params, 16, 0, true, 16) else {
            panic!("模式 0 应可推导");
        };
        assert_eq!(explicit.freq_res(0), Some(true));
        assert_eq!(explicit.freq_res(1), Some(true));

        let Ok(low) = AspxInterval::derive(&params, 16, 1, true, 16) else {
            panic!("模式 1 应可推导");
        };
        assert_eq!(low.freq_res(0), Some(false));

        let Ok(high) = AspxInterval::derive(&params, 16, 3, true, 16) else {
            panic!("模式 3 应可推导");
        };
        assert_eq!(high.freq_res(0), Some(true));
        assert_eq!(high.freq_res(2), None, "包络数之外不得暴露");
    }

    /// 噪声包络数由信号包络数唯一决定，两个方向的不匹配都必须拒绝。
    #[test]
    fn rejects_noise_count_inconsistent_with_signal_envelopes() {
        let mut single = AspxIntervalParams::fixfix(1);
        single.num_noise = 2;
        assert_eq!(
            AspxInterval::derive(&single, 16, 1, true, 16),
            Err(FrameError::NoiseEnvelopeCountMismatch {
                num_env: 1,
                num_noise: 2,
                expected: 1,
            })
        );

        let mut multiple = AspxIntervalParams::fixfix(2);
        multiple.num_noise = 1;
        assert_eq!(
            AspxInterval::derive(&multiple, 16, 1, true, 16),
            Err(FrameError::NoiseEnvelopeCountMismatch {
                num_env: 2,
                num_noise: 1,
                expected: 2,
            })
        );
    }

    /// 模式 2 的整数比较必须与 `span > n/6 + 3,25` 的浮点定义逐点一致。
    #[test]
    fn duration_threshold_matches_the_floating_point_definition() {
        extern crate std;
        for &(slots, _, _, _) in &TAB_BORDER {
            for span in 0i16..=24 {
                let exact = f64::from(span) > f64::from(slots) / 6.0 + 3.25;
                let threshold = i32::from(slots).saturating_mul(2).saturating_add(39);
                let integer = i32::from(span).saturating_mul(12) > threshold;
                assert_eq!(integer, exact, "时隙 {slots} 跨度 {span} 的阈值判断不一致");
            }
        }
    }

    /// 模式 2 下，单包络铺满整个间隔时跨度足够大，判为高分辨率。
    #[test]
    fn duration_mode_marks_long_envelopes_as_high_resolution() {
        let params = AspxIntervalParams::fixfix(1);
        // 时隙 16 的单包络跨度为 16，阈值为 16/6+3,25 ≈ 5,92。
        let Ok(long) = AspxInterval::derive(&params, 16, 2, true, 16) else {
            panic!("应可推导");
        };
        assert_eq!(long.freq_res(0), Some(true));

        // 四包络时跨度为 4，低于阈值。
        let short = AspxIntervalParams::fixfix(4);
        let Ok(interval) = AspxInterval::derive(&short, 16, 2, true, 16) else {
            panic!("应可推导");
        };
        for atsg in 0..4 {
            assert_eq!(interval.freq_res(atsg), Some(false), "包络 {atsg}");
        }
    }

    /// 表 193：VARFIX 与其余类别的中间噪声边界取法相反。
    #[test]
    fn noise_mid_border_follows_table_193() {
        // VARFIX：tsg_ptr < 0 取 1，否则取 num_atsg_sig-1。
        assert_eq!(noise_mid_border(IntervalClass::VarFix, -1, 4), 1);
        assert_eq!(noise_mid_border(IntervalClass::VarFix, 2, 4), 3);
        // FIXVAR/VARVAR：tsg_ptr < 0 取 num_atsg_sig-1，否则夹在 1 与它之间。
        assert_eq!(noise_mid_border(IntervalClass::FixVar, -1, 4), 3);
        assert_eq!(noise_mid_border(IntervalClass::VarVar, 0, 4), 1);
        assert_eq!(noise_mid_border(IntervalClass::VarVar, 2, 4), 2);
        assert_eq!(noise_mid_border(IntervalClass::VarVar, 9, 4), 3);
    }

    /// 不可达的时隙数必须拒绝，而不是套用相邻行。
    #[test]
    fn rejects_timeslot_counts_outside_table_194() {
        let params = AspxIntervalParams::fixfix(1);
        for slots in [0u8, 5, 7, 9, 14, 17, 32] {
            assert_eq!(
                AspxInterval::derive(&params, slots, 1, true, 16),
                Err(FrameError::UnsupportedTimeslots {
                    num_aspx_timeslots: slots
                })
            );
        }
    }

    /// FIXFIX 的包络数只有 1、2、4 三种，3 与 5 在表 194 中没有列。
    #[test]
    fn rejects_fixfix_envelope_counts_absent_from_table_194() {
        let num_env = 3u8;
        let params = AspxIntervalParams::fixfix(num_env);
        assert_eq!(
            AspxInterval::derive(&params, 16, 1, true, 16),
            Err(FrameError::UnsupportedFixfixEnvelopes { num_env })
        );
    }

    /// 未填充实例的公开查询一律为空。
    #[test]
    fn empty_interval_exposes_no_borders() {
        let interval = AspxInterval::empty();
        assert_eq!(interval.num_atsg_sig(), 0);
        assert_eq!(interval.num_atsg_noise(), 0);
        for index in [0, 1] {
            assert_eq!(interval.sig_border(index), None);
            assert_eq!(interval.noise_border(index), None);
            assert_eq!(interval.freq_res(index), None);
        }
    }
}
