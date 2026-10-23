//! Audio-substream dialogue-enhancement 帧内 data 语法与 Huffman 解码。
//!
//! 对应 `TS103190-2:v1.3.1:6.2.7.5`–`6.2.7.6` 与
//! `TS103190-1:v1.4.1:4.2.14.13`、`4.3.14.4`–`4.3.14.5`、附录
//! `A.4`。本模块把四张 DE 码本解为 absolute/differential 整数码值，并解析 panning、
//! keep、M/S、hybrid contribution 与 simulcast gate；[`DialogEnhancementState`] 再按物理
//! substream 延续配置与参数索引。它不反量化、不执行 dialogue enhancement，也不修改 PCM。

use crate::huffman::{HuffmanError, HuffmanTable, tables};
use core::fmt;
use macindecode_ac4_bitstream::audio_substream::{
    DialogEnhancementConfiguration, DialogEnhancementConfigurationUpdate, DialogEnhancementMetadata,
};
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

mod state;
pub use state::{
    DialogEnhancementEffectiveData, DialogEnhancementEffectiveDataBlock,
    DialogEnhancementEffectiveParameterData, DialogEnhancementEffectiveSimulcastData,
    DialogEnhancementState, DialogEnhancementStateError,
};

/// P1 `4.3.14.5.1` 固定的 DE 参数频带数。
pub const DIALOG_ENHANCEMENT_PARAMETER_BANDS: usize = 8;
/// 表 171 最多声明的 DE 参数声道数。
pub const MAX_DIALOG_ENHANCEMENT_PARAMETER_CHANNELS: usize = 3;
/// 单个 `de_data()` 最多携带的 Huffman 参数码值数。
pub const MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES: usize =
    DIALOG_ENHANCEMENT_PARAMETER_BANDS * MAX_DIALOG_ENHANCEMENT_PARAMETER_CHANNELS;

const DE_ABS_0_OFFSET: i16 = 0;
const DE_DIFF_0_OFFSET: i16 = 31;
const DE_ABS_1_OFFSET: i16 = 30;
const DE_DIFF_1_OFFSET: i16 = 60;

/// 解码 audio-substream `de_data()` 失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogEnhancementDataError {
    /// 读取固定宽度字段时越过 tools metadata 边界。
    Read(ReadError),
    /// DE Huffman 码字截断或生成表损坏。
    Huffman(HuffmanError),
    /// dependent frame 沿用配置，但调用方没有提供前一有效配置。
    MissingConfiguration {
        /// `de_data()` 的起始 bit offset。
        bit_position: u64,
    },
    /// 解析结果被修改成与原帧上下文不一致的 configuration 更新形态。
    InconsistentMetadata {
        /// 检测到不一致时的 bit offset。
        bit_position: u64,
    },
    /// 调用方提供的历史配置不符合 2/2/3 比特字段范围。
    InvalidConfiguration {
        /// `de_method`。
        method: u8,
        /// `de_max_gain`。
        max_gain: u8,
        /// `de_channel_config`。
        channel_config: u8,
    },
    /// 固定容量不足以保存声明的参数码值。
    CapacityExceeded {
        /// 尝试保存的码值数。
        declared: usize,
        /// 实现与规范共同上限。
        limit: usize,
    },
    /// 已知 `dialog_enhancement()` 语法后仍有多余 tools metadata 比特。
    TrailingBits {
        /// 首个多余比特的原 substream offset。
        bit_position: u64,
        /// 尚未消费的比特数。
        remaining_bits: u64,
    },
}

impl fmt::Display for DialogEnhancementDataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Read(error) => write!(
                formatter,
                "Failed to read dialogue-enhancement data: {error}"
            ),
            Self::Huffman(error) => write!(
                formatter,
                "Failed to decode dialogue-enhancement Huffman data: {error}"
            ),
            Self::MissingConfiguration { bit_position } => write!(
                formatter,
                "Dependent dialogue-enhancement data has no previous configuration at bit offset {bit_position}"
            ),
            Self::InconsistentMetadata { bit_position } => write!(
                formatter,
                "Dialogue-enhancement metadata is inconsistent with its frame context at bit offset {bit_position}"
            ),
            Self::InvalidConfiguration {
                method,
                max_gain,
                channel_config,
            } => write!(
                formatter,
                "Dialogue-enhancement configuration ({method}, {max_gain}, {channel_config}) exceeds its field widths"
            ),
            Self::CapacityExceeded { declared, limit } => write!(
                formatter,
                "Dialogue-enhancement parameter count {declared} exceeds limit {limit}"
            ),
            Self::TrailingBits {
                bit_position,
                remaining_bits,
            } => write!(
                formatter,
                "Dialogue-enhancement data has {remaining_bits} trailing bits at bit offset {bit_position}"
            ),
        }
    }
}

impl core::error::Error for DialogEnhancementDataError {}

impl From<ReadError> for DialogEnhancementDataError {
    fn from(error: ReadError) -> Self {
        Self::Read(error)
    }
}

impl From<HuffmanError> for DialogEnhancementDataError {
    fn from(error: HuffmanError) -> Self {
        Self::Huffman(error)
    }
}

/// 新传输的 dialogue panning 参数原始索引。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogEnhancementMixCoefficients {
    /// 5 比特 `de_mix_coef1_idx`。
    pub first_index: u8,
    /// 三参数声道时的 5 比特 `de_mix_coef2_idx`。
    pub second_index: Option<u8>,
}

/// 当前 `de_data()` 的 dialogue panning 更新形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogEnhancementPositionUpdate {
    /// method/channel gate 不适用，或这是不携带 panning 的 simulcast data。
    NotApplicable,
    /// dependent primary data 的 `de_keep_pos_flag` 为真。
    KeepPrevious,
    /// I-frame 默认更新，或 dependent primary data 显式更新。
    New(DialogEnhancementMixCoefficients),
}

/// 一个新传输参数集的 Huffman 解码码值与固定字段。
///
/// [`codes`](Self::codes) 按 channel → band 顺序保存。I-frame 的首码是 absolute 值，其余
/// 是 differential 值；dependent frame 的全部码值都是相对 `de_par_prev` 的 differential。
/// 本类型不执行跨 band/channel 或跨帧的差分还原。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogEnhancementParameterData {
    codes: [i16; MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES],
    parameter_channels: u8,
    first_code_absolute: bool,
    /// 仅 channel-independent/hybrid-independent 双声道分支传输的 `de_ms_proc_flag`。
    pub mid_side_processing: Option<bool>,
    /// hybrid method 新参数之后的 5 比特 `de_signal_contribution`。
    pub signal_contribution: Option<u8>,
}

impl DialogEnhancementParameterData {
    /// 实际传输参数的声道数；M/S 双声道数据为 1，否则等于 `de_nr_channels`。
    #[must_use]
    pub const fn parameter_channels(&self) -> u8 {
        self.parameter_channels
    }

    /// 当前集合的首个码值是否由 absolute 码本解码。
    #[must_use]
    pub const fn first_code_is_absolute(&self) -> bool {
        self.first_code_absolute
    }

    /// 按 channel → band 传输顺序取得全部 absolute/differential 码值。
    #[must_use]
    pub fn codes(&self) -> &[i16] {
        let len =
            usize::from(self.parameter_channels).saturating_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS);
        self.codes.get(..len).unwrap_or(&[])
    }

    /// 取得一个参数声道、频带位置的 absolute/differential 码值。
    #[must_use]
    pub fn code(&self, channel: usize, band: usize) -> Option<i16> {
        if channel >= usize::from(self.parameter_channels)
            || band >= DIALOG_ENHANCEMENT_PARAMETER_BANDS
        {
            return None;
        }
        let index = channel
            .checked_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS)?
            .checked_add(band)?;
        self.codes.get(index).copied()
    }
}

/// 当前 `de_data()` 的参数更新形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogEnhancementParameterUpdate {
    /// `de_nr_channels == 0`，语法中没有 parameter keep flag 或码值。
    NotApplicable,
    /// dependent frame 的 `de_keep_data_flag` 为真。
    KeepPrevious,
    /// I-frame 默认更新，或 dependent frame 显式传输新参数码值。
    New(DialogEnhancementParameterData),
}

/// 一次 primary 或 simulcast `de_data()` 的帧内更新。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogEnhancementDataBlock {
    /// dialogue panning 更新。
    pub position: DialogEnhancementPositionUpdate,
    /// 参数码值更新。
    pub parameters: DialogEnhancementParameterUpdate,
}

/// P2 channel mode 13/14 的第二份 core-decoding DE data gate。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogEnhancementSimulcastData {
    /// 当前 channel mode 不传输 `b_de_simulcast`。
    NotSignaled,
    /// gate 适用，但 `b_de_simulcast` 为假。
    NotPresent,
    /// `b_de_simulcast` 为真，随后携带第二份 `de_data()`。
    Present(DialogEnhancementDataBlock),
}

/// 一帧已完成 Huffman 语法解码的 dialogue-enhancement data。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogEnhancementDecodedData {
    /// 解析 `de_data()` 时使用的前置 info `b_audio_ndot`。
    pub b_iframe: bool,
    /// 当前帧解析 data 时使用的有效配置；可能来自本帧，也可能由调用方提供。
    pub configuration: DialogEnhancementConfiguration,
    /// full/普通解码使用的首份 `de_data()`。
    pub primary: DialogEnhancementDataBlock,
    /// channel mode 13/14 可选的 core simulcast data。
    pub simulcast: DialogEnhancementSimulcastData,
}

/// 为 bitstream 层保留的 dialogue-enhancement metadata 提供 Huffman data 解码。
///
/// 导入本 trait 后可继续以 `metadata.decode_data(...)` 的形式调用；trait 位于 decode
/// crate，避免让纯语法 crate 反向依赖规范表。
pub trait DialogEnhancementMetadataExt {
    /// 解码当前 tools metadata 中完整的 `de_data()` 与可选 simulcast data。
    ///
    /// `previous_configuration` 仅在当前更新为
    /// [`KeepPrevious`](DialogEnhancementConfigurationUpdate::KeepPrevious) 时使用。返回的参数
    /// 是 Huffman 码本映射后的 absolute/differential 整数，不是跨帧还原的有效 `de_par`。
    /// DE 缺席时返回 `Ok(None)`。解析成功必须恰好耗尽 tools metadata body。
    ///
    /// # Errors
    ///
    /// dependent frame 缺少前一配置、固定字段/Huffman 码字截断、配置超出字段范围或已知语法
    /// 后仍有尾随比特时返回错误。此方法无状态，失败不会修改调用方数据。
    fn decode_data(
        self,
        payload: &[u8],
        previous_configuration: Option<DialogEnhancementConfiguration>,
    ) -> Result<Option<DialogEnhancementDecodedData>, DialogEnhancementDataError>;
}

impl DialogEnhancementMetadataExt for DialogEnhancementMetadata {
    fn decode_data(
        self,
        payload: &[u8],
        previous_configuration: Option<DialogEnhancementConfiguration>,
    ) -> Result<Option<DialogEnhancementDecodedData>, DialogEnhancementDataError> {
        if !self.data_present {
            return Ok(None);
        }

        let bit_position = self.unparsed_body_bit_offset();
        let b_iframe = self
            .b_iframe()
            .ok_or(DialogEnhancementDataError::InconsistentMetadata { bit_position })?;
        let configuration = match self.configuration {
            DialogEnhancementConfigurationUpdate::NotPresent => {
                return Err(DialogEnhancementDataError::InconsistentMetadata { bit_position });
            }
            DialogEnhancementConfigurationUpdate::KeepPrevious => {
                if b_iframe {
                    return Err(DialogEnhancementDataError::InconsistentMetadata { bit_position });
                }
                previous_configuration
                    .ok_or(DialogEnhancementDataError::MissingConfiguration { bit_position })?
            }
            DialogEnhancementConfigurationUpdate::New(configuration) => configuration,
        };
        validate_configuration(configuration)?;

        let mut reader = BitReader::new_bounded(
            payload,
            self.unparsed_body_bit_offset(),
            u64::from(self.unparsed_body_len_bits()),
        )?;
        let primary = parse_data_block(&mut reader, configuration, b_iframe, false)?;
        let simulcast = if self.simulcast_gate() {
            if reader.read_flag()? {
                DialogEnhancementSimulcastData::Present(parse_data_block(
                    &mut reader,
                    configuration,
                    b_iframe,
                    true,
                )?)
            } else {
                DialogEnhancementSimulcastData::NotPresent
            }
        } else {
            DialogEnhancementSimulcastData::NotSignaled
        };

        if reader.remaining_bits() != 0 {
            return Err(DialogEnhancementDataError::TrailingBits {
                bit_position: reader.bit_position(),
                remaining_bits: reader.remaining_bits(),
            });
        }

        Ok(Some(DialogEnhancementDecodedData {
            b_iframe,
            configuration,
            primary,
            simulcast,
        }))
    }
}

fn validate_configuration(
    configuration: DialogEnhancementConfiguration,
) -> Result<(), DialogEnhancementDataError> {
    if configuration.method > 3 || configuration.max_gain > 3 || configuration.channel_config > 7 {
        return Err(DialogEnhancementDataError::InvalidConfiguration {
            method: configuration.method,
            max_gain: configuration.max_gain,
            channel_config: configuration.channel_config,
        });
    }
    Ok(())
}

fn parse_data_block(
    reader: &mut BitReader<'_>,
    configuration: DialogEnhancementConfiguration,
    b_iframe: bool,
    simulcast: bool,
) -> Result<DialogEnhancementDataBlock, DialogEnhancementDataError> {
    let channel_count = configuration.channel_count();
    if channel_count == 0 {
        return Ok(DialogEnhancementDataBlock {
            position: DialogEnhancementPositionUpdate::NotApplicable,
            parameters: DialogEnhancementParameterUpdate::NotApplicable,
        });
    }

    let position = if matches!(configuration.method, 1 | 3) && channel_count > 1 && !simulcast {
        let keep = !b_iframe && reader.read_flag()?;
        if keep {
            DialogEnhancementPositionUpdate::KeepPrevious
        } else {
            let first_index = u8::try_from(reader.read_bits(5)?).unwrap_or(u8::MAX);
            let second_index = if channel_count == 3 {
                Some(u8::try_from(reader.read_bits(5)?).unwrap_or(u8::MAX))
            } else {
                None
            };
            DialogEnhancementPositionUpdate::New(DialogEnhancementMixCoefficients {
                first_index,
                second_index,
            })
        }
    } else {
        DialogEnhancementPositionUpdate::NotApplicable
    };

    let keep_parameters = !b_iframe && reader.read_flag()?;
    let parameters = if keep_parameters {
        DialogEnhancementParameterUpdate::KeepPrevious
    } else {
        let mid_side_processing = if matches!(configuration.method, 0 | 2) && channel_count == 2 {
            Some(reader.read_flag()?)
        } else {
            None
        };
        let parameter_channels = channel_count
            .checked_sub(u8::from(mid_side_processing == Some(true)))
            .ok_or(DialogEnhancementDataError::CapacityExceeded {
                declared: 0,
                limit: MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES,
            })?;
        let code_count = usize::from(parameter_channels)
            .checked_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS)
            .ok_or(DialogEnhancementDataError::CapacityExceeded {
                declared: usize::MAX,
                limit: MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES,
            })?;
        if code_count > MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES {
            return Err(DialogEnhancementDataError::CapacityExceeded {
                declared: code_count,
                limit: MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES,
            });
        }

        let mut codes = [0i16; MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES];
        for index in 0..code_count {
            let absolute = b_iframe && index == 0;
            let value = decode_parameter_code(reader, configuration.method, absolute)?;
            let Some(slot) = codes.get_mut(index) else {
                return Err(DialogEnhancementDataError::CapacityExceeded {
                    declared: index.saturating_add(1),
                    limit: MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES,
                });
            };
            *slot = value;
        }
        let signal_contribution = if configuration.method >= 2 {
            Some(u8::try_from(reader.read_bits(5)?).unwrap_or(u8::MAX))
        } else {
            None
        };
        DialogEnhancementParameterUpdate::New(DialogEnhancementParameterData {
            codes,
            parameter_channels,
            first_code_absolute: b_iframe,
            mid_side_processing,
            signal_contribution,
        })
    };

    Ok(DialogEnhancementDataBlock {
        position,
        parameters,
    })
}

fn parameter_codebook(method: u8, absolute: bool) -> (&'static HuffmanTable, i16) {
    match (method & 1, absolute) {
        (0, true) => (&tables::DE_HCB_ABS_0, DE_ABS_0_OFFSET),
        (0, false) => (&tables::DE_HCB_DIFF_0, DE_DIFF_0_OFFSET),
        (_, true) => (&tables::DE_HCB_ABS_1, DE_ABS_1_OFFSET),
        (_, false) => (&tables::DE_HCB_DIFF_1, DE_DIFF_1_OFFSET),
    }
}

fn decode_parameter_code(
    reader: &mut BitReader<'_>,
    method: u8,
    absolute: bool,
) -> Result<i16, DialogEnhancementDataError> {
    let (table, offset) = parameter_codebook(method, absolute);
    let symbol = i16::try_from(table.decode(reader)?).unwrap_or(i16::MAX);
    Ok(symbol.saturating_sub(offset))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::testutil::BitBuf;
    use macindecode_ac4_bitstream::audio_substream::{Ac4AudioSubstream, SubstreamContext};

    fn configuration(method: u8, channel_config: u8) -> DialogEnhancementConfiguration {
        DialogEnhancementConfiguration {
            method,
            max_gain: 2,
            channel_config,
        }
    }

    fn metadata(
        payload: &[u8],
        bit_len: usize,
        update: DialogEnhancementConfigurationUpdate,
        b_iframe: bool,
        simulcast_gate: bool,
    ) -> DialogEnhancementMetadata {
        DialogEnhancementMetadata::from_raw_parts(
            payload,
            true,
            update,
            0,
            u32::try_from(bit_len).unwrap_or(u32::MAX),
            Some(b_iframe),
            simulcast_gate,
        )
        .unwrap()
    }

    fn push_parameter_codes(
        bits: &mut BitBuf,
        method: u8,
        b_iframe: bool,
        parameter_channels: u8,
        first_value: i16,
    ) {
        let count =
            usize::from(parameter_channels).saturating_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS);
        for index in 0..count {
            let absolute = b_iframe && index == 0;
            let (table, offset) = parameter_codebook(method, absolute);
            let value = if absolute { first_value } else { 0 };
            let symbol = u16::try_from(offset.saturating_add(value)).unwrap_or(u16::MAX);
            bits.push_symbol(table, symbol);
        }
    }

    fn append_bits(target: &mut BitBuf, source: &BitBuf) {
        for index in 0..source.bit_len() {
            let byte = source.as_slice().get(index / 8).copied().unwrap_or(0);
            let shift = 7usize.saturating_sub(index % 8);
            target.push((byte >> shift) & 1 == 1);
        }
    }

    #[test]
    fn codebook_selection_and_offsets_match_annex_a_four_tables() {
        assert_eq!(parameter_codebook(0, true).0.len(), 32);
        assert_eq!(parameter_codebook(0, false).0.len(), 63);
        assert_eq!(parameter_codebook(1, true).0.len(), 61);
        assert_eq!(parameter_codebook(1, false).0.len(), 121);
        assert_eq!(parameter_codebook(0, true).1, 0);
        assert_eq!(parameter_codebook(2, false).1, 31);
        assert_eq!(parameter_codebook(1, true).1, 30);
        assert_eq!(parameter_codebook(3, false).1, 60);
        assert!(core::ptr::eq(
            parameter_codebook(0, true).0,
            parameter_codebook(2, true).0
        ));
        assert!(core::ptr::eq(
            parameter_codebook(1, false).0,
            parameter_codebook(3, false).0
        ));

        for (method, absolute, symbol, expected) in [
            (0, true, 31, 31),
            (0, false, 0, -31),
            (1, true, 0, -30),
            (1, false, 120, 60),
        ] {
            let (table, _) = parameter_codebook(method, absolute);
            let mut bits = BitBuf::new();
            bits.push_symbol(table, symbol);
            let mut reader = BitReader::new_bounded(
                bits.as_slice(),
                0,
                u64::try_from(bits.bit_len()).unwrap_or(u64::MAX),
            )
            .unwrap();
            assert_eq!(
                decode_parameter_code(&mut reader, method, absolute).unwrap(),
                expected
            );
            assert_eq!(reader.remaining_bits(), 0);
        }
    }

    #[test]
    fn decodes_independent_cross_channel_hybrid_data() {
        let configuration = configuration(3, 7);
        let mut bits = BitBuf::new();
        bits.push_bits(4, 5);
        bits.push_bits(20, 5);
        push_parameter_codes(&mut bits, configuration.method, true, 3, 2);
        bits.push_bits(17, 5);

        for cut in 0..bits.bit_len() {
            assert!(
                metadata(
                    bits.as_slice(),
                    cut,
                    DialogEnhancementConfigurationUpdate::New(configuration),
                    true,
                    false,
                )
                .decode_data(bits.as_slice(), None)
                .is_err(),
                "截断在 data bit {cut} 后必须失败"
            );
        }

        let decoded = metadata(
            bits.as_slice(),
            bits.bit_len(),
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            false,
        )
        .decode_data(bits.as_slice(), None)
        .unwrap()
        .unwrap();

        assert_eq!(decoded.configuration, configuration);
        assert!(decoded.b_iframe);
        assert_eq!(
            decoded.primary.position,
            DialogEnhancementPositionUpdate::New(DialogEnhancementMixCoefficients {
                first_index: 4,
                second_index: Some(20),
            })
        );
        let DialogEnhancementParameterUpdate::New(parameters) = decoded.primary.parameters else {
            panic!("I-frame 应传输新参数")
        };
        assert_eq!(parameters.parameter_channels(), 3);
        assert!(parameters.first_code_is_absolute());
        assert_eq!(parameters.mid_side_processing, None);
        assert_eq!(parameters.signal_contribution, Some(17));
        assert_eq!(parameters.codes().len(), 24);
        assert_eq!(parameters.codes().first(), Some(&2));
        assert!(parameters.codes().iter().skip(1).all(|&value| value == 0));
        assert_eq!(parameters.code(2, 7), Some(0));
        assert_eq!(parameters.code(3, 0), None);
        assert_eq!(
            decoded.simulcast,
            DialogEnhancementSimulcastData::NotSignaled
        );
    }

    #[test]
    fn ms_processing_reduces_two_input_channels_to_one_parameter_channel() {
        let configuration = configuration(2, 6);
        let mut bits = BitBuf::new();
        bits.push(true);
        push_parameter_codes(&mut bits, configuration.method, true, 1, 5);
        bits.push_bits(31, 5);

        let decoded = metadata(
            bits.as_slice(),
            bits.bit_len(),
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            false,
        )
        .decode_data(bits.as_slice(), None)
        .unwrap()
        .unwrap();
        assert_eq!(
            decoded.primary.position,
            DialogEnhancementPositionUpdate::NotApplicable
        );
        let DialogEnhancementParameterUpdate::New(parameters) = decoded.primary.parameters else {
            panic!("I-frame 应传输新参数")
        };
        assert_eq!(parameters.mid_side_processing, Some(true));
        assert_eq!(parameters.parameter_channels(), 1);
        assert_eq!(parameters.codes().len(), 8);
        assert_eq!(parameters.code(0, 0), Some(5));
        assert_eq!(parameters.signal_contribution, Some(31));
    }

    #[test]
    fn dependent_data_uses_previous_configuration_and_preserves_keep_updates() {
        let previous_configuration = configuration(1, 6);
        let mut bits = BitBuf::new();
        bits.push(true);
        bits.push(true);
        let frame_metadata = metadata(
            bits.as_slice(),
            bits.bit_len(),
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            false,
        );

        assert_eq!(
            frame_metadata
                .decode_data(bits.as_slice(), None)
                .unwrap_err(),
            DialogEnhancementDataError::MissingConfiguration { bit_position: 0 }
        );
        let decoded = frame_metadata
            .decode_data(bits.as_slice(), Some(previous_configuration))
            .unwrap()
            .unwrap();
        assert_eq!(decoded.configuration, previous_configuration);
        assert!(!decoded.b_iframe);
        assert_eq!(
            decoded.primary.position,
            DialogEnhancementPositionUpdate::KeepPrevious
        );
        assert_eq!(
            decoded.primary.parameters,
            DialogEnhancementParameterUpdate::KeepPrevious
        );

        let new_configuration = configuration(0, 6);
        let mut bits = BitBuf::new();
        bits.push(false);
        bits.push(false);
        push_parameter_codes(&mut bits, new_configuration.method, false, 2, 0);
        let decoded = metadata(
            bits.as_slice(),
            bits.bit_len(),
            DialogEnhancementConfigurationUpdate::New(new_configuration),
            false,
            false,
        )
        .decode_data(bits.as_slice(), Some(previous_configuration))
        .unwrap()
        .unwrap();
        let DialogEnhancementParameterUpdate::New(parameters) = decoded.primary.parameters else {
            panic!("dependent update 应传输新参数")
        };
        assert!(!parameters.first_code_is_absolute());
        assert_eq!(parameters.mid_side_processing, Some(false));
        assert_eq!(parameters.parameter_channels(), 2);
        assert!(parameters.codes().iter().all(|&value| value == 0));
    }

    fn push_iframe_cross_two_data(
        bits: &mut BitBuf,
        configuration: DialogEnhancementConfiguration,
    ) {
        bits.push_bits(9, 5);
        push_parameter_codes(bits, configuration.method, true, 2, 0);
    }

    #[test]
    fn simulcast_has_its_own_data_without_repeating_position() {
        let configuration = configuration(1, 6);
        let mut bits = BitBuf::new();
        push_iframe_cross_two_data(&mut bits, configuration);
        bits.push(true);
        push_parameter_codes(&mut bits, configuration.method, true, 2, -3);

        for cut in 0..bits.bit_len() {
            assert!(
                metadata(
                    bits.as_slice(),
                    cut,
                    DialogEnhancementConfigurationUpdate::New(configuration),
                    true,
                    true,
                )
                .decode_data(bits.as_slice(), None)
                .is_err(),
                "截断在 simulcast bit {cut} 后必须失败"
            );
        }

        let decoded = metadata(
            bits.as_slice(),
            bits.bit_len(),
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            true,
        )
        .decode_data(bits.as_slice(), None)
        .unwrap()
        .unwrap();
        assert_eq!(
            decoded.primary.position,
            DialogEnhancementPositionUpdate::New(DialogEnhancementMixCoefficients {
                first_index: 9,
                second_index: None,
            })
        );
        let DialogEnhancementSimulcastData::Present(simulcast) = decoded.simulcast else {
            panic!("应解析 simulcast data")
        };
        assert_eq!(
            simulcast.position,
            DialogEnhancementPositionUpdate::NotApplicable
        );
        let DialogEnhancementParameterUpdate::New(parameters) = simulcast.parameters else {
            panic!("simulcast I-frame 应传输新参数")
        };
        assert_eq!(parameters.code(0, 0), Some(-3));

        let mut absent = BitBuf::new();
        push_iframe_cross_two_data(&mut absent, configuration);
        absent.push(false);
        let decoded = metadata(
            absent.as_slice(),
            absent.bit_len(),
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            true,
        )
        .decode_data(absent.as_slice(), None)
        .unwrap()
        .unwrap();
        assert_eq!(
            decoded.simulcast,
            DialogEnhancementSimulcastData::NotPresent
        );
    }

    #[test]
    fn zero_channel_data_is_empty_but_simulcast_gate_still_needs_its_flag() {
        let configuration = configuration(0, 0);
        let empty = BitBuf::new();
        let absent = DialogEnhancementMetadata::from_raw_parts(
            empty.as_slice(),
            false,
            DialogEnhancementConfigurationUpdate::NotPresent,
            0,
            0,
            None,
            false,
        )
        .unwrap();
        assert_eq!(absent.decode_data(empty.as_slice(), None).unwrap(), None);

        let decoded = metadata(
            empty.as_slice(),
            0,
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            false,
        )
        .decode_data(empty.as_slice(), None)
        .unwrap()
        .unwrap();
        assert_eq!(
            decoded.primary,
            DialogEnhancementDataBlock {
                position: DialogEnhancementPositionUpdate::NotApplicable,
                parameters: DialogEnhancementParameterUpdate::NotApplicable,
            }
        );

        assert!(matches!(
            metadata(
                empty.as_slice(),
                0,
                DialogEnhancementConfigurationUpdate::New(configuration),
                true,
                true,
            )
            .decode_data(empty.as_slice(), None),
            Err(DialogEnhancementDataError::Read(_))
        ));
    }

    #[test]
    fn rejects_truncated_huffman_fixed_fields_invalid_context_and_trailing_bits() {
        let mut one_bit = BitBuf::new();
        one_bit.push(false);
        assert!(matches!(
            metadata(
                one_bit.as_slice(),
                one_bit.bit_len(),
                DialogEnhancementConfigurationUpdate::New(configuration(0, 1)),
                true,
                false,
            )
            .decode_data(one_bit.as_slice(), None),
            Err(DialogEnhancementDataError::Huffman(HuffmanError::Read(_)))
        ));

        let empty = BitBuf::new();
        assert!(matches!(
            metadata(
                empty.as_slice(),
                0,
                DialogEnhancementConfigurationUpdate::New(configuration(1, 6)),
                true,
                false,
            )
            .decode_data(empty.as_slice(), None),
            Err(DialogEnhancementDataError::Read(_))
        ));

        let inconsistent = metadata(
            empty.as_slice(),
            0,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            true,
            false,
        );
        assert_eq!(
            inconsistent
                .decode_data(empty.as_slice(), Some(configuration(0, 0)))
                .unwrap_err(),
            DialogEnhancementDataError::InconsistentMetadata { bit_position: 0 }
        );

        let invalid = DialogEnhancementConfiguration {
            method: 4,
            max_gain: 0,
            channel_config: 0,
        };
        let dependent = metadata(
            empty.as_slice(),
            0,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            false,
        );
        assert!(matches!(
            dependent.decode_data(empty.as_slice(), Some(invalid)),
            Err(DialogEnhancementDataError::InvalidConfiguration { method: 4, .. })
        ));

        assert_eq!(
            metadata(
                one_bit.as_slice(),
                one_bit.bit_len(),
                DialogEnhancementConfigurationUpdate::New(configuration(0, 0)),
                true,
                false,
            )
            .decode_data(one_bit.as_slice(), None)
            .unwrap_err(),
            DialogEnhancementDataError::TrailingBits {
                bit_position: 0,
                remaining_bits: 1,
            }
        );
    }

    #[test]
    fn full_audio_parser_preserves_the_exact_iframe_context_for_data_decode() {
        let configuration = configuration(0, 0);
        let mut tools = BitBuf::new();
        tools.push(true);
        tools.push_bits(u32::from(configuration.method), 2);
        tools.push_bits(u32::from(configuration.max_gain), 2);
        tools.push_bits(u32::from(configuration.channel_config), 3);

        let mut payload = BitBuf::new();
        payload.push_bits(0, 15);
        payload.push(false);
        payload.push(false);
        payload.push(false);
        payload.push(false);
        payload.push(false);
        payload.push_bits(u32::try_from(tools.bit_len()).unwrap_or(u32::MAX), 7);
        payload.push(false);
        append_bits(&mut payload, &tools);
        payload.push(false);
        payload.byte_align();

        let context = SubstreamContext {
            sus_ver: 1,
            alternative: false,
            ajoc: true,
            channel_mode: None,
            b_iframe: Some(true),
            alternative_oamd: None,
        };
        let parsed = Ac4AudioSubstream::parse(payload.as_slice(), context).unwrap();
        assert_eq!(
            parsed.tools_metadata.dialog_enhancement.b_iframe(),
            Some(true)
        );
        let decoded = parsed
            .tools_metadata
            .dialog_enhancement
            .decode_data(payload.as_slice(), None)
            .unwrap()
            .unwrap();
        assert_eq!(decoded.configuration, configuration);
        assert_eq!(
            decoded.primary.parameters,
            DialogEnhancementParameterUpdate::NotApplicable
        );
        assert_eq!(
            decoded.simulcast,
            DialogEnhancementSimulcastData::NotSignaled
        );

        let mut state = DialogEnhancementState::new();
        let effective = state
            .decode_frame(parsed.tools_metadata.dialog_enhancement, payload.as_slice())
            .unwrap()
            .unwrap();
        assert_eq!(effective.configuration, configuration);
        assert_eq!(effective.primary.parameters, None);
        assert_eq!(state.configuration(), Some(configuration));
    }
}
