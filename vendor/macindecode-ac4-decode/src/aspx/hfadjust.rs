//! HF 包络调整：当前区间的包络估计（`TS103190-1:v1.4.1:5.7.6.4.2.1`）。
//!
//! `Pseudocode 90`–`94` 把 `Q_high` 的实际包络、传输来的标度因子、正弦标记与
//! 由它们算出的正弦/噪声电平，全部铺到「QMF 子带 × 信号包络」这张矩阵上，供
//! `5.7.6.4.2.2` 的补偿增益使用。
//!
//! # `5.7.6.3.3.2` 的 tiling 在这里第一次用上
//!
//! `sbg_sig[atsg]` 按该包络的 `atsg_freqres` 在高低分辨率两张表之间选一张。组数
//! 这一维此前由包络解码逐包络携带，边界表这一维一直没有消费者——`Pseudocode 90`、
//! `91`、`93` 三处都要它。
//!
//! # 一处规范笔误
//!
//! `Pseudocode 91` 写 `if (atsg_sig[atsg] == atsg_noise[atsg_noise + 1])`，
//! `atsg_noise` 同时充当循环变量与数组名。按上下文，右侧应是噪声包络的时间
//! 边界表，即 [`AspxInterval::noise_border`]。
//!
//! # `b_sine_at_end` 不是笔误
//!
//! `Pseudocode 95` 定义 `b_sine_at_end` 却不在本节读它，条件里读的是
//! `Pseudocode 92` 的 `p_sine_at_end`。两者算式相同、所属区间不同：`b_` 由**本**
//! 区间的 `aspx_tsg_ptr` 与 `num_atsg_sig` 定出，唯一用途是成为**下一**区间的
//! `p_`。[`SineState`] 携带的正是这个跨区间值。
//!
//! 因此它有一处顺序陷阱：[`estimate`] 末尾会把本区间的 `b_sine_at_end` 写进
//! [`SineState`]，`5.7.6.4.2.2` 再去读就拿到本区间的值而非上一区间的。为让下游
//! 取不到被推进过的状态，判据在推进**之前**就固化进
//! [`EnvelopeEstimate::sine_onset`]，`5.7.6.4.2.2` 只读那里。
//!
//! # `aspx_tsg_ptr` 已经减一
//!
//! 表 53 在语法层直接定义 `aspx_tsg_ptr = tmp - 1`，所以后续伪码使用的就是
//! [`super::frames::AspxIntervalParams::tsg_ptr`] 保存的有符号值：`−1` 是原始码字
//! `0`，`0` 指向第 0 个包络，等于 `num_atsg_sig` 才表示指针落在区间末尾。
//!
//! # 时间归一化取 QMF 区域平均
//!
//! `Pseudocode 90` 的累加区间乘了 `num_ts_in_ats`，分母却漏掉同一倍率。若按
//! 该伪码字面执行，倍率为 2 时每个传输标度因子会对应两个 QMF 时隙的能量之和，
//! 与 `5.7.6.4.2.1` 对 signal scale factor 的「QMF 区域平均能量」定义和正文、
//! `Pseudocode 85` 的区域平均，以及 `Pseudocode 94`/`95` 的能量配平关系冲突。
//! 因此这里把分母也换算到 QMF 时隙域，按实际累加的复样本数取平均。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "下标由 64 个子带、表 128 的包络数上界与已校验的边界表派生；\
              伪码的每重循环都同时索引多张「子带 × 包络」矩阵，换成迭代器要\
              先 zip 起七张表，比原文更难核对"
)]

use crate::aspx::bands::AspxBandTables;
use crate::aspx::dequant::ScaleFactors;
use crate::aspx::frames::{AspxInterval, MAX_ATSG_SIG};
use crate::aspx::qmf::QmfSlot;
use crate::aspx::tables::NUM_QMF_SUBBANDS;
use macindecode_ac4_bitstream::math::sqrt_f32;

/// 子带数，`5.7.3.2` 规定恒为 64。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// 包络估计无法计算的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HfAdjustError {
    /// 信号包络数为零或超出表 128 的上界。
    EnvelopeCountOutOfRange { envelopes: usize },
    /// A-SPX 范围超出 64 个 QMF 子带。
    SubbandOutOfRange { sbx: usize, num_sb_aspx: usize },
    /// `num_ts_in_ats` 不是表 192 定义的 1 或 2。
    TimeslotFactorOutOfRange { factor: u8 },
    /// `aspx_add_harmonic` 没有覆盖全部高分辨率子带组。
    HarmonicDataTooShort { needed: usize, provided: usize },
    /// 包络的时隙区间为空或首尾颠倒。
    EmptyEnvelope { envelope: usize },
    /// `Q_high` 短于包络覆盖的时隙。
    HighBandTooShort { needed: usize, provided: usize },
    /// 子带组边界表缺项或不覆盖 A-SPX 范围。
    BorderTableMismatch { envelope: usize, subband: usize },
    /// 标度因子缺少某个子带组。
    MissingScaleFactor { envelope: usize, group: usize },
}

/// `Pseudocode 92`/`95` 共用的正弦位置判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinePlacement {
    /// 表 53 已执行 `tmp - 1` 后的 `aspx_tsg_ptr`。
    pointer: i8,
}

impl SinePlacement {
    /// 保留 [`super::frames::AspxIntervalParams::tsg_ptr`] 的规范表示。
    #[must_use]
    pub const fn from_params(tsg_ptr: i8) -> Self {
        Self { pointer: tsg_ptr }
    }

    /// 供其他模块的判据钉住「该包络不自带正弦」这一夹具前提。
    #[cfg(test)]
    pub(crate) fn starts_by_for_test(&self, envelope: usize) -> bool {
        self.starts_by(envelope)
    }

    /// `atsg >= aspx_tsg_ptr`，保留 `−1` 小于全部包络下标的语义。
    fn starts_by(&self, envelope: usize) -> bool {
        let Ok(envelope) = i16::try_from(envelope) else {
            return true;
        };
        envelope >= i16::from(self.pointer)
    }

    /// 指针是否恰好等于某个包络下标或包络数。
    fn equals(&self, value: usize) -> bool {
        i16::try_from(value) == Ok(i16::from(self.pointer))
    }

    /// `Pseudocode 95`/`99` 的 `atsg == aspx_tsg_ptr`：正弦正好在此包络起始。
    fn starts_at(&self, envelope: usize) -> bool {
        self.equals(envelope)
    }

    /// 当前指针是否恰好落在区间末尾，即 `Pseudocode 95` 的 `b_sine_at_end == 0`。
    fn is_at_end(&self, envelopes: usize) -> bool {
        self.equals(envelopes)
    }
}

/// 跨 A-SPX 区间的正弦状态。
///
/// `Pseudocode 92` 只读上一区间的**最后一个**包络列
/// （`sine_idx_sb_prev[sb][num_atsg_sig_prev-1]`），因此这里不保存整张矩阵。
/// 规范另说明：当前区间的子带范围更大时，上一区间未覆盖的子带按零处理——全零
/// 初值与之一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SineState {
    last_column: [bool; SUBBANDS],
    placement_at_end: bool,
}

impl SineState {
    /// 建立空状态：无正弦，指针不在末尾。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_column: [false; SUBBANDS],
            placement_at_end: false,
        }
    }

    /// 构造一个非初值状态，供聚合状态的重置判据使用。
    #[cfg(test)]
    pub(crate) fn mark_non_default_for_test(&mut self) {
        self.last_column[0] = true;
        self.placement_at_end = true;
    }
}

impl Default for SineState {
    fn default() -> Self {
        Self::new()
    }
}

/// `5.7.6.4.2.1` 产出的七张「子带 × 包络」矩阵。
#[derive(Debug, Clone, PartialEq)]
pub struct EnvelopeEstimate {
    est_sig: [[f32; MAX_ATSG_SIG]; SUBBANDS],
    scf_sig: [[f32; MAX_ATSG_SIG]; SUBBANDS],
    scf_noise: [[f32; MAX_ATSG_SIG]; SUBBANDS],
    sine_idx: [[bool; MAX_ATSG_SIG]; SUBBANDS],
    sine_area: [[bool; MAX_ATSG_SIG]; SUBBANDS],
    sine_lev: [[f32; MAX_ATSG_SIG]; SUBBANDS],
    noise_lev: [[f32; MAX_ATSG_SIG]; SUBBANDS],
    /// 本区间的 `aspx_tsg_ptr`，供 `5.7.6.4.2.2` 的 `atsg == aspx_tsg_ptr`。
    placement: SinePlacement,
    /// 上一区间的 `p_sine_at_end == 0`，在 [`SineState`] 被推进**之前**取。
    prev_at_end: bool,
    /// 生成这些矩阵的完整频带布局，防止同宽但不同交叉子带/分组的结果被错接。
    source_bands: AspxBandTables,
    /// 生成这些矩阵的完整时间布局，供后续消费者拒绝同包络数的外来区间。
    source_interval: AspxInterval,
    /// 估计 `Q_high` 时采用的每 ATS QMF 时隙数。
    source_num_ts_in_ats: u8,
    subbands: u8,
    envelopes: u8,
}

impl EnvelopeEstimate {
    /// 建立空结果。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            est_sig: [[0.0; MAX_ATSG_SIG]; SUBBANDS],
            scf_sig: [[0.0; MAX_ATSG_SIG]; SUBBANDS],
            scf_noise: [[0.0; MAX_ATSG_SIG]; SUBBANDS],
            sine_idx: [[false; MAX_ATSG_SIG]; SUBBANDS],
            sine_area: [[false; MAX_ATSG_SIG]; SUBBANDS],
            sine_lev: [[0.0; MAX_ATSG_SIG]; SUBBANDS],
            noise_lev: [[0.0; MAX_ATSG_SIG]; SUBBANDS],
            placement: SinePlacement { pointer: -1 },
            prev_at_end: false,
            source_bands: AspxBandTables::empty(),
            source_interval: AspxInterval::empty(),
            source_num_ts_in_ats: 0,
            subbands: 0,
            envelopes: 0,
        }
    }

    /// A-SPX 范围内的子带数，即 `num_sb_aspx`。
    #[must_use]
    pub const fn subbands(&self) -> u8 {
        self.subbands
    }

    /// 信号包络数，即 `num_atsg_sig`。
    #[must_use]
    pub const fn envelopes(&self) -> u8 {
        self.envelopes
    }

    fn in_range(&self, sb: usize, atsg: usize) -> bool {
        sb < usize::from(self.subbands) && atsg < usize::from(self.envelopes)
    }

    /// `est_sig_sb[sb][atsg]`，`Q_high` 的实际包络。
    #[must_use]
    pub fn estimated(&self, sb: usize, atsg: usize) -> Option<f32> {
        self.in_range(sb, atsg).then(|| self.est_sig[sb][atsg])
    }

    /// `scf_sig_sb[sb][atsg]`。
    #[must_use]
    pub fn signal_scale(&self, sb: usize, atsg: usize) -> Option<f32> {
        self.in_range(sb, atsg).then(|| self.scf_sig[sb][atsg])
    }

    /// `scf_noise_sb[sb][atsg]`。
    #[must_use]
    pub fn noise_scale(&self, sb: usize, atsg: usize) -> Option<f32> {
        self.in_range(sb, atsg).then(|| self.scf_noise[sb][atsg])
    }

    /// `sine_idx_sb[sb][atsg]`：该子带正中是否插入正弦。
    #[must_use]
    pub fn sine_index(&self, sb: usize, atsg: usize) -> Option<bool> {
        self.in_range(sb, atsg).then(|| self.sine_idx[sb][atsg])
    }

    /// `sine_area_sb[sb][atsg]`：该子带所在的子带组内是否有正弦。
    #[must_use]
    pub fn sine_area(&self, sb: usize, atsg: usize) -> Option<bool> {
        self.in_range(sb, atsg).then(|| self.sine_area[sb][atsg])
    }

    /// `sine_lev_sb[sb][atsg]`。
    #[must_use]
    pub fn sine_level(&self, sb: usize, atsg: usize) -> Option<f32> {
        self.in_range(sb, atsg).then(|| self.sine_lev[sb][atsg])
    }

    /// `noise_lev_sb[sb][atsg]`。
    #[must_use]
    pub fn noise_level(&self, sb: usize, atsg: usize) -> Option<f32> {
        self.in_range(sb, atsg).then(|| self.noise_lev[sb][atsg])
    }

    /// `Pseudocode 95`/`99` 的 `atsg == aspx_tsg_ptr || atsg == p_sine_at_end`。
    ///
    /// 两个下标各属不同区间：左边是本区间的指针，右边是上一区间的指针落在其末尾
    /// 时留下的 `0`（`p_sine_at_end` 只取 `0` 或 `−1`，故右边只可能在第 0 个包络
    /// 成立）。[`estimate`] 在推进 [`SineState`] 之前取值，`5.7.6.4.2.2` 因此拿不
    /// 到被推进过的状态。
    #[must_use]
    pub fn sine_onset(&self, atsg: usize) -> Option<bool> {
        (atsg < usize::from(self.envelopes))
            .then(|| self.placement.starts_at(atsg) || (self.prev_at_end && atsg == 0))
    }

    /// 这些矩阵是否由给定的完整频带布局生成。
    pub(super) fn matches_bands(&self, bands: &AspxBandTables) -> bool {
        self.source_bands == *bands
    }

    /// 生成这些矩阵的完整时间布局。
    pub(super) const fn source_interval(&self) -> AspxInterval {
        self.source_interval
    }

    /// 估计 `Q_high` 时采用的每 ATS QMF 时隙数。
    pub(super) const fn source_num_ts_in_ats(&self) -> u8 {
        self.source_num_ts_in_ats
    }
}

impl Default for EnvelopeEstimate {
    fn default() -> Self {
        Self::new()
    }
}

/// `5.7.6.3.3.2` 的 tiling：按包络的频率分辨率选一张信号子带组表。
///
/// 返回 `(组数, 取第 i 条边界的闭包)`。`Pseudocode 78` 只做这一件事，组数那一维
/// 由包络解码逐包络携带，边界表这一维到这里才有消费者。
fn signal_tiling(
    bands: &AspxBandTables,
    high_resolution: bool,
) -> (usize, impl Fn(usize) -> u8 + '_) {
    let groups = if high_resolution {
        usize::from(bands.num_sbg_sig_highres())
    } else {
        usize::from(bands.num_sbg_sig_lowres())
    };
    let border = move |index: usize| {
        if high_resolution {
            bands.sig_highres_border(index).unwrap_or(0)
        } else {
            bands.sig_lowres_border(index).unwrap_or(0)
        }
    };
    (groups, border)
}

/// `Pseudocode 90`–`94`：算出当前区间的全部包络矩阵。
///
/// `q_high` 是 [`super::hfgen`] 的输出，按时隙索引，其第 0 项对应时隙
/// `atsg_sig[0] * num_ts_in_ats`。`scale_factors` 是 `5.7.6.3.5` 的反量化结果。
/// 成功时 `sines` 前进到本区间。
///
/// # Errors
///
/// 见 [`HfAdjustError`]。任一条不成立时都不改写 `out` 与 `sines`。
#[expect(
    clippy::too_many_arguments,
    reason = "`5.7.6.4.2.1` 的输入就是这几路，聚成结构体只是换个地方写"
)]
pub fn estimate(
    q_high: &[QmfSlot],
    bands: &AspxBandTables,
    interval: &AspxInterval,
    scale_factors: &ScaleFactors,
    add_harmonic: &[bool],
    placement: SinePlacement,
    interpolation: bool,
    num_ts_in_ats: u8,
    sines: &mut SineState,
    out: &mut EnvelopeEstimate,
) -> Result<(), HfAdjustError> {
    let envelopes = usize::from(interval.num_atsg_sig());
    if envelopes == 0 || envelopes > MAX_ATSG_SIG {
        return Err(HfAdjustError::EnvelopeCountOutOfRange { envelopes });
    }
    if !matches!(num_ts_in_ats, 1 | 2) {
        return Err(HfAdjustError::TimeslotFactorOutOfRange {
            factor: num_ts_in_ats,
        });
    }
    let sbx = usize::from(bands.sbx());
    let num_sb_aspx = usize::from(bands.num_sb_aspx());
    if sbx + num_sb_aspx > SUBBANDS || num_sb_aspx == 0 {
        return Err(HfAdjustError::SubbandOutOfRange { sbx, num_sb_aspx });
    }
    let highres_groups = usize::from(bands.num_sbg_sig_highres());
    if add_harmonic.len() < highres_groups {
        return Err(HfAdjustError::HarmonicDataTooShort {
            needed: highres_groups,
            provided: add_harmonic.len(),
        });
    }

    // `q_high[0]` 对应 `atsg_sig[0] * num_ts_in_ats`，全部时隙都相对它取下标。
    // 先在 ATS 边界域求差，再换算成 QMF 时隙；边界是 i16，倍率已限为 1/2，
    // 因此乘积在 usize 上不会溢出。
    let first_border = i32::from(interval.sig_border(0).unwrap_or(0));
    let last_border = i32::from(interval.sig_border(envelopes).unwrap_or(0));
    if last_border <= first_border {
        return Err(HfAdjustError::EmptyEnvelope { envelope: 0 });
    }
    let factor = usize::from(num_ts_in_ats);
    let needed = usize::try_from(last_border - first_border).unwrap_or(0) * factor;
    if q_high.len() < needed {
        return Err(HfAdjustError::HighBandTooShort {
            needed,
            provided: q_high.len(),
        });
    }

    // `p_sine_at_end`：上一区间留下的值。下面的推进会覆盖它，故先取住。
    let prev_at_end = sines.placement_at_end;

    let mut est_sig = [[0.0f32; MAX_ATSG_SIG]; SUBBANDS];
    let mut scf_sig = [[0.0f32; MAX_ATSG_SIG]; SUBBANDS];
    let mut scf_noise = [[0.0f32; MAX_ATSG_SIG]; SUBBANDS];
    let mut sine_idx = [[false; MAX_ATSG_SIG]; SUBBANDS];
    let mut sine_area = [[false; MAX_ATSG_SIG]; SUBBANDS];
    let mut sine_lev = [[0.0f32; MAX_ATSG_SIG]; SUBBANDS];
    let mut noise_lev = [[0.0f32; MAX_ATSG_SIG]; SUBBANDS];

    for atsg in 0..envelopes {
        let high_res = interval.freq_res(atsg).unwrap_or(false);
        let (groups, border) = signal_tiling(bands, high_res);
        let tsa = i32::from(interval.sig_border(atsg).unwrap_or(0));
        let tsz = i32::from(interval.sig_border(atsg + 1).unwrap_or(0));
        if tsz <= tsa {
            return Err(HfAdjustError::EmptyEnvelope { envelope: atsg });
        }
        let span = usize::try_from(tsz - tsa).unwrap_or(0);
        let slots = span * factor;
        let base = usize::try_from(tsa - first_border).unwrap_or(0) * factor;

        // `Pseudocode 90`：实际包络。插值关闭时在整个子带组上平均，打开时逐子带。
        let mut group = 0usize;
        for sb in 0..num_sb_aspx {
            if usize::from(border(group + 1)) == sb + sbx && group + 1 < groups {
                group += 1;
            }
            let mut sum = 0.0f64;
            for offset in 0..slots {
                let slot = &q_high[base + offset];
                if interpolation {
                    let sample = slot.re[sb + sbx];
                    let imag = slot.im[sb + sbx];
                    sum +=
                        f64::from(sample) * f64::from(sample) + f64::from(imag) * f64::from(imag);
                } else {
                    for j in usize::from(border(group))..usize::from(border(group + 1)) {
                        let re = slot.re[j];
                        let im = slot.im[j];
                        sum += f64::from(re) * f64::from(re) + f64::from(im) * f64::from(im);
                    }
                }
            }
            // Pseudocode 90 的分母漏了 `num_ts_in_ats`；按本节对 signal scale
            // factor 的区域平均定义，必须除以实际累加的 QMF 时隙数。
            let mut value = sum / slots as f64;
            if !interpolation {
                let width = usize::from(border(group + 1)) - usize::from(border(group));
                if width == 0 {
                    return Err(HfAdjustError::BorderTableMismatch {
                        envelope: atsg,
                        subband: sb,
                    });
                }
                value /= width as f64;
            }
            est_sig[sb][atsg] = value as f32;
        }

        // `Pseudocode 91`：信号标度因子按本包络的分辨率铺开。
        for sbg in 0..groups {
            let low = usize::from(border(sbg));
            let high = usize::from(border(sbg + 1));
            let value = scale_factors
                .sig(atsg, sbg)
                .ok_or(HfAdjustError::MissingScaleFactor {
                    envelope: atsg,
                    group: sbg,
                })?;
            for sb in low.saturating_sub(sbx)..high.saturating_sub(sbx) {
                if sb < num_sb_aspx {
                    scf_sig[sb][atsg] = value;
                }
            }
        }
    }

    // `Pseudocode 91` 的噪声一半：噪声包络比信号包络粗，按时间边界对齐。
    // 伪码此处把循环变量 `atsg_noise` 当数组用，右侧应是噪声包络的时间边界。
    let mut atsg_noise = 0usize;
    let noise_groups = usize::from(bands.num_sbg_noise());
    for atsg in 0..envelopes {
        if atsg_noise < usize::from(interval.num_atsg_noise())
            && interval.sig_border(atsg) == interval.noise_border(atsg_noise + 1)
        {
            atsg_noise += 1;
        }
        for sbg in 0..noise_groups {
            let low = usize::from(bands.noise_border(sbg).unwrap_or(0));
            let high = usize::from(bands.noise_border(sbg + 1).unwrap_or(0));
            let value =
                scale_factors
                    .noise(atsg_noise, sbg)
                    .ok_or(HfAdjustError::MissingScaleFactor {
                        envelope: atsg_noise,
                        group: sbg,
                    })?;
            for sb in low.saturating_sub(sbx)..high.saturating_sub(sbx) {
                if sb < num_sb_aspx {
                    scf_noise[sb][atsg] = value;
                }
            }
        }
    }

    // `Pseudocode 92`：正弦标记放在高分辨率子带组的正中。
    for atsg in 0..envelopes {
        for sbg in 0..highres_groups {
            let low = usize::from(bands.sig_highres_border(sbg).unwrap_or(0));
            let high = usize::from(bands.sig_highres_border(sbg + 1).unwrap_or(0));
            // 相对 sbx 取中点，与伪码的 `0.5*(sbz+sba)` 一致（整数截断）。
            let mid = (low.saturating_sub(sbx) + high.saturating_sub(sbx)) / 2;
            let harmonic = add_harmonic[sbg];
            for sb in low.saturating_sub(sbx)..high.saturating_sub(sbx) {
                if sb >= num_sb_aspx {
                    break;
                }
                let carried = sines.last_column[sb + sbx];
                let active = sb == mid && (placement.starts_by(atsg) || prev_at_end || carried);
                sine_idx[sb][atsg] = active && harmonic;
            }
        }
    }

    // `Pseudocode 93`：正弦标记在本包络的子带组内铺满。
    for atsg in 0..envelopes {
        let high_res = interval.freq_res(atsg).unwrap_or(false);
        let (groups, border) = signal_tiling(bands, high_res);
        for sbg in 0..groups {
            let low = usize::from(border(sbg)).saturating_sub(sbx);
            let high = usize::from(border(sbg + 1)).saturating_sub(sbx);
            let present = (low..high.min(num_sb_aspx)).any(|sb| sine_idx[sb][atsg]);
            for sb in low..high.min(num_sb_aspx) {
                sine_area[sb][atsg] = present;
            }
        }
    }

    // `Pseudocode 94`：正弦与噪声电平。两个 sqrt 都在这里。
    for atsg in 0..envelopes {
        for sb in 0..num_sb_aspx {
            let factor = scf_sig[sb][atsg] / (1.0 + scf_noise[sb][atsg]);
            sine_lev[sb][atsg] = sqrt_f32(factor * f32::from(u8::from(sine_idx[sb][atsg])));
            noise_lev[sb][atsg] = sqrt_f32(factor * scf_noise[sb][atsg]);
        }
    }

    out.est_sig = est_sig;
    out.scf_sig = scf_sig;
    out.scf_noise = scf_noise;
    out.sine_idx = sine_idx;
    out.sine_area = sine_area;
    out.sine_lev = sine_lev;
    out.noise_lev = noise_lev;
    out.placement = placement;
    out.prev_at_end = prev_at_end;
    out.source_bands = *bands;
    out.source_interval = *interval;
    out.source_num_ts_in_ats = num_ts_in_ats;
    out.subbands = num_sb_aspx as u8;
    out.envelopes = envelopes as u8;

    // 本区间最后一个包络的正弦列成为下一区间的历史。
    let mut last_column = [false; SUBBANDS];
    for sb in 0..num_sb_aspx {
        last_column[sb + sbx] = sine_idx[sb][envelopes - 1];
    }
    sines.last_column = last_column;
    sines.placement_at_end = placement.is_at_end(envelopes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aspx::frames::AspxIntervalParams;

    /// 一组能推出完整频带表的配置。
    fn bands() -> AspxBandTables {
        AspxBandTables::derive(false, 0, 0, 0, 0).expect("应能推出频带表")
    }

    /// FIXFIX、覆盖 `slots` 个 A-SPX 时隙的区间。
    fn interval_with_envelopes(slots: u8, envelopes: u8) -> AspxInterval {
        let params = AspxIntervalParams::fixfix(envelopes);
        AspxInterval::derive(&params, slots, 1, true, 16).expect("应能推导区间")
    }

    /// 单包络区间。
    fn interval(slots: u8) -> AspxInterval {
        interval_with_envelopes(slots, 1)
    }

    /// 把 `Q_high` 铺成常数：每个子带取 `value`，实部虚部各一半功率。
    fn constant_high(slots: usize, sbx: usize, count: usize, value: f32) -> [QmfSlot; 32] {
        let mut out = [QmfSlot::zero(); 32];
        for slot in out.iter_mut().take(slots) {
            for sb in sbx..sbx + count {
                slot.re[sb] = value;
                slot.im[sb] = value;
            }
        }
        out
    }

    /// 全 1 的标度因子。
    fn unit_scale_factors(
        envelopes: usize,
        sig_groups: usize,
        noise_groups: usize,
    ) -> ScaleFactors {
        let mut out = ScaleFactors::new();
        out.fill_for_test(envelopes, sig_groups, noise_groups, 1.0, 1.0);
        out
    }

    #[test]
    fn the_pointer_keeps_the_minus_one_applied_by_table_53() {
        let placement = SinePlacement::from_params(-1);
        assert_eq!(placement.pointer, -1, "不得再次还原到原始码字 0");
        assert!(placement.starts_by(0), "0 >= −1");
        assert!(!placement.is_at_end(3));

        let placement = SinePlacement::from_params(0);
        assert_eq!(placement.pointer, 0);
        assert!(placement.starts_by(0), "指针 0 从第 0 个包络生效");
        assert!(!placement.is_at_end(3));

        let placement = SinePlacement::from_params(3);
        assert!(placement.is_at_end(3), "只有解码值等于包络数才在末尾");
        assert!(!SinePlacement::from_params(2).is_at_end(3));
    }

    #[test]
    fn the_estimate_uses_the_squared_magnitude_not_just_the_real_part() {
        // 实部与虚部各取 3：模平方为 18。只算实部会得到 9。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = interval(16);
        let slots = 16usize;
        let q_high = constant_high(slots, sbx, count, 3.0);
        let sf = unit_scale_factors(
            1,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        let mut sines = SineState::new();
        let mut out = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            true,
            1,
            &mut sines,
            &mut out,
        )
        .expect("应能估计");
        for sb in 0..count {
            assert_eq!(out.estimated(sb, 0), Some(18.0), "子带 {sb} 的模平方");
        }
    }

    #[test]
    fn a_factor_of_two_averages_all_qmf_timeslots() {
        // 一个 ATS 含两个 QMF 时隙。每个复样本的模平方为 2，16 个 ATS 累加
        // 32 个样本得到 64，再按实际 QMF 时隙数 32 归一化，结果应为 2。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = interval(16);
        let q_high = constant_high(32, sbx, count, 1.0);
        let sf = unit_scale_factors(
            1,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        let mut sines = SineState::new();
        let mut out = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            true,
            2,
            &mut sines,
            &mut out,
        )
        .expect("表 192 的倍率 2 应可估计");
        for sb in 0..count {
            assert_eq!(out.estimated(sb, 0), Some(2.0), "子带 {sb} 的 QMF 时隙平均");
        }
    }

    #[test]
    fn interpolation_off_averages_across_the_subband_group() {
        // 关掉插值时同一子带组内的所有子带得到同一个平均值；打开时逐子带。
        // 只给组内一个子带非零功率，两种模式的差别就显出来了。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let interval = interval(16);
        let mut q_high = [QmfSlot::zero(); 32];
        for slot in q_high.iter_mut().take(16) {
            slot.re[sbx] = 4.0;
        }
        let sf = unit_scale_factors(
            1,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        let mut sines = SineState::new();

        let mut interpolated = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            true,
            1,
            &mut sines.clone(),
            &mut interpolated,
        )
        .expect("应能估计");
        let mut averaged = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            false,
            1,
            &mut sines,
            &mut averaged,
        )
        .expect("应能估计");

        assert_eq!(
            interpolated.estimated(0, 0),
            Some(16.0),
            "插值：只有第 0 子带有功率"
        );
        assert_eq!(interpolated.estimated(1, 0), Some(0.0));
        // 平均模式：功率摊到整个子带组，组内每个子带同值且小于 16。
        let first = averaged.estimated(0, 0).expect("应有值");
        assert!(
            first > 0.0 && first < 16.0,
            "组内平均应低于单带峰值：{first}"
        );
        assert_eq!(averaged.estimated(1, 0), Some(first), "组内应同值");
        assert!(count > 2, "本用例需要组内至少两个子带");
    }

    #[test]
    fn the_levels_take_the_square_root_of_the_signal_to_noise_factor() {
        // Pseudocode 94：sig_noise_fact = scf_sig / (1 + scf_noise)。
        // scf 全 1 时因子为 0,5；无正弦则 sine_lev = 0、noise_lev = sqrt(0,5)。
        let bands = bands();
        let interval = interval(16);
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let q_high = constant_high(16, sbx, count, 1.0);
        let sf = unit_scale_factors(
            1,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        let mut sines = SineState::new();
        let mut out = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            true,
            1,
            &mut sines,
            &mut out,
        )
        .expect("应能估计");
        for sb in 0..count {
            assert_eq!(out.sine_level(sb, 0), Some(0.0), "无正弦时电平为零");
            assert_eq!(
                out.noise_level(sb, 0),
                Some(sqrt_f32(0.5)),
                "noise_lev = sqrt(sig_noise_fact · scf_noise)"
            );
        }
    }

    #[test]
    fn a_sine_lands_in_the_middle_of_its_high_resolution_group() {
        let bands = bands();
        let interval = interval(16);
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let q_high = constant_high(16, sbx, count, 1.0);
        let sf = unit_scale_factors(
            1,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        // 组宽必须至少 3，否则「正中」与「组首」是同一个子带，判据看不出区别。
        // 本配置的前十个高分辨率组宽都是 1，取一个宽 3 的。
        let group = (0..usize::from(bands.num_sbg_sig_highres()))
            .find(|&g| {
                let low = bands.sig_highres_border(g).unwrap_or(0);
                let high = bands.sig_highres_border(g + 1).unwrap_or(0);
                high.saturating_sub(low) >= 3
            })
            .expect("应有宽度至少 3 的高分辨率组");
        let mut harmonic = [false; 32];
        harmonic[group] = true;
        let mut sines = SineState::new();
        let mut out = EnvelopeEstimate::new();
        // aspx_tsg_ptr = −1 时 atsg >= −1 恒真，正弦立即生效。
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &harmonic,
            SinePlacement::from_params(-1),
            true,
            1,
            &mut sines,
            &mut out,
        )
        .expect("应能估计");

        let low = usize::from(bands.sig_highres_border(group).unwrap_or(0)) - sbx;
        let high = usize::from(bands.sig_highres_border(group + 1).unwrap_or(0)) - sbx;
        let mid = (low + high) / 2;
        assert!(
            mid > low && mid + 1 < high || high - low >= 3,
            "组宽应足以区分正中与组首"
        );
        for sb in low..high {
            let expected = sb == mid;
            assert_eq!(
                out.sine_index(sb, 0),
                Some(expected),
                "组 {group} 内只有正中的子带 {mid} 该被标记，实际看子带 {sb}"
            );
        }

        // `Pseudocode 93`：sine_area 在该子带所属的**低分辨率**组内铺满。判据要
        // 落在一个 sine_idx 为假的子带上——只看 mid 自己，传不传播都是真。
        let spread = (0..count)
            .find(|&sb| out.sine_area(sb, 0) == Some(true) && out.sine_index(sb, 0) == Some(false))
            .expect("低分辨率组宽至少 2，正中之外应有子带被 sine_area 覆盖");
        assert_ne!(spread, mid, "该子带不应是被直接标记的那个");
    }

    #[test]
    fn a_decoded_zero_pointer_starts_in_the_first_envelope() {
        let bands = bands();
        let interval = interval_with_envelopes(16, 2);
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let q_high = constant_high(16, sbx, count, 1.0);
        let sf = unit_scale_factors(
            2,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        let mut harmonic = [false; 32];
        harmonic[0] = true;
        let mut sines = SineState::new();
        let mut out = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &harmonic,
            SinePlacement::from_params(0),
            true,
            1,
            &mut sines,
            &mut out,
        )
        .expect("解码后的指针 0 应可估计");

        assert!(
            (0..count).any(|sb| out.sine_index(sb, 0) == Some(true)),
            "指针 0 必须从第 0 个包络生效，不能再次加一"
        );
    }

    #[test]
    fn the_sine_marker_carries_into_the_next_interval() {
        // 上一区间末尾有正弦时，下一区间即使指针未到也要延续。
        let bands = bands();
        let interval = interval(16);
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let q_high = constant_high(16, sbx, count, 1.0);
        let sf = unit_scale_factors(
            1,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        let mut harmonic = [false; 32];
        harmonic[0] = true;
        let mut sines = SineState::new();
        let mut out = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &harmonic,
            SinePlacement::from_params(-1),
            true,
            1,
            &mut sines,
            &mut out,
        )
        .expect("第一区间");
        let carried = sines.last_column;
        assert!(carried.iter().any(|&x| x), "第一区间应留下正弦标记");

        // 第二区间：指针落在区间末尾（当前包络不生效），但历史标记仍应延续。
        let mut second = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &harmonic,
            SinePlacement::from_params(1),
            true,
            1,
            &mut sines,
            &mut second,
        )
        .expect("第二区间");
        assert!(
            (0..count).any(|sb| second.sine_index(sb, 0) == Some(true)),
            "上一区间的正弦应延续"
        );
    }

    #[test]
    fn a_pointer_at_the_previous_interval_end_starts_the_next_sine() {
        let bands = bands();
        let interval = interval(16);
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let q_high = constant_high(16, sbx, count, 1.0);
        let sf = unit_scale_factors(
            1,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        let mut harmonic = [false; 32];
        harmonic[0] = true;
        let mut sines = SineState::new();
        let mut first = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &harmonic,
            SinePlacement::from_params(1),
            true,
            1,
            &mut sines,
            &mut first,
        )
        .expect("第一区间");
        assert!(
            !(0..count).any(|sb| first.sine_index(sb, 0) == Some(true)),
            "末尾指针在本区间尚未生效"
        );

        let mut second = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &harmonic,
            SinePlacement::from_params(1),
            true,
            1,
            &mut sines,
            &mut second,
        )
        .expect("第二区间");
        assert!(
            (0..count).any(|sb| second.sine_index(sb, 0) == Some(true)),
            "上一区间指针在末尾时，本区间应立即插入正弦"
        );
    }

    #[test]
    fn the_onset_predicate_binds_each_index_to_its_own_interval() {
        // `Pseudocode 95`/`99` 的 `atsg == aspx_tsg_ptr || atsg == p_sine_at_end`。
        // 左边是本区间的指针，右边只在上一区间的指针落在其末尾时于包络 0 成立。
        let bands = bands();
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let q_high = constant_high(16, sbx, count, 1.0);
        let sf = unit_scale_factors(
            2,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        let two = interval_with_envelopes(16, 2);

        // 第一区间：两个包络，指针 1 指向包络 1，且不在末尾（末尾是 2）。
        let mut sines = SineState::new();
        let mut first = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &two,
            &sf,
            &[false; 32],
            SinePlacement::from_params(1),
            true,
            1,
            &mut sines,
            &mut first,
        )
        .expect("第一区间");
        assert_eq!(first.sine_onset(0), Some(false), "包络 0 不是起点");
        assert_eq!(first.sine_onset(1), Some(true), "指针正指向包络 1");
        assert_eq!(first.sine_onset(2), None, "包络数是 2");

        // 第二区间：指针 2 恰在两个包络的末尾，本区间因此没有起点包络。
        let mut second = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &two,
            &sf,
            &[false; 32],
            SinePlacement::from_params(2),
            true,
            1,
            &mut sines,
            &mut second,
        )
        .expect("第二区间");
        assert_eq!(second.sine_onset(0), Some(false), "上一区间的指针不在末尾");
        assert_eq!(second.sine_onset(1), Some(false), "指针 2 越过了两个包络");

        // 第三区间：指针 −1 永不等于任何包络下标，但上一区间的指针落在末尾，
        // 包络 0 仍是起点。`estimate` 已经把状态推进到第三区间，若判据到这里
        // 才去读 SineState，拿到的会是「−1 不在末尾」。
        let mut third = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &two,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            true,
            1,
            &mut sines,
            &mut third,
        )
        .expect("第三区间");
        assert_eq!(third.sine_onset(0), Some(true), "继承上一区间的末尾指针");
        assert_eq!(third.sine_onset(1), Some(false), "只在包络 0 成立");
        assert!(!sines.placement_at_end, "推进后的状态已是第三区间自己的");
    }

    #[test]
    fn rejected_input_leaves_the_output_untouched() {
        let bands = bands();
        let interval = interval(16);
        let sbx = usize::from(bands.sbx());
        let count = usize::from(bands.num_sb_aspx());
        let q_high = constant_high(16, sbx, count, 2.0);
        let sf = unit_scale_factors(
            1,
            usize::from(bands.num_sbg_sig_lowres()),
            usize::from(bands.num_sbg_noise()),
        );
        let mut sines = SineState::new();
        let mut out = EnvelopeEstimate::new();
        estimate(
            &q_high,
            &bands,
            &interval,
            &sf,
            &[false; 32],
            SinePlacement::from_params(-1),
            true,
            1,
            &mut sines,
            &mut out,
        )
        .expect("哨兵结果应可生成");
        let snapshot = out.clone();
        let sine_snapshot = sines;

        // Q_high 太短。
        let short = &q_high[..2];
        assert_eq!(
            estimate(
                short,
                &bands,
                &interval,
                &sf,
                &[false; 32],
                SinePlacement::from_params(-1),
                true,
                1,
                &mut sines,
                &mut out
            ),
            Err(HfAdjustError::HighBandTooShort {
                needed: 16,
                provided: 2
            })
        );

        // 表 192 之外的倍率必须在任何乘法之前被拒绝。
        for factor in [0, 3] {
            assert_eq!(
                estimate(
                    &q_high,
                    &bands,
                    &interval,
                    &sf,
                    &[false; 32],
                    SinePlacement::from_params(-1),
                    true,
                    factor,
                    &mut sines,
                    &mut out
                ),
                Err(HfAdjustError::TimeslotFactorOutOfRange { factor })
            );
        }

        // harmonic 向量短于高分辨率组数时不得把缺项当成 false。
        assert_eq!(
            estimate(
                &q_high,
                &bands,
                &interval,
                &sf,
                &[],
                SinePlacement::from_params(-1),
                true,
                1,
                &mut sines,
                &mut out
            ),
            Err(HfAdjustError::HarmonicDataTooShort {
                needed: usize::from(bands.num_sbg_sig_highres()),
                provided: 0
            })
        );

        assert_eq!(out, snapshot, "被拒绝的输入不应改写任一矩阵");
        assert_eq!(sines, sine_snapshot, "被拒绝的输入不应推进状态");
    }
}
