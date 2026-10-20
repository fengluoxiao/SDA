//! `audio_data_ajoc()` 的组装。
//!
//! `TS103190-2:v1.3.1` 的 `6.2.3.4` 给出语法。它是 A-JOC 路径下音频数据的
//! 最外层，把 `var_channel_element()`、`ajoc()`、`ajoc_dmx_de_data()` 与两处
//! `oamd_dyndata_single()` 串起来，走完即落到 `audio_size`。
//!
//! 这里也是 M3 逐对象位置的出口：按 P2 表 7，`b_ajoc` 为真时逐对象动态数据
//! 位于本元素内，排在 `var_channel_element()` 与 `ajoc()` 之后，因此取到它
//! 必须先具备频谱前端的比特解析能力。
//!
//! 下混与上混各有一份 `oamd_dyndata_single()`，两者的对象数不同：下混是
//! `n_fullband_dmx_signals + b_lfe`，上混是 `n_fullband_upmix_signals + b_lfe`。

use crate::ajoc::de::{
    AjocBedInfo, AjocDeError, AjocDeState, AjocDmxDeData, parse_bed_info, parse_dmx_de_data,
};
use crate::ajoc::{Ajoc, AjocError, AjocObjectControl, AjocObjectMatrix, parse_ajoc};
use crate::aspx::syntax::AspxData;
use crate::channel::{ChannelContext, ChannelElement};
use crate::var_element::{
    MAX_FULLBAND_DMX_SIGNALS, VarChannelElement, VarChannelParams, VarChannelState,
    VarChannelWorkspace, VarElementError, parse_var_channel_element,
};
use core::fmt;
use macindecode_ac4_bitstream::oamd::{
    OamdAlternativeDataSetDomain, OamdDyndataSingle, OamdError, OamdMetadataBlock, OamdTimingData,
    ObjectDescriptor,
};
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

/// `audio_data_ajoc()` 中独立携带 OAMD 的解码模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDataMode {
    /// core decoding mode，对应下混侧。
    Core,
    /// full decoding mode，对应上混侧。
    Full,
}

impl fmt::Display for AudioDataMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core => write!(f, "core"),
            Self::Full => write!(f, "full"),
        }
    }
}

/// `audio_data_ajoc()` 解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDataError {
    /// 读取比特时越过了数据末尾。
    Read(ReadError),
    /// `var_channel_element()` 解析失败。
    VarElement(VarElementError),
    /// `ajoc()` 解析失败。
    Ajoc(AjocError),
    /// `ajoc_dmx_de_data()` 解析失败。
    AjocDe(AjocDeError),
    /// OAMD 动态数据解析失败。
    Oamd(OamdError),
    /// `b_static_dmx` 为真，需要 `audio_data_chan()`。
    ///
    /// 该分支走声道编码的下混，与 A-JOC 的可变声道元素是两条完全不同的路径；
    /// M2 的探针判定本编码链恒为 `b_static_dmx == 0`，故显式拒绝而非猜测。
    StaticDownmixUnsupported,
    /// 指定解码模式没有可用的 `num_obj_info_blocks`。
    TimingUnavailable {
        /// 缺少时间数据的解码模式。
        mode: AudioDataMode,
    },
    /// I 帧的 `num_obj_info_blocks` 为零。
    ZeroObjectInfoBlocksInIframe {
        /// 出现非法零值的解码模式。
        mode: AudioDataMode,
    },
    /// OAMD 扩展的跳过长度小于 `ajoc_bed_info()` 已消耗的比特数。
    ///
    /// `6.2.3.4` 用 `skip_bits - ajoc_bed_info()` 得到剩余长度，差为负说明
    /// 声明的扩展长度与实际内容矛盾。
    ExtensionUnderflow {
        /// 声明的总比特数。
        declared: u32,
        /// `ajoc_bed_info()` 消耗的比特数。
        consumed: u8,
    },
    /// 调用方提供的对象描述或信息块工作区不足。
    ObjectWorkspaceTooSmall {
        /// 需要的对象数。
        needed: usize,
        /// 实际提供的个数。
        provided: usize,
    },
    /// 对象描述中的 LFE 标志与 `6.2.3.4` 的信号顺序不一致。
    InvalidLfeLayout {
        /// 出错的解码模式。
        mode: AudioDataMode,
        /// 对象下标。
        index: usize,
        /// 该下标是否应为 LFE。
        expected: bool,
        /// 描述符中的实际值。
        actual: bool,
    },
}

impl fmt::Display for AudioDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioDataError::Read(error) => write!(f, "{error}"),
            AudioDataError::VarElement(error) => write!(f, "{error}"),
            AudioDataError::Ajoc(error) => write!(f, "{error}"),
            AudioDataError::AjocDe(error) => write!(f, "{error}"),
            AudioDataError::Oamd(error) => write!(f, "{error}"),
            AudioDataError::StaticDownmixUnsupported => {
                write!(
                    f,
                    "b_static_dmx is true and requires unsupported audio_data_chan"
                )
            }
            AudioDataError::TimingUnavailable { mode } => write!(
                f,
                "{mode} mode carries no timing data and has no prior num_obj_info_blocks to continue"
            ),
            AudioDataError::ZeroObjectInfoBlocksInIframe { mode } => {
                write!(
                    f,
                    "num_obj_info_blocks must not be zero for {mode} mode in an I-frame"
                )
            }
            AudioDataError::ExtensionUnderflow { declared, consumed } => write!(
                f,
                "OAMD extension declares {declared} bits, fewer than the {consumed} consumed by ajoc_bed_info"
            ),
            AudioDataError::ObjectWorkspaceTooSmall { needed, provided } => {
                write!(
                    f,
                    "Element requires {needed} object slots, but only {provided} were provided"
                )
            }
            AudioDataError::InvalidLfeLayout {
                mode,
                index,
                expected,
                actual,
            } => write!(
                f,
                "LFE flag for object {index} in {mode} mode should be {expected}, got {actual}"
            ),
        }
    }
}

impl core::error::Error for AudioDataError {}

impl From<ReadError> for AudioDataError {
    fn from(error: ReadError) -> Self {
        AudioDataError::Read(error)
    }
}

impl From<VarElementError> for AudioDataError {
    fn from(error: VarElementError) -> Self {
        AudioDataError::VarElement(error)
    }
}

impl From<AjocError> for AudioDataError {
    fn from(error: AjocError) -> Self {
        AudioDataError::Ajoc(error)
    }
}

impl From<AjocDeError> for AudioDataError {
    fn from(error: AjocDeError) -> Self {
        AudioDataError::AjocDe(error)
    }
}

impl From<OamdError> for AudioDataError {
    fn from(error: OamdError) -> Self {
        AudioDataError::Oamd(error)
    }
}

/// `audio_data_ajoc()` 的调用参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioDataParams {
    /// 声道元素的解码上下文。
    pub context: ChannelContext,
    /// `n_fullband_dmx_signals`，不含 LFE。
    pub n_fb_dmx_signals: u8,
    /// `n_fullband_upmix_signals`，不含 LFE。
    pub n_fb_upmix_signals: u32,
    /// `b_lfe`。
    pub b_lfe: bool,
    /// `b_iframe`。
    pub b_iframe: bool,
    /// `b_static_dmx`；为真时本实现拒绝。
    pub b_static_dmx: bool,
    /// `b_alternative`；为真时两处 `oamd_dyndata_single()` 都携带候选 dataset。
    pub b_alternative: bool,
    /// 当前帧对 substream group 生效的 `num_obj_info_blocks`。
    ///
    /// 调用方应先合并 group 级 `oamd_timing_data()` 与其跨帧状态。core 侧未携带
    /// 独立 timing，或 full 侧令 `b_derive_timing_from_dmx == 0` 时使用此值。
    pub group_num_obj_info_blocks: Option<u8>,
}

/// `audio_data_ajoc()` 的跨帧状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioDataState {
    /// `var_channel_element()` 的状态。
    pub var_channel: VarChannelState,
    /// `ajoc_dmx_de_data()` 的状态。
    pub de: AjocDeState,
    /// core 模式上一帧生效的块数。
    pub dmx_num_obj_info_blocks: Option<u8>,
    /// full 模式上一帧生效的块数。
    pub umx_num_obj_info_blocks: Option<u8>,
}

impl AudioDataState {
    /// 一个尚未收到任何帧的初始状态。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// 调用方提供的工作区与对象描述。
#[derive(Debug)]
pub struct AudioDataWorkspace<'a> {
    /// 声道数据元素。
    pub elements: &'a mut [ChannelElement],
    /// A-SPX 数据元素。
    pub aspx: &'a mut [AspxData],
    /// A-JOC 逐对象控制信息。
    pub controls: &'a mut [AjocObjectControl],
    /// A-JOC 逐对象混合矩阵。
    pub matrices: &'a mut [AjocObjectMatrix],
    /// 下混对象的分类，长度须为 `n_fb_dmx_signals + b_lfe`；存在 LFE 时索引 0
    /// 必须为 LFE。可用 [`macindecode_ac4_bitstream::oamd::ObjectDescriptors::from_ajoc_assignment`]
    /// 从 `dmx_assignment` 构造。
    pub dmx_objects: &'a [ObjectDescriptor],
    /// 上混对象的分类，长度须为 `n_fb_upmix_signals + b_lfe`，顺序约束同上；
    /// 应从 `upmix_assignment` 单独构造。
    pub umx_objects: &'a [ObjectDescriptor],
    /// 下混动态数据的信息块，按对象与块下标线性填充。
    ///
    /// 每项自带 `object_index` 与 `block_index`，可直接交给
    /// [`macindecode_ac4_bitstream::oamd::OamdState::apply_blocks`]；填充顺序是对象在外、块在内，
    /// 但调用方不必依赖该顺序。
    pub dmx_blocks: &'a mut [OamdMetadataBlock],
    /// 上混动态数据的信息块，约定同上。
    pub umx_blocks: &'a mut [OamdMetadataBlock],
}

/// 一个 `audio_data_ajoc()` 的解析结果摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioDataAjoc {
    /// `var_channel_element()` 的结果。
    pub var_element: VarChannelElement,
    /// `ajoc()` 的结果。
    pub ajoc: Ajoc,
    /// `ajoc_dmx_de_data()` 的结果。
    pub dmx_de: AjocDmxDeData,
    /// `ajoc_bed_info()`，仅 `b_oamd_extension_present` 为真时出现。
    pub bed_info: Option<AjocBedInfo>,
    /// `b_some_signals_inactive`。
    pub some_signals_inactive: bool,
    /// `b_dmx_timing` 携带的时间数据。
    pub dmx_timing: Option<OamdTimingData>,
    /// `b_umx_timing` 携带的时间数据。
    pub umx_timing: Option<OamdTimingData>,
    /// `b_derive_timing_from_dmx`，仅 `b_umx_timing` 为假时传输。
    pub derive_timing_from_dmx: Option<bool>,
    /// 本帧 core 模式生效的 `num_obj_info_blocks`。
    pub dmx_num_obj_info_blocks: u8,
    /// 本帧 full 模式生效的 `num_obj_info_blocks`。
    pub umx_num_obj_info_blocks: u8,
    /// core/downmix 侧完整 `oamd_dyndata_single()` 的已验证边界。
    ///
    /// alternative dataset 的 bit offset 相对构造 `reader` 时传入的源切片；调用方须把
    /// 同一切片传给 [`OamdDyndataSingle::alternative_data_sets`]。
    pub dmx_oamd: OamdDyndataSingle,
    /// full/upmix 侧完整 `oamd_dyndata_single()` 的已验证边界，约定同 [`Self::dmx_oamd`]。
    pub umx_oamd: OamdDyndataSingle,
    dmx_active_mask: [bool; MAX_FULLBAND_DMX_SIGNALS as usize],
    dmx_blocks_written: usize,
    umx_blocks_written: usize,
}

impl AudioDataAjoc {
    /// `dmx_active_signals_mask[]` 的第 `signal` 位。
    ///
    /// `b_some_signals_inactive` 为假时不传输该掩码，全部信号视为活动。
    #[must_use]
    pub fn dmx_signal_active(&self, signal: usize) -> Option<bool> {
        if signal >= usize::from(self.var_element.n_dmx_signals) {
            return None;
        }
        if !self.some_signals_inactive {
            return Some(true);
        }
        self.dmx_active_mask.get(signal).copied()
    }

    /// 写入 `dmx_blocks` 的信息块数，即对象数乘以
    /// [`Self::dmx_num_obj_info_blocks`]。
    #[must_use]
    pub const fn dmx_blocks_written(&self) -> usize {
        self.dmx_blocks_written
    }

    /// 写入 `umx_blocks` 的信息块数。
    #[must_use]
    pub const fn umx_blocks_written(&self) -> usize {
        self.umx_blocks_written
    }
}

/// 解析 `audio_data_ajoc()`，见 P2 `6.2.3.4`。
///
/// # Errors
///
/// 见 [`AudioDataError`]。
pub fn parse_audio_data_ajoc(
    reader: &mut BitReader<'_>,
    params: AudioDataParams,
    state: &mut AudioDataState,
    workspace: AudioDataWorkspace<'_>,
) -> Result<AudioDataAjoc, AudioDataError> {
    if params.b_static_dmx {
        return Err(AudioDataError::StaticDownmixUnsupported);
    }

    let AudioDataWorkspace {
        elements,
        aspx,
        controls,
        matrices,
        dmx_objects,
        umx_objects,
        dmx_blocks,
        umx_blocks,
    } = workspace;

    let lfe = u32::from(params.b_lfe);
    let n_dmx_objects = u32::from(params.n_fb_dmx_signals).saturating_add(lfe);
    let n_umx_objects = params.n_fb_upmix_signals.saturating_add(lfe);
    ensure_objects(
        dmx_objects,
        n_dmx_objects,
        params.b_lfe,
        AudioDataMode::Core,
    )?;
    ensure_objects(
        umx_objects,
        n_umx_objects,
        params.b_lfe,
        AudioDataMode::Full,
    )?;

    let mut next_state = *state;

    let some_signals_inactive = reader.read_flag()?;
    let mut dmx_active_mask = [true; MAX_FULLBAND_DMX_SIGNALS as usize];
    if some_signals_inactive {
        // 掩码按 n_fullband_dmx_signals 传输，不含 LFE。
        for signal in 0..usize::from(params.n_fb_dmx_signals) {
            let active = reader.read_flag()?;
            if let Some(slot) = dmx_active_mask.get_mut(signal) {
                *slot = active;
            }
        }
    }

    let var_element = parse_var_channel_element(
        reader,
        VarChannelParams {
            context: params.context,
            n_dmx_signals: params.n_fb_dmx_signals,
            b_has_lfe: params.b_lfe,
            b_iframe: params.b_iframe,
        },
        &mut next_state.var_channel,
        VarChannelWorkspace { elements, aspx },
    )?;

    let dmx_timing = if reader.read_flag()? {
        Some(OamdTimingData::parse(reader)?)
    } else {
        None
    };
    // 独立 timing 缺席时，当前 group timing 优先于该模式的跨帧历史。
    let dmx_num_obj_info_blocks = effective_block_count(
        dmx_timing,
        params
            .group_num_obj_info_blocks
            .or(next_state.dmx_num_obj_info_blocks),
        params.b_iframe,
        AudioDataMode::Core,
    )?;
    next_state.dmx_num_obj_info_blocks = Some(dmx_num_obj_info_blocks);

    let (dmx_oamd, dmx_blocks_written) = parse_dyndata_single(
        reader,
        dmx_objects,
        dmx_num_obj_info_blocks,
        params.b_iframe,
        params
            .b_alternative
            .then_some(OamdAlternativeDataSetDomain::AjocCore),
        dmx_blocks,
    )?;

    let bed_info = if reader.read_flag()? {
        let declared = reader.variable_bits_scaled_u32(3, 8, 3)?;
        let info = parse_bed_info(reader)?;
        let remaining = declared.checked_sub(u32::from(info.bits_read)).ok_or(
            AudioDataError::ExtensionUnderflow {
                declared,
                consumed: info.bits_read,
            },
        )?;
        reader.skip_bits(u64::from(remaining))?;
        Some(info)
    } else {
        None
    };

    let ajoc = parse_ajoc(
        reader,
        params.n_fb_dmx_signals,
        params.n_fb_upmix_signals,
        controls,
        matrices,
    )?;
    let dmx_de = parse_dmx_de_data(
        reader,
        params.n_fb_dmx_signals,
        params.n_fb_upmix_signals,
        params.b_iframe,
        &mut next_state.de,
    )?;

    let (umx_timing, derive_timing_from_dmx) = if reader.read_flag()? {
        (Some(OamdTimingData::parse(reader)?), None)
    } else {
        (None, Some(reader.read_flag()?))
    };
    // 只有 b_derive_timing_from_dmx 为真时，core timing 才可用于 full 模式；
    // 否则回落到 group timing 或 full 模式自身的跨帧历史。
    let umx_fallback = if derive_timing_from_dmx == Some(true) {
        Some(dmx_num_obj_info_blocks)
    } else {
        params
            .group_num_obj_info_blocks
            .or(next_state.umx_num_obj_info_blocks)
    };
    let umx_num_obj_info_blocks = effective_block_count(
        umx_timing,
        umx_fallback,
        params.b_iframe,
        AudioDataMode::Full,
    )?;
    next_state.umx_num_obj_info_blocks = Some(umx_num_obj_info_blocks);

    let (umx_oamd, umx_blocks_written) = parse_dyndata_single(
        reader,
        umx_objects,
        umx_num_obj_info_blocks,
        params.b_iframe,
        params
            .b_alternative
            .then_some(OamdAlternativeDataSetDomain::AjocFull),
        umx_blocks,
    )?;

    *state = next_state;
    Ok(AudioDataAjoc {
        var_element,
        ajoc,
        dmx_de,
        bed_info,
        some_signals_inactive,
        dmx_timing,
        umx_timing,
        derive_timing_from_dmx,
        dmx_num_obj_info_blocks,
        umx_num_obj_info_blocks,
        dmx_oamd,
        umx_oamd,
        dmx_active_mask,
        dmx_blocks_written,
        umx_blocks_written,
    })
}

/// `oamd_dyndata_single()`，见 P2 `6.2.8.3`。
///
/// 每个对象连续 `n_blocks` 个 `object_info_block()`；首块在 I 帧时不得引用
/// 前序状态。共享解析器同时验证并保留随后的全部 alternative datasets。
///
/// 返回完整元素边界与写入 `out` 的信息块数。
fn parse_dyndata_single(
    reader: &mut BitReader<'_>,
    objects: &[ObjectDescriptor],
    n_blocks: u8,
    b_iframe: bool,
    alternative_domain: Option<OamdAlternativeDataSetDomain>,
    out: &mut [OamdMetadataBlock],
) -> Result<(OamdDyndataSingle, usize), AudioDataError> {
    let needed = objects.len().saturating_mul(usize::from(n_blocks));
    if out.len() < needed {
        return Err(AudioDataError::ObjectWorkspaceTooSmall {
            needed,
            provided: out.len(),
        });
    }
    let mut written = 0usize;
    let parsed = OamdDyndataSingle::parse_with_block_observer(
        reader,
        objects,
        n_blocks,
        b_iframe,
        alternative_domain,
        |block| {
            if let Some(slot) = out.get_mut(written) {
                *slot = block;
            }
            written = written.saturating_add(1);
        },
    )?;
    Ok((parsed, written))
}

fn effective_block_count(
    explicit: Option<OamdTimingData>,
    fallback: Option<u8>,
    b_iframe: bool,
    mode: AudioDataMode,
) -> Result<u8, AudioDataError> {
    let count = explicit
        .map(|timing| timing.num_obj_info_blocks)
        .or(fallback)
        .ok_or(AudioDataError::TimingUnavailable { mode })?;
    if b_iframe && count == 0 {
        return Err(AudioDataError::ZeroObjectInfoBlocksInIframe { mode });
    }
    Ok(count)
}

fn ensure_objects(
    objects: &[ObjectDescriptor],
    needed: u32,
    b_lfe: bool,
    mode: AudioDataMode,
) -> Result<(), AudioDataError> {
    let needed = usize::try_from(needed).unwrap_or(usize::MAX);
    if objects.len() != needed {
        return Err(AudioDataError::ObjectWorkspaceTooSmall {
            needed,
            provided: objects.len(),
        });
    }
    for (index, object) in objects.iter().enumerate() {
        let expected = b_lfe && index == 0;
        if object.b_lfe != expected {
            return Err(AudioDataError::InvalidLfeLayout {
                mode,
                index,
                expected,
                actual: object.b_lfe,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ajoc::MAX_AJOC_DMX_SIGNALS;
    use crate::testutil::BitBuf;
    use macindecode_ac4_bitstream::oamd::{
        InfoStatus, OamdAlternativeDataSetState, OamdAlternativeDataSetStateError, ObjectType,
    };

    const CONTEXT: ChannelContext = ChannelContext {
        frame_len_base: 2048,
        sampling_frequency_hz: 48_000,
    };

    fn bed(is_lfe: bool) -> ObjectDescriptor {
        ObjectDescriptor {
            obj_type: ObjectType::Bed,
            b_lfe: is_lfe,
            b_ajoc_coded: true,
        }
    }

    fn dynamic() -> ObjectDescriptor {
        ObjectDescriptor {
            obj_type: ObjectType::Dynamic,
            b_lfe: false,
            b_ajoc_coded: true,
        }
    }

    /// 单信号带 LFE 的完整元素分别使用 core 与 group timing。
    ///
    /// 这是全链路判据：`b_some_signals_inactive`、`var_channel_element()`、
    /// 两处时间数据与动态数据、`ajoc()`、`ajoc_dmx_de_data()` 串成一体，任一
    /// 段的宽度或循环次数写错都会偏移。
    #[test]
    fn full_element_lands_exactly_and_keeps_timings_independent() {
        let num_umx = 2usize; // n_fb_upmix_signals = 1 加 LFE
        let mut buf = BitBuf::new();
        buf.push(false); // b_some_signals_inactive = 0

        // var_channel_element：A-SPX、一个全频带信号、带 LFE。
        buf.push(true); // var_codec_mode = A-SPX
        buf.push_aspx_config();
        buf.push(true); // companding_control(1) 的 b_compand_on[0]
        buf.push_bits(2, 3); // LFE 的 max_sfb
        buf.push_empty_sf_data(2);
        buf.push_mono_data(2);
        buf.push_aspx_data_1ch();

        buf.push(true); // b_dmx_timing
        buf.push_timing(1); // num_obj_info_blocks = 1
        for _ in 0..2 {
            buf.push_inactive_object_block(); // 两个下混对象各一块
        }
        buf.push(false); // b_oamd_extension_present

        buf.push_minimal_ajoc(1); // ajoc 的对象数是 n_fb_upmix_signals
        buf.push_minimal_dmx_de(1);

        buf.push(false); // b_umx_timing = 0
        buf.push(false); // b_derive_timing_from_dmx
        // derive=false：full 模式使用 group timing 的 2 块，而不是 core 的 1 块。
        for _ in 0..num_umx.saturating_mul(2) {
            buf.push_inactive_object_block();
        }
        let expected = buf.bit_len();

        let mut elements = [ChannelElement::new(), ChannelElement::new()];
        let mut aspx = [AspxData::empty()];
        let mut controls = [AjocObjectControl::default()];
        let mut matrices = [AjocObjectMatrix::new()];
        let dmx_objects = [bed(true), dynamic()];
        let umx_objects = [bed(true), dynamic()];
        let mut dmx_blocks = [OamdMetadataBlock::default(); 4];
        let mut umx_blocks = [OamdMetadataBlock::default(); 4];
        let mut state = AudioDataState::new();

        let mut reader = BitReader::new(buf.as_slice());
        let data = parse_audio_data_ajoc(
            &mut reader,
            AudioDataParams {
                context: CONTEXT,
                n_fb_dmx_signals: 1,
                n_fb_upmix_signals: 1,
                b_lfe: true,
                b_iframe: true,
                b_static_dmx: false,
                b_alternative: false,
                group_num_obj_info_blocks: Some(2),
            },
            &mut state,
            AudioDataWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
                controls: &mut controls,
                matrices: &mut matrices,
                dmx_objects: &dmx_objects,
                umx_objects: &umx_objects,
                dmx_blocks: &mut dmx_blocks,
                umx_blocks: &mut umx_blocks,
            },
        )
        .expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );
        assert!(!data.some_signals_inactive);
        assert_eq!(data.dmx_num_obj_info_blocks, 1);
        assert_eq!(data.umx_num_obj_info_blocks, 2);
        assert_eq!(data.dmx_blocks_written(), 2);
        assert_eq!(data.umx_blocks_written(), 4);
        assert_eq!(data.derive_timing_from_dmx, Some(false));
        assert!(data.umx_timing.is_none());
        assert_eq!(data.dmx_signal_active(0), Some(true));
        assert_eq!(data.dmx_signal_active(1), None, "掩码按不含 LFE 的信号数");
        assert_eq!(state.dmx_num_obj_info_blocks, Some(1));
        assert_eq!(state.umx_num_obj_info_blocks, Some(2));
    }

    /// A-JOC 的 core/downmix 与 full/upmix 两处 `oamd_dyndata_single()` 都必须
    /// 完整消费并保留 alternative datasets；解析层不根据 presentation target
    /// 选择其中任何一项，两份同 index、同布局的列表也不得共享 `b_keep` 状态。
    #[test]
    fn full_element_preserves_both_alternative_oamd_lists() {
        let mut buf = BitBuf::new();
        buf.push(false); // b_some_signals_inactive = 0

        // var_channel_element：A-SPX、一个全频带信号、带 LFE。
        buf.push(true);
        buf.push_aspx_config();
        buf.push(true);
        buf.push_bits(2, 3);
        buf.push_empty_sf_data(2);
        buf.push_mono_data(2);
        buf.push_aspx_data_1ch();

        buf.push(true); // b_dmx_timing
        buf.push_timing(1);
        for _ in 0..2 {
            buf.push_inactive_object_block();
        }
        // core/downmix alternative：一个公共 gain dataset。
        buf.push(true); // b_ducking_disabled
        buf.push_bits(2, 2); // object_sound_category
        buf.push_bits(1, 2); // n_alt_data_sets
        buf.push(false); // b_keep
        buf.push(true); // b_common_data
        buf.push(true); // b_alt_gain
        buf.push_bits(5, 6);
        buf.push(false); // b_additional_data
        let dmx_oamd_end = buf.bit_len();

        buf.push(false); // b_oamd_extension_present
        buf.push_minimal_ajoc(1);
        buf.push_minimal_dmx_de(1);
        buf.push(false); // b_umx_timing
        buf.push(true); // b_derive_timing_from_dmx
        for _ in 0..2 {
            buf.push_inactive_object_block();
        }
        // full/upmix alternative：一个新公共 gain dataset 加一个 keep dataset。
        buf.push(false); // b_ducking_disabled
        buf.push_bits(1, 2); // object_sound_category
        buf.push_bits(2, 2); // n_alt_data_sets
        buf.push(false); // dataset 0: b_keep
        buf.push(true); // b_common_data
        buf.push(true); // b_alt_gain
        buf.push_bits(9, 6);
        buf.push(false); // b_additional_data
        buf.push(true); // dataset 1: b_keep
        buf.push(false); // b_additional_data
        let expected = buf.bit_len();

        let mut elements = [ChannelElement::new(), ChannelElement::new()];
        let mut aspx = [AspxData::empty()];
        let mut controls = [AjocObjectControl::default()];
        let mut matrices = [AjocObjectMatrix::new()];
        let dmx_objects = [bed(true), dynamic()];
        let umx_objects = [bed(true), dynamic()];
        let mut dmx_blocks = [OamdMetadataBlock::default(); 2];
        let mut umx_blocks = [OamdMetadataBlock::default(); 2];
        let mut state = AudioDataState::new();

        let mut reader = BitReader::new(buf.as_slice());
        let data = parse_audio_data_ajoc(
            &mut reader,
            AudioDataParams {
                context: CONTEXT,
                n_fb_dmx_signals: 1,
                n_fb_upmix_signals: 1,
                b_lfe: true,
                b_iframe: true,
                b_static_dmx: false,
                b_alternative: true,
                group_num_obj_info_blocks: None,
            },
            &mut state,
            AudioDataWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
                controls: &mut controls,
                matrices: &mut matrices,
                dmx_objects: &dmx_objects,
                umx_objects: &umx_objects,
                dmx_blocks: &mut dmx_blocks,
                umx_blocks: &mut umx_blocks,
            },
        )
        .expect("两处 alternative OAMD 都应能解析");

        assert_eq!(reader.bit_position(), expected as u64);
        assert_eq!(data.dmx_oamd.end_bit_offset(), dmx_oamd_end as u64);
        assert_eq!(data.umx_oamd.end_bit_offset(), expected as u64);
        assert_eq!(data.dmx_blocks_written(), 2);
        assert_eq!(data.umx_blocks_written(), 2);

        let dmx_header = data.dmx_oamd.alternative().expect("core 应有 header");
        assert_eq!(dmx_header.domain, OamdAlternativeDataSetDomain::AjocCore);
        assert!(dmx_header.b_ducking_disabled);
        assert_eq!(dmx_header.object_sound_category, 2);
        assert_eq!(dmx_header.n_data_sets, 1);
        let mut dmx_sets = data
            .dmx_oamd
            .alternative_data_sets(buf.as_slice())
            .expect("边界应可重放")
            .expect("core 应有 dataset 迭代器");
        let dmx_set = dmx_sets.next().expect("应有一个 core dataset").unwrap();
        assert!(dmx_sets.next().is_none());
        assert_eq!(dmx_set.domain, OamdAlternativeDataSetDomain::AjocCore);
        assert!(!dmx_set.keep);
        assert_eq!(dmx_set.common_data, Some(true));
        let dmx_point = dmx_set
            .data_points()
            .expect("数据点边界应有效")
            .next()
            .expect("应有公共数据点")
            .unwrap();
        assert_eq!(dmx_point.alternative_gain, Some(5));

        let umx_header = data.umx_oamd.alternative().expect("full 应有 header");
        assert_eq!(umx_header.domain, OamdAlternativeDataSetDomain::AjocFull);
        assert!(!umx_header.b_ducking_disabled);
        assert_eq!(umx_header.object_sound_category, 1);
        assert_eq!(umx_header.n_data_sets, 2);
        let mut umx_sets = data
            .umx_oamd
            .alternative_data_sets(buf.as_slice())
            .expect("边界应可重放")
            .expect("full 应有 dataset 迭代器");
        let umx_first = umx_sets.next().expect("应有首个 full dataset").unwrap();
        assert_eq!(umx_first.domain, OamdAlternativeDataSetDomain::AjocFull);
        let umx_point = umx_first
            .data_points()
            .expect("数据点边界应有效")
            .next()
            .expect("应有公共数据点")
            .unwrap();
        assert_eq!(umx_point.alternative_gain, Some(9));

        let mut core_state = OamdAlternativeDataSetState::new();
        let core_effective = core_state.apply(dmx_set).unwrap();
        assert_eq!(
            core_effective
                .data_points()
                .first()
                .and_then(|point| point.alternative_gain),
            Some(5)
        );
        assert_eq!(
            core_state.domain(),
            Some(OamdAlternativeDataSetDomain::AjocCore)
        );
        let previous = core_state;
        assert_eq!(
            core_state.apply(umx_first),
            Err(OamdAlternativeDataSetStateError::DataSetDomainChanged {
                previous: OamdAlternativeDataSetDomain::AjocCore,
                current: OamdAlternativeDataSetDomain::AjocFull,
            })
        );
        assert_eq!(core_state, previous, "跨域失败不得覆盖 core gain");

        let mut full_state = OamdAlternativeDataSetState::new();
        let full_effective = full_state.apply(umx_first).unwrap();
        assert_eq!(
            full_effective
                .data_points()
                .first()
                .and_then(|point| point.alternative_gain),
            Some(9)
        );
        let umx_kept = umx_sets.next().expect("应有 keep dataset").unwrap();
        assert!(umx_kept.keep);
        assert!(umx_sets.next().is_none());
    }

    /// `b_some_signals_inactive` 的掩码按 `n_fullband_dmx_signals` 传输。
    ///
    /// 该长度**不含 LFE**。误用含 LFE 的对象数会多读一位，故必须用完整元素
    /// 的落点来判——只断言「至少读了几位」抓不到多读。
    #[test]
    fn inactive_mask_excludes_the_lfe() {
        let mut buf = BitBuf::new();
        buf.push(true); // b_some_signals_inactive
        buf.push(true); // 唯一的全频带信号活动；LFE 不在掩码内
        buf.push(true); // var_codec_mode = A-SPX
        buf.push_aspx_config();
        buf.push(true); // companding_control(1)
        buf.push_bits(2, 3); // LFE 的 max_sfb
        buf.push_empty_sf_data(2);
        buf.push_mono_data(2);
        buf.push_aspx_data_1ch();
        buf.push(true); // b_dmx_timing
        buf.push_timing(1);
        for _ in 0..2 {
            buf.push_inactive_object_block();
        }
        buf.push(false); // b_oamd_extension_present
        buf.push_minimal_ajoc(1);
        buf.push_minimal_dmx_de(1);
        buf.push(false); // b_umx_timing
        buf.push(true); // b_derive_timing_from_dmx
        for _ in 0..2 {
            buf.push_inactive_object_block();
        }
        let expected = buf.bit_len();

        let mut elements = [ChannelElement::new(), ChannelElement::new()];
        let mut aspx = [AspxData::empty()];
        let mut controls = [AjocObjectControl::default()];
        let mut matrices = [AjocObjectMatrix::new()];
        let dmx_objects = [bed(true), dynamic()];
        let umx_objects = [bed(true), dynamic()];
        let mut dmx_blocks = [OamdMetadataBlock::default(); 4];
        let mut umx_blocks = [OamdMetadataBlock::default(); 4];
        let mut state = AudioDataState::new();

        let mut reader = BitReader::new(buf.as_slice());
        let data = parse_audio_data_ajoc(
            &mut reader,
            AudioDataParams {
                context: CONTEXT,
                n_fb_dmx_signals: 1,
                n_fb_upmix_signals: 1,
                b_lfe: true,
                b_iframe: true,
                b_static_dmx: false,
                b_alternative: false,
                group_num_obj_info_blocks: None,
            },
            &mut state,
            AudioDataWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
                controls: &mut controls,
                matrices: &mut matrices,
                dmx_objects: &dmx_objects,
                umx_objects: &umx_objects,
                dmx_blocks: &mut dmx_blocks,
                umx_blocks: &mut umx_blocks,
            },
        )
        .expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "掩码只按不含 LFE 的信号数传输"
        );
        assert!(data.some_signals_inactive);
        assert_eq!(data.dmx_signal_active(0), Some(true));
        assert_eq!(data.dmx_signal_active(1), None, "掩码不覆盖 LFE");
    }

    /// 本帧未传时间数据且无历史时必须报错，而不是把块数当成零。
    #[test]
    fn rejects_missing_timing_without_history() {
        let mut buf = BitBuf::new();
        buf.push(false); // b_some_signals_inactive
        buf.push(true); // var_codec_mode = A-SPX
        buf.push_aspx_config();
        buf.push(true); // companding_control(1)
        buf.push_bits(2, 3);
        buf.push_empty_sf_data(2);
        buf.push_mono_data(2);
        buf.push_aspx_data_1ch();
        buf.push(false); // b_dmx_timing = 0，且状态里没有历史

        let mut elements = [ChannelElement::new(), ChannelElement::new()];
        let mut aspx = [AspxData::empty()];
        let mut controls = [AjocObjectControl::default()];
        let mut matrices = [AjocObjectMatrix::new()];
        let dmx_objects = [bed(true), dynamic()];
        let umx_objects = [bed(true), dynamic()];
        let mut dmx_blocks = [OamdMetadataBlock::default(); 4];
        let mut umx_blocks = [OamdMetadataBlock::default(); 4];
        let mut state = AudioDataState::new();

        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_audio_data_ajoc(
                &mut reader,
                AudioDataParams {
                    context: CONTEXT,
                    n_fb_dmx_signals: 1,
                    n_fb_upmix_signals: 1,
                    b_lfe: true,
                    b_iframe: true,
                    b_static_dmx: false,
                    b_alternative: false,
                    group_num_obj_info_blocks: None,
                },
                &mut state,
                AudioDataWorkspace {
                    elements: &mut elements,
                    aspx: &mut aspx,
                    controls: &mut controls,
                    matrices: &mut matrices,
                    dmx_objects: &dmx_objects,
                    umx_objects: &umx_objects,
                    dmx_blocks: &mut dmx_blocks,
                    umx_blocks: &mut umx_blocks,
                },
            ),
            Err(AudioDataError::TimingUnavailable {
                mode: AudioDataMode::Core
            })
        );
        assert_eq!(state.dmx_num_obj_info_blocks, None, "失败帧不得记下块数");
        assert_eq!(state.umx_num_obj_info_blocks, None, "失败帧不得记下块数");
    }

    /// `6.3.9.3.6`：I 帧的两种解码模式都不得使用零个对象信息块。
    #[test]
    fn iframe_rejects_zero_object_info_blocks() {
        for mode in [AudioDataMode::Core, AudioDataMode::Full] {
            assert_eq!(
                effective_block_count(None, Some(0), true, mode),
                Err(AudioDataError::ZeroObjectInfoBlocksInIframe { mode })
            );
            assert_eq!(
                effective_block_count(None, Some(0), false, mode),
                Ok(0),
                "非 I 帧允许没有对象更新"
            );
        }
    }

    /// 中途失败的帧不得提交跨帧状态。
    ///
    /// 状态含 `aspx_config`、逐元素交叉偏移、对话配置与块数四项，它们在元素
    /// 内部分散写入；直接写而非用副本时，一次失败会留下半新半旧的组合。
    #[test]
    fn failed_frame_does_not_commit_state() {
        let mut elements = [ChannelElement::new(), ChannelElement::new()];
        let mut aspx = [AspxData::empty()];
        let mut controls = [AjocObjectControl::default()];
        let mut matrices = [AjocObjectMatrix::new()];
        let dmx_objects = [bed(true), dynamic()];
        let umx_objects = [bed(true), dynamic()];
        let mut dmx_blocks = [OamdMetadataBlock::default(); 4];
        let mut umx_blocks = [OamdMetadataBlock::default(); 4];
        let mut state = AudioDataState::new();

        let params = AudioDataParams {
            context: CONTEXT,
            n_fb_dmx_signals: 1,
            n_fb_upmix_signals: 1,
            b_lfe: true,
            b_iframe: true,
            b_static_dmx: false,
            b_alternative: false,
            group_num_obj_info_blocks: Some(1),
        };

        // 先跑一帧建立完整状态。
        let mut good = BitBuf::new();
        good.push(false);
        good.push(true);
        good.push_aspx_config();
        good.push(true);
        good.push_bits(2, 3);
        good.push_empty_sf_data(2);
        good.push_mono_data(2);
        good.push_aspx_data_1ch();
        good.push(true);
        good.push_timing(1);
        for _ in 0..2 {
            good.push_inactive_object_block();
        }
        good.push(false);
        good.push_minimal_ajoc(1);
        good.push_minimal_dmx_de(1);
        good.push(false);
        good.push(false);
        for _ in 0..2 {
            good.push_inactive_object_block();
        }
        let mut reader = BitReader::new(good.as_slice());
        parse_audio_data_ajoc(
            &mut reader,
            params,
            &mut state,
            AudioDataWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
                controls: &mut controls,
                matrices: &mut matrices,
                dmx_objects: &dmx_objects,
                umx_objects: &umx_objects,
                dmx_blocks: &mut dmx_blocks,
                umx_blocks: &mut umx_blocks,
            },
        )
        .expect("基准帧应能解析");
        let before = state;

        // 次帧走到 ajoc_dmx_de_data 之后才截断：此时 var_channel 与 de 都已
        // 写入新值，若直接提交就会与未更新的块数混在一起。
        let mut damaged = BitBuf::new();
        damaged.push(false);
        damaged.push(true);
        damaged.push_aspx_config();
        damaged.push(true);
        damaged.push_bits(2, 3);
        damaged.push_empty_sf_data(2);
        damaged.push_mono_data(2);
        damaged.push_aspx_data_1ch();
        damaged.push(true);
        damaged.push_timing(2); // 与基准帧不同的块数
        for _ in 0..4 {
            damaged.push_inactive_object_block();
        }
        damaged.push(false);
        damaged.push_minimal_ajoc(1);
        damaged.push_minimal_dmx_de(1);
        // 到此为止；后续的 b_umx_timing 与上混动态数据缺失。

        let mut reader = BitReader::new(damaged.as_slice());
        let result = parse_audio_data_ajoc(
            &mut reader,
            params,
            &mut state,
            AudioDataWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
                controls: &mut controls,
                matrices: &mut matrices,
                dmx_objects: &dmx_objects,
                umx_objects: &umx_objects,
                dmx_blocks: &mut dmx_blocks,
                umx_blocks: &mut umx_blocks,
            },
        );
        assert!(result.is_err(), "上混段缺失应报错");
        assert_eq!(state, before, "失败帧不得提交任何跨帧状态");
    }

    /// I 帧的首块不读 `reuse` 标志，直接取 `AllNew`。
    ///
    /// 两条路径的比特数可能相同——`b_no_delta` 为假时那一位会被当作 reuse
    /// 标志读成 `Reuse`——故必须断言状态而非只看落点。
    #[test]
    fn iframe_first_block_skips_the_reuse_flag() {
        let objects = [bed(false)]; // 活动、非动态：渲染信息整段不出现
        let mut buf = BitBuf::new();
        buf.push(false); // b_object_not_active = 0
        buf.push(true); // ObjectBasicInfo 的 default_metadata
        buf.push(false); // b_additional_data

        let mut out = [OamdMetadataBlock::default(); 2];
        let mut reader = BitReader::new(buf.as_slice());
        parse_dyndata_single(&mut reader, &objects, 1, true, None, &mut out).expect("应能解析");

        assert_eq!(reader.bit_position(), 3);
        let Some(block) = out.first() else {
            panic!("应有一个信息块");
        };
        assert_eq!(
            block.info.basic_info_status,
            InfoStatus::AllNew,
            "I 帧首块必须取 AllNew，那一位是 default_metadata 而非 reuse 标志"
        );

        // 同样三位、b_no_delta 为假时，中间那位被当作 reuse 标志。
        let mut reader = BitReader::new(buf.as_slice());
        parse_dyndata_single(&mut reader, &objects, 1, false, None, &mut out).expect("应能解析");
        let Some(block) = out.first() else {
            panic!("应有一个信息块");
        };
        assert_eq!(block.info.basic_info_status, InfoStatus::Reuse);
    }

    /// 静态下混分支必须显式拒绝，且不消耗任何比特。
    #[test]
    fn rejects_static_downmix_before_reading() {
        let buf = BitBuf::new();
        let dmx_objects = [bed(true)];
        let umx_objects = [bed(true)];

        let mut elements = [ChannelElement::new()];
        let mut aspx = [AspxData::empty()];
        let mut controls = [AjocObjectControl::default()];
        let mut matrices = [AjocObjectMatrix::new()];
        let mut dmx_blocks = [OamdMetadataBlock::default(); 2];
        let mut umx_blocks = [OamdMetadataBlock::default(); 2];
        let mut state = AudioDataState::new();
        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_audio_data_ajoc(
                &mut reader,
                AudioDataParams {
                    context: CONTEXT,
                    n_fb_dmx_signals: 0,
                    n_fb_upmix_signals: 0,
                    b_lfe: true,
                    b_iframe: true,
                    b_static_dmx: true,
                    b_alternative: false,
                    group_num_obj_info_blocks: None,
                },
                &mut state,
                AudioDataWorkspace {
                    elements: &mut elements,
                    aspx: &mut aspx,
                    controls: &mut controls,
                    matrices: &mut matrices,
                    dmx_objects: &dmx_objects,
                    umx_objects: &umx_objects,
                    dmx_blocks: &mut dmx_blocks,
                    umx_blocks: &mut umx_blocks,
                },
            ),
            Err(AudioDataError::StaticDownmixUnsupported)
        );
        assert_eq!(reader.bit_position(), 0);
    }

    /// 对象描述的长度必须恰好等于含 LFE 的对象数。
    #[test]
    fn rejects_object_descriptor_count_mismatch() {
        let buf = BitBuf::new();
        let mut elements = [ChannelElement::new()];
        let mut aspx = [AspxData::empty()];
        let mut controls = [AjocObjectControl::default()];
        let mut matrices = [AjocObjectMatrix::new()];
        let dmx_objects = [bed(true)]; // 少了一个：1 个全频带信号加 LFE 应为 2
        let umx_objects = [bed(true), dynamic()];
        let mut dmx_blocks = [OamdMetadataBlock::default(); 4];
        let mut umx_blocks = [OamdMetadataBlock::default(); 4];
        let mut state = AudioDataState::new();

        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_audio_data_ajoc(
                &mut reader,
                AudioDataParams {
                    context: CONTEXT,
                    n_fb_dmx_signals: 1,
                    n_fb_upmix_signals: 1,
                    b_lfe: true,
                    b_iframe: true,
                    b_static_dmx: false,
                    b_alternative: false,
                    group_num_obj_info_blocks: None,
                },
                &mut state,
                AudioDataWorkspace {
                    elements: &mut elements,
                    aspx: &mut aspx,
                    controls: &mut controls,
                    matrices: &mut matrices,
                    dmx_objects: &dmx_objects,
                    umx_objects: &umx_objects,
                    dmx_blocks: &mut dmx_blocks,
                    umx_blocks: &mut umx_blocks,
                },
            ),
            Err(AudioDataError::ObjectWorkspaceTooSmall {
                needed: 2,
                provided: 1
            })
        );
        assert_eq!(reader.bit_position(), 0);
    }

    /// `6.2.3.4` 在存在 LFE 时固定令信号 0 为 LFE，其余信号均非 LFE。
    #[test]
    fn rejects_lfe_descriptor_out_of_signal_order() {
        let misplaced = [dynamic(), bed(true)];
        assert_eq!(
            ensure_objects(&misplaced, 2, true, AudioDataMode::Core),
            Err(AudioDataError::InvalidLfeLayout {
                mode: AudioDataMode::Core,
                index: 0,
                expected: true,
                actual: false,
            })
        );

        let duplicate = [bed(true), bed(true)];
        assert_eq!(
            ensure_objects(&duplicate, 2, true, AudioDataMode::Full),
            Err(AudioDataError::InvalidLfeLayout {
                mode: AudioDataMode::Full,
                index: 1,
                expected: false,
                actual: true,
            })
        );

        assert_eq!(
            ensure_objects(&[bed(true)], 1, false, AudioDataMode::Core),
            Err(AudioDataError::InvalidLfeLayout {
                mode: AudioDataMode::Core,
                index: 0,
                expected: false,
                actual: true,
            })
        );
    }

    /// 解出的块可直接交给 `OamdState`，位置在帧内逐块推进。
    ///
    /// 这是 M3「帧内多次更新」与「逐对象位置」的接口：`oamd_substream()` 与
    /// `audio_data_ajoc()` 两条路径共用同一套状态延续规则，只是入口不同。
    /// 差分位置必须相对**同一帧内的前一块**推进，若误相对帧首解算，第三块的
    /// 结果就会错。
    #[test]
    fn blocks_feed_the_shared_oamd_state_machine() {
        use macindecode_ac4_bitstream::oamd::OamdState;

        let objects = [dynamic()];
        let mut buf = BitBuf::new();
        buf.push_absolute_position_block(true, 20, 30, true, 5);
        buf.push_differential_position_block(2, 0, 1); // +2 / 0 / +1
        buf.push_differential_position_block(2, 7, 0); // +2 / -1 / 0

        let mut out = [OamdMetadataBlock::default(); 4];
        let mut reader = BitReader::new(buf.as_slice());
        let (_, written) =
            parse_dyndata_single(&mut reader, &objects, 3, true, None, &mut out).expect("应能解析");
        assert_eq!(written, 3);
        assert_eq!(
            reader.bit_position(),
            u64::try_from(buf.bit_len()).unwrap_or(0)
        );

        let mut state = OamdState::new();
        state
            .apply_blocks(out.get(..written).unwrap_or(&[]), Some(3))
            .expect("状态应能延续");

        let Some(object) = state.object(0) else {
            panic!("应有对象 0 的状态");
        };
        let Some(render) = object.render else {
            panic!("应有渲染状态");
        };
        assert!(object.active);
        assert_eq!(
            (render.position.x, render.position.y, render.position.z),
            (24, 29, 6),
            "两次差分应逐块累加到绝对位置之上"
        );
        assert_eq!(state.previous_num_obj_info_blocks(), Some(3));
    }

    /// `oamd_dyndata_single()` 按对象与块数展开，I 帧首块不引用前序状态。
    #[test]
    fn dyndata_single_expands_per_object_and_block() {
        for blocks in 1u8..=3 {
            let objects = [bed(true), dynamic(), dynamic()];
            let mut buf = BitBuf::new();
            for _ in 0..objects.len() {
                for _ in 0..blocks {
                    buf.push_inactive_object_block();
                }
            }
            let expected = buf.bit_len();

            let mut out = [OamdMetadataBlock::default(); 16];
            let mut reader = BitReader::new(buf.as_slice());
            let (_, written) =
                parse_dyndata_single(&mut reader, &objects, blocks, true, None, &mut out)
                    .expect("应能解析");

            assert_eq!(
                reader.bit_position(),
                u64::try_from(expected).unwrap_or(0),
                "{blocks} 块时的落点"
            );
            assert_eq!(written, objects.len().saturating_mul(usize::from(blocks)));

            // 两个下标必须由解析器写出，而不是留给调用方按顺序反推：
            // 每个信息块的长度可变，位置与对象的对应关系一旦靠约定传递，
            // 任何一侧的循环顺序改动都不会被落点判据发现。
            for (index, block) in out.iter().take(written).enumerate() {
                let object = index / usize::from(blocks);
                let slot = index % usize::from(blocks);
                assert_eq!(
                    (block.object_index, block.block_index),
                    (
                        u8::try_from(object).unwrap_or(u8::MAX),
                        u8::try_from(slot).unwrap_or(u8::MAX)
                    ),
                    "{blocks} 块时第 {index} 项的下标"
                );
            }
        }
    }

    /// 信息块工作区不足时在读取任何比特前拒绝。
    #[test]
    fn dyndata_single_rejects_small_workspace() {
        let objects = [bed(true), dynamic()];
        let buf = BitBuf::new();
        let mut out = [OamdMetadataBlock::default(); 3];
        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_dyndata_single(&mut reader, &objects, 2, true, None, &mut out),
            Err(AudioDataError::ObjectWorkspaceTooSmall {
                needed: 4,
                provided: 3
            })
        );
        assert_eq!(reader.bit_position(), 0);
    }

    /// 常量对齐：A-JOC 的下混上界与 `var_channel_element` 的一致。
    #[test]
    fn downmix_limits_agree_across_modules() {
        assert_eq!(
            MAX_AJOC_DMX_SIGNALS,
            usize::from(MAX_FULLBAND_DMX_SIGNALS),
            "两处上界都来自 4 位的 n_fullband_dmx_signals_minus1"
        );
    }
}
