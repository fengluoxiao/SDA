//! Presentation DRC gain-set 熵解码与差分还原。
//!
//! 对应 `TS103190-1:v1.4.1:4.2.14.9`–`4.2.14.10`、`4.3.13.5`–`4.3.13.7`
//! 与附录 `A.5`。本模块只把 `DRC_HCB` 符号还原为逐 channel-group/subframe/band 的
//! 整数 dB₂ 码值，并保留 version 1+ 的扩展比特；不维护跨帧配置，不平滑或应用增益，也不
//! 修改 PCM。

use crate::huffman::{HuffmanError, tables};
use core::fmt;
use macindecode_ac4_bitstream::presentation_substream::{
    PresentationDrcFrameBits, PresentationDrcGainSet,
};
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

/// P2 表 69 扩展后的 `nr_drc_channels` 上限。
///
/// 规范变量名中的 channel 实际表示共享同一 DRC gain 的声道组，而非物理声道数。
pub const MAX_PRESENTATION_DRC_CHANNEL_GROUPS: usize = 4;
/// 表 163 中 `drc_gains_config = 3` 的最大参数频带数。
pub const MAX_PRESENTATION_DRC_BANDS: usize = 4;
/// 表 169 中一帧的最大 DRC subframe 数。
pub const MAX_PRESENTATION_DRC_SUBFRAMES: usize = 8;
/// 单个 gain-set 可还原的最大 gain 数：4 channel groups × 8 subframes × 4 bands。
pub const MAX_PRESENTATION_DRC_GAIN_VALUES: usize = 128;

/// 附录 A.5 表 A.62 的 `cb_off`。
const DRC_HCB_OFFSET: i16 = 127;

/// 解码 channel-dependent DRC gains 所需的规范派生形状。
///
/// `nr_drc_channels` 应按 P1 表 168 与 P2 表 69 从 presentation channel configuration
/// 派生，`nr_drc_subframes` 应按 P1 表 169 从单个 codec frame 的长度派生。配置 0、未知
/// version 以及只读取 extension 的场景不需要该上下文。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationDrcGainsContext {
    nr_drc_channels: u8,
    nr_drc_subframes: u8,
}

impl PresentationDrcGainsContext {
    /// 构造已经按规范派生的 gain 形状。
    ///
    /// `nr_drc_channels` 必须是表 168 与 P2 表 69 定义的 `1, 3, 4` 之一；
    /// `nr_drc_subframes` 必须是表 169 的
    /// `1, 2, 3, 4, 6, 8` 之一。不在上述集合内时返回 `None`，不会静默钳位。
    #[must_use]
    pub const fn new(nr_drc_channels: u8, nr_drc_subframes: u8) -> Option<Self> {
        if !matches!(nr_drc_channels, 1 | 3 | 4)
            || !matches!(nr_drc_subframes, 1 | 2 | 3 | 4 | 6 | 8)
        {
            return None;
        }
        Some(Self {
            nr_drc_channels,
            nr_drc_subframes,
        })
    }

    /// 规范变量 `nr_drc_channels`，即 DRC channel-group 数。
    #[must_use]
    pub const fn nr_drc_channels(self) -> u8 {
        self.nr_drc_channels
    }

    /// 规范变量 `nr_drc_subframes`。
    #[must_use]
    pub const fn nr_drc_subframes(self) -> u8 {
        self.nr_drc_subframes
    }
}

/// Presentation DRC gain-set 解码失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationDrcGainsError {
    /// 读取固定宽度字段时越过 gain-set envelope。
    Read(ReadError),
    /// `DRC_HCB` 码字截断或生成表损坏。
    Huffman(HuffmanError),
    /// channel-dependent gain-set 缺少规范派生的形状上下文。
    MissingContext {
        /// 2 比特 `drc_gains_config`。
        gains_configuration: u8,
    },
    /// 手工构造的 gain-set 使用了规范未定义的 gains configuration。
    UnsupportedConfiguration {
        /// 实际配置码值。
        gains_configuration: u8,
    },
    /// 还原值数量超过规范固定上限。
    CapacityExceeded {
        /// 尝试写入的数量。
        declared: usize,
        /// 实现与规范共同上限。
        limit: usize,
    },
    /// version 0 的已知 `drc_gains()` 后仍有多余比特。
    TrailingBits {
        /// 首个多余比特在 presentation payload 内的偏移。
        bit_position: u64,
        /// gain-set envelope 中尚未消费的比特数。
        remaining_bits: u64,
    },
}

impl fmt::Display for PresentationDrcGainsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Read(error) => {
                write!(formatter, "Failed to read presentation DRC gains: {error}")
            }
            Self::Huffman(error) => write!(
                formatter,
                "Failed to decode presentation DRC gains: {error}"
            ),
            Self::MissingContext {
                gains_configuration,
            } => write!(
                formatter,
                "Presentation DRC gains configuration {gains_configuration} requires channel and subframe context"
            ),
            Self::UnsupportedConfiguration {
                gains_configuration,
            } => write!(
                formatter,
                "Presentation DRC gains configuration {gains_configuration} is outside the 2-bit syntax"
            ),
            Self::CapacityExceeded { declared, limit } => write!(
                formatter,
                "Presentation DRC gain count {declared} exceeds limit {limit}"
            ),
            Self::TrailingBits {
                bit_position,
                remaining_bits,
            } => write!(
                formatter,
                "Presentation DRC version 0 gain-set has {remaining_bits} trailing bits at bit offset {bit_position}"
            ),
        }
    }
}

impl core::error::Error for PresentationDrcGainsError {}

impl From<ReadError> for PresentationDrcGainsError {
    fn from(error: ReadError) -> Self {
        Self::Read(error)
    }
}

impl From<HuffmanError> for PresentationDrcGainsError {
    fn from(error: HuffmanError) -> Self {
        Self::Huffman(error)
    }
}

/// 已差分还原的 `drc_gains()` 整数码值。
///
/// [`gain`](Self::gain) 返回 `drc_gain[ch][sf][band]`，单位是规范定义的整数 dB₂；首值
/// 等于 `drc_gain_val - 64`，其余值按表 75 的 reference reset 顺序累加 Huffman diff。
/// 规范的 1 dB₂ 对应幅度因子 `2^(1/6)`；这里不做平滑、限幅、单位换算或增益应用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationDrcGains {
    values: [i16; MAX_PRESENTATION_DRC_GAIN_VALUES],
    len: usize,
    gains_configuration: u8,
    nr_drc_channels: u8,
    nr_drc_subframes: u8,
    nr_drc_bands: u8,
    initial_gain_code: u8,
}

impl PresentationDrcGains {
    fn empty(
        gains_configuration: u8,
        nr_drc_channels: u8,
        nr_drc_subframes: u8,
        nr_drc_bands: u8,
        initial_gain_code: u8,
    ) -> Self {
        Self {
            values: [0; MAX_PRESENTATION_DRC_GAIN_VALUES],
            len: 0,
            gains_configuration,
            nr_drc_channels,
            nr_drc_subframes,
            nr_drc_bands,
            initial_gain_code,
        }
    }

    fn push(&mut self, value: i16) -> Result<(), PresentationDrcGainsError> {
        let declared = self.len.saturating_add(1);
        let Some(slot) = self.values.get_mut(self.len) else {
            return Err(PresentationDrcGainsError::CapacityExceeded {
                declared,
                limit: MAX_PRESENTATION_DRC_GAIN_VALUES,
            });
        };
        *slot = value;
        self.len = declared;
        Ok(())
    }

    /// 2 比特 `drc_gains_config`。
    #[must_use]
    pub const fn gains_configuration(&self) -> u8 {
        self.gains_configuration
    }

    /// 规范变量 `nr_drc_channels`，即共享 gain 的 channel-group 数。
    #[must_use]
    pub const fn nr_drc_channels(&self) -> u8 {
        self.nr_drc_channels
    }

    /// 规范变量 `nr_drc_subframes`。
    #[must_use]
    pub const fn nr_drc_subframes(&self) -> u8 {
        self.nr_drc_subframes
    }

    /// 表 163 从 `drc_gains_config` 派生的 `nr_drc_bands`。
    #[must_use]
    pub const fn nr_drc_bands(&self) -> u8 {
        self.nr_drc_bands
    }

    /// 首个 7 比特 `drc_gain_val` 原始码值。
    #[must_use]
    pub const fn initial_gain_code(&self) -> u8 {
        self.initial_gain_code
    }

    /// 按表 75 的传输顺序取得全部已还原值：channel → band → subframe。
    #[must_use]
    pub fn values(&self) -> &[i16] {
        self.values.get(..self.len).unwrap_or(&[])
    }

    /// 取得 `drc_gain[channel][subframe][band]`。
    #[must_use]
    pub fn gain(&self, channel: usize, subframe: usize, band: usize) -> Option<i16> {
        let channels = usize::from(self.nr_drc_channels);
        let subframes = usize::from(self.nr_drc_subframes);
        let bands = usize::from(self.nr_drc_bands);
        if channel >= channels || subframe >= subframes || band >= bands {
            return None;
        }
        let channel_stride = bands.checked_mul(subframes)?;
        let channel_offset = channel.checked_mul(channel_stride)?;
        let band_offset = band.checked_mul(subframes)?;
        let index = channel_offset
            .checked_add(band_offset)?
            .checked_add(subframe)?;
        self.values.get(index).copied()
    }
}

/// 一个 gain-set 的已知语法与未来版本扩展。
///
/// version 0/1 的 [`gains`](Self::gains) 为 `Some`；version 2/3 不含当前版本 gains，故为
/// `None`。version 0 的 [`extension`](Self::extension) 为 `None`；version 1–3 为 `Some`，
/// 即使扩展长度为零也保持 presence 可区分。
#[derive(Debug, Clone, Copy)]
pub struct PresentationDrcDecodedGainSet<'a> {
    /// version 0/1 中已解析并差分还原的 `drc_gains()`。
    pub gains: Option<PresentationDrcGains>,
    /// version 1+ 的 `drc2_bits` 有界视图。
    pub extension: Option<PresentationDrcFrameBits<'a>>,
}

impl<'a, 'b> PartialEq<PresentationDrcDecodedGainSet<'b>> for PresentationDrcDecodedGainSet<'a> {
    fn eq(&self, other: &PresentationDrcDecodedGainSet<'b>) -> bool {
        self.gains == other.gains && self.extension == other.extension
    }
}

impl Eq for PresentationDrcDecodedGainSet<'_> {}

/// 为 bitstream 层保留的 DRC gain-set envelope 提供 Huffman gains 解码。
///
/// 导入本 trait 后可继续以 `gain_set.decode_gains(...)` 的形式调用；trait 位于 decode
/// crate，避免让纯语法 crate 反向依赖规范表。
pub trait PresentationDrcGainSetExt<'a> {
    /// 解码当前 gain-set 的已知 `drc_gains()` 并定出 version 1+ 扩展边界。
    ///
    /// `drc_gains_config == 0` 只携带一个 wideband gain，`context` 可为 `None`。配置
    /// 1–3 必须传入按表 168/169 与 P2 表 69 派生的上下文。version 2/3 没有当前规范 gains，
    /// 同样不需要上下文，完整 body 作为 extension 返回。
    ///
    /// # Errors
    ///
    /// 固定字段或 Huffman 码字截断、配置 1–3 缺少上下文，或 version 0 解完规定数量后仍有
    /// 尾随比特时返回错误。失败不会修改任何跨帧状态。
    fn decode_gains(
        self,
        context: Option<PresentationDrcGainsContext>,
    ) -> Result<PresentationDrcDecodedGainSet<'a>, PresentationDrcGainsError>;
}

impl<'a> PresentationDrcGainSetExt<'a> for PresentationDrcGainSet<'a> {
    fn decode_gains(
        self,
        context: Option<PresentationDrcGainsContext>,
    ) -> Result<PresentationDrcDecodedGainSet<'a>, PresentationDrcGainsError> {
        let mut reader = self.payload.reader()?;
        let version = u8::try_from(reader.read_bits(2)?).unwrap_or(u8::MAX);
        if version >= 2 {
            return Ok(PresentationDrcDecodedGainSet {
                gains: None,
                extension: Some(remaining_view(&reader, self.payload)?),
            });
        }

        let nr_drc_bands = match self.gains_configuration {
            0 | 1 => 1,
            2 => 2,
            3 => 4,
            gains_configuration => {
                return Err(PresentationDrcGainsError::UnsupportedConfiguration {
                    gains_configuration,
                });
            }
        };
        let (nr_drc_channels, nr_drc_subframes) = if self.gains_configuration == 0 {
            (1, 1)
        } else {
            let context = context.ok_or(PresentationDrcGainsError::MissingContext {
                gains_configuration: self.gains_configuration,
            })?;
            (context.nr_drc_channels, context.nr_drc_subframes)
        };

        let initial_gain_code = u8::try_from(reader.read_bits(7)?).unwrap_or(u8::MAX);
        let initial_gain = i16::from(initial_gain_code).saturating_sub(64);
        let mut gains = PresentationDrcGains::empty(
            self.gains_configuration,
            nr_drc_channels,
            nr_drc_subframes,
            nr_drc_bands,
            initial_gain_code,
        );
        let mut reference = initial_gain;
        for channel in 0..usize::from(nr_drc_channels) {
            let mut channel_first = reference;
            for band in 0..usize::from(nr_drc_bands) {
                let mut band_first = reference;
                for subframe in 0..usize::from(nr_drc_subframes) {
                    let first = channel == 0 && band == 0 && subframe == 0;
                    let value = if first {
                        initial_gain
                    } else {
                        let symbol = tables::DRC_HCB.decode(&mut reader)?;
                        let difference = i16::try_from(symbol)
                            .unwrap_or(i16::MAX)
                            .saturating_sub(DRC_HCB_OFFSET);
                        reference.saturating_add(difference)
                    };
                    gains.push(value)?;
                    reference = value;
                    if subframe == 0 {
                        band_first = value;
                        if band == 0 {
                            channel_first = value;
                        }
                    }
                }
                reference = band_first;
            }
            reference = channel_first;
        }

        let extension = if version == 0 {
            if reader.remaining_bits() != 0 {
                return Err(PresentationDrcGainsError::TrailingBits {
                    bit_position: reader.bit_position(),
                    remaining_bits: reader.remaining_bits(),
                });
            }
            None
        } else {
            Some(remaining_view(&reader, self.payload)?)
        };
        Ok(PresentationDrcDecodedGainSet {
            gains: Some(gains),
            extension,
        })
    }
}

fn remaining_view<'a>(
    reader: &BitReader<'a>,
    payload: PresentationDrcFrameBits<'a>,
) -> Result<PresentationDrcFrameBits<'a>, PresentationDrcGainsError> {
    Ok(PresentationDrcFrameBits::new(
        payload.source(),
        reader.bit_position(),
        reader.remaining_bits(),
    )?)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::testutil::BitBuf;
    use macindecode_ac4_bitstream::presentation_substream::PresentationAddDataBits;

    fn gain_set<'a>(
        bits: &'a BitBuf,
        bit_len: usize,
        gains_configuration: u8,
        version: u8,
    ) -> PresentationDrcGainSet<'a> {
        PresentationDrcGainSet {
            decoder_mode_id: 0,
            gains_configuration,
            size_value_offset: 0,
            version,
            payload: PresentationAddDataBits::new(
                bits.as_slice(),
                0,
                u64::try_from(bit_len).unwrap_or(u64::MAX),
            )
            .unwrap(),
        }
    }

    #[test]
    fn context_accepts_only_normative_shape_bounds() {
        assert_eq!(
            PresentationDrcGainsContext::new(1, 1)
                .unwrap()
                .nr_drc_channels(),
            1
        );
        assert_eq!(
            PresentationDrcGainsContext::new(4, 8)
                .unwrap()
                .nr_drc_subframes(),
            8
        );
        assert_eq!(PresentationDrcGainsContext::new(0, 1), None);
        assert_eq!(PresentationDrcGainsContext::new(2, 1), None);
        assert_eq!(PresentationDrcGainsContext::new(5, 1), None);
        assert_eq!(PresentationDrcGainsContext::new(1, 0), None);
        assert_eq!(PresentationDrcGainsContext::new(1, 5), None);
    }

    #[test]
    fn decodes_fixed_gain_and_preserves_version_one_extension() {
        let mut version_zero = BitBuf::new();
        version_zero.push_bits(0, 2);
        version_zero.push_bits(64, 7);
        let decoded = gain_set(&version_zero, version_zero.bit_len(), 0, 0)
            .decode_gains(None)
            .unwrap();
        let gains = decoded.gains.unwrap();
        assert_eq!(gains.initial_gain_code(), 64);
        assert_eq!(gains.values(), [0]);
        assert_eq!(gains.gain(0, 0, 0), Some(0));
        assert_eq!(gains.gain(1, 0, 0), None);
        assert_eq!(decoded.extension, None);

        let mut version_one = BitBuf::new();
        version_one.push_bits(1, 2);
        version_one.push_bits(63, 7);
        let extension_offset = version_one.bit_len();
        version_one.push_bits(0b101, 3);
        let decoded = gain_set(&version_one, version_one.bit_len(), 0, 1)
            .decode_gains(None)
            .unwrap();
        assert_eq!(decoded.gains.unwrap().gain(0, 0, 0), Some(-1));
        let extension = decoded.extension.unwrap();
        assert_eq!(
            extension.bit_offset(),
            u64::try_from(extension_offset).unwrap_or(u64::MAX)
        );
        assert_eq!(extension.len_bits(), 3);
        assert_eq!(
            extension.iter().collect::<std::vec::Vec<_>>(),
            [true, false, true]
        );
    }

    #[test]
    fn decodes_huffman_gains_with_normative_reference_resets() {
        let mut bits = BitBuf::new();
        bits.push_bits(1, 2);
        bits.push_bits(64, 7);
        for difference in 1u16..=11 {
            bits.push_symbol(&tables::DRC_HCB, DRC_HCB_OFFSET as u16 + difference);
        }
        let extension_offset = bits.bit_len();
        bits.push_bits(0b110, 3);
        let context = PresentationDrcGainsContext::new(3, 2).unwrap();
        let decoded = gain_set(&bits, bits.bit_len(), 2, 1)
            .decode_gains(Some(context))
            .unwrap();
        let gains = decoded.gains.unwrap();

        assert_eq!(gains.gains_configuration(), 2);
        assert_eq!(gains.nr_drc_channels(), 3);
        assert_eq!(gains.nr_drc_subframes(), 2);
        assert_eq!(gains.nr_drc_bands(), 2);
        assert_eq!(gains.values(), [0, 1, 2, 5, 4, 9, 10, 17, 12, 21, 22, 33]);
        assert_eq!(gains.gain(0, 0, 0), Some(0));
        assert_eq!(gains.gain(0, 1, 0), Some(1));
        assert_eq!(gains.gain(0, 0, 1), Some(2));
        assert_eq!(gains.gain(0, 1, 1), Some(5));
        assert_eq!(gains.gain(1, 0, 0), Some(4));
        assert_eq!(gains.gain(1, 1, 1), Some(17));
        assert_eq!(gains.gain(2, 0, 0), Some(12));
        assert_eq!(gains.gain(2, 1, 1), Some(33));
        let extension = decoded.extension.unwrap();
        assert_eq!(
            extension.bit_offset(),
            u64::try_from(extension_offset).unwrap_or(u64::MAX)
        );
        assert_eq!(extension.len_bits(), 3);
        assert_eq!(
            extension.iter().collect::<std::vec::Vec<_>>(),
            [true, true, false]
        );
    }

    #[test]
    fn decodes_gain_shape_at_capacity() {
        let mut bits = BitBuf::new();
        bits.push_bits(0, 2);
        bits.push_bits(64, 7);
        for _ in 1..MAX_PRESENTATION_DRC_GAIN_VALUES {
            bits.push_symbol(&tables::DRC_HCB, DRC_HCB_OFFSET as u16);
        }
        let context = PresentationDrcGainsContext::new(4, 8).unwrap();
        let gains = gain_set(&bits, bits.bit_len(), 3, 0)
            .decode_gains(Some(context))
            .unwrap()
            .gains
            .unwrap();

        assert_eq!(gains.values().len(), MAX_PRESENTATION_DRC_GAIN_VALUES);
        assert!(gains.values().iter().all(|&gain| gain == 0));
        assert_eq!(gains.gain(3, 7, 3), Some(0));
        assert_eq!(gains.gain(4, 0, 0), None);
    }

    #[test]
    fn preserves_unknown_version_body_without_context() {
        let mut bits = BitBuf::new();
        bits.push_bits(2, 2);
        let extension_offset = bits.bit_len();
        bits.push_bits(0b1101, 4);
        let decoded = gain_set(&bits, bits.bit_len(), 3, 2)
            .decode_gains(None)
            .unwrap();

        assert_eq!(decoded.gains, None);
        let extension = decoded.extension.unwrap();
        assert_eq!(
            extension.bit_offset(),
            u64::try_from(extension_offset).unwrap_or(u64::MAX)
        );
        assert_eq!(extension.len_bits(), 4);
        assert_eq!(
            extension.iter().collect::<std::vec::Vec<_>>(),
            [true, true, false, true]
        );
    }

    #[test]
    fn rejects_missing_context_truncated_codeword_and_version_zero_tail() {
        let mut missing = BitBuf::new();
        missing.push_bits(0, 2);
        missing.push_bits(64, 7);
        assert_eq!(
            gain_set(&missing, missing.bit_len(), 1, 0)
                .decode_gains(None)
                .unwrap_err(),
            PresentationDrcGainsError::MissingContext {
                gains_configuration: 1,
            }
        );

        let mut truncated = BitBuf::new();
        truncated.push_bits(0, 2);
        truncated.push_bits(64, 7);
        truncated.push_symbol(&tables::DRC_HCB, 0);
        let truncated_len = truncated.bit_len().saturating_sub(1);
        assert!(matches!(
            gain_set(&truncated, truncated_len, 1, 0)
                .decode_gains(Some(PresentationDrcGainsContext::new(1, 2).unwrap())),
            Err(PresentationDrcGainsError::Huffman(HuffmanError::Read(_)))
        ));

        let mut trailing = BitBuf::new();
        trailing.push_bits(0, 2);
        trailing.push_bits(64, 7);
        trailing.push(true);
        assert_eq!(
            gain_set(&trailing, trailing.bit_len(), 1, 0)
                .decode_gains(Some(PresentationDrcGainsContext::new(1, 1).unwrap()))
                .unwrap_err(),
            PresentationDrcGainsError::TrailingBits {
                bit_position: 9,
                remaining_bits: 1,
            }
        );
    }
}
