//! ASF 的成帧与窗口分组。
//!
//! 覆盖 `TS103190-1:v1.4.1` 的 `asf_transform_info()`（`4.2.8.1` 表 37）与
//! `asf_psy_info()`（`4.2.8.2` 表 38），以及 `4.3.6` 的 `Pseudocode 2` 至
//! `Pseudocode 5`：窗口数、窗口分组、`get_max_sfb(g)`、`get_transf_length(g)`
//! 与 `sect_sfb_offset[g][sfb]`。
//!
//! 本模块不做熵解码，因此不依赖附录 A 的 Huffman 码本。
//! 当前只支持 44,1 kHz 与 48 kHz；96 kHz、192 kHz 的表格尚未导入，解析
//! 入口会在读取码流前明确拒绝这些采样率。

use super::tables::{
    n_grp_bits_long_base, n_grp_bits_short_base, n_msfb_bits_48, n_side_bits_48, num_sfb_48,
    num_windows_first_half, sfb_offsets_48, transform_length_48,
};
use core::fmt;
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

/// 一帧内的最大窗口数。
///
/// `4.3.6.2.6` 的 NOTE 列出全部可能取值，上界为 16（表 109 对角线首项
/// `n_grp_bits = 15`，`num_windows = n_grp_bits + 1`）。
pub const MAX_WINDOWS: usize = 16;

/// 单个变换长度下的最大尺度因子频带数。
///
/// 取表 B.1 的最大值 63（变换长度 2 048）。
pub const MAX_SFB: usize = 63;

/// ASF 成帧解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsfError {
    /// 读取比特时越过了数据末尾。
    Read(ReadError),
    /// `frame_len_base` 不在表 99 至表 105 内。
    UnsupportedFrameLenBase {
        /// 码流声明的帧长基准。
        frame_len_base: u16,
    },
    /// 当前尚未支持该采样频率下的 ASF 表格。
    UnsupportedSamplingFrequency {
        /// 目标采样频率，单位为 Hz。
        sampling_frequency_hz: u32,
    },
    /// `transf_length` 索引在该 `frame_len_base` 下没有对应的变换长度。
    ///
    /// 表 103 中标为 `×` 的组合会走到这里。
    UnsupportedTransformIndex {
        /// 帧长基准。
        frame_len_base: u16,
        /// 码流声明的两比特索引。
        index: u8,
    },
    /// `max_sfb` 超过了该变换长度的 `num_sfb`。
    ///
    /// `4.3.6.2.2` 规定该值不大于 `num_sfb`，因此这是码流层面的越界而非
    /// 本实现的限制。它同时是解析错位的早期信号。
    MaxSfbOutOfRange {
        /// 解出的 `max_sfb`。
        max_sfb: u8,
        /// 该变换长度允许的上界。
        num_sfb: u8,
        /// 对应的变换长度。
        transform_length: u16,
    },
    /// 推导出的窗口数超出 [`MAX_WINDOWS`]。
    TooManyWindows {
        /// 推导结果。
        num_windows: u32,
    },
    /// 谱线总数超过了帧长。
    ///
    /// 各窗口恰好铺满一帧，而 `sfb_offset[max_sfb]` 不超过变换长度，因此
    /// `sect_sfb_offset` 的末值必然不超过 `frame_len_base`。超出说明分组或
    /// 偏移推导有误。
    LineCountExceedsFrame {
        /// 推导出的谱线总数。
        lines: u32,
        /// 帧长基准。
        frame_len_base: u16,
    },
}

impl fmt::Display for AsfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsfError::Read(error) => write!(f, "{error}"),
            AsfError::UnsupportedFrameLenBase { frame_len_base } => {
                write!(
                    f,
                    "frame_len_base {frame_len_base} is not in Tables 99 through 105"
                )
            }
            AsfError::UnsupportedSamplingFrequency {
                sampling_frequency_hz,
            } => write!(
                f,
                "ASF framing does not support {sampling_frequency_hz} Hz; only 44100 Hz and 48000 Hz are supported"
            ),
            AsfError::UnsupportedTransformIndex {
                frame_len_base,
                index,
            } => write!(
                f,
                "transf_length index {index} has no corresponding transform length for frame_len_base {frame_len_base}"
            ),
            AsfError::MaxSfbOutOfRange {
                max_sfb,
                num_sfb,
                transform_length,
            } => write!(
                f,
                "max_sfb {max_sfb} for transform length {transform_length} exceeds limit {num_sfb}"
            ),
            AsfError::TooManyWindows { num_windows } => {
                write!(
                    f,
                    "Derived {num_windows} windows, exceeding limit {MAX_WINDOWS}"
                )
            }
            AsfError::LineCountExceedsFrame {
                lines,
                frame_len_base,
            } => write!(
                f,
                "Total spectral-line count {lines} exceeds frame length {frame_len_base}"
            ),
        }
    }
}

impl core::error::Error for AsfError {}

impl From<ReadError> for AsfError {
    fn from(error: ReadError) -> Self {
        AsfError::Read(error)
    }
}

/// `asf_transform_info()` 解出的成帧形态。
///
/// 三个分支互斥，且由 `frame_len_base` 与 `b_long_frame` 唯一决定。把它们做成
/// 枚举而非「一个布尔加两个索引」，是因为 `b_long_frame` 只在
/// `frame_len_base >= 1536` 时传输：另一条路径下它根本不存在，用 `Option`
/// 表达会让每个使用处都要重新判断哪种组合合法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsfFraming {
    /// `frame_len_base >= 1536` 且 `b_long_frame == 1`：整帧一个变换。
    Long,
    /// `frame_len_base >= 1536` 且 `b_long_frame == 0`：两个半帧各自成帧。
    Split {
        /// `transf_length[0]`，前半帧。
        first: u8,
        /// `transf_length[1]`，后半帧。
        second: u8,
    },
    /// `frame_len_base < 1536`：只传输一个 `transf_length`。
    ///
    /// 此时不传输 `b_long_frame`。表 110 在索引指向整帧长度时给出
    /// `n_grp_bits = 0`，`Pseudocode 3` 由此得到 `num_windows = 1`，与长帧
    /// 同解，因此无需另设标志。
    Single {
        /// `transf_length`。
        index: u8,
    },
}

/// `asf_transform_info()` 的解析结果，见 `4.2.8.1` 表 37。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsfTransformInfo {
    /// 序列配置给出的帧长基准，不由本元素传输。
    pub frame_len_base: u16,
    /// 本次解码使用的采样频率，单位为 Hz。
    pub sampling_frequency_hz: u32,
    /// 解出的成帧形态。
    pub framing: AsfFraming,
}

impl AsfTransformInfo {
    /// 解析 `asf_transform_info()`。
    ///
    /// # Errors
    ///
    /// 目前仅接受 44 100 Hz 与 48 000 Hz。采样频率或 `frame_len_base` 不受支持、
    /// 或索引组合在表 103 中标为 `×` 时报错；数据不足时返回
    /// [`AsfError::Read`]。配置在读取任何码流比特前完成校验。
    pub fn parse(
        reader: &mut BitReader<'_>,
        frame_len_base: u16,
        sampling_frequency_hz: u32,
    ) -> Result<Self, AsfError> {
        ensure_supported_sampling_frequency(sampling_frequency_hz)?;
        ensure_supported_frame_len_base(frame_len_base)?;

        let framing = if frame_len_base >= 1536 {
            if reader.read_flag()? {
                AsfFraming::Long
            } else {
                let first = u8::try_from(reader.read_bits(2)?).unwrap_or(u8::MAX);
                let second = u8::try_from(reader.read_bits(2)?).unwrap_or(u8::MAX);
                AsfFraming::Split { first, second }
            }
        } else {
            AsfFraming::Single {
                index: u8::try_from(reader.read_bits(2)?).unwrap_or(u8::MAX),
            }
        };

        let info = Self {
            frame_len_base,
            sampling_frequency_hz,
            framing,
        };
        // 立刻把索引换算成变换长度：组合非法时应在此处失败，而不是等到
        // 后续按错误的长度去查频带表。
        for slot in info.slots() {
            info.transform_length(slot)?;
        }
        Ok(info)
    }

    /// 本次成帧实际使用的 `transf_length` 索引，按半帧顺序。
    fn slots(&self) -> [u8; 2] {
        match self.framing {
            AsfFraming::Long => [4, 4],
            AsfFraming::Split { first, second } => [first, second],
            AsfFraming::Single { index } => [index, index],
        }
    }

    /// 由 `transf_length` 索引取变换长度。
    ///
    /// # Errors
    ///
    /// 索引在该 `frame_len_base` 下没有对应长度时返回
    /// [`AsfError::UnsupportedTransformIndex`]。
    pub fn transform_length(&self, index: u8) -> Result<u16, AsfError> {
        ensure_supported_sampling_frequency(self.sampling_frequency_hz)?;
        ensure_supported_frame_len_base(self.frame_len_base)?;
        transform_length_48(self.frame_len_base, index).ok_or(AsfError::UnsupportedTransformIndex {
            frame_len_base: self.frame_len_base,
            index,
        })
    }

    /// `asf_psy_info()` 中的 `b_different_framing`，见表 38。
    ///
    /// 只有 `frame_len_base >= 1536` 的非长帧、且两个半帧变换长度不同时为真。
    #[must_use]
    pub const fn different_framing(&self) -> bool {
        matches!(self.framing, AsfFraming::Split { first, second } if first != second)
    }

    /// `n_grp_bits`，见 `4.3.6.2.4` 表 109 与表 110。
    ///
    /// # Errors
    ///
    /// 组合不在表内时返回 [`AsfError::UnsupportedTransformIndex`]。
    pub fn n_grp_bits(&self) -> Result<u8, AsfError> {
        match self.framing {
            // 4.3.6.2.4：长帧且 frame_len_base >= 1536 时为 0。
            AsfFraming::Long => Ok(0),
            AsfFraming::Split { first, second } => {
                n_grp_bits_long_base(first, second).ok_or(AsfError::UnsupportedTransformIndex {
                    frame_len_base: self.frame_len_base,
                    index: first,
                })
            }
            AsfFraming::Single { index } => n_grp_bits_short_base(self.frame_len_base, index)
                .ok_or(AsfError::UnsupportedTransformIndex {
                    frame_len_base: self.frame_len_base,
                    index,
                }),
        }
    }
}

fn ensure_supported_sampling_frequency(sampling_frequency_hz: u32) -> Result<(), AsfError> {
    if matches!(sampling_frequency_hz, 44_100 | 48_000) {
        return Ok(());
    }
    Err(AsfError::UnsupportedSamplingFrequency {
        sampling_frequency_hz,
    })
}

fn ensure_supported_frame_len_base(frame_len_base: u16) -> Result<(), AsfError> {
    if transform_length_48(frame_len_base, 4).is_some() {
        return Ok(());
    }
    Err(AsfError::UnsupportedFrameLenBase { frame_len_base })
}

/// `asf_psy_info()` 的调用上下文，见表 38 的两个形参。
///
/// 两者在 `two_channel_data()` 与 `three_channel_data()` 中均为 0（表 26、
/// 表 27 直接写死 `sf_info(ASF, 0, 0)`），只有 `ASPX_ACPL_1` 路径会置位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AsfPsyContext {
    /// `b_dual_maxsfb`：side 声道另有一套 `max_sfb`。
    pub dual_maxsfb: bool,
    /// `b_side_limited`：side 声道的 `max_sfb` 以 `n_side_bits` 传输。
    pub side_limited: bool,
}

/// `asf_psy_info()` 的解析结果，见 `4.2.8.2` 表 38。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsfPsyInfo {
    /// `max_sfb[0]` 与 `max_sfb[1]`；后者仅在 `b_different_framing` 时传输。
    pub max_sfb: [Option<u8>; 2],
    /// `max_sfb_side[0]` 与 `max_sfb_side[1]`。
    pub max_sfb_side: [Option<u8>; 2],
    /// `scale_factor_grouping[i]`，前 `n_grp_bits` 项有效。
    pub scale_factor_grouping: [bool; MAX_WINDOWS],
    /// 实际读入的分组比特数。
    pub n_grp_bits: u8,
    /// 解析时的上下文，`get_max_sfb` 需要它。
    pub context: AsfPsyContext,
}

impl AsfPsyInfo {
    /// 解析 `asf_psy_info(b_dual_maxsfb, b_side_limited)`。
    ///
    /// `max_sfb[i]` 的比特数取决于**该半帧自身**的变换长度（`4.3.6.2.1`），
    /// 因此两个索引可能用不同宽度读取。
    ///
    /// # Errors
    ///
    /// `max_sfb` 超过该变换长度的 `num_sfb` 时返回
    /// [`AsfError::MaxSfbOutOfRange`]；数据不足时返回 [`AsfError::Read`]。
    pub fn parse(
        reader: &mut BitReader<'_>,
        transform: &AsfTransformInfo,
        context: AsfPsyContext,
    ) -> Result<Self, AsfError> {
        let slots = transform.slots();
        let mut out = Self {
            max_sfb: [None; 2],
            max_sfb_side: [None; 2],
            scale_factor_grouping: [false; MAX_WINDOWS],
            n_grp_bits: 0,
            context,
        };

        let halves = if transform.different_framing() { 2 } else { 1 };
        for half in 0..halves {
            let slot = slots.get(half).copied().unwrap_or(0);
            let length = transform.transform_length(slot)?;
            if context.side_limited {
                let bits = n_side_bits_48(length).unwrap_or(0);
                let value = read_max_sfb(reader, bits, length)?;
                assign(&mut out.max_sfb_side, half, value);
            } else {
                let bits = n_msfb_bits_48(length).unwrap_or(0);
                let value = read_max_sfb(reader, bits, length)?;
                assign(&mut out.max_sfb, half, value);
                if context.dual_maxsfb {
                    let value = read_max_sfb(reader, bits, length)?;
                    assign(&mut out.max_sfb_side, half, value);
                }
            }
        }

        let n_grp_bits = transform.n_grp_bits()?;
        out.n_grp_bits = n_grp_bits;
        for index in 0..usize::from(n_grp_bits) {
            let bit = reader.read_flag()?;
            if let Some(slot) = out.scale_factor_grouping.get_mut(index) {
                *slot = bit;
            }
        }
        Ok(out)
    }
}

/// 读入一个 `max_sfb` 并当场校验上界。
fn read_max_sfb(
    reader: &mut BitReader<'_>,
    bits: u8,
    transform_length: u16,
) -> Result<u8, AsfError> {
    let value = u8::try_from(reader.read_bits(u32::from(bits))?).unwrap_or(u8::MAX);
    let num_sfb = num_sfb_48(transform_length).unwrap_or(0);
    if value > num_sfb {
        return Err(AsfError::MaxSfbOutOfRange {
            max_sfb: value,
            num_sfb,
            transform_length,
        });
    }
    Ok(value)
}

fn assign(target: &mut [Option<u8>; 2], index: usize, value: u8) {
    if let Some(slot) = target.get_mut(index) {
        *slot = Some(value);
    }
}

/// 窗口分组与频带偏移，由 `Pseudocode 3` 至 `Pseudocode 5` 推导。
///
/// 全部字段定长，构造过程不分配。`sect_sfb_offset` 是其中最大的一块，
/// 16 × 64 个 `u16`。
#[derive(Debug, Clone)]
pub struct AsfWindowLayout {
    num_windows: u8,
    num_window_groups: u8,
    window_to_group: [u8; MAX_WINDOWS],
    num_win_in_group: [u8; MAX_WINDOWS],
    max_sfb_of_group: [u8; MAX_WINDOWS],
    transf_index_of_group: [u8; MAX_WINDOWS],
    transform_length_of_group: [u16; MAX_WINDOWS],
    sect_sfb_offset: [[u16; MAX_SFB + 1]; MAX_WINDOWS],
    total_lines: u32,
}

/// 将谱工作区和重建结果绑定到生成它们的窗口布局。
///
/// 频带偏移可由这些字段和静态 SFB 表唯一推导；不复制 16 × 65 的偏移表，避免
/// 每个通道工作区再增加约 2 KiB。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "audio-decode")]
pub(crate) struct AsfLayoutKey {
    num_windows: u8,
    num_window_groups: u8,
    num_win_in_group: [u8; MAX_WINDOWS],
    max_sfb_of_group: [u8; MAX_WINDOWS],
    transf_index_of_group: [u8; MAX_WINDOWS],
    transform_length_of_group: [u16; MAX_WINDOWS],
    total_lines: u32,
}

#[cfg(feature = "audio-decode")]
impl AsfLayoutKey {
    pub(crate) const fn empty() -> Self {
        Self {
            num_windows: 0,
            num_window_groups: 0,
            num_win_in_group: [0; MAX_WINDOWS],
            max_sfb_of_group: [0; MAX_WINDOWS],
            transf_index_of_group: [0; MAX_WINDOWS],
            transform_length_of_group: [0; MAX_WINDOWS],
            total_lines: 0,
        }
    }
}

impl AsfWindowLayout {
    /// 一个未填充的布局，供调用方预留槽位。
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            num_windows: 0,
            num_window_groups: 0,
            window_to_group: [0; MAX_WINDOWS],
            num_win_in_group: [0; MAX_WINDOWS],
            max_sfb_of_group: [0; MAX_WINDOWS],
            transf_index_of_group: [0; MAX_WINDOWS],
            transform_length_of_group: [0; MAX_WINDOWS],
            sect_sfb_offset: [[0; MAX_SFB + 1]; MAX_WINDOWS],
            total_lines: 0,
        }
    }

    /// 供谱解析与重建核对调用方没有混用不同声道或帧的布局。
    #[cfg(feature = "audio-decode")]
    pub(crate) const fn key(&self) -> AsfLayoutKey {
        AsfLayoutKey {
            num_windows: self.num_windows,
            num_window_groups: self.num_window_groups,
            num_win_in_group: self.num_win_in_group,
            max_sfb_of_group: self.max_sfb_of_group,
            transf_index_of_group: self.transf_index_of_group,
            transform_length_of_group: self.transform_length_of_group,
            total_lines: self.total_lines,
        }
    }

    /// `sf_info_lfe()` 的布局，见 `4.2.7.2` 表 35。
    ///
    /// 该元素不传输成帧信息：`b_long_frame` 固定为 1、`num_window_groups`
    /// 固定为 1，变换长度即 `frame_len_base`。码流中只有 `max_sfb[0]`，宽度
    /// 取表 106 的 `n_msfbl_bits`。
    ///
    /// # Errors
    ///
    /// 采样率或 `frame_len_base` 不受支持、`max_sfb` 超过 `num_sfb` 时报错。
    pub fn for_lfe(
        frame_len_base: u16,
        sampling_frequency_hz: u32,
        max_sfb: u8,
    ) -> Result<Self, AsfError> {
        ensure_supported_sampling_frequency(sampling_frequency_hz)?;
        ensure_supported_frame_len_base(frame_len_base)?;
        let num_sfb = num_sfb_48(frame_len_base).unwrap_or(0);
        if max_sfb > num_sfb {
            return Err(AsfError::MaxSfbOutOfRange {
                max_sfb,
                num_sfb,
                transform_length: frame_len_base,
            });
        }
        let offsets = sfb_offsets_48(frame_len_base)
            .ok_or(AsfError::UnsupportedFrameLenBase { frame_len_base })?;

        let mut layout = Self::empty();
        layout.num_windows = 1;
        layout.num_window_groups = 1;
        layout.num_win_in_group[0] = 1;
        layout.max_sfb_of_group[0] = max_sfb;
        // 长帧在 get_transf_length() 中以索引 4 表示，见 Pseudocode 2。
        layout.transf_index_of_group[0] = 4;
        layout.transform_length_of_group[0] = frame_len_base;
        for sfb in 0..=usize::from(max_sfb) {
            let Some(value) = offsets.get(sfb).copied() else {
                break;
            };
            if let Some(row) = layout.sect_sfb_offset.get_mut(0)
                && let Some(slot) = row.get_mut(sfb)
            {
                *slot = value;
            }
        }
        layout.total_lines = u32::from(offsets.get(usize::from(max_sfb)).copied().unwrap_or(0));
        Ok(layout)
    }

    /// 由成帧与 psy 信息推导整套窗口分组。
    ///
    /// `side_channel` 对应 `Pseudocode 5` 注释中的 `b_side_channel`，仅在
    /// `stereo_codec_mode == ASPX_ACPL_1` 解码 side 声道时为真。
    ///
    /// # Errors
    ///
    /// 窗口数越界、变换长度不在表内，或谱线总数超过帧长时报错。
    pub fn derive(
        transform: &AsfTransformInfo,
        psy: &AsfPsyInfo,
        side_channel: bool,
    ) -> Result<Self, AsfError> {
        let mut layout = Self {
            num_windows: 1,
            num_window_groups: 1,
            window_to_group: [0; MAX_WINDOWS],
            num_win_in_group: [0; MAX_WINDOWS],
            max_sfb_of_group: [0; MAX_WINDOWS],
            transf_index_of_group: [0; MAX_WINDOWS],
            transform_length_of_group: [0; MAX_WINDOWS],
            sect_sfb_offset: [[0; MAX_SFB + 1]; MAX_WINDOWS],
            total_lines: 0,
        };

        layout.derive_groups(transform, psy)?;
        layout.derive_offsets(transform, psy, side_channel)?;
        Ok(layout)
    }

    /// `Pseudocode 3`：`num_windows`、`num_window_groups` 与 `window_to_group`。
    fn derive_groups(
        &mut self,
        transform: &AsfTransformInfo,
        psy: &AsfPsyInfo,
    ) -> Result<(), AsfError> {
        if matches!(transform.framing, AsfFraming::Long) {
            return Ok(());
        }

        let n_grp_bits = u32::from(psy.n_grp_bits);
        let mut num_windows = n_grp_bits.saturating_add(1);
        let mut grouping = psy.scale_factor_grouping;

        if transform.different_framing() {
            let AsfFraming::Split { first, .. } = transform.framing else {
                unreachable!("different_framing implies Split")
            };
            let windows_0 = usize::from(num_windows_first_half(first).ok_or(
                AsfError::UnsupportedTransformIndex {
                    frame_len_base: transform.frame_len_base,
                    index: first,
                },
            )?);
            // 后半帧的分组比特整体右移一位，让两个半帧的交界处不成组。
            let mut index = usize::try_from(n_grp_bits).unwrap_or(0);
            while index >= windows_0 {
                let previous = grouping
                    .get(index.saturating_sub(1))
                    .copied()
                    .unwrap_or(false);
                if let Some(slot) = grouping.get_mut(index) {
                    *slot = previous;
                }
                index = index.saturating_sub(1);
            }
            if let Some(slot) = grouping.get_mut(windows_0.saturating_sub(1)) {
                *slot = false;
            }
            num_windows = num_windows.saturating_add(1);
        }

        if num_windows > MAX_WINDOWS as u32 {
            return Err(AsfError::TooManyWindows { num_windows });
        }
        self.num_windows = u8::try_from(num_windows).unwrap_or(u8::MAX);

        let mut groups: u32 = 1;
        for index in 0..num_windows.saturating_sub(1) {
            let slot = usize::try_from(index).unwrap_or(0);
            if !grouping.get(slot).copied().unwrap_or(false) {
                groups = groups.saturating_add(1);
            }
            if let Some(target) = self.window_to_group.get_mut(slot.saturating_add(1)) {
                *target = u8::try_from(groups.saturating_sub(1)).unwrap_or(u8::MAX);
            }
        }
        self.num_window_groups = u8::try_from(groups).unwrap_or(u8::MAX);
        Ok(())
    }

    /// `Pseudocode 2`、`Pseudocode 4` 与 `Pseudocode 5`。
    fn derive_offsets(
        &mut self,
        transform: &AsfTransformInfo,
        psy: &AsfPsyInfo,
        side_channel: bool,
    ) -> Result<(), AsfError> {
        let mut group_offset: u32 = 0;
        for group in 0..usize::from(self.num_window_groups) {
            let mut windows: u32 = 0;
            for window in 0..usize::from(self.num_windows) {
                if self.window_to_group.get(window).copied().unwrap_or(0)
                    == u8::try_from(group).unwrap_or(u8::MAX)
                {
                    windows = windows.saturating_add(1);
                }
            }

            let half = self.half_of_group(transform, group);
            let index = transform.slots().get(half).copied().unwrap_or(0);
            let length = transform.transform_length(index)?;
            let max_sfb = select_max_sfb(psy, half, side_channel);
            let offsets = sfb_offsets_48(length).ok_or(AsfError::UnsupportedTransformIndex {
                frame_len_base: transform.frame_len_base,
                index,
            })?;
            if usize::from(max_sfb) >= offsets.len() {
                return Err(AsfError::MaxSfbOutOfRange {
                    max_sfb,
                    num_sfb: num_sfb_48(length).unwrap_or(0),
                    transform_length: length,
                });
            }

            for sfb in 0..=usize::from(max_sfb) {
                let base = u32::from(offsets.get(sfb).copied().unwrap_or(0));
                let value = group_offset.saturating_add(base.saturating_mul(windows));
                if let Some(row) = self.sect_sfb_offset.get_mut(group)
                    && let Some(slot) = row.get_mut(sfb)
                {
                    *slot = u16::try_from(value).unwrap_or(u16::MAX);
                }
            }

            let span = u32::from(offsets.get(usize::from(max_sfb)).copied().unwrap_or(0));
            group_offset = group_offset.saturating_add(span.saturating_mul(windows));

            store(
                &mut self.num_win_in_group,
                group,
                u8::try_from(windows).unwrap_or(u8::MAX),
            );
            store(&mut self.max_sfb_of_group, group, max_sfb);
            store(&mut self.transf_index_of_group, group, index);
            if let Some(slot) = self.transform_length_of_group.get_mut(group) {
                *slot = length;
            }
        }

        if group_offset > u32::from(transform.frame_len_base) {
            return Err(AsfError::LineCountExceedsFrame {
                lines: group_offset,
                frame_len_base: transform.frame_len_base,
            });
        }
        self.total_lines = group_offset;
        Ok(())
    }

    /// 组 `g` 属于哪个半帧，即 `Pseudocode 2` 与 `Pseudocode 5` 中的 `idx`。
    fn half_of_group(&self, transform: &AsfTransformInfo, group: usize) -> usize {
        let AsfFraming::Split { first, .. } = transform.framing else {
            return 0;
        };
        let Some(windows_0) = num_windows_first_half(first) else {
            return 0;
        };
        let boundary = self
            .window_to_group
            .get(usize::from(windows_0))
            .copied()
            .unwrap_or(0);
        if group >= usize::from(boundary) { 1 } else { 0 }
    }

    /// 窗口总数。
    #[must_use]
    pub const fn num_windows(&self) -> u8 {
        self.num_windows
    }

    /// 窗口组数。
    #[must_use]
    pub const fn num_window_groups(&self) -> u8 {
        self.num_window_groups
    }

    /// 窗口 `w` 所属的组号。
    #[must_use]
    pub fn window_to_group(&self, window: usize) -> Option<u8> {
        if window >= usize::from(self.num_windows) {
            return None;
        }
        self.window_to_group.get(window).copied()
    }

    /// 组 `g` 内的窗口数。
    #[must_use]
    pub fn num_win_in_group(&self, group: usize) -> Option<u8> {
        if group >= usize::from(self.num_window_groups) {
            return None;
        }
        self.num_win_in_group.get(group).copied()
    }

    /// `get_max_sfb(g)`，见 `Pseudocode 5`。
    #[must_use]
    pub fn max_sfb(&self, group: usize) -> Option<u8> {
        if group >= usize::from(self.num_window_groups) {
            return None;
        }
        self.max_sfb_of_group.get(group).copied()
    }

    /// `get_transf_length(g)` 返回的索引，见 `Pseudocode 2`。
    ///
    /// `asf_section_data()` 用它选择 `n_sect_bits`：索引不大于 2 时取 3 比特，
    /// 否则取 5 比特。
    #[must_use]
    pub fn transf_length_index(&self, group: usize) -> Option<u8> {
        if group >= usize::from(self.num_window_groups) {
            return None;
        }
        self.transf_index_of_group.get(group).copied()
    }

    /// 组 `g` 的实际变换长度，单位为采样。
    #[must_use]
    pub fn transform_length(&self, group: usize) -> Option<u16> {
        if group >= usize::from(self.num_window_groups) {
            return None;
        }
        self.transform_length_of_group.get(group).copied()
    }

    /// `sect_sfb_offset[g][sfb]`，`sfb` 取 0 至 `max_sfb(g)`（含）。
    #[must_use]
    pub fn sect_sfb_offset(&self, group: usize, sfb: usize) -> Option<u16> {
        let max_sfb = usize::from(self.max_sfb(group)?);
        if sfb > max_sfb {
            return None;
        }
        self.sect_sfb_offset.get(group)?.get(sfb).copied()
    }

    /// 全部窗口组覆盖的谱线总数。
    ///
    /// `max_sfb` 小于 `num_sfb` 时高频带不传输，因此该值不超过 `frame_len_base`
    /// 而非恒等于它。
    #[must_use]
    pub const fn total_lines(&self) -> u32 {
        self.total_lines
    }
}

/// `Pseudocode 5` 的取值选择。
fn select_max_sfb(psy: &AsfPsyInfo, half: usize, side_channel: bool) -> u8 {
    let use_side = psy.context.side_limited || (psy.context.dual_maxsfb && side_channel);
    let source = if use_side {
        &psy.max_sfb_side
    } else {
        &psy.max_sfb
    };
    // 未传输 max_sfb[1] 时沿用索引 0，见表 38：只有 b_different_framing 才有
    // 第二组取值。
    source
        .get(half)
        .copied()
        .flatten()
        .or_else(|| source.first().copied().flatten())
        .unwrap_or(0)
}

fn store(target: &mut [u8; MAX_WINDOWS], index: usize, value: u8) {
    if let Some(slot) = target.get_mut(index) {
        *slot = value;
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "测试内的下标与算术越界即用例失败，无需再包一层错误处理"
)]
mod tests {
    use super::*;
    use crate::asf::tables::{NUM_SFB_48, TRANSFORM_LENGTHS_48};

    /// 定长比特缓冲。本模块构造的最长片段是 32 比特，16 字节绰绰有余。
    struct BitBuf {
        bytes: [u8; 16],
        len: usize,
    }

    impl BitBuf {
        const fn new() -> Self {
            Self {
                bytes: [0; 16],
                len: 0,
            }
        }

        fn push(&mut self, bit: bool) {
            if bit {
                self.bytes[self.len / 8] |= 1 << (7 - self.len % 8);
            }
            self.len += 1;
        }

        fn push_bits(&mut self, value: u32, width: u32) {
            for shift in (0..width).rev() {
                self.push((value >> shift) & 1 == 1);
            }
        }
    }

    /// 以给定的成帧参数造一段 `asf_transform_info()` + `asf_psy_info()`。
    ///
    /// `max_sfb` 一律取该变换长度的 `num_sfb`（上界），`grouping` 决定全部
    /// 分组比特取值。
    fn build(frame_len_base: u16, framing: AsfFraming, grouping: bool) -> BitBuf {
        let mut buf = BitBuf::new();
        let info = AsfTransformInfo {
            frame_len_base,
            sampling_frequency_hz: 48_000,
            framing,
        };
        match framing {
            AsfFraming::Long => buf.push(true),
            AsfFraming::Split { first, second } => {
                buf.push(false);
                buf.push_bits(u32::from(first), 2);
                buf.push_bits(u32::from(second), 2);
            }
            AsfFraming::Single { index } => buf.push_bits(u32::from(index), 2),
        }

        let halves = if info.different_framing() { 2 } else { 1 };
        for half in 0..halves {
            let slot = info.slots()[half];
            let length = info.transform_length(slot).expect("测试参数应合法");
            let width = u32::from(n_msfb_bits_48(length).expect("应有 n_msfb_bits"));
            let max_sfb = num_sfb_48(length).expect("应有 num_sfb");
            buf.push_bits(u32::from(max_sfb), width);
        }

        for _ in 0..info.n_grp_bits().expect("测试参数应合法") {
            buf.push(grouping);
        }
        buf
    }

    fn parse_all(
        frame_len_base: u16,
        framing: AsfFraming,
        grouping: bool,
    ) -> (AsfTransformInfo, AsfWindowLayout) {
        let buf = build(frame_len_base, framing, grouping);
        let mut reader = BitReader::new(&buf.bytes);
        let transform =
            AsfTransformInfo::parse(&mut reader, frame_len_base, 48_000).expect("成帧应可解析");
        assert_eq!(transform.framing, framing, "解出的成帧与构造不符");
        let psy = AsfPsyInfo::parse(&mut reader, &transform, AsfPsyContext::default())
            .expect("psy 应可解析");
        assert_eq!(
            reader.bit_position(),
            buf.len as u64,
            "解析消耗的比特数与构造长度不符"
        );
        let layout = AsfWindowLayout::derive(&transform, &psy, false).expect("分组推导应成功");
        (transform, layout)
    }

    /// 长帧只读一个比特，且推出单窗口单组。
    #[test]
    fn long_frame_yields_single_window() {
        let (transform, layout) = parse_all(2048, AsfFraming::Long, false);
        assert_eq!(transform.n_grp_bits().unwrap(), 0);
        assert_eq!(layout.num_windows(), 1);
        assert_eq!(layout.num_window_groups(), 1);
        assert_eq!(layout.transf_length_index(0), Some(4));
        assert_eq!(layout.transform_length(0), Some(2048));
        assert_eq!(layout.total_lines(), 2048);
    }

    /// 全部 16 种半帧组合都恰好铺满一帧。
    ///
    /// `max_sfb` 取上界时 `sfb_offset[max_sfb]` 即变换长度，于是谱线总数等于
    /// 各窗口长度之和。窗口必须无缝铺满一帧，因此该和恒等于 `frame_len_base`。
    /// 这是对 `Pseudocode 3` 与 `Pseudocode 4` 最直接的约束：分组数、组内窗口
    /// 数、半帧归属任一处算错，总数都对不上。
    #[test]
    fn every_split_combination_tiles_the_frame() {
        for first in 0..4u8 {
            for second in 0..4u8 {
                for grouping in [false, true] {
                    let framing = AsfFraming::Split { first, second };
                    let (_, layout) = parse_all(2048, framing, grouping);
                    assert_eq!(
                        layout.total_lines(),
                        2048,
                        "({first}, {second}) grouping={grouping} 未铺满一帧"
                    );
                }
            }
        }
    }

    /// 窗口数只能取 `4.3.6.2.6` 的 NOTE 列出的值，且这些值都可达。
    #[test]
    fn window_counts_match_the_normative_list() {
        const ALLOWED: [u8; 11] = [1, 2, 3, 4, 5, 6, 8, 9, 10, 12, 16];
        let mut seen = [false; 17];
        for first in 0..4u8 {
            for second in 0..4u8 {
                let (_, layout) = parse_all(2048, AsfFraming::Split { first, second }, false);
                let windows = layout.num_windows();
                assert!(
                    ALLOWED.contains(&windows),
                    "({first}, {second}) 推出 {windows} 个窗口，不在规范列表内"
                );
                seen[usize::from(windows)] = true;
            }
        }
        let (_, long) = parse_all(2048, AsfFraming::Long, false);
        seen[usize::from(long.num_windows())] = true;
        for value in ALLOWED {
            assert!(seen[usize::from(value)], "{value} 未被任何组合覆盖");
        }
    }

    /// 分组比特全 0 时每个窗口自成一组，全 1 时按半帧数合并。
    ///
    /// 不同成帧下 `Pseudocode 3` 会强制在半帧交界处断开，因此全 1 也得到两组，
    /// 而非一组。
    #[test]
    fn grouping_bits_control_group_count() {
        for first in 0..4u8 {
            for second in 0..4u8 {
                let framing = AsfFraming::Split { first, second };
                let (transform, all_zero) = parse_all(2048, framing, false);
                assert_eq!(
                    all_zero.num_window_groups(),
                    all_zero.num_windows(),
                    "({first}, {second}) 全 0 分组应每窗一组"
                );

                let (_, all_one) = parse_all(2048, framing, true);
                let expected = if transform.different_framing() { 2 } else { 1 };
                assert_eq!(
                    all_one.num_window_groups(),
                    expected,
                    "({first}, {second}) 全 1 分组应得 {expected} 组"
                );
            }
        }
    }

    /// 不同成帧下两个半帧各自使用自己的变换长度。
    #[test]
    fn split_halves_keep_their_own_transform_length() {
        // 前半 8 个 128 点窗口，后半 1 个 1024 点窗口。
        let (_, layout) = parse_all(
            2048,
            AsfFraming::Split {
                first: 0,
                second: 3,
            },
            false,
        );
        assert_eq!(layout.num_windows(), 9);
        assert_eq!(layout.num_window_groups(), 9);
        for group in 0..8 {
            assert_eq!(layout.transform_length(group), Some(128), "组 {group}");
            assert_eq!(layout.transf_length_index(group), Some(0));
        }
        assert_eq!(layout.transform_length(8), Some(1024));
        assert_eq!(layout.transf_length_index(8), Some(3));
    }

    /// `sect_sfb_offset` 在组内按窗口数倍增。
    #[test]
    fn sect_sfb_offsets_scale_with_window_count() {
        // 全 1 分组且成帧相同：8 个 256 点窗口合成一组。
        let (_, layout) = parse_all(
            2048,
            AsfFraming::Split {
                first: 1,
                second: 1,
            },
            true,
        );
        assert_eq!(layout.num_window_groups(), 1);
        assert_eq!(layout.num_win_in_group(0), Some(8));
        let max_sfb = layout.max_sfb(0).expect("应有 max_sfb");
        let offsets = sfb_offsets_48(256).expect("256 应在表内");
        for (sfb, &base) in offsets.iter().take(usize::from(max_sfb) + 1).enumerate() {
            assert_eq!(layout.sect_sfb_offset(0, sfb), Some(base * 8), "sfb {sfb}");
        }
        assert_eq!(layout.total_lines(), 2048);
    }

    /// 每一组的偏移都从上一组的末尾接续，中间不留空隙。
    #[test]
    fn group_offsets_start_where_the_previous_group_ends() {
        let (_, layout) = parse_all(
            2048,
            AsfFraming::Split {
                first: 2,
                second: 2,
            },
            false,
        );
        let mut expected = 0u16;
        for group in 0..usize::from(layout.num_window_groups()) {
            assert_eq!(
                layout.sect_sfb_offset(group, 0),
                Some(expected),
                "组 {group} 的起点不接续"
            );
            let max_sfb = usize::from(layout.max_sfb(group).expect("应有 max_sfb"));
            expected = layout
                .sect_sfb_offset(group, max_sfb)
                .expect("应有末项偏移");
        }
        assert_eq!(u32::from(expected), layout.total_lines());
    }

    /// `max_sfb` 超过该变换长度的 `num_sfb` 时必须报错。
    ///
    /// 变换长度 1 024 的 `n_msfb_bits` 为 6，可表示到 63，而 `num_sfb` 只有
    /// 49——字段宽度本身容得下非法值，因此这条检查不是多余的。
    #[test]
    fn rejects_max_sfb_above_num_sfb() {
        let mut buf = BitBuf::new();
        buf.push_bits(0b11, 2); // frame_len_base 1024，索引 3 即整帧
        buf.push_bits(63, 6); // max_sfb = 63 > num_sfb = 49
        let mut reader = BitReader::new(&buf.bytes);
        let transform = AsfTransformInfo::parse(&mut reader, 1024, 48_000).unwrap();
        assert!(matches!(
            AsfPsyInfo::parse(&mut reader, &transform, AsfPsyContext::default()),
            Err(AsfError::MaxSfbOutOfRange {
                max_sfb: 63,
                num_sfb: 49,
                transform_length: 1024,
            })
        ));
    }

    /// 恰好等于 `num_sfb` 是合法的，边界不得连同越界一起拒绝。
    #[test]
    fn accepts_max_sfb_equal_to_num_sfb() {
        let mut buf = BitBuf::new();
        buf.push_bits(0b11, 2);
        buf.push_bits(49, 6);
        let mut reader = BitReader::new(&buf.bytes);
        let transform = AsfTransformInfo::parse(&mut reader, 1024, 48_000).unwrap();
        let psy = AsfPsyInfo::parse(&mut reader, &transform, AsfPsyContext::default()).unwrap();
        assert_eq!(psy.max_sfb[0], Some(49));
    }

    /// 表 103 中标为 `×` 的索引组合在解析成帧时就应失败。
    #[test]
    fn rejects_transform_index_absent_from_the_table() {
        // frame_len_base = 512 时索引 3 在表 103 中为 ×。
        let mut buf = BitBuf::new();
        buf.push_bits(0b11, 2);
        let mut reader = BitReader::new(&buf.bytes);
        assert!(matches!(
            AsfTransformInfo::parse(&mut reader, 512, 48_000),
            Err(AsfError::UnsupportedTransformIndex {
                frame_len_base: 512,
                index: 3,
            })
        ));
    }

    /// 仅作为部分块出现的长度不是合法帧基准，且配置错误必须先于读错误返回。
    #[test]
    fn rejects_unsupported_frame_base_before_reading() {
        let mut reader = BitReader::new(&[]);
        assert!(matches!(
            AsfTransformInfo::parse(&mut reader, 480, 48_000),
            Err(AsfError::UnsupportedFrameLenBase {
                frame_len_base: 480,
            })
        ));
        assert_eq!(reader.bit_position(), 0, "配置错误不应消耗输入");
    }

    /// 高采样率表尚未导入时必须明确拒绝，不得静默套用 48 kHz 表。
    #[test]
    fn rejects_high_sampling_frequency_before_reading() {
        for sampling_frequency_hz in [96_000, 192_000] {
            let mut reader = BitReader::new(&[]);
            assert!(matches!(
                AsfTransformInfo::parse(&mut reader, 2048, sampling_frequency_hz),
                Err(AsfError::UnsupportedSamplingFrequency {
                    sampling_frequency_hz: actual,
                }) if actual == sampling_frequency_hz
            ));
            assert_eq!(reader.bit_position(), 0, "配置错误不应消耗输入");
        }
    }

    /// 44,1 kHz 与 48 kHz 共用规范表，两者都应被入口接受。
    #[test]
    fn accepts_both_supported_sampling_frequencies() {
        for sampling_frequency_hz in [44_100, 48_000] {
            let buf = build(2048, AsfFraming::Long, false);
            let mut reader = BitReader::new(&buf.bytes);
            let transform =
                AsfTransformInfo::parse(&mut reader, 2048, sampling_frequency_hz).unwrap();
            assert_eq!(transform.sampling_frequency_hz, sampling_frequency_hz);
            assert_eq!(transform.transform_length(4), Ok(2048));
        }
    }

    /// `frame_len_base < 1536` 不传输 `b_long_frame`，只读两比特索引。
    #[test]
    fn short_base_reads_only_the_index() {
        let buf = build(1024, AsfFraming::Single { index: 3 }, false);
        let mut reader = BitReader::new(&buf.bytes);
        let transform = AsfTransformInfo::parse(&mut reader, 1024, 48_000).unwrap();
        assert_eq!(reader.bit_position(), 2, "只应消耗两比特");
        assert_eq!(
            transform.n_grp_bits().unwrap(),
            0,
            "表 110：索引 3 无分组比特"
        );

        let psy = AsfPsyInfo::parse(&mut reader, &transform, AsfPsyContext::default()).unwrap();
        let layout = AsfWindowLayout::derive(&transform, &psy, false).unwrap();
        assert_eq!(layout.num_windows(), 1, "n_grp_bits 为 0 时与长帧同解");
        assert_eq!(layout.total_lines(), 1024);
    }

    /// 越界查询一律返回 `None`，不得回退到相邻组。
    #[test]
    fn queries_beyond_the_group_count_return_none() {
        let (_, layout) = parse_all(2048, AsfFraming::Long, false);
        assert_eq!(layout.max_sfb(1), None);
        assert_eq!(layout.transform_length(1), None);
        assert_eq!(layout.num_win_in_group(1), None);
        assert_eq!(layout.window_to_group(1), None);
        // 长帧 2048 的 max_sfb 取上界 63，sfb 可到 63，64 越界。
        assert!(layout.sect_sfb_offset(0, 63).is_some());
        assert_eq!(layout.sect_sfb_offset(0, 64), None);
    }

    /// 表内每个变换长度的 `num_sfb` 都不超过 [`MAX_SFB`]。
    #[test]
    fn every_transform_length_fits_the_workspace() {
        for (index, &length) in TRANSFORM_LENGTHS_48.iter().enumerate() {
            let offsets = sfb_offsets_48(length).expect("应有偏移列");
            let num_sfb = usize::from(NUM_SFB_48[index]);
            assert_eq!(offsets.len(), num_sfb + 1);
            assert!(num_sfb <= MAX_SFB, "{length} 的 num_sfb 超出 MAX_SFB");
        }
    }
}
