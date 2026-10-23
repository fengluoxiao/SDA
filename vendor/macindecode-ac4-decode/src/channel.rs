//! 声道数据元素、立体声处理与压扩控制。
//!
//! 覆盖 `TS103190-1:v1.4.1` 的：
//!
//! - `4.2.6.2` `mono_data()`、`4.2.6.7` `two_channel_data()`、`4.2.6.8`
//!   `three_channel_data()`、`4.2.6.11` `three_channel_info()`；
//! - `4.2.7.1` `sf_info()` 与 `4.2.7.2` `sf_info_lfe()`；
//! - `4.2.10` `chparam_info()` 与 `sap_data()`；
//! - `4.2.11` `companding_control()`。
//!
//! 这些元素由 P2 `6.2.4.4` 的 `var_channel_element()` 组合。本模块解析语法并
//! 提供 `5.3.2`、`5.3.3.2`–`5.3.3.3` 的 MDCT 域声道矩阵；谱线反量化、解组与
//! IMDCT 仍由 `asf` 模块和上层解码会话负责。
//!
//! `sf_data()` 需要附录 A 的 Huffman 码本，因此整个模块置于 `audio-decode`
//! feature 下。

use crate::asf::framing::{
    AsfError, AsfPsyContext, AsfPsyInfo, AsfTransformInfo, AsfWindowLayout, MAX_SFB, MAX_WINDOWS,
};
use crate::asf::spectrum::{AsfSpectrumError, AsfWorkspace};
use crate::asf::tables::n_msfbl_bits_48;
use crate::huffman::{HuffmanError, tables};
use core::fmt;
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

/// 一个声道元素内的最大声道数。
///
/// `var_channel_element()` 只用到 `mono_data()`、`two_channel_data()` 与
/// `three_channel_data()`，因此上界为 3。`5_X_channel_element()` 与
/// `7_X_channel_element()` 另有四声道与五声道变体，本模块暂不覆盖。
pub const MAX_ELEMENT_CHANNELS: usize = 3;

/// 一个声道元素内的最大 `chparam_info()` 个数。
///
/// `three_channel_info()` 含两个，`two_channel_data()` 至多一个。
pub const MAX_STEREO_PARAMS: usize = 2;

/// 声道元素解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelError {
    /// 读取比特时越过了数据末尾。
    Read(ReadError),
    /// Huffman 解码失败。
    Huffman(HuffmanError),
    /// ASF 成帧或分组信息有问题。
    Framing(AsfError),
    /// ASF 谱数据有问题。
    Spectrum(AsfSpectrumError),
    /// 码流选择了语音频谱前端，本实现尚未覆盖。
    ///
    /// `4.3.5.2` 表 94 规定 `spec_frontend` 取 1 表示 SSF。它需要 `4.2.9` 的
    /// 算术编码与附录 C 的表格，两者均未实现，因此显式拒绝而非猜测性解析。
    SpeechFrontendUnsupported,
    /// `sf_info_lfe()` 在该 `frame_len_base` 下没有 `n_msfbl_bits`。
    LfeBitWidthUnavailable {
        /// 帧长基准。
        frame_len_base: u16,
    },
    /// 调用方提供的工作区数量不足。
    WorkspaceTooSmall {
        /// 该元素需要的声道数。
        needed: usize,
        /// 实际提供的数量。
        provided: usize,
    },
    /// `companding_control()` 的逐声道状态超过定长存储容量。
    CompandingChannelsTooMany {
        /// `nc` 要求的声道状态数。
        channels: u8,
        /// 定长数组可保存的状态数。
        capacity: usize,
    },
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelError::Read(error) => write!(f, "{error}"),
            ChannelError::Huffman(error) => write!(f, "{error}"),
            ChannelError::Framing(error) => write!(f, "{error}"),
            ChannelError::Spectrum(error) => write!(f, "{error}"),
            ChannelError::SpeechFrontendUnsupported => {
                write!(
                    f,
                    "spec_frontend selected SSF; the speech spectrum front end is unsupported"
                )
            }
            ChannelError::LfeBitWidthUnavailable { frame_len_base } => write!(
                f,
                "Table 106 has no n_msfbl_bits for frame_len_base {frame_len_base}"
            ),
            ChannelError::WorkspaceTooSmall { needed, provided } => {
                write!(
                    f,
                    "Element requires {needed} workspaces, but only {provided} were provided"
                )
            }
            ChannelError::CompandingChannelsTooMany { channels, capacity } => write!(
                f,
                "Companding control contains {channels} per-channel states, exceeding fixed capacity {capacity}"
            ),
        }
    }
}

impl core::error::Error for ChannelError {}

/// MDCT 域立体声或三声道矩阵处理失败。
///
/// 处理只接受由同一个 [`ChannelElement`] 解析并完成反量化的分组谱线。错误发生
/// 时调用方应丢弃该元素全部声道的跨帧合成状态，不能让其中一半继续推进。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMatrixError {
    /// 元素还没有解析出受支持的声道数。
    UnsupportedChannelCount {
        /// 实际声道数。
        channels: u8,
    },
    /// 元素缺少该声道的窗口布局。
    MissingLayout {
        /// 声道下标。
        channel: usize,
    },
    /// 两声道元素缺少 `b_enable_mdct_stereo_proc`。
    MissingMdctStereoFlag,
    /// 元素缺少规范要求的 `chparam_info()`。
    MissingStereoParameters {
        /// 所需参数个数。
        needed: usize,
        /// 实际参数个数。
        provided: usize,
    },
    /// `sap_mode == 3` 却没有 `sap_data()`。
    MissingSapData {
        /// 第几份 `chparam_info()`。
        parameter: usize,
    },
    /// 启用 SAP 的偶数频带缺少 alpha 差值。
    MissingSapAlphaDelta {
        /// 第几份 `chparam_info()`。
        parameter: usize,
        /// 窗口组。
        group: u8,
        /// 标度因子带。
        sfb: u8,
    },
    /// 调用方给出的谱线缓冲不足。
    SpectrumTooSmall {
        /// 声道下标。
        channel: usize,
        /// 布局要求的谱线数。
        needed: usize,
        /// 实际谱线数。
        provided: usize,
    },
    /// 窗口布局无法给出一个有效频带范围。
    InvalidBandRange {
        /// 窗口组。
        group: u8,
        /// 标度因子带。
        sfb: u8,
    },
    /// 三声道矩阵选择码 `12…15` 为保留值。
    ReservedMatrixSelector {
        /// `chel_matsel` 原始值；缺席时记为 `u8::MAX`。
        selector: u8,
    },
}

impl fmt::Display for ChannelMatrixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedChannelCount { channels } => {
                write!(
                    f,
                    "Channel element contains {channels} channels; matrix processing supports only 1..3"
                )
            }
            Self::MissingLayout { channel } => write!(f, "Channel {channel} lacks a window layout"),
            Self::MissingMdctStereoFlag => {
                write!(f, "Two-channel element lacks b_enable_mdct_stereo_proc")
            }
            Self::MissingStereoParameters { needed, provided } => write!(
                f,
                "Channel matrix requires {needed} chparam_info entries, but only {provided} were provided"
            ),
            Self::MissingSapData { parameter } => {
                write!(
                    f,
                    "chparam_info entry {parameter} selects SAP but lacks sap_data"
                )
            }
            Self::MissingSapAlphaDelta {
                parameter,
                group,
                sfb,
            } => write!(
                f,
                "chparam_info entry {parameter}, window group {group}, band {sfb} lacks a SAP alpha delta"
            ),
            Self::SpectrumTooSmall {
                channel,
                needed,
                provided,
            } => write!(
                f,
                "Spectral-line buffer for channel {channel} is too small: need {needed}, have {provided}"
            ),
            Self::InvalidBandRange { group, sfb } => {
                write!(
                    f,
                    "Invalid spectral-line range for window group {group}, band {sfb}"
                )
            }
            Self::ReservedMatrixSelector { selector } => {
                write!(
                    f,
                    "chel_matsel {selector} is a reserved three-channel matrix selector"
                )
            }
        }
    }
}

impl core::error::Error for ChannelMatrixError {}

impl From<ReadError> for ChannelError {
    fn from(error: ReadError) -> Self {
        ChannelError::Read(error)
    }
}

impl From<HuffmanError> for ChannelError {
    fn from(error: HuffmanError) -> Self {
        ChannelError::Huffman(error)
    }
}

impl From<AsfError> for ChannelError {
    fn from(error: AsfError) -> Self {
        ChannelError::Framing(error)
    }
}

impl From<AsfSpectrumError> for ChannelError {
    fn from(error: AsfSpectrumError) -> Self {
        ChannelError::Spectrum(error)
    }
}

/// 声道元素的解码上下文，由序列配置给出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelContext {
    /// 帧长基准，见表 99。
    pub frame_len_base: u16,
    /// 采样频率，单位为 Hz。目前只支持 44 100 与 48 000。
    pub sampling_frequency_hz: u32,
}

/// `companding_control()` 的解析结果，见 `4.2.11` 表 49。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompandingControl {
    /// `sync_flag`，`num_chan` 大于 1 时才传输。
    pub sync: bool,
    /// `b_compand_on[ch]`，前 [`Self::channels`] 项有效。
    pub compand_on: [bool; 8],
    /// 实际读入的 `b_compand_on` 个数，即 `nc`。
    pub channels: u8,
    /// `b_compand_avg`，任一声道关闭压扩时才传输。
    pub compand_avg: Option<bool>,
}

impl CompandingControl {
    /// 解析 `companding_control(num_chan)`。
    ///
    /// `sync_flag` 为真时全部声道共用一份控制位，因此只读一项。
    ///
    /// # Errors
    ///
    /// 数据不足时返回 [`ChannelError::Read`]；实际状态数 `nc` 超过
    /// [`Self::compand_on`] 的定长容量时返回
    /// [`ChannelError::CompandingChannelsTooMany`]。
    pub fn parse(reader: &mut BitReader<'_>, num_chan: u8) -> Result<Self, ChannelError> {
        let mut out = Self::default();
        if num_chan > 1 {
            out.sync = reader.read_flag()?;
        }
        let count = if out.sync { 1 } else { num_chan };
        let capacity = out.compand_on.len();
        if usize::from(count) > capacity {
            return Err(ChannelError::CompandingChannelsTooMany {
                channels: count,
                capacity,
            });
        }
        let mut need_average = false;
        for slot in out.compand_on.iter_mut().take(usize::from(count)) {
            let on = reader.read_flag()?;
            *slot = on;
            if !on {
                need_average = true;
            }
        }
        out.channels = count;
        if need_average {
            out.compand_avg = Some(reader.read_flag()?);
        }
        Ok(out)
    }
}

/// `sap_data()` 的解析结果，见 `4.2.10.2` 表 48。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SapData {
    /// `sap_coeff_all`。
    pub coeff_all: bool,
    /// `delta_code_time`，`num_window_groups` 为 1 时不传输。
    pub delta_code_time: Option<bool>,
    coeff_used: [[bool; MAX_SFB]; MAX_WINDOWS],
    dpcm_alpha_q: [[Option<u8>; MAX_SFB]; MAX_WINDOWS],
}

impl SapData {
    const fn empty() -> Self {
        Self {
            coeff_all: false,
            delta_code_time: None,
            coeff_used: [[false; MAX_SFB]; MAX_WINDOWS],
            dpcm_alpha_q: [[None; MAX_SFB]; MAX_WINDOWS],
        }
    }

    /// `sap_coeff_used[g][sfb]`。
    #[must_use]
    pub fn coeff_used(&self, group: usize, sfb: usize) -> Option<bool> {
        self.coeff_used.get(group)?.get(sfb).copied()
    }

    /// `dpcm_alpha_q[g][sfb]` 的**码本符号下标**，未映射为差值。
    ///
    /// 与标度因子共用 `ASF_HCB_SCALEFAC`，见 `4.3.8.2.4`。
    #[must_use]
    pub fn dpcm_alpha_q(&self, group: usize, sfb: usize) -> Option<u8> {
        self.dpcm_alpha_q.get(group)?.get(sfb).copied().flatten()
    }
}

/// `chparam_info()` 的解析结果，见 `4.2.10.1` 表 47。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChparamInfo {
    /// `sap_mode`，见表 114：0 无 SAP，1 按频带 M/S，2 全频带 M/S，3 完整 SAP。
    pub sap_mode: u8,
    ms_used: [[bool; MAX_SFB]; MAX_WINDOWS],
    sap: Option<SapData>,
}

impl ChparamInfo {
    /// 一个未填充的实例，供调用方预留槽位。
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            sap_mode: 0,
            ms_used: [[false; MAX_SFB]; MAX_WINDOWS],
            sap: None,
        }
    }

    /// 解析 `chparam_info()`。
    ///
    /// `ms_used` 与 `sap_coeff_used` 的项数由 `layout` 的分组与 `get_max_sfb(g)`
    /// 决定，因此必须在对应的 `sf_info()` 之后调用。
    ///
    /// # Errors
    ///
    /// 数据不足时返回 [`ChannelError::Read`]；Huffman 解码失败时返回
    /// [`ChannelError::Huffman`]。
    pub fn parse(
        reader: &mut BitReader<'_>,
        layout: &AsfWindowLayout,
    ) -> Result<Self, ChannelError> {
        let mut out = Self::empty();
        out.sap_mode = u8::try_from(reader.read_bits(2)?).unwrap_or(0);

        if out.sap_mode == 1 {
            for group in 0..usize::from(layout.num_window_groups()) {
                for sfb in 0..usize::from(layout.max_sfb(group).unwrap_or(0)) {
                    let bit = reader.read_flag()?;
                    if let Some(row) = out.ms_used.get_mut(group)
                        && let Some(slot) = row.get_mut(sfb)
                    {
                        *slot = bit;
                    }
                }
            }
        }
        if out.sap_mode == 3 {
            out.sap = Some(parse_sap_data(reader, layout)?);
        }
        Ok(out)
    }

    /// `ms_used[g][sfb]`；只有 `sap_mode == 1` 时由码流给出。
    #[must_use]
    pub fn ms_used(&self, group: usize, sfb: usize) -> Option<bool> {
        self.ms_used.get(group)?.get(sfb).copied()
    }

    /// `sap_data()`；只有 `sap_mode == 3` 时存在。
    #[must_use]
    pub const fn sap(&self) -> Option<&SapData> {
        self.sap.as_ref()
    }
}

/// `sap_data()`，见表 48。
///
/// 系数标志按两个频带一组传输：偶数频带读一位，其后的奇数频带沿用同一值。
/// alpha 差值同样只在偶数频带出现。
fn parse_sap_data(
    reader: &mut BitReader<'_>,
    layout: &AsfWindowLayout,
) -> Result<SapData, ChannelError> {
    let mut out = SapData::empty();
    out.coeff_all = reader.read_flag()?;

    for group in 0..usize::from(layout.num_window_groups()) {
        let max_sfb = usize::from(layout.max_sfb(group).unwrap_or(0));
        let mut sfb = 0usize;
        while sfb < max_sfb {
            let used = if out.coeff_all {
                true
            } else {
                reader.read_flag()?
            };
            if let Some(row) = out.coeff_used.get_mut(group) {
                if let Some(slot) = row.get_mut(sfb) {
                    *slot = used;
                }
                // 奇数频带沿用相邻偶数频带的标志，不单独传输。
                if let Some(slot) = row.get_mut(sfb.saturating_add(1))
                    && sfb.saturating_add(1) < max_sfb
                {
                    *slot = used;
                }
            }
            sfb = sfb.saturating_add(2);
        }
    }

    if layout.num_window_groups() != 1 {
        out.delta_code_time = Some(reader.read_flag()?);
    }

    for group in 0..usize::from(layout.num_window_groups()) {
        let max_sfb = usize::from(layout.max_sfb(group).unwrap_or(0));
        let mut sfb = 0usize;
        while sfb < max_sfb {
            if out.coeff_used.get(group).and_then(|row| row.get(sfb)) == Some(&true) {
                let symbol = tables::ASF_HCB_SCALEFAC.decode(reader)?;
                if let Some(row) = out.dpcm_alpha_q.get_mut(group)
                    && let Some(slot) = row.get_mut(sfb)
                {
                    *slot = Some(u8::try_from(symbol).unwrap_or(u8::MAX));
                }
            }
            sfb = sfb.saturating_add(2);
        }
    }
    Ok(out)
}

/// 一个声道元素的解析结果与工作区。
///
/// 全部字段定长，解析不分配。结构较大（约 60 KiB，主要来自三个
/// [`AsfWorkspace`]），应由调用方持有并跨元素复用，而不是逐个构造。
#[derive(Debug, Clone)]
pub struct ChannelElement {
    channels: u8,
    stereo_count: u8,
    layouts: [AsfWindowLayout; MAX_ELEMENT_CHANNELS],
    spectra: [AsfWorkspace; MAX_ELEMENT_CHANNELS],
    stereo: [ChparamInfo; MAX_STEREO_PARAMS],
    /// `b_enable_mdct_stereo_proc`，仅 `two_channel_data()` 传输。
    pub mdct_stereo_proc: Option<bool>,
    /// `chel_matsel`，仅 `three_channel_info()` 传输。
    pub chel_matsel: Option<u8>,
}

impl Default for ChannelElement {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelElement {
    /// 构造一个空元素。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            channels: 0,
            stereo_count: 0,
            layouts: [
                AsfWindowLayout::empty(),
                AsfWindowLayout::empty(),
                AsfWindowLayout::empty(),
            ],
            spectra: [
                AsfWorkspace::new(),
                AsfWorkspace::new(),
                AsfWorkspace::new(),
            ],
            stereo: [ChparamInfo::empty(), ChparamInfo::empty()],
            mdct_stereo_proc: None,
            chel_matsel: None,
        }
    }

    fn reset(&mut self) {
        // 与 AsfWorkspace 同理：跨元素复用时必须清空全部固定容量，否则声道数
        // 减少时公开查询会暴露上一个元素的尾部状态。
        self.channels = 0;
        self.stereo_count = 0;
        self.mdct_stereo_proc = None;
        self.chel_matsel = None;
        for slot in &mut self.layouts {
            *slot = AsfWindowLayout::empty();
        }
        for slot in &mut self.spectra {
            *slot = AsfWorkspace::new();
        }
        for slot in &mut self.stereo {
            *slot = ChparamInfo::empty();
        }
    }

    /// 解析 `mono_data(b_lfe)`，见 `4.2.6.2` 表 21。
    ///
    /// `b_lfe` 为真时频谱前端固定为 ASF 且走 `sf_info_lfe()`；否则由码流的
    /// `spec_frontend` 比特选择，选中 SSF 时报错。
    ///
    /// # Errors
    ///
    /// 见 [`ChannelError`]。
    pub fn parse_mono_data(
        &mut self,
        reader: &mut BitReader<'_>,
        context: ChannelContext,
        b_lfe: bool,
    ) -> Result<(), ChannelError> {
        self.reset();
        let layout = if b_lfe {
            let bits = n_msfbl_bits_48(context.frame_len_base).ok_or(
                ChannelError::LfeBitWidthUnavailable {
                    frame_len_base: context.frame_len_base,
                },
            )?;
            let max_sfb = u8::try_from(reader.read_bits(u32::from(bits))?).unwrap_or(u8::MAX);
            AsfWindowLayout::for_lfe(
                context.frame_len_base,
                context.sampling_frequency_hz,
                max_sfb,
            )?
        } else {
            // 表 94：0 为 ASF，1 为 SSF。
            if reader.read_flag()? {
                return Err(ChannelError::SpeechFrontendUnsupported);
            }
            parse_sf_info(reader, context)?
        };
        self.store_layout(0, layout);
        self.channels = 1;
        self.decode_channel(reader, 0)
    }

    /// 解析 `two_channel_data()`，见 `4.2.6.7` 表 26。
    ///
    /// `b_enable_mdct_stereo_proc` 为真时两个声道共用一份 `sf_info()`
    /// （`4.3.5.10`），否则各自传输一份。
    ///
    /// # Errors
    ///
    /// 见 [`ChannelError`]。
    pub fn parse_two_channel_data(
        &mut self,
        reader: &mut BitReader<'_>,
        context: ChannelContext,
    ) -> Result<(), ChannelError> {
        self.reset();
        let shared = reader.read_flag()?;
        self.mdct_stereo_proc = Some(shared);
        if shared {
            let layout = parse_sf_info(reader, context)?;
            let stereo = ChparamInfo::parse(reader, &layout)?;
            self.store_stereo(0, stereo);
            self.store_layout(0, layout.clone());
            self.store_layout(1, layout);
        } else {
            let first = parse_sf_info(reader, context)?;
            let second = parse_sf_info(reader, context)?;
            self.store_layout(0, first);
            self.store_layout(1, second);
        }
        self.channels = 2;
        self.decode_channel(reader, 0)?;
        self.decode_channel(reader, 1)
    }

    /// 解析 `three_channel_data()`，见 `4.2.6.8` 表 27。
    ///
    /// 三个声道共用一份 `sf_info()`；`three_channel_info()` 给出矩阵选择码与
    /// 两份 `chparam_info()`。
    ///
    /// # Errors
    ///
    /// 见 [`ChannelError`]。
    pub fn parse_three_channel_data(
        &mut self,
        reader: &mut BitReader<'_>,
        context: ChannelContext,
    ) -> Result<(), ChannelError> {
        self.reset();
        let layout = parse_sf_info(reader, context)?;

        // three_channel_info()，见 4.2.6.11 表 30。
        self.chel_matsel = Some(u8::try_from(reader.read_bits(4)?).unwrap_or(0));
        let first = ChparamInfo::parse(reader, &layout)?;
        let second = ChparamInfo::parse(reader, &layout)?;
        self.store_stereo(0, first);
        self.store_stereo(1, second);

        self.store_layout(0, layout.clone());
        self.store_layout(1, layout.clone());
        self.store_layout(2, layout);
        self.channels = 3;
        for channel in 0..3 {
            self.decode_channel(reader, channel)?;
        }
        Ok(())
    }

    fn store_layout(&mut self, index: usize, layout: AsfWindowLayout) {
        if let Some(slot) = self.layouts.get_mut(index) {
            *slot = layout;
        }
    }

    fn store_stereo(&mut self, index: usize, info: ChparamInfo) {
        if let Some(slot) = self.stereo.get_mut(index) {
            *slot = info;
        }
        self.stereo_count = u8::try_from(index.saturating_add(1)).unwrap_or(u8::MAX);
    }

    /// 对第 `index` 个声道执行 `sf_data(ASF)`。
    fn decode_channel(
        &mut self,
        reader: &mut BitReader<'_>,
        index: usize,
    ) -> Result<(), ChannelError> {
        // 布局与工作区分处两个字段，借用不相交。
        let (Some(layout), Some(workspace)) =
            (self.layouts.get(index), self.spectra.get_mut(index))
        else {
            return Err(ChannelError::WorkspaceTooSmall {
                needed: index.saturating_add(1),
                provided: MAX_ELEMENT_CHANNELS,
            });
        };
        workspace.decode(reader, layout)?;
        Ok(())
    }

    /// 本元素的声道数。
    #[must_use]
    pub const fn channels(&self) -> u8 {
        self.channels
    }

    /// 第 `index` 个声道的窗口布局。
    #[must_use]
    pub fn layout(&self, index: usize) -> Option<&AsfWindowLayout> {
        if index >= usize::from(self.channels) {
            return None;
        }
        self.layouts.get(index)
    }

    /// 第 `index` 个声道的谱数据。
    #[must_use]
    pub fn spectrum(&self, index: usize) -> Option<&AsfWorkspace> {
        if index >= usize::from(self.channels) {
            return None;
        }
        self.spectra.get(index)
    }

    /// 本元素携带的 `chparam_info()` 个数。
    #[must_use]
    pub const fn stereo_param_count(&self) -> u8 {
        self.stereo_count
    }

    /// 第 `index` 份 `chparam_info()`。
    #[must_use]
    pub fn stereo_params(&self, index: usize) -> Option<&ChparamInfo> {
        if index >= usize::from(self.stereo_count) {
            return None;
        }
        self.stereo.get(index)
    }

    /// 对已经反量化、仍按窗口组排列的谱线应用 MDCT 域声道矩阵。
    ///
    /// 两声道元素按 `5.3.3.2` 使用 `chparam_info()` 的逐频带系数；三声道元素按
    /// `5.3.3.3` 表 178 组合两份系数。`b_enable_mdct_stereo_proc == false` 与单声道
    /// 元素保持原样。矩阵必须发生在 [`crate::asf::reconstruct::ungroup_spectrum`]
    /// 和 IMDCT 之前。
    ///
    /// `spectra` 的前 [`Self::channels`] 项分别对应 `sf_data()` 的输入次序，每项
    /// 至少包含该声道布局的 [`AsfWindowLayout::total_lines`] 条谱线。
    ///
    /// # Errors
    ///
    /// 元素状态不完整、SAP 差值缺失、三声道选择码为保留值，或任一缓冲不足时
    /// 返回 [`ChannelMatrixError`]。所有可由输入判断的错误都在改写谱线前返回。
    pub fn apply_channel_matrix(
        &self,
        spectra: &mut [&mut [f32]; MAX_ELEMENT_CHANNELS],
    ) -> Result<(), ChannelMatrixError> {
        let channels = usize::from(self.channels);
        if !(1..=MAX_ELEMENT_CHANNELS).contains(&channels) {
            return Err(ChannelMatrixError::UnsupportedChannelCount {
                channels: self.channels,
            });
        }
        for channel in 0..channels {
            let layout = self
                .layout(channel)
                .ok_or(ChannelMatrixError::MissingLayout { channel })?;
            let needed = usize::try_from(layout.total_lines()).unwrap_or(usize::MAX);
            let provided = spectra.get(channel).map(|values| values.len()).unwrap_or(0);
            if provided < needed {
                return Err(ChannelMatrixError::SpectrumTooSmall {
                    channel,
                    needed,
                    provided,
                });
            }
        }

        let [first, second, third] = spectra;
        match channels {
            1 => Ok(()),
            2 => {
                let enabled = self
                    .mdct_stereo_proc
                    .ok_or(ChannelMatrixError::MissingMdctStereoFlag)?;
                if !enabled {
                    return Ok(());
                }
                let layout = self
                    .layout(0)
                    .ok_or(ChannelMatrixError::MissingLayout { channel: 0 })?;
                validate_band_ranges(layout)?;
                let parameters =
                    self.stereo_params(0)
                        .ok_or(ChannelMatrixError::MissingStereoParameters {
                            needed: 1,
                            provided: usize::from(self.stereo_count),
                        })?;
                let alpha = reconstruct_sap_alpha(parameters, 0, layout)?;
                apply_two_channel_matrix(layout, parameters, &alpha, first, second)
            }
            3 => {
                let selector = self.chel_matsel.unwrap_or(u8::MAX);
                if selector > 11 {
                    return Err(ChannelMatrixError::ReservedMatrixSelector { selector });
                }
                let layout = self
                    .layout(0)
                    .ok_or(ChannelMatrixError::MissingLayout { channel: 0 })?;
                validate_band_ranges(layout)?;
                let first_parameters =
                    self.stereo_params(0)
                        .ok_or(ChannelMatrixError::MissingStereoParameters {
                            needed: 2,
                            provided: usize::from(self.stereo_count),
                        })?;
                let second_parameters =
                    self.stereo_params(1)
                        .ok_or(ChannelMatrixError::MissingStereoParameters {
                            needed: 2,
                            provided: usize::from(self.stereo_count),
                        })?;
                let first_alpha = reconstruct_sap_alpha(first_parameters, 0, layout)?;
                let second_alpha = reconstruct_sap_alpha(second_parameters, 1, layout)?;
                apply_three_channel_matrix(
                    layout,
                    selector,
                    (first_parameters, &first_alpha),
                    (second_parameters, &second_alpha),
                    first,
                    second,
                    third,
                )
            }
            _ => unreachable!("channel count was restricted to 1..3 at entry"),
        }
    }
}

/// 一份 `chparam_info()` 在一个时频 tile 上展开出的 2×2 系数。
#[derive(Debug, Clone, Copy, PartialEq)]
struct StereoMatrix {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
}

impl StereoMatrix {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
    };
    const MID_SIDE: Self = Self {
        a: 1.0,
        b: 1.0,
        c: 1.0,
        d: -1.0,
    };
}

type SapAlpha = [[i16; MAX_SFB]; MAX_WINDOWS];

/// `5.3.2` `Pseudocode 59`：把 SAP 的频率/时间差分还原为逐带 alpha。
fn reconstruct_sap_alpha(
    parameters: &ChparamInfo,
    parameter: usize,
    layout: &AsfWindowLayout,
) -> Result<SapAlpha, ChannelMatrixError> {
    let mut alpha = [[0i16; MAX_SFB]; MAX_WINDOWS];
    if parameters.sap_mode != 3 {
        return Ok(alpha);
    }
    let sap = parameters
        .sap()
        .ok_or(ChannelMatrixError::MissingSapData { parameter })?;
    let mut max_sfb_previous = layout.max_sfb(0).unwrap_or(0);

    for group in 0..usize::from(layout.num_window_groups()) {
        let max_sfb = layout.max_sfb(group).unwrap_or(0);
        for sfb in 0..usize::from(max_sfb) {
            if sap.coeff_used(group, sfb) != Some(true) {
                continue;
            }
            let value = if sfb % 2 == 1 {
                alpha
                    .get(group)
                    .and_then(|row| row.get(sfb.saturating_sub(1)))
                    .copied()
                    .unwrap_or(0)
            } else {
                let symbol = sap.dpcm_alpha_q(group, sfb).ok_or(
                    ChannelMatrixError::MissingSapAlphaDelta {
                        parameter,
                        group: u8::try_from(group).unwrap_or(u8::MAX),
                        sfb: u8::try_from(sfb).unwrap_or(u8::MAX),
                    },
                )?;
                let delta = i16::from(symbol).saturating_sub(60);
                let code_in_time = group > 0
                    && max_sfb == max_sfb_previous
                    && sap.delta_code_time.unwrap_or(false);
                if code_in_time {
                    alpha
                        .get(group.saturating_sub(1))
                        .and_then(|row| row.get(sfb))
                        .copied()
                        .unwrap_or(0)
                        .saturating_add(delta)
                } else if sfb == 0 {
                    delta
                } else {
                    alpha
                        .get(group)
                        .and_then(|row| row.get(sfb.saturating_sub(2)))
                        .copied()
                        .unwrap_or(0)
                        .saturating_add(delta)
                }
            };
            if let Some(slot) = alpha.get_mut(group).and_then(|row| row.get_mut(sfb)) {
                *slot = value;
            }
        }
        max_sfb_previous = max_sfb;
    }
    Ok(alpha)
}

/// `5.3.2` `Pseudocode 59`：为一个时频 tile 选择四个矩阵系数。
fn stereo_matrix(
    parameters: &ChparamInfo,
    alpha: &SapAlpha,
    group: usize,
    sfb: usize,
) -> StereoMatrix {
    match parameters.sap_mode {
        0 => StereoMatrix::IDENTITY,
        1 if parameters.ms_used(group, sfb) == Some(true) => StereoMatrix::MID_SIDE,
        1 => StereoMatrix::IDENTITY,
        2 => StereoMatrix::MID_SIDE,
        3 if parameters.sap().and_then(|sap| sap.coeff_used(group, sfb)) == Some(true) => {
            let quantized = alpha
                .get(group)
                .and_then(|row| row.get(sfb))
                .copied()
                .unwrap_or(0);
            let gain = f32::from(quantized) * 0.1;
            StereoMatrix {
                a: 1.0 + gain,
                b: 1.0,
                c: 1.0 - gain,
                d: -1.0,
            }
        }
        3 => StereoMatrix::IDENTITY,
        _ => StereoMatrix::IDENTITY,
    }
}

fn band_range(
    layout: &AsfWindowLayout,
    group: usize,
    sfb: usize,
) -> Result<(usize, usize), ChannelMatrixError> {
    let invalid = || ChannelMatrixError::InvalidBandRange {
        group: u8::try_from(group).unwrap_or(u8::MAX),
        sfb: u8::try_from(sfb).unwrap_or(u8::MAX),
    };
    let start = usize::from(layout.sect_sfb_offset(group, sfb).ok_or_else(invalid)?);
    let end = usize::from(
        layout
            .sect_sfb_offset(group, sfb.saturating_add(1))
            .ok_or_else(invalid)?,
    );
    let total = usize::try_from(layout.total_lines()).unwrap_or(usize::MAX);
    if start > end || end > total {
        return Err(invalid());
    }
    Ok((start, end))
}

/// 在任何谱线被改写之前遍历一次全部范围，使矩阵处理具备事务性。
fn validate_band_ranges(layout: &AsfWindowLayout) -> Result<(), ChannelMatrixError> {
    for group in 0..usize::from(layout.num_window_groups()) {
        for sfb in 0..usize::from(layout.max_sfb(group).unwrap_or(0)) {
            band_range(layout, group, sfb)?;
        }
    }
    Ok(())
}

fn apply_two_channel_matrix(
    layout: &AsfWindowLayout,
    parameters: &ChparamInfo,
    alpha: &SapAlpha,
    first: &mut [f32],
    second: &mut [f32],
) -> Result<(), ChannelMatrixError> {
    for group in 0..usize::from(layout.num_window_groups()) {
        for sfb in 0..usize::from(layout.max_sfb(group).unwrap_or(0)) {
            let coefficients = stereo_matrix(parameters, alpha, group, sfb);
            let (start, end) = band_range(layout, group, sfb)?;
            let invalid = || ChannelMatrixError::InvalidBandRange {
                group: u8::try_from(group).unwrap_or(u8::MAX),
                sfb: u8::try_from(sfb).unwrap_or(u8::MAX),
            };
            let first_band = first.get_mut(start..end).ok_or_else(invalid)?;
            let second_band = second.get_mut(start..end).ok_or_else(invalid)?;
            for (output0, output1) in first_band.iter_mut().zip(second_band.iter_mut()) {
                let (input0, input1) = (*output0, *output1);
                *output0 = coefficients.a * input0 + coefficients.b * input1;
                *output1 = coefficients.c * input0 + coefficients.d * input1;
            }
        }
    }
    Ok(())
}

/// 表 178。两份 2×2 系数按 `chel_matsel` 组合为一个 3×3 矩阵。
fn three_channel_matrix(
    selector: u8,
    first: StereoMatrix,
    second: StereoMatrix,
) -> Option<[[f32; 3]; 3]> {
    let StereoMatrix {
        a: a0,
        b: b0,
        c: c0,
        d: d0,
    } = first;
    let StereoMatrix {
        a: a1,
        b: b1,
        c: c1,
        d: d1,
    } = second;
    match selector {
        0 => Some([
            [a0 * a1, b0 * a1, b1],
            [c0, d0, 0.0],
            [a0 * c1, b0 * c1, d1],
        ]),
        1 => Some([
            [d0, c0, 0.0],
            [b0 * a1, a0 * a1, b1],
            [b0 * c1, a0 * c1, d1],
        ]),
        2 => Some([
            [a0 * a1, b1, b0 * a1],
            [a0 * c1, d1, b0 * c1],
            [c0, 0.0, d0],
        ]),
        3 => Some([
            [a1, c0 * b1, d0 * b1],
            [0.0, a0, b0],
            [c1, c0 * d1, d0 * d1],
        ]),
        4 => Some([
            [a0, 0.0, b0],
            [c0 * b1, a1, d0 * b1],
            [c0 * d1, c1, d0 * d1],
        ]),
        5 => Some([
            [a1, d0 * b1, c0 * b1],
            [c1, d0 * d1, c0 * d1],
            [0.0, b0, a0],
        ]),
        6 => Some([
            [d0 * d1, c0 * d1, c1],
            [b0, a0, 0.0],
            [d0 * b1, c0 * b1, a1],
        ]),
        7 => Some([
            [a0, b0, 0.0],
            [c0 * d1, d0 * d1, c1],
            [c0 * b1, d0 * b1, a1],
        ]),
        8 => Some([
            [d0 * d1, c1, c0 * d1],
            [d0 * b1, a1, c0 * b1],
            [b0, 0.0, a0],
        ]),
        9 => Some([
            [d1, b0 * c1, a0 * c1],
            [0.0, d0, c0],
            [b1, b0 * a1, a0 * a1],
        ]),
        10 => Some([
            [d0, 0.0, c0],
            [b0 * c1, d1, a0 * c1],
            [b0 * a1, b1, a0 * a1],
        ]),
        11 => Some([
            [d1, a0 * c1, b0 * c1],
            [b1, a0 * a1, b0 * a1],
            [0.0, c0, d0],
        ]),
        _ => None,
    }
}

fn apply_three_channel_matrix(
    layout: &AsfWindowLayout,
    selector: u8,
    first_parameters: (&ChparamInfo, &SapAlpha),
    second_parameters: (&ChparamInfo, &SapAlpha),
    first: &mut [f32],
    second: &mut [f32],
    third: &mut [f32],
) -> Result<(), ChannelMatrixError> {
    for group in 0..usize::from(layout.num_window_groups()) {
        for sfb in 0..usize::from(layout.max_sfb(group).unwrap_or(0)) {
            let first_coefficients =
                stereo_matrix(first_parameters.0, first_parameters.1, group, sfb);
            let second_coefficients =
                stereo_matrix(second_parameters.0, second_parameters.1, group, sfb);
            let coefficients =
                three_channel_matrix(selector, first_coefficients, second_coefficients)
                    .ok_or(ChannelMatrixError::ReservedMatrixSelector { selector })?;
            let [[m00, m01, m02], [m10, m11, m12], [m20, m21, m22]] = coefficients;
            let (start, end) = band_range(layout, group, sfb)?;
            let invalid = || ChannelMatrixError::InvalidBandRange {
                group: u8::try_from(group).unwrap_or(u8::MAX),
                sfb: u8::try_from(sfb).unwrap_or(u8::MAX),
            };
            let first_band = first.get_mut(start..end).ok_or_else(invalid)?;
            let second_band = second.get_mut(start..end).ok_or_else(invalid)?;
            let third_band = third.get_mut(start..end).ok_or_else(invalid)?;
            for ((output0, output1), output2) in first_band
                .iter_mut()
                .zip(second_band.iter_mut())
                .zip(third_band.iter_mut())
            {
                let (input0, input1, input2) = (*output0, *output1, *output2);
                *output0 = m00 * input0 + m01 * input1 + m02 * input2;
                *output1 = m10 * input0 + m11 * input1 + m12 * input2;
                *output2 = m20 * input0 + m21 * input1 + m22 * input2;
            }
        }
    }
    Ok(())
}

/// `sf_info(ASF, 0, 0)`，见 `4.2.7.1` 表 34。
///
/// `b_dual_maxsfb` 与 `b_side_limited` 在 `two_channel_data()` 与
/// `three_channel_data()` 中均写死为 0（表 26、表 27）。
fn parse_sf_info(
    reader: &mut BitReader<'_>,
    context: ChannelContext,
) -> Result<AsfWindowLayout, ChannelError> {
    let transform = AsfTransformInfo::parse(
        reader,
        context.frame_len_base,
        context.sampling_frequency_hz,
    )?;
    let psy = AsfPsyInfo::parse(reader, &transform, AsfPsyContext::default())?;
    Ok(AsfWindowLayout::derive(&transform, &psy, false)?)
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "测试内的下标与算术越界即用例失败，无需再包一层错误处理"
)]
mod tests {
    use super::*;

    const CONTEXT: ChannelContext = ChannelContext {
        frame_len_base: 2048,
        sampling_frequency_hz: 48_000,
    };

    /// 定长比特缓冲。
    struct BitBuf {
        bytes: [u8; 128],
        len: usize,
    }

    impl BitBuf {
        const fn new() -> Self {
            Self {
                bytes: [0; 128],
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

        fn push_codeword(&mut self, name: &str, symbol: usize) {
            let (_, _, lengths, codewords) = tables::ALL_CODEBOOKS
                .iter()
                .find(|(entry, ..)| *entry == name)
                .expect("码本应存在");
            self.push_bits(codewords[symbol], u32::from(lengths[symbol]));
        }

        /// 长帧 `sf_info(ASF, 0, 0)`：`b_long_frame` 加 6 比特 `max_sfb`。
        fn push_long_sf_info(&mut self, max_sfb: u32) {
            self.push(true);
            self.push_bits(max_sfb, 6);
        }

        /// 一个最简 `sf_data(ASF)`：单个无码本区段，无标度因子与噪声填充。
        fn push_empty_sf_data(&mut self, max_sfb: u32) {
            self.push_bits(0, 4); // sect_cb = 0
            self.push_bits(max_sfb - 1, 5); // sect_len = 1 + (max_sfb - 1)
            self.push_bits(0, 8); // reference_scale_factor
            self.push(false); // b_snf_data_exists
        }
    }

    /// LFE 的 `mono_data(1)` 不传输成帧信息，只读 `n_msfbl_bits` 位 `max_sfb`。
    #[test]
    fn lfe_mono_data_reads_only_max_sfb() {
        let mut buf = BitBuf::new();
        buf.push_bits(3, 3); // n_msfbl_bits(2048) = 3，max_sfb = 3
        buf.push_empty_sf_data(3);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_mono_data(&mut reader, CONTEXT, true)
            .expect("LFE 应可解析");

        assert_eq!(reader.bit_position(), buf.len as u64);
        assert_eq!(element.channels(), 1);
        let layout = element.layout(0).expect("应有布局");
        assert_eq!(layout.num_windows(), 1);
        assert_eq!(layout.num_window_groups(), 1);
        assert_eq!(layout.transform_length(0), Some(2048));
        assert_eq!(layout.max_sfb(0), Some(3));
        // sfb_offset[3] = 12，见附录 B 表 B.4。
        assert_eq!(layout.total_lines(), 12);
        assert_eq!(element.spectrum(0).expect("应有谱").quant_spec().len(), 12);
    }

    /// `n_msfbl_bits` 只有 3 比特，最大表示 7，远小于 2048 的 `num_sfb` 63。
    ///
    /// 这解释了 LFE 为何不需要更宽的字段：它只编码最低几个频带。
    #[test]
    fn lfe_max_sfb_field_is_narrow_by_design() {
        assert_eq!(n_msfbl_bits_48(2048), Some(3));
        let mut buf = BitBuf::new();
        buf.push_bits(7, 3); // 该字段能表示的最大值
        buf.push_empty_sf_data(7);
        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element.parse_mono_data(&mut reader, CONTEXT, true).unwrap();
        assert_eq!(element.layout(0).unwrap().max_sfb(0), Some(7));
        assert_eq!(reader.bit_position(), buf.len as u64);
    }

    /// 非 LFE 的 `mono_data(0)` 先读一比特 `spec_frontend`。
    #[test]
    fn mono_data_reads_spectral_frontend_selector() {
        let mut buf = BitBuf::new();
        buf.push(false); // 表 94：0 = ASF
        buf.push_long_sf_info(2);
        buf.push_empty_sf_data(2);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_mono_data(&mut reader, CONTEXT, false)
            .unwrap();
        assert_eq!(reader.bit_position(), buf.len as u64);
        assert_eq!(element.layout(0).unwrap().max_sfb(0), Some(2));
    }

    /// `spec_frontend` 选中 SSF 时显式拒绝，而非按 ASF 猜测性解析。
    #[test]
    fn mono_data_rejects_speech_frontend() {
        let mut buf = BitBuf::new();
        buf.push(true); // 表 94：1 = SSF
        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        assert_eq!(
            element.parse_mono_data(&mut reader, CONTEXT, false),
            Err(ChannelError::SpeechFrontendUnsupported)
        );
    }

    /// `b_enable_mdct_stereo_proc` 为真时两声道共用一份 `sf_info()`。
    #[test]
    fn two_channel_data_shares_one_sf_info_when_flagged() {
        let mut buf = BitBuf::new();
        buf.push(true); // b_enable_mdct_stereo_proc
        buf.push_long_sf_info(2);
        buf.push_bits(0, 2); // chparam_info：sap_mode = 0
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_two_channel_data(&mut reader, CONTEXT)
            .unwrap();

        assert_eq!(reader.bit_position(), buf.len as u64);
        assert_eq!(element.channels(), 2);
        assert_eq!(element.mdct_stereo_proc, Some(true));
        assert_eq!(element.stereo_param_count(), 1);
        assert_eq!(element.stereo_params(0).unwrap().sap_mode, 0);
        // 共用布局：两声道的分组与频带完全一致。
        assert_eq!(element.layout(0).unwrap().max_sfb(0), Some(2));
        assert_eq!(element.layout(1).unwrap().max_sfb(0), Some(2));
    }

    /// 标志为假时两声道各自传输 `sf_info()`，可以有不同的 `max_sfb`。
    #[test]
    fn two_channel_data_reads_two_sf_infos_when_unflagged() {
        let mut buf = BitBuf::new();
        buf.push(false);
        buf.push_long_sf_info(2);
        buf.push_long_sf_info(5);
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(5);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_two_channel_data(&mut reader, CONTEXT)
            .unwrap();

        assert_eq!(reader.bit_position(), buf.len as u64);
        assert_eq!(element.mdct_stereo_proc, Some(false));
        assert_eq!(element.stereo_param_count(), 0, "无 chparam_info");
        assert_eq!(element.layout(0).unwrap().max_sfb(0), Some(2));
        assert_eq!(element.layout(1).unwrap().max_sfb(0), Some(5));
        assert_eq!(element.spectrum(0).unwrap().quant_spec().len(), 8);
        assert_eq!(element.spectrum(1).unwrap().quant_spec().len(), 20);
    }

    /// `three_channel_data()`：一份 `sf_info()`、四比特矩阵码、两份
    /// `chparam_info()`，随后三份 `sf_data()`。
    #[test]
    fn three_channel_data_shares_one_layout_across_three_channels() {
        let mut buf = BitBuf::new();
        buf.push_long_sf_info(2);
        buf.push_bits(0b1011, 4); // chel_matsel
        buf.push_bits(0, 2); // 第一份 chparam_info
        buf.push_bits(2, 2); // 第二份：sap_mode = 2，全频带 M/S，无额外比特
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_three_channel_data(&mut reader, CONTEXT)
            .unwrap();

        assert_eq!(reader.bit_position(), buf.len as u64);
        assert_eq!(element.channels(), 3);
        assert_eq!(element.chel_matsel, Some(0b1011));
        assert_eq!(element.stereo_param_count(), 2);
        assert_eq!(element.stereo_params(1).unwrap().sap_mode, 2);
        for channel in 0..3 {
            assert_eq!(element.layout(channel).unwrap().max_sfb(0), Some(2));
        }
    }

    /// `sap_mode == 1` 时逐频带传输 `ms_used`。
    #[test]
    fn chparam_info_reads_one_ms_bit_per_band() {
        let mut buf = BitBuf::new();
        buf.push(true);
        buf.push_long_sf_info(4);
        buf.push_bits(1, 2); // sap_mode = 1
        buf.push_bits(0b1010, 4); // 四个频带的 ms_used
        buf.push_empty_sf_data(4);
        buf.push_empty_sf_data(4);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_two_channel_data(&mut reader, CONTEXT)
            .unwrap();
        assert_eq!(reader.bit_position(), buf.len as u64);

        let stereo = element.stereo_params(0).unwrap();
        assert_eq!(stereo.sap_mode, 1);
        assert_eq!(stereo.ms_used(0, 0), Some(true));
        assert_eq!(stereo.ms_used(0, 1), Some(false));
        assert_eq!(stereo.ms_used(0, 2), Some(true));
        assert_eq!(stereo.ms_used(0, 3), Some(false));
    }

    /// `sap_mode == 3` 时读 `sap_data()`：系数标志两带一组，alpha 差值同样。
    ///
    /// `num_window_groups` 为 1，故不传输 `delta_code_time`。
    #[test]
    fn sap_data_pairs_bands_and_skips_delta_flag_for_one_group() {
        let mut buf = BitBuf::new();
        buf.push(true);
        buf.push_long_sf_info(4);
        buf.push_bits(3, 2); // sap_mode = 3
        buf.push(false); // sap_coeff_all = 0
        buf.push(true); // 频带 0、1
        buf.push(false); // 频带 2、3
        buf.push_codeword("ASF_HCB_SCALEFAC", 60); // 频带 0 的 alpha 差值
        buf.push_empty_sf_data(4);
        buf.push_empty_sf_data(4);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_two_channel_data(&mut reader, CONTEXT)
            .unwrap();
        assert_eq!(reader.bit_position(), buf.len as u64);

        let sap = element.stereo_params(0).unwrap().sap().expect("应有 sap");
        assert!(!sap.coeff_all);
        assert_eq!(sap.delta_code_time, None, "单组不传输时间差分标志");
        assert_eq!(sap.coeff_used(0, 0), Some(true));
        assert_eq!(sap.coeff_used(0, 1), Some(true), "奇数带沿用相邻偶数带");
        assert_eq!(sap.coeff_used(0, 2), Some(false));
        assert_eq!(sap.coeff_used(0, 3), Some(false));
        assert_eq!(sap.dpcm_alpha_q(0, 0), Some(60));
        assert_eq!(sap.dpcm_alpha_q(0, 2), None, "未启用的频带无差值");
    }

    /// `sap_coeff_all` 为真时不传输逐带标志，但 alpha 差值照常按对出现。
    #[test]
    fn sap_data_skips_flags_when_all_bands_used() {
        let mut buf = BitBuf::new();
        buf.push(true);
        buf.push_long_sf_info(4);
        buf.push_bits(3, 2);
        buf.push(true); // sap_coeff_all = 1
        buf.push_codeword("ASF_HCB_SCALEFAC", 60);
        buf.push_codeword("ASF_HCB_SCALEFAC", 59);
        buf.push_empty_sf_data(4);
        buf.push_empty_sf_data(4);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_two_channel_data(&mut reader, CONTEXT)
            .unwrap();
        assert_eq!(reader.bit_position(), buf.len as u64);

        let sap = element.stereo_params(0).unwrap().sap().unwrap();
        assert!(sap.coeff_all);
        for sfb in 0..4 {
            assert_eq!(sap.coeff_used(0, sfb), Some(true), "频带 {sfb}");
        }
        assert_eq!(sap.dpcm_alpha_q(0, 0), Some(60));
        assert_eq!(sap.dpcm_alpha_q(0, 2), Some(59));
    }

    /// `sap_mode = 2` 对每条谱线应用规范的和/差矩阵，且没有隐藏的归一化。
    #[test]
    fn full_mid_side_matrix_produces_sum_and_difference() {
        let mut buf = BitBuf::new();
        buf.push(true);
        buf.push_long_sf_info(2);
        buf.push_bits(2, 2);
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_two_channel_data(&mut reader, CONTEXT)
            .unwrap();

        let mut first = [1.0f32; 8];
        let mut second = [2.0f32; 8];
        let mut unused = [];
        {
            let mut spectra = [&mut first[..], &mut second[..], &mut unused[..]];
            element.apply_channel_matrix(&mut spectra).unwrap();
        }
        assert_eq!(first, [3.0; 8]);
        assert_eq!(second, [-1.0; 8]);
    }

    /// `sap_mode = 1` 只变换 `ms_used` 为真的标度因子带。
    #[test]
    fn selective_mid_side_matrix_respects_band_mask() {
        let mut buf = BitBuf::new();
        buf.push(true);
        buf.push_long_sf_info(4);
        buf.push_bits(1, 2);
        buf.push_bits(0b1010, 4);
        buf.push_empty_sf_data(4);
        buf.push_empty_sf_data(4);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_two_channel_data(&mut reader, CONTEXT)
            .unwrap();

        let mut first = [1.0f32; 16];
        let mut second = [2.0f32; 16];
        let mut unused = [];
        {
            let mut spectra = [&mut first[..], &mut second[..], &mut unused[..]];
            element.apply_channel_matrix(&mut spectra).unwrap();
        }
        assert_eq!(
            first,
            [
                3.0, 3.0, 3.0, 3.0, 1.0, 1.0, 1.0, 1.0, 3.0, 3.0, 3.0, 3.0, 1.0, 1.0, 1.0, 1.0
            ]
        );
        assert_eq!(
            second,
            [
                -1.0, -1.0, -1.0, -1.0, 2.0, 2.0, 2.0, 2.0, -1.0, -1.0, -1.0, -1.0, 2.0, 2.0, 2.0,
                2.0
            ]
        );
    }

    /// 完整 SAP 先按频率差分还原 alpha，再让相邻奇数带沿用偶数带系数。
    #[test]
    fn sap_matrix_reconstructs_alpha_pairs() {
        let mut buf = BitBuf::new();
        buf.push(true);
        buf.push_long_sf_info(4);
        buf.push_bits(3, 2);
        buf.push(true);
        buf.push_codeword("ASF_HCB_SCALEFAC", 60);
        buf.push_codeword("ASF_HCB_SCALEFAC", 59);
        buf.push_empty_sf_data(4);
        buf.push_empty_sf_data(4);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_two_channel_data(&mut reader, CONTEXT)
            .unwrap();

        let mut first = [1.0f32; 16];
        let mut second = [2.0f32; 16];
        let mut unused = [];
        {
            let mut spectra = [&mut first[..], &mut second[..], &mut unused[..]];
            element.apply_channel_matrix(&mut spectra).unwrap();
        }
        for value in first.iter().take(8) {
            assert_eq!(*value, 3.0);
        }
        for value in second.iter().take(8) {
            assert_eq!(*value, -1.0);
        }
        for value in first.iter().skip(8) {
            assert!((*value - 2.9).abs() < 1.0e-6);
        }
        for value in second.iter().skip(8) {
            assert!((*value + 0.9).abs() < 1.0e-6);
        }
    }

    /// 表 178 的十二种矩阵逐项核对，尤其覆盖并非简单置换的选择码 3、4、9、11。
    #[test]
    fn three_channel_matrix_matches_table_178() {
        let first = StereoMatrix {
            a: 2.0,
            b: 3.0,
            c: 5.0,
            d: 7.0,
        };
        let second = StereoMatrix {
            a: 11.0,
            b: 13.0,
            c: 17.0,
            d: 19.0,
        };
        let expected = [
            [[22.0, 33.0, 13.0], [5.0, 7.0, 0.0], [34.0, 51.0, 19.0]],
            [[7.0, 5.0, 0.0], [33.0, 22.0, 13.0], [51.0, 34.0, 19.0]],
            [[22.0, 13.0, 33.0], [34.0, 19.0, 51.0], [5.0, 0.0, 7.0]],
            [[11.0, 65.0, 91.0], [0.0, 2.0, 3.0], [17.0, 95.0, 133.0]],
            [[2.0, 0.0, 3.0], [65.0, 11.0, 91.0], [95.0, 17.0, 133.0]],
            [[11.0, 91.0, 65.0], [17.0, 133.0, 95.0], [0.0, 3.0, 2.0]],
            [[133.0, 95.0, 17.0], [3.0, 2.0, 0.0], [91.0, 65.0, 11.0]],
            [[2.0, 3.0, 0.0], [95.0, 133.0, 17.0], [65.0, 91.0, 11.0]],
            [[133.0, 17.0, 95.0], [91.0, 11.0, 65.0], [3.0, 0.0, 2.0]],
            [[19.0, 51.0, 34.0], [0.0, 7.0, 5.0], [13.0, 33.0, 22.0]],
            [[7.0, 0.0, 5.0], [51.0, 19.0, 34.0], [33.0, 13.0, 22.0]],
            [[19.0, 34.0, 51.0], [13.0, 22.0, 33.0], [0.0, 5.0, 7.0]],
        ];
        for (selector, matrix) in expected.into_iter().enumerate() {
            assert_eq!(
                three_channel_matrix(u8::try_from(selector).unwrap(), first, second),
                Some(matrix),
                "chel_matsel {selector}"
            );
        }
        assert_eq!(three_channel_matrix(12, first, second), None);
    }

    /// 三声道选择码 9 的实际谱线变换应保留中间输入，并把首尾还原为差/和。
    #[test]
    fn three_channel_selector_nine_transforms_spectra() {
        let mut buf = BitBuf::new();
        buf.push_long_sf_info(2);
        buf.push_bits(9, 4);
        buf.push_bits(0, 2);
        buf.push_bits(2, 2);
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_three_channel_data(&mut reader, CONTEXT)
            .unwrap();

        let mut first = [1.0f32; 8];
        let mut second = [2.0f32; 8];
        let mut third = [4.0f32; 8];
        {
            let mut spectra = [&mut first[..], &mut second[..], &mut third[..]];
            element.apply_channel_matrix(&mut spectra).unwrap();
        }
        assert_eq!(first, [3.0; 8]);
        assert_eq!(second, [2.0; 8]);
        assert_eq!(third, [5.0; 8]);
    }

    /// 表 178 之外的选择码必须在任何谱线被改写前拒绝。
    #[test]
    fn reserved_three_channel_selector_is_transactional() {
        let mut buf = BitBuf::new();
        buf.push_long_sf_info(2);
        buf.push_bits(12, 4);
        buf.push_bits(2, 2);
        buf.push_bits(2, 2);
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);

        let mut reader = BitReader::new(&buf.bytes);
        let mut element = ChannelElement::new();
        element
            .parse_three_channel_data(&mut reader, CONTEXT)
            .unwrap();

        let mut first = [1.0f32; 8];
        let mut second = [2.0f32; 8];
        let mut third = [4.0f32; 8];
        let original = (first, second, third);
        let error = {
            let mut spectra = [&mut first[..], &mut second[..], &mut third[..]];
            element.apply_channel_matrix(&mut spectra).unwrap_err()
        };
        assert_eq!(
            error,
            ChannelMatrixError::ReservedMatrixSelector { selector: 12 }
        );
        assert_eq!((first, second, third), original);
    }

    /// `companding_control()`：`sync_flag` 为真时只读一份控制位。
    #[test]
    fn companding_control_collapses_to_one_flag_when_synced() {
        let mut buf = BitBuf::new();
        buf.push(true); // sync_flag
        buf.push(true); // b_compand_on[0]
        let mut reader = BitReader::new(&buf.bytes);
        let control = CompandingControl::parse(&mut reader, 5).unwrap();
        assert_eq!(reader.bit_position(), 2);
        assert!(control.sync);
        assert_eq!(control.channels, 1);
        assert_eq!(control.compand_avg, None, "全部开启则不传平均标志");
    }

    /// 不同步时逐声道读取，且任一关闭都会追加 `b_compand_avg`。
    #[test]
    fn companding_control_reads_per_channel_and_appends_average() {
        let mut buf = BitBuf::new();
        buf.push(false); // sync_flag
        buf.push(true);
        buf.push(false); // 该声道关闭压扩
        buf.push(true);
        buf.push(true);
        buf.push(true);
        buf.push(true); // b_compand_avg
        let mut reader = BitReader::new(&buf.bytes);
        let control = CompandingControl::parse(&mut reader, 5).unwrap();
        assert_eq!(reader.bit_position(), 7);
        assert!(!control.sync);
        assert_eq!(control.channels, 5);
        assert!(!control.compand_on[1]);
        assert_eq!(control.compand_avg, Some(true));
    }

    /// 不同步时 `nc == num_chan`，超过定长数组容量必须在读取逐声道标志前拒绝。
    #[test]
    fn companding_control_rejects_more_channels_than_storage() {
        let mut buf = BitBuf::new();
        buf.push(false); // sync_flag：逐声道传输
        let mut reader = BitReader::new(&buf.bytes);

        assert_eq!(
            CompandingControl::parse(&mut reader, 9),
            Err(ChannelError::CompandingChannelsTooMany {
                channels: 9,
                capacity: 8,
            })
        );
        assert_eq!(reader.bit_position(), 1, "不得继续消费逐声道控制位");
    }

    /// 单声道不传输 `sync_flag`。
    #[test]
    fn companding_control_omits_sync_flag_for_one_channel() {
        let mut buf = BitBuf::new();
        buf.push(true); // b_compand_on[0]
        let mut reader = BitReader::new(&buf.bytes);
        let control = CompandingControl::parse(&mut reader, 1).unwrap();
        assert_eq!(reader.bit_position(), 1);
        assert!(!control.sync);
        assert_eq!(control.channels, 1);
    }

    /// 元素跨次复用时不得暴露上一次的声道。
    #[test]
    fn reuse_clears_channels_from_the_previous_element() {
        let mut element = ChannelElement::new();

        let mut buf = BitBuf::new();
        buf.push_long_sf_info(2);
        buf.push_bits(0b0101, 4);
        buf.push_bits(0, 2);
        buf.push_bits(0, 2);
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);
        buf.push_empty_sf_data(2);
        let mut reader = BitReader::new(&buf.bytes);
        element
            .parse_three_channel_data(&mut reader, CONTEXT)
            .unwrap();
        assert_eq!(element.channels(), 3);
        assert_eq!(element.stereo_param_count(), 2);

        let mut buf = BitBuf::new();
        buf.push_bits(3, 3);
        buf.push_empty_sf_data(3);
        let mut reader = BitReader::new(&buf.bytes);
        element.parse_mono_data(&mut reader, CONTEXT, true).unwrap();

        assert_eq!(element.channels(), 1);
        assert!(element.layout(1).is_none(), "上一次的第二声道不得可见");
        assert!(element.spectrum(2).is_none());
        assert_eq!(element.stereo_param_count(), 0);
        assert!(element.stereo_params(0).is_none());
        assert_eq!(element.chel_matsel, None, "上一次的矩阵码不得残留");
        assert_eq!(element.mdct_stereo_proc, None);
    }

    /// 新元素在首声道谱数据处截断时，尚未访问的声道不得保留上一次的频谱。
    #[test]
    fn failed_reuse_clears_spectra_from_the_previous_element() {
        let mut element = ChannelElement::new();

        let mut complete = BitBuf::new();
        complete.push_long_sf_info(2);
        complete.push_bits(0, 4);
        complete.push_bits(0, 2);
        complete.push_bits(0, 2);
        complete.push_empty_sf_data(2);
        complete.push_empty_sf_data(2);
        complete.push_empty_sf_data(2);
        let mut reader = BitReader::new(&complete.bytes);
        element
            .parse_three_channel_data(&mut reader, CONTEXT)
            .unwrap();
        for channel in 0..3 {
            assert!(!element.spectrum(channel).unwrap().quant_spec().is_empty());
        }

        let mut truncated = BitBuf::new();
        truncated.push_long_sf_info(2);
        truncated.push_bits(0, 4);
        truncated.push_bits(0, 2);
        truncated.push_bits(0, 2);
        assert_eq!(truncated.len, 15);
        let mut reader = BitReader::new(&truncated.bytes[..2]);
        assert!(matches!(
            element.parse_three_channel_data(&mut reader, CONTEXT),
            Err(ChannelError::Spectrum(AsfSpectrumError::Read(_)))
        ));

        for channel in 1..3 {
            assert!(
                element
                    .spectrum(channel)
                    .is_none_or(|spectrum| spectrum.quant_spec().is_empty()),
                "声道 {channel} 不得暴露上一次的谱数据"
            );
        }
    }
}
