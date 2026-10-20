//! A-SPX 的九个语法元素。
//!
//! 覆盖 `TS103190-1:v1.4.1` `4.2.12` 的表 50–58：`aspx_config()`、
//! `aspx_data_1ch()`、`aspx_data_2ch()`、`aspx_framing()`、
//! `aspx_delta_dir()`、`aspx_hfgen_iwc_1ch()`、`aspx_hfgen_iwc_2ch()`、
//! `aspx_ec_data()` 与 `aspx_huff_data()`。
//!
//! 三处跨帧状态由调用方持有并传入：
//!
//! - `aspx_config()` 只在 I 帧出现，非 I 帧沿用上一个 I 帧的值；
//! - `aspx_xover_subband_offset` 同样只在 I 帧出现；
//! - `previous_stop_pos` 逐声道延续，`5.7.6.3.3.1` 规定初值为
//!   `num_aspx_timeslots`。
//!
//! 循环次数取自两处推导：[`super::bands`] 给出各表的子带组数，
//! [`super::frames`] 给出包络数与逐包络的 `atsg_freqres`。

use super::bands::{AspxBandTables, BandError};
use super::codebooks::{cb_off, table_for};
use super::frames::{
    AspxInterval, AspxIntervalParams, FrameError, MAX_ATSG_NOISE, MAX_ATSG_SIG, MAX_REL_BORDERS,
};
use super::tables::{
    EnvelopeKind, HcbType, IntervalClass, MAX_ASPX_TIMESLOTS, MAX_SBG_MASTER, StereoMode,
    get_aspx_hcb, num_aspx_timeslots,
};
use crate::huffman::HuffmanError;
use core::fmt;
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

/// 一个包络内的最大子带组数，取主子带组表的上界。
pub const MAX_SBG_PER_ENVELOPE: usize = MAX_SBG_MASTER;

/// A-SPX 语法解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspxError {
    /// 读取比特时越过了数据末尾。
    Read(ReadError),
    /// Huffman 解码失败。
    Huffman(HuffmanError),
    /// 子带组表推导失败。
    Bands(BandError),
    /// 时频成帧推导失败。
    Frames(FrameError),
    /// `frame_len_base` 不在表 192 内，无法定出 `num_aspx_timeslots`。
    UnsupportedFrameLenBase {
        /// 帧长基准。
        frame_len_base: u16,
    },
    /// FIXFIX 的 `tmp_num_env` 解出了八个包络。
    ///
    /// `4.3.10.1.9` 明确「Eight envelopes is prohibited」，表 128 亦将
    /// FIXFIX 的上界定为 4。
    EightEnvelopesProhibited,
}

impl fmt::Display for AspxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AspxError::Read(error) => write!(f, "{error}"),
            AspxError::Huffman(error) => write!(f, "{error}"),
            AspxError::Bands(error) => write!(f, "{error}"),
            AspxError::Frames(error) => write!(f, "{error}"),
            AspxError::UnsupportedFrameLenBase { frame_len_base } => {
                write!(f, "frame_len_base {frame_len_base} is not in Table 192")
            }
            AspxError::EightEnvelopesProhibited => {
                write!(f, "Eight FIXFIX envelopes are forbidden by the syntax")
            }
        }
    }
}

impl core::error::Error for AspxError {}

impl From<ReadError> for AspxError {
    fn from(error: ReadError) -> Self {
        AspxError::Read(error)
    }
}

impl From<HuffmanError> for AspxError {
    fn from(error: HuffmanError) -> Self {
        AspxError::Huffman(error)
    }
}

impl From<BandError> for AspxError {
    fn from(error: BandError) -> Self {
        AspxError::Bands(error)
    }
}

impl From<FrameError> for AspxError {
    fn from(error: FrameError) -> Self {
        AspxError::Frames(error)
    }
}

/// `aspx_config()`，见 `4.2.12.1` 表 50。共 15 位，只在 I 帧出现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspxConfig {
    /// `aspx_quant_mode_env`：假为 1,5 dB，真为 3 dB。
    pub quant_mode_env: bool,
    /// `aspx_start_freq`，3 位。
    pub start_freq: u8,
    /// `aspx_stop_freq`，2 位。
    pub stop_freq: u8,
    /// `aspx_master_freq_scale`：假为低分辨率模板，真为高分辨率模板。
    pub master_freq_scale: bool,
    /// `aspx_interpolation`。
    pub interpolation: bool,
    /// `aspx_preflat`。
    pub preflat: bool,
    /// `aspx_limiter`，见表 122。
    pub limiter: bool,
    /// `aspx_noise_sbg`，2 位。
    pub noise_sbg: u8,
    /// `aspx_num_env_bits_fixfix`，见表 123：假为 1 位，真为 2 位。
    pub num_env_bits_fixfix: bool,
    /// `aspx_freq_res_mode`，2 位，见表 124。
    pub freq_res_mode: u8,
}

impl AspxConfig {
    /// 解析 `aspx_config()`。
    ///
    /// # Errors
    ///
    /// 数据不足时返回 [`AspxError::Read`]。
    pub fn parse(reader: &mut BitReader<'_>) -> Result<Self, AspxError> {
        Ok(Self {
            quant_mode_env: reader.read_flag()?,
            start_freq: read_u8(reader, 3)?,
            stop_freq: read_u8(reader, 2)?,
            master_freq_scale: reader.read_flag()?,
            interpolation: reader.read_flag()?,
            preflat: reader.read_flag()?,
            limiter: reader.read_flag()?,
            noise_sbg: read_u8(reader, 2)?,
            num_env_bits_fixfix: reader.read_flag()?,
            freq_res_mode: read_u8(reader, 2)?,
        })
    }
}

/// 单个 `aspx_data_1ch()` 或 `aspx_data_2ch()` 元素的跨帧状态。
///
/// **粒度是元素而非帧**：`aspx_xover_subband_offset` 由每个数据元素各自传
/// 输（表 51/52），`previous_stop_pos` 更是逐声道延续（`5.7.6.3.3.1`）。
/// 只有 `aspx_config()` 是 `var_channel_element()` 级的一份，故不在此处，
/// 由调用方持有并按引用传入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AspxState {
    xover: u8,
    previous_stop_pos: [i16; 2],
}

impl AspxState {
    /// 一个尚未解析过任何元素的初始状态。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            xover: 0,
            previous_stop_pos: [0; 2],
        }
    }

    /// 当前沿用的 `aspx_xover_subband_offset`。
    #[must_use]
    pub const fn xover(&self) -> u8 {
        self.xover
    }

    /// 重置逐声道的 `previous_stop_pos`，`5.7.6.3.3.1` 规定初值为时隙数。
    pub fn reset_stop_pos(&mut self, num_aspx_timeslots: u8) {
        self.previous_stop_pos = [i16::from(num_aspx_timeslots); 2];
    }
}

/// `aspx_framing()` 与 `aspx_delta_dir()` 的逐声道解析结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspxChannelFraming {
    /// `Pseudocode 76` 所需的成帧字段。
    pub params: AspxIntervalParams,
    /// 由 `params` 推出的包络边界与逐包络频率分辨率。
    pub interval: AspxInterval,
    /// `aspx_qmode_env[ch]`，FIXFIX 单包络时强制为假。
    pub qmode_env: bool,
    sig_delta_dir: [bool; MAX_ATSG_SIG],
    noise_delta_dir: [bool; MAX_ATSG_NOISE],
}

impl AspxChannelFraming {
    const fn empty() -> Self {
        Self {
            params: AspxIntervalParams::fixfix(1),
            interval: AspxInterval::empty(),
            qmode_env: false,
            sig_delta_dir: [false; MAX_ATSG_SIG],
            noise_delta_dir: [false; MAX_ATSG_NOISE],
        }
    }

    /// `aspx_sig_delta_dir[ch][env]`：假为频率方向，真为时间方向。
    #[must_use]
    pub fn sig_delta_dir(&self, env: usize) -> Option<bool> {
        if env >= usize::from(self.params.num_env) {
            return None;
        }
        self.sig_delta_dir.get(env).copied()
    }

    /// `aspx_noise_delta_dir[ch][env]`。
    #[must_use]
    pub fn noise_delta_dir(&self, env: usize) -> Option<bool> {
        if env >= usize::from(self.params.num_noise) {
            return None;
        }
        self.noise_delta_dir.get(env).copied()
    }

    /// 供其他模块的判据组装一份成帧结果；生产路径只由 `aspx_framing()` 产出。
    ///
    /// `params` 与 `interval` 分开传，**不由本函数替调用方推导**：判据要造得出
    /// 「区间声明三个包络、`num_env` 只有两个」这类失配去验下游报不报错，构造器
    /// 一旦替它们对齐，那类判据就再也响不了。
    #[cfg(test)]
    pub(crate) fn for_test(
        params: AspxIntervalParams,
        interval: AspxInterval,
        qmode_env: bool,
        sig_delta_dir: &[bool],
        noise_delta_dir: &[bool],
    ) -> Self {
        let mut out = Self::empty();
        out.params = params;
        out.interval = interval;
        out.qmode_env = qmode_env;
        for (slot, value) in out.sig_delta_dir.iter_mut().zip(sig_delta_dir) {
            *slot = *value;
        }
        for (slot, value) in out.noise_delta_dir.iter_mut().zip(noise_delta_dir) {
            *slot = *value;
        }
        out
    }
}

/// `aspx_hfgen_iwc_1ch()` 与 `aspx_hfgen_iwc_2ch()` 的逐声道结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspxHfGen {
    tna_mode: [u8; super::tables::MAX_SBG_NOISE as usize],
    add_harmonic: [bool; MAX_SBG_MASTER],
    fic_used_in_sfb: [bool; MAX_SBG_MASTER],
    tic_used_in_slot: [bool; MAX_ASPX_TIMESLOTS],
    num_sbg_noise: u8,
    num_sbg_sig_highres: u8,
    num_timeslots: u8,
}

impl AspxHfGen {
    const fn empty() -> Self {
        Self {
            tna_mode: [0; super::tables::MAX_SBG_NOISE as usize],
            add_harmonic: [false; MAX_SBG_MASTER],
            fic_used_in_sfb: [false; MAX_SBG_MASTER],
            tic_used_in_slot: [false; MAX_ASPX_TIMESLOTS],
            num_sbg_noise: 0,
            num_sbg_sig_highres: 0,
            num_timeslots: 0,
        }
    }

    /// `aspx_tna_mode[n]`，2 位，见表 131。
    #[must_use]
    pub fn tna_mode(&self, sbg: usize) -> Option<u8> {
        if sbg >= usize::from(self.num_sbg_noise) {
            return None;
        }
        self.tna_mode.get(sbg).copied()
    }

    /// `aspx_add_harmonic[n]`，见表 132。
    #[must_use]
    pub fn add_harmonic(&self, sbg: usize) -> Option<bool> {
        if sbg >= usize::from(self.num_sbg_sig_highres) {
            return None;
        }
        self.add_harmonic.get(sbg).copied()
    }

    /// 逐噪声子带组的 `aspx_tna_mode`，长度即 `num_sbg_noise`。
    ///
    /// 与逐元素的 [`Self::tna_mode`] 相比，切片把有效长度交给类型：`5.7.6.4.1.3`
    /// 用长度与频带表的噪声子带组数比对，递整条定长数组过去那条比对就永不触发。
    #[must_use]
    pub fn tna_mode_slice(&self) -> Option<&[u8]> {
        self.tna_mode.get(..usize::from(self.num_sbg_noise))
    }

    /// 逐高分辨率信号子带组的 `aspx_add_harmonic`，长度即 `num_sbg_sig_highres`。
    #[must_use]
    pub fn add_harmonic_slice(&self) -> Option<&[bool]> {
        self.add_harmonic
            .get(..usize::from(self.num_sbg_sig_highres))
    }

    /// 供其他模块的判据组装一份 HF 生成参数；生产路径只由 `aspx_hfgen_iwc_*()`
    /// 产出。两个计数取各自切片的长度，判据因此能造出与频带表不符的组数。
    #[cfg(test)]
    pub(crate) fn for_test(tna_mode: &[u8], add_harmonic: &[bool]) -> Self {
        let mut out = Self::empty();
        for (slot, value) in out.tna_mode.iter_mut().zip(tna_mode) {
            *slot = *value;
        }
        for (slot, value) in out.add_harmonic.iter_mut().zip(add_harmonic) {
            *slot = *value;
        }
        out.num_sbg_noise = u8::try_from(tna_mode.len()).unwrap_or(u8::MAX);
        out.num_sbg_sig_highres = u8::try_from(add_harmonic.len()).unwrap_or(u8::MAX);
        out
    }

    /// `aspx_fic_used_in_sfb[n]`，见表 134。
    #[must_use]
    pub fn fic_used_in_sfb(&self, sbg: usize) -> Option<bool> {
        if sbg >= usize::from(self.num_sbg_sig_highres) {
            return None;
        }
        self.fic_used_in_sfb.get(sbg).copied()
    }

    /// `aspx_tic_used_in_slot[n]`，见表 136。
    #[must_use]
    pub fn tic_used_in_slot(&self, slot: usize) -> Option<bool> {
        if slot >= usize::from(self.num_timeslots) {
            return None;
        }
        self.tic_used_in_slot.get(slot).copied()
    }
}

/// 一个声道的 A-SPX 标度因子。
///
/// 值是 Huffman 符号下标减去 `cb_off` 后的**差值或绝对值**，未做增量还原，
/// 也未反量化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspxEnvelopes {
    sig: [[i16; MAX_SBG_PER_ENVELOPE]; MAX_ATSG_SIG],
    sig_sbg: [u8; MAX_ATSG_SIG],
    num_env: u8,
    noise: [[i16; super::tables::MAX_SBG_NOISE as usize]; MAX_ATSG_NOISE],
    noise_sbg: u8,
    num_noise: u8,
}

impl AspxEnvelopes {
    const fn empty() -> Self {
        Self {
            sig: [[0; MAX_SBG_PER_ENVELOPE]; MAX_ATSG_SIG],
            sig_sbg: [0; MAX_ATSG_SIG],
            num_env: 0,
            noise: [[0; super::tables::MAX_SBG_NOISE as usize]; MAX_ATSG_NOISE],
            noise_sbg: 0,
            num_noise: 0,
        }
    }

    /// 第 `env` 个信号包络的子带组数。
    ///
    /// 按 `Pseudocode 78`，高分辨率包络取 `num_sbg_sig_highres`，低分辨率
    /// 取 `num_sbg_sig_lowres`。
    #[must_use]
    pub fn sig_sbg_count(&self, env: usize) -> Option<u8> {
        if env >= usize::from(self.num_env) {
            return None;
        }
        self.sig_sbg.get(env).copied()
    }

    /// `aspx_data_sig[ch][env][sbg]`。
    #[must_use]
    pub fn sig(&self, env: usize, sbg: usize) -> Option<i16> {
        if sbg >= usize::from(self.sig_sbg_count(env)?) {
            return None;
        }
        self.sig.get(env)?.get(sbg).copied()
    }

    /// `aspx_data_noise[ch][env][sbg]`。
    #[must_use]
    pub fn noise(&self, env: usize, sbg: usize) -> Option<i16> {
        if env >= usize::from(self.num_noise) || sbg >= usize::from(self.noise_sbg) {
            return None;
        }
        self.noise.get(env)?.get(sbg).copied()
    }

    /// 噪声包络的子带组数，即 `num_sbg_noise`。
    #[must_use]
    pub const fn noise_sbg_count(&self) -> u8 {
        self.noise_sbg
    }

    /// 第 `env` 个信号包络的有效前缀，供 `5.7.6.3.4` 直接取用。
    ///
    /// 返回的是**解析时确定的**子带组数那一段，不是定长数组的全长。这不只是
    /// 省事：`5.7.6.3.4` 用 `data.len() < groups` 判断「解析出的组数够不够本
    /// 包络的频率分辨率」，若把整条定长数组递过去，那条检查就永远不会触发，
    /// 解析与频带表脱节将不再有任何一处报错。
    #[must_use]
    pub fn sig_slice(&self, env: usize) -> Option<&[i16]> {
        let groups = usize::from(self.sig_sbg_count(env)?);
        self.sig.get(env)?.get(..groups)
    }

    /// 第 `env` 个噪声包络的有效前缀，长度恒为 `num_sbg_noise`。
    #[must_use]
    pub fn noise_slice(&self, env: usize) -> Option<&[i16]> {
        if env >= usize::from(self.num_noise) {
            return None;
        }
        let groups = usize::from(self.noise_sbg);
        self.noise.get(env)?.get(..groups)
    }

    /// 供其他模块的判据组装一份差分符号；生产路径只由 `aspx_ec_data()` 产出。
    ///
    /// 逐包络的子带组数取各条切片自身的长度，因此判据可以造出「本包络声明高分
    /// 辨率、给的却是低分辨率的组数」——那正是 `5.7.6.3.4` 要报错的一种失配。
    /// 噪声侧只有一个组数，取首条切片的长度。
    #[cfg(test)]
    pub(crate) fn for_test(sig: &[&[i16]], noise: &[&[i16]]) -> Self {
        let mut out = Self::empty();
        out.num_env = u8::try_from(sig.len()).unwrap_or(u8::MAX);
        for (index, data) in sig.iter().enumerate().take(MAX_ATSG_SIG) {
            let Some(row) = out.sig.get_mut(index) else {
                continue;
            };
            for (slot, value) in row.iter_mut().zip(data.iter()) {
                *slot = *value;
            }
            if let Some(count) = out.sig_sbg.get_mut(index) {
                *count = u8::try_from(data.len()).unwrap_or(u8::MAX);
            }
        }
        out.num_noise = u8::try_from(noise.len()).unwrap_or(u8::MAX);
        out.noise_sbg = noise
            .first()
            .map_or(0, |data| u8::try_from(data.len()).unwrap_or(u8::MAX));
        for (index, data) in noise.iter().enumerate().take(MAX_ATSG_NOISE) {
            let Some(row) = out.noise.get_mut(index) else {
                continue;
            };
            for (slot, value) in row.iter_mut().zip(data.iter()) {
                *slot = *value;
            }
        }
        out
    }
}

/// 一个 `aspx_data_1ch()` 或 `aspx_data_2ch()` 元素的解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AspxData {
    /// 本元素的声道数：1 或 2。
    pub channels: u8,
    /// `aspx_balance`，仅双声道元素有值，见表 125。
    pub balance: Option<bool>,
    /// 由配置与交叉偏移推出的子带组表。
    pub bands: AspxBandTables,
    framing: [AspxChannelFraming; 2],
    hfgen: [AspxHfGen; 2],
    envelopes: [AspxEnvelopes; 2],
}

impl AspxData {
    /// 一个未填充的实例，供调用方预留工作区。
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            channels: 0,
            balance: None,
            bands: AspxBandTables::empty(),
            framing: [AspxChannelFraming::empty(); 2],
            hfgen: [AspxHfGen::empty(); 2],
            envelopes: [AspxEnvelopes::empty(); 2],
        }
    }

    /// 第 `ch` 个声道的成帧结果。
    #[must_use]
    pub fn framing(&self, ch: usize) -> Option<&AspxChannelFraming> {
        if ch >= usize::from(self.channels) {
            return None;
        }
        self.framing.get(ch)
    }

    /// 第 `ch` 个声道的高频生成与交织编码标志。
    #[must_use]
    pub fn hfgen(&self, ch: usize) -> Option<&AspxHfGen> {
        if ch >= usize::from(self.channels) {
            return None;
        }
        self.hfgen.get(ch)
    }

    /// 第 `ch` 个声道的标度因子。
    #[must_use]
    pub fn envelopes(&self, ch: usize) -> Option<&AspxEnvelopes> {
        if ch >= usize::from(self.channels) {
            return None;
        }
        self.envelopes.get(ch)
    }

    /// 供其他模块的判据组装一个已解析的元素。
    #[cfg(test)]
    pub(crate) fn for_test(
        channels: u8,
        bands: AspxBandTables,
        framing: [AspxChannelFraming; 2],
        hfgen: [AspxHfGen; 2],
        envelopes: [AspxEnvelopes; 2],
    ) -> Self {
        Self::for_test_balanced(channels, None, bands, framing, hfgen, envelopes)
    }

    /// 同上，但可指定 `aspx_balance`。
    #[cfg(test)]
    pub(crate) fn for_test_balanced(
        channels: u8,
        balance: Option<bool>,
        bands: AspxBandTables,
        framing: [AspxChannelFraming; 2],
        hfgen: [AspxHfGen; 2],
        envelopes: [AspxEnvelopes; 2],
    ) -> Self {
        Self {
            channels,
            balance,
            bands,
            framing,
            hfgen,
            envelopes,
        }
    }

    /// 解析 `aspx_data_1ch()`，见 `4.2.12.2` 表 51。
    ///
    /// # Errors
    ///
    /// 见 [`AspxError`]。
    pub fn parse_1ch(
        reader: &mut BitReader<'_>,
        config: &AspxConfig,
        state: &mut AspxState,
        frame_len_base: u16,
        b_iframe: bool,
    ) -> Result<Self, AspxError> {
        Self::parse(reader, config, state, frame_len_base, b_iframe, false)
    }

    /// 解析 `aspx_data_2ch()`，见 `4.2.12.3` 表 52。
    ///
    /// `aspx_balance` 为真时第二个声道不传输 `aspx_framing()`，两声道共用第
    /// 一个声道的成帧。
    ///
    /// # Errors
    ///
    /// 见 [`AspxError`]。
    pub fn parse_2ch(
        reader: &mut BitReader<'_>,
        config: &AspxConfig,
        state: &mut AspxState,
        frame_len_base: u16,
        b_iframe: bool,
    ) -> Result<Self, AspxError> {
        Self::parse(reader, config, state, frame_len_base, b_iframe, true)
    }

    fn parse(
        reader: &mut BitReader<'_>,
        config: &AspxConfig,
        state: &mut AspxState,
        frame_len_base: u16,
        b_iframe: bool,
        two_channels: bool,
    ) -> Result<Self, AspxError> {
        let slots = num_aspx_timeslots(frame_len_base)
            .ok_or(AspxError::UnsupportedFrameLenBase { frame_len_base })?;

        // 交叉子带偏移只在 I 帧传输，其余帧沿用。先保存在局部变量中，整份
        // 元素解析成功后再提交，避免损坏的 I 帧污染后续帧的跨帧状态。
        let xover = if b_iframe {
            read_u8(reader, 3)?
        } else {
            state.xover
        };
        let bands = AspxBandTables::derive(
            config.master_freq_scale,
            config.start_freq,
            config.stop_freq,
            config.noise_sbg,
            xover,
        )?;

        let mut out = Self::empty();
        out.bands = bands;
        out.channels = if two_channels { 2 } else { 1 };

        let previous = state.previous_stop_pos.first().copied().unwrap_or(0);
        let first = parse_framing(reader, config, slots, b_iframe, previous)?;
        if let Some(slot) = out.framing.first_mut() {
            *slot = first;
        }

        let balance = if two_channels {
            Some(reader.read_flag()?)
        } else {
            None
        };
        out.balance = balance;

        if two_channels {
            let second = if balance == Some(false) {
                let prev = state.previous_stop_pos.get(1).copied().unwrap_or(0);
                parse_framing(reader, config, slots, b_iframe, prev)?
            } else {
                // aspx_balance 为真：第二声道共用第一声道的成帧。
                first
            };
            if let Some(slot) = out.framing.get_mut(1) {
                *slot = second;
            }
        }

        for ch in 0..usize::from(out.channels) {
            let framing = out.framing.get(ch).copied().unwrap_or(first);
            let mut updated = framing;
            parse_delta_dir(reader, &mut updated)?;
            if let Some(slot) = out.framing.get_mut(ch) {
                *slot = updated;
            }
        }

        if two_channels {
            parse_hfgen_2ch(reader, &mut out, balance == Some(true), slots)?;
        } else {
            parse_hfgen_1ch(reader, &mut out, slots)?;
        }

        // 表 51/52：单声道恒为 LEVEL；双声道第一路 LEVEL，第二路随 balance。
        // 表 52 先传输两路信号包络，再传输两路噪声包络，故类型循环必须在外。
        for kind in [EnvelopeKind::Signal, EnvelopeKind::Noise] {
            for ch in 0..usize::from(out.channels) {
                let stereo = if ch == 1 && balance == Some(true) {
                    StereoMode::Balance
                } else {
                    StereoMode::Level
                };
                let framing = out.framing.get(ch).copied().unwrap_or(first);
                let Some(envelopes) = out.envelopes.get_mut(ch) else {
                    continue;
                };
                parse_ec_data(reader, &out.bands, &framing, kind, stereo, envelopes)?;
            }
        }

        state.xover = xover;
        for ch in 0..usize::from(out.channels) {
            let stop = out
                .framing
                .get(ch)
                .map_or(0, |framing| framing.interval.stop_pos());
            if let Some(slot) = state.previous_stop_pos.get_mut(ch) {
                *slot = stop;
            }
        }
        Ok(out)
    }
}

/// `aspx_framing(ch)`，见 `4.2.12.4` 表 53。
fn parse_framing(
    reader: &mut BitReader<'_>,
    config: &AspxConfig,
    slots: u8,
    b_iframe: bool,
    previous_stop_pos: i16,
) -> Result<AspxChannelFraming, AspxError> {
    let int_class = parse_interval_class(reader)?;
    // 表 53 注 1：时隙数不超过 8 时，相对边界相关字段退化为 1 位。
    let rel_bits = if slots > 8 { 2 } else { 1 };

    let mut params = AspxIntervalParams::fixfix(1);
    params.int_class = int_class;

    match int_class {
        IntervalClass::FixFix => {
            let env_bits = u32::from(config.num_env_bits_fixfix).saturating_add(1);
            let tmp = read_u8(reader, env_bits)?;
            if tmp >= 3 {
                return Err(AspxError::EightEnvelopesProhibited);
            }
            params.num_env = 1u8.checked_shl(u32::from(tmp)).unwrap_or(1);
            if config.freq_res_mode == 0 {
                // FIXFIX 只传输第 0 项，其余由 Pseudocode 76 复制。
                let value = reader.read_flag()?;
                if let Some(slot) = params.freq_res.first_mut() {
                    *slot = value;
                }
            }
        }
        IntervalClass::FixVar => {
            params.var_bord_right = Some(read_u8(reader, 2)?);
            params.num_rel_right = read_u8(reader, rel_bits)?;
            read_relative_borders(
                reader,
                rel_bits,
                params.num_rel_right,
                &mut params.rel_bord_right,
            )?;
        }
        IntervalClass::VarFix => {
            if b_iframe {
                params.var_bord_left = Some(read_u8(reader, 2)?);
            }
            params.num_rel_left = read_u8(reader, rel_bits)?;
            read_relative_borders(
                reader,
                rel_bits,
                params.num_rel_left,
                &mut params.rel_bord_left,
            )?;
        }
        IntervalClass::VarVar => {
            if b_iframe {
                params.var_bord_left = Some(read_u8(reader, 2)?);
            }
            params.num_rel_left = read_u8(reader, rel_bits)?;
            read_relative_borders(
                reader,
                rel_bits,
                params.num_rel_left,
                &mut params.rel_bord_left,
            )?;
            params.var_bord_right = Some(read_u8(reader, 2)?);
            params.num_rel_right = read_u8(reader, rel_bits)?;
            read_relative_borders(
                reader,
                rel_bits,
                params.num_rel_right,
                &mut params.rel_bord_right,
            )?;
        }
    }

    if int_class != IntervalClass::FixFix {
        params.num_env = params
            .num_rel_left
            .saturating_add(params.num_rel_right)
            .saturating_add(1);
        let bits = tsg_ptr_bits(params.num_env);
        let raw = read_u8(reader, bits)?;
        params.tsg_ptr = i8::try_from(i16::from(raw).saturating_sub(1)).unwrap_or(-1);
        if config.freq_res_mode == 0 {
            for env in 0..usize::from(params.num_env) {
                let value = reader.read_flag()?;
                if let Some(slot) = params.freq_res.get_mut(env) {
                    *slot = value;
                }
            }
        }
    }

    params.num_noise = if params.num_env > 1 { 2 } else { 1 };

    let interval = AspxInterval::derive(
        &params,
        slots,
        config.freq_res_mode,
        b_iframe,
        previous_stop_pos,
    )?;

    // 表 51/52：FIXFIX 且单包络时量化步长强制为 1,5 dB。
    let qmode_env =
        config.quant_mode_env && !(int_class == IntervalClass::FixFix && params.num_env == 1);

    Ok(AspxChannelFraming {
        params,
        interval,
        qmode_env,
        sig_delta_dir: [false; MAX_ATSG_SIG],
        noise_delta_dir: [false; MAX_ATSG_NOISE],
    })
}

/// 表 126 的变长前缀码：`0`、`10`、`110`、`111`。
fn parse_interval_class(reader: &mut BitReader<'_>) -> Result<IntervalClass, AspxError> {
    if !reader.read_flag()? {
        return Ok(IntervalClass::FixFix);
    }
    if !reader.read_flag()? {
        return Ok(IntervalClass::FixVar);
    }
    if reader.read_flag()? {
        Ok(IntervalClass::VarVar)
    } else {
        Ok(IntervalClass::VarFix)
    }
}

/// `aspx_rel_bord_*[rel] = 2 * tmp + 2`。
fn read_relative_borders(
    reader: &mut BitReader<'_>,
    rel_bits: u32,
    count: u8,
    out: &mut [u8; MAX_REL_BORDERS],
) -> Result<(), AspxError> {
    for rel in 0..usize::from(count) {
        let tmp = read_u8(reader, rel_bits)?;
        let value = tmp.saturating_mul(2).saturating_add(2);
        if let Some(slot) = out.get_mut(rel) {
            *slot = value;
        }
    }
    Ok(())
}

/// `ptr_bits = ceil(log2(aspx_num_env + 2))`，见表 53 注 2。
fn tsg_ptr_bits(num_env: u8) -> u32 {
    let value = u32::from(num_env).saturating_add(2);
    value.next_power_of_two().ilog2()
}

/// `aspx_delta_dir(ch)`，见 `4.2.12.5` 表 54。
fn parse_delta_dir(
    reader: &mut BitReader<'_>,
    framing: &mut AspxChannelFraming,
) -> Result<(), AspxError> {
    for env in 0..usize::from(framing.params.num_env) {
        let value = reader.read_flag()?;
        if let Some(slot) = framing.sig_delta_dir.get_mut(env) {
            *slot = value;
        }
    }
    for env in 0..usize::from(framing.params.num_noise) {
        let value = reader.read_flag()?;
        if let Some(slot) = framing.noise_delta_dir.get_mut(env) {
            *slot = value;
        }
    }
    Ok(())
}

/// `aspx_hfgen_iwc_1ch()`，见 `4.2.12.6` 表 55。
fn parse_hfgen_1ch(
    reader: &mut BitReader<'_>,
    data: &mut AspxData,
    slots: u8,
) -> Result<(), AspxError> {
    let noise_sbg = data.bands.num_sbg_noise();
    let highres = data.bands.num_sbg_sig_highres();
    let mut out = AspxHfGen::empty();
    out.num_sbg_noise = noise_sbg;
    out.num_sbg_sig_highres = highres;
    out.num_timeslots = slots;

    for sbg in 0..usize::from(noise_sbg) {
        let mode = read_u8(reader, 2)?;
        if let Some(slot) = out.tna_mode.get_mut(sbg) {
            *slot = mode;
        }
    }
    if reader.read_flag()? {
        read_flags(reader, highres, &mut out.add_harmonic)?;
    }
    if reader.read_flag()? {
        read_flags(reader, highres, &mut out.fic_used_in_sfb)?;
    }
    if reader.read_flag()? {
        read_slot_flags(reader, slots, &mut out.tic_used_in_slot)?;
    }

    if let Some(slot) = data.hfgen.first_mut() {
        *slot = out;
    }
    Ok(())
}

/// `aspx_hfgen_iwc_2ch(aspx_balance)`，见 `4.2.12.7` 表 56。
fn parse_hfgen_2ch(
    reader: &mut BitReader<'_>,
    data: &mut AspxData,
    balance: bool,
    slots: u8,
) -> Result<(), AspxError> {
    let noise_sbg = data.bands.num_sbg_noise();
    let highres = data.bands.num_sbg_sig_highres();
    let mut left = AspxHfGen::empty();
    let mut right = AspxHfGen::empty();
    for target in [&mut left, &mut right] {
        target.num_sbg_noise = noise_sbg;
        target.num_sbg_sig_highres = highres;
        target.num_timeslots = slots;
    }

    for sbg in 0..usize::from(noise_sbg) {
        let mode = read_u8(reader, 2)?;
        if let Some(slot) = left.tna_mode.get_mut(sbg) {
            *slot = mode;
        }
    }
    if balance {
        // aspx_balance 为真时右声道沿用左声道的 tna_mode，不再传输。
        right.tna_mode = left.tna_mode;
    } else {
        for sbg in 0..usize::from(noise_sbg) {
            let mode = read_u8(reader, 2)?;
            if let Some(slot) = right.tna_mode.get_mut(sbg) {
                *slot = mode;
            }
        }
    }

    // 两个 add_harmonic 各有独立的存在标志。
    if reader.read_flag()? {
        read_flags(reader, highres, &mut left.add_harmonic)?;
    }
    if reader.read_flag()? {
        read_flags(reader, highres, &mut right.add_harmonic)?;
    }

    if reader.read_flag()? {
        if reader.read_flag()? {
            read_flags(reader, highres, &mut left.fic_used_in_sfb)?;
        }
        if reader.read_flag()? {
            read_flags(reader, highres, &mut right.fic_used_in_sfb)?;
        }
    }

    if reader.read_flag()? {
        let tic_copy = reader.read_flag()?;
        let (tic_left, tic_right) = if tic_copy {
            (false, false)
        } else {
            (reader.read_flag()?, reader.read_flag()?)
        };
        if tic_copy || tic_left {
            read_slot_flags(reader, slots, &mut left.tic_used_in_slot)?;
        }
        if tic_right {
            read_slot_flags(reader, slots, &mut right.tic_used_in_slot)?;
        }
        if tic_copy {
            right.tic_used_in_slot = left.tic_used_in_slot;
        }
    }

    if let Some(slot) = data.hfgen.first_mut() {
        *slot = left;
    }
    if let Some(slot) = data.hfgen.get_mut(1) {
        *slot = right;
    }
    Ok(())
}

fn read_flags(
    reader: &mut BitReader<'_>,
    count: u8,
    out: &mut [bool; MAX_SBG_MASTER],
) -> Result<(), AspxError> {
    for slot in out.iter_mut().take(usize::from(count)) {
        *slot = reader.read_flag()?;
    }
    Ok(())
}

fn read_slot_flags(
    reader: &mut BitReader<'_>,
    count: u8,
    out: &mut [bool; MAX_ASPX_TIMESLOTS],
) -> Result<(), AspxError> {
    for slot in out.iter_mut().take(usize::from(count)) {
        *slot = reader.read_flag()?;
    }
    Ok(())
}

/// 一次 `aspx_ec_data()` 调用，见 `4.2.12.8` 表 57。
fn parse_ec_data(
    reader: &mut BitReader<'_>,
    bands: &AspxBandTables,
    framing: &AspxChannelFraming,
    kind: EnvelopeKind,
    stereo: StereoMode,
    out: &mut AspxEnvelopes,
) -> Result<(), AspxError> {
    match kind {
        EnvelopeKind::Signal => {
            out.num_env = framing.params.num_env;
            // 分辨率逐包络决定取高分辨率还是低分辨率的子带组数。
            for env in 0..usize::from(framing.params.num_env) {
                let high = framing.interval.freq_res(env).unwrap_or(false);
                let num_sbg = if high {
                    bands.num_sbg_sig_highres()
                } else {
                    bands.num_sbg_sig_lowres()
                };
                if let Some(slot) = out.sig_sbg.get_mut(env) {
                    *slot = num_sbg;
                }
                let direction = framing.sig_delta_dir(env).unwrap_or(false);
                let Some(row) = out.sig.get_mut(env) else {
                    continue;
                };
                parse_huff_data(
                    reader,
                    kind,
                    stereo,
                    framing.qmode_env,
                    direction,
                    num_sbg,
                    row,
                )?;
            }
        }
        EnvelopeKind::Noise => {
            out.num_noise = framing.params.num_noise;
            out.noise_sbg = bands.num_sbg_noise();
            // 噪声包络始终取 num_sbg_noise，且无量化步长维度。
            for env in 0..usize::from(framing.params.num_noise) {
                let direction = framing.noise_delta_dir(env).unwrap_or(false);
                let num_sbg = out.noise_sbg;
                let Some(row) = out.noise.get_mut(env) else {
                    continue;
                };
                parse_huff_data(reader, kind, stereo, false, direction, num_sbg, row)?;
            }
        }
    }
    Ok(())
}

/// `aspx_huff_data()`，见 `4.2.12.9` 表 58。
///
/// `direction` 为假走频率方向：首项用 `F0` 码本编绝对值，其余用 `DF` 编
/// 差值；为真走时间方向，全部用 `DT` 编差值。
fn parse_huff_data(
    reader: &mut BitReader<'_>,
    kind: EnvelopeKind,
    stereo: StereoMode,
    quant_mode: bool,
    direction: bool,
    num_sbg: u8,
    out: &mut [i16],
) -> Result<(), AspxError> {
    if direction {
        let book = get_aspx_hcb(kind, stereo, quant_mode, HcbType::Dt);
        let table = table_for(book);
        let offset = cb_off(book);
        for sbg in 0..usize::from(num_sbg) {
            let value = decode_one(reader, table, offset)?;
            if let Some(slot) = out.get_mut(sbg) {
                *slot = value;
            }
        }
        return Ok(());
    }

    let first = get_aspx_hcb(kind, stereo, quant_mode, HcbType::F0);
    let value = decode_one(reader, table_for(first), cb_off(first))?;
    if let Some(slot) = out.first_mut() {
        *slot = value;
    }
    let rest = get_aspx_hcb(kind, stereo, quant_mode, HcbType::Df);
    let table = table_for(rest);
    let offset = cb_off(rest);
    for sbg in 1..usize::from(num_sbg) {
        let value = decode_one(reader, table, offset)?;
        if let Some(slot) = out.get_mut(sbg) {
            *slot = value;
        }
    }
    Ok(())
}

fn decode_one(
    reader: &mut BitReader<'_>,
    table: &'static crate::huffman::HuffmanTable,
    offset: i16,
) -> Result<i16, AspxError> {
    let symbol = table.decode(reader)?;
    Ok(i16::try_from(symbol)
        .unwrap_or(i16::MAX)
        .saturating_sub(offset))
}

fn read_u8(reader: &mut BitReader<'_>, bits: u32) -> Result<u8, AspxError> {
    let value = reader.read_bits(bits)?;
    Ok(u8::try_from(value).unwrap_or(u8::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个可按位写入的固定容量缓冲，供构造测试码流。
    struct BitBuf {
        bytes: [u8; 512],
        len: usize,
    }

    impl BitBuf {
        const fn new() -> Self {
            Self {
                bytes: [0; 512],
                len: 0,
            }
        }

        fn push(&mut self, bit: bool) {
            let index = self.len / 8;
            let shift = 7usize.saturating_sub(self.len % 8);
            if bit && let Some(slot) = self.bytes.get_mut(index) {
                *slot |= 1u8
                    .checked_shl(u32::try_from(shift).unwrap_or(0))
                    .unwrap_or(0);
            }
            self.len = self.len.saturating_add(1);
        }

        fn push_bits(&mut self, value: u32, width: u32) {
            for bit in (0..width).rev() {
                self.push((value >> bit) & 1 == 1);
            }
        }

        /// 写入一份 `aspx_config()`：15 位。
        fn push_config(&mut self, config: &AspxConfig) {
            self.push(config.quant_mode_env);
            self.push_bits(u32::from(config.start_freq), 3);
            self.push_bits(u32::from(config.stop_freq), 2);
            self.push(config.master_freq_scale);
            self.push(config.interpolation);
            self.push(config.preflat);
            self.push(config.limiter);
            self.push_bits(u32::from(config.noise_sbg), 2);
            self.push(config.num_env_bits_fixfix);
            self.push_bits(u32::from(config.freq_res_mode), 2);
        }
    }

    /// 高分辨率模板、起止皆 0、噪声组参数 1，`freq_res_mode` 取 3（恒高）。
    fn base_config() -> AspxConfig {
        AspxConfig {
            quant_mode_env: false,
            start_freq: 0,
            stop_freq: 0,
            master_freq_scale: true,
            interpolation: false,
            preflat: false,
            limiter: false,
            noise_sbg: 1,
            num_env_bits_fixfix: false,
            freq_res_mode: 3,
        }
    }

    fn state_with(_config: AspxConfig) -> AspxState {
        let mut state = AspxState::new();
        state.reset_stop_pos(16);
        state
    }

    /// `aspx_config()` 恰好消耗 15 位，且各字段按表 50 的次序落位。
    #[test]
    fn config_consumes_exactly_fifteen_bits() {
        let config = AspxConfig {
            quant_mode_env: true,
            start_freq: 5,
            stop_freq: 2,
            master_freq_scale: true,
            interpolation: true,
            preflat: false,
            limiter: true,
            noise_sbg: 3,
            num_env_bits_fixfix: true,
            freq_res_mode: 2,
        };
        let mut buf = BitBuf::new();
        buf.push_config(&config);
        assert_eq!(buf.len, 15);

        let mut reader = BitReader::new(&buf.bytes);
        let parsed = AspxConfig::parse(&mut reader).expect("应能解析");
        assert_eq!(parsed, config);
        assert_eq!(reader.bit_position(), 15);
    }

    /// 表 126 的四个码字长度分别为 1、2、3、3。
    #[test]
    fn interval_class_is_a_prefix_code() {
        let cases: [(&[bool], IntervalClass, u64); 4] = [
            (&[false], IntervalClass::FixFix, 1),
            (&[true, false], IntervalClass::FixVar, 2),
            (&[true, true, false], IntervalClass::VarFix, 3),
            (&[true, true, true], IntervalClass::VarVar, 3),
        ];
        for (bits, expected, width) in cases {
            let mut buf = BitBuf::new();
            for &bit in bits {
                buf.push(bit);
            }
            let mut reader = BitReader::new(&buf.bytes);
            assert_eq!(parse_interval_class(&mut reader), Ok(expected));
            assert_eq!(reader.bit_position(), width, "{expected:?} 的码字长度");
        }
    }

    /// `ptr_bits = ceil(log2(num_env + 2))`，对全部合法包络数逐一核对。
    #[test]
    fn tsg_pointer_width_matches_the_ceiling_logarithm() {
        extern crate std;
        for num_env in 1u8..=5 {
            let exact = (f64::from(num_env) + 2.0).log2().ceil();
            let exact = u32::try_from(exact as i64).unwrap_or(0);
            assert_eq!(
                tsg_ptr_bits(num_env),
                exact,
                "num_env {num_env} 的 ptr_bits"
            );
        }
    }

    /// FIXFIX 的八包络为语法所禁止，拒绝时不得提交已读取的 I 帧状态。
    #[test]
    fn rejects_eight_envelopes_without_committing_state() {
        let mut config = base_config();
        config.num_env_bits_fixfix = true; // tmp_num_env 用 2 位
        let mut state = state_with(config);
        let before = state;

        let mut buf = BitBuf::new();
        buf.push_bits(5, 3); // xover：失败时不得提交
        buf.push(false); // aspx_int_class = FIXFIX
        buf.push_bits(3, 2); // tmp_num_env = 3 → 8 个包络
        let mut reader = BitReader::new(&buf.bytes);

        assert_eq!(
            AspxData::parse_1ch(&mut reader, &config, &mut state, 2048, true),
            Err(AspxError::EightEnvelopesProhibited)
        );
        assert_eq!(state, before, "失败的 I 帧不得修改跨帧状态");
    }

    /// 不受支持的帧长基准在读取任何比特前拒绝。

    #[test]
    fn rejects_unsupported_frame_len_base_before_reading() {
        let config = base_config();
        let mut state = state_with(config);
        let buf = BitBuf::new();
        let mut reader = BitReader::new(&buf.bytes);
        assert_eq!(
            AspxData::parse_1ch(&mut reader, &config, &mut state, 1000, true),
            Err(AspxError::UnsupportedFrameLenBase {
                frame_len_base: 1000
            })
        );
        assert_eq!(reader.bit_position(), 0);
    }

    /// `aspx_xover_subband_offset` 只在 I 帧传输，非 I 帧沿用上一次的值。
    #[test]
    fn crossover_offset_is_only_transmitted_on_iframes() {
        let config = base_config();
        let mut state = state_with(config);

        let mut buf = BitBuf::new();
        buf.push_bits(3, 3); // xover = 3
        buf.push(false); // FIXFIX
        buf.push_bits(0, 1); // tmp_num_env = 0 → 1 个包络
        let mut reader = BitReader::new(&buf.bytes);
        let iframe = AspxData::parse_1ch(&mut reader, &config, &mut state, 2048, true);
        assert!(iframe.is_ok(), "I 帧应能解析：{iframe:?}");
        assert_eq!(state.xover(), 3);
        let after_iframe = reader.bit_position();

        // 非 I 帧不再读 3 位偏移，故同样的后续内容起点前移。
        let mut buf = BitBuf::new();
        buf.push(false); // FIXFIX
        buf.push_bits(0, 1);
        let mut reader = BitReader::new(&buf.bytes);
        let inter = AspxData::parse_1ch(&mut reader, &config, &mut state, 2048, false);
        assert!(inter.is_ok(), "非 I 帧应能解析：{inter:?}");
        assert_eq!(state.xover(), 3, "偏移应沿用");
        assert_eq!(
            reader.bit_position(),
            after_iframe.saturating_sub(3),
            "非 I 帧应恰好少读 3 位"
        );
    }

    /// 单声道元素的落点必须与构造长度相等。
    ///
    /// 这是本层的主判据：任一字段宽度或循环次数写错，落点立即偏移。
    #[test]
    fn single_channel_element_lands_exactly() {
        let config = base_config();
        let mut state = state_with(config);
        let Ok(bands) = AspxBandTables::derive(
            config.master_freq_scale,
            config.start_freq,
            config.stop_freq,
            config.noise_sbg,
            0,
        ) else {
            panic!("频带表应可推导");
        };
        let highres = bands.num_sbg_sig_highres();
        let noise_sbg = bands.num_sbg_noise();

        let mut buf = BitBuf::new();
        buf.push_bits(0, 3); // xover = 0
        buf.push(false); // FIXFIX
        buf.push_bits(0, 1); // tmp_num_env = 0 → 1 个包络
        // freq_res_mode 为 3，故不传输 aspx_freq_res。
        buf.push(false); // sig_delta_dir[0]：频率方向
        buf.push(false); // noise_delta_dir[0]：频率方向
        for _ in 0..noise_sbg {
            buf.push_bits(0, 2); // tna_mode
        }
        buf.push(false); // aspx_ah_present
        buf.push(false); // aspx_fic_present
        buf.push(false); // aspx_tic_present

        // 信号包络：F0 一个符号，其后 highres-1 个 DF 符号。
        let sig_f0 = get_aspx_hcb(EnvelopeKind::Signal, StereoMode::Level, false, HcbType::F0);
        let sig_df = get_aspx_hcb(EnvelopeKind::Signal, StereoMode::Level, false, HcbType::Df);
        push_symbol(&mut buf, table_for(sig_f0), 0);
        for _ in 1..highres {
            push_symbol(&mut buf, table_for(sig_df), 0);
        }
        // 噪声包络：F0 一个符号，其后 noise_sbg-1 个 DF 符号。
        let noise_f0 = get_aspx_hcb(EnvelopeKind::Noise, StereoMode::Level, false, HcbType::F0);
        let noise_df = get_aspx_hcb(EnvelopeKind::Noise, StereoMode::Level, false, HcbType::Df);
        push_symbol(&mut buf, table_for(noise_f0), 0);
        for _ in 1..noise_sbg {
            push_symbol(&mut buf, table_for(noise_df), 0);
        }

        let expected = buf.len;
        let mut reader = BitReader::new(&buf.bytes);
        let data =
            AspxData::parse_1ch(&mut reader, &config, &mut state, 2048, true).expect("应能解析");
        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );

        assert_eq!(data.channels, 1);
        assert_eq!(data.balance, None);
        let Some(envelopes) = data.envelopes(0) else {
            panic!("应有第 0 声道");
        };
        assert_eq!(envelopes.sig_sbg_count(0), Some(highres));
        assert_eq!(envelopes.noise_sbg_count(), noise_sbg);
        assert_eq!(data.envelopes(1), None, "单声道不得暴露第二路");
    }

    /// 逐包络的频率分辨率必须真正切换子带组数。
    ///
    /// FIXVAR 两包络、`aspx_freq_res_mode` 取 0，两个包络分别标为高、低分
    /// 辨率。若忽略 `atsg_freqres` 而一律取 `num_sbg_sig_highres`，第二个
    /// 包络会多读若干个 Huffman 符号，落点立即偏移——这是 `frames` 与本层
    /// 唯一的接合点，`freq_res_mode` 取 1 或 3 的用例覆盖不到。
    #[test]
    fn per_envelope_resolution_switches_the_subband_count() {
        let mut config = base_config();
        config.freq_res_mode = 0; // 逐包络传输 aspx_freq_res
        let mut state = state_with(config);
        let Ok(bands) = AspxBandTables::derive(true, 0, 0, config.noise_sbg, 0) else {
            panic!("频带表应可推导");
        };
        let highres = bands.num_sbg_sig_highres();
        let lowres = bands.num_sbg_sig_lowres();
        let noise_sbg = bands.num_sbg_noise();
        assert_ne!(highres, lowres, "两种分辨率的组数必须不同，否则判据无效");

        let mut buf = BitBuf::new();
        buf.push_bits(0, 3); // xover
        buf.push(true); // 前缀
        buf.push(false); // → FIXVAR
        buf.push_bits(0, 2); // var_bord_right = 0
        buf.push_bits(1, 2); // num_rel_right = 1 → num_env = 2
        buf.push_bits(0, 2); // rel_bord_right[0] → 2
        buf.push_bits(1, 2); // tsg_ptr 原始值 1 → 0
        buf.push(true); // freq_res[0] = 高分辨率
        buf.push(false); // freq_res[1] = 低分辨率
        buf.push(false); // sig_delta_dir[0]
        buf.push(false); // sig_delta_dir[1]
        buf.push(false); // noise_delta_dir[0]
        buf.push(false); // noise_delta_dir[1]
        for _ in 0..noise_sbg {
            buf.push_bits(0, 2); // tna_mode
        }
        buf.push(false); // ah_present
        buf.push(false); // fic_present
        buf.push(false); // tic_present

        let sig_f0 = get_aspx_hcb(EnvelopeKind::Signal, StereoMode::Level, false, HcbType::F0);
        let sig_df = get_aspx_hcb(EnvelopeKind::Signal, StereoMode::Level, false, HcbType::Df);
        for count in [highres, lowres] {
            push_symbol(&mut buf, table_for(sig_f0), 0);
            for _ in 1..count {
                push_symbol(&mut buf, table_for(sig_df), 0);
            }
        }
        let noise_f0 = get_aspx_hcb(EnvelopeKind::Noise, StereoMode::Level, false, HcbType::F0);
        let noise_df = get_aspx_hcb(EnvelopeKind::Noise, StereoMode::Level, false, HcbType::Df);
        for _ in 0..2 {
            push_symbol(&mut buf, table_for(noise_f0), 0);
            for _ in 1..noise_sbg {
                push_symbol(&mut buf, table_for(noise_df), 0);
            }
        }

        let expected = buf.len;
        let mut reader = BitReader::new(&buf.bytes);
        let data =
            AspxData::parse_1ch(&mut reader, &config, &mut state, 2048, true).expect("应能解析");
        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );

        let Some(envelopes) = data.envelopes(0) else {
            panic!("应有第 0 声道");
        };
        assert_eq!(
            envelopes.sig_sbg_count(0),
            Some(highres),
            "首个包络高分辨率"
        );
        assert_eq!(envelopes.sig_sbg_count(1), Some(lowres), "次个包络低分辨率");
        assert_eq!(envelopes.sig_sbg_count(2), None, "包络数之外不得暴露");
        assert_eq!(envelopes.sig(1, usize::from(lowres)), None, "越界不得暴露");
    }

    /// `aspx_freq_res_mode` 取 1 时全部包络走低分辨率。
    #[test]
    fn low_resolution_mode_uses_the_decimated_table() {
        let mut config = base_config();
        config.freq_res_mode = 1;
        let mut state = state_with(config);
        let Ok(bands) = AspxBandTables::derive(true, 0, 0, config.noise_sbg, 0) else {
            panic!("频带表应可推导");
        };
        let lowres = bands.num_sbg_sig_lowres();
        let noise_sbg = bands.num_sbg_noise();

        let mut buf = BitBuf::new();
        buf.push_bits(0, 3); // xover
        buf.push(false); // FIXFIX
        buf.push_bits(0, 1); // 1 个包络
        buf.push(false); // sig_delta_dir[0]
        buf.push(false); // noise_delta_dir[0]
        for _ in 0..noise_sbg {
            buf.push_bits(0, 2);
        }
        buf.push(false); // ah_present
        buf.push(false); // fic_present
        buf.push(false); // tic_present

        let sig_f0 = get_aspx_hcb(EnvelopeKind::Signal, StereoMode::Level, false, HcbType::F0);
        let sig_df = get_aspx_hcb(EnvelopeKind::Signal, StereoMode::Level, false, HcbType::Df);
        push_symbol(&mut buf, table_for(sig_f0), 0);
        for _ in 1..lowres {
            push_symbol(&mut buf, table_for(sig_df), 0);
        }
        let noise_f0 = get_aspx_hcb(EnvelopeKind::Noise, StereoMode::Level, false, HcbType::F0);
        let noise_df = get_aspx_hcb(EnvelopeKind::Noise, StereoMode::Level, false, HcbType::Df);
        push_symbol(&mut buf, table_for(noise_f0), 0);
        for _ in 1..noise_sbg {
            push_symbol(&mut buf, table_for(noise_df), 0);
        }

        let expected = buf.len;
        let mut reader = BitReader::new(&buf.bytes);
        let data =
            AspxData::parse_1ch(&mut reader, &config, &mut state, 2048, true).expect("应能解析");
        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );
        let Some(envelopes) = data.envelopes(0) else {
            panic!("应有第 0 声道");
        };
        assert_eq!(envelopes.sig_sbg_count(0), Some(lowres));
    }

    /// 构造一个只填了频带表的元素，供直接调用 `aspx_hfgen_iwc_*` 使用。
    fn hfgen_fixture(channels: u8) -> (AspxData, u8, u8) {
        let config = base_config();
        let Ok(bands) = AspxBandTables::derive(true, 0, 0, config.noise_sbg, 0) else {
            panic!("频带表应可推导");
        };
        let highres = bands.num_sbg_sig_highres();
        let noise_sbg = bands.num_sbg_noise();
        let mut data = AspxData::empty();
        data.bands = bands;
        data.channels = channels;
        (data, highres, noise_sbg)
    }

    /// `aspx_hfgen_iwc_2ch()` 的时间交织分支必须逐一按表 56 计费。
    ///
    /// `aspx_tic_copy` 为真时**不传输** `aspx_tic_left` 与 `aspx_tic_right`，
    /// 且右声道的标志由左声道复制而来。六种组合的长度各不相同，任一处多读
    /// 或少读都会改变落点。
    #[test]
    fn two_channel_time_interleaving_branches_are_billed_exactly() {
        // (present, copy, left, right, 除时隙标志外的 tic 比特数, 右路是否复制左路)
        let cases: [(bool, bool, bool, bool, usize, bool); 6] = [
            (false, false, false, false, 1, false),
            (true, true, false, false, 2, true),
            (true, false, true, false, 4, false),
            (true, false, false, true, 4, false),
            (true, false, true, true, 4, false),
            (true, false, false, false, 4, false),
        ];
        for (present, copy, left, right, base_bits, copied) in cases {
            let (mut data, highres, noise_sbg) = hfgen_fixture(2);
            let slots = 16u8;

            let mut buf = BitBuf::new();
            for _ in 0..noise_sbg.saturating_mul(2) {
                buf.push_bits(0, 2); // 两路各一份 tna_mode（balance 为假）
            }
            buf.push(false); // ah_left
            buf.push(false); // ah_right
            buf.push(false); // fic_present
            buf.push(present);
            if present {
                buf.push(copy);
                if !copy {
                    buf.push(left);
                    buf.push(right);
                }
                if copy || left {
                    for slot in 0..slots {
                        buf.push(slot % 2 == 0);
                    }
                }
                if right {
                    for slot in 0..slots {
                        buf.push(slot % 3 == 0);
                    }
                }
            }
            let expected = buf.len;

            let mut slot_reads = 0usize;
            if present && (copy || left) {
                slot_reads = slot_reads.saturating_add(usize::from(slots));
            }
            if present && right {
                slot_reads = slot_reads.saturating_add(usize::from(slots));
            }
            let predicted = usize::from(noise_sbg)
                .saturating_mul(4)
                .saturating_add(3)
                .saturating_add(base_bits)
                .saturating_add(slot_reads);
            assert_eq!(expected, predicted, "构造长度与表 56 的计费不符");

            let mut reader = BitReader::new(&buf.bytes);
            parse_hfgen_2ch(&mut reader, &mut data, false, slots).expect("应能解析");
            assert_eq!(
                reader.bit_position(),
                u64::try_from(expected).unwrap_or(0),
                "组合 present={present} copy={copy} left={left} right={right} 的落点"
            );

            let (Some(l), Some(r)) = (data.hfgen(0), data.hfgen(1)) else {
                panic!("双声道应有两份 hfgen");
            };
            if copied {
                for slot in 0..usize::from(slots) {
                    assert_eq!(
                        l.tic_used_in_slot(slot),
                        r.tic_used_in_slot(slot),
                        "aspx_tic_copy 为真时右路应复制左路"
                    );
                }
            }
            assert_eq!(l.add_harmonic(usize::from(highres)), None, "越界不得暴露");
        }
    }

    /// `aspx_balance` 为真时右声道不再传输 `aspx_tna_mode`，而是沿用左声道。
    #[test]
    fn balanced_pair_copies_the_tna_mode() {
        let (mut data, _, noise_sbg) = hfgen_fixture(2);
        assert!(noise_sbg >= 1);

        let mut buf = BitBuf::new();
        for sbg in 0..noise_sbg {
            buf.push_bits(u32::from(sbg % 4), 2); // 只有左声道一份
        }
        buf.push(false); // ah_left
        buf.push(false); // ah_right
        buf.push(false); // fic_present
        buf.push(false); // tic_present
        let expected = buf.len;

        let mut reader = BitReader::new(&buf.bytes);
        parse_hfgen_2ch(&mut reader, &mut data, true, 16).expect("应能解析");
        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "balance 为真时不得读第二份 tna_mode"
        );

        let (Some(left), Some(right)) = (data.hfgen(0), data.hfgen(1)) else {
            panic!("应有两份 hfgen");
        };
        for sbg in 0..usize::from(noise_sbg) {
            assert_eq!(
                left.tna_mode(sbg),
                right.tna_mode(sbg),
                "第 {sbg} 组的 tna_mode 应相同"
            );
        }
        assert_eq!(left.tna_mode(usize::from(noise_sbg)), None);
    }

    /// `aspx_fic_present` 之下还有左右两个独立的存在标志。
    #[test]
    fn frequency_interleaving_has_nested_presence_flags() {
        for (fic_left, fic_right) in [(false, false), (true, false), (false, true), (true, true)] {
            let (mut data, highres, noise_sbg) = hfgen_fixture(2);

            let mut buf = BitBuf::new();
            for _ in 0..noise_sbg.saturating_mul(2) {
                buf.push_bits(0, 2);
            }
            buf.push(false); // ah_left
            buf.push(false); // ah_right
            buf.push(true); // fic_present
            buf.push(fic_left);
            if fic_left {
                for sbg in 0..highres {
                    buf.push(sbg % 2 == 0);
                }
            }
            buf.push(fic_right);
            if fic_right {
                for sbg in 0..highres {
                    buf.push(sbg % 3 == 0);
                }
            }
            buf.push(false); // tic_present
            let expected = buf.len;

            let mut reader = BitReader::new(&buf.bytes);
            parse_hfgen_2ch(&mut reader, &mut data, false, 16).expect("应能解析");
            assert_eq!(
                reader.bit_position(),
                u64::try_from(expected).unwrap_or(0),
                "fic_left={fic_left} fic_right={fic_right} 的落点"
            );

            let (Some(left), Some(right)) = (data.hfgen(0), data.hfgen(1)) else {
                panic!("应有两份 hfgen");
            };
            assert_eq!(left.fic_used_in_sfb(0), Some(fic_left));
            assert_eq!(right.fic_used_in_sfb(0), Some(fic_right));
        }
    }

    /// 单声道的 `aspx_hfgen_iwc_1ch()` 三个存在标志各自独立计费。
    #[test]
    fn single_channel_hfgen_presence_flags_are_independent() {
        for (ah, fic, tic) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (true, true, true),
        ] {
            let (mut data, highres, noise_sbg) = hfgen_fixture(1);
            let slots = 16u8;

            let mut buf = BitBuf::new();
            for _ in 0..noise_sbg {
                buf.push_bits(0, 2);
            }
            buf.push(ah);
            if ah {
                for sbg in 0..highres {
                    buf.push(sbg % 2 == 0);
                }
            }
            buf.push(fic);
            if fic {
                for sbg in 0..highres {
                    buf.push(sbg % 3 == 0);
                }
            }
            buf.push(tic);
            if tic {
                for slot in 0..slots {
                    buf.push(slot % 2 == 0);
                }
            }
            let expected = buf.len;

            let mut reader = BitReader::new(&buf.bytes);
            parse_hfgen_1ch(&mut reader, &mut data, slots).expect("应能解析");
            assert_eq!(
                reader.bit_position(),
                u64::try_from(expected).unwrap_or(0),
                "ah={ah} fic={fic} tic={tic} 的落点"
            );

            let Some(hfgen) = data.hfgen(0) else {
                panic!("应有一份 hfgen");
            };
            assert_eq!(hfgen.add_harmonic(0), Some(ah));
            assert_eq!(hfgen.fic_used_in_sfb(0), Some(fic));
            assert_eq!(hfgen.tic_used_in_slot(0), Some(tic));
            assert_eq!(hfgen.tic_used_in_slot(usize::from(slots)), None);
        }
    }

    /// `aspx_balance` 为真时两路共用成帧，包络仍按表 52 的跨声道顺序传输。
    #[test]
    fn balanced_pair_shares_framing_and_follows_table_52_order() {
        let config = base_config();
        let mut state = state_with(config);
        let Ok(bands) = AspxBandTables::derive(true, 0, 0, config.noise_sbg, 0) else {
            panic!("频带表应可推导");
        };
        let highres = bands.num_sbg_sig_highres();
        let noise_sbg = bands.num_sbg_noise();

        let mut buf = BitBuf::new();
        buf.push_bits(0, 3); // xover
        buf.push(false); // 声道 0：FIXFIX
        buf.push_bits(0, 1); // 1 个包络
        buf.push(true); // aspx_balance = 1 → 不再有第二份 aspx_framing
        buf.push(false); // 声道 0 sig_delta_dir
        buf.push(false); // 声道 0 noise_delta_dir
        buf.push(false); // 声道 1 sig_delta_dir
        buf.push(false); // 声道 1 noise_delta_dir
        for _ in 0..noise_sbg {
            buf.push_bits(0, 2); // tna_mode[0]，balance 为真故不传第二份
        }
        buf.push(false); // aspx_ah_left
        buf.push(false); // aspx_ah_right
        buf.push(false); // aspx_fic_present
        buf.push(false); // aspx_tic_present

        // 表 52：先传输两路信号包络，再传输两路噪声包络。四段的首符号
        // 刻意各不相同，使按声道交错读取无法假通过。
        for (stereo, first_symbol) in [(StereoMode::Level, 1u16), (StereoMode::Balance, 2)] {
            let f0 = get_aspx_hcb(EnvelopeKind::Signal, stereo, false, HcbType::F0);
            let df = get_aspx_hcb(EnvelopeKind::Signal, stereo, false, HcbType::Df);
            push_symbol(&mut buf, table_for(f0), first_symbol);
            let zero_diff = u16::try_from(cb_off(df)).unwrap_or(0);
            for _ in 1..highres {
                push_symbol(&mut buf, table_for(df), zero_diff);
            }
        }
        for (stereo, first_symbol) in [(StereoMode::Level, 3u16), (StereoMode::Balance, 4)] {
            let nf0 = get_aspx_hcb(EnvelopeKind::Noise, stereo, false, HcbType::F0);
            let ndf = get_aspx_hcb(EnvelopeKind::Noise, stereo, false, HcbType::Df);
            push_symbol(&mut buf, table_for(nf0), first_symbol);
            let zero_diff = u16::try_from(cb_off(ndf)).unwrap_or(0);
            for _ in 1..noise_sbg {
                push_symbol(&mut buf, table_for(ndf), zero_diff);
            }
        }

        let expected = buf.len;
        let mut reader = BitReader::new(&buf.bytes);
        let data =
            AspxData::parse_2ch(&mut reader, &config, &mut state, 2048, true).expect("应能解析");
        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应相等"
        );
        assert_eq!(data.balance, Some(true));

        let (Some(first), Some(second)) = (data.framing(0), data.framing(1)) else {
            panic!("双声道应有两份成帧");
        };
        assert_eq!(
            first.params, second.params,
            "balance 为真时两路共用成帧参数"
        );
        assert_eq!(first.interval, second.interval);

        let (Some(left), Some(right)) = (data.envelopes(0), data.envelopes(1)) else {
            panic!("双声道应有两份包络数据");
        };
        assert_eq!(left.sig(0, 0), Some(1));
        assert_eq!(right.sig(0, 0), Some(2));
        assert_eq!(left.noise(0, 0), Some(3));
        assert_eq!(right.noise(0, 0), Some(4));
    }

    /// FIXFIX 且单包络时量化步长强制为 1,5 dB，即便配置写的是 3 dB。
    #[test]
    fn single_envelope_fixfix_forces_fine_quantization() {
        let mut config = base_config();
        config.quant_mode_env = true; // 3 dB
        let mut state = state_with(config);

        let mut buf = BitBuf::new();
        buf.push_bits(0, 3);
        buf.push(false); // FIXFIX
        buf.push_bits(0, 1); // 1 个包络
        let mut reader = BitReader::new(&buf.bytes);
        let framing = parse_framing(&mut reader, &config, 16, true, 16);
        let Ok(framing) = framing else {
            panic!("应能解析：{framing:?}");
        };
        let _ = &mut state;
        assert!(!framing.qmode_env, "单包络 FIXFIX 应回落到 1,5 dB");

        // 两个包络时不再强制。
        let mut buf = BitBuf::new();
        buf.push(false); // FIXFIX
        buf.push_bits(1, 1); // tmp_num_env = 1 → 2 个包络
        let mut reader = BitReader::new(&buf.bytes);
        let Ok(framing) = parse_framing(&mut reader, &config, 16, true, 16) else {
            panic!("应能解析");
        };
        assert!(framing.qmode_env, "两个包络应保留 3 dB");
    }

    /// 时隙数不超过 8 时，相对边界字段由 2 位退化为 1 位。
    ///
    /// 该分支只在 `frame_len_base` 为 512 与 384 时可达。
    #[test]
    fn relative_border_fields_narrow_for_short_frames() {
        let config = base_config();

        // frame_len_base 512 → 8 个时隙 → 1 位。
        let mut buf = BitBuf::new();
        buf.push(true); // 前缀
        buf.push(false); // → FIXVAR
        buf.push_bits(0, 2); // var_bord_right
        buf.push_bits(1, 1); // num_rel_right = 1（1 位）
        buf.push_bits(0, 1); // rel_bord_right[0] → 2*0+2 = 2
        buf.push_bits(0, 2); // tsg_ptr：num_env = 2 → ptr_bits = 2
        let short_len = buf.len;
        let mut reader = BitReader::new(&buf.bytes);
        let Ok(framing) = parse_framing(&mut reader, &config, 8, true, 8) else {
            panic!("短帧应能解析");
        };
        assert_eq!(reader.bit_position(), u64::try_from(short_len).unwrap_or(0));
        assert_eq!(framing.params.num_rel_right, 1);
        assert_eq!(framing.params.rel_bord_right.first(), Some(&2));

        // frame_len_base 2048 → 16 个时隙 → 同样的字段占 2 位。
        let mut buf = BitBuf::new();
        buf.push(true);
        buf.push(false); // FIXVAR
        buf.push_bits(0, 2); // var_bord_right
        buf.push_bits(1, 2); // num_rel_right = 1（2 位）
        buf.push_bits(0, 2); // rel_bord_right[0]
        buf.push_bits(0, 2); // tsg_ptr
        let long_len = buf.len;
        let mut reader = BitReader::new(&buf.bytes);
        let Ok(framing) = parse_framing(&mut reader, &config, 16, true, 16) else {
            panic!("长帧应能解析");
        };
        assert_eq!(reader.bit_position(), u64::try_from(long_len).unwrap_or(0));
        assert_eq!(framing.params.num_rel_right, 1);
        assert_eq!(
            long_len.saturating_sub(short_len),
            2,
            "两个 1 位字段各加宽一位"
        );
    }

    /// 按码本的 `_LEN`／`_CW` 表写入一个符号的码字。
    ///
    /// 取自 `build.rs` 生成的原始长度与码字数组，而非解码用的 trie；两者
    /// 是同一份 C 表的两种形态，故这条路径同时校验 trie 的构造。
    fn push_symbol(buf: &mut BitBuf, table: &crate::huffman::HuffmanTable, symbol: u16) {
        use crate::huffman::tables::ALL_CODEBOOKS;
        for &(name, candidate, lengths, codewords) in ALL_CODEBOOKS {
            if !core::ptr::eq(candidate, table) {
                continue;
            }
            let index = usize::from(symbol);
            let (Some(&width), Some(&codeword)) = (lengths.get(index), codewords.get(index)) else {
                panic!("{name} 没有第 {symbol} 个符号");
            };
            buf.push_bits(codeword, u32::from(width));
            return;
        }
        panic!("码本不在 ALL_CODEBOOKS 内");
    }
}
