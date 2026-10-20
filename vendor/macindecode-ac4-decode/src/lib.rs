//! AC-4 音频数值重建：ASF、A-SPX、A-JOC、QMF 与统一 Full engine。
//!
//! 规范基线 `TS103190:2025-07`，见 `docs/SPEC_TRACEABILITY.md`。
//!
//! 本 crate 建立在 [`macindecode_ac4_bitstream`] 的语法与元数据之上，方向单向：
//! syntax -> decode -> scene（ADR-0011、ADR-0013）。它不依赖容器或平台。
//!
//! **一切需要 ETSI 表的处理都在这里**，包括 Huffman 码本机制与两处 Huffman 编码
//! 的元数据解码（[`drc_gains`] 的 DRC gain set、[`dialog_enhancement`] 的 DE 参数）。
//! 这两处按规范属于元数据而非 DSP，但它们消费的是同一批随附 C 表；放在这里，
//! 规范表流水线与 `spec_lock` 的冻结摘要才只有一份真相源。
//!
//! `spec-tables` 消费用户从官方 ETSI PDF 本地生成的静态表；`audio-decode` 在其
//! 基础上再消费规范随附 C 表，启用完整音频重建路径。两类表值均不随仓库或
//! crate 分发，准备方法见 crate README。

#![no_std]

#[cfg(feature = "audio-decode")]
extern crate alloc;

#[cfg(feature = "spec-tables")]
#[allow(
    dead_code,
    reason = "生成文件同时覆盖 spec-tables 与 audio-decode 的规范表"
)]
pub(crate) mod spec_tables {
    include!(concat!(env!("OUT_DIR"), "/ts103190_pdf_tables.rs"));
}

pub mod ajoc;
#[cfg(feature = "audio-decode")]
pub use ajoc::de as ajoc_de;
pub mod asf;
pub mod aspx;
#[cfg(feature = "audio-decode")]
pub mod audio_data;
#[cfg(feature = "audio-decode")]
pub mod channel;
#[cfg(feature = "audio-decode")]
pub mod dialog_enhancement;
#[cfg(feature = "audio-decode")]
pub mod drc_gains;
#[cfg(feature = "audio-decode")]
pub mod element_drive;
#[cfg(feature = "audio-decode")]
pub mod frame_alignment;
#[cfg(feature = "audio-decode")]
pub mod full_ajoc;
#[cfg(feature = "audio-decode")]
pub mod huffman;
#[cfg(feature = "audio-decode")]
pub mod substream_audio;
#[cfg(all(test, feature = "audio-decode"))]
mod testutil;
#[cfg(feature = "audio-decode")]
pub mod var_element;

#[cfg(feature = "audio-decode")]
pub use dialog_enhancement::{
    DIALOG_ENHANCEMENT_PARAMETER_BANDS, DialogEnhancementDataBlock, DialogEnhancementDataError,
    DialogEnhancementDecodedData, DialogEnhancementEffectiveData,
    DialogEnhancementEffectiveDataBlock, DialogEnhancementEffectiveParameterData,
    DialogEnhancementEffectiveSimulcastData, DialogEnhancementMetadataExt,
    DialogEnhancementMixCoefficients, DialogEnhancementParameterData,
    DialogEnhancementParameterUpdate, DialogEnhancementPositionUpdate,
    DialogEnhancementSimulcastData, DialogEnhancementState, DialogEnhancementStateError,
    MAX_DIALOG_ENHANCEMENT_PARAMETER_CHANNELS, MAX_DIALOG_ENHANCEMENT_PARAMETER_CODES,
};
#[cfg(feature = "audio-decode")]
pub use drc_gains::{
    MAX_PRESENTATION_DRC_BANDS, MAX_PRESENTATION_DRC_CHANNEL_GROUPS,
    MAX_PRESENTATION_DRC_GAIN_VALUES, MAX_PRESENTATION_DRC_SUBFRAMES,
    PresentationDrcDecodedGainSet, PresentationDrcGainSetExt, PresentationDrcGains,
    PresentationDrcGainsContext, PresentationDrcGainsError,
};
#[cfg(feature = "audio-decode")]
pub use huffman::{HuffmanError, HuffmanTable};
#[cfg(feature = "audio-decode")]
pub use substream_audio::{
    Ac4SubstreamAjoc, AjocAudioWorkspace, AjocSubstreamContext, SubstreamAudioError,
    parse_substream_ajoc,
};
