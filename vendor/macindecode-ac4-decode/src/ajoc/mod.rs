//! A-JOC 高级联合对象编码。
//!
//! `TS103190-2:v1.3.1` 的 `6.2.5` 给出语法、`6.3.6` 给出语义，重建过程在
//! `5.7.2.3` 与 `5.7.3`，码本见附录 `A.1.1`。
//!
//! 推导层不需要 Huffman 码本，但需要用户从官方 PDF 本地生成的表，故位于
//! `spec-tables` feature 下：
//!
//! - [`bands`]：`5.7.3.1` 与表 28 的参数频带到 QMF 子带映射，即
//!   `Pseudocode 18` 的 `sb_to_pb()`。
//! - [`dequant`]：`5.7.3.3` 表 29–32 的 dry/wet 均匀反量化。
//!
//! 解码层需要附录 `A.1.1` 的码本，因此只在 `audio-decode` feature 下存在
//! （模块名不写成文档链接，默认配置下它们并不存在）：
//!
//! - `syntax`：`6.2.5` 的 `ajoc()`、`ajoc_ctrl_info()`、`ajoc_data()`、
//!   `ajoc_data_point_info()` 与 `ajoc_huff_data()`。混合矩阵停在量化域，
//!   值是 Huffman 符号下标减去 `cb_off` 的结果，既不做差分还原也不反量化；
//! - `diff`：`5.7.3.2` `Pseudocode 16` 的频率/时间方向差分与跨帧量化历史；
//! - `interp`：`5.7.3.4` `Pseudocode 17` 的 rolling 时间插值与 substream 共享
//!   ramp 游标；
//! - `decorrelator`：`5.7.3.5` 与 Part 1 `5.7.7.4` 的三组 all-pass IIR、
//!   七路 D0/D2/D1 循环和逐 QMF 时隙 transient ducker；
//! - `reconstruction`：`5.7.3.6` `Pseudocode 18` 的对象 QMF 矩阵重建，统一串接
//!   差分、反量化、rolling 插值、pre 矩阵与去相关器；
//! - `de`：`6.2.3.5`–`6.2.3.6` 的对话增强与床信息。
//!
//! 语法层的公共项在本模块根上重新导出，`ajoc::` 因此仍是它们的规范路径；
//! `de` 自成一节，按子模块访问。
//!
//! QMF 控制延迟、LFE 插回和终端 QMF 合成由 CLI 产品驱动层接通，不属于纯矩阵
//! 原语的状态所有权。

#[cfg(feature = "spec-tables")]
pub mod bands;
#[cfg(feature = "audio-decode")]
pub mod de;
#[cfg(feature = "audio-decode")]
pub mod decorrelator;
#[cfg(feature = "spec-tables")]
pub mod dequant;
#[cfg(feature = "audio-decode")]
pub mod diff;
#[cfg(feature = "audio-decode")]
pub mod interp;
#[cfg(feature = "audio-decode")]
pub mod reconstruction;
#[cfg(feature = "audio-decode")]
pub mod syntax;

/// `ajoc_num_dpoints` 的上界。该字段占 2 位，故数据点不超过 3 个。
pub const MAX_DATA_POINTS: usize = 3;

/// `ajoc_num_decorr` 的上界。该字段占 3 位，故去相关器不超过 7 个。
pub const MAX_DECORRELATORS: usize = 7;

/// 一个对象的最大参数频带数，见表 78 的首行。
pub const MAX_AJOC_BANDS: usize = 23;

/// `ajoc()` 可接受的最大下混信号数。
///
/// `n_fullband_dmx_signals_minus1` 占 4 位，故为 16。
pub const MAX_AJOC_DMX_SIGNALS: usize = 16;

/// A-JOC 混合矩阵的数据类型，见 `Pseudocode 27` 的 `data_type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixKind {
    /// 直达分量。
    Dry,
    /// 去相关分量。
    Wet,
}

#[cfg(feature = "audio-decode")]
pub use syntax::{
    Ajoc, AjocDataPoints, AjocError, AjocHcbType, AjocObjectControl, AjocObjectMatrix,
    ajoc_num_bands, cb_off, parse_ajoc, table_for,
};
