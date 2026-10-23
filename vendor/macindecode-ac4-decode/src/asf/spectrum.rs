//! ASF 的熵编码谱数据。
//!
//! 覆盖 `TS103190-1:v1.4.1` 的 `sf_data(ASF)`（`4.2.7.3` 表 36）所含四个元素：
//! `asf_section_data()`（表 39）、`asf_spectral_data()`（表 40）、
//! `asf_scalefac_data()`（表 41）与 `asf_snf_data()`（表 42），以及 `5.1.2.2`
//! 的基数分解、符号位与码本 11 的转义扩展。
//!
//! 四者耦合：`asf_spectral_data()` 得到的 `max_quant_idx[g][sfb]` 决定后两个
//! 元素是否为该频带传输码字，因此必须整体解析，不能拆开。
//!
//! 本模块保留原始码值：`huff_decode()` 返回的是码本内的**符号下标**，到实际
//! 标度因子与噪声填充量的映射属于 `6.2.6.4` 与 `5.1.4` 的重建步骤，不在此处
//! 完成。

use super::framing::{AsfError, AsfLayoutKey, AsfWindowLayout, MAX_SFB, MAX_WINDOWS};
use super::tables::{num_sfb_48, spectrum_codebook};
use crate::huffman::{HuffmanError, tables};
use core::fmt;
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

/// 一帧内的最大谱线数。
///
/// 表 99 在 44,1 kHz 与 48 kHz 下的最大 `frame_len_base` 为 2 048，而谱线总数
/// 不超过帧长（见 [`AsfWindowLayout::total_lines`]）。
pub const MAX_SPECTRAL_LINES: usize = 2048;

/// `ext_decode()` 一元前缀的上界。
///
/// `Pseudocode 20` 给出总比特数 `1 + N_ext + (N_ext + 4)`，值域
/// `[2^(N_ext+4), 2^(N_ext+5) - 1]`。表 40 标注该元素为 5 至 21 比特，
/// `5.1.3.1` NOTE 1 规定 `quant_spec` 上限为 8 191——两条独立表述都恰好把
/// `N_ext` 卡在 8。
pub const MAX_EXT_PREFIX: u32 = 8;

/// `5.1.3.1` NOTE 1 规定的量化谱线幅度上限。
pub const MAX_QUANT_MAGNITUDE: i32 = 8191;

/// 谱数据解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsfSpectrumError {
    /// 读取比特时越过了数据末尾。
    Read(ReadError),
    /// Huffman 解码失败。
    Huffman(HuffmanError),
    /// 成帧或分组信息本身有问题。
    Framing(AsfError),
    /// `sect_cb` 取了 12 至 15。
    ///
    /// `4.3.6.3.1` 规定这些值不指示任何码本且不得使用。四比特字段落在该区间
    /// 的概率是四分之一，因此这同时是解析错位最灵敏的早期信号。
    ReservedSectionCodebook {
        /// 读出的 `sect_cb`。
        sect_cb: u8,
        /// 所在窗口组。
        group: usize,
    },
    /// 区段越过了 `max_sfb`。
    ///
    /// 表 39 的循环条件是 `k < max_sfb`，各区段应恰好铺满 `[0, max_sfb)`。
    /// 规范未显式声明这一点，本实现按此判定：越界说明区段长度解析错位。
    SectionOverrunsMaxSfb {
        /// 所在窗口组。
        group: usize,
        /// 区段结束的频带下标。
        end: u32,
        /// 该组的 `max_sfb`。
        max_sfb: u8,
    },
    /// 区段数超出工作区容量。
    TooManySections {
        /// 所在窗口组。
        group: usize,
    },
    /// 谱线下标超出 [`MAX_SPECTRAL_LINES`]。
    LineIndexOutOfRange {
        /// 越界的谱线下标。
        line: u32,
    },
    /// `ext_decode()` 的一元前缀超过 [`MAX_EXT_PREFIX`]。
    ExtensionPrefixTooLong {
        /// 读到的前缀长度。
        prefix: u32,
    },
    /// 重建出的谱线幅度超过 `5.1.3.1` 规定的上限。
    QuantMagnitudeOutOfRange {
        /// 越界的幅度。
        magnitude: i32,
    },
}

impl fmt::Display for AsfSpectrumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsfSpectrumError::Read(error) => write!(f, "{error}"),
            AsfSpectrumError::Huffman(error) => write!(f, "{error}"),
            AsfSpectrumError::Framing(error) => write!(f, "{error}"),
            AsfSpectrumError::ReservedSectionCodebook { sect_cb, group } => write!(
                f,
                "sect_cb {sect_cb} in window group {group} is forbidden; values 12 through 15 must not be used"
            ),
            AsfSpectrumError::SectionOverrunsMaxSfb {
                group,
                end,
                max_sfb,
            } => write!(
                f,
                "Section in window group {group} ends at {end}, exceeding max_sfb {max_sfb}"
            ),
            AsfSpectrumError::TooManySections { group } => {
                write!(
                    f,
                    "Section count in window group {group} exceeds workspace capacity {MAX_SFB}"
                )
            }
            AsfSpectrumError::LineIndexOutOfRange { line } => {
                write!(
                    f,
                    "Spectral-line index {line} exceeds limit {MAX_SPECTRAL_LINES}"
                )
            }
            AsfSpectrumError::ExtensionPrefixTooLong { prefix } => write!(
                f,
                "ext_decode unary prefix {prefix} exceeds limit {MAX_EXT_PREFIX}"
            ),
            AsfSpectrumError::QuantMagnitudeOutOfRange { magnitude } => {
                write!(
                    f,
                    "Quantized spectral-line magnitude {magnitude} exceeds limit {MAX_QUANT_MAGNITUDE}"
                )
            }
        }
    }
}

impl core::error::Error for AsfSpectrumError {}

impl From<ReadError> for AsfSpectrumError {
    fn from(error: ReadError) -> Self {
        AsfSpectrumError::Read(error)
    }
}

impl From<HuffmanError> for AsfSpectrumError {
    fn from(error: HuffmanError) -> Self {
        AsfSpectrumError::Huffman(error)
    }
}

impl From<AsfError> for AsfSpectrumError {
    fn from(error: AsfError) -> Self {
        AsfSpectrumError::Framing(error)
    }
}

/// 表 41 与表 42 共用的逐组频带上界 `min(get_max_sfb(g), num_sfb_48(...))`。
///
/// 标度因子还原（`5.1.3.2` `Pseudocode 21`）必须走同一个上界，否则重建会比
/// 解析多看或少看几个频带，而 DPCM 是累加的，一处错位会污染其后全部频带。
#[must_use]
pub fn coded_band_count(layout: &AsfWindowLayout, group: usize) -> usize {
    let max_sfb = usize::from(layout.max_sfb(group).unwrap_or(0));
    let limit = layout
        .transform_length(group)
        .and_then(num_sfb_48)
        .map_or(0, usize::from);
    max_sfb.min(limit).min(MAX_SFB)
}

/// `asf_section_data()` 解出的一个区段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Section {
    /// `sect_cb[g][i]`，0 表示该区段无谱线数据。
    pub codebook: u8,
    /// `sect_start[g][i]`，起始频带下标。
    pub start: u8,
    /// `sect_end[g][i]`，结束频带下标，不含。
    pub end: u8,
}

/// ASF 谱解码工作区。
///
/// 全部字段定长，解码过程不分配。结构约 14 KiB，按通道复用而非逐帧构造。
#[derive(Debug, Clone)]
pub struct AsfWorkspace {
    quant_spec: [i16; MAX_SPECTRAL_LINES],
    lines: u32,
    layout_key: AsfLayoutKey,
    sections: [[Section; MAX_SFB]; MAX_WINDOWS],
    num_sec: [u8; MAX_WINDOWS],
    sfb_cb: [[u8; MAX_SFB]; MAX_WINDOWS],
    max_quant_idx: [[u16; MAX_SFB]; MAX_WINDOWS],
    dpcm_sf: [[Option<u8>; MAX_SFB]; MAX_WINDOWS],
    dpcm_snf: [[Option<u8>; MAX_SFB]; MAX_WINDOWS],
    reference_scale_factor: u8,
    snf_present: bool,
}

impl Default for AsfWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl AsfWorkspace {
    /// 构造一个空工作区。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            quant_spec: [0; MAX_SPECTRAL_LINES],
            lines: 0,
            layout_key: AsfLayoutKey::empty(),
            sections: [[Section {
                codebook: 0,
                start: 0,
                end: 0,
            }; MAX_SFB]; MAX_WINDOWS],
            num_sec: [0; MAX_WINDOWS],
            sfb_cb: [[0; MAX_SFB]; MAX_WINDOWS],
            max_quant_idx: [[0; MAX_SFB]; MAX_WINDOWS],
            dpcm_sf: [[None; MAX_SFB]; MAX_WINDOWS],
            dpcm_snf: [[None; MAX_SFB]; MAX_WINDOWS],
            reference_scale_factor: 0,
            snf_present: false,
        }
    }

    fn reset(&mut self) {
        // 工作区按通道跨帧复用。必须清空全部固定容量，而非只清理新布局中的
        // 活动组；否则窗口组数减少时，公开查询会暴露上一帧的尾部状态。
        *self = Self::new();
    }

    /// 解析 `sf_data(ASF)` 的四个元素。
    ///
    /// # Errors
    ///
    /// 区段编号落在保留区间、区段越过 `max_sfb`、转义前缀过长或幅度越界时
    /// 报错；数据不足时返回 [`AsfSpectrumError::Read`]。
    pub fn decode(
        &mut self,
        reader: &mut BitReader<'_>,
        layout: &AsfWindowLayout,
    ) -> Result<(), AsfSpectrumError> {
        self.reset();
        self.lines = layout.total_lines();
        self.layout_key = layout.key();
        if usize::try_from(self.lines).unwrap_or(usize::MAX) > MAX_SPECTRAL_LINES {
            return Err(AsfSpectrumError::LineIndexOutOfRange { line: self.lines });
        }
        self.decode_sections(reader, layout)?;
        self.decode_spectral(reader, layout)?;
        self.decode_scalefactors(reader, layout)?;
        self.decode_noise_fill(reader, layout)?;
        Ok(())
    }

    /// `asf_section_data()`，见表 39。
    fn decode_sections(
        &mut self,
        reader: &mut BitReader<'_>,
        layout: &AsfWindowLayout,
    ) -> Result<(), AsfSpectrumError> {
        for group in 0..usize::from(layout.num_window_groups()) {
            let max_sfb = layout.max_sfb(group).unwrap_or(0);
            // 表 39：变换长度索引不大于 2 时区段长度用 3 比特，否则 5 比特。
            let index = layout.transf_length_index(group).unwrap_or(0);
            let (escape, width) = if index <= 2 {
                (7u32, 3u32)
            } else {
                (31u32, 5u32)
            };

            let mut band: u32 = 0;
            let mut count: usize = 0;
            while band < u32::from(max_sfb) {
                let codebook = u8::try_from(reader.read_bits(4)?).unwrap_or(u8::MAX);
                if codebook > 11 {
                    return Err(AsfSpectrumError::ReservedSectionCodebook {
                        sect_cb: codebook,
                        group,
                    });
                }

                let mut length: u32 = 1;
                loop {
                    let increment = u32::try_from(reader.read_bits(width)?).unwrap_or(0);
                    length = length.saturating_add(increment);
                    if increment != escape {
                        break;
                    }
                }

                let end = band.saturating_add(length);
                if end > u32::from(max_sfb) {
                    return Err(AsfSpectrumError::SectionOverrunsMaxSfb {
                        group,
                        end,
                        max_sfb,
                    });
                }
                if count >= MAX_SFB {
                    return Err(AsfSpectrumError::TooManySections { group });
                }

                for sfb in band..end {
                    let slot = usize::try_from(sfb).unwrap_or(0);
                    if let Some(row) = self.sfb_cb.get_mut(group)
                        && let Some(cell) = row.get_mut(slot)
                    {
                        *cell = codebook;
                    }
                }
                if let Some(row) = self.sections.get_mut(group)
                    && let Some(cell) = row.get_mut(count)
                {
                    *cell = Section {
                        codebook,
                        start: u8::try_from(band).unwrap_or(u8::MAX),
                        end: u8::try_from(end).unwrap_or(u8::MAX),
                    };
                }
                band = end;
                count = count.saturating_add(1);
            }

            if let Some(slot) = self.num_sec.get_mut(group) {
                *slot = u8::try_from(count).unwrap_or(u8::MAX);
            }
        }
        Ok(())
    }

    /// `asf_spectral_data()`，见表 40 与 `5.1.2.2`。
    ///
    /// 表 40 只遍历 `num_sec_lsf[g]` 个区段，其余属于高采样率扩展。
    /// `asf_psy_info()` 已把 `max_sfb` 限制在 `num_sfb` 之内（见
    /// `read_max_sfb`），因此区段不可能越过 `num_sfb_48`，
    /// `num_sec_lsf[g]` 恒等于 `num_sec[g]`，表 39 的分割分支在本实现支持的
    /// 配置下不可达。导入高采样率表时需要在此实现分割。
    fn decode_spectral(
        &mut self,
        reader: &mut BitReader<'_>,
        layout: &AsfWindowLayout,
    ) -> Result<(), AsfSpectrumError> {
        for group in 0..usize::from(layout.num_window_groups()) {
            for index in 0..usize::from(self.num_sec.get(group).copied().unwrap_or(0)) {
                let section = self
                    .sections
                    .get(group)
                    .and_then(|row| row.get(index))
                    .copied()
                    .unwrap_or_default();
                let Some(codebook) = spectrum_codebook(section.codebook) else {
                    continue;
                };
                let table = spectrum_table(section.codebook)?;

                let start = layout
                    .sect_sfb_offset(group, usize::from(section.start))
                    .unwrap_or(0);
                let end = layout
                    .sect_sfb_offset(group, usize::from(section.end))
                    .unwrap_or(0);

                let mut line = u32::from(start);
                let mut sfb = usize::from(section.start);
                while line < u32::from(end) {
                    let symbol = table.decode(reader)?;
                    let mut values = [0i32; 4];
                    let dimension = usize::from(codebook.dimension);
                    split_symbol(
                        u32::from(symbol),
                        codebook.modulus,
                        codebook.offset,
                        dimension,
                        &mut values,
                    );

                    // 5.1.2.2：无符号码本的每条非零谱线跟随一个符号位。
                    let mut negative = [false; 4];
                    if codebook.unsigned {
                        for slot in 0..dimension {
                            if values.get(slot).copied().unwrap_or(0) != 0 {
                                let bit = reader.read_flag()?;
                                if let Some(cell) = negative.get_mut(slot) {
                                    *cell = bit;
                                }
                            }
                        }
                    }

                    // 5.1.2.2：码本 11 的幅度 16 是转义标记，实际值另行编码。
                    if section.codebook == 11 {
                        for slot in 0..dimension {
                            if values.get(slot).copied().unwrap_or(0) == 16 {
                                let extended = ext_decode(reader)?;
                                if let Some(cell) = values.get_mut(slot) {
                                    *cell = extended;
                                }
                            }
                        }
                    }

                    for slot in 0..dimension {
                        let magnitude = values.get(slot).copied().unwrap_or(0);
                        if magnitude.abs() > MAX_QUANT_MAGNITUDE {
                            return Err(AsfSpectrumError::QuantMagnitudeOutOfRange { magnitude });
                        }
                        let signed = if negative.get(slot).copied().unwrap_or(false) {
                            0i32.saturating_sub(magnitude)
                        } else {
                            magnitude
                        };

                        let position = line.saturating_add(u32::try_from(slot).unwrap_or(0));
                        let cell = usize::try_from(position).unwrap_or(usize::MAX);
                        if cell >= MAX_SPECTRAL_LINES {
                            return Err(AsfSpectrumError::LineIndexOutOfRange { line: position });
                        }
                        if let Some(target) = self.quant_spec.get_mut(cell) {
                            *target = i16::try_from(signed).unwrap_or(0);
                        }

                        sfb = advance_sfb(layout, group, sfb, usize::from(section.end), position);
                        let absolute = u16::try_from(signed.abs()).unwrap_or(u16::MAX);
                        if let Some(row) = self.max_quant_idx.get_mut(group)
                            && let Some(target) = row.get_mut(sfb)
                        {
                            *target = (*target).max(absolute);
                        }
                    }

                    line = line.saturating_add(u32::try_from(dimension).unwrap_or(1));
                }
            }
        }
        Ok(())
    }

    /// `asf_scalefac_data()`，见表 41。
    ///
    /// 只有承载非零内容的频带才传输标度因子差值，且第一个这样的频带以
    /// `reference_scale_factor` 表示、不额外编码。
    fn decode_scalefactors(
        &mut self,
        reader: &mut BitReader<'_>,
        layout: &AsfWindowLayout,
    ) -> Result<(), AsfSpectrumError> {
        self.reference_scale_factor = u8::try_from(reader.read_bits(8)?).unwrap_or(0);
        let mut first_found = false;
        for group in 0..usize::from(layout.num_window_groups()) {
            for sfb in 0..self.coded_band_count(layout, group) {
                if self.sfb_codebook(group, sfb).unwrap_or(0) == 0 {
                    continue;
                }
                if self.max_quant_idx(group, sfb).unwrap_or(0) == 0 {
                    continue;
                }
                if !first_found {
                    first_found = true;
                    continue;
                }
                let symbol = tables::ASF_HCB_SCALEFAC.decode(reader)?;
                if let Some(row) = self.dpcm_sf.get_mut(group)
                    && let Some(cell) = row.get_mut(sfb)
                {
                    *cell = Some(u8::try_from(symbol).unwrap_or(u8::MAX));
                }
            }
        }
        Ok(())
    }

    /// `asf_snf_data()`，见表 42。
    ///
    /// 条件与标度因子恰好互补：无码本或全零的频带才需要噪声填充。
    fn decode_noise_fill(
        &mut self,
        reader: &mut BitReader<'_>,
        layout: &AsfWindowLayout,
    ) -> Result<(), AsfSpectrumError> {
        self.snf_present = reader.read_flag()?;
        if !self.snf_present {
            return Ok(());
        }
        for group in 0..usize::from(layout.num_window_groups()) {
            for sfb in 0..self.coded_band_count(layout, group) {
                let silent = self.sfb_codebook(group, sfb).unwrap_or(0) == 0
                    || self.max_quant_idx(group, sfb).unwrap_or(0) == 0;
                if !silent {
                    continue;
                }
                let symbol = tables::ASF_HCB_SNF.decode(reader)?;
                if let Some(row) = self.dpcm_snf.get_mut(group)
                    && let Some(cell) = row.get_mut(sfb)
                {
                    *cell = Some(u8::try_from(symbol).unwrap_or(u8::MAX));
                }
            }
        }
        Ok(())
    }

    /// 表 41 与表 42 共用的循环上界，见 [`coded_band_count`]。
    fn coded_band_count(&self, layout: &AsfWindowLayout, group: usize) -> usize {
        coded_band_count(layout, group)
    }

    /// 解出的量化谱线，长度为 [`AsfWindowLayout::total_lines`]。
    #[must_use]
    pub fn quant_spec(&self) -> &[i16] {
        let lines = usize::try_from(self.lines)
            .unwrap_or(0)
            .min(MAX_SPECTRAL_LINES);
        self.quant_spec.get(..lines).unwrap_or(&[])
    }

    /// 解析本工作区时使用的窗口布局键。
    pub(crate) const fn layout_key(&self) -> AsfLayoutKey {
        self.layout_key
    }

    /// 窗口组 `g` 的区段数。
    #[must_use]
    pub fn section_count(&self, group: usize) -> Option<u8> {
        self.num_sec.get(group).copied()
    }

    /// 窗口组 `g` 的第 `index` 个区段。
    #[must_use]
    pub fn section(&self, group: usize, index: usize) -> Option<Section> {
        if index >= usize::from(self.section_count(group)?) {
            return None;
        }
        self.sections.get(group)?.get(index).copied()
    }

    /// `sfb_cb[g][sfb]`：该频带所用的码本编号。
    #[must_use]
    pub fn sfb_codebook(&self, group: usize, sfb: usize) -> Option<u8> {
        self.sfb_cb.get(group)?.get(sfb).copied()
    }

    /// `max_quant_idx[g][sfb]`：该频带内量化谱线绝对值的最大值。
    #[must_use]
    pub fn max_quant_idx(&self, group: usize, sfb: usize) -> Option<u16> {
        self.max_quant_idx.get(group)?.get(sfb).copied()
    }

    /// `reference_scale_factor`，8 比特原始码值。
    #[must_use]
    pub const fn reference_scale_factor(&self) -> u8 {
        self.reference_scale_factor
    }

    /// `dpcm_sf[g][sfb]` 的**码本符号下标**，未映射为差值。
    #[must_use]
    pub fn dpcm_sf(&self, group: usize, sfb: usize) -> Option<u8> {
        self.dpcm_sf.get(group)?.get(sfb).copied().flatten()
    }

    /// `dpcm_snf[g][sfb]` 的**码本符号下标**，未映射为差值。
    #[must_use]
    pub fn dpcm_snf(&self, group: usize, sfb: usize) -> Option<u8> {
        self.dpcm_snf.get(group)?.get(sfb).copied().flatten()
    }

    /// `b_snf_data_exists`。
    #[must_use]
    pub const fn noise_fill_present(&self) -> bool {
        self.snf_present
    }
}

/// 取 `sect_cb` 对应的 Huffman 码本。
fn spectrum_table(sect_cb: u8) -> Result<&'static crate::huffman::HuffmanTable, AsfSpectrumError> {
    Ok(match sect_cb {
        1 => &tables::ASF_HCB_1,
        2 => &tables::ASF_HCB_2,
        3 => &tables::ASF_HCB_3,
        4 => &tables::ASF_HCB_4,
        5 => &tables::ASF_HCB_5,
        6 => &tables::ASF_HCB_6,
        7 => &tables::ASF_HCB_7,
        8 => &tables::ASF_HCB_8,
        9 => &tables::ASF_HCB_9,
        10 => &tables::ASF_HCB_10,
        11 => &tables::ASF_HCB_11,
        other => {
            return Err(AsfSpectrumError::ReservedSectionCodebook {
                sect_cb: other,
                group: 0,
            });
        }
    })
}

/// `Pseudocode 19` 的基数分解：把码本下标还原成若干条谱线。
///
/// 高位在前，即 `quant_spec_1` 对应最高位。
fn split_symbol(symbol: u32, modulus: u16, offset: i16, dimension: usize, out: &mut [i32; 4]) {
    let base = u32::from(modulus).max(1);
    let bias = i32::from(offset);
    let mut remainder = symbol;
    for slot in 0..dimension {
        let power = dimension.saturating_sub(slot).saturating_sub(1);
        let divisor = base
            .checked_pow(u32::try_from(power).unwrap_or(0))
            .unwrap_or(1);
        let digit = remainder.checked_div(divisor).unwrap_or(0);
        remainder = remainder.saturating_sub(digit.saturating_mul(divisor));
        if let Some(cell) = out.get_mut(slot) {
            *cell = i32::try_from(digit).unwrap_or(0).saturating_sub(bias);
        }
    }
}

/// `Pseudocode 20` 的 `ext_decode()`。
fn ext_decode(reader: &mut BitReader<'_>) -> Result<i32, AsfSpectrumError> {
    let mut prefix: u32 = 0;
    while reader.read_flag()? {
        prefix = prefix.saturating_add(1);
        if prefix > MAX_EXT_PREFIX {
            return Err(AsfSpectrumError::ExtensionPrefixTooLong { prefix });
        }
    }
    let width = prefix.saturating_add(4);
    let value = u32::try_from(reader.read_bits(width)?).unwrap_or(0);
    let base = 1u32.checked_shl(width).unwrap_or(0);
    Ok(i32::try_from(base.saturating_add(value)).unwrap_or(MAX_QUANT_MAGNITUDE))
}

/// 把谱线下标推进到它所属的频带。
///
/// 区段内按下标递增遍历，因此只需单向前进。
fn advance_sfb(
    layout: &AsfWindowLayout,
    group: usize,
    mut sfb: usize,
    end: usize,
    line: u32,
) -> usize {
    while sfb.saturating_add(1) < end {
        let boundary = layout
            .sect_sfb_offset(group, sfb.saturating_add(1))
            .unwrap_or(u16::MAX);
        if line < u32::from(boundary) {
            break;
        }
        sfb = sfb.saturating_add(1);
    }
    sfb
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "测试内的下标与算术越界即用例失败，无需再包一层错误处理"
)]
mod tests {
    use super::*;
    use crate::asf::framing::{AsfPsyContext, AsfPsyInfo, AsfTransformInfo};

    /// 定长比特缓冲。本模块构造的最长片段不足 64 字节。
    struct BitBuf {
        bytes: [u8; 64],
        len: usize,
    }

    impl BitBuf {
        const fn new() -> Self {
            Self {
                bytes: [0; 64],
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

        /// 写入某码本中给定符号的码字。
        fn push_codeword(&mut self, name: &str, symbol: usize) {
            let (_, _, lengths, codewords) = tables::ALL_CODEBOOKS
                .iter()
                .find(|(entry, ..)| *entry == name)
                .expect("码本应存在");
            self.push_bits(codewords[symbol], u32::from(lengths[symbol]));
        }
    }

    /// 造一个长帧、`max_sfb = 2` 的布局：1 窗口 1 组，8 条谱线。
    fn long_frame_layout(max_sfb: u32) -> AsfWindowLayout {
        let mut buf = BitBuf::new();
        buf.push(true); // b_long_frame
        buf.push_bits(max_sfb, 6); // n_msfb_bits(2048) = 6
        let mut reader = BitReader::new(&buf.bytes);
        let transform = AsfTransformInfo::parse(&mut reader, 2048, 48_000).unwrap();
        let psy = AsfPsyInfo::parse(&mut reader, &transform, AsfPsyContext::default()).unwrap();
        AsfWindowLayout::derive(&transform, &psy, false).unwrap()
    }

    /// 完整走一遍 `sf_data(ASF)`，并核对落点与每一条谱线。
    ///
    /// 构造的载荷可手工验算：长帧、`max_sfb = 2`，单区段用码本 1（四维、有
    /// 符号、基数 3、偏置 1），覆盖 sfb 0 与 1 共 8 条谱线。符号 0 分解为
    /// 四个 0 位减偏置即 `-1`；符号 40 是 `1111`(3) 即四个 0。
    #[test]
    fn decodes_a_hand_built_frame() {
        let layout = long_frame_layout(2);
        assert_eq!(layout.total_lines(), 8);
        assert_eq!(layout.sect_sfb_offset(0, 0), Some(0));
        assert_eq!(layout.sect_sfb_offset(0, 1), Some(4));
        assert_eq!(layout.sect_sfb_offset(0, 2), Some(8));

        let mut buf = BitBuf::new();
        // asf_section_data：长帧索引为 4，故 n_sect_bits = 5。
        buf.push_bits(1, 4); // sect_cb = 1
        buf.push_bits(1, 5); // sect_len = 1 + 1 = 2，恰好覆盖 max_sfb
        // asf_spectral_data：两个四维码字。
        buf.push_codeword("ASF_HCB_1", 0);
        buf.push_codeword("ASF_HCB_1", 40);
        // asf_scalefac_data：参考值，其后无差值码字。
        buf.push_bits(0xA5, 8);
        // asf_snf_data：存在，sfb 1 全零需要噪声填充。
        buf.push(true);
        buf.push_codeword("ASF_HCB_SNF", 11);

        let mut reader = BitReader::new(&buf.bytes);
        let mut workspace = AsfWorkspace::new();
        workspace.decode(&mut reader, &layout).unwrap();

        assert_eq!(
            reader.bit_position(),
            buf.len as u64,
            "解析消耗的比特数与构造长度不符"
        );
        assert_eq!(workspace.quant_spec(), &[-1, -1, -1, -1, 0, 0, 0, 0]);
        assert_eq!(workspace.section_count(0), Some(1));
        assert_eq!(
            workspace.section(0, 0),
            Some(Section {
                codebook: 1,
                start: 0,
                end: 2
            })
        );
        assert_eq!(workspace.sfb_codebook(0, 0), Some(1));
        assert_eq!(workspace.sfb_codebook(0, 1), Some(1));
        assert_eq!(workspace.max_quant_idx(0, 0), Some(1));
        assert_eq!(workspace.max_quant_idx(0, 1), Some(0), "第二频带全零");
        assert_eq!(workspace.reference_scale_factor(), 0xA5);
        assert_eq!(workspace.dpcm_sf(0, 0), None, "首个非零频带不编码差值");
        assert!(workspace.noise_fill_present());
        assert_eq!(workspace.dpcm_snf(0, 1), Some(11));
        assert_eq!(workspace.dpcm_snf(0, 0), None, "非静默频带不传噪声填充");
    }

    /// 工作区复用到更少的窗口组时，不得暴露上一帧的尾部状态。
    #[test]
    fn reuse_clears_groups_no_longer_active() {
        let layout = long_frame_layout(1);
        let mut buf = BitBuf::new();
        buf.push_bits(0, 4); // 一个无码本区段
        buf.push_bits(0, 5); // sect_len = 1
        buf.push_bits(0x42, 8);
        buf.push(false);

        let mut workspace = AsfWorkspace::new();
        workspace.num_sec[1] = 1;
        workspace.sections[1][0] = Section {
            codebook: 11,
            start: 0,
            end: 1,
        };
        workspace.sfb_cb[1][0] = 11;
        workspace.max_quant_idx[1][0] = 8191;
        workspace.dpcm_sf[1][0] = Some(60);
        workspace.dpcm_snf[1][0] = Some(11);

        let mut reader = BitReader::new(&buf.bytes);
        workspace.decode(&mut reader, &layout).unwrap();

        assert_eq!(reader.bit_position(), buf.len as u64);
        assert_eq!(workspace.section_count(1), Some(0));
        assert_eq!(workspace.section(1, 0), None);
        assert_eq!(workspace.sfb_codebook(1, 0), Some(0));
        assert_eq!(workspace.max_quant_idx(1, 0), Some(0));
        assert_eq!(workspace.dpcm_sf(1, 0), None);
        assert_eq!(workspace.dpcm_snf(1, 0), None);
    }

    /// 标度因子与噪声填充的传输条件恰好互补。
    ///
    /// 表 41 的条件是「有码本且非全零」，表 42 是「无码本或全零」，二者并集
    /// 是全部频带、交集为空。因此每个频带至多有一个差值，且只有首个非零频带
    /// 两者皆无。
    #[test]
    fn scalefactor_and_noise_fill_conditions_are_complementary() {
        let layout = long_frame_layout(4);
        let mut buf = BitBuf::new();
        // 两个区段：前两带用码本 1，后两带无码本。
        buf.push_bits(1, 4);
        buf.push_bits(1, 5);
        buf.push_bits(0, 4);
        buf.push_bits(1, 5);
        // 前两带共 8 条谱线，两个四维码字；第二个取全零符号。
        buf.push_codeword("ASF_HCB_1", 0);
        buf.push_codeword("ASF_HCB_1", 0);
        buf.push_bits(0x33, 8);
        buf.push_codeword("ASF_HCB_SCALEFAC", 60);
        buf.push(true);
        buf.push_codeword("ASF_HCB_SNF", 3);
        buf.push_codeword("ASF_HCB_SNF", 4);

        let mut reader = BitReader::new(&buf.bytes);
        let mut workspace = AsfWorkspace::new();
        workspace.decode(&mut reader, &layout).unwrap();
        assert_eq!(reader.bit_position(), buf.len as u64);

        let mut without_either = 0;
        for sfb in 0..4 {
            let has_sf = workspace.dpcm_sf(0, sfb).is_some();
            let has_snf = workspace.dpcm_snf(0, sfb).is_some();
            assert!(!(has_sf && has_snf), "频带 {sfb} 同时有两种差值");
            if !has_sf && !has_snf {
                without_either += 1;
            }
        }
        assert_eq!(without_either, 1, "只有首个非零频带两者皆无");
    }

    /// 基数分解可逆：每个码本的每个符号分解后重组都应还原。
    ///
    /// `Pseudocode 19` 把码本下标按 `cb_mod` 进制拆成 `CB_DIM` 位再减偏置。
    /// 逐符号验证覆盖全部 1 241 个组合（81×6 + 64×2 + 169×2 + 289），任一位序
    /// 或除数写错都会立刻暴露。
    #[test]
    fn base_decomposition_is_invertible() {
        let mut checked = 0usize;
        for sect_cb in 1..=11u8 {
            let codebook = spectrum_codebook(sect_cb).expect("1 至 11 应有码本");
            let dimension = usize::from(codebook.dimension);
            for symbol in 0..u32::from(codebook.length) {
                let mut values = [0i32; 4];
                split_symbol(
                    symbol,
                    codebook.modulus,
                    codebook.offset,
                    dimension,
                    &mut values,
                );
                let mut rebuilt = 0u32;
                for (slot, &value) in values.iter().take(dimension).enumerate() {
                    let digit = value + i32::from(codebook.offset);
                    assert!(
                        (0..i32::from(codebook.modulus)).contains(&digit),
                        "码本 {sect_cb} 符号 {symbol} 的第 {slot} 位越界"
                    );
                    rebuilt = rebuilt * u32::from(codebook.modulus) + digit as u32;
                }
                assert_eq!(rebuilt, symbol, "码本 {sect_cb} 符号 {symbol} 重组不符");
                checked += 1;
            }
        }
        assert_eq!(checked, 1_241, "遍历到的符号总数与码本规模不符");
    }

    /// `ext_decode()` 的边界与 `Pseudocode 20` 一致。
    ///
    /// 值域 `[2^(N_ext+4), 2^(N_ext+5) - 1]`：`N_ext = 0` 给出 16 至 31，
    /// `N_ext = 8` 给出 4 096 至 8 191，恰好落在 `5.1.3.1` NOTE 1 的上限上。
    #[test]
    fn extension_decode_covers_the_declared_range() {
        for (prefix, expected_low, expected_high) in [(0u32, 16i32, 31i32), (8, 4096, 8191)] {
            for (payload, expected) in [(0u32, expected_low), (u32::MAX, expected_high)] {
                let mut buf = BitBuf::new();
                for _ in 0..prefix {
                    buf.push(true);
                }
                buf.push(false);
                let width = prefix + 4;
                buf.push_bits(payload & ((1u32 << width) - 1), width);
                let mut reader = BitReader::new(&buf.bytes);
                assert_eq!(ext_decode(&mut reader).unwrap(), expected);
                assert_eq!(
                    reader.bit_position(),
                    u64::from(1 + prefix + width),
                    "N_ext = {prefix} 的比特数与 Pseudocode 20 不符"
                );
            }
        }
    }

    /// 一元前缀超过 8 时报错，而非解出越界幅度。
    #[test]
    fn extension_decode_rejects_overlong_prefix() {
        let mut buf = BitBuf::new();
        for _ in 0..9 {
            buf.push(true);
        }
        buf.push(false);
        let mut reader = BitReader::new(&buf.bytes);
        assert!(matches!(
            ext_decode(&mut reader),
            Err(AsfSpectrumError::ExtensionPrefixTooLong { prefix: 9 })
        ));
    }

    /// `sect_cb` 落在保留区间时立刻报错。
    #[test]
    fn rejects_reserved_section_codebook() {
        let layout = long_frame_layout(2);
        for sect_cb in 12..=15u32 {
            let mut buf = BitBuf::new();
            buf.push_bits(sect_cb, 4);
            let mut reader = BitReader::new(&buf.bytes);
            let mut workspace = AsfWorkspace::new();
            assert!(
                matches!(
                    workspace.decode(&mut reader, &layout),
                    Err(AsfSpectrumError::ReservedSectionCodebook { sect_cb: actual, .. })
                        if u32::from(actual) == sect_cb
                ),
                "sect_cb = {sect_cb} 未被拒绝"
            );
        }
    }

    /// 区段越过 `max_sfb` 时报错，不得写出界外的 `sfb_cb`。
    #[test]
    fn rejects_section_overrunning_max_sfb() {
        let layout = long_frame_layout(2);
        let mut buf = BitBuf::new();
        buf.push_bits(1, 4);
        buf.push_bits(4, 5); // sect_len = 5 > max_sfb = 2
        let mut reader = BitReader::new(&buf.bytes);
        let mut workspace = AsfWorkspace::new();
        assert!(matches!(
            workspace.decode(&mut reader, &layout),
            Err(AsfSpectrumError::SectionOverrunsMaxSfb {
                end: 5,
                max_sfb: 2,
                ..
            })
        ));
    }

    /// 区段长度的转义编码可累加。
    ///
    /// 长帧的 `n_sect_bits` 为 5、转义值为 31：长度 33 需要一次转义再补 1。
    #[test]
    fn section_length_escape_accumulates() {
        let layout = long_frame_layout(33);
        let mut buf = BitBuf::new();
        buf.push_bits(0, 4); // 码本 0：无谱线数据
        buf.push_bits(31, 5); // 转义
        buf.push_bits(1, 5); // 补足到 1 + 31 + 1 = 33
        buf.push_bits(0x11, 8); // reference_scale_factor
        buf.push(false); // b_snf_data_exists = 0
        let mut reader = BitReader::new(&buf.bytes);
        let mut workspace = AsfWorkspace::new();
        workspace.decode(&mut reader, &layout).unwrap();
        assert_eq!(reader.bit_position(), buf.len as u64);
        assert_eq!(
            workspace.section(0, 0),
            Some(Section {
                codebook: 0,
                start: 0,
                end: 33
            })
        );
        assert_eq!(workspace.section_count(0), Some(1));
    }

    /// 无符号码本的每条非零谱线跟随一个符号位，零线不跟随。
    ///
    /// 码本 3 为四维无符号、基数 3、偏置 0：符号 0 全零不读符号位，符号 80
    /// 全为 2 要读四位。
    #[test]
    fn unsigned_codebooks_read_one_sign_bit_per_nonzero_line() {
        let layout = long_frame_layout(2);
        let mut buf = BitBuf::new();
        buf.push_bits(3, 4); // sect_cb = 3
        buf.push_bits(1, 5); // 覆盖两个频带
        buf.push_codeword("ASF_HCB_3", 0); // 四个 0，无符号位
        buf.push_codeword("ASF_HCB_3", 80); // 四个 2
        buf.push_bits(0b1010, 4); // 四个符号位
        buf.push_bits(0x00, 8);
        buf.push(false);

        let mut reader = BitReader::new(&buf.bytes);
        let mut workspace = AsfWorkspace::new();
        workspace.decode(&mut reader, &layout).unwrap();
        assert_eq!(reader.bit_position(), buf.len as u64);
        assert_eq!(workspace.quant_spec(), &[0, 0, 0, 0, -2, 2, -2, 2]);
    }
}
