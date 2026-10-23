//! A-JOC 的对话增强下混与床渲染信息。
//!
//! `TS103190-2:v1.3.1` 的 `6.2.3.5` `ajoc_dmx_de_data()` 与 `6.2.3.6`
//! `ajoc_bed_info()`，语义见 `6.3.6.6`–`6.3.6.7`。两者都由 `6.2.3.4` 的
//! `audio_data_ajoc()` 调用。
//!
//! 下混系数留在量化域：表 82 给出的是 `n/15`，本模块只保留分子 `n`，不做
//! 除法，故不引入浮点。

use crate::ajoc::MAX_AJOC_DMX_SIGNALS;
use core::fmt;
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

/// 可参与对话增强的上混对象数上限。
///
/// 与 [`macindecode_ac4_bitstream::oamd::MAX_OAMD_OBJECTS`] 取同一个值：上混信号数本身没有位宽
/// 上界，此处设一个覆盖实际编码链的容量，超出即报错而非静默截断。
pub const MAX_DE_OBJECTS: usize = 32;

/// 表 82 `de_dlg_dmx_coeff` 的分母。系数即 `coeff_idx / 15`。
pub const DE_COEFF_DENOMINATOR: u8 = 15;

/// 对话增强数据解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AjocDeError {
    /// 读取比特时越过了数据末尾。
    Read(ReadError),
    /// 上混对象数超过 [`MAX_DE_OBJECTS`]。
    UmxSignalsOutOfRange {
        /// 传入的值。
        num_umx_signals: u32,
        /// 容量上界。
        limit: usize,
    },
    /// 下混信号数超过 [`MAX_AJOC_DMX_SIGNALS`]。
    DmxSignalsOutOfRange {
        /// 传入的值。
        num_dmx_signals: u8,
        /// 容量上界。
        limit: usize,
    },
    /// 当前帧未传输 `de_main_dlg_flag[]`，且没有可沿用的历史配置。
    MissingConfiguration,
    /// 非 I 帧沿用上一帧的 `de_main_dlg_flag[]`，但两帧的对象数不同。
    ///
    /// `6.3.6.6.3` 的 `dlg_obj()` 遍历全部上混对象，对象数变化时旧标志无法
    /// 对齐，`num_dlg_obj` 也就无从沿用。
    ObjectCountChanged {
        /// 上一帧记录的对象数。
        previous: u32,
        /// 本帧的对象数。
        current: u32,
    },
}

impl fmt::Display for AjocDeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AjocDeError::Read(error) => write!(f, "{error}"),
            AjocDeError::UmxSignalsOutOfRange {
                num_umx_signals,
                limit,
            } => write!(
                f,
                "Upmix object count {num_umx_signals} exceeds capacity {limit}"
            ),
            AjocDeError::DmxSignalsOutOfRange {
                num_dmx_signals,
                limit,
            } => write!(
                f,
                "Downmix signal count {num_dmx_signals} exceeds capacity {limit}"
            ),
            AjocDeError::MissingConfiguration => {
                write!(
                    f,
                    "Current frame has no dialogue-enhancement configuration and no prior configuration to continue"
                )
            }
            AjocDeError::ObjectCountChanged { previous, current } => write!(
                f,
                "Previous frame recorded {previous} objects and current frame has {current}; de_main_dlg_flag cannot be continued"
            ),
        }
    }
}

impl core::error::Error for AjocDeError {}

impl From<ReadError> for AjocDeError {
    fn from(error: ReadError) -> Self {
        AjocDeError::Read(error)
    }
}

/// `ajoc_dmx_de_data()` 的跨帧状态。
///
/// `de_main_dlg_flag[]` 只在 `b_dmx_de_cfg` 为真时传输；为假时沿用上一帧，
/// 由它派生的 `num_dlg_obj` 决定后续系数的循环次数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AjocDeState {
    main_dlg_flag: [bool; MAX_DE_OBJECTS],
    num_umx_signals: u32,
    configured: bool,
}

impl AjocDeState {
    /// 一个尚未收到任何配置的初始状态。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            main_dlg_flag: [false; MAX_DE_OBJECTS],
            num_umx_signals: 0,
            configured: false,
        }
    }

    /// 是否已收到过 `de_main_dlg_flag[]`。
    #[must_use]
    pub const fn is_configured(&self) -> bool {
        self.configured
    }

    /// `de_main_dlg_flag[obj]`。
    #[must_use]
    pub fn main_dlg_flag(&self, obj: usize) -> Option<bool> {
        if !self.configured || obj >= usize::try_from(self.num_umx_signals).unwrap_or(0) {
            return None;
        }
        self.main_dlg_flag.get(obj).copied()
    }
}

impl Default for AjocDeState {
    fn default() -> Self {
        Self::new()
    }
}

/// `ajoc_dmx_de_data()` 的解析结果，见 `6.2.3.5`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AjocDmxDeData {
    /// `b_dmx_de_cfg`：本帧是否携带对话增强配置。
    pub cfg_present: bool,
    /// `b_keep_dmx_de_coeffs`：为真时沿用上一帧的系数。
    ///
    /// `6.3.6.6.2` 要求解码器在 I 帧或 `b_dmx_de_cfg` 为真时**忽略**该标志；
    /// 但语法层面它仍然决定系数是否出现，故解析一律按字面取值。
    pub keep_coeffs: bool,
    /// `de_max_gain`，2 位；仅 `b_dmx_de_cfg` 为真时传输。
    pub max_gain: Option<u8>,
    /// `num_dlg_obj`，由 `Pseudocode 28` 的 `dlg_obj()` 派生。
    pub num_dlg_obj: u8,
    dlg_idx: [u8; MAX_DE_OBJECTS],
    coeff_idx: [[u8; MAX_AJOC_DMX_SIGNALS]; MAX_DE_OBJECTS],
    coeffs_present: bool,
    num_dmx_signals: u8,
}

impl AjocDmxDeData {
    const fn empty() -> Self {
        Self {
            cfg_present: false,
            keep_coeffs: false,
            max_gain: None,
            num_dlg_obj: 0,
            dlg_idx: [0; MAX_DE_OBJECTS],
            coeff_idx: [[0; MAX_AJOC_DMX_SIGNALS]; MAX_DE_OBJECTS],
            coeffs_present: false,
            num_dmx_signals: 0,
        }
    }

    /// `dlg_idx[dio]`：第 `dio` 个对话对象对应的上混对象下标。
    #[must_use]
    pub fn dlg_idx(&self, dio: usize) -> Option<u8> {
        if dio >= usize::from(self.num_dlg_obj) {
            return None;
        }
        self.dlg_idx.get(dio).copied()
    }

    /// `de_dlg_dmx_coeff_idx[dio][dmxo]`，即表 82 的分子（0 至 15）。
    ///
    /// 本帧未传输系数时返回 `None`——沿用上一帧是调用方的职责，此处不替它
    /// 猜测。
    #[must_use]
    pub fn coeff_idx(&self, dio: usize, dmxo: usize) -> Option<u8> {
        if !self.coeffs_present
            || dio >= usize::from(self.num_dlg_obj)
            || dmxo >= usize::from(self.num_dmx_signals)
        {
            return None;
        }
        self.coeff_idx.get(dio)?.get(dmxo).copied()
    }

    /// 本帧是否传输了下混系数。
    #[must_use]
    pub const fn coeffs_present(&self) -> bool {
        self.coeffs_present
    }
}

/// 解析 `ajoc_dmx_de_data()`，见 `6.2.3.5`。
///
/// `state` 保存 `de_main_dlg_flag[]`，供 `b_dmx_de_cfg` 为假的帧沿用。
///
/// # I 帧与缺席的配置
///
/// `b_dmx_de_cfg` 为假时规范只说配置不在本帧（`6.3.6.6.1`），没有说该沿用
/// 什么。I 帧的定义（`4.5.2` 的 `b_audio_ndot`）要求该帧可独立于前序帧解码，
/// 因此 I 帧上的缺席只能解释为**没有对话对象**——`de_main_dlg_flag[]` 全零，
/// `num_dlg_obj` 为 0，系数循环不执行。若把它当作「未知」，编码器就无法写出
/// 一个不带对话增强的 I 帧。非 I 帧上的缺席仍是沿用，无历史即报错。
///
/// # `b_keep_dmx_de_coeffs` 的「忽略」
///
/// `6.3.6.6.2` 说该标志在 I 帧或 `b_dmx_de_cfg` 为真时应被解码器忽略。那是
/// **语义**层面的话——不得据它复用前一帧的系数；`6.2.3.5` 的语法表里系数是否
/// 出现无条件由该标志决定，故本函数的比特消耗不因 `b_iframe` 改变。
///
/// # Errors
///
/// 见 [`AjocDeError`]。
pub fn parse_dmx_de_data(
    reader: &mut BitReader<'_>,
    num_dmx_signals: u8,
    num_umx_signals: u32,
    b_iframe: bool,
    state: &mut AjocDeState,
) -> Result<AjocDmxDeData, AjocDeError> {
    if usize::from(num_dmx_signals) > MAX_AJOC_DMX_SIGNALS {
        return Err(AjocDeError::DmxSignalsOutOfRange {
            num_dmx_signals,
            limit: MAX_AJOC_DMX_SIGNALS,
        });
    }
    let umx = usize::try_from(num_umx_signals).unwrap_or(usize::MAX);
    if umx > MAX_DE_OBJECTS {
        return Err(AjocDeError::UmxSignalsOutOfRange {
            num_umx_signals,
            limit: MAX_DE_OBJECTS,
        });
    }

    let mut out = AjocDmxDeData::empty();
    out.num_dmx_signals = num_dmx_signals;
    out.cfg_present = reader.read_flag()?;
    out.keep_coeffs = reader.read_flag()?;

    // 解析全程写入状态副本，仅在成功后提交。
    let mut next_state = *state;

    if out.cfg_present {
        out.max_gain = Some(read_u8(reader, 2)?);
        next_state.main_dlg_flag = [false; MAX_DE_OBJECTS];
        for obj in 0..umx {
            let flag = reader.read_flag()?;
            if let Some(slot) = next_state.main_dlg_flag.get_mut(obj) {
                *slot = flag;
            }
        }
        next_state.num_umx_signals = num_umx_signals;
        next_state.configured = true;
    } else if b_iframe {
        // I 帧不得依赖前序帧：配置缺席即「没有对话对象」，而非未知。
        next_state.main_dlg_flag = [false; MAX_DE_OBJECTS];
        next_state.num_umx_signals = num_umx_signals;
        next_state.configured = true;
    } else {
        if !next_state.configured {
            return Err(AjocDeError::MissingConfiguration);
        }
        if next_state.num_umx_signals != num_umx_signals {
            return Err(AjocDeError::ObjectCountChanged {
                previous: next_state.num_umx_signals,
                current: num_umx_signals,
            });
        }
    }

    // `Pseudocode 28` dlg_obj()：按上混对象顺序收集置位的下标。
    let mut num_dlg_obj = 0usize;
    for obj in 0..umx {
        if !next_state.main_dlg_flag.get(obj).copied().unwrap_or(false) {
            continue;
        }
        if let Some(slot) = out.dlg_idx.get_mut(num_dlg_obj) {
            *slot = u8::try_from(obj).unwrap_or(u8::MAX);
        }
        num_dlg_obj = num_dlg_obj.saturating_add(1);
    }
    out.num_dlg_obj = u8::try_from(num_dlg_obj).unwrap_or(u8::MAX);

    if !out.keep_coeffs {
        out.coeffs_present = true;
        for dio in 0..num_dlg_obj {
            for dmxo in 0..usize::from(num_dmx_signals) {
                let value = read_coeff_idx(reader)?;
                if let Some(slot) = out.coeff_idx.get_mut(dio).and_then(|row| row.get_mut(dmxo)) {
                    *slot = value;
                }
            }
        }
    }

    *state = next_state;
    Ok(out)
}

/// 表 82 `de_dlg_dmx_coeff_idx` 的变长前缀码。
///
/// 三种码长：`0` 一位表示系数 0；`1111` 四位表示系数 1（即 15/15）；其余
/// `1xxxx` 五位表示 1/15 至 14/15。四位码与五位码不冲突，因为后者的
/// `xxxx` 只取到 `1101`，前四位恒不等于 `1111`。
///
/// 返回的是表 82 的**分子**，取值 0 至 15。
fn read_coeff_idx(reader: &mut BitReader<'_>) -> Result<u8, AjocDeError> {
    if !reader.read_flag()? {
        return Ok(0);
    }
    let high = read_u8(reader, 3)?;
    if high == 0b111 {
        return Ok(DE_COEFF_DENOMINATOR);
    }
    let low = u8::from(reader.read_flag()?);
    let index = high.saturating_mul(2).saturating_add(low);
    Ok(index.saturating_add(1))
}

/// `ajoc_bed_info()` 的解析结果，见 `6.2.3.6`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AjocBedInfo {
    /// `b_obj_without_bed_info_present`。
    pub obj_without_bed_info_present: bool,
    /// `num_obj_with_bed_render_info`，3 位；仅上一标志为真时传输。
    ///
    /// `6.3.6.7.2` 的 NOTE 指出床对象是最先传输的那些对象。
    pub num_obj_with_bed_render_info: Option<u8>,
    /// 本元素消耗的比特数。
    ///
    /// `audio_data_ajoc()` 用它从 `b_oamd_extension_present` 的跳过长度里
    /// 扣除已解析的部分，故必须一并返回。
    pub bits_read: u8,
}

/// 解析 `ajoc_bed_info()`，见 `6.2.3.6`。
///
/// # Errors
///
/// 数据不足时返回 [`AjocDeError::Read`]。
pub fn parse_bed_info(reader: &mut BitReader<'_>) -> Result<AjocBedInfo, AjocDeError> {
    let present = reader.read_flag()?;
    if !present {
        return Ok(AjocBedInfo {
            obj_without_bed_info_present: false,
            num_obj_with_bed_render_info: None,
            bits_read: 1,
        });
    }
    Ok(AjocBedInfo {
        obj_without_bed_info_present: true,
        num_obj_with_bed_render_info: Some(read_u8(reader, 3)?),
        bits_read: 4,
    })
}

fn read_u8(reader: &mut BitReader<'_>, bits: u32) -> Result<u8, AjocDeError> {
    let value = reader.read_bits(bits)?;
    Ok(u8::try_from(value).unwrap_or(u8::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BitBuf {
        bytes: [u8; 256],
        len: usize,
    }

    impl BitBuf {
        const fn new() -> Self {
            Self {
                bytes: [0; 256],
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

        fn as_slice(&self) -> &[u8] {
            let bytes = self.len.div_ceil(8);
            self.bytes.get(..bytes).unwrap_or(&self.bytes)
        }

        /// 按表 82 写入一个系数下标。
        fn push_coeff(&mut self, index: u8) {
            match index {
                0 => self.push(false),
                DE_COEFF_DENOMINATOR => self.push_bits(0b1111, 4),
                other => {
                    let payload = u32::from(other.saturating_sub(1));
                    self.push_bits(0b1_0000 | payload, 5);
                }
            }
        }
    }

    /// 表 82 的十六个取值构成前缀码，且长度只有 1、4、5 三种。
    ///
    /// 逐值往返：按表写入再解析，必须回到原值且消耗的比特数与码长一致。
    #[test]
    fn coefficient_code_is_a_prefix_code() {
        for index in 0..=DE_COEFF_DENOMINATOR {
            let mut buf = BitBuf::new();
            buf.push_coeff(index);
            let expected_len = match index {
                0 => 1,
                DE_COEFF_DENOMINATOR => 4,
                _ => 5,
            };
            assert_eq!(buf.len, expected_len, "系数 {index} 的码长");

            let mut reader = BitReader::new(buf.as_slice());
            assert_eq!(read_coeff_idx(&mut reader), Ok(index), "系数 {index} 往返");
            assert_eq!(
                reader.bit_position(),
                u64::try_from(expected_len).unwrap_or(0),
                "系数 {index} 消耗的比特数"
            );
        }
    }

    /// 四位码 `1111` 与任一五位码的前四位都不相同。
    ///
    /// 这是表 82 前缀无关的关键：五位码的低四位只取到 `1101`。
    #[test]
    fn four_bit_code_never_prefixes_a_five_bit_one() {
        for index in 1..DE_COEFF_DENOMINATOR {
            let mut buf = BitBuf::new();
            buf.push_coeff(index);
            let mut prefix = 0u8;
            for bit in 0..4 {
                let byte = buf.bytes.first().copied().unwrap_or(0);
                let value = (byte >> (7 - bit)) & 1;
                prefix = (prefix << 1) | value;
            }
            assert_ne!(prefix, 0b1111, "系数 {index} 的前四位与四位码冲突");
        }
    }

    /// `b_dmx_de_cfg` 为真时传输配置，`dlg_obj()` 按顺序收集置位下标。
    #[test]
    fn configuration_derives_the_dialogue_object_indices() {
        let mut buf = BitBuf::new();
        buf.push(true); // b_dmx_de_cfg
        buf.push(true); // b_keep_dmx_de_coeffs → 本帧不传系数
        buf.push_bits(2, 2); // de_max_gain
        // de_main_dlg_flag[0..4] = 0,1,0,1
        buf.push(false);
        buf.push(true);
        buf.push(false);
        buf.push(true);
        let expected = buf.len;

        let mut state = AjocDeState::new();
        let mut reader = BitReader::new(buf.as_slice());
        let data = parse_dmx_de_data(&mut reader, 2, 4, true, &mut state).expect("应能解析");

        assert_eq!(reader.bit_position(), u64::try_from(expected).unwrap_or(0));
        assert!(data.cfg_present);
        assert_eq!(data.max_gain, Some(2));
        assert_eq!(data.num_dlg_obj, 2);
        assert_eq!(data.dlg_idx(0), Some(1));
        assert_eq!(data.dlg_idx(1), Some(3));
        assert_eq!(data.dlg_idx(2), None);
        assert!(!data.coeffs_present(), "b_keep 为真时不传系数");
        assert_eq!(data.coeff_idx(0, 0), None);
        assert!(state.is_configured());
        assert_eq!(state.main_dlg_flag(1), Some(true));
        assert_eq!(state.main_dlg_flag(4), None);
    }

    /// 系数按 `num_dlg_obj × num_dmx_signals` 传输，落点必须相等。
    #[test]
    fn coefficients_follow_the_dialogue_object_count() {
        let mut buf = BitBuf::new();
        buf.push(true); // b_dmx_de_cfg
        buf.push(false); // b_keep_dmx_de_coeffs = 0 → 传系数
        buf.push_bits(1, 2); // de_max_gain
        buf.push(true); // 对象 0 是主对话
        buf.push(false);
        buf.push(true); // 对象 2 是主对话
        // 两个对话对象 × 三个下混信号，覆盖三种码长。
        let values: [[u8; 3]; 2] = [[0, 15, 7], [1, 14, 0]];
        for row in values {
            for value in row {
                buf.push_coeff(value);
            }
        }
        let expected = buf.len;

        let mut state = AjocDeState::new();
        let mut reader = BitReader::new(buf.as_slice());
        let data = parse_dmx_de_data(&mut reader, 3, 3, true, &mut state).expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );
        assert_eq!(data.num_dlg_obj, 2);
        assert!(data.coeffs_present());
        for (dio, row) in values.iter().enumerate() {
            for (dmxo, &value) in row.iter().enumerate() {
                assert_eq!(
                    data.coeff_idx(dio, dmxo),
                    Some(value),
                    "系数 [{dio}][{dmxo}]"
                );
            }
        }
        assert_eq!(data.coeff_idx(2, 0), None, "越界不得暴露");
        assert_eq!(data.coeff_idx(0, 3), None);
    }

    /// `b_dmx_de_cfg` 为假时沿用上一帧的 `de_main_dlg_flag[]`。
    ///
    /// 该标志决定 `num_dlg_obj`，进而决定系数段的长度；沿用错了落点立刻偏移。
    #[test]
    fn configuration_is_carried_across_frames() {
        let mut first = BitBuf::new();
        first.push(true); // b_dmx_de_cfg
        first.push(true); // 本帧不传系数
        first.push_bits(0, 2);
        for flag in [true, false, true, true] {
            first.push(flag); // 三个主对话对象
        }

        let mut state = AjocDeState::new();
        let mut reader = BitReader::new(first.as_slice());
        let data = parse_dmx_de_data(&mut reader, 2, 4, true, &mut state).expect("首帧应能解析");
        assert_eq!(data.num_dlg_obj, 3);

        // 次帧不带配置，但要传系数：长度由沿用的 num_dlg_obj 决定。
        let mut second = BitBuf::new();
        second.push(false); // b_dmx_de_cfg = 0
        second.push(false); // b_keep_dmx_de_coeffs = 0
        for _ in 0..3 {
            for _ in 0..2 {
                second.push_coeff(15); // 四位码
            }
        }
        let expected = second.len;

        let mut reader = BitReader::new(second.as_slice());
        let data = parse_dmx_de_data(&mut reader, 2, 4, false, &mut state).expect("次帧应能解析");
        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "系数段长度应由沿用的 num_dlg_obj 决定"
        );
        assert_eq!(data.num_dlg_obj, 3);
        assert_eq!(data.max_gain, None, "本帧未传配置");
        assert_eq!(data.coeff_idx(2, 1), Some(15));
    }

    /// 空状态下没有可沿用的 `de_main_dlg_flag[]`，不得把未知对象数当成零。
    #[test]
    fn rejects_missing_configuration_before_deriving_dialogue_count() {
        let mut buf = BitBuf::new();
        buf.push(false); // b_dmx_de_cfg = 0
        buf.push(false); // b_keep_dmx_de_coeffs = 0
        buf.push_coeff(15); // 模拟随后可能存在的系数，解析器不得静默跨过

        let mut state = AjocDeState::new();
        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_dmx_de_data(&mut reader, 2, 4, false, &mut state),
            Err(AjocDeError::MissingConfiguration)
        );
        assert_eq!(
            reader.bit_position(),
            2,
            "只能消费固定的两个标志，不得假定 num_dlg_obj 为零"
        );
        assert!(!state.is_configured(), "失败帧不得提交伪配置");
    }

    /// I 帧上缺席的配置意味着「没有对话对象」，而不是「未知」。
    ///
    /// 实测码流的每一帧都是 `b_dmx_de_cfg == 0`：该编码链根本不带对话增强。
    /// 若在 I 帧上也报 `MissingConfiguration`，整条链都解不下去；而 I 帧按
    /// `4.5.2` 必须能独立解码，缺席只能读作全零标志。
    #[test]
    fn iframe_treats_absent_configuration_as_no_dialogue_objects() {
        let mut buf = BitBuf::new();
        buf.push(false); // b_dmx_de_cfg = 0
        buf.push(false); // b_keep_dmx_de_coeffs = 0 → 系数「存在」
        let expected = buf.len;

        let mut state = AjocDeState::new();
        let mut reader = BitReader::new(buf.as_slice());
        let data = parse_dmx_de_data(&mut reader, 9, 20, true, &mut state).expect("I 帧应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "num_dlg_obj 为 0，系数循环不执行，只消耗两个标志"
        );
        assert_eq!(data.num_dlg_obj, 0);
        assert_eq!(data.dlg_idx(0), None);
        assert!(data.coeffs_present(), "keep 为假，语法层面系数段存在但为空");
        assert!(state.is_configured(), "I 帧确立了一份空配置");
        assert_eq!(state.main_dlg_flag(0), Some(false));
        assert_eq!(state.main_dlg_flag(20), None, "记下的是本帧的对象数");

        // 随后的非 I 帧可以沿用这份空配置，同样只消耗两个标志。
        let mut reader = BitReader::new(buf.as_slice());
        let data = parse_dmx_de_data(&mut reader, 9, 20, false, &mut state).expect("次帧应能解析");
        assert_eq!(data.num_dlg_obj, 0);
        assert_eq!(reader.bit_position(), u64::try_from(expected).unwrap_or(0));
    }

    /// 沿用状态时对象数变化必须报错，而不是拿错位的标志硬算。
    #[test]
    fn rejects_object_count_change_while_carrying_state() {
        let mut first = BitBuf::new();
        first.push(true);
        first.push(true);
        first.push_bits(0, 2);
        for _ in 0..4 {
            first.push(true);
        }
        let mut state = AjocDeState::new();
        let mut reader = BitReader::new(first.as_slice());
        parse_dmx_de_data(&mut reader, 2, 4, true, &mut state).expect("首帧应能解析");

        let mut second = BitBuf::new();
        second.push(false); // 不带配置
        second.push(true);
        let mut reader = BitReader::new(second.as_slice());
        assert_eq!(
            parse_dmx_de_data(&mut reader, 2, 5, false, &mut state),
            Err(AjocDeError::ObjectCountChanged {
                previous: 4,
                current: 5
            })
        );
    }

    /// 越界的信号数必须在读取任何比特前拒绝。
    #[test]
    fn rejects_out_of_range_signal_counts_before_reading() {
        let buf = BitBuf::new();
        let mut state = AjocDeState::new();

        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_dmx_de_data(&mut reader, 17, 4, true, &mut state),
            Err(AjocDeError::DmxSignalsOutOfRange {
                num_dmx_signals: 17,
                limit: MAX_AJOC_DMX_SIGNALS
            })
        );
        assert_eq!(reader.bit_position(), 0);

        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_dmx_de_data(&mut reader, 2, 33, true, &mut state),
            Err(AjocDeError::UmxSignalsOutOfRange {
                num_umx_signals: 33,
                limit: MAX_DE_OBJECTS
            })
        );
        assert_eq!(reader.bit_position(), 0);
    }

    /// 失败的帧不得提交跨帧状态。
    #[test]
    fn failed_frame_does_not_commit_state() {
        // 只写两位；十六个对象需要 2 + 2 + 16 = 20 位，切片补齐到一字节后
        // 仍只有 8 位可读，故必然在标志中途越界。
        let mut buf = BitBuf::new();
        buf.push(true); // b_dmx_de_cfg
        buf.push(true); // b_keep_dmx_de_coeffs
        let mut state = AjocDeState::new();
        let mut reader = BitReader::new(buf.as_slice());
        assert!(
            parse_dmx_de_data(&mut reader, 2, 16, true, &mut state).is_err(),
            "标志不足应报错"
        );
        assert!(!state.is_configured(), "失败帧不得写入配置");
    }

    /// 已有有效状态时，中途失败的帧不得破坏它。
    ///
    /// 「首帧失败」测不出这一点：`configured` 本就在标志循环之后才置真。
    /// 真正的风险是带配置的帧先清空 `de_main_dlg_flag[]`、写入一半再越界，
    /// 把上一帧仍然有效的配置毁掉。
    #[test]
    fn failed_frame_does_not_damage_an_existing_configuration() {
        let mut first = BitBuf::new();
        first.push(true); // b_dmx_de_cfg
        first.push(true); // 不传系数
        first.push_bits(0, 2);
        for obj in 0..16 {
            first.push(obj % 3 == 0); // 六个主对话对象
        }
        let mut state = AjocDeState::new();
        let mut reader = BitReader::new(first.as_slice());
        let data = parse_dmx_de_data(&mut reader, 2, 16, true, &mut state).expect("首帧应能解析");
        assert_eq!(data.num_dlg_obj, 6);
        let before = state;

        // 次帧同样带配置，但只给到 de_max_gain 就截断。
        let mut second = BitBuf::new();
        second.push(true); // b_dmx_de_cfg
        second.push(true);
        second.push_bits(3, 2); // de_max_gain
        let mut reader = BitReader::new(second.as_slice());
        assert!(
            parse_dmx_de_data(&mut reader, 2, 16, true, &mut state).is_err(),
            "标志不足应报错"
        );
        assert_eq!(state, before, "失败帧不得破坏已有配置");
        assert_eq!(state.main_dlg_flag(0), Some(true));
        assert_eq!(state.main_dlg_flag(1), Some(false));
    }

    /// `ajoc_bed_info()` 只有一位或四位两种长度，并返回消耗的比特数。
    #[test]
    fn bed_info_reports_its_own_length() {
        let mut buf = BitBuf::new();
        buf.push(false);
        let mut reader = BitReader::new(buf.as_slice());
        let info = parse_bed_info(&mut reader).expect("应能解析");
        assert_eq!(info.bits_read, 1);
        assert_eq!(info.num_obj_with_bed_render_info, None);
        assert_eq!(reader.bit_position(), 1);

        let mut buf = BitBuf::new();
        buf.push(true);
        buf.push_bits(5, 3);
        let mut reader = BitReader::new(buf.as_slice());
        let info = parse_bed_info(&mut reader).expect("应能解析");
        assert_eq!(info.bits_read, 4);
        assert_eq!(info.num_obj_with_bed_render_info, Some(5));
        assert_eq!(reader.bit_position(), 4);
    }
}
