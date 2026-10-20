//! Dialogue-enhancement 跨帧有效配置与参数索引状态。
//!
//! 对应 `TS103190-1:v1.4.1:4.2.14.13` 与 `4.3.14.4`–`4.3.14.5`。本模块只还原
//! `de_par` 量化索引及 keep 语义；不做参数反量化或信号处理。

use super::{
    DIALOG_ENHANCEMENT_PARAMETER_BANDS, DialogEnhancementConfiguration,
    DialogEnhancementConfigurationUpdate, DialogEnhancementDataError, DialogEnhancementDecodedData,
    DialogEnhancementMetadata, DialogEnhancementMetadataExt, DialogEnhancementMixCoefficients,
    DialogEnhancementParameterData, DialogEnhancementParameterUpdate,
    DialogEnhancementPositionUpdate, DialogEnhancementSimulcastData,
    MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES,
};
use core::fmt;

/// 已按 `ref_val`/`de_par_prev` 还原的 dialogue-enhancement 参数量化索引。
///
/// [`indices`](Self::indices) 按 parameter channel → band 保存。这里仍是规范中的整数索引，
/// 不是反量化后的增益或滤波参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogEnhancementEffectiveParameterData {
    indices: [i32; MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES],
    parameter_channels: u8,
    /// 当前有效参数是否作用于双声道 M/S 表示的 Mid 信号。
    pub mid_side_processing: bool,
    /// hybrid method 当前有效的 5 比特 signal contribution。
    pub signal_contribution: Option<u8>,
}

impl DialogEnhancementEffectiveParameterData {
    /// 当前有效参数集合的声道数；双声道 M/S 数据为 1。
    #[must_use]
    pub const fn parameter_channels(self) -> u8 {
        self.parameter_channels
    }

    /// 按 parameter channel → band 顺序取得全部量化索引。
    #[must_use]
    pub fn indices(&self) -> &[i32] {
        let len =
            usize::from(self.parameter_channels).saturating_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS);
        self.indices.get(..len).unwrap_or(&[])
    }

    /// 取得一个 parameter channel、频带位置的有效量化索引。
    #[must_use]
    pub fn index(&self, channel: usize, band: usize) -> Option<i32> {
        if channel >= usize::from(self.parameter_channels)
            || band >= DIALOG_ENHANCEMENT_PARAMETER_BANDS
        {
            return None;
        }
        let index = channel
            .checked_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS)?
            .checked_add(band)?;
        self.indices.get(index).copied()
    }
}

/// 一份 primary 或 simulcast `de_data()` 在当前帧生效的 metadata。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogEnhancementEffectiveDataBlock {
    /// cross-channel primary data 的当前有效 panning；语法不适用时为 `None`。
    pub position: Option<DialogEnhancementMixCoefficients>,
    /// 当前有效参数索引；`de_nr_channels == 0` 时为 `None`。
    pub parameters: Option<DialogEnhancementEffectiveParameterData>,
}

/// P2 channel mode 13/14 在当前帧生效的独立 core-decoding DE data。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogEnhancementEffectiveSimulcastData {
    /// 当前 channel mode 不传输 simulcast gate。
    NotSignaled,
    /// gate 适用，但当前帧没有 separate core data。
    NotPresent,
    /// 当前帧存在 separate core data。
    Present(DialogEnhancementEffectiveDataBlock),
}

/// 一帧已还原 keep 与 differential 语义的 dialogue-enhancement metadata。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogEnhancementEffectiveData {
    /// 当前物理 substream 的 `b_audio_ndot`。
    pub b_iframe: bool,
    /// 当前有效 DE 配置。
    pub configuration: DialogEnhancementConfiguration,
    /// full/普通解码的当前有效 data。
    pub primary: DialogEnhancementEffectiveDataBlock,
    /// 可选的 separate core-decoding data。
    pub simulcast: DialogEnhancementEffectiveSimulcastData,
}

/// 延续 dialogue-enhancement metadata 状态失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogEnhancementStateError {
    /// 帧内语法或 Huffman 解码失败。
    Data(DialogEnhancementDataError),
    /// 当前 metadata 没有精确的物理 substream `b_iframe` 上下文。
    MissingFrameContext {
        /// tools body 起始 bit offset。
        bit_position: u64,
    },
    /// `de_keep_pos_flag` 要求沿用 panning，但上一帧没有兼容值。
    MissingPosition {
        /// tools body 起始 bit offset。
        bit_position: u64,
    },
    /// `de_keep_data_flag` 要求沿用参数，但没有兼容的已传输参数集。
    MissingParameters {
        /// `true` 表示 separate core/simulcast data，`false` 表示 primary data。
        simulcast: bool,
        /// tools body 起始 bit offset。
        bit_position: u64,
    },
    /// 解码后的更新形态或固定字段与当前配置不一致。
    InconsistentData {
        /// 不一致的字段或约束。
        what: &'static str,
        /// tools body 起始 bit offset。
        bit_position: u64,
    },
    /// differential 相加超出 `i32` 可表示范围。
    ParameterIndexOverflow {
        /// `true` 表示 separate core/simulcast data，`false` 表示 primary data。
        simulcast: bool,
        /// parameter channel。
        channel: u8,
        /// 参数 band。
        band: u8,
        /// 相加前的 `ref_val` 或 `de_par_prev`。
        reference: i32,
        /// 当前 Huffman differential。
        differential: i16,
    },
}

impl fmt::Display for DialogEnhancementStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Data(error) => write!(formatter, "{error}"),
            Self::MissingFrameContext { bit_position } => write!(
                formatter,
                "Dialogue-enhancement state needs an exact physical-substream b_iframe at bit offset {bit_position}"
            ),
            Self::MissingPosition { bit_position } => write!(
                formatter,
                "Dialogue-enhancement position keep has no compatible previous value at bit offset {bit_position}"
            ),
            Self::MissingParameters {
                simulcast,
                bit_position,
            } => write!(
                formatter,
                "Dialogue-enhancement {} parameter keep has no compatible previous value at bit offset {bit_position}",
                if simulcast { "simulcast" } else { "primary" }
            ),
            Self::InconsistentData { what, bit_position } => write!(
                formatter,
                "Dialogue-enhancement data is inconsistent ({what}) at bit offset {bit_position}"
            ),
            Self::ParameterIndexOverflow {
                simulcast,
                channel,
                band,
                reference,
                differential,
            } => write!(
                formatter,
                "Dialogue-enhancement {} parameter [{channel}][{band}] overflows: {reference} + {differential}",
                if simulcast { "simulcast" } else { "primary" }
            ),
        }
    }
}

impl core::error::Error for DialogEnhancementStateError {}

impl From<DialogEnhancementDataError> for DialogEnhancementStateError {
    fn from(error: DialogEnhancementDataError) -> Self {
        Self::Data(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DialogEnhancementPositionHistory {
    coefficients: DialogEnhancementMixCoefficients,
    channel_config: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DialogEnhancementParameterSnapshot {
    configuration: DialogEnhancementConfiguration,
    data: DialogEnhancementEffectiveParameterData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct DialogEnhancementParameterHistory {
    // `de_keep_data_flag` repeats the latest transmitted parameter data, not merely the previous
    // frame. Keep this across dependent inactive frames.
    latest: Option<DialogEnhancementParameterSnapshot>,
    // `de_par_prev` is explicitly the corresponding parameter set in the previous frame. An
    // inactive frame therefore clears only this differential base.
    previous: Option<DialogEnhancementParameterSnapshot>,
}

/// 一个物理 audio substream 的 dialogue-enhancement metadata 状态。
///
/// 调用方必须按物理 `substream_index` 分别持有本类型；它不能在 presentation 或不同 audio
/// substream 间共享。dependent frame 会延续 configuration、panning、primary parameters 与
/// separate core/simulcast parameters；primary 与 simulcast 的 `de_par_prev` 相互独立。
/// I-frame 从空状态求值，成功后整体替换历史。
///
/// [`decode_frame`](Self::decode_frame) 只在完整帧内解码和所有 keep/differential 还原都成功后
/// 提交候选状态。失败不修改本类型；如果失败意味着码流连续性已经丢失，调用方应在处理下一帧
/// 前显式 [`reset`](Self::reset)。本类型不反量化参数、不执行 DE，也不修改 PCM。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DialogEnhancementState {
    configuration: Option<DialogEnhancementConfiguration>,
    position: Option<DialogEnhancementPositionHistory>,
    primary: DialogEnhancementParameterHistory,
    simulcast: DialogEnhancementParameterHistory,
}

impl DialogEnhancementState {
    /// 创建没有任何物理 substream 历史的状态。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            configuration: None,
            position: None,
            primary: DialogEnhancementParameterHistory {
                latest: None,
                previous: None,
            },
            simulcast: DialogEnhancementParameterHistory {
                latest: None,
                previous: None,
            },
        }
    }

    /// 最近一次成功提交、可供 dependent frame 沿用的配置。
    #[must_use]
    pub const fn configuration(self) -> Option<DialogEnhancementConfiguration> {
        self.configuration
    }

    /// 上一成功帧可由 `de_keep_pos_flag` 沿用的 panning。
    #[must_use]
    pub const fn position(self) -> Option<DialogEnhancementMixCoefficients> {
        match self.position {
            Some(position) => Some(position.coefficients),
            None => None,
        }
    }

    /// 最近一次成功传输的 primary 参数集，可由 `de_keep_data_flag` 沿用。
    #[must_use]
    pub const fn primary_parameters(self) -> Option<DialogEnhancementEffectiveParameterData> {
        match self.primary.latest {
            Some(parameters) => Some(parameters.data),
            None => None,
        }
    }

    /// 最近一次成功传输的 simulcast 参数集，可由 separate core data 的 keep flag 沿用。
    #[must_use]
    pub const fn simulcast_parameters(self) -> Option<DialogEnhancementEffectiveParameterData> {
        match self.simulcast.latest {
            Some(parameters) => Some(parameters.data),
            None => None,
        }
    }

    /// 在 seek、换源、物理拓扑变化、不连续或丢帧后清空全部 DE 历史。
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// 解码并事务性应用一个完整物理 audio substream 的 DE metadata。
    ///
    /// `metadata` 必须来自以同一 `payload` 解析出的 [`DialogEnhancementMetadata`]。DE 缺席时
    /// 返回 `Ok(None)`；dependent 缺席帧保留 configuration 与 latest-transmitted 参数，但会按
    /// P1 `4.3.14.5.3` 使下一帧 differential 的 `de_par_prev` 归零。I-frame 缺席会清空状态。
    /// configuration 更新时，dependent differential 会按 L/R/C 对应关系迁移上一帧参数；上一帧
    /// 未使用的声道以零为基准。panning keep 与 parameter keep 仍要求其各自的完整映射兼容；仅
    /// `de_max_gain` 改变不影响兼容性。
    ///
    /// # Errors
    ///
    /// 除帧内 data 解码错误外，缺少精确 `b_iframe`、keep 没有兼容历史、更新形态不一致或索引
    /// 相加溢出时返回错误。任何失败都不会修改状态。
    pub fn decode_frame(
        &mut self,
        metadata: DialogEnhancementMetadata,
        payload: &[u8],
    ) -> Result<Option<DialogEnhancementEffectiveData>, DialogEnhancementStateError> {
        let bit_position = metadata.unparsed_body_bit_offset();
        let b_iframe = metadata
            .b_iframe()
            .ok_or(DialogEnhancementStateError::MissingFrameContext { bit_position })?;
        let mut next = if b_iframe { Self::new() } else { *self };

        if !metadata.data_present {
            if metadata.configuration != DialogEnhancementConfigurationUpdate::NotPresent
                || metadata.unparsed_body_len_bits() != 0
            {
                return Err(DialogEnhancementStateError::InconsistentData {
                    what: "DE absence carries configuration or data bits",
                    bit_position,
                });
            }
            next.position = None;
            next.primary.previous = None;
            next.simulcast.previous = None;
            *self = next;
            return Ok(None);
        }

        let decoded = metadata.decode_data(payload, next.configuration)?.ok_or(
            DialogEnhancementStateError::InconsistentData {
                what: "active metadata decoded as absent",
                bit_position,
            },
        )?;
        let effective = next.resolve_decoded(decoded, bit_position)?;
        *self = next;
        Ok(Some(effective))
    }

    fn resolve_decoded(
        &mut self,
        decoded: DialogEnhancementDecodedData,
        bit_position: u64,
    ) -> Result<DialogEnhancementEffectiveData, DialogEnhancementStateError> {
        if decoded.b_iframe && self.configuration.is_some() {
            return Err(DialogEnhancementStateError::InconsistentData {
                what: "I-frame candidate retained an earlier configuration",
                bit_position,
            });
        }

        self.configuration = Some(decoded.configuration);

        let primary_position = resolve_position(
            decoded.primary.position,
            decoded.configuration,
            decoded.b_iframe,
            false,
            Some(&mut self.position),
            bit_position,
        )?;
        let primary_parameters = resolve_parameters(
            decoded.primary.parameters,
            decoded.configuration,
            decoded.b_iframe,
            false,
            &mut self.primary,
            bit_position,
        )?;
        let primary = DialogEnhancementEffectiveDataBlock {
            position: primary_position,
            parameters: primary_parameters,
        };

        let simulcast = match decoded.simulcast {
            DialogEnhancementSimulcastData::NotSignaled => {
                self.simulcast.previous = None;
                DialogEnhancementEffectiveSimulcastData::NotSignaled
            }
            DialogEnhancementSimulcastData::NotPresent => {
                self.simulcast.previous = None;
                DialogEnhancementEffectiveSimulcastData::NotPresent
            }
            DialogEnhancementSimulcastData::Present(data) => {
                let position = resolve_position(
                    data.position,
                    decoded.configuration,
                    decoded.b_iframe,
                    true,
                    None,
                    bit_position,
                )?;
                let parameters = resolve_parameters(
                    data.parameters,
                    decoded.configuration,
                    decoded.b_iframe,
                    true,
                    &mut self.simulcast,
                    bit_position,
                )?;
                DialogEnhancementEffectiveSimulcastData::Present(
                    DialogEnhancementEffectiveDataBlock {
                        position,
                        parameters,
                    },
                )
            }
        };

        Ok(DialogEnhancementEffectiveData {
            b_iframe: decoded.b_iframe,
            configuration: decoded.configuration,
            primary,
            simulcast,
        })
    }
}

fn resolve_position(
    update: DialogEnhancementPositionUpdate,
    configuration: DialogEnhancementConfiguration,
    b_iframe: bool,
    simulcast: bool,
    history: Option<&mut Option<DialogEnhancementPositionHistory>>,
    bit_position: u64,
) -> Result<Option<DialogEnhancementMixCoefficients>, DialogEnhancementStateError> {
    let applicable =
        !simulcast && matches!(configuration.method, 1 | 3) && configuration.channel_count() > 1;
    if !applicable {
        if update != DialogEnhancementPositionUpdate::NotApplicable {
            return Err(DialogEnhancementStateError::InconsistentData {
                what: "panning update outside its method/channel gate",
                bit_position,
            });
        }
        if let Some(history) = history {
            *history = None;
        }
        return Ok(None);
    }

    let Some(history) = history else {
        return Err(DialogEnhancementStateError::InconsistentData {
            what: "primary panning has no state slot",
            bit_position,
        });
    };
    let effective = match update {
        DialogEnhancementPositionUpdate::NotApplicable => {
            return Err(DialogEnhancementStateError::InconsistentData {
                what: "required primary panning is not applicable",
                bit_position,
            });
        }
        DialogEnhancementPositionUpdate::KeepPrevious => {
            if b_iframe {
                return Err(DialogEnhancementStateError::InconsistentData {
                    what: "I-frame keeps primary panning",
                    bit_position,
                });
            }
            let previous =
                history.ok_or(DialogEnhancementStateError::MissingPosition { bit_position })?;
            if previous.channel_config != configuration.channel_config {
                return Err(DialogEnhancementStateError::MissingPosition { bit_position });
            }
            previous.coefficients
        }
        DialogEnhancementPositionUpdate::New(coefficients) => {
            let second_matches = match configuration.channel_count() {
                2 => coefficients.second_index.is_none(),
                3 => coefficients.second_index.is_some(),
                _ => false,
            };
            if coefficients.first_index > 31
                || coefficients.second_index.is_some_and(|index| index > 31)
                || !second_matches
            {
                return Err(DialogEnhancementStateError::InconsistentData {
                    what: "panning indices do not match their 5-bit/channel shape",
                    bit_position,
                });
            }
            coefficients
        }
    };
    *history = Some(DialogEnhancementPositionHistory {
        coefficients: effective,
        channel_config: configuration.channel_config,
    });
    Ok(Some(effective))
}

fn resolve_parameters(
    update: DialogEnhancementParameterUpdate,
    configuration: DialogEnhancementConfiguration,
    b_iframe: bool,
    simulcast: bool,
    history: &mut DialogEnhancementParameterHistory,
    bit_position: u64,
) -> Result<Option<DialogEnhancementEffectiveParameterData>, DialogEnhancementStateError> {
    if configuration.channel_count() == 0 {
        if update != DialogEnhancementParameterUpdate::NotApplicable {
            return Err(DialogEnhancementStateError::InconsistentData {
                what: "parameter update exists with de_nr_channels == 0",
                bit_position,
            });
        }
        *history = DialogEnhancementParameterHistory::default();
        return Ok(None);
    }

    let effective = match update {
        DialogEnhancementParameterUpdate::NotApplicable => {
            return Err(DialogEnhancementStateError::InconsistentData {
                what: "parameter update missing with de_nr_channels > 0",
                bit_position,
            });
        }
        DialogEnhancementParameterUpdate::KeepPrevious => {
            if b_iframe {
                return Err(DialogEnhancementStateError::InconsistentData {
                    what: "I-frame keeps parameter data",
                    bit_position,
                });
            }
            let snapshot =
                history
                    .latest
                    .ok_or(DialogEnhancementStateError::MissingParameters {
                        simulcast,
                        bit_position,
                    })?;
            if !effective_parameters_match_configuration(snapshot, configuration) {
                return Err(DialogEnhancementStateError::MissingParameters {
                    simulcast,
                    bit_position,
                });
            }
            snapshot.data
        }
        DialogEnhancementParameterUpdate::New(parameters) => reconstruct_parameters(
            parameters,
            configuration,
            b_iframe,
            simulcast,
            history.previous,
            bit_position,
        )?,
    };

    let snapshot = DialogEnhancementParameterSnapshot {
        configuration,
        data: effective,
    };
    history.latest = Some(snapshot);
    history.previous = Some(snapshot);
    Ok(Some(effective))
}

fn effective_parameters_match_configuration(
    snapshot: DialogEnhancementParameterSnapshot,
    configuration: DialogEnhancementConfiguration,
) -> bool {
    if snapshot.configuration.method != configuration.method
        || snapshot.configuration.channel_config != configuration.channel_config
    {
        return false;
    }
    let parameters = snapshot.data;
    let mid_side_gate = matches!(configuration.method, 0 | 2) && configuration.channel_count() == 2;
    if parameters.mid_side_processing && !mid_side_gate {
        return false;
    }
    let expected_channels = configuration
        .channel_count()
        .saturating_sub(u8::from(parameters.mid_side_processing));
    parameters.parameter_channels == expected_channels
        && parameters.signal_contribution.is_some() == (configuration.method >= 2)
}

fn reconstruct_parameters(
    data: DialogEnhancementParameterData,
    configuration: DialogEnhancementConfiguration,
    b_iframe: bool,
    simulcast: bool,
    previous: Option<DialogEnhancementParameterSnapshot>,
    bit_position: u64,
) -> Result<DialogEnhancementEffectiveParameterData, DialogEnhancementStateError> {
    if data.first_code_is_absolute() != b_iframe {
        return Err(DialogEnhancementStateError::InconsistentData {
            what: "absolute/differential first-code mode disagrees with b_iframe",
            bit_position,
        });
    }

    let mid_side_gate = matches!(configuration.method, 0 | 2) && configuration.channel_count() == 2;
    let mid_side_processing = match (mid_side_gate, data.mid_side_processing) {
        (true, Some(value)) => value,
        (false, None) => false,
        _ => {
            return Err(DialogEnhancementStateError::InconsistentData {
                what: "M/S flag disagrees with method/channel gate",
                bit_position,
            });
        }
    };
    let parameter_channels = configuration
        .channel_count()
        .checked_sub(u8::from(mid_side_processing))
        .ok_or(DialogEnhancementStateError::InconsistentData {
            what: "M/S flag exceeds declared channel count",
            bit_position,
        })?;
    if data.parameter_channels() != parameter_channels {
        return Err(DialogEnhancementStateError::InconsistentData {
            what: "parameter channel count disagrees with configuration",
            bit_position,
        });
    }
    if data.signal_contribution.is_some() != (configuration.method >= 2) {
        return Err(DialogEnhancementStateError::InconsistentData {
            what: "signal contribution disagrees with hybrid method gate",
            bit_position,
        });
    }

    let code_count = usize::from(parameter_channels)
        .checked_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS)
        .ok_or(DialogEnhancementStateError::InconsistentData {
            what: "parameter code count overflow",
            bit_position,
        })?;
    if data.codes().len() != code_count || code_count > MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES {
        return Err(DialogEnhancementStateError::InconsistentData {
            what: "parameter code count disagrees with fixed shape",
            bit_position,
        });
    }

    let mut indices = [0i32; MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES];
    for channel in 0..usize::from(parameter_channels) {
        for band in 0..DIALOG_ENHANCEMENT_PARAMETER_BANDS {
            let index = channel
                .checked_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS)
                .and_then(|base| base.checked_add(band))
                .ok_or(DialogEnhancementStateError::InconsistentData {
                    what: "parameter index overflow",
                    bit_position,
                })?;
            let code = data.codes().get(index).copied().ok_or(
                DialogEnhancementStateError::InconsistentData {
                    what: "parameter code is missing",
                    bit_position,
                },
            )?;
            let value = if b_iframe && index == 0 {
                i32::from(code)
            } else {
                let reference = if b_iframe {
                    let reference_index = if band == 0 {
                        channel
                            .checked_sub(1)
                            .and_then(|previous_channel| {
                                previous_channel.checked_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS)
                            })
                            .ok_or(DialogEnhancementStateError::InconsistentData {
                                what: "I-frame channel anchor is missing",
                                bit_position,
                            })?
                    } else {
                        index.saturating_sub(1)
                    };
                    indices.get(reference_index).copied().ok_or(
                        DialogEnhancementStateError::InconsistentData {
                            what: "I-frame ref_val is missing",
                            bit_position,
                        },
                    )?
                } else {
                    previous_parameter_index(
                        previous,
                        configuration,
                        mid_side_processing,
                        channel,
                        band,
                    )
                    .unwrap_or(0)
                };
                reference.checked_add(i32::from(code)).ok_or(
                    DialogEnhancementStateError::ParameterIndexOverflow {
                        simulcast,
                        channel: u8::try_from(channel).unwrap_or(u8::MAX),
                        band: u8::try_from(band).unwrap_or(u8::MAX),
                        reference,
                        differential: code,
                    },
                )?
            };
            let slot =
                indices
                    .get_mut(index)
                    .ok_or(DialogEnhancementStateError::InconsistentData {
                        what: "effective parameter capacity exceeded",
                        bit_position,
                    })?;
            *slot = value;
        }
    }

    Ok(DialogEnhancementEffectiveParameterData {
        indices,
        parameter_channels,
        mid_side_processing,
        signal_contribution: data.signal_contribution,
    })
}

fn previous_parameter_index(
    previous: Option<DialogEnhancementParameterSnapshot>,
    configuration: DialogEnhancementConfiguration,
    mid_side_processing: bool,
    channel: usize,
    band: usize,
) -> Option<i32> {
    let previous = previous?;
    let key = parameter_channel_key(configuration, mid_side_processing, channel)?;
    for previous_channel in 0..usize::from(previous.data.parameter_channels) {
        if parameter_channel_key(
            previous.configuration,
            previous.data.mid_side_processing,
            previous_channel,
        ) == Some(key)
        {
            return previous.data.index(previous_channel, band);
        }
    }
    None
}

fn parameter_channel_key(
    configuration: DialogEnhancementConfiguration,
    mid_side_processing: bool,
    channel: usize,
) -> Option<u8> {
    const MID_SIDE_KEY: u8 = 1 << 3;
    if mid_side_processing {
        return (channel == 0).then_some(MID_SIDE_KEY | configuration.channel_config);
    }

    let mut parameter_channel = 0usize;
    for channel_mask in [0b100u8, 0b010, 0b001] {
        if configuration.channel_config & channel_mask == 0 {
            continue;
        }
        if parameter_channel == channel {
            return Some(channel_mask);
        }
        parameter_channel = parameter_channel.saturating_add(1);
    }
    None
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::super::parameter_codebook;
    use super::*;
    use crate::testutil::BitBuf;

    fn configuration(
        method: u8,
        max_gain: u8,
        channel_config: u8,
    ) -> DialogEnhancementConfiguration {
        DialogEnhancementConfiguration {
            method,
            max_gain,
            channel_config,
        }
    }

    fn metadata(
        bits: &BitBuf,
        update: DialogEnhancementConfigurationUpdate,
        b_iframe: bool,
        simulcast_gate: bool,
    ) -> DialogEnhancementMetadata {
        DialogEnhancementMetadata::from_raw_parts(
            bits.as_slice(),
            true,
            update,
            0,
            u32::try_from(bits.bit_len()).unwrap_or(u32::MAX),
            Some(b_iframe),
            simulcast_gate,
        )
        .unwrap()
    }

    fn absent_metadata(b_iframe: Option<bool>) -> DialogEnhancementMetadata {
        DialogEnhancementMetadata::from_raw_parts(
            &[],
            false,
            DialogEnhancementConfigurationUpdate::NotPresent,
            0,
            0,
            b_iframe,
            false,
        )
        .unwrap()
    }

    fn push_parameter_values(bits: &mut BitBuf, method: u8, b_iframe: bool, values: &[i16]) {
        for (index, &value) in values.iter().enumerate() {
            let absolute = b_iframe && index == 0;
            let (table, offset) = parameter_codebook(method, absolute);
            let symbol = i32::from(offset).saturating_add(i32::from(value));
            assert!(symbol >= 0);
            assert!(usize::try_from(symbol).is_ok_and(|symbol| symbol < table.len()));
            bits.push_symbol(table, u16::try_from(symbol).unwrap_or(u16::MAX));
        }
    }

    fn push_constant_parameter_values(
        bits: &mut BitBuf,
        configuration: DialogEnhancementConfiguration,
        b_iframe: bool,
        first: i16,
    ) {
        let count = usize::from(configuration.channel_count())
            .saturating_mul(DIALOG_ENHANCEMENT_PARAMETER_BANDS);
        let mut values = [0i16; MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES];
        if let Some(first_value) = values.first_mut() {
            *first_value = first;
        }
        let values = values.get(..count).unwrap_or(&[]);
        push_parameter_values(bits, configuration.method, b_iframe, values);
    }

    fn decode_active(
        state: &mut DialogEnhancementState,
        bits: &BitBuf,
        update: DialogEnhancementConfigurationUpdate,
        b_iframe: bool,
        simulcast_gate: bool,
    ) -> DialogEnhancementEffectiveData {
        state
            .decode_frame(
                metadata(bits, update, b_iframe, simulcast_gate),
                bits.as_slice(),
            )
            .unwrap()
            .unwrap()
    }

    #[test]
    fn iframe_reconstructs_ref_val_across_bands_and_channels() {
        let configuration = configuration(1, 2, 7);
        let raw = [
            10, 1, 2, 3, 4, 5, 6, 7, // ch0: absolute, then previous band
            -2, 1, 1, 1, 1, 1, 1, 1, // ch1 band0 anchors to ch0 band0
            4, -1, -1, -1, -1, -1, -1, -1, // ch2 band0 anchors to ch1 band0
        ];
        let mut bits = BitBuf::new();
        bits.push_bits(3, 5);
        bits.push_bits(19, 5);
        push_parameter_values(&mut bits, configuration.method, true, &raw);

        let mut state = DialogEnhancementState::new();
        let effective = decode_active(
            &mut state,
            &bits,
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            false,
        );
        let parameters = effective.primary.parameters.unwrap();
        assert_eq!(
            parameters.indices(),
            [
                10, 11, 13, 16, 20, 25, 31, 38, // ref_val resets to ch0 band0
                8, 9, 10, 11, 12, 13, 14, 15, // ref_val resets to ch1 band0
                12, 11, 10, 9, 8, 7, 6, 5,
            ]
        );
        assert_eq!(parameters.parameter_channels(), 3);
        assert!(!parameters.mid_side_processing);
        assert_eq!(parameters.signal_contribution, None);
        assert_eq!(parameters.index(2, 7), Some(5));
        assert_eq!(parameters.index(3, 0), None);
        assert_eq!(
            effective.primary.position,
            Some(DialogEnhancementMixCoefficients {
                first_index: 3,
                second_index: Some(19),
            })
        );
        assert_eq!(state.configuration(), Some(configuration));
        assert_eq!(state.primary_parameters(), Some(parameters));
    }

    #[test]
    fn dependent_uses_de_par_prev_and_inactive_frame_resets_its_base_to_zero() {
        let configuration = configuration(0, 2, 1);
        let mut iframe = BitBuf::new();
        push_constant_parameter_values(&mut iframe, configuration, true, 5);
        let mut state = DialogEnhancementState::new();
        decode_active(
            &mut state,
            &iframe,
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            false,
        );

        let mut dependent = BitBuf::new();
        dependent.push(false); // de_keep_data_flag
        push_parameter_values(
            &mut dependent,
            configuration.method,
            false,
            &[1, 2, 3, 4, 5, 6, 7, 8],
        );
        let effective = decode_active(
            &mut state,
            &dependent,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            false,
        );
        assert_eq!(
            effective.primary.parameters.unwrap().indices(),
            [6, 7, 8, 9, 10, 11, 12, 13]
        );

        assert_eq!(
            state
                .decode_frame(absent_metadata(Some(false)), &[])
                .unwrap(),
            None
        );
        assert_eq!(state.configuration(), Some(configuration));
        assert_eq!(
            state.primary_parameters().unwrap().indices(),
            [6, 7, 8, 9, 10, 11, 12, 13]
        );

        let mut keep_after_inactive = state;
        let mut keep = BitBuf::new();
        keep.push(true);
        let kept = decode_active(
            &mut keep_after_inactive,
            &keep,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            false,
        );
        assert_eq!(
            kept.primary.parameters.unwrap().indices(),
            [6, 7, 8, 9, 10, 11, 12, 13]
        );

        let mut after_inactive = BitBuf::new();
        after_inactive.push(false); // new differential data
        push_parameter_values(
            &mut after_inactive,
            configuration.method,
            false,
            &[2, 2, 2, 2, 2, 2, 2, 2],
        );
        let effective = decode_active(
            &mut state,
            &after_inactive,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            false,
        );
        assert_eq!(
            effective.primary.parameters.unwrap().indices(),
            [2, 2, 2, 2, 2, 2, 2, 2]
        );
    }

    #[test]
    fn keep_resolves_primary_and_simulcast_histories_independently() {
        let configuration = configuration(3, 1, 6);
        let mut iframe = BitBuf::new();
        iframe.push_bits(9, 5);
        push_constant_parameter_values(&mut iframe, configuration, true, 1);
        iframe.push_bits(7, 5);
        iframe.push(true); // b_de_simulcast
        push_constant_parameter_values(&mut iframe, configuration, true, -2);
        iframe.push_bits(11, 5);

        let mut state = DialogEnhancementState::new();
        let first = decode_active(
            &mut state,
            &iframe,
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            true,
        );
        assert_eq!(
            first.primary.parameters.unwrap().signal_contribution,
            Some(7)
        );
        let DialogEnhancementEffectiveSimulcastData::Present(first_simulcast) = first.simulcast
        else {
            panic!("I-frame 应产生 separate core data")
        };
        assert_eq!(
            first_simulcast.parameters.unwrap().signal_contribution,
            Some(11)
        );
        assert_ne!(
            first.primary.parameters.unwrap().indices(),
            first_simulcast.parameters.unwrap().indices()
        );

        let mut dependent = BitBuf::new();
        dependent.push(true); // keep primary position
        dependent.push(true); // keep primary parameters
        dependent.push(true); // b_de_simulcast
        dependent.push(true); // keep simulcast parameters
        let kept = decode_active(
            &mut state,
            &dependent,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            true,
        );
        assert_eq!(kept.primary, first.primary);
        assert_eq!(kept.simulcast, first.simulcast);
        assert_eq!(state.position().unwrap().first_index, 9);
        assert_eq!(state.primary_parameters(), first.primary.parameters);
        assert_eq!(state.simulcast_parameters(), first_simulcast.parameters);
    }

    #[test]
    fn configuration_update_maps_previous_parameters_by_physical_channel() {
        let first_configuration = configuration(0, 0, 7);
        let mut iframe = BitBuf::new();
        push_parameter_values(
            &mut iframe,
            first_configuration.method,
            true,
            &[
                10, 0, 0, 0, 0, 0, 0, 0, // Left becomes 10 in every band.
                10, 0, 0, 0, 0, 0, 0, 0, // Right becomes 20 in every band.
                10, 0, 0, 0, 0, 0, 0, 0, // Centre becomes 30 in every band.
            ],
        );
        let mut state = DialogEnhancementState::new();
        decode_active(
            &mut state,
            &iframe,
            DialogEnhancementConfigurationUpdate::New(first_configuration),
            true,
            false,
        );

        let remapped = configuration(0, 0, 5);
        let mut changed_channel = BitBuf::new();
        changed_channel.push(false); // de_keep_data_flag
        changed_channel.push(false); // de_ms_proc_flag
        push_parameter_values(&mut changed_channel, remapped.method, false, &[1; 16]);
        let effective = decode_active(
            &mut state,
            &changed_channel,
            DialogEnhancementConfigurationUpdate::New(remapped),
            false,
            false,
        );
        assert_eq!(
            effective.primary.parameters.unwrap().indices(),
            [
                11, 11, 11, 11, 11, 11, 11, 11, // Left keeps its previous base.
                31, 31, 31, 31, 31, 31, 31, 31, // Centre moves from slot 2 to slot 1.
            ]
        );

        let introduced = configuration(0, 0, 6);
        let mut introduced_channel = BitBuf::new();
        introduced_channel.push(false); // de_keep_data_flag
        introduced_channel.push(false); // de_ms_proc_flag
        push_parameter_values(&mut introduced_channel, introduced.method, false, &[1; 16]);
        let effective = decode_active(
            &mut state,
            &introduced_channel,
            DialogEnhancementConfigurationUpdate::New(introduced),
            false,
            false,
        );
        assert_eq!(
            effective.primary.parameters.unwrap().indices(),
            [
                12, 12, 12, 12, 12, 12, 12, 12, // Left remains present.
                1, 1, 1, 1, 1, 1, 1, 1, // Right was unused in the previous frame.
            ]
        );
    }

    #[test]
    fn mid_side_differential_history_only_matches_the_same_representation() {
        let configuration = configuration(0, 0, 6);
        let mut iframe = BitBuf::new();
        iframe.push(true); // de_ms_proc_flag
        push_parameter_values(
            &mut iframe,
            configuration.method,
            true,
            &[5, 0, 0, 0, 0, 0, 0, 0],
        );
        let mut state = DialogEnhancementState::new();
        decode_active(
            &mut state,
            &iframe,
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            false,
        );

        let mut continued_mid = BitBuf::new();
        continued_mid.push(false); // de_keep_data_flag
        continued_mid.push(true); // de_ms_proc_flag
        push_parameter_values(&mut continued_mid, configuration.method, false, &[1; 8]);
        let effective = decode_active(
            &mut state,
            &continued_mid,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            false,
        );
        assert_eq!(effective.primary.parameters.unwrap().indices(), [6; 8]);

        let mut switched_to_channels = BitBuf::new();
        switched_to_channels.push(false); // de_keep_data_flag
        switched_to_channels.push(false); // de_ms_proc_flag
        push_parameter_values(
            &mut switched_to_channels,
            configuration.method,
            false,
            &[1; 16],
        );
        let effective = decode_active(
            &mut state,
            &switched_to_channels,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            false,
        );
        assert_eq!(effective.primary.parameters.unwrap().indices(), [1; 16]);

        let mut switched_back_to_mid = BitBuf::new();
        switched_back_to_mid.push(false); // de_keep_data_flag
        switched_back_to_mid.push(true); // de_ms_proc_flag
        push_parameter_values(
            &mut switched_back_to_mid,
            configuration.method,
            false,
            &[2; 8],
        );
        let effective = decode_active(
            &mut state,
            &switched_back_to_mid,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            false,
        );
        assert_eq!(effective.primary.parameters.unwrap().indices(), [2; 8]);
    }

    #[test]
    fn compatible_method_and_max_gain_updates_keep_differential_history() {
        let first_configuration = configuration(0, 0, 1);
        let mut iframe = BitBuf::new();
        push_constant_parameter_values(&mut iframe, first_configuration, true, 5);
        let mut state = DialogEnhancementState::new();
        decode_active(
            &mut state,
            &iframe,
            DialogEnhancementConfigurationUpdate::New(first_configuration),
            true,
            false,
        );

        let hybrid = configuration(2, 0, 1);
        let mut changed_method = BitBuf::new();
        changed_method.push(false); // de_keep_data_flag
        push_parameter_values(&mut changed_method, hybrid.method, false, &[1; 8]);
        changed_method.push_bits(7, 5);
        let effective = decode_active(
            &mut state,
            &changed_method,
            DialogEnhancementConfigurationUpdate::New(hybrid),
            false,
            false,
        );
        let parameters = effective.primary.parameters.unwrap();
        assert_eq!(parameters.indices(), [6; 8]);
        assert_eq!(parameters.signal_contribution, Some(7));

        let new_gain = configuration(2, 3, 1);
        let mut gain_only = BitBuf::new();
        gain_only.push(true); // de_keep_data_flag
        let effective = decode_active(
            &mut state,
            &gain_only,
            DialogEnhancementConfigurationUpdate::New(new_gain),
            false,
            false,
        );
        assert_eq!(effective.primary.parameters, Some(parameters));

        let first_cross = configuration(1, 0, 6);
        let mut iframe = BitBuf::new();
        iframe.push_bits(9, 5);
        push_constant_parameter_values(&mut iframe, first_cross, true, 3);
        let mut cross_state = DialogEnhancementState::new();
        decode_active(
            &mut cross_state,
            &iframe,
            DialogEnhancementConfigurationUpdate::New(first_cross),
            true,
            false,
        );

        let hybrid_cross = configuration(3, 0, 6);
        let mut dependent = BitBuf::new();
        dependent.push(true); // de_keep_pos_flag
        dependent.push(false); // de_keep_data_flag
        push_parameter_values(&mut dependent, hybrid_cross.method, false, &[1; 16]);
        dependent.push_bits(11, 5);
        let effective = decode_active(
            &mut cross_state,
            &dependent,
            DialogEnhancementConfigurationUpdate::New(hybrid_cross),
            false,
            false,
        );
        assert_eq!(effective.primary.position.unwrap().first_index, 9);
        assert_eq!(effective.primary.parameters.unwrap().indices(), [4; 16]);
    }

    #[test]
    fn failed_topology_keep_is_transactional_and_substream_states_are_isolated() {
        let initial_configuration = configuration(0, 1, 1);
        let mut iframe = BitBuf::new();
        push_constant_parameter_values(&mut iframe, initial_configuration, true, 4);
        let mut first_substream = DialogEnhancementState::new();
        decode_active(
            &mut first_substream,
            &iframe,
            DialogEnhancementConfigurationUpdate::New(initial_configuration),
            true,
            false,
        );

        let mut keep = BitBuf::new();
        keep.push(true);
        let mut second_substream = DialogEnhancementState::new();
        assert!(matches!(
            second_substream.decode_frame(
                metadata(
                    &keep,
                    DialogEnhancementConfigurationUpdate::KeepPrevious,
                    false,
                    false,
                ),
                keep.as_slice(),
            ),
            Err(DialogEnhancementStateError::Data(
                DialogEnhancementDataError::MissingConfiguration { .. }
            ))
        ));
        assert_eq!(second_substream, DialogEnhancementState::new());
        let kept = decode_active(
            &mut first_substream,
            &keep,
            DialogEnhancementConfigurationUpdate::KeepPrevious,
            false,
            false,
        );
        assert_eq!(kept.primary.parameters.unwrap().index(0, 0), Some(4));

        let snapshot = first_substream;
        let incompatible = configuration(0, 1, 2);
        assert_eq!(
            first_substream
                .decode_frame(
                    metadata(
                        &keep,
                        DialogEnhancementConfigurationUpdate::New(incompatible),
                        false,
                        false,
                    ),
                    keep.as_slice(),
                )
                .unwrap_err(),
            DialogEnhancementStateError::MissingParameters {
                simulcast: false,
                bit_position: 0,
            }
        );
        assert_eq!(first_substream, snapshot);
    }

    #[test]
    fn iframe_absence_resets_state_and_unknown_absence_is_transactional() {
        let configuration = configuration(0, 2, 1);
        let mut iframe = BitBuf::new();
        push_constant_parameter_values(&mut iframe, configuration, true, 6);
        let mut state = DialogEnhancementState::new();
        decode_active(
            &mut state,
            &iframe,
            DialogEnhancementConfigurationUpdate::New(configuration),
            true,
            false,
        );
        let snapshot = state;

        assert_eq!(
            state.decode_frame(absent_metadata(None), &[]).unwrap_err(),
            DialogEnhancementStateError::MissingFrameContext { bit_position: 0 }
        );
        assert_eq!(state, snapshot);

        assert_eq!(
            state
                .decode_frame(absent_metadata(Some(true)), &[])
                .unwrap(),
            None
        );
        assert_eq!(state, DialogEnhancementState::new());
    }
}
