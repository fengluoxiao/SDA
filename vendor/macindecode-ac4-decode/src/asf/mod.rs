//! ASF 音频频谱前端。
//!
//! `TS103190-1:v1.4.1` 的 `4.2.8` 给出语法，`4.3.6` 给出语义与派生量。
//!
//! - `tables`：附录 B 的尺度因子频带表、表 99–110 的派生量、表 A.2–A.15 的
//!   码本元数据；
//! - `framing`：`asf_transform_info()` 与 `asf_psy_info()` 的解析，以及
//!   `Pseudocode 2`–`Pseudocode 5` 的窗口分组与 `sect_sfb_offset` 推导。
//!
//! - `spectrum`：`sf_data(ASF)` 的四个熵编码元素。它需要附录 A 的 Huffman
//!   码本，因此置于 `audio-decode` feature 下；PDF 表驱动的框架与 IMDCT 置于
//!   `spec-tables` feature 下。
//! - [`dequant`]：`5.1.3.2` 的反量化表，由构建脚本以整数判据生成，不依赖
//!   ETSI 附件，故不受 feature 约束；
//! - `reconstruct`：`5.1.3` 的量化重建与缩放，以及 `5.1.5` 的谱解组；
//! - `imdct`：`5.5` 的块切换、窗口序列与 crate 内 Stockham IFFT。

pub mod dequant;
#[cfg(feature = "spec-tables")]
pub mod framing;
#[cfg(feature = "spec-tables")]
pub mod imdct;
#[cfg(feature = "audio-decode")]
pub mod reconstruct;
#[cfg(feature = "audio-decode")]
pub mod spectrum;
#[cfg(feature = "spec-tables")]
pub mod tables;

#[cfg(feature = "audio-decode")]
pub use reconstruct::{MAX_SCALE_FACTOR, ReconstructError, ScaleFactors, scale_factors};
#[cfg(feature = "audio-decode")]
pub use spectrum::{
    AsfSpectrumError, AsfWorkspace, MAX_EXT_PREFIX, MAX_QUANT_MAGNITUDE, MAX_SPECTRAL_LINES,
    Section, coded_band_count,
};

#[cfg(feature = "spec-tables")]
pub use framing::{
    AsfError, AsfFraming, AsfPsyInfo, AsfTransformInfo, AsfWindowLayout, MAX_SFB, MAX_WINDOWS,
};
#[cfg(feature = "spec-tables")]
pub use tables::{
    NUM_SFB_48, SpectrumCodebook, TRANSFORM_LENGTHS_48, n_grp_bits_long_base,
    n_grp_bits_short_base, n_msfb_bits_48, n_msfbl_bits_48, n_side_bits_48, num_sfb_48,
    num_windows_first_half, sfb_offsets_48, spectrum_codebook, transform_length_48,
};
