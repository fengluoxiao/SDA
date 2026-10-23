//! A-JOC 高级联合对象编码。
//!
//! `TS103190-2:v1.3.1` 的 `6.2.5` 给出语法，`6.3.6` 给出语义，码本见附录
//! `A.1.1`。覆盖 `ajoc()`、`ajoc_ctrl_info()`、`ajoc_data()`、
//! `ajoc_data_point_info()` 与 `ajoc_huff_data()`。
//!
//! 本模块只做语法遍历与原始码值保留：混合矩阵留在量化域，既不反量化也不
//! 做时间插值。
//!
//! 逐对象矩阵由调用方提供。上混信号数没有位宽上界——
//! `n_fullband_upmix_signals_minus1` 占 4 位，取满时还能由 `variable_bits(3)`
//! 继续加大——因此工作区容量交由调用方决定，超出即报错而非静默截断。

use crate::huffman::{HuffmanError, HuffmanTable, tables};
use core::fmt;
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

// 这些定义在无 `audio-decode` feature 时也供纯推导层使用；这里重新导出以保留
// 早期版本留下的 `ajoc::syntax::*` 路径。
pub use super::{
    MAX_AJOC_BANDS, MAX_AJOC_DMX_SIGNALS, MAX_DATA_POINTS, MAX_DECORRELATORS, MatrixKind,
};

/// A-JOC 解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AjocError {
    /// 读取比特时越过了数据末尾。
    Read(ReadError),
    /// Huffman 解码失败。
    Huffman(HuffmanError),
    /// 下混信号数超过 [`MAX_AJOC_DMX_SIGNALS`]。
    DmxSignalsOutOfRange {
        /// 传入的值。
        num_dmx_signals: u8,
        /// 规范上界。
        limit: u8,
    },
    /// 上混信号数无法表示为当前平台的工作区长度。
    UmxSignalsOutOfRange {
        /// 传入的值。
        num_umx_signals: u32,
    },
    /// 调用方提供的逐对象矩阵不足。
    ObjectWorkspaceTooSmall {
        /// 需要的对象数。
        needed: usize,
        /// 实际提供的个数。
        provided: usize,
    },
}

impl fmt::Display for AjocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AjocError::Read(error) => write!(f, "{error}"),
            AjocError::Huffman(error) => write!(f, "{error}"),
            AjocError::DmxSignalsOutOfRange {
                num_dmx_signals,
                limit,
            } => write!(
                f,
                "Downmix signal count {num_dmx_signals} exceeds limit {limit}"
            ),
            AjocError::UmxSignalsOutOfRange { num_umx_signals } => {
                write!(
                    f,
                    "Upmix signal count {num_umx_signals} cannot be represented as a workspace length"
                )
            }
            AjocError::ObjectWorkspaceTooSmall { needed, provided } => {
                write!(
                    f,
                    "Element requires {needed} object matrices, but only {provided} were provided"
                )
            }
        }
    }
}

impl core::error::Error for AjocError {}

impl From<ReadError> for AjocError {
    fn from(error: ReadError) -> Self {
        AjocError::Read(error)
    }
}

impl From<HuffmanError> for AjocError {
    fn from(error: HuffmanError) -> Self {
        AjocError::Huffman(error)
    }
}

/// 表 78：`ajoc_num_bands_code` 到 `ajoc_num_bands` 的映射。
///
/// 该表**单调递减**：码值越大频带越少，与直觉相反，故单列成表而非公式。
const AJOC_NUM_BANDS: [u8; 8] = [23, 15, 12, 9, 7, 5, 3, 1];

/// 由 `ajoc_num_bands_code` 取出 `ajoc_num_bands`，见表 78。
#[must_use]
pub fn ajoc_num_bands(code: u8) -> Option<u8> {
    AJOC_NUM_BANDS.get(usize::from(code)).copied()
}

/// A-JOC 码本的差分方向，见 `Pseudocode 27` 的 `hcb_type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AjocHcbType {
    /// 频带方向的首个值。
    F0,
    /// 频带方向的差值。
    Df,
    /// 时间方向的差值。
    Dt,
}

/// `get_ajoc_hcb()`，见 `Pseudocode 27`。
///
/// `quant_select` 按表 79 取值：**假为 Fine，真为 Coarse**——与
/// `aspx_quant_mode_env` 的方向相反，故此处不复用后者的布尔约定。
#[must_use]
pub fn table_for(kind: MatrixKind, coarse: bool, hcb: AjocHcbType) -> &'static HuffmanTable {
    use AjocHcbType::{Df, Dt, F0};
    use MatrixKind::{Dry, Wet};
    match (kind, coarse, hcb) {
        (Dry, true, F0) => &tables::AJOC_HCB_DRY_COARSE_F0,
        (Dry, true, Df) => &tables::AJOC_HCB_DRY_COARSE_DF,
        (Dry, true, Dt) => &tables::AJOC_HCB_DRY_COARSE_DT,
        (Dry, false, F0) => &tables::AJOC_HCB_DRY_FINE_F0,
        (Dry, false, Df) => &tables::AJOC_HCB_DRY_FINE_DF,
        (Dry, false, Dt) => &tables::AJOC_HCB_DRY_FINE_DT,
        (Wet, true, F0) => &tables::AJOC_HCB_WET_COARSE_F0,
        (Wet, true, Df) => &tables::AJOC_HCB_WET_COARSE_DF,
        (Wet, true, Dt) => &tables::AJOC_HCB_WET_COARSE_DT,
        (Wet, false, F0) => &tables::AJOC_HCB_WET_FINE_F0,
        (Wet, false, Df) => &tables::AJOC_HCB_WET_FINE_DF,
        (Wet, false, Dt) => &tables::AJOC_HCB_WET_FINE_DT,
    }
}

/// `huff_decode_diff()` 要减去的码本偏移 `cb_off`。
///
/// **与 A-SPX 的规律不同**，不可套用：附录 `A.1.1` 给出的十二个偏移里，
/// `F0` 与 `DF` 一律为 0，只有 `DT` 取符号数的一半。相应地
/// `len_DF == len_F0` 而 `len_DT == 2 * len_F0 - 1`。参见
/// [`crate::aspx::codebooks::cb_off`]，那里 `DF` 与 `DT` 都带偏移。
#[must_use]
pub fn cb_off(kind: MatrixKind, coarse: bool, hcb: AjocHcbType) -> i16 {
    match hcb {
        AjocHcbType::F0 | AjocHcbType::Df => 0,
        AjocHcbType::Dt => {
            let len = table_for(kind, coarse, hcb).len();
            i16::try_from(len.saturating_sub(1) / 2).unwrap_or(0)
        }
    }
}

/// `ajoc_data_point_info()` 的解析结果，见 `6.2.5.4`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AjocDataPoints {
    /// `ajoc_num_dpoints`，2 位。
    pub count: u8,
    start_pos: [u8; MAX_DATA_POINTS],
    ramp_len_minus1: [u8; MAX_DATA_POINTS],
}

impl AjocDataPoints {
    /// `ajoc_start_pos[dp]`，5 位。
    #[must_use]
    pub fn start_pos(&self, dp: usize) -> Option<u8> {
        if dp >= usize::from(self.count) {
            return None;
        }
        self.start_pos.get(dp).copied()
    }

    /// `ajoc_ramp_len_minus1[dp]`，6 位。
    #[must_use]
    pub fn ramp_len_minus1(&self, dp: usize) -> Option<u8> {
        if dp >= usize::from(self.count) {
            return None;
        }
        self.ramp_len_minus1.get(dp).copied()
    }

    #[cfg(test)]
    pub(crate) const fn with_count_for_test(count: u8) -> Self {
        Self {
            count,
            start_pos: [0; MAX_DATA_POINTS],
            ramp_len_minus1: [0; MAX_DATA_POINTS],
        }
    }

    /// 解析 `ajoc_data_point_info()`。
    fn parse(reader: &mut BitReader<'_>) -> Result<Self, AjocError> {
        let mut out = Self {
            count: read_u8(reader, 2)?,
            ..Self::default()
        };
        for dp in 0..usize::from(out.count) {
            let start = read_u8(reader, 5)?;
            let ramp = read_u8(reader, 6)?;
            if let Some(slot) = out.start_pos.get_mut(dp) {
                *slot = start;
            }
            if let Some(slot) = out.ramp_len_minus1.get_mut(dp) {
                *slot = ramp;
            }
        }
        Ok(out)
    }
}

/// 一个上混对象的 A-JOC 控制信息，见 `6.2.5.2`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AjocObjectControl {
    /// `ajoc_object_present[o]`。
    pub present: bool,
    /// `ajoc_num_bands[o]`，由表 78 的码值取出。
    pub num_bands: u8,
    /// `ajoc_quant_select[o]`：**假为 Fine，真为 Coarse**，见表 79。
    pub coarse: bool,
    /// `ajoc_sparse_select[o]`。
    pub sparse: bool,
    dry_present: [bool; MAX_AJOC_DMX_SIGNALS],
    wet_present: [bool; MAX_DECORRELATORS],
}

impl AjocObjectControl {
    /// `ajoc_mix_mtx_dry_present[o][ch]`。
    ///
    /// 非稀疏模式下全部下混信号都有数据，故恒为真。
    #[must_use]
    pub fn dry_present(&self, ch: usize) -> bool {
        if !self.sparse {
            return self.present;
        }
        self.dry_present.get(ch).copied().unwrap_or(false)
    }

    /// `ajoc_mix_mtx_wet_present[o][de]`。
    #[must_use]
    pub fn wet_present(&self, de: usize) -> bool {
        self.wet_present.get(de).copied().unwrap_or(false)
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        present: bool,
        num_bands: u8,
        coarse: bool,
        sparse: bool,
        dry_present: &[bool],
        wet_present: &[bool],
    ) -> Self {
        let mut out = Self {
            present,
            num_bands,
            coarse,
            sparse,
            ..Self::default()
        };
        for (target, source) in out.dry_present.iter_mut().zip(dry_present) {
            *target = *source;
        }
        for (target, source) in out.wet_present.iter_mut().zip(wet_present) {
            *target = *source;
        }
        out
    }
}

/// 一个上混对象的量化混合矩阵。
///
/// 值是 Huffman 符号下标减去 `cb_off` 的结果，未做增量还原，也未反量化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AjocObjectMatrix {
    dry: [[[i16; MAX_AJOC_BANDS]; MAX_AJOC_DMX_SIGNALS]; MAX_DATA_POINTS],
    wet: [[[i16; MAX_AJOC_BANDS]; MAX_DECORRELATORS]; MAX_DATA_POINTS],
    dry_time_direction: [[bool; MAX_AJOC_DMX_SIGNALS]; MAX_DATA_POINTS],
    wet_time_direction: [[bool; MAX_DECORRELATORS]; MAX_DATA_POINTS],
    num_bands: u8,
    num_dpoints: u8,
    num_dmx: u8,
    num_decorr: u8,
}

impl AjocObjectMatrix {
    /// 一个全零矩阵，供调用方预留工作区。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dry: [[[0; MAX_AJOC_BANDS]; MAX_AJOC_DMX_SIGNALS]; MAX_DATA_POINTS],
            wet: [[[0; MAX_AJOC_BANDS]; MAX_DECORRELATORS]; MAX_DATA_POINTS],
            dry_time_direction: [[false; MAX_AJOC_DMX_SIGNALS]; MAX_DATA_POINTS],
            wet_time_direction: [[false; MAX_DECORRELATORS]; MAX_DATA_POINTS],
            num_bands: 0,
            num_dpoints: 0,
            num_dmx: 0,
            num_decorr: 0,
        }
    }

    /// `mix_mtx_dry[o][dp][ch][band]`。
    #[must_use]
    pub fn dry(&self, dp: usize, ch: usize, band: usize) -> Option<i16> {
        if dp >= usize::from(self.num_dpoints)
            || ch >= usize::from(self.num_dmx)
            || band >= usize::from(self.num_bands)
        {
            return None;
        }
        self.dry.get(dp)?.get(ch)?.get(band).copied()
    }

    /// `mix_mtx_wet[o][dp][de][band]`。
    #[must_use]
    pub fn wet(&self, dp: usize, de: usize, band: usize) -> Option<i16> {
        if dp >= usize::from(self.num_dpoints)
            || de >= usize::from(self.num_decorr)
            || band >= usize::from(self.num_bands)
        {
            return None;
        }
        self.wet.get(dp)?.get(de)?.get(band).copied()
    }

    /// 对应 dry 矩阵段的 `diff_type`；真表示时间方向，假表示频带方向。
    ///
    /// 稀疏模式下未传输的段应结合 [`AjocObjectControl::dry_present`] 判断。
    #[must_use]
    pub fn dry_time_direction(&self, dp: usize, ch: usize) -> Option<bool> {
        if dp >= usize::from(self.num_dpoints)
            || ch >= usize::from(self.num_dmx)
            || self.num_bands == 0
        {
            return None;
        }
        self.dry_time_direction.get(dp)?.get(ch).copied()
    }

    /// 对应 wet 矩阵段的 `diff_type`；真表示时间方向，假表示频带方向。
    ///
    /// 稀疏模式下未传输的段应结合 [`AjocObjectControl::wet_present`] 判断。
    #[must_use]
    pub fn wet_time_direction(&self, dp: usize, de: usize) -> Option<bool> {
        if dp >= usize::from(self.num_dpoints)
            || de >= usize::from(self.num_decorr)
            || self.num_bands == 0
        {
            return None;
        }
        self.wet_time_direction.get(dp)?.get(de).copied()
    }

    /// 本对象的参数频带数。
    #[must_use]
    pub const fn num_bands(&self) -> u8 {
        self.num_bands
    }

    #[cfg(test)]
    pub(crate) fn for_test(num_bands: u8, num_dpoints: u8, num_dmx: u8, num_decorr: u8) -> Self {
        Self {
            num_bands,
            num_dpoints,
            num_dmx,
            num_decorr,
            ..Self::new()
        }
    }

    #[cfg(test)]
    pub(crate) fn set_dry_for_test(
        &mut self,
        dp: usize,
        ch: usize,
        values: &[i16],
        time_direction: bool,
    ) {
        if let Some(row) = self.dry.get_mut(dp).and_then(|plane| plane.get_mut(ch)) {
            for (target, source) in row.iter_mut().zip(values) {
                *target = *source;
            }
        }
        if let Some(direction) = self
            .dry_time_direction
            .get_mut(dp)
            .and_then(|items| items.get_mut(ch))
        {
            *direction = time_direction;
        }
    }

    #[cfg(test)]
    pub(crate) fn set_wet_for_test(
        &mut self,
        dp: usize,
        de: usize,
        values: &[i16],
        time_direction: bool,
    ) {
        if let Some(row) = self.wet.get_mut(dp).and_then(|plane| plane.get_mut(de)) {
            for (target, source) in row.iter_mut().zip(values) {
                *target = *source;
            }
        }
        if let Some(direction) = self
            .wet_time_direction
            .get_mut(dp)
            .and_then(|items| items.get_mut(de))
        {
            *direction = time_direction;
        }
    }
}

impl Default for AjocObjectMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// 一个 `ajoc()` 元素的解析结果摘要。
///
/// 逐对象的矩阵留在调用方提供的工作区里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ajoc {
    /// `ajoc_num_decorr`，3 位。
    pub num_decorr: u8,
    /// `ajoc_b_nodt`：为真时首个数据点强制走频带方向差分。
    pub b_nodt: bool,
    /// 数据点信息。
    pub data_points: AjocDataPoints,
    /// 上混对象数。
    pub num_umx_signals: u32,
    /// 下混信号数。
    pub num_dmx_signals: u8,
    decorr_enable: [bool; MAX_DECORRELATORS],
}

impl Ajoc {
    /// `ajoc_decorr_enable[d]`。
    #[must_use]
    pub fn decorr_enable(&self, de: usize) -> Option<bool> {
        if de >= usize::from(self.num_decorr) {
            return None;
        }
        self.decorr_enable.get(de).copied()
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        num_decorr: u8,
        num_dpoints: u8,
        num_umx_signals: u32,
        num_dmx_signals: u8,
    ) -> Self {
        Self {
            num_decorr,
            b_nodt: false,
            data_points: AjocDataPoints::with_count_for_test(num_dpoints),
            num_umx_signals,
            num_dmx_signals,
            decorr_enable: [true; MAX_DECORRELATORS],
        }
    }

    #[cfg(test)]
    pub(crate) fn set_decorr_enable_for_test(&mut self, de: usize, enabled: bool) {
        if let Some(slot) = self.decorr_enable.get_mut(de) {
            *slot = enabled;
        }
    }
}

/// 解析 `ajoc()`，见 `6.2.5.1`。
///
/// `controls` 与 `matrices` 由调用方提供，长度须不小于 `num_umx_signals`；
/// 两者按对象下标填充，可跨帧复用。
///
/// # Errors
///
/// 见 [`AjocError`]。
pub fn parse_ajoc(
    reader: &mut BitReader<'_>,
    num_dmx_signals: u8,
    num_umx_signals: u32,
    controls: &mut [AjocObjectControl],
    matrices: &mut [AjocObjectMatrix],
) -> Result<Ajoc, AjocError> {
    if usize::from(num_dmx_signals) > MAX_AJOC_DMX_SIGNALS {
        return Err(AjocError::DmxSignalsOutOfRange {
            num_dmx_signals,
            limit: u8::try_from(MAX_AJOC_DMX_SIGNALS).unwrap_or(u8::MAX),
        });
    }
    let needed = usize::try_from(num_umx_signals)
        .map_err(|_| AjocError::UmxSignalsOutOfRange { num_umx_signals })?;
    if controls.len() < needed || matrices.len() < needed {
        return Err(AjocError::ObjectWorkspaceTooSmall {
            needed,
            provided: controls.len().min(matrices.len()),
        });
    }

    let num_decorr = read_u8(reader, 3)?;
    let mut out = Ajoc {
        num_decorr,
        b_nodt: false,
        data_points: AjocDataPoints::default(),
        num_umx_signals,
        num_dmx_signals,
        decorr_enable: [false; MAX_DECORRELATORS],
    };

    parse_ctrl_info(reader, &mut out, needed, controls)?;
    parse_data(reader, &mut out, needed, controls, matrices)?;
    Ok(out)
}

/// `ajoc_ctrl_info()`，见 `6.2.5.2`。
fn parse_ctrl_info(
    reader: &mut BitReader<'_>,
    out: &mut Ajoc,
    num_umx_signals: usize,
    controls: &mut [AjocObjectControl],
) -> Result<(), AjocError> {
    for de in 0..usize::from(out.num_decorr) {
        let enabled = reader.read_flag()?;
        if let Some(slot) = out.decorr_enable.get_mut(de) {
            *slot = enabled;
        }
    }
    for object in controls.iter_mut().take(num_umx_signals) {
        *object = AjocObjectControl {
            present: reader.read_flag()?,
            ..AjocObjectControl::default()
        };
    }

    out.data_points = AjocDataPoints::parse(reader)?;
    if out.data_points.count == 0 {
        // 没有数据点时逐对象的编码参数整段不传输。
        return Ok(());
    }

    for object in controls.iter_mut().take(num_umx_signals) {
        if !object.present {
            continue;
        }
        let code = read_u8(reader, 3)?;
        object.num_bands = ajoc_num_bands(code).unwrap_or(0);
        object.coarse = reader.read_flag()?;
        object.sparse = reader.read_flag()?;
        if !object.sparse {
            continue;
        }
        for ch in 0..usize::from(out.num_dmx_signals) {
            let present = reader.read_flag()?;
            if let Some(slot) = object.dry_present.get_mut(ch) {
                *slot = present;
            }
        }
        for de in 0..usize::from(out.num_decorr) {
            // 去相关器未启用时不传输标志，直接视为不存在。
            let present = if out.decorr_enable.get(de).copied().unwrap_or(false) {
                reader.read_flag()?
            } else {
                false
            };
            if let Some(slot) = object.wet_present.get_mut(de) {
                *slot = present;
            }
        }
    }
    Ok(())
}

/// `ajoc_data()`，见 `6.2.5.3`。
fn parse_data(
    reader: &mut BitReader<'_>,
    out: &mut Ajoc,
    num_umx_signals: usize,
    controls: &[AjocObjectControl],
    matrices: &mut [AjocObjectMatrix],
) -> Result<(), AjocError> {
    out.b_nodt = reader.read_flag()?;
    let dpoints = out.data_points.count;

    for (index, matrix) in matrices.iter_mut().take(num_umx_signals).enumerate() {
        // 矩阵按对象跨帧复用，必须整体清零：稀疏模式下未传输的项就是零，
        // 残留上一帧的值会被当作本帧的系数。
        *matrix = AjocObjectMatrix::new();
        let Some(object) = controls.get(index) else {
            continue;
        };
        matrix.num_bands = object.num_bands;
        matrix.num_dpoints = dpoints;
        matrix.num_dmx = out.num_dmx_signals;
        matrix.num_decorr = out.num_decorr;
        if !object.present {
            continue;
        }

        for dp in 0..usize::from(dpoints) {
            // 首个数据点在 ajoc_b_nodt 为真时不传输方向标志。
            let dfonly = dp == 0 && out.b_nodt;
            for ch in 0..usize::from(out.num_dmx_signals) {
                if !object.dry_present(ch) {
                    continue;
                }
                let Some(row) = matrix.dry.get_mut(dp).and_then(|plane| plane.get_mut(ch)) else {
                    continue;
                };
                let time_direction = parse_huff_data(
                    reader,
                    MatrixKind::Dry,
                    object.coarse,
                    object.num_bands,
                    dfonly,
                    row,
                )?;
                if let Some(slot) = matrix
                    .dry_time_direction
                    .get_mut(dp)
                    .and_then(|directions| directions.get_mut(ch))
                {
                    *slot = time_direction;
                }
            }
            for de in 0..usize::from(out.num_decorr) {
                // 非稀疏模式无条件传输全部去相关器；`decorr_enable` 只控制
                // 稀疏模式下是否出现逐去相关器的存在标志。
                let carried = !object.sparse || object.wet_present(de);
                if !carried {
                    continue;
                }
                let Some(row) = matrix.wet.get_mut(dp).and_then(|plane| plane.get_mut(de)) else {
                    continue;
                };
                let time_direction = parse_huff_data(
                    reader,
                    MatrixKind::Wet,
                    object.coarse,
                    object.num_bands,
                    dfonly,
                    row,
                )?;
                if let Some(slot) = matrix
                    .wet_time_direction
                    .get_mut(dp)
                    .and_then(|directions| directions.get_mut(de))
                {
                    *slot = time_direction;
                }
            }
        }
    }
    Ok(())
}

/// `ajoc_huff_data()`，见 `6.2.5.5`。
///
/// `diff_type` 为 0 走频带方向：首项用 `F0` 码本，其余用 `DF`；为 1 走时间
/// 方向，全部用 `DT`。`b_dfonly` 为真时不传输该标志，直接取频带方向。
fn parse_huff_data(
    reader: &mut BitReader<'_>,
    kind: MatrixKind,
    coarse: bool,
    num_bands: u8,
    dfonly: bool,
    out: &mut [i16; MAX_AJOC_BANDS],
) -> Result<bool, AjocError> {
    let time_direction = if dfonly { false } else { reader.read_flag()? };

    if time_direction {
        let table = table_for(kind, coarse, AjocHcbType::Dt);
        let offset = cb_off(kind, coarse, AjocHcbType::Dt);
        for band in 0..usize::from(num_bands) {
            let value = decode_one(reader, table, offset)?;
            if let Some(slot) = out.get_mut(band) {
                *slot = value;
            }
        }
        return Ok(time_direction);
    }

    let first = table_for(kind, coarse, AjocHcbType::F0);
    let value = decode_one(reader, first, cb_off(kind, coarse, AjocHcbType::F0))?;
    if let Some(slot) = out.first_mut() {
        *slot = value;
    }
    let table = table_for(kind, coarse, AjocHcbType::Df);
    let offset = cb_off(kind, coarse, AjocHcbType::Df);
    for band in 1..usize::from(num_bands) {
        let value = decode_one(reader, table, offset)?;
        if let Some(slot) = out.get_mut(band) {
            *slot = value;
        }
    }
    Ok(time_direction)
}

fn decode_one(
    reader: &mut BitReader<'_>,
    table: &'static HuffmanTable,
    offset: i16,
) -> Result<i16, AjocError> {
    let symbol = table.decode(reader)?;
    Ok(i16::try_from(symbol)
        .unwrap_or(i16::MAX)
        .saturating_sub(offset))
}

fn read_u8(reader: &mut BitReader<'_>, bits: u32) -> Result<u8, AjocError> {
    let value = reader.read_bits(bits)?;
    Ok(u8::try_from(value).unwrap_or(u8::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::huffman::tables::ALL_CODEBOOKS;

    struct BitBuf {
        bytes: [u8; 2048],
        len: usize,
    }

    impl BitBuf {
        const fn new() -> Self {
            Self {
                bytes: [0; 2048],
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

        fn push_symbol(&mut self, table: &HuffmanTable, symbol: u16) {
            for &(name, candidate, lengths, codewords) in ALL_CODEBOOKS {
                if !core::ptr::eq(candidate, table) {
                    continue;
                }
                let index = usize::from(symbol);
                let (Some(&width), Some(&codeword)) = (lengths.get(index), codewords.get(index))
                else {
                    panic!("{name} 没有第 {symbol} 个符号");
                };
                self.push_bits(codeword, u32::from(width));
                return;
            }
            panic!("码本不在 ALL_CODEBOOKS 内");
        }

        /// 一段频带方向的 `ajoc_huff_data()`：首项 `F0`，其余 `DF`。
        fn push_freq_direction(
            &mut self,
            kind: MatrixKind,
            coarse: bool,
            num_bands: u8,
            emit_flag: bool,
        ) {
            if emit_flag {
                self.push(false); // diff_type = 0
            }
            self.push_symbol(table_for(kind, coarse, AjocHcbType::F0), 0);
            for _ in 1..num_bands {
                self.push_symbol(table_for(kind, coarse, AjocHcbType::Df), 0);
            }
        }
    }

    fn workspaces() -> ([AjocObjectControl; 4], [AjocObjectMatrix; 4]) {
        (
            [AjocObjectControl::default(); 4],
            [AjocObjectMatrix::new(); 4],
        )
    }

    /// 表 78 单调递减，且首尾与规范一致。
    #[test]
    fn band_count_table_is_strictly_decreasing() {
        assert_eq!(ajoc_num_bands(0), Some(23));
        assert_eq!(ajoc_num_bands(7), Some(1));
        assert_eq!(ajoc_num_bands(8), None, "码值只有 3 位");
        for pair in AJOC_NUM_BANDS.windows(2) {
            let (Some(high), Some(low)) = (pair.first(), pair.get(1)) else {
                unreachable!("windows(2) 必有两项");
            };
            assert!(high > low, "表 78 必须严格递减：{high} 之后是 {low}");
        }
        assert!(
            usize::from(AJOC_NUM_BANDS[0]) <= MAX_AJOC_BANDS,
            "频带上界常量应覆盖表 78 的首行"
        );
    }

    /// 由生成代码取出一张码本的名字。
    fn codebook_name(table: &HuffmanTable) -> &'static str {
        for &(name, candidate, _, _) in ALL_CODEBOOKS {
            if core::ptr::eq(candidate, table) {
                return name;
            }
        }
        panic!("码本不在 ALL_CODEBOOKS 内");
    }

    /// 每个标识必须落到 `Pseudocode 27` 命名规则指出的那张码本上。
    ///
    /// 「互不相同」挡不住整组错配——把 COARSE 与 FINE 成对互换后长度关系
    /// 依旧自洽，落点判据也无感（构造与解析共用同一张映射表）。此处直接
    /// 比对生成代码里的码本名，是唯一能钉死映射的判据。
    #[test]
    fn identifiers_map_to_the_codebooks_named_in_the_specification() {
        let expected: [(MatrixKind, bool, AjocHcbType, &str); 12] = [
            (
                MatrixKind::Dry,
                true,
                AjocHcbType::F0,
                "AJOC_HCB_DRY_COARSE_F0",
            ),
            (
                MatrixKind::Dry,
                true,
                AjocHcbType::Df,
                "AJOC_HCB_DRY_COARSE_DF",
            ),
            (
                MatrixKind::Dry,
                true,
                AjocHcbType::Dt,
                "AJOC_HCB_DRY_COARSE_DT",
            ),
            (
                MatrixKind::Dry,
                false,
                AjocHcbType::F0,
                "AJOC_HCB_DRY_FINE_F0",
            ),
            (
                MatrixKind::Dry,
                false,
                AjocHcbType::Df,
                "AJOC_HCB_DRY_FINE_DF",
            ),
            (
                MatrixKind::Dry,
                false,
                AjocHcbType::Dt,
                "AJOC_HCB_DRY_FINE_DT",
            ),
            (
                MatrixKind::Wet,
                true,
                AjocHcbType::F0,
                "AJOC_HCB_WET_COARSE_F0",
            ),
            (
                MatrixKind::Wet,
                true,
                AjocHcbType::Df,
                "AJOC_HCB_WET_COARSE_DF",
            ),
            (
                MatrixKind::Wet,
                true,
                AjocHcbType::Dt,
                "AJOC_HCB_WET_COARSE_DT",
            ),
            (
                MatrixKind::Wet,
                false,
                AjocHcbType::F0,
                "AJOC_HCB_WET_FINE_F0",
            ),
            (
                MatrixKind::Wet,
                false,
                AjocHcbType::Df,
                "AJOC_HCB_WET_FINE_DF",
            ),
            (
                MatrixKind::Wet,
                false,
                AjocHcbType::Dt,
                "AJOC_HCB_WET_FINE_DT",
            ),
        ];
        for (kind, coarse, hcb, name) in expected {
            assert_eq!(
                codebook_name(table_for(kind, coarse, hcb)),
                name,
                "{kind:?}/coarse={coarse}/{hcb:?} 应映射到 {name}"
            );
        }
    }

    /// 十二个标识映射到互不相同的码本。
    #[test]
    fn every_identifier_maps_to_a_distinct_table() {
        let mut seen: [Option<&'static HuffmanTable>; 12] = [None; 12];
        let mut index = 0usize;
        for kind in [MatrixKind::Dry, MatrixKind::Wet] {
            for coarse in [false, true] {
                for hcb in [AjocHcbType::F0, AjocHcbType::Df, AjocHcbType::Dt] {
                    let table = table_for(kind, coarse, hcb);
                    assert!(
                        !seen
                            .iter()
                            .flatten()
                            .any(|&other| core::ptr::eq(other, table)),
                        "{kind:?}/{coarse}/{hcb:?} 与前面的码本重复"
                    );
                    if let Some(slot) = seen.get_mut(index) {
                        *slot = Some(table);
                    }
                    index = index.saturating_add(1);
                }
            }
        }
        assert_eq!(index, 12, "附录 A.1.1 共十二张码本");
    }

    /// A-JOC 的码本长度关系与 A-SPX 不同：`DF` 与 `F0` 等长，只有 `DT` 加倍。
    #[test]
    fn difference_codebook_lengths_differ_from_aspx() {
        for kind in [MatrixKind::Dry, MatrixKind::Wet] {
            for coarse in [false, true] {
                let base = table_for(kind, coarse, AjocHcbType::F0).len();
                let df = table_for(kind, coarse, AjocHcbType::Df).len();
                let dt = table_for(kind, coarse, AjocHcbType::Dt).len();
                assert_eq!(df, base, "{kind:?}/{coarse} 的 DF 应与 F0 等长");
                assert_eq!(
                    dt,
                    base.saturating_mul(2).saturating_sub(1),
                    "{kind:?}/{coarse} 的 DT 应为 F0 的两倍减一"
                );
            }
        }
    }

    /// `cb_off` 必须与附录 A.1.1 标注的十二个值一致。
    ///
    /// 期望值抄自 PDF，作为推算逻辑的第二来源。**`DF` 为 0 是与 A-SPX 的
    /// 关键差异**：那边 `DF` 与 `DT` 都带偏移，此处只有 `DT` 带。
    #[test]
    fn offsets_match_the_specification() {
        // (kind, coarse, F0, DF, DT)
        let expected: [(MatrixKind, bool, i16, i16, i16); 4] = [
            (MatrixKind::Dry, true, 0, 0, 50),
            (MatrixKind::Dry, false, 0, 0, 100),
            (MatrixKind::Wet, true, 0, 0, 20),
            (MatrixKind::Wet, false, 0, 0, 40),
        ];
        for (kind, coarse, f0, df, dt) in expected {
            assert_eq!(cb_off(kind, coarse, AjocHcbType::F0), f0);
            assert_eq!(
                cb_off(kind, coarse, AjocHcbType::Df),
                df,
                "{kind:?}/{coarse} 的 DF 偏移必须为零"
            );
            assert_eq!(cb_off(kind, coarse, AjocHcbType::Dt), dt);
        }
    }

    /// `ajoc_data_point_info()` 每个数据点恰好占 11 位。
    #[test]
    fn data_point_info_consumes_eleven_bits_each() {
        for count in 0u32..=3 {
            let mut buf = BitBuf::new();
            buf.push_bits(count, 2);
            for dp in 0..count {
                buf.push_bits(dp, 5); // start_pos
                buf.push_bits(dp + 1, 6); // ramp_len_minus1
            }
            let expected = buf.len;

            let mut reader = BitReader::new(buf.as_slice());
            let info = AjocDataPoints::parse(&mut reader).expect("应能解析");
            assert_eq!(reader.bit_position(), u64::try_from(expected).unwrap_or(0));
            assert_eq!(info.count, u8::try_from(count).unwrap_or(0));
            for dp in 0..count as usize {
                assert_eq!(info.start_pos(dp), u8::try_from(dp).ok());
            }
            assert_eq!(info.start_pos(count as usize), None, "越界不得暴露");
        }
    }

    /// `ajoc_num_dpoints` 为零时逐对象的编码参数整段不传输。
    #[test]
    fn zero_data_points_omit_the_per_object_parameters() {
        let mut buf = BitBuf::new();
        buf.push_bits(0, 3); // ajoc_num_decorr = 0
        buf.push(true); // ajoc_object_present[0]
        buf.push(true); // ajoc_object_present[1]
        buf.push_bits(0, 2); // ajoc_num_dpoints = 0
        buf.push(false); // ajoc_b_nodt
        let expected = buf.len;

        let (mut controls, mut matrices) = workspaces();
        let mut reader = BitReader::new(buf.as_slice());
        let ajoc = parse_ajoc(&mut reader, 2, 2, &mut controls, &mut matrices).expect("应能解析");

        assert_eq!(reader.bit_position(), u64::try_from(expected).unwrap_or(0));
        assert_eq!(ajoc.data_points.count, 0);
        assert_eq!(matrices[0].dry(0, 0, 0), None, "无数据点时矩阵为空");
    }

    /// 非稀疏模式：全部下混信号与全部去相关器都有数据。
    #[test]
    fn dense_mode_carries_every_channel_and_decorrelator() {
        let num_bands = 3u8;
        let mut buf = BitBuf::new();
        buf.push_bits(2, 3); // ajoc_num_decorr = 2
        buf.push(true); // decorr_enable[0]
        buf.push(false); // decorr_enable[1]
        buf.push(true); // object_present[0]
        buf.push(false); // object_present[1]
        buf.push_bits(1, 2); // ajoc_num_dpoints = 1
        buf.push_bits(0, 5); // start_pos[0]
        buf.push_bits(0, 6); // ramp_len_minus1[0]
        buf.push_bits(6, 3); // num_bands_code = 6 → 3 个频带
        buf.push(true); // quant_select = Coarse
        buf.push(false); // sparse_select = 0
        buf.push(true); // ajoc_b_nodt = 1 → 首个数据点不传方向标志

        // 对象 0：两个下混信号的 dry，以及全部两个去相关器的 wet；
        // 即使 decorr_enable[1] 为假，非稀疏语法仍传输对应矩阵段。
        for _ in 0..2 {
            buf.push_freq_direction(MatrixKind::Dry, true, num_bands, false);
        }
        for _ in 0..2 {
            buf.push_freq_direction(MatrixKind::Wet, true, num_bands, false);
        }
        let expected = buf.len;

        let (mut controls, mut matrices) = workspaces();
        let mut reader = BitReader::new(buf.as_slice());
        let ajoc = parse_ajoc(&mut reader, 2, 2, &mut controls, &mut matrices).expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );
        assert!(ajoc.b_nodt);
        assert_eq!(ajoc.decorr_enable(0), Some(true));
        assert_eq!(ajoc.decorr_enable(1), Some(false));
        assert_eq!(ajoc.decorr_enable(2), None);
        assert_eq!(controls[0].num_bands, num_bands);
        assert!(controls[0].coarse, "quant_select 为 1 时是 Coarse");
        assert_eq!(matrices[0].dry(0, 1, 2), Some(0));
        assert_eq!(
            matrices[0].wet(0, 1, 2),
            Some(0),
            "非稀疏模式不得因去相关器未启用而跳过 wet 段"
        );
        assert_eq!(matrices[0].wet_time_direction(0, 1), Some(false));
        assert_eq!(matrices[0].dry(0, 2, 0), None, "越界不得暴露");
        assert_eq!(
            matrices[1].dry(0, 0, 0),
            None,
            "ajoc_object_present 为假的对象没有频带，不得暴露任何系数"
        );
    }

    /// 稀疏模式：逐信号的存在标志决定哪些段出现在码流里。
    #[test]
    fn sparse_mode_only_carries_flagged_entries() {
        let num_bands = 1u8;
        let mut buf = BitBuf::new();
        buf.push_bits(1, 3); // ajoc_num_decorr = 1
        buf.push(true); // decorr_enable[0]
        buf.push(true); // object_present[0]
        buf.push_bits(1, 2); // ajoc_num_dpoints = 1
        buf.push_bits(0, 5);
        buf.push_bits(0, 6);
        buf.push_bits(7, 3); // num_bands_code = 7 → 1 个频带
        buf.push(false); // quant_select = Fine
        buf.push(true); // sparse_select = 1
        buf.push(true); // dry_present[0]
        buf.push(false); // dry_present[1]
        buf.push(false); // dry_present[2]
        buf.push(true); // wet_present[0]（去相关器已启用，故传输）
        buf.push(false); // ajoc_b_nodt = 0 → 每段都带方向标志

        buf.push_freq_direction(MatrixKind::Dry, false, num_bands, true);
        buf.push_freq_direction(MatrixKind::Wet, false, num_bands, true);
        let expected = buf.len;

        let (mut controls, mut matrices) = workspaces();
        let mut reader = BitReader::new(buf.as_slice());
        parse_ajoc(&mut reader, 3, 1, &mut controls, &mut matrices).expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );
        assert!(controls[0].sparse);
        assert!(controls[0].dry_present(0));
        assert!(!controls[0].dry_present(1));
        assert!(!controls[0].coarse, "quant_select 为 0 时是 Fine");
        assert_eq!(matrices[0].dry(0, 1, 0), Some(0), "未传输的项应为零");
    }

    /// 去相关器未启用时不传输 `ajoc_mix_mtx_wet_present`。
    #[test]
    fn disabled_decorrelators_omit_their_presence_flag() {
        let mut buf = BitBuf::new();
        buf.push_bits(2, 3); // ajoc_num_decorr = 2
        buf.push(false); // decorr_enable[0] = 0
        buf.push(true); // decorr_enable[1] = 1
        buf.push(true); // object_present[0]
        buf.push_bits(1, 2); // 一个数据点
        buf.push_bits(0, 5);
        buf.push_bits(0, 6);
        buf.push_bits(7, 3); // 1 个频带
        buf.push(true); // Coarse
        buf.push(true); // sparse
        buf.push(false); // dry_present[0]
        // decorr 0 未启用，此处没有 wet_present[0]。
        buf.push(true); // wet_present[1]
        buf.push(true); // b_nodt
        buf.push_freq_direction(MatrixKind::Wet, true, 1, false);
        let expected = buf.len;

        let (mut controls, mut matrices) = workspaces();
        let mut reader = BitReader::new(buf.as_slice());
        parse_ajoc(&mut reader, 1, 1, &mut controls, &mut matrices).expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "未启用的去相关器不得占用比特"
        );
        assert!(!controls[0].wet_present(0));
        assert!(controls[0].wet_present(1));
    }

    /// `ajoc_b_nodt` 只影响首个数据点，其余数据点仍带方向标志。
    #[test]
    fn nodt_only_suppresses_the_first_data_point_flag() {
        let mut buf = BitBuf::new();
        buf.push_bits(0, 3); // 无去相关器
        buf.push(true); // object_present[0]
        buf.push_bits(2, 2); // 两个数据点
        buf.push_bits(0, 5);
        buf.push_bits(0, 6);
        buf.push_bits(1, 5);
        buf.push_bits(1, 6);
        buf.push_bits(7, 3); // 1 个频带
        buf.push(true); // Coarse
        buf.push(false); // 非稀疏
        buf.push(true); // b_nodt

        buf.push_freq_direction(MatrixKind::Dry, true, 1, false); // dp 0：无标志
        buf.push_freq_direction(MatrixKind::Dry, true, 1, true); // dp 1：有标志
        let expected = buf.len;

        let (mut controls, mut matrices) = workspaces();
        let mut reader = BitReader::new(buf.as_slice());
        parse_ajoc(&mut reader, 1, 1, &mut controls, &mut matrices).expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "只有首个数据点省略方向标志"
        );
        assert_eq!(matrices[0].dry(1, 0, 0), Some(0));
        assert_eq!(matrices[0].dry_time_direction(0, 0), Some(false));
        assert_eq!(matrices[0].dry_time_direction(1, 0), Some(false));
        assert_eq!(matrices[0].dry(2, 0, 0), None);
    }

    /// 时间方向的数据段用 `DT` 码本，且偏移非零。
    #[test]
    fn time_direction_uses_the_offset_codebook() {
        let mut buf = BitBuf::new();
        buf.push_bits(0, 3);
        buf.push(true); // object_present[0]
        buf.push_bits(1, 2);
        buf.push_bits(0, 5);
        buf.push_bits(0, 6);
        buf.push_bits(7, 3); // 1 个频带
        buf.push(true); // Coarse
        buf.push(false); // 非稀疏
        buf.push(false); // b_nodt = 0
        buf.push(true); // diff_type = 1 → 时间方向
        buf.push_symbol(table_for(MatrixKind::Dry, true, AjocHcbType::Dt), 0);
        let expected = buf.len;

        let (mut controls, mut matrices) = workspaces();
        let mut reader = BitReader::new(buf.as_slice());
        parse_ajoc(&mut reader, 1, 1, &mut controls, &mut matrices).expect("应能解析");

        assert_eq!(reader.bit_position(), u64::try_from(expected).unwrap_or(0));
        // 符号 0 减去 cb_off 50 得 −50，验证偏移确实被应用。
        assert_eq!(matrices[0].dry(0, 0, 0), Some(-50));
        assert_eq!(
            matrices[0].dry_time_direction(0, 0),
            Some(true),
            "原始差值必须连同时间方向一起保留"
        );
    }

    /// 工作区不足与下混信号数越界都必须在读取任何比特前拒绝。
    #[test]
    fn rejects_bad_inputs_before_reading() {
        let buf = BitBuf::new();
        let (mut controls, mut matrices) = workspaces();

        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_ajoc(&mut reader, 17, 1, &mut controls, &mut matrices),
            Err(AjocError::DmxSignalsOutOfRange {
                num_dmx_signals: 17,
                limit: 16
            })
        );
        assert_eq!(reader.bit_position(), 0);

        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_ajoc(&mut reader, 2, 5, &mut controls, &mut matrices),
            Err(AjocError::ObjectWorkspaceTooSmall {
                needed: 5,
                provided: 4
            })
        );
        assert_eq!(reader.bit_position(), 0);

        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_ajoc(&mut reader, 2, 256, &mut controls, &mut matrices),
            Err(AjocError::ObjectWorkspaceTooSmall {
                needed: 256,
                provided: 4
            }),
            "上混对象数不得在进入工作区检查前截断成 u8"
        );
        assert_eq!(reader.bit_position(), 0);
    }

    /// 矩阵按对象跨帧复用时必须整体清零。
    ///
    /// 稀疏模式下未传输的项就是零；残留上一帧的系数会被当作本帧的值。
    #[test]
    fn reused_matrices_are_cleared_between_frames() {
        // 第一帧：非稀疏，两个下混信号都写入非零值。
        let mut first = BitBuf::new();
        first.push_bits(0, 3); // 无去相关器
        first.push(true); // object_present[0]
        first.push_bits(1, 2); // 一个数据点
        first.push_bits(0, 5);
        first.push_bits(0, 6);
        first.push_bits(7, 3); // 1 个频带
        first.push(true); // Coarse
        first.push(false); // 非稀疏
        first.push(true); // b_nodt：首个数据点不带方向标志
        for _ in 0..2 {
            first.push_symbol(table_for(MatrixKind::Dry, true, AjocHcbType::F0), 1);
        }

        let (mut controls, mut matrices) = workspaces();
        let mut reader = BitReader::new(first.as_slice());
        parse_ajoc(&mut reader, 2, 1, &mut controls, &mut matrices).expect("首帧应能解析");
        assert_eq!(matrices[0].dry(0, 1, 0), Some(1), "首帧应写入非零值");

        // 第二帧：稀疏模式，只有第 0 个下混信号有数据。
        let mut second = BitBuf::new();
        second.push_bits(0, 3);
        second.push(true);
        second.push_bits(1, 2);
        second.push_bits(0, 5);
        second.push_bits(0, 6);
        second.push_bits(7, 3);
        second.push(true);
        second.push(true); // 稀疏
        second.push(true); // dry_present[0]
        second.push(false); // dry_present[1]
        second.push(true); // b_nodt
        second.push_symbol(table_for(MatrixKind::Dry, true, AjocHcbType::F0), 0);

        let mut reader = BitReader::new(second.as_slice());
        parse_ajoc(&mut reader, 2, 1, &mut controls, &mut matrices).expect("次帧应能解析");
        assert_eq!(
            matrices[0].dry(0, 1, 0),
            Some(0),
            "未传输的项必须是零，而非上一帧的残留"
        );
    }
}
