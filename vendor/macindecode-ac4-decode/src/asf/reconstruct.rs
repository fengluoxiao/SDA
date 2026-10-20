//! ASF 量化重建与缩放（`TS103190-1:v1.4.1:5.1.3`）。
//!
//! `5.1.3.2` 的重建分三步：
//!
//! 1. 还原绝对标度因子 `sf`——`Pseudocode 21` 的 DPCM 累加，纯整数；
//! 2. 由 `sf` 得增益 `k_sf = 2^((sf − 100) / 4)`；
//! 3. 反量化 `rec_spec = sign(q) × |q|^(4/3)`，再乘以增益得 `scaled_spec`。
//!
//! 三步均已实现。后两步采用 ADR-0002 定下的数值格式：`f32` 存储、`f64`
//! 累加；`|q|^(4/3)` 走构建期以精确整数判据生成并冻结摘要的 8 192 项表，
//! `2^((sf−100)/4)` 拆成 `2^q × 2^(r/4)` 由位构造与四个常量给出，不引入
//! `libm`——`core` 连 `sqrt` 都不提供。
//!
//! 第一步能独立成立，是因为它纯整数、无损，且规范给了一条取值域约束：
//! `5.1.3.2` 的 NOTE 规定只有 `0…255` 是合法标度因子值。
//!
//! **但那条约束很弱，不足以单独支撑实现。** 注入实验里，偏移取 59 而非 60、
//! 首带也消费差值这两种错法都产生了系统性偏移，实测八条流却一次都没出界——
//! 合法区间宽 256，而实测取值只占 `77…159`，两侧各有大片余量吸收错误。真正
//! 区分对错的是手工验算的 DPCM 链（见本模块测试），取值域只是兜底。

use crate::asf::dequant::reconstruct_line;
use crate::asf::framing::{AsfLayoutKey, AsfWindowLayout, MAX_SFB, MAX_WINDOWS};
use crate::asf::spectrum::{AsfWorkspace, coded_band_count};
use core::fmt;

/// 标度因子的合法取值上界，`5.1.3.2` NOTE。
pub const MAX_SCALE_FACTOR: i32 = 255;

/// 码本下标到标度因子差值的偏移，`5.1.3.2`：`sf_n = sf_(n−1) + cw_idx_n − 60`。
const SCALE_FACTOR_OFFSET: i32 = 60;

/// `k_sf = 2^((sf − 100) / 4)` 中的常数 100，`5.1.3.2`。
const GAIN_BIAS: i32 = 100;

/// `2^(r/4)`，`r ∈ {0,1,2,3}`。
///
/// 写位模式而非十进制字面量：这四个值是决策的一部分（ADR-0002），位模式让
/// 它们不经过任何字面量解析规则。
const QUARTER_POWERS: [f32; 4] = [
    f32::from_bits(0x3F80_0000), // 2^0
    f32::from_bits(0x3F98_37F0), // 2^(1/4)
    f32::from_bits(0x3FB5_04F3), // 2^(1/2)
    f32::from_bits(0x3FD7_44FD), // 2^(3/4)
];

/// 量化重建失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconstructError {
    /// 首个有效频带之后缺少规范要求的标度因子差值。
    MissingScaleFactorDelta {
        /// 窗口组。
        group: u8,
        /// 频带下标。
        sfb: u8,
    },
    /// 累加后的标度因子走出了 `0…255`。
    ///
    /// 差值域是 `[−60, 60]`（码本 121 个符号），单步跨度远大于合法区间宽度的
    /// 四分之一，因此解析错位很容易在几个频带内把 `sf` 顶出区间。
    ScaleFactorOutOfRange {
        /// 窗口组。
        group: u8,
        /// 频带下标。
        sfb: u8,
        /// 越界的值。
        value: i32,
    },
    /// 输出缓冲装不下本帧的谱线。
    OutputTooSmall {
        /// 需要的谱线数。
        needed: usize,
        /// 调用方提供的容量。
        provided: usize,
    },
    /// 输入谱线不足本帧布局声明的条数。
    InputTooSmall {
        /// 需要的谱线数。
        needed: usize,
        /// 调用方提供的条数。
        provided: usize,
    },
    /// 谱工作区、标度因子与窗口布局不是由同一份成帧信息生成。
    ///
    /// 两个字段各自回答「该输入是否匹配调用方提供的布局」。[`scale_factors`]
    /// 阶段还没有标度因子这项输入，故 `factors_match` 恒为真。
    LayoutMismatch {
        /// 工作区是否匹配调用方提供的布局。
        workspace_matches: bool,
        /// 标度因子是否匹配调用方提供的布局。
        factors_match: bool,
    },
    /// 标度因子不是由当前谱工作区中的码值还原而来。
    ScaleFactorSourceMismatch,
    /// 匹配布局中的频带偏移无法映射到谱线切片。
    InvalidBandRange {
        /// 窗口组。
        group: u8,
        /// 频带下标。
        sfb: u8,
    },
}

impl fmt::Display for ReconstructError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReconstructError::MissingScaleFactorDelta { group, sfb } => {
                write!(
                    f,
                    "Window group {group}, band {sfb} lacks a scale-factor delta"
                )
            }
            ReconstructError::ScaleFactorOutOfRange { group, sfb, value } => {
                write!(
                    f,
                    "Window group {group}, band {sfb} reconstructs invalid scale factor {value}"
                )
            }
            ReconstructError::OutputTooSmall { needed, provided } => {
                write!(
                    f,
                    "Output buffer requires {needed} spectral lines, but only {provided} were provided"
                )
            }
            ReconstructError::InputTooSmall { needed, provided } => {
                write!(
                    f,
                    "Ungrouping input requires {needed} spectral lines, but only {provided} were provided"
                )
            }
            ReconstructError::LayoutMismatch {
                workspace_matches,
                factors_match,
            } => write!(
                f,
                "Scaling input mixes window layouts (workspace match: {workspace_matches}, scale-factor match: {factors_match})"
            ),
            ReconstructError::ScaleFactorSourceMismatch => f.write_str(
                "Scale factors do not match code values in the current spectrum workspace",
            ),
            ReconstructError::InvalidBandRange { group, sfb } => {
                write!(
                    f,
                    "Window group {group}, band {sfb} cannot be mapped to a spectral-line range"
                )
            }
        }
    }
}

impl core::error::Error for ReconstructError {}

/// 一个通道逐频带的绝对标度因子。
///
/// `None` 表示该频带不传输标度因子：`sfb_cb` 为 0，或该带的量化谱线全为零。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaleFactors {
    values: [[Option<u8>; MAX_SFB]; MAX_WINDOWS],
    layout_key: AsfLayoutKey,
}

impl Default for ScaleFactors {
    fn default() -> Self {
        Self::new()
    }
}

impl ScaleFactors {
    /// 构造一份未绑定窗口布局的全空标度因子表。
    ///
    /// 该值适合作为占位符；缩放真实谱线时，应使用 [`scale_factors`] 针对对应
    /// 窗口布局生成的结果。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            values: [[None; MAX_SFB]; MAX_WINDOWS],
            layout_key: AsfLayoutKey::empty(),
        }
    }

    const fn for_layout(layout: &AsfWindowLayout) -> Self {
        Self {
            values: [[None; MAX_SFB]; MAX_WINDOWS],
            layout_key: layout.key(),
        }
    }

    /// 查询某频带的绝对标度因子。
    #[must_use]
    pub fn get(&self, group: usize, sfb: usize) -> Option<u8> {
        self.values.get(group)?.get(sfb).copied().flatten()
    }

    /// 已还原的频带数。
    #[must_use]
    pub fn count(&self) -> usize {
        self.values
            .iter()
            .flat_map(|row| row.iter())
            .filter(|value| value.is_some())
            .count()
    }
}

/// 还原 `asf_scalefac_data()` 的绝对标度因子，见 `Pseudocode 21`。
///
/// 起点是 `reference_scale_factor`；**首个被传输的频带不消费差值**，其后每个
/// 频带累加 `dpcm_sf − 60`。频带的取舍条件与解析侧完全相同——`sfb_cb` 非零且
/// `max_quant_idx` 非零——两侧共用 [`coded_band_count`] 的循环上界。
///
/// # Errors
///
/// 工作区与布局不是同一份成帧信息生成时返回
/// [`ReconstructError::LayoutMismatch`]；首个有效频带之后缺少差值时返回
/// [`ReconstructError::MissingScaleFactorDelta`]；任一步累加后的标度因子走出
/// `0…255` 时返回 [`ReconstructError::ScaleFactorOutOfRange`]。
pub fn scale_factors(
    workspace: &AsfWorkspace,
    layout: &AsfWindowLayout,
) -> Result<ScaleFactors, ReconstructError> {
    // 混用不会自己暴露：频带取舍全部来自 layout，而码值来自 workspace，窄
    // 布局配宽谱只是少还原几个频带，返回值看上去完全合法。只调本函数而不做
    // 缩放的调用方（例如只统计标度因子范围）拿不到任何信号。
    if workspace.layout_key() != layout.key() {
        return Err(ReconstructError::LayoutMismatch {
            workspace_matches: false,
            factors_match: true,
        });
    }
    let mut out = ScaleFactors::for_layout(layout);
    let mut scale_factor = i32::from(workspace.reference_scale_factor());
    let mut first_found = false;

    for group in 0..usize::from(layout.num_window_groups()) {
        for sfb in 0..coded_band_count(layout, group) {
            if workspace.sfb_codebook(group, sfb).unwrap_or(0) == 0 {
                continue;
            }
            if workspace.max_quant_idx(group, sfb).unwrap_or(0) == 0 {
                continue;
            }
            if first_found {
                // 解析侧对首个之后的每个合格频带都应写入 dpcm_sf；缺席意味着
                // 工作区不完整或两侧的频带取舍不一致，不能伪造一个差值继续。
                let symbol = workspace.dpcm_sf(group, sfb).ok_or(
                    ReconstructError::MissingScaleFactorDelta {
                        group: u8::try_from(group).unwrap_or(u8::MAX),
                        sfb: u8::try_from(sfb).unwrap_or(u8::MAX),
                    },
                )?;
                let symbol = i32::from(symbol);
                scale_factor = scale_factor
                    .saturating_add(symbol)
                    .saturating_sub(SCALE_FACTOR_OFFSET);
            } else {
                first_found = true;
            }
            if !(0..=MAX_SCALE_FACTOR).contains(&scale_factor) {
                return Err(ReconstructError::ScaleFactorOutOfRange {
                    group: u8::try_from(group).unwrap_or(u8::MAX),
                    sfb: u8::try_from(sfb).unwrap_or(u8::MAX),
                    value: scale_factor,
                });
            }
            if let Some(row) = out.values.get_mut(group)
                && let Some(cell) = row.get_mut(sfb)
            {
                *cell = Some(u8::try_from(scale_factor).unwrap_or(u8::MAX));
            }
        }
    }
    Ok(out)
}

/// 标度因子增益 `k_sf = 2^((sf − 100) / 4)`，见 `5.1.3.2`。
///
/// 拆成 `2^q × 2^(r/4)`，其中 `sf − 100 = 4q + r` 且 `r ∈ {0,1,2,3}`。`2^q` 由
/// 指数域位构造给出，尾数全零，因此与常量相乘只是指数平移，**不产生舍入**；
/// `q` 的取值范围是 `−25…38`，离 f32 的指数边界很远，不会溢出或转为次正规。
///
/// 用 `div_euclid`/`rem_euclid` 而非 `/` 与 `%`：`sf < 100` 时被除数为负，截断
/// 除法会把 `−99 / 4` 算成 `−24`，而规范要的是下取整的 `−25`。
#[must_use]
pub fn scale_factor_gain(sf: u8) -> f32 {
    let shifted = i32::from(sf).saturating_sub(GAIN_BIAS);
    let quarters = shifted.div_euclid(4);
    let residue = shifted.rem_euclid(4);
    let biased = quarters.saturating_add(127);
    let power = f32::from_bits((biased.max(0) as u32) << 23);
    let factor = QUARTER_POWERS
        .get(residue.max(0) as usize)
        .copied()
        .unwrap_or(1.0);
    power * factor
}

/// 把量化谱线整体还原为缩放后的谱线，见 `5.1.3.2`。
///
/// `out` 至少要容纳 [`AsfWindowLayout::total_lines`] 条谱线，返回实际写入的
/// 条数。不携带标度因子的频带写 0——它们的量化谱线本就全为零（`sfb_cb` 为 0
/// 或 `max_quant_idx` 为 0），`max_sfb` 之上的谱线同理。
///
/// # Errors
///
/// `out` 容量不足时返回 [`ReconstructError::OutputTooSmall`]；三个输入不是来自
/// 同一窗口布局时返回 [`ReconstructError::LayoutMismatch`]；标度因子不是由当前
/// 工作区还原时返回 [`ReconstructError::ScaleFactorSourceMismatch`]；匹配布局的
/// 频带无法映射到谱线切片时返回 [`ReconstructError::InvalidBandRange`]。
pub fn scale_spectrum(
    workspace: &AsfWorkspace,
    layout: &AsfWindowLayout,
    factors: &ScaleFactors,
    out: &mut [f32],
) -> Result<usize, ReconstructError> {
    let quant = workspace.quant_spec();
    let lines = quant.len();
    let layout_key = layout.key();
    let workspace_matches = workspace.layout_key() == layout_key;
    let factors_match = factors.layout_key == layout_key;
    if !workspace_matches || !factors_match {
        return Err(ReconstructError::LayoutMismatch {
            workspace_matches,
            factors_match,
        });
    }
    // 相同窗口布局只证明频带边界一致。共享 sf_info 的多个声道拥有相同布局，
    // 但各自传输独立的参考值与 DPCM 差值；精确重建并比较可拒绝拿错声道的
    // factors，同时允许数值确实相同（因而混用无害）的结果。
    let expected_factors = scale_factors(workspace, layout)?;
    if factors.values != expected_factors.values {
        return Err(ReconstructError::ScaleFactorSourceMismatch);
    }
    if out.len() < lines {
        return Err(ReconstructError::OutputTooSmall {
            needed: lines,
            provided: out.len(),
        });
    }
    let Some(target) = out.get_mut(..lines) else {
        return Err(ReconstructError::OutputTooSmall {
            needed: lines,
            provided: out.len(),
        });
    };
    target.fill(0.0);

    for group in 0..usize::from(layout.num_window_groups()) {
        for sfb in 0..coded_band_count(layout, group) {
            let Some(sf) = factors.get(group, sfb) else {
                continue;
            };
            let gain = scale_factor_gain(sf);
            let invalid_range = || ReconstructError::InvalidBandRange {
                group: u8::try_from(group).unwrap_or(u8::MAX),
                sfb: u8::try_from(sfb).unwrap_or(u8::MAX),
            };
            let start = layout
                .sect_sfb_offset(group, sfb)
                .ok_or_else(invalid_range)?;
            let end = layout
                .sect_sfb_offset(group, sfb.saturating_add(1))
                .ok_or_else(invalid_range)?;
            let (start, end) = (usize::from(start), usize::from(end));
            let quants = quant.get(start..end).ok_or_else(invalid_range)?;
            let slots = target.get_mut(start..end).ok_or_else(invalid_range)?;
            for (slot, &value) in slots.iter_mut().zip(quants.iter()) {
                *slot = gain * reconstruct_line(value);
            }
        }
    }
    Ok(lines)
}

/// 逐窗口在解组输出中的起始偏移，以及输出总长。
///
/// 窗口铺满一帧（`4.3.6.2` 的成帧规则），故总长恒为 `frame_len_base`；但逐窗
/// 口长度取自各自所属组的变换长度，短块与长块混排时并不相等，不能按平均值推。
fn window_offsets(
    layout: &AsfWindowLayout,
) -> Result<([u32; MAX_WINDOWS + 1], usize), ReconstructError> {
    let mut offsets = [0u32; MAX_WINDOWS + 1];
    let windows = usize::from(layout.num_windows());
    for window in 0..windows.min(MAX_WINDOWS) {
        let invalid = || ReconstructError::InvalidBandRange {
            group: u8::try_from(window).unwrap_or(u8::MAX),
            sfb: u8::MAX,
        };
        let group = layout.window_to_group(window).ok_or_else(invalid)?;
        let length = layout
            .transform_length(usize::from(group))
            .ok_or_else(invalid)?;
        let previous = offsets.get(window).copied().ok_or_else(invalid)?;
        let slot = offsets
            .get_mut(window.saturating_add(1))
            .ok_or_else(invalid)?;
        *slot = previous.saturating_add(u32::from(length));
    }
    let total = offsets.get(windows.min(MAX_WINDOWS)).copied().unwrap_or(0);
    Ok((offsets, usize::try_from(total).unwrap_or(usize::MAX)))
}

/// 把分组顺序的谱线重排为按窗口排列，见 `5.1.5.2` `Pseudocode 25`。
///
/// 码流内谱线按「组 → 频带 → 组内窗口」编排，而变换需要「窗口 → 频率升序」。
/// 本函数只搬运不计算，`max_sfb` 之上的谱线保持 0。
///
/// `scaled` 必须是同一 `layout` 下 [`scale_spectrum`] 的输出——两者共用
/// [`coded_band_count`] 与 `sect_sfb_offset`，错配会把谱线搬到错误的窗口。
/// 该契约靠文档而非类型保证：`scaled` 是裸切片，不携带布局键。
///
/// 返回输出中有效的谱线数，即各窗口变换长度之和。
///
/// # Errors
///
/// 输入不足 [`AsfWindowLayout::total_lines`] 条时返回
/// [`ReconstructError::InputTooSmall`]；输出装不下全部窗口时返回
/// [`ReconstructError::OutputTooSmall`]；布局本身取不出窗口或频带偏移时返回
/// [`ReconstructError::InvalidBandRange`]。
pub fn ungroup_spectrum(
    layout: &AsfWindowLayout,
    scaled: &[f32],
    out: &mut [f32],
) -> Result<usize, ReconstructError> {
    let lines = usize::try_from(layout.total_lines()).unwrap_or(usize::MAX);
    if scaled.len() < lines {
        return Err(ReconstructError::InputTooSmall {
            needed: lines,
            provided: scaled.len(),
        });
    }
    let (window_offsets, total) = window_offsets(layout)?;
    if out.len() < total {
        return Err(ReconstructError::OutputTooSmall {
            needed: total,
            provided: out.len(),
        });
    }
    let Some(target) = out.get_mut(..total) else {
        return Err(ReconstructError::OutputTooSmall {
            needed: total,
            provided: out.len(),
        });
    };
    target.fill(0.0);

    let mut window = 0usize;
    for group in 0..usize::from(layout.num_window_groups()) {
        let invalid = |sfb: usize| ReconstructError::InvalidBandRange {
            group: u8::try_from(group).unwrap_or(u8::MAX),
            sfb: u8::try_from(sfb).unwrap_or(u8::MAX),
        };
        let count = usize::from(layout.num_win_in_group(group).ok_or_else(|| invalid(0))?);
        let length = layout.transform_length(group).ok_or_else(|| invalid(0))?;
        let offsets = crate::asf::tables::sfb_offsets_48(length).ok_or_else(|| invalid(0))?;

        for sfb in 0..coded_band_count(layout, group) {
            let low = usize::from(offsets.get(sfb).copied().ok_or_else(|| invalid(sfb))?);
            let high = usize::from(
                offsets
                    .get(sfb.saturating_add(1))
                    .copied()
                    .ok_or_else(|| invalid(sfb))?,
            );
            let width = high.checked_sub(low).ok_or_else(|| invalid(sfb))?;
            let base = usize::from(
                layout
                    .sect_sfb_offset(group, sfb)
                    .ok_or_else(|| invalid(sfb))?,
            );
            for slot in 0..count {
                let source = base.saturating_add(slot.saturating_mul(width));
                let start = usize::try_from(
                    window_offsets
                        .get(window.saturating_add(slot))
                        .copied()
                        .ok_or_else(|| invalid(sfb))?,
                )
                .unwrap_or(usize::MAX)
                .saturating_add(low);
                let end = start.saturating_add(width);
                let lines = scaled
                    .get(source..source.saturating_add(width))
                    .ok_or_else(|| invalid(sfb))?;
                let slots = target.get_mut(start..end).ok_or_else(|| invalid(sfb))?;
                slots.copy_from_slice(lines);
            }
        }
        window = window.saturating_add(count);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asf::framing::{AsfPsyContext, AsfPsyInfo, AsfTransformInfo};
    use crate::asf::spectrum::MAX_SPECTRAL_LINES;
    use crate::huffman::tables;
    use crate::testutil::BitBuf;
    use macindecode_ac4_bitstream::reader::BitReader;

    fn long_frame_layout(max_sfb: u32) -> AsfWindowLayout {
        let mut buf = BitBuf::new();
        buf.push(true); // b_long_frame
        buf.push_bits(max_sfb, 6); // n_msfb_bits(2048) = 6
        let mut reader = BitReader::new(buf.as_slice());
        let transform = AsfTransformInfo::parse(&mut reader, 2048, 48_000).expect("变换信息");
        let psy = AsfPsyInfo::parse(&mut reader, &transform, AsfPsyContext::default())
            .expect("心理声学信息");
        AsfWindowLayout::derive(&transform, &psy, false).expect("窗口布局")
    }

    /// 两个 512 点窗口各自成组，用来约束 DPCM 状态跨组延续。
    fn two_group_layout(max_sfb: u32) -> AsfWindowLayout {
        let mut transform_buf = BitBuf::new();
        transform_buf.push_bits(2, 2); // frame_len_base=1024 下取两个 512 点窗口
        let mut reader = BitReader::new(transform_buf.as_slice());
        let transform = AsfTransformInfo::parse(&mut reader, 1024, 48_000).expect("变换信息");
        assert_eq!(transform.n_grp_bits(), Ok(1));

        let length = transform.transform_length(2).expect("变换长度");
        let width = crate::asf::tables::n_msfb_bits_48(length).expect("max_sfb 位宽");
        let mut psy_buf = BitBuf::new();
        psy_buf.push_bits(max_sfb, u32::from(width));
        psy_buf.push(false); // 两个窗口不成组
        let mut reader = BitReader::new(psy_buf.as_slice());
        let psy = AsfPsyInfo::parse(&mut reader, &transform, AsfPsyContext::default())
            .expect("心理声学信息");
        let layout = AsfWindowLayout::derive(&transform, &psy, false).expect("窗口布局");
        assert_eq!(layout.num_window_groups(), 2);
        layout
    }

    /// 一个窗口组含两个 512 点窗口，用来约束组内窗口的交织。
    ///
    /// 与 [`two_group_layout`] 只差一位 `b_group`：那里两窗口各自成组，这里
    /// 合成一组。分组决定谱线在码流里是「按组分段」还是「组内逐带交织」。
    fn one_group_two_windows(max_sfb: u32) -> AsfWindowLayout {
        let mut transform_buf = BitBuf::new();
        transform_buf.push_bits(2, 2);
        let mut reader = BitReader::new(transform_buf.as_slice());
        let transform = AsfTransformInfo::parse(&mut reader, 1024, 48_000).expect("变换信息");

        let length = transform.transform_length(2).expect("变换长度");
        let width = crate::asf::tables::n_msfb_bits_48(length).expect("max_sfb 位宽");
        let mut psy_buf = BitBuf::new();
        psy_buf.push_bits(max_sfb, u32::from(width));
        psy_buf.push(true); // 两个窗口成组
        let mut reader = BitReader::new(psy_buf.as_slice());
        let psy = AsfPsyInfo::parse(&mut reader, &transform, AsfPsyContext::default())
            .expect("心理声学信息");
        let layout = AsfWindowLayout::derive(&transform, &psy, false).expect("窗口布局");
        assert_eq!(layout.num_window_groups(), 1);
        assert_eq!(layout.num_win_in_group(0), Some(2));
        assert_eq!(layout.transform_length(0), Some(512));
        layout
    }

    /// 构造 `max_sfb` 个全部非零的频带，逐带写入给定的差值码本下标。
    ///
    /// 单区段用码本 1（四维、有符号、基数 3、偏置 1），符号 0 解为四个 `−1`，
    /// 因此每个频带的 `max_quant_idx` 都是 1，全部参与标度因子链。
    fn decode_all_nonzero(reference: u32, deltas: &[u16]) -> (AsfWorkspace, AsfWindowLayout) {
        let bands = u32::try_from(deltas.len()).expect("频带数");
        let max_sfb = bands.saturating_add(1);
        let layout = long_frame_layout(max_sfb);
        let mut buf = BitBuf::new();
        buf.push_bits(1, 4); // sect_cb = 1
        buf.push_bits(bands, 5); // sect_len = max_sfb − 1，恰好铺满
        for _ in 0..max_sfb {
            buf.push_symbol(&tables::ASF_HCB_1, 0);
        }
        buf.push_bits(reference, 8);
        for &delta in deltas {
            buf.push_symbol(&tables::ASF_HCB_SCALEFAC, delta);
        }
        buf.push(false); // b_snf_data_exists

        let mut reader = BitReader::new(buf.as_slice());
        let mut workspace = AsfWorkspace::new();
        workspace.decode(&mut reader, &layout).expect("谱数据");
        assert_eq!(
            reader.bit_position(),
            buf.bit_len() as u64,
            "解析消耗的比特数与构造长度不符"
        );
        (workspace, layout)
    }

    /// `Pseudocode 21` 的 DPCM 链：首带取参考值，其后逐带累加 `符号 − 60`。
    #[test]
    fn scale_factor_chain_accumulates_from_the_reference() {
        let (workspace, layout) = decode_all_nonzero(100, &[70, 55]);

        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");

        assert_eq!(factors.get(0, 0), Some(100), "首带不消费差值");
        assert_eq!(factors.get(0, 1), Some(110), "100 + 70 − 60");
        assert_eq!(factors.get(0, 2), Some(105), "110 + 55 − 60");
        assert_eq!(factors.count(), 3);
    }

    /// `Pseudocode 21` 把累加器放在窗口组循环之外，后一组必须接着前一组累加。
    #[test]
    fn scale_factor_chain_continues_across_window_groups() {
        let layout = two_group_layout(1);
        let mut buf = BitBuf::new();
        for _ in 0..2 {
            buf.push_bits(1, 4); // sect_cb = 1
            buf.push_bits(0, 3); // sect_len = 1
        }
        for _ in 0..2 {
            buf.push_symbol(&tables::ASF_HCB_1, 0); // 每组唯一频带均非零
        }
        buf.push_bits(100, 8); // 第一组取参考值
        buf.push_symbol(&tables::ASF_HCB_SCALEFAC, 70); // 第二组：100 + 70 − 60
        buf.push(false); // b_snf_data_exists

        let mut reader = BitReader::new(buf.as_slice());
        let mut workspace = AsfWorkspace::new();
        workspace.decode(&mut reader, &layout).expect("谱数据");
        assert_eq!(reader.bit_position(), buf.bit_len() as u64);

        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");

        assert_eq!(factors.get(0, 0), Some(100));
        assert_eq!(factors.get(1, 0), Some(110), "后一组沿用前组末值");
        assert_eq!(factors.count(), 2);
    }

    /// 失败解析留下的半成品工作区不得把缺失差值伪造成符号 0。
    #[test]
    fn missing_scale_factor_delta_is_rejected() {
        let layout = long_frame_layout(2);
        let mut buf = BitBuf::new();
        buf.push_bits(1, 4); // sect_cb = 1
        buf.push_bits(1, 5); // sect_len = 2
        for _ in 0..2 {
            buf.push_symbol(&tables::ASF_HCB_1, 0);
        }
        buf.push_bits(100, 8);
        // 不写第二频带需要的差值；末字节的 0 填充不足以组成一个码字。

        let mut reader = BitReader::new(buf.as_slice());
        let mut workspace = AsfWorkspace::new();
        assert!(workspace.decode(&mut reader, &layout).is_err());
        assert_eq!(workspace.dpcm_sf(0, 1), None);

        let error = scale_factors(&workspace, &layout).expect_err("缺失差值必须失败");

        assert_eq!(
            error,
            ReconstructError::MissingScaleFactorDelta { group: 0, sfb: 1 }
        );
    }

    /// 差值恒为 60 时标度因子不动——码本下标 60 的码长为 1，是最常见的取值。
    #[test]
    fn the_neutral_symbol_leaves_the_scale_factor_unchanged() {
        let (workspace, layout) = decode_all_nonzero(200, &[60, 60, 60]);

        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");

        for sfb in 0..4 {
            assert_eq!(factors.get(0, sfb), Some(200), "第 {sfb} 带");
        }
    }

    /// 全零的频带不参与标度因子链，也不占据它的位置。
    #[test]
    fn silent_bands_carry_no_scale_factor() {
        let layout = long_frame_layout(2);
        let mut buf = BitBuf::new();
        buf.push_bits(1, 4); // sect_cb = 1
        buf.push_bits(1, 5); // sect_len = 2
        buf.push_symbol(&tables::ASF_HCB_1, 0); // sfb 0：四个 −1
        buf.push_symbol(&tables::ASF_HCB_1, 40); // sfb 1：四个 0
        buf.push_bits(0xA5, 8);
        buf.push(false); // b_snf_data_exists

        let mut reader = BitReader::new(buf.as_slice());
        let mut workspace = AsfWorkspace::new();
        workspace.decode(&mut reader, &layout).expect("谱数据");
        assert_eq!(workspace.max_quant_idx(0, 1), Some(0), "第二频带应全零");

        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");

        assert_eq!(factors.get(0, 0), Some(0xA5));
        assert_eq!(factors.get(0, 1), None, "全零频带不传标度因子");
        assert_eq!(factors.count(), 1);
    }

    /// `5.1.3.2` 的 NOTE 只认 `0…255`，两端都必须拒绝。
    #[test]
    fn scale_factors_outside_the_valid_range_are_rejected() {
        for (reference, delta, expected) in [(5u32, 0u16, -55i32), (250, 120, 310)] {
            let (workspace, layout) = decode_all_nonzero(reference, &[delta]);

            assert_eq!(
                scale_factors(&workspace, &layout),
                Err(ReconstructError::ScaleFactorOutOfRange {
                    group: 0,
                    sfb: 1,
                    value: expected,
                }),
                "参考值 {reference} 加差值 {delta} − 60 应越界"
            );
        }
    }

    /// `Pseudocode 21` 的两个条件里，`sfb_cb != 0` 不构成独立约束。
    ///
    /// 码本 0 的区段不写谱线（`decode_spectral` 直接跳过），该带的
    /// `max_quant_idx` 必然停在 0，于是第二个条件已经把它挡住了。删掉第一个
    /// 条件在单元测试和八条实测流上都毫无反应——那不是覆盖缺口，是等价变换。
    /// 保留它是为了与规范原文逐行对应。
    #[test]
    fn the_codebook_condition_is_implied_by_the_magnitude_condition() {
        let layout = long_frame_layout(2);
        let mut buf = BitBuf::new();
        buf.push_bits(0, 4); // sect_cb = 0，无谱线数据
        buf.push_bits(0, 5); // sect_len = 1
        buf.push_bits(1, 4); // sect_cb = 1
        buf.push_bits(0, 5); // sect_len = 1
        buf.push_symbol(&tables::ASF_HCB_1, 0); // sfb 1：四个 −1
        buf.push_bits(90, 8);
        buf.push(false); // b_snf_data_exists

        let mut reader = BitReader::new(buf.as_slice());
        let mut workspace = AsfWorkspace::new();
        workspace.decode(&mut reader, &layout).expect("谱数据");

        assert_eq!(workspace.sfb_codebook(0, 0), Some(0));
        assert_eq!(
            workspace.max_quant_idx(0, 0),
            Some(0),
            "码本 0 的频带不写谱线，幅度上界应停在 0"
        );

        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");

        assert_eq!(factors.get(0, 0), None, "无码本频带不参与标度因子链");
        assert_eq!(factors.get(0, 1), Some(90), "链的起点落在首个有谱线的频带");
    }

    /// `k_sf = 2^((sf − 100) / 4)` 在四个整数幂点上必须精确。
    #[test]
    fn the_gain_is_exact_at_integer_powers_of_two() {
        assert_eq!(scale_factor_gain(100), 1.0, "sf = 100 是增益的原点");
        assert_eq!(scale_factor_gain(104), 2.0);
        assert_eq!(scale_factor_gain(96), 0.5);
        assert_eq!(scale_factor_gain(140), 1024.0, "2^10");
        assert_eq!(scale_factor_gain(60), 1.0 / 1024.0, "2^−10");
    }

    /// 四分之一步进恰好是 `2^(1/4)` 的连乘，且 `sf < 100` 要下取整。
    ///
    /// 截断除法会把 `−99 / 4` 算成 `−24`，据此得到的增益比正确值大一倍——
    /// 恰好是 `sf = 99` 与 `sf = 103` 分不清的那类错误。
    #[test]
    fn the_gain_steps_by_a_quarter_octave() {
        for step in 0..4u8 {
            let sf = 100u8.saturating_add(step);
            let expected = QUARTER_POWERS
                .get(usize::from(step))
                .copied()
                .expect("四个常量");
            assert_eq!(scale_factor_gain(sf), expected, "sf = {sf}");
        }
        // 每四步恰好翻倍，跨越原点两侧。
        for sf in 60u8..=250 {
            let low = scale_factor_gain(sf);
            let high = scale_factor_gain(sf.saturating_add(4));
            assert_eq!(high, low * 2.0, "sf = {sf} 到 {} 应恰好翻倍", sf + 4);
        }
        assert_eq!(
            scale_factor_gain(99),
            QUARTER_POWERS[3] / 2.0,
            "sf < 100 要下取整"
        );
    }

    /// 四个常量确实是 `2^(r/4)`：各自的四次方等于 `2^r`。
    ///
    /// 上一个用例拿 `QUARTER_POWERS[step]` 当期望值，那是自证的——把常量表
    /// 打乱顺序，等式两边一起变，用例照过。这里给出不依赖表自身的判据。
    #[test]
    fn the_quarter_powers_are_what_they_claim() {
        extern crate std;
        for (r, &value) in QUARTER_POWERS.iter().enumerate() {
            let fourth = f64::from(value).powi(4);
            let expected = f64::from(1u32 << r);
            assert!(
                (fourth - expected).abs() < expected * 1e-6,
                "QUARTER_POWERS[{r}] = {value} 的四次方是 {fourth}，应为 {expected}"
            );
        }
    }

    /// 四个常量是各自实数值的**正确舍入**，用整数判据验证。
    ///
    /// 上一条的四次方对照有 `1e-6` 容差，差 1 ulp 照过；而 ADR-0002 声称增益
    /// 「在 f32 上精确到最后一位」，这个声称需要同等强度的判据。判据与反量化
    /// 表同源：`v = m × 2^e` 是 `2^(r/4)` 的最近 f32，当且仅当 `2^r` 落在两个
    /// 邻接中点的四次方之间，即
    /// `((2m−1)·2^(e−1))^4 ≤ 2^r ≤ ((2m+1)·2^(e−1))^4`。全整数，最大 `2^100`。
    #[test]
    fn the_quarter_powers_are_correctly_rounded() {
        for (r, &value) in QUARTER_POWERS.iter().enumerate() {
            let bits = value.to_bits();
            let mantissa = u128::from((bits & 0x007F_FFFF) | 0x0080_0000);
            let exponent = i32::try_from((bits >> 23) & 0xFF).expect("指数域") - 127 - 23;
            let shift = (4 * (exponent - 1)).unsigned_abs();
            let target = 1u128 << (u32::try_from(r).expect("下标") + shift);
            let low = (2 * mantissa - 1).pow(4);
            let high = (2 * mantissa + 1).pow(4);
            assert!(
                low <= target && target <= high,
                "QUARTER_POWERS[{r}] = 0x{bits:08X} 不是 2^({r}/4) 的最近 f32"
            );
        }
    }

    /// 增益随标度因子严格单调递增。
    ///
    /// 这条同样不依赖常量表的取值，只依赖它是个递增的四分之一八度阶梯；
    /// 常量顺序一乱，`sf = 101…103` 之间立刻不单调。
    #[test]
    fn the_gain_increases_with_the_scale_factor() {
        for sf in 0..255u8 {
            let low = scale_factor_gain(sf);
            let high = scale_factor_gain(sf.saturating_add(1));
            assert!(low < high, "sf = {sf} 的增益 {low} 不小于下一档的 {high}");
        }
    }

    /// 增益在规范允许的两端都不溢出、不转为次正规。
    #[test]
    fn the_gain_stays_normal_across_the_whole_range() {
        for sf in 0..=255u8 {
            let gain = scale_factor_gain(sf);
            assert!(gain.is_finite() && gain > 0.0, "sf = {sf} 的增益是 {gain}");
            assert!(gain.is_normal(), "sf = {sf} 的增益 {gain} 落进了次正规区");
        }
        assert_eq!(scale_factor_gain(0), f32::from_bits(0x3300_0000), "2^−25");
        assert_eq!(
            scale_factor_gain(255),
            scale_factor_gain(252) * QUARTER_POWERS[3]
        );
    }

    /// 整帧缩放：逐条谱线等于 `k_sf × sign(q) × |q|^(4/3)`。
    #[test]
    fn scaling_applies_the_band_gain_to_every_line() {
        // sf = 104 即增益 2；码本 1 的符号 0 解出四个 −1。
        let (workspace, layout) = decode_all_nonzero(104, &[]);
        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");
        assert_eq!(factors.get(0, 0), Some(104));

        let mut out = [0.0f32; 64];
        let written = scale_spectrum(&workspace, &layout, &factors, &mut out).expect("缩放");

        assert_eq!(written, usize::try_from(layout.total_lines()).unwrap());
        for (index, value) in out.iter().take(4).enumerate() {
            assert_eq!(*value, -2.0, "第 {index} 条谱线：−1^(4/3) × 2");
        }
    }

    /// 无标度因子的频带写 0，而不是沿用上一个增益。
    #[test]
    fn bands_without_a_scale_factor_are_zeroed() {
        let layout = long_frame_layout(2);
        let mut buf = BitBuf::new();
        buf.push_bits(1, 4); // sect_cb = 1
        buf.push_bits(1, 5); // sect_len = 2
        buf.push_symbol(&tables::ASF_HCB_1, 0); // sfb 0：四个 −1
        buf.push_symbol(&tables::ASF_HCB_1, 40); // sfb 1：四个 0
        buf.push_bits(104, 8);
        buf.push(false);

        let mut reader = BitReader::new(buf.as_slice());
        let mut workspace = AsfWorkspace::new();
        workspace.decode(&mut reader, &layout).expect("谱数据");
        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");
        assert_eq!(factors.get(0, 1), None);

        let mut out = [7.0f32; 64]; // 预填非零，确认确实被覆盖
        scale_spectrum(&workspace, &layout, &factors, &mut out).expect("缩放");

        assert_eq!(&out[0..4], &[-2.0; 4]);
        assert_eq!(&out[4..8], &[0.0; 4], "全零频带应写 0");
    }

    /// 输出缓冲不足时报错，而不是只写一部分。
    #[test]
    fn scaling_rejects_a_short_output_buffer() {
        let (workspace, layout) = decode_all_nonzero(104, &[]);
        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");
        let lines = usize::try_from(layout.total_lines()).unwrap();

        let mut out = [0.0f32; 3];
        assert_eq!(
            scale_spectrum(&workspace, &layout, &factors, &mut out),
            Err(ReconstructError::OutputTooSmall {
                needed: lines,
                provided: 3,
            })
        );
    }

    /// 工作区、标度因子和布局是同一个语义单元，混用时必须失败而非输出静音。
    #[test]
    fn scaling_rejects_inputs_from_different_layouts() {
        let (workspace, layout) = decode_all_nonzero(104, &[]);
        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");
        let mut out = [7.0f32; 64];

        assert_eq!(
            scale_spectrum(&workspace, &AsfWindowLayout::empty(), &factors, &mut out),
            Err(ReconstructError::LayoutMismatch {
                workspace_matches: false,
                factors_match: false,
            })
        );
        assert_eq!(out, [7.0; 64], "失败前不得清空调用方缓冲");

        assert_eq!(
            scale_spectrum(&workspace, &layout, &ScaleFactors::new(), &mut out),
            Err(ReconstructError::LayoutMismatch {
                workspace_matches: true,
                factors_match: false,
            })
        );
        assert_eq!(out, [7.0; 64]);
    }

    /// `scale_factors` 自身也必须核对，不能只靠下游的缩放兜住。
    ///
    /// 混用返回的是一份「合法但不完整」的标度因子：宽谱配窄布局只还原了 1
    /// 个频带而非 4 个，没有任何错误。只统计标度因子而不做缩放的调用方，
    /// 拿不到任何信号。
    #[test]
    fn scale_factors_rejects_a_foreign_layout() {
        let (wide, wide_layout) = decode_all_nonzero(104, &[70, 55, 61]);
        let (_, narrow_layout) = decode_all_nonzero(104, &[]);

        assert_eq!(
            scale_factors(&wide, &wide_layout).map(|f| f.count()),
            Ok(4),
            "同源时应还原四个频带"
        );
        assert_eq!(
            scale_factors(&wide, &narrow_layout),
            Err(ReconstructError::LayoutMismatch {
                workspace_matches: false,
                factors_match: true,
            })
        );
    }

    /// 标度因子与布局自洽、只有谱工作区使用了另一种布局。
    ///
    /// 前一条的两个场景里 `factors_match` 都是假，因此只核对标度因子的实现
    /// 也能照过。这里让 `factors` 与 `layout` 完全一致，唯一的错配在谱数据
    /// 上，约束工作区的布局键也确实参与核对。
    #[test]
    fn scaling_rejects_a_foreign_workspace_alone() {
        let (foreign, _) = decode_all_nonzero(104, &[]);
        let (workspace, layout) = decode_all_nonzero(104, &[70]);
        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");
        let mut out = [7.0f32; 64];

        assert_eq!(
            scale_spectrum(&foreign, &layout, &factors, &mut out),
            Err(ReconstructError::LayoutMismatch {
                workspace_matches: false,
                factors_match: true,
            })
        );
        assert_eq!(out, [7.0; 64], "失败前不得清空调用方缓冲");
    }

    /// 共享 `sf_info` 的声道布局完全相同，仍不能交叉使用各自的标度因子。
    #[test]
    fn scaling_rejects_foreign_factors_with_the_same_layout() {
        // 两个工作区的频带划分与量化谱线完全相同，只有参考标度因子不同：
        // sf=104 的增益为 2，sf=108 的增益为 4。
        let (first, first_layout) = decode_all_nonzero(104, &[]);
        let (second, second_layout) = decode_all_nonzero(108, &[]);
        let first_factors = scale_factors(&first, &first_layout).expect("第一声道标度因子");
        let second_factors = scale_factors(&second, &second_layout).expect("第二声道标度因子");
        let mut out = [7.0f32; 64];

        assert_eq!(
            first_layout.key(),
            second_layout.key(),
            "测试前提是布局相同"
        );
        assert_eq!(
            scale_spectrum(&second, &second_layout, &first_factors, &mut out),
            Err(ReconstructError::ScaleFactorSourceMismatch)
        );
        assert_eq!(out, [7.0; 64], "失败前不得改写调用方缓冲");

        scale_spectrum(&second, &second_layout, &second_factors, &mut out)
            .expect("同源标度因子应通过");
        assert_eq!(&out[..4], &[-4.0; 4]);
    }

    /// 解组是一个排列：每条输入谱线恰好落到一个输出位置。
    ///
    /// 这是本步最强的自洽判据——搬运不做任何计算，因此「不重不漏」等价于
    /// 正确。用 `1…lines` 编码输入下标，输出里的每个非零值反查回唯一的来源。
    #[test]
    fn ungrouping_is_a_permutation() {
        // 两种分组形态都要走到：组内交织由前者约束，组间窗口累加由后者约束。
        // 只测一组时，「窗口下标忘记跨组累加」不会改变任何输出。
        let layouts = [
            one_group_two_windows(1),
            one_group_two_windows(2),
            one_group_two_windows(5),
            two_group_layout(1),
            two_group_layout(2),
        ];
        for layout in &layouts {
            let max_sfb = layout.max_sfb(0).unwrap_or(0);
            let lines = usize::try_from(layout.total_lines()).expect("谱线数");
            let mut scaled = [0.0f32; MAX_SPECTRAL_LINES];
            for (index, slot) in scaled.iter_mut().take(lines).enumerate() {
                *slot = index as f32 + 1.0;
            }
            let mut out = [0.0f32; MAX_SPECTRAL_LINES];

            let total = ungroup_spectrum(layout, &scaled, &mut out).expect("解组");

            assert_eq!(total, 1024, "两个 512 点窗口应铺满 frame_len_base");
            let mut used = [false; MAX_SPECTRAL_LINES];
            let mut moved = 0usize;
            for &value in out.iter().take(total) {
                if value == 0.0 {
                    continue;
                }
                let index = (value - 1.0) as usize;
                assert!(index < lines, "输出值 {value} 不在输入范围内");
                let slot = used.get_mut(index).expect("下标已校验");
                assert!(!*slot, "输入第 {index} 条被搬了两次");
                *slot = true;
                moved = moved.saturating_add(1);
            }
            assert_eq!(moved, lines, "max_sfb = {max_sfb}：应搬运全部输入");
        }
    }

    /// 组内两窗口按频带交织：码流里「带 0 的两窗口」相邻，输出里相距一个窗口。
    ///
    /// 512 点变换的前两个频带各宽 4 线（表 B.6 的 `0, 4, 8`），因此码流顺序是
    /// 带 0 窗 0、带 0 窗 1、带 1 窗 0、带 1 窗 1，而输出里窗 1 整体后移 512。
    #[test]
    fn ungrouping_interleaves_windows_within_a_group() {
        let layout = one_group_two_windows(2);
        assert_eq!(layout.total_lines(), 16);
        let mut scaled = [0.0f32; MAX_SPECTRAL_LINES];
        for (index, slot) in scaled.iter_mut().take(16).enumerate() {
            *slot = index as f32 + 1.0;
        }
        let mut out = [0.0f32; MAX_SPECTRAL_LINES];

        ungroup_spectrum(&layout, &scaled, &mut out).expect("解组");

        let at = |range: core::ops::Range<usize>| out.get(range).expect("范围应有效");
        assert_eq!(at(0..4), &[1.0, 2.0, 3.0, 4.0], "窗 0 的带 0");
        assert_eq!(at(4..8), &[9.0, 10.0, 11.0, 12.0], "窗 0 的带 1");
        assert_eq!(at(512..516), &[5.0, 6.0, 7.0, 8.0], "窗 1 的带 0");
        assert_eq!(at(516..520), &[13.0, 14.0, 15.0, 16.0], "窗 1 的带 1");
        assert_eq!(at(8..12), &[0.0; 4], "max_sfb 之上补零");
    }

    /// 单窗口的长帧解组后是恒等搬运，其余补零。
    #[test]
    fn ungrouping_a_long_frame_is_the_identity() {
        let layout = long_frame_layout(3);
        let lines = usize::try_from(layout.total_lines()).expect("谱线数");
        assert_eq!(layout.num_windows(), 1);
        let mut scaled = [0.0f32; MAX_SPECTRAL_LINES];
        for (index, slot) in scaled.iter_mut().take(lines).enumerate() {
            *slot = index as f32 + 1.0;
        }
        // 预填非零：`max_sfb` 之上的谱线必须被清零，而不是留下调用方的旧值。
        let mut out = [7.0f32; MAX_SPECTRAL_LINES];

        let total = ungroup_spectrum(&layout, &scaled, &mut out).expect("解组");

        assert_eq!(total, 2048);
        assert_eq!(out.get(..lines), scaled.get(..lines), "长帧不重排");
        assert!(
            out.get(lines..total)
                .expect("输出应覆盖全帧")
                .iter()
                .all(|value| *value == 0.0),
            "其余补零"
        );
    }

    /// 输入不足或输出装不下时报错，而不是搬一半。
    #[test]
    fn ungrouping_rejects_undersized_buffers() {
        extern crate std;
        use std::string::ToString;

        let layout = one_group_two_windows(2);
        let scaled = [1.0f32; MAX_SPECTRAL_LINES];
        let mut out = [7.0f32; MAX_SPECTRAL_LINES];

        assert_eq!(
            ungroup_spectrum(&layout, &scaled[..4], &mut out),
            Err(ReconstructError::InputTooSmall {
                needed: 16,
                provided: 4,
            })
        );
        let error =
            ungroup_spectrum(&layout, &scaled, &mut out[..100]).expect_err("输出不足必须失败");
        assert_eq!(
            error,
            ReconstructError::OutputTooSmall {
                needed: 1024,
                provided: 100,
            }
        );
        assert_eq!(
            error.to_string(),
            "Output buffer requires 1024 spectral lines, but only 100 were provided"
        );
        assert!(out.iter().all(|v| *v == 7.0), "失败前不得改写调用方缓冲");
    }

    /// 重建与解析必须走同一个频带上界，否则 DPCM 会整体错位。
    #[test]
    fn reconstruction_walks_the_same_bands_as_parsing() {
        let (workspace, layout) = decode_all_nonzero(120, &[61, 62, 59]);

        let factors = scale_factors(&workspace, &layout).expect("标度因子应可还原");

        // 解析侧写了三个差值，重建侧就应恰好产出四个标度因子。
        let parsed = (0..crate::asf::spectrum::coded_band_count(&layout, 0))
            .filter(|&sfb| workspace.dpcm_sf(0, sfb).is_some())
            .count();
        assert_eq!(parsed, 3, "解析侧应写下三个差值");
        assert_eq!(factors.count(), parsed + 1, "首带不消费差值，故多出一个");
        assert_eq!(factors.get(0, 3), Some(122), "120 + 1 + 2 − 1");
    }
}
