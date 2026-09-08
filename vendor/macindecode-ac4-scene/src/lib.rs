//! AC-4 渲染前场景的流式 Rust API。
//!
//! 本 crate 定义容器无关的场景输出契约。输入时间只使用调用方已经换算好的整数
//! 采样位置；MP4 sample table、priming 与 edit list 的解释仍属于容器适配层。
//!
//! 解码会话面向已验证的 A-JOC Core/downmix 与 Full/upmix 子集。Core 输出是
//! A-SPX 后、A-JOC 上混前的对象信号；Full 输出单元是空间对象组。两者都不能
//! 据此推断编码前的创作对象 identity。
//!
//! 当前版本建立了数据契约、Session 控制面、presentation 选择与错误模型；启用
//! `audio-decode` 时，Session 已自持并事务驱动 A-JOC engine，并按配置把 Core
//! 诊断出口或 Full 重建出口的对象/LFE normalized PCM、group OAMD common 和对应
//! downmix/upmix 逐对象更新组装到同一份表 188 到期时间线。更新保留完整状态、raw
//! timing、changed mask、control source AU 与跨帧队列；只有 offset 0 更新进入帧
//! 起点状态，逐对象绝对时间倒退会失败关闭。
//! 公开 `decode_access_unit` 返回 Session 自有存储的借用视图；等待随机访问点时返回
//! 空帧序列，解析或 DSP 失败时不会发布半成品视图。
//!
//! 成功 AU 还可通过 `DecodedAccessUnit::presentation_metadata` 观察所选 presentation 的
//! 完整 processing metadata、当前有效 DRC 配置与 group-gain 码值。原 payload 复制到 Session
//! 可复用存储中；该侧车只解析和保留，不应用任何 presentation processing，也不进入
//! `Ac4SceneFrame` renderer 语义。
//!
//! 显式启用核心带诊断时，`DecodedAccessUnit` 还提供不属于 SceneFrame 的 pre-A-SPX
//! normalized 侧车；默认关闭，普通 renderer 不为其复制 PCM。它与 Scene PCM 共享
//! `1.0` nominal full scale，但不经过表 188 场景对齐，也不把传输声道声明为 renderer
//! 元素。需要历史 `±32768` 输出尺度的文件 adapter 应在 Scene 边界之外乘精确的 `2^15`。

#![no_std]

extern crate alloc;

mod assembly;
mod error;
#[cfg(feature = "audio-decode")]
mod full_engine;
mod group_oamd;
mod model;
mod session;

pub use error::{
    BitstreamFailure, DecodeError, DecodeErrorContext, DecodeErrorKind, DecodeStage, NeedMoreData,
    PresentationSelectionError, UnsupportedReason,
};

pub use macindecode_ac4_bitstream::{
    Ac4PresentationSubstream, PresentationDrcConfiguration, PresentationSubstreamCapacity,
    PresentationSubstreamContext, PresentationSubstreamError, PresentationSubstreamGroupGainCodes,
};
pub use model::{
    Ac4DecoderConfig, Ac4SceneFrame, AccessUnit, AccessUnitContext, BedKind, CartesianPosition,
    CodecDelay, DecodeMode, DecodeStatus, DecodedAccessUnit, FrameDiagnostics, HeadphoneMode,
    HeadphoneState, MetadataFields, ObjectExtent, ObjectKind, PcmLayout, PcmPlane, PcmSampleFormat,
    PlanarPcm, PresentationSelection, PresentationSelectionMetadata,
    PresentationSelectionMetadataIdentity, PresentationSelectionMetadataMatch,
    PresentationSelectionMetadataMatchBasis, PresentationSubstreamMetadata, RawOamdCommonState,
    RawOamdState, RawOamdTiming, RawOamdUpdate, ResetKind, SceneBed, SceneBedComponent,
    SceneElementId, SceneElementSource, SceneFrameIter, SceneMetadataUpdate, SceneObject,
    SceneObjectState, ScenePath, ScenePresentation, SceneTimeline, SpeakerLabel, ZoneState,
};
#[cfg(feature = "audio-decode")]
pub use model::{CoreBandPcmChannel, CoreBandPcmFrame};
pub use session::Ac4DecoderSession;
