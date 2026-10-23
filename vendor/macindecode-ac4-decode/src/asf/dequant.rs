//! `5.1.3.2` 的反量化表。
//!
//! 表由构建脚本以 ADR-0002 规定的整数判据生成——不调用宿主 `powf`，因为让
//! 构建机的 libm 参与表值选择会把它拖进可复现性边界。整个位表的 SHA-256 在
//! 构建期核对，生成规则与摘要都记在 ADR-0002 内。
//!
//! 表是纯数学量，与 ETSI 附件无关，因此不受 `audio-decode` feature 约束。

include!(concat!(env!("OUT_DIR"), "/dequant_table.rs"));

/// `rec_spec = sign(q) × |q|^(4/3)`，见 `5.1.3.2`。
///
/// `5.1.3.1` NOTE 1 把 `|quant_spec|` 的上限定在 8 191，解析侧已通过
/// `AsfSpectrumError::QuantMagnitudeOutOfRange` 拒绝越界值；这里再夹一次只为
/// 让本函数对任意输入都有定义。
#[must_use]
pub fn reconstruct_line(quant: i16) -> f32 {
    let magnitude = usize::from(quant.unsigned_abs());
    let value = REC_SPEC
        .get(magnitude)
        .or_else(|| REC_SPEC.last())
        .copied()
        .unwrap_or(0.0);
    if quant < 0 { -value } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表的端点与符号约定。
    #[test]
    fn endpoints_follow_the_specification() {
        assert_eq!(reconstruct_line(0), 0.0);
        assert_eq!(reconstruct_line(1), 1.0);
        assert_eq!(reconstruct_line(-1), -1.0);
        assert_eq!(reconstruct_line(8), 16.0, "8^(4/3) = 16 应精确");
        assert_eq!(reconstruct_line(27), 81.0, "27^(4/3) = 81 应精确");
        assert_eq!(reconstruct_line(-27), -81.0);
    }

    /// 立方数的 `x^(4/3)` 是整数，可作为不依赖浮点函数的独立判据。
    ///
    /// `q = k³` 时 `q^(4/3) = k⁴`，只要 `k⁴` 在 f32 的精确整数范围内，表值就
    /// 必须**恰好**等于它。这一条完全绕开了生成器自身的判据。
    #[test]
    fn perfect_cubes_are_exact_integers() {
        let mut checked = 0;
        for k in 1u32..=20 {
            let cube = k.saturating_mul(k).saturating_mul(k);
            if cube > 8191 {
                break;
            }
            let expected = k.saturating_mul(k).saturating_mul(k).saturating_mul(k);
            let got = reconstruct_line(i16::try_from(cube).expect("立方数在 i16 内"));
            assert_eq!(got, expected as f32, "{cube}^(4/3) 应恰好是 {expected}");
            checked += 1;
        }
        assert_eq!(checked, 20, "1³…20³ 都不超过 8 191");
    }

    /// 表严格递增，且覆盖规范声明的全部幅度。
    #[test]
    fn the_table_spans_the_declared_magnitude_range() {
        assert_eq!(REC_SPEC.len(), 8192);
        for pair in REC_SPEC.windows(2) {
            let (Some(low), Some(high)) = (pair.first(), pair.last()) else {
                panic!("窗口应有两项");
            };
            assert!(low < high, "{low:?} 应小于 {high:?}");
        }
        assert!(reconstruct_line(8191).is_finite());
    }
}
