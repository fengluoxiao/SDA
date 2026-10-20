//! Full A-JOC 解码路径的已验证支持边界。
//!
//! 本模块把进入 A-SPX/QMF 与 Full A-JOC 数值通路前的判定收成不可伪造的
//! [`SupportedAspxFrame`] 和 [`SupportedAjocFullFrame`]。产品驱动只能消费凭证，
//! 不能在 CLI 或 Scene adapter 中另抄一套布尔门禁。
//!
//! [`FullAjocDecoder`] 在同一模块边界内消费这些凭证，统一拥有音频语法、ASF
//! overlap、表 188 控制延迟、A-SPX/QMF、A-JOC、OAMD、LFE 与终端合成状态；
//! 逐帧出口借用 decoder 的内部缓冲，不持有整文件 sink。

use crate::{
    ajoc::{
        Ajoc, MAX_AJOC_DMX_SIGNALS, MAX_DATA_POINTS, reconstruction::MAX_RECONSTRUCTED_OBJECTS,
    },
    channel::CompandingControl,
    frame_alignment::{FrameAlignment, frame_alignment},
    substream_audio::{Ac4SubstreamAjoc, AjocSubstreamContext},
    var_element::VarChannelElement,
};
use alloc::{format, string::String};
use core::fmt;

mod asf;
mod decoder;
mod syntax;

pub use asf::{
    DecodedFullAjocAsfFrame, FullAjocAsfBuffer, FullAjocAsfChannelObservation, FullAjocAsfError,
    FullAjocAsfErrorKind, FullAjocAsfFrameObservation, FullAjocAsfPcmChannel, FullAjocAsfStage,
};
pub use decoder::{
    DecodedFullAjocAudioFrame, DecodedFullAjocFrame, DecodedFullAjocFrontendFrame,
    FullAjocAlignedSideInformation, FullAjocAudioFrameError, FullAjocAudioFrameInput,
    FullAjocDecodeError, FullAjocDecodeErrorKind, FullAjocDecodeMode, FullAjocDecoder,
    FullAjocFrameInput, FullAjocFrameProvenance, FullAjocFrontendError, FullAjocGroupOamdState,
    FullAjocOamdFrameSnapshot, FullAjocOamdObjectState, FullAjocOamdTimingState,
    FullAjocOamdUpdateSnapshot, FullAjocObservation, FullAjocPcmChannel, FullAjocPcmSource,
    FullAjocUnsupported,
};
pub use syntax::{
    DecodedFullAjocSyntaxFrame, FullAjocSyntaxBuffer, FullAjocSyntaxError,
    FullAjocSyntaxFrameInput, FullAjocSyntaxObservation,
};

pub use crate::aspx::{AspxReach, collect_aspx_reach};

/// `5.7.5.2` 的逐声道或平均压扩是否实际启用。
///
/// `b_compand_avg` 独立于逐声道开关；只要任一分支启用就必须进入尚未实现的
/// companding 数值路径。
#[must_use]
pub fn companding_is_active(control: &CompandingControl) -> bool {
    control
        .compand_on
        .iter()
        .take(usize::from(control.channels))
        .any(|flag| *flag)
        || control.compand_avg == Some(true)
}

/// 已解析、但当前 A-SPX 数值路径尚不能正确执行的分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AspxBlocker {
    /// `5.7.5` companding 实际启用。
    ActiveCompanding,
    /// `5.7.6.5` FIC/TIC 交织实际启用。
    Interleaving,
    /// 元素使用 SIMPLE 而不是 A-SPX codec mode。
    SimpleTimeline,
    /// 短帧终端 QMF 时间轴尚未覆盖。
    ShortFrameTimeline { frame_length: u16 },
    /// 帧长不属于表 188/189 的公共集合。
    UnsupportedFrameAlignment { frame_length: u16 },
}

impl AspxBlocker {
    /// 在推进数值状态前核对当前已验证的 A-SPX 子集。
    pub fn check(
        element: &VarChannelElement,
        frame_length: u16,
    ) -> Result<SupportedAspxFrame, Self> {
        Self::check_values(
            element.codec_mode_aspx,
            element.companding.as_ref(),
            element.aspx_reach(),
            frame_length,
        )
    }

    fn check_values(
        codec_mode_aspx: bool,
        companding: Option<&CompandingControl>,
        reach: AspxReach,
        frame_length: u16,
    ) -> Result<SupportedAspxFrame, Self> {
        let identity = AspxFrameIdentity {
            codec_mode_aspx,
            companding: companding.copied(),
            reach,
            frame_length,
        };
        if companding.is_some_and(companding_is_active) {
            return Err(Self::ActiveCompanding);
        }
        if reach.interleaved() {
            return Err(Self::Interleaving);
        }
        if !codec_mode_aspx {
            return Err(Self::SimpleTimeline);
        }
        if frame_length < 1_536 {
            return Err(Self::ShortFrameTimeline { frame_length });
        }
        let alignment = frame_alignment(frame_length)
            .ok_or(Self::UnsupportedFrameAlignment { frame_length })?;
        Ok(SupportedAspxFrame {
            alignment,
            identity,
        })
    }

    /// 生成与既有 CLI wire diagnostics 兼容的详细原因。
    #[must_use]
    pub fn detail(&self) -> String {
        format!("{self}")
    }
}

impl core::error::Error for AspxBlocker {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AspxFrameIdentity {
    codec_mode_aspx: bool,
    companding: Option<CompandingControl>,
    reach: AspxReach,
    frame_length: u16,
}

impl fmt::Display for AspxBlocker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ActiveCompanding => formatter
                .write_str("Unsupported 5.7.5 companding is enabled; A-SPX PCM cannot be exported"),
            Self::Interleaving => formatter.write_str(
                "Unsupported 5.7.6.5 FIC/TIC interleaving is enabled; A-SPX PCM cannot be exported",
            ),
            Self::SimpleTimeline => formatter.write_str(
                "Global six-timeslot timeline for final QMF in SIMPLE mode is unresolved",
            ),
            Self::ShortFrameTimeline { frame_length } => write!(
                formatter,
                "ts_offset_hfgen=3 for short frame {frame_length} and the global six-timeslot final-QMF timeline are unresolved"
            ),
            Self::UnsupportedFrameAlignment { frame_length } => write!(
                formatter,
                "Frame length {frame_length} is not in the common support set of Tables 188/189"
            ),
        }
    }
}

/// 已通过 A-SPX 与表 188 时间轴门禁的一帧凭证。
///
/// 字段私有；只有 [`AspxBlocker::check`] 能从 [`VarChannelElement`] 构造。凭证
/// 同时保留被验证的解析摘要，消费方可以拒绝跨帧错配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedAspxFrame {
    alignment: FrameAlignment,
    identity: AspxFrameIdentity,
}

impl SupportedAspxFrame {
    /// 该凭证绑定的表 188 帧对齐参数。
    #[must_use]
    pub const fn alignment(self) -> FrameAlignment {
        self.alignment
    }

    /// 当前凭证是否属于这份解析摘要。
    #[must_use]
    pub fn matches(self, element: &VarChannelElement, frame_length: u16) -> bool {
        self.identity
            == AspxFrameIdentity {
                codec_mode_aspx: element.codec_mode_aspx,
                companding: element.companding,
                reach: element.aspx_reach(),
                frame_length,
            }
    }
}

/// 已解析、但当前 Full A-JOC 标量路径尚不能正确执行的分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FullAjocBlocker {
    /// A-SPX/QMF 前置路径本身不受支持。
    Aspx(AspxBlocker),
    /// 当前子集只冻结 48 kHz 时间轴。
    SamplingFrequency { sampling_frequency_hz: u32 },
    /// 当前输出范围只接受一条物理 A-JOC substream。
    MultipleSubstreams { physical_substreams: usize },
    /// 输入数不在当前重建容量内。
    DmxSignals { num_dmx_signals: u8 },
    /// 输出数不在当前重建容量内。
    UmxSignals { num_umx_signals: u32 },
    /// `ajoc_num_dpoints == 3` 是可表示但正文未定义的保留值。
    ReservedDataPointCount,
    /// 数据点数量超出 2 位字段的可表示范围。
    DataPointCountOutOfRange { data_points: u8 },
    /// alternative OAMD 已解析保留，但当前对象出口尚未应用被选 dataset。
    AlternativeObjectMetadata,
    /// Pseudocode 18 的活动 dialogue enhancement 分支尚未覆盖。
    ActiveDialogueEnhancement { dialogue_objects: u8 },
}

impl FullAjocBlocker {
    /// 生成与既有 CLI wire diagnostics 兼容的详细原因。
    #[must_use]
    pub fn detail(&self) -> String {
        format!("{self}")
    }
}

impl fmt::Display for FullAjocBlocker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Aspx(blocker) => write!(
                formatter,
                "Unsupported A-JOC full prerequisite path: {blocker}"
            ),
            Self::SamplingFrequency {
                sampling_frequency_hz,
            } => write!(
                formatter,
                "A-JOC full currently supports only 48000 Hz; got {sampling_frequency_hz} Hz"
            ),
            Self::MultipleSubstreams {
                physical_substreams,
            } => write!(
                formatter,
                "A-JOC full currently supports one physical substream; output range contains {physical_substreams}"
            ),
            Self::DmxSignals { num_dmx_signals } => write!(
                formatter,
                "A-JOC full downmix signal count {num_dmx_signals} is outside 1..={MAX_AJOC_DMX_SIGNALS}"
            ),
            Self::UmxSignals { num_umx_signals } => write!(
                formatter,
                "A-JOC full upmix signal count {num_umx_signals} is outside 1..={MAX_RECONSTRUCTED_OBJECTS}"
            ),
            Self::ReservedDataPointCount => formatter.write_str(
                "ajoc_num_dpoints is reserved value 3 and cannot enter A-JOC full reconstruction",
            ),
            Self::DataPointCountOutOfRange { data_points } => write!(
                formatter,
                "ajoc_num_dpoints is {data_points}, exceeding the two-bit field limit {MAX_DATA_POINTS}"
            ),
            Self::AlternativeObjectMetadata => formatter.write_str(
                "A-JOC alternative object metadata is preserved but not applied to object output",
            ),
            Self::ActiveDialogueEnhancement { dialogue_objects } => write!(
                formatter,
                "A-JOC full enables {dialogue_objects} dialogue-enhancement objects"
            ),
        }
    }
}

impl core::error::Error for FullAjocBlocker {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Aspx(blocker) => Some(blocker),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FullAjocFrameIdentity {
    ajoc: Ajoc,
    sampling_frequency_hz: u32,
    physical_substreams: usize,
    dialogue_objects: u8,
}

/// 已通过当前 Full A-JOC 支持边界的一帧凭证。
///
/// 凭证内嵌同一次检查产生的 A-SPX/表 188 凭证，并拥有被验证的 A-JOC 与会话
/// 摘要；字段私有，调用方只能提交解析结果重新判定，不能用另一帧的安全常量取得
/// 凭证后再与当前控制错配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedAjocFullFrame {
    aspx: SupportedAspxFrame,
    identity: FullAjocFrameIdentity,
}

impl SupportedAjocFullFrame {
    /// 从同一条已解析 substream 及其派生上下文核对 Full A-JOC 支持边界。
    pub fn check(
        parsed: &Ac4SubstreamAjoc,
        context: &AjocSubstreamContext,
        physical_substreams: usize,
    ) -> Result<Self, FullAjocBlocker> {
        if context.params.b_alternative {
            return Err(FullAjocBlocker::AlternativeObjectMetadata);
        }
        let element = &parsed.audio.var_element;
        let ajoc = &parsed.audio.ajoc;
        let channel_context = context.params.context;
        let aspx = AspxBlocker::check(element, channel_context.frame_len_base)
            .map_err(FullAjocBlocker::Aspx)?;
        Self::check_values(
            aspx,
            channel_context.sampling_frequency_hz,
            physical_substreams,
            *ajoc,
            parsed.audio.dmx_de.num_dlg_obj,
        )
    }

    fn check_values(
        aspx: SupportedAspxFrame,
        sampling_frequency_hz: u32,
        physical_substreams: usize,
        ajoc: Ajoc,
        dialogue_objects: u8,
    ) -> Result<Self, FullAjocBlocker> {
        if sampling_frequency_hz != 48_000 {
            return Err(FullAjocBlocker::SamplingFrequency {
                sampling_frequency_hz,
            });
        }
        if physical_substreams != 1 {
            return Err(FullAjocBlocker::MultipleSubstreams {
                physical_substreams,
            });
        }
        if ajoc.num_dmx_signals == 0 || usize::from(ajoc.num_dmx_signals) > MAX_AJOC_DMX_SIGNALS {
            return Err(FullAjocBlocker::DmxSignals {
                num_dmx_signals: ajoc.num_dmx_signals,
            });
        }
        let max_objects = u32::try_from(MAX_RECONSTRUCTED_OBJECTS).unwrap_or(u32::MAX);
        if !(1..=max_objects).contains(&ajoc.num_umx_signals) {
            return Err(FullAjocBlocker::UmxSignals {
                num_umx_signals: ajoc.num_umx_signals,
            });
        }
        if ajoc.data_points.count == 3 {
            return Err(FullAjocBlocker::ReservedDataPointCount);
        }
        if usize::from(ajoc.data_points.count) > MAX_DATA_POINTS {
            return Err(FullAjocBlocker::DataPointCountOutOfRange {
                data_points: ajoc.data_points.count,
            });
        }
        if dialogue_objects != 0 {
            return Err(FullAjocBlocker::ActiveDialogueEnhancement { dialogue_objects });
        }
        Ok(Self {
            aspx,
            identity: FullAjocFrameIdentity {
                ajoc,
                sampling_frequency_hz,
                physical_substreams,
                dialogue_objects,
            },
        })
    }

    /// 取出同一次判定内嵌的 A-SPX/表 188 凭证。
    #[must_use]
    pub const fn aspx(self) -> SupportedAspxFrame {
        self.aspx
    }

    /// 核对消费方提交的控制是否仍属于发放凭证的同一帧。
    #[must_use]
    pub fn matches(
        self,
        element: &VarChannelElement,
        frame_length: u16,
        sampling_frequency_hz: u32,
        physical_substreams: usize,
        ajoc: &Ajoc,
        dialogue_objects: u8,
    ) -> bool {
        self.aspx.matches(element, frame_length)
            && self.identity
                == FullAjocFrameIdentity {
                    ajoc: *ajoc,
                    sampling_frequency_hz,
                    physical_substreams,
                    dialogue_objects,
                }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported_aspx(frame_length: u16) -> SupportedAspxFrame {
        AspxBlocker::check_values(true, None, AspxReach::default(), frame_length)
            .expect("测试 A-SPX 参数必须受支持")
    }

    fn check(
        sampling_frequency_hz: u32,
        physical_substreams: usize,
        num_dmx_signals: u8,
        num_umx_signals: u32,
        data_points: u8,
        dialogue_objects: u8,
    ) -> Result<SupportedAjocFullFrame, FullAjocBlocker> {
        SupportedAjocFullFrame::check_values(
            supported_aspx(2_048),
            sampling_frequency_hz,
            physical_substreams,
            Ajoc::for_test(0, data_points, num_umx_signals, num_dmx_signals),
            dialogue_objects,
        )
    }

    #[test]
    fn aspx_gate_rejects_every_known_unimplemented_branch() {
        let blocked = |aspx, companding, reach, frame_length| {
            AspxBlocker::check_values(aspx, companding, reach, frame_length).expect_err("应被拦下")
        };

        assert_eq!(
            blocked(false, None, AspxReach::default(), 2_048),
            AspxBlocker::SimpleTimeline
        );
        assert_eq!(
            blocked(true, None, AspxReach::default(), 1_024),
            AspxBlocker::ShortFrameTimeline {
                frame_length: 1_024
            }
        );
        assert_eq!(
            blocked(true, None, AspxReach::new(false, true, false, false), 2_048),
            AspxBlocker::Interleaving
        );

        let companded = CompandingControl {
            compand_on: [true, false, false, false, false, false, false, false],
            channels: 1,
            ..CompandingControl::default()
        };
        assert_eq!(
            blocked(true, Some(&companded), AspxReach::default(), 2_048),
            AspxBlocker::ActiveCompanding
        );
    }

    #[test]
    fn aspx_gate_accepts_the_confirmed_long_frame_subset() {
        let inactive = CompandingControl {
            channels: 1,
            compand_avg: Some(false),
            ..CompandingControl::default()
        };
        for frame_length in [1_536, 1_920, 2_048] {
            assert!(
                AspxBlocker::check_values(
                    true,
                    Some(&inactive),
                    AspxReach::default(),
                    frame_length
                )
                .is_ok(),
                "长帧 {frame_length} 的已实现子集应可导出"
            );
        }
    }

    #[test]
    fn aspx_credential_is_bound_to_the_parsed_element_and_frame() {
        let mut element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let credential = AspxBlocker::check(&element, 2_048).expect("测试元素应受支持");
        assert!(credential.matches(&element, 2_048));
        assert!(!credential.matches(&element, 1_920), "不得跨帧长复用");

        element.codec_mode_aspx = false;
        assert!(
            !credential.matches(&element, 2_048),
            "不得与另一份元素摘要错配"
        );
    }

    #[test]
    fn current_full_subset_produces_the_only_credential() {
        assert!(check(48_000, 1, 9, 20, 2, 0).is_ok());
    }

    #[test]
    fn full_credential_owns_and_checks_the_validated_frame_identity() {
        let element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let aspx = AspxBlocker::check(&element, 2_048).expect("测试元素应受支持");
        let mut ajoc = Ajoc::for_test(0, 2, 20, 1);
        let credential = SupportedAjocFullFrame::check_values(aspx, 48_000, 1, ajoc, 0)
            .expect("测试 full 参数应受支持");
        assert!(credential.matches(&element, 2_048, 48_000, 1, &ajoc, 0));

        ajoc.num_umx_signals = 19;
        assert!(
            !credential.matches(&element, 2_048, 48_000, 1, &ajoc, 0),
            "不得与另一份 A-JOC 控制错配"
        );
    }

    #[test]
    fn every_declared_full_boundary_is_accepted() {
        for frame_length in [1_536, 1_920, 2_048] {
            for num_dmx_signals in [1, 16] {
                for num_umx_signals in [1, 20, 24, 64, 128] {
                    for data_points in [0, 1, 2] {
                        assert!(
                            SupportedAjocFullFrame::check_values(
                                supported_aspx(frame_length),
                                48_000,
                                1,
                                Ajoc::for_test(0, data_points, num_umx_signals, num_dmx_signals,),
                                0,
                            )
                            .is_ok(),
                            "frame={frame_length}, dmx={num_dmx_signals}, umx={num_umx_signals}, dpoints={data_points}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_full_specific_boundary_fails_closed() {
        for (result, expected) in [
            (
                check(44_100, 1, 9, 20, 2, 0),
                FullAjocBlocker::SamplingFrequency {
                    sampling_frequency_hz: 44_100,
                },
            ),
            (
                check(48_000, 2, 9, 20, 2, 0),
                FullAjocBlocker::MultipleSubstreams {
                    physical_substreams: 2,
                },
            ),
            (
                check(48_000, 1, 0, 20, 2, 0),
                FullAjocBlocker::DmxSignals { num_dmx_signals: 0 },
            ),
            (
                check(48_000, 1, 17, 20, 2, 0),
                FullAjocBlocker::DmxSignals {
                    num_dmx_signals: 17,
                },
            ),
            (
                check(48_000, 1, 9, 0, 2, 0),
                FullAjocBlocker::UmxSignals { num_umx_signals: 0 },
            ),
            (
                check(48_000, 1, 9, 129, 2, 0),
                FullAjocBlocker::UmxSignals {
                    num_umx_signals: 129,
                },
            ),
            (
                check(48_000, 1, 9, 20, 3, 0),
                FullAjocBlocker::ReservedDataPointCount,
            ),
            (
                check(48_000, 1, 9, 20, 4, 0),
                FullAjocBlocker::DataPointCountOutOfRange { data_points: 4 },
            ),
            (
                check(48_000, 1, 9, 20, 2, 1),
                FullAjocBlocker::ActiveDialogueEnhancement {
                    dialogue_objects: 1,
                },
            ),
        ] {
            assert_eq!(result.expect_err("边界必须拒绝"), expected);
        }
    }

    #[test]
    fn full_credential_cannot_bypass_the_aspx_gate() {
        let error = AspxBlocker::check_values(false, None, AspxReach::default(), 2_048)
            .map_err(FullAjocBlocker::Aspx)
            .and_then(|aspx| {
                SupportedAjocFullFrame::check_values(
                    aspx,
                    48_000,
                    1,
                    Ajoc::for_test(0, 1, 20, 9),
                    0,
                )
            })
            .expect_err("SIMPLE 必须先被 A-SPX 门禁拒绝");
        assert!(matches!(error, FullAjocBlocker::Aspx(_)));
    }
}
