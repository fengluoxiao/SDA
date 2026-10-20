//! A-JOC dry/wet 矩阵参数的反量化。
//!
//! `TS103190-2:v1.3.1:5.7.3.3` 的表 29–32 全部是以零为中心的均匀量化表。
//! PDF 只排印十进制近似值；这里用分母为 `2^11` 的精确有理数生成，避免把不同
//! 位数的舍入文本当成常量抄进实现。

use super::MatrixKind;
use crate::spec_tables::ajoc::QUANTIZER_ROWS;
use core::fmt;

const DENOMINATOR: i32 = 2_048;

#[derive(Debug, Clone, Copy)]
struct Quantizer {
    levels: i16,
    midpoint: i16,
    step_numerator: i16,
}

#[expect(
    clippy::indexing_slicing,
    reason = "四种枚举组合映射到生成表的四个固定行；生成器同时锁定行数"
)]
const fn quantizer(kind: MatrixKind, coarse: bool) -> Quantizer {
    let row = match (kind, coarse) {
        (MatrixKind::Dry, true) => 0,
        (MatrixKind::Dry, false) => 1,
        (MatrixKind::Wet, true) => 2,
        (MatrixKind::Wet, false) => 3,
    };
    let (levels, midpoint, step_numerator) = QUANTIZER_ROWS[row];
    Quantizer {
        levels,
        midpoint,
        step_numerator,
    }
}

/// 所选量化表的行数，即 `Pseudocode 16` 的 `nquant`。
#[must_use]
pub const fn quantized_levels(kind: MatrixKind, coarse: bool) -> i16 {
    quantizer(kind, coarse).levels
}

/// A-JOC 反量化失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DequantError {
    /// 量化值不在对应表的行号范围内。
    QuantizedOutOfRange {
        /// dry 或 wet。
        kind: MatrixKind,
        /// `true` 为 coarse，`false` 为 fine。
        coarse: bool,
        /// 调用方给出的值。
        value: i16,
        /// 合法值的半开上界。
        levels: i16,
    },
    /// 已验证范围内的整数公式仍发生溢出，表示内置量化器常量损坏。
    ArithmeticOverflow {
        /// dry 或 wet。
        kind: MatrixKind,
        /// `true` 为 coarse，`false` 为 fine。
        coarse: bool,
        /// 正在换算的量化值。
        value: i16,
    },
}

impl fmt::Display for DequantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QuantizedOutOfRange {
                kind,
                coarse,
                value,
                levels,
            } => write!(
                f,
                "A-JOC {kind:?} {} quantized value {value} is outside 0..{levels}",
                if *coarse { "coarse" } else { "fine" }
            ),
            Self::ArithmeticOverflow {
                kind,
                coarse,
                value,
            } => write!(
                f,
                "Integer dequantization formula overflow for A-JOC {kind:?} {} quantized value {value}",
                if *coarse { "coarse" } else { "fine" }
            ),
        }
    }
}

impl core::error::Error for DequantError {}

/// 按表 29–32 反量化一个 A-JOC 矩阵参数。
///
/// 先在整数域计算分子再除以 `2^11`：最大分子只有 10 250，可由 `f32` 精确
/// 表示，除以二的幂也不引入 PDF 小数排印的舍入误差。
///
/// # Errors
///
/// `quantized` 不是所选表的合法行号时返回 [`DequantError`]。
pub fn dequantise(kind: MatrixKind, coarse: bool, quantized: i16) -> Result<f32, DequantError> {
    let quantizer = quantizer(kind, coarse);
    if !(0..quantizer.levels).contains(&quantized) {
        return Err(DequantError::QuantizedOutOfRange {
            kind,
            coarse,
            value: quantized,
            levels: quantizer.levels,
        });
    }

    let numerator = i32::from(quantized)
        .checked_sub(i32::from(quantizer.midpoint))
        .and_then(|centered| centered.checked_mul(i32::from(quantizer.step_numerator)))
        .ok_or(DequantError::ArithmeticOverflow {
            kind,
            coarse,
            value: quantized,
        })?;
    Ok(numerator as f32 / DENOMINATOR as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: [(MatrixKind, bool); 4] = [
        (MatrixKind::Dry, true),
        (MatrixKind::Dry, false),
        (MatrixKind::Wet, true),
        (MatrixKind::Wet, false),
    ];

    #[test]
    fn every_table_entry_uses_the_exact_binary_rational() {
        for (kind, coarse) in CASES {
            let table = quantizer(kind, coarse);
            for q in 0..table.levels {
                let expected = (i32::from(q - table.midpoint) * i32::from(table.step_numerator))
                    as f32
                    / DENOMINATOR as f32;
                assert_eq!(
                    dequantise(kind, coarse, q).expect("表内值应合法").to_bits(),
                    expected.to_bits(),
                    "{kind:?} coarse={coarse} q={q}"
                );
            }
        }
    }

    #[test]
    fn endpoints_are_symmetric_around_zero() {
        for (kind, coarse) in CASES {
            let table = quantizer(kind, coarse);
            assert_eq!(dequantise(kind, coarse, table.midpoint), Ok(0.0));
            let low = dequantise(kind, coarse, 0).expect("下端点合法");
            let high = dequantise(kind, coarse, table.levels - 1).expect("上端点合法");
            assert_eq!(low.to_bits(), (-high).to_bits());
        }
    }

    #[test]
    fn rejects_values_on_both_sides_of_every_table() {
        for (kind, coarse) in CASES {
            let levels = quantizer(kind, coarse).levels;
            for value in [-1, levels] {
                assert_eq!(
                    dequantise(kind, coarse, value),
                    Err(DequantError::QuantizedOutOfRange {
                        kind,
                        coarse,
                        value,
                        levels,
                    })
                );
            }
        }
    }
}
