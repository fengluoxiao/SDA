//! A-JOC dry/wet 矩阵参数的差分解码。
//!
//! `TS103190-2:v1.3.1:5.7.3.2` 的 `Pseudocode 16` 把 Huffman 输出还原为量化
//! 矩阵。时间方向依赖上一数据点，首个数据点还依赖上一 AC-4 帧；调用方因此为
//! 每个对象持有一份 [`DiffState`]。入口先在状态副本上预检整帧，确认后续阶段
//! 不会失败，才同时改写状态与输出。

use super::dequant::quantized_levels;
use super::syntax::{Ajoc, AjocObjectControl, AjocObjectMatrix};
use super::{MAX_AJOC_BANDS, MAX_AJOC_DMX_SIGNALS, MAX_DATA_POINTS, MAX_DECORRELATORS, MatrixKind};
use core::fmt;

/// 差分解码后的单个对象量化矩阵。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantizedObjectMatrix {
    dry: [[[i16; MAX_AJOC_BANDS]; MAX_AJOC_DMX_SIGNALS]; MAX_DATA_POINTS],
    wet: [[[i16; MAX_AJOC_BANDS]; MAX_DECORRELATORS]; MAX_DATA_POINTS],
    num_bands: u8,
    num_dpoints: u8,
    num_dmx: u8,
    num_decorr: u8,
}

impl QuantizedObjectMatrix {
    /// 空矩阵；供调用方按对象数预留工作区。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dry: [[[0; MAX_AJOC_BANDS]; MAX_AJOC_DMX_SIGNALS]; MAX_DATA_POINTS],
            wet: [[[0; MAX_AJOC_BANDS]; MAX_DECORRELATORS]; MAX_DATA_POINTS],
            num_bands: 0,
            num_dpoints: 0,
            num_dmx: 0,
            num_decorr: 0,
        }
    }

    /// 完整量化 dry 系数。
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

    /// 完整量化 wet 系数。
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

    /// 该对象当前携带的参数频带数；inactive/零数据点对象为零。
    #[must_use]
    pub const fn num_bands(&self) -> u8 {
        self.num_bands
    }

    /// 当前帧数据点数。
    #[must_use]
    pub const fn num_dpoints(&self) -> u8 {
        self.num_dpoints
    }
}

impl Default for QuantizedObjectMatrix {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HistoryRow {
    values: [i16; MAX_AJOC_BANDS],
    num_bands: u8,
    coarse: bool,
    valid: bool,
}

impl HistoryRow {
    const fn new() -> Self {
        Self {
            values: [0; MAX_AJOC_BANDS],
            num_bands: 0,
            coarse: false,
            valid: false,
        }
    }

    fn compatible(&self, num_bands: u8, coarse: bool) -> bool {
        self.valid && self.num_bands == num_bands && self.coarse == coarse
    }

    fn mark_shape(&mut self, num_bands: u8, coarse: bool) {
        self.num_bands = num_bands;
        self.coarse = coarse;
        self.valid = true;
    }

    fn commit(&mut self, values: &[i16; MAX_AJOC_BANDS], num_bands: u8, coarse: bool) {
        self.values = *values;
        self.mark_shape(num_bands, coarse);
    }
}

/// 一个对象跨 AC-4 帧保存的量化矩阵历史。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffState {
    dry: [HistoryRow; MAX_AJOC_DMX_SIGNALS],
    wet: [HistoryRow; MAX_DECORRELATORS],
}

impl DiffState {
    /// 空历史。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dry: [HistoryRow::new(); MAX_AJOC_DMX_SIGNALS],
            wet: [HistoryRow::new(); MAX_DECORRELATORS],
        }
    }

    /// 丢弃该对象的全部跨帧历史。
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for DiffState {
    fn default() -> Self {
        Self::new()
    }
}

/// A-JOC 差分解码失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffError {
    /// `Ajoc` 声明的对象数无法表示为本平台长度。
    ObjectCountOutOfRange { objects: u32 },
    /// 调用方提供的四个逐对象切片至少有一个不足。
    ObjectWorkspaceTooSmall {
        needed: usize,
        controls: usize,
        raw: usize,
        states: usize,
        output: usize,
    },
    /// `Ajoc` 的固定上界字段越界。
    DimensionOutOfRange {
        num_dpoints: u8,
        num_dmx: u8,
        num_decorr: u8,
    },
    /// active 对象的数据点存在，却没有合法参数频带数。
    BandCountOutOfRange { object: usize, num_bands: u8 },
    /// 原始矩阵的声明形状或某个所需元素与 `Ajoc`/control 不一致。
    MissingRawValue {
        kind: MatrixKind,
        object: usize,
        data_point: usize,
        row: usize,
        band: Option<usize>,
    },
    /// `DIFF_FREQ` 的 F0 绝对值不在量化表内。
    AbsoluteOutOfRange {
        kind: MatrixKind,
        object: usize,
        data_point: usize,
        row: usize,
        value: i16,
        levels: i16,
    },
    /// `DIFF_TIME` 找不到量化模式和频带形状都兼容的上一组值。
    IncompatibleHistory {
        kind: MatrixKind,
        object: usize,
        data_point: usize,
        row: usize,
        num_bands: u8,
        coarse: bool,
    },
    /// `DIFF_TIME` 的历史值与差值相加后不在量化表内。
    TimeResultOutOfRange {
        kind: MatrixKind,
        object: usize,
        data_point: usize,
        row: usize,
        band: usize,
        previous: i16,
        delta: i16,
        levels: i16,
    },
}

impl fmt::Display for DiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectCountOutOfRange { objects } => {
                write!(
                    f,
                    "A-JOC object count {objects} cannot be represented as a workspace length"
                )
            }
            Self::ObjectWorkspaceTooSmall {
                needed,
                controls,
                raw,
                states,
                output,
            } => write!(
                f,
                "A-JOC differencing requires {needed} objects; control/raw/state/output provide {controls}/{raw}/{states}/{output}"
            ),
            Self::DimensionOutOfRange {
                num_dpoints,
                num_dmx,
                num_decorr,
            } => write!(
                f,
                "A-JOC differencing dimensions out of range: dpoints={num_dpoints}, dmx={num_dmx}, decorr={num_decorr}"
            ),
            Self::BandCountOutOfRange { object, num_bands } => {
                write!(
                    f,
                    "Parameter-band count {num_bands} for A-JOC object {object} is out of range"
                )
            }
            Self::MissingRawValue {
                kind,
                object,
                data_point,
                row,
                band: Some(band),
            } => write!(
                f,
                "A-JOC object {object}, data point {data_point}, {kind:?} row {row} lacks raw band {band}"
            ),
            Self::MissingRawValue {
                kind,
                object,
                data_point,
                row,
                band: None,
            } => write!(
                f,
                "A-JOC object {object}, data point {data_point}, {kind:?} row {row} lacks a differential direction"
            ),
            Self::AbsoluteOutOfRange {
                kind,
                object,
                data_point,
                row,
                value,
                levels,
            } => write!(
                f,
                "Initial value {value} for A-JOC object {object}, data point {data_point}, {kind:?} row {row} is outside 0..{levels}"
            ),
            Self::IncompatibleHistory {
                kind,
                object,
                data_point,
                row,
                num_bands,
                coarse,
            } => write!(
                f,
                "A-JOC object {object}, data point {data_point}, {kind:?} row {row} has no compatible history (bands={num_bands}, coarse={coarse})"
            ),
            Self::TimeResultOutOfRange {
                kind,
                object,
                data_point,
                row,
                band,
                previous,
                delta,
                levels,
            } => write!(
                f,
                "Time differential {previous} + {delta} for A-JOC object {object}, data point {data_point}, {kind:?} row {row}, band {band} is outside 0..{levels}"
            ),
        }
    }
}

impl core::error::Error for DiffError {}

/// 把一帧 Huffman 原始值还原为完整量化矩阵。
///
/// inactive 对象不更新历史；sparse 未传输行写入量化中点并提交为下一数据点/帧
/// 的历史。入口先以状态副本演算并验证整帧全部索引、F0、`DIFF_TIME` 依赖和
/// 时间累加范围；验证成功后的提交阶段不会中途失败。任一返回错误都会保持
/// `states` 和 `output` 原样。
///
/// # Errors
///
/// 输入形状、绝对值或跨帧历史不合法时返回 [`DiffError`]。
pub fn decode(
    ajoc: &Ajoc,
    controls: &[AjocObjectControl],
    raw: &[AjocObjectMatrix],
    states: &mut [DiffState],
    output: &mut [QuantizedObjectMatrix],
) -> Result<(), DiffError> {
    let objects = validate_workspaces(ajoc, controls, raw, states, output)?;
    validate_frame(ajoc, controls, raw, states, objects)?;

    for object in 0..objects {
        let (Some(control), Some(raw_matrix), Some(state), Some(target)) = (
            controls.get(object),
            raw.get(object),
            states.get_mut(object),
            output.get_mut(object),
        ) else {
            continue;
        };
        *target = QuantizedObjectMatrix::new();
        target.num_dpoints = ajoc.data_points.count;
        target.num_dmx = ajoc.num_dmx_signals;
        target.num_decorr = ajoc.num_decorr;
        if !control.present || ajoc.data_points.count == 0 {
            continue;
        }
        target.num_bands = control.num_bands;
        process_object(ajoc, control, raw_matrix, state, target);
    }
    Ok(())
}

fn validate_workspaces(
    ajoc: &Ajoc,
    controls: &[AjocObjectControl],
    raw: &[AjocObjectMatrix],
    states: &[DiffState],
    output: &[QuantizedObjectMatrix],
) -> Result<usize, DiffError> {
    let objects =
        usize::try_from(ajoc.num_umx_signals).map_err(|_| DiffError::ObjectCountOutOfRange {
            objects: ajoc.num_umx_signals,
        })?;
    if controls.len() < objects
        || raw.len() < objects
        || states.len() < objects
        || output.len() < objects
    {
        return Err(DiffError::ObjectWorkspaceTooSmall {
            needed: objects,
            controls: controls.len(),
            raw: raw.len(),
            states: states.len(),
            output: output.len(),
        });
    }
    if usize::from(ajoc.data_points.count) > MAX_DATA_POINTS
        || usize::from(ajoc.num_dmx_signals) > MAX_AJOC_DMX_SIGNALS
        || usize::from(ajoc.num_decorr) > MAX_DECORRELATORS
    {
        return Err(DiffError::DimensionOutOfRange {
            num_dpoints: ajoc.data_points.count,
            num_dmx: ajoc.num_dmx_signals,
            num_decorr: ajoc.num_decorr,
        });
    }
    Ok(objects)
}

fn validate_frame(
    ajoc: &Ajoc,
    controls: &[AjocObjectControl],
    raw: &[AjocObjectMatrix],
    states: &[DiffState],
    objects: usize,
) -> Result<(), DiffError> {
    for object in 0..objects {
        let (Some(control), Some(raw_matrix), Some(state)) = (
            controls.get(object),
            raw.get(object),
            states.get(object).copied(),
        ) else {
            continue;
        };
        if !control.present || ajoc.data_points.count == 0 {
            continue;
        }
        if control.num_bands == 0 || usize::from(control.num_bands) > MAX_AJOC_BANDS {
            return Err(DiffError::BandCountOutOfRange {
                object,
                num_bands: control.num_bands,
            });
        }
        if raw_matrix.num_bands() != control.num_bands {
            return Err(DiffError::MissingRawValue {
                kind: MatrixKind::Dry,
                object,
                data_point: 0,
                row: 0,
                band: Some(usize::from(control.num_bands)),
            });
        }
        validate_object(ajoc, control, raw_matrix, state, object)?;
    }
    Ok(())
}

fn validate_object(
    ajoc: &Ajoc,
    control: &AjocObjectControl,
    raw: &AjocObjectMatrix,
    mut state: DiffState,
    object: usize,
) -> Result<(), DiffError> {
    for dp in 0..usize::from(ajoc.data_points.count) {
        for ch in 0..usize::from(ajoc.num_dmx_signals) {
            let Some(history) = state.dry.get_mut(ch) else {
                continue;
            };
            if !control.dry_present(ch) {
                commit_neutral(history, control.num_bands, control.coarse, MatrixKind::Dry);
                continue;
            }
            let direction = raw
                .dry_time_direction(dp, ch)
                .ok_or(DiffError::MissingRawValue {
                    kind: MatrixKind::Dry,
                    object,
                    data_point: dp,
                    row: ch,
                    band: None,
                })?;
            validate_row(RowValidation {
                kind: MatrixKind::Dry,
                object,
                data_point: dp,
                row: ch,
                num_bands: control.num_bands,
                coarse: control.coarse,
                time_direction: direction,
                raw: |band| raw.dry(dp, ch, band),
                history,
            })?;
        }
        for de in 0..usize::from(ajoc.num_decorr) {
            let Some(history) = state.wet.get_mut(de) else {
                continue;
            };
            if control.sparse && !control.wet_present(de) {
                commit_neutral(history, control.num_bands, control.coarse, MatrixKind::Wet);
                continue;
            }
            let direction = raw
                .wet_time_direction(dp, de)
                .ok_or(DiffError::MissingRawValue {
                    kind: MatrixKind::Wet,
                    object,
                    data_point: dp,
                    row: de,
                    band: None,
                })?;
            validate_row(RowValidation {
                kind: MatrixKind::Wet,
                object,
                data_point: dp,
                row: de,
                num_bands: control.num_bands,
                coarse: control.coarse,
                time_direction: direction,
                raw: |band| raw.wet(dp, de, band),
                history,
            })?;
        }
    }
    Ok(())
}

struct RowValidation<'a, F>
where
    F: Fn(usize) -> Option<i16>,
{
    kind: MatrixKind,
    object: usize,
    data_point: usize,
    row: usize,
    num_bands: u8,
    coarse: bool,
    time_direction: bool,
    raw: F,
    history: &'a mut HistoryRow,
}

fn validate_row<F>(context: RowValidation<'_, F>) -> Result<(), DiffError>
where
    F: Fn(usize) -> Option<i16>,
{
    let RowValidation {
        kind,
        object,
        data_point,
        row,
        num_bands,
        coarse,
        time_direction,
        raw,
        history,
    } = context;
    if time_direction && !history.compatible(num_bands, coarse) {
        return Err(DiffError::IncompatibleHistory {
            kind,
            object,
            data_point,
            row,
            num_bands,
            coarse,
        });
    }
    let nquant = quantized_levels(kind, coarse);
    let mut decoded = [0; MAX_AJOC_BANDS];
    for band in 0..usize::from(num_bands) {
        let delta = raw(band).ok_or(DiffError::MissingRawValue {
            kind,
            object,
            data_point,
            row,
            band: Some(band),
        })?;
        let value = if time_direction {
            let previous = history.values.get(band).copied().unwrap_or(0);
            add_time(previous, delta, nquant).ok_or(DiffError::TimeResultOutOfRange {
                kind,
                object,
                data_point,
                row,
                band,
                previous,
                delta,
                levels: nquant,
            })?
        } else if band == 0 {
            if !(0..nquant).contains(&delta) {
                return Err(DiffError::AbsoluteOutOfRange {
                    kind,
                    object,
                    data_point,
                    row,
                    value: delta,
                    levels: nquant,
                });
            }
            delta
        } else {
            let previous = decoded.get(band.saturating_sub(1)).copied().unwrap_or(0);
            add_mod(previous, delta, nquant)
        };
        if let Some(slot) = decoded.get_mut(band) {
            *slot = value;
        }
    }
    history.commit(&decoded, num_bands, coarse);
    Ok(())
}

fn process_object(
    ajoc: &Ajoc,
    control: &AjocObjectControl,
    raw: &AjocObjectMatrix,
    state: &mut DiffState,
    output: &mut QuantizedObjectMatrix,
) {
    for dp in 0..usize::from(ajoc.data_points.count) {
        for ch in 0..usize::from(ajoc.num_dmx_signals) {
            let (Some(history), Some(target)) = (
                state.dry.get_mut(ch),
                output.dry.get_mut(dp).and_then(|plane| plane.get_mut(ch)),
            ) else {
                continue;
            };
            if !control.dry_present(ch) {
                fill_neutral(
                    target,
                    control.num_bands,
                    quantized_levels(MatrixKind::Dry, control.coarse),
                );
                history.commit(target, control.num_bands, control.coarse);
                continue;
            }
            decode_row(
                control.num_bands,
                control.coarse,
                raw.dry_time_direction(dp, ch).unwrap_or(false),
                |band| raw.dry(dp, ch, band).unwrap_or(0),
                history,
                target,
                MatrixKind::Dry,
            );
        }
        for de in 0..usize::from(ajoc.num_decorr) {
            let (Some(history), Some(target)) = (
                state.wet.get_mut(de),
                output.wet.get_mut(dp).and_then(|plane| plane.get_mut(de)),
            ) else {
                continue;
            };
            if control.sparse && !control.wet_present(de) {
                fill_neutral(
                    target,
                    control.num_bands,
                    quantized_levels(MatrixKind::Wet, control.coarse),
                );
                history.commit(target, control.num_bands, control.coarse);
                continue;
            }
            decode_row(
                control.num_bands,
                control.coarse,
                raw.wet_time_direction(dp, de).unwrap_or(false),
                |band| raw.wet(dp, de, band).unwrap_or(0),
                history,
                target,
                MatrixKind::Wet,
            );
        }
    }
}

fn decode_row<F>(
    num_bands: u8,
    coarse: bool,
    time_direction: bool,
    raw: F,
    history: &mut HistoryRow,
    output: &mut [i16; MAX_AJOC_BANDS],
    kind: MatrixKind,
) where
    F: Fn(usize) -> i16,
{
    let nquant = quantized_levels(kind, coarse);
    for band in 0..usize::from(num_bands) {
        let delta = raw(band);
        let value = if time_direction {
            let previous = history.values.get(band).copied().unwrap_or(0);
            add_time(previous, delta, nquant).unwrap_or(0)
        } else if band == 0 {
            delta
        } else {
            let previous = output.get(band.saturating_sub(1)).copied().unwrap_or(0);
            add_mod(previous, delta, nquant)
        };
        if let Some(slot) = output.get_mut(band) {
            *slot = value;
        }
    }
    history.commit(output, num_bands, coarse);
}

fn fill_neutral(output: &mut [i16; MAX_AJOC_BANDS], num_bands: u8, nquant: i16) {
    let neutral = nquant.saturating_sub(1) / 2;
    for value in output.iter_mut().take(usize::from(num_bands)) {
        *value = neutral;
    }
}

fn commit_neutral(history: &mut HistoryRow, num_bands: u8, coarse: bool, kind: MatrixKind) {
    let mut values = [0; MAX_AJOC_BANDS];
    fill_neutral(&mut values, num_bands, quantized_levels(kind, coarse));
    history.commit(&values, num_bands, coarse);
}

fn add_time(previous: i16, delta: i16, nquant: i16) -> Option<i16> {
    let sum = i32::from(previous).saturating_add(i32::from(delta));
    if !(0..i32::from(nquant)).contains(&sum) {
        return None;
    }
    i16::try_from(sum).ok()
}

fn add_mod(base: i16, delta: i16, modulus: i16) -> i16 {
    // 两个 i16 的和严格落在 i32 范围；saturating_add 在此与普通加法逐位相同，
    // 同时满足 crate 的“算术副作用必须显式”lint。
    let sum = i32::from(base).saturating_add(i32::from(delta));
    i16::try_from(sum.rem_euclid(i32::from(modulus))).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(
        present: bool,
        bands: u8,
        coarse: bool,
        sparse: bool,
        dry: &[bool],
        wet: &[bool],
    ) -> AjocObjectControl {
        AjocObjectControl::for_test(present, bands, coarse, sparse, dry, wet)
    }

    fn frame(bands: u8, dpoints: u8, dmx: u8, decorr: u8) -> AjocObjectMatrix {
        AjocObjectMatrix::for_test(bands, dpoints, dmx, decorr)
    }

    #[test]
    fn frequency_direction_wraps_every_later_band() {
        let ajoc = Ajoc::for_test(1, 1, 1, 1);
        let controls = [control(true, 3, false, false, &[], &[])];
        let mut raw = frame(3, 1, 1, 1);
        raw.set_dry_for_test(0, 0, &[100, 2, -3], false);
        raw.set_wet_for_test(0, 0, &[40, 2, -3], false);
        let mut states = [DiffState::new()];
        let mut output = [QuantizedObjectMatrix::new()];

        decode(&ajoc, &controls, &[raw], &mut states, &mut output).expect("频率差分应成功");

        assert_eq!(
            (
                output[0].dry(0, 0, 0),
                output[0].dry(0, 0, 1),
                output[0].dry(0, 0, 2)
            ),
            (Some(100), Some(1), Some(99))
        );
        assert_eq!(
            (
                output[0].wet(0, 0, 0),
                output[0].wet(0, 0, 1),
                output[0].wet(0, 0, 2)
            ),
            (Some(40), Some(1), Some(39))
        );
    }

    #[test]
    fn time_direction_chains_within_and_across_frames() {
        let first_ajoc = Ajoc::for_test(1, 1, 1, 1);
        let controls = [control(true, 2, false, false, &[], &[])];
        let mut first = frame(2, 1, 1, 1);
        first.set_dry_for_test(0, 0, &[50, 0], false);
        first.set_wet_for_test(0, 0, &[20, 0], false);
        let mut states = [DiffState::new()];
        let mut output = [QuantizedObjectMatrix::new()];
        decode(&first_ajoc, &controls, &[first], &mut states, &mut output).expect("首帧应成功");

        let second_ajoc = Ajoc::for_test(1, 2, 1, 1);
        let mut second = frame(2, 2, 1, 1);
        second.set_dry_for_test(0, 0, &[40, -40], true);
        second.set_dry_for_test(1, 0, &[-10, 2], true);
        second.set_wet_for_test(0, 0, &[10, -10], true);
        second.set_wet_for_test(1, 0, &[-10, 2], true);
        decode(&second_ajoc, &controls, &[second], &mut states, &mut output)
            .expect("跨帧时间差分应成功");

        assert_eq!(
            (output[0].dry(0, 0, 0), output[0].dry(0, 0, 1)),
            (Some(90), Some(10))
        );
        assert_eq!(
            (output[0].dry(1, 0, 0), output[0].dry(1, 0, 1)),
            (Some(80), Some(12))
        );
        assert_eq!(
            (output[0].wet(0, 0, 0), output[0].wet(0, 0, 1)),
            (Some(30), Some(10))
        );
        assert_eq!(
            (output[0].wet(1, 0, 0), output[0].wet(1, 0, 1)),
            (Some(20), Some(12))
        );
    }

    #[test]
    fn rejects_time_results_out_of_range_transactionally() {
        let one = Ajoc::for_test(1, 1, 1, 1);
        let controls = [control(true, 2, false, false, &[], &[])];
        let mut absolute = frame(2, 1, 1, 1);
        absolute.set_dry_for_test(0, 0, &[1, 99], false);
        absolute.set_wet_for_test(0, 0, &[0, 0], false);
        let mut states = [DiffState::new()];
        let mut output = [QuantizedObjectMatrix::new()];
        decode(&one, &controls, &[absolute], &mut states, &mut output).expect("首帧应成功");
        assert_eq!(output[0].dry(0, 0, 0), Some(1));
        assert_eq!(output[0].dry(0, 0, 1), Some(100));

        let before_states = states;
        let before_output = output.clone();
        let two = Ajoc::for_test(1, 2, 1, 1);
        let mut lower = frame(2, 2, 1, 1);
        lower.set_dry_for_test(0, 0, &[-1, 0], true);
        lower.set_dry_for_test(1, 0, &[-1, 0], true);
        lower.set_wet_for_test(0, 0, &[0, 0], true);
        lower.set_wet_for_test(1, 0, &[0, 0], true);
        assert_eq!(
            decode(&two, &controls, &[lower], &mut states, &mut output),
            Err(DiffError::TimeResultOutOfRange {
                kind: MatrixKind::Dry,
                object: 0,
                data_point: 1,
                row: 0,
                band: 0,
                previous: 0,
                delta: -1,
                levels: 101,
            })
        );
        assert_eq!(states, before_states);
        assert_eq!(output, before_output);

        let mut upper = frame(2, 1, 1, 1);
        upper.set_dry_for_test(0, 0, &[0, 1], true);
        upper.set_wet_for_test(0, 0, &[0, 0], true);
        assert_eq!(
            decode(&one, &controls, &[upper], &mut states, &mut output),
            Err(DiffError::TimeResultOutOfRange {
                kind: MatrixKind::Dry,
                object: 0,
                data_point: 0,
                row: 0,
                band: 1,
                previous: 100,
                delta: 1,
                levels: 101,
            })
        );
        assert_eq!(states, before_states);
        assert_eq!(output, before_output);
    }

    #[test]
    fn sparse_omissions_commit_the_neutral_history() {
        let ajoc = Ajoc::for_test(1, 1, 1, 1);
        let sparse = [control(true, 3, false, true, &[false], &[false])];
        let raw = frame(3, 1, 1, 1);
        let mut states = [DiffState::new()];
        let mut output = [QuantizedObjectMatrix::new()];
        decode(&ajoc, &sparse, &[raw], &mut states, &mut output).expect("稀疏中点应成功");
        for band in 0..3 {
            assert_eq!(output[0].dry(0, 0, band), Some(50));
            assert_eq!(output[0].wet(0, 0, band), Some(20));
        }

        let dense = [control(true, 3, false, false, &[], &[])];
        let mut delta = frame(3, 1, 1, 1);
        delta.set_dry_for_test(0, 0, &[1, 1, 1], true);
        delta.set_wet_for_test(0, 0, &[1, 1, 1], true);
        decode(&ajoc, &dense, &[delta], &mut states, &mut output).expect("中点应成为时间历史");
        for band in 0..3 {
            assert_eq!(output[0].dry(0, 0, band), Some(51));
            assert_eq!(output[0].wet(0, 0, band), Some(21));
        }
    }

    #[test]
    fn frequency_data_can_establish_a_new_grid_before_time_data() {
        let old = Ajoc::for_test(0, 1, 1, 1);
        let old_control = [control(true, 1, false, false, &[], &[])];
        let mut old_raw = frame(1, 1, 1, 0);
        old_raw.set_dry_for_test(0, 0, &[10], false);
        let mut states = [DiffState::new()];
        let mut output = [QuantizedObjectMatrix::new()];
        decode(&old, &old_control, &[old_raw], &mut states, &mut output).expect("旧网格应建立历史");

        let changed = Ajoc::for_test(0, 2, 1, 1);
        let new_control = [control(true, 3, true, false, &[], &[])];
        let mut new_raw = frame(3, 2, 1, 0);
        new_raw.set_dry_for_test(0, 0, &[25, 1, 1], false);
        new_raw.set_dry_for_test(1, 0, &[1, 1, 1], true);
        decode(&changed, &new_control, &[new_raw], &mut states, &mut output)
            .expect("同帧 DF 应先建立新形状");
        assert_eq!(
            (
                output[0].dry(1, 0, 0),
                output[0].dry(1, 0, 1),
                output[0].dry(1, 0, 2)
            ),
            (Some(26), Some(27), Some(28))
        );
    }

    #[test]
    fn inactive_and_zero_data_point_frames_preserve_history() {
        let controls = [control(true, 1, false, false, &[], &[])];
        let one = Ajoc::for_test(1, 1, 1, 1);
        let mut absolute = frame(1, 1, 1, 1);
        absolute.set_dry_for_test(0, 0, &[20], false);
        absolute.set_wet_for_test(0, 0, &[10], false);
        let mut states = [DiffState::new()];
        let mut output = [QuantizedObjectMatrix::new()];
        decode(&one, &controls, &[absolute], &mut states, &mut output).expect("首帧应成功");

        let inactive = [control(false, 0, false, false, &[], &[])];
        decode(
            &one,
            &inactive,
            &[frame(0, 1, 1, 1)],
            &mut states,
            &mut output,
        )
        .expect("inactive 帧应成功");
        assert_eq!(output[0].num_bands(), 0);
        let zero = Ajoc::for_test(1, 0, 1, 1);
        decode(
            &zero,
            &controls,
            &[frame(0, 0, 1, 1)],
            &mut states,
            &mut output,
        )
        .expect("零数据点应成功");

        let mut time = frame(1, 1, 1, 1);
        time.set_dry_for_test(0, 0, &[1], true);
        time.set_wet_for_test(0, 0, &[1], true);
        decode(&one, &controls, &[time], &mut states, &mut output).expect("两种空帧都不应清历史");
        assert_eq!(output[0].dry(0, 0, 0), Some(21));
        assert_eq!(output[0].wet(0, 0, 0), Some(11));
    }

    #[test]
    fn incompatible_time_history_is_transactional() {
        let ajoc = Ajoc::for_test(1, 1, 1, 1);
        let fine = [control(true, 2, false, false, &[], &[])];
        let mut absolute = frame(2, 1, 1, 1);
        absolute.set_dry_for_test(0, 0, &[20, 0], false);
        absolute.set_wet_for_test(0, 0, &[10, 0], false);
        let mut states = [DiffState::new()];
        let mut output = [QuantizedObjectMatrix::new()];
        decode(&ajoc, &fine, &[absolute], &mut states, &mut output).expect("首帧应成功");

        let before_states = states;
        let before_output = output.clone();
        let coarse = [control(true, 2, true, false, &[], &[])];
        let mut incompatible = frame(2, 1, 1, 1);
        incompatible.set_dry_for_test(0, 0, &[0, 0], false);
        incompatible.set_wet_for_test(0, 0, &[1, 1], true);
        assert!(matches!(
            decode(&ajoc, &coarse, &[incompatible], &mut states, &mut output),
            Err(DiffError::IncompatibleHistory {
                kind: MatrixKind::Wet,
                ..
            })
        ));
        assert_eq!(states, before_states);
        assert_eq!(output, before_output);

        let wider = [control(true, 3, false, false, &[], &[])];
        let mut changed_grid = frame(3, 1, 1, 1);
        changed_grid.set_dry_for_test(0, 0, &[1, 1, 1], true);
        changed_grid.set_wet_for_test(0, 0, &[1, 1, 1], true);
        assert!(matches!(
            decode(&ajoc, &wider, &[changed_grid], &mut states, &mut output),
            Err(DiffError::IncompatibleHistory {
                kind: MatrixKind::Dry,
                num_bands: 3,
                ..
            })
        ));
        assert_eq!(states, before_states);
        assert_eq!(output, before_output);
    }

    #[test]
    fn rejects_bad_absolute_and_short_workspaces_without_committing() {
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [control(true, 1, true, false, &[], &[])];
        let mut raw = frame(1, 1, 1, 0);
        raw.set_dry_for_test(0, 0, &[51], false);
        let mut states = [DiffState::new()];
        let mut output = [QuantizedObjectMatrix::new()];
        assert_eq!(
            decode(&ajoc, &controls, &[raw], &mut states, &mut output),
            Err(DiffError::AbsoluteOutOfRange {
                kind: MatrixKind::Dry,
                object: 0,
                data_point: 0,
                row: 0,
                value: 51,
                levels: 51,
            })
        );
        assert_eq!(states, [DiffState::new()]);
        assert_eq!(output, [QuantizedObjectMatrix::new()]);

        assert!(matches!(
            decode(&ajoc, &[], &[], &mut [], &mut []),
            Err(DiffError::ObjectWorkspaceTooSmall { needed: 1, .. })
        ));
    }

    #[test]
    fn reset_discards_time_history() {
        let ajoc = Ajoc::for_test(0, 1, 1, 1);
        let controls = [control(true, 1, false, false, &[], &[])];
        let mut absolute = frame(1, 1, 1, 0);
        absolute.set_dry_for_test(0, 0, &[50], false);
        let mut states = [DiffState::new()];
        let mut output = [QuantizedObjectMatrix::new()];
        decode(&ajoc, &controls, &[absolute], &mut states, &mut output).expect("首帧应成功");
        states[0].reset();

        let mut time = frame(1, 1, 1, 0);
        time.set_dry_for_test(0, 0, &[1], true);
        assert!(matches!(
            decode(&ajoc, &controls, &[time], &mut states, &mut output),
            Err(DiffError::IncompatibleHistory { .. })
        ));
    }
}
