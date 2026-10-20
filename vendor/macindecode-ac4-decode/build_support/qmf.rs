//! QMF 调制表与规范 QWIN 原型窗。

use super::math::negate_f32_bits;
use super::sha256::sha256_hex;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

const QMF_MODULATION_SHA256: &str =
    "4e7f81b7353b5e1915d249615dbd2746ac7db1cb830f9d6abb2f6c451d820d25";
const QMF_WINDOW_SHA256: &str = "870d3c80e4fe7ee368b5da1f7db487dfa84be44c4cf8c82813b78fb06a8e94a2";

/// `5.7.3`/`5.7.4` 的 QMF 调制相位 `exp(+j 2πk/512)`，`k = 0…511`。
///
/// 两条伪码的指数都是 `j·(π/128)·(sb+0.5)·m`，其中 `m` 为奇数（分析 `2n−1`、
/// 合成 `2n−255`），化简为 `2π·(2sb+1)·m / 512`。`(2sb+1)·m` 恒为奇数，所以
/// 512 圈上只用得到奇数下标——但表按整圈存：省下的一半只有 2 KiB，而按奇偶
/// 折半索引会多出一层容易写反的换算。
///
/// 生成规则与 IFFT 根表相同（ADR-0003）：只在第一象限调用锁定版本的 `libm`，
/// 轴点精确写入，其余三象限由对称派生，使数学上的零不会变成有限精度 π 的残差。
pub(crate) fn emit_qmf_modulation() {
    const POINTS: usize = 512;
    const QUARTER: usize = POINTS / 4;
    const HALF: usize = POINTS / 2;

    let mut roots = vec![[0u32; 2]; POINTS];
    roots[0] = [1.0f32.to_bits(), 0.0f32.to_bits()];
    roots[QUARTER] = [0.0f32.to_bits(), 1.0f32.to_bits()];
    roots[HALF] = [(-1.0f32).to_bits(), 0.0f32.to_bits()];
    for offset in 1..QUARTER {
        let angle = core::f64::consts::TAU * offset as f64 / POINTS as f64;
        let cosine = (libm::cos(angle) as f32).to_bits();
        let sine = (libm::sin(angle) as f32).to_bits();
        roots[offset] = [cosine, sine];
        roots[QUARTER + offset] = [negate_f32_bits(sine), cosine];
    }
    for exponent in 1..=HALF {
        let first_half = roots[exponent];
        roots[POINTS - exponent] = [first_half[0], negate_f32_bits(first_half[1])];
    }

    // 与 IFFT 根表同一套结构判据：轴点精确、单位圆、共轭对称。
    for (index, [real, imaginary]) in roots.iter().enumerate() {
        let (re, im) = (f32::from_bits(*real), f32::from_bits(*imaginary));
        assert!(re.is_finite() && im.is_finite(), "第 {index} 项非有限");
        let magnitude = f64::from(re) * f64::from(re) + f64::from(im) * f64::from(im);
        assert!(
            (magnitude - 1.0).abs() <= 1.0e-6,
            "第 {index} 项模平方 {magnitude} 偏离单位圆"
        );
    }
    for exponent in 1..HALF {
        let [re, im] = roots[exponent];
        let [cre, cim] = roots[POINTS - exponent];
        assert_eq!(re, cre, "第 {exponent} 项与共轭项实部应逐位相同");
        assert_eq!(im, negate_f32_bits(cim), "第 {exponent} 项共轭关系不成立");
    }

    let mut blob = Vec::with_capacity(POINTS * 8);
    for [real, imaginary] in &roots {
        blob.extend_from_slice(&real.to_le_bytes());
        blob.extend_from_slice(&imaginary.to_le_bytes());
    }
    let digest = sha256_hex(&blob);
    assert_eq!(
        digest, QMF_MODULATION_SHA256,
        "QMF 调制表摘要与冻结值不符；生成规则、libm 版本或舍入发生变化"
    );

    let mut generated = String::new();
    generated.push_str("/// `exp(+j 2πk/512)` 的 f32 表，由构建脚本生成并核对摘要。\n");
    let _ = writeln!(
        generated,
        "pub(crate) static QMF_MODULATION: [[f32; 2]; {POINTS}] = ["
    );
    for [real, imaginary] in &roots {
        let _ = writeln!(
            generated,
            "    [f32::from_bits(0x{real:08X}), f32::from_bits(0x{imaginary:08X})],"
        );
    }
    generated.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("qmf_modulation.rs");
    std::fs::write(&out, generated)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", out.display()));
}

/// `5.7.3`/`5.7.4` 的 QMF 原型窗，取自表 D.3（随附 C 表的 `QWIN[640]`）。
///
/// 构建期核对两条**与摘要无关**的结构判据。摘要只能保证「和上次一样」，这两条
/// 才说明这 640 个数确实是一张能重建的原型窗：
///
/// - **镜像**：`|QWIN[n]| == |QWIN[640−n]|`，且仅在 `n` 为 128 的倍数时反号。
/// - **多相功率互补**：每个相位 `p` 满足 `Σ_k QWIN[64k+p]² == 1`。任一系数抄错
///   都会让它所在相位的和偏离 1，而这一条不需要任何外部参照。
pub(crate) fn emit_qmf_window(floats: &BTreeMap<String, Vec<f32>>) {
    const LEN: usize = 640;
    const SUBBANDS: usize = 64;
    const PHASE_TAPS: usize = LEN / SUBBANDS;

    let window = floats
        .get("QWIN")
        .unwrap_or_else(|| panic!("规范随附 C 表中没有 QWIN；请重新运行 scripts/fetch_specs.py"));
    assert_eq!(window.len(), LEN, "QWIN 应有 {LEN} 项");
    assert_eq!(window[0], 0.0, "QWIN[0] 应为 0，镜像判据以此为轴");

    for n in 1..LEN {
        let mirrored = window[LEN - n];
        let flipped = n % 128 == 0;
        let expected = if flipped { -mirrored } else { mirrored };
        assert_eq!(
            window[n],
            expected,
            "QWIN[{n}] 与 QWIN[{}] 的镜像关系不成立（应{}反号）",
            LEN - n,
            if flipped { "" } else { "不" }
        );
    }

    let mut worst = 0.0f64;
    for phase in 0..SUBBANDS {
        let sum: f64 = (0..PHASE_TAPS)
            .map(|k| {
                let value = f64::from(window[SUBBANDS * k + phase]);
                value * value
            })
            .sum();
        worst = worst.max((sum - 1.0).abs());
    }
    assert!(
        worst <= 1.0e-6,
        "QMF 原型窗的多相功率互补最大偏差 {worst:e} 超出预算"
    );

    let mut blob = Vec::with_capacity(LEN * 4);
    for value in window {
        blob.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    let digest = sha256_hex(&blob);
    assert_eq!(
        digest, QMF_WINDOW_SHA256,
        "QMF 原型窗摘要与冻结值不符：解析或 f32 化发生了变化"
    );

    let mut out = String::with_capacity(LEN * 24);
    out.push_str("/// `5.7.3` 表 D.3 的 QMF 原型窗，由构建脚本从规范随附 C 表生成。\n");
    out.push_str("pub(crate) static QMF_WINDOW: [f32; 640] = [\n");
    for value in window {
        let _ = writeln!(out, "    f32::from_bits({:#010x}),", value.to_bits());
    }
    out.push_str("];\n");

    let path = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("qmf_window.rs");
    std::fs::write(&path, out)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", path.display()));
}
