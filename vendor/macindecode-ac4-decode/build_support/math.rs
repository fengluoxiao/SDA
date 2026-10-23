//! 反量化、IFFT、IMDCT 与 KBD 数学表。

use super::sha256::sha256_hex;
use std::fmt::Write as _;
use std::path::PathBuf;

const IFFT_ROOTS_SHA256: &str = "76261b2212836ee22fbf162ea6a2bcc35233bdd54dec8fa60cb15b51a452ef6b";
const IMDCT_ROTATION_SHA256: &str =
    "cd702daceaf47a407381e8238ab43c310e3f4c8139a17d84b66ccd4481da524e";
const KBD_LEFT_SHA256: &str = "6e57e7e89e52b5b57b27ab81b5472aeabacb1a88d00ddb76fdc313835fc0c895";

/// `Pseudocode 61` 的 IFFT 根 `exp(+j 2πe/M)`，`M = N/2`。
///
/// ADR-0004 选择完整根表：十五档 `M` 各存 `e = 0…M−1` 的实部、虚部。只在
/// 第一象限调用锁定版本的 `libm`；轴点精确写入，其余半圆由共轭派生，因此
/// 数学上的零不会变成有限精度 π 带来的微小残差。
pub(crate) fn emit_ifft_roots(transform_lengths: &[usize]) {
    const EXPECTED_COMPLEX_ROOTS: usize = 5_332;

    let mut offsets = Vec::with_capacity(transform_lengths.len() + 1);
    let mut all_roots = Vec::with_capacity(EXPECTED_COMPLEX_ROOTS);
    offsets.push(0u16);

    for &transform_length in transform_lengths {
        let length = transform_length / 2;
        assert_eq!(length % 4, 0, "M={length} 必须可整分四个象限");
        let quarter = length / 4;
        let half = length / 2;
        let mut roots = vec![[0u32; 2]; length];

        roots[0] = [1.0f32.to_bits(), 0.0f32.to_bits()];
        roots[quarter] = [0.0f32.to_bits(), 1.0f32.to_bits()];
        roots[half] = [(-1.0f32).to_bits(), 0.0f32.to_bits()];

        for offset in 1..quarter {
            let angle = core::f64::consts::TAU * offset as f64 / length as f64;
            let cosine = (libm::cos(angle) as f32).to_bits();
            let sine = (libm::sin(angle) as f32).to_bits();
            roots[offset] = [cosine, sine];
            roots[quarter + offset] = [negate_f32_bits(sine), cosine];
        }

        // 后半圆从前半圆逐位派生，保证共轭关系精确成立。
        for exponent in 1..=half {
            let first_half = roots[exponent];
            roots[length - exponent] = [first_half[0], negate_f32_bits(first_half[1])];
        }

        check_ifft_root_structure(length, &roots);
        all_roots.extend_from_slice(&roots);
        offsets.push(
            u16::try_from(all_roots.len())
                .unwrap_or_else(|_| panic!("IFFT 根偏移 {} 超出 u16", all_roots.len())),
        );
    }

    assert_eq!(offsets.len(), transform_lengths.len() + 1);
    assert_eq!(all_roots.len(), EXPECTED_COMPLEX_ROOTS);
    let mut blob = Vec::with_capacity(all_roots.len() * 8);
    for [real, imaginary] in &all_roots {
        blob.extend_from_slice(&real.to_le_bytes());
        blob.extend_from_slice(&imaginary.to_le_bytes());
    }
    let digest = sha256_hex(&blob);
    assert_eq!(
        digest, IFFT_ROOTS_SHA256,
        "IFFT 根表摘要与 ADR-0003 记录不符；生成规则、libm 版本或舍入发生变化"
    );

    let mut generated = String::new();
    generated.push_str("/// 每档 IFFT 根在 `IFFT_ROOTS` 中的复数项偏移，与变换长度表同序。\n");
    let _ = writeln!(
        generated,
        "pub(crate) const IFFT_ROOT_OFFSETS: [u16; {}] = {:?};",
        offsets.len(),
        offsets
    );
    generated.push_str("/// `exp(+j 2πe/M)` 的完整 f32 根表，由构建脚本生成并核对摘要。\n");
    let _ = writeln!(
        generated,
        "pub(crate) static IFFT_ROOTS: [[f32; 2]; {}] = [",
        all_roots.len()
    );
    for [real, imaginary] in &all_roots {
        let _ = writeln!(
            generated,
            "    [f32::from_bits(0x{real:08X}), f32::from_bits(0x{imaginary:08X})],"
        );
    }
    generated.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("ifft_roots.rs");
    std::fs::write(&out, generated)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", out.display()));
}

/// `Pseudocode 60`/`62` 的前后旋转因子 `−cos(2π(8k+1)/16N)`、`−sin(…)`。
///
/// Step 2 与 Step 4 共用同一组，故按 `k = 0…N/2−1` 存一份即可。全部角度落在
/// `(0, π/2)`，因此两个分量恒为负——这正是构建期象限判据的依据。
pub(crate) fn emit_imdct_rotation(transform_lengths: &[usize]) {
    const EXPECTED_PAIRS: usize = 5_332;

    let mut offsets = Vec::with_capacity(transform_lengths.len() + 1);
    let mut all_pairs: Vec<[u32; 2]> = Vec::with_capacity(EXPECTED_PAIRS);
    offsets.push(0u16);

    for &transform_length in transform_lengths {
        let length = transform_length as f64;
        let mut pairs = Vec::with_capacity(transform_length / 2);
        for k in 0..transform_length / 2 {
            let angle = core::f64::consts::TAU * ((8 * k + 1) as f64) / (16.0 * length);
            pairs.push([
                (-libm::cos(angle) as f32).to_bits(),
                (-libm::sin(angle) as f32).to_bits(),
            ]);
        }
        check_rotation_structure(transform_length, &pairs);
        all_pairs.extend_from_slice(&pairs);
        offsets.push(
            u16::try_from(all_pairs.len())
                .unwrap_or_else(|_| panic!("旋转因子偏移 {} 超出 u16", all_pairs.len())),
        );
    }

    assert_eq!(all_pairs.len(), EXPECTED_PAIRS);
    let mut blob = Vec::with_capacity(all_pairs.len() * 8);
    for [cosine, sine] in &all_pairs {
        blob.extend_from_slice(&cosine.to_le_bytes());
        blob.extend_from_slice(&sine.to_le_bytes());
    }
    assert_eq!(
        sha256_hex(&blob),
        IMDCT_ROTATION_SHA256,
        "旋转因子表摘要与 ADR-0003 记录不符；生成规则、libm 版本或舍入发生变化"
    );

    let mut generated = String::new();
    generated.push_str("/// 每档旋转因子在 `IMDCT_ROTATION` 中的复数项偏移，与变换长度表同序。\n");
    let _ = writeln!(
        generated,
        "pub(crate) const IMDCT_ROTATION_OFFSETS: [u16; {}] = {:?};",
        offsets.len(),
        offsets
    );
    generated.push_str("/// `[xcos1[k], xsin1[k]]` 的完整 f32 表，由构建脚本生成并核对摘要。\n");
    let _ = writeln!(
        generated,
        "pub(crate) static IMDCT_ROTATION: [[f32; 2]; {}] = [",
        all_pairs.len()
    );
    for [cosine, sine] in &all_pairs {
        let _ = writeln!(
            generated,
            "    [f32::from_bits(0x{cosine:08X}), f32::from_bits(0x{sine:08X})],"
        );
    }
    generated.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("imdct_rotation.rs");
    std::fs::write(&out, generated)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", out.display()));
}

/// 旋转因子的构建期结构判据，见规范可追踪性 5.18。
///
/// 单位圆只证明配对自洽——共同角度偏移能保持它——故另加象限与顺序两条：
/// 角度在 `(0, π/2)` 内严格递增，`−cos` 随之严格递增而 `−sin` 严格递减。
fn check_rotation_structure(transform_length: usize, pairs: &[[u32; 2]]) {
    const TOLERANCE: f64 = 1.0e-7;
    let mut previous: Option<(f64, f64)> = None;
    for (k, &[cosine_bits, sine_bits]) in pairs.iter().enumerate() {
        let cosine = f64::from(f32::from_bits(cosine_bits));
        let sine = f64::from(f32::from_bits(sine_bits));

        let deviation = (cosine * cosine + sine * sine - 1.0).abs();
        assert!(
            deviation <= TOLERANCE,
            "N={transform_length}、k={k} 的单位圆偏差 {deviation:e}"
        );
        assert!(
            cosine < 0.0 && sine < 0.0,
            "N={transform_length}、k={k} 的两个分量都应为负，实得 {cosine}, {sine}"
        );
        if let Some((previous_cosine, previous_sine)) = previous {
            assert!(
                cosine > previous_cosine,
                "N={transform_length}、k={k} 的 xcos1 应严格递增"
            );
            assert!(
                sine < previous_sine,
                "N={transform_length}、k={k} 的 xsin1 应严格递减"
            );
        }
        previous = Some((cosine, sine));
    }
}

/// `5.5.3` 的 KBD 左窗 `√(S(n)/S(N))`，`α` 按 `N_W` 查表 186。
///
/// 右窗不单独建表：`KBD_RIGHT(N, 2N−1−n) = KBD_LEFT(N, n)`，逆序索引即可。
pub(crate) fn emit_kbd_windows(transform_lengths: &[usize], alpha_halves: &[u8]) {
    const EXPECTED_VALUES: usize = 10_664;
    assert_eq!(transform_lengths.len(), alpha_halves.len());
    let mut offsets = Vec::with_capacity(transform_lengths.len() + 1);
    let mut all_values: Vec<u32> = Vec::with_capacity(EXPECTED_VALUES);
    offsets.push(0u16);

    for (index, &taper) in transform_lengths.iter().enumerate() {
        let pi_alpha = core::f64::consts::PI * (f64::from(alpha_halves[index]) / 2.0);
        let denominator = bessel_i0(pi_alpha);

        // W(N,p,α)，p = 0…N，含端点共 N+1 项。
        let mut weights = Vec::with_capacity(taper + 1);
        for p in 0..=taper {
            let ratio = 2.0 * (p as f64) / (taper as f64) - 1.0;
            let argument = pi_alpha * libm::sqrt((1.0 - ratio * ratio).max(0.0));
            weights.push(bessel_i0(argument) / denominator);
        }

        // 前缀和与总和共用同一条累加链：分开累加会改变舍入，进而改变表值。
        let mut prefix = Vec::with_capacity(taper + 1);
        let mut accumulator = 0.0f64;
        for weight in &weights {
            accumulator += weight;
            prefix.push(accumulator);
        }
        let total = accumulator;

        let mut values = Vec::with_capacity(taper);
        for slot in prefix.iter().take(taper) {
            values.push((libm::sqrt(slot / total) as f32).to_bits());
        }

        check_kbd_structure(taper, &values);
        all_values.extend_from_slice(&values);
        offsets.push(
            u16::try_from(all_values.len())
                .unwrap_or_else(|_| panic!("KBD 窗偏移 {} 超出 u16", all_values.len())),
        );
    }

    assert_eq!(all_values.len(), EXPECTED_VALUES);
    let mut blob = Vec::with_capacity(all_values.len() * 4);
    for value in &all_values {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        sha256_hex(&blob),
        KBD_LEFT_SHA256,
        "KBD 窗表摘要与 ADR-0003 记录不符；生成规则、libm 版本或舍入发生变化"
    );

    let mut generated = String::new();
    generated.push_str("/// 每档 KBD 左窗在 `KBD_LEFT` 中的项偏移，与变换长度表同序。\n");
    let _ = writeln!(
        generated,
        "pub(crate) const KBD_LEFT_OFFSETS: [u16; {}] = {:?};",
        offsets.len(),
        offsets
    );
    generated.push_str("/// `KBD_LEFT(N_W, n)` 的完整 f32 表，由构建脚本生成并核对摘要。\n");
    let _ = writeln!(
        generated,
        "pub(crate) static KBD_LEFT: [f32; {}] = [",
        all_values.len()
    );
    for value in &all_values {
        let _ = writeln!(generated, "    f32::from_bits(0x{value:08X}),");
    }
    generated.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("kbd_windows.rs");
    std::fs::write(&out, generated)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", out.display()));
}

/// 零阶修正贝塞尔 `I(x) = Σ (x^k / (2^k k!))²`，见 `5.5.3`。
///
/// 全正项，先增后减；逐项递推与终止条件按 ADR-0003 第 5 条固定，因为它们决定
/// 表值。浮点下 `term × (x/2 ÷ k)` 与 `(term × x/2) ÷ k` 的舍入一般不同——实测
/// 在这十五档上两种写法给出同一张表，但那是巧合而非可依赖的性质，故仍照 ADR
/// 的写法实现。任何真正改变表值的偏离由摘要捕获。
fn bessel_i0(x: f64) -> f64 {
    let half = x / 2.0;
    let mut term = 1.0f64;
    let mut sum = 1.0f64;
    let mut k = 1usize;
    loop {
        term *= half / (k as f64);
        let squared = term * term;
        sum += squared;
        if squared <= sum * f64::EPSILON / 4.0 {
            return sum;
        }
        k += 1;
        assert!(k < 4096, "I₀({x}) 不收敛");
    }
}

/// KBD 左窗的构建期结构判据，见规范可追踪性 5.18。
///
/// Princen-Bradley 恒等式由 `W` 关于 `p = N/2` 的对称性精确推出，但它对整表
/// 倒序不敏感，故另加值域与单调性。
fn check_kbd_structure(taper: usize, values: &[u32]) {
    const TOLERANCE: f64 = 1.0e-7;
    let decoded: Vec<f64> = values
        .iter()
        .map(|&bits| f64::from(f32::from_bits(bits)))
        .collect();

    for (n, &value) in decoded.iter().enumerate() {
        let mirrored = decoded[taper - 1 - n];
        let deviation = (value * value + mirrored * mirrored - 1.0).abs();
        assert!(
            deviation <= TOLERANCE,
            "N={taper}、n={n} 的 Princen-Bradley 偏差 {deviation:e}"
        );
        assert!(
            value > 0.0 && value <= 1.0,
            "N={taper}、n={n} 的窗值 {value} 越出 (0, 1]"
        );
    }
    for (n, window) in decoded.windows(2).enumerate() {
        assert!(window[1] >= window[0], "N={taper}、n={n} 处窗值应单调不减");
    }
}

pub(crate) fn negate_f32_bits(bits: u32) -> u32 {
    if bits & 0x7FFF_FFFF == 0 {
        0
    } else {
        bits ^ 0x8000_0000
    }
}

fn check_ifft_root_structure(length: usize, roots: &[[u32; 2]]) {
    const TOLERANCE: f64 = 1.0e-7;
    let quarter = length / 4;
    let axes = [
        (0, [1.0f32.to_bits(), 0.0f32.to_bits()]),
        (quarter, [0.0f32.to_bits(), 1.0f32.to_bits()]),
        (2 * quarter, [(-1.0f32).to_bits(), 0.0f32.to_bits()]),
        (3 * quarter, [0.0f32.to_bits(), (-1.0f32).to_bits()]),
    ];
    for (index, expected) in axes {
        assert_eq!(roots[index], expected, "M={length} 的轴点 e={index}");
    }

    for exponent in 0..length {
        let [real_bits, imaginary_bits] = roots[exponent];
        let real = f64::from(f32::from_bits(real_bits));
        let imaginary = f64::from(f32::from_bits(imaginary_bits));
        let deviation = (real * real + imaginary * imaginary - 1.0).abs();
        assert!(
            deviation <= TOLERANCE,
            "M={length}、e={exponent} 的单位圆偏差 {deviation:e}"
        );

        if exponent != 0 {
            let conjugate = roots[length - exponent];
            assert_eq!(
                real_bits, conjugate[0],
                "M={length}、e={exponent} 的共轭实部"
            );
            assert_eq!(
                imaginary_bits,
                negate_f32_bits(conjugate[1]),
                "M={length}、e={exponent} 的共轭虚部"
            );
        }
    }

    for (exponent, &[real_bits, imaginary_bits]) in roots.iter().take(quarter).enumerate().skip(1) {
        assert_eq!(
            real_bits & 0x8000_0000,
            0,
            "M={length}、e={exponent} 的第一象限实部符号"
        );
        assert_eq!(
            imaginary_bits & 0x8000_0000,
            0,
            "M={length}、e={exponent} 的第一象限虚部符号"
        );
    }
}

/// `5.1.3.2` 的反量化表 `|q|^(4/3)`，`q` 取 `0…8191`。
///
/// `5.1.3.1` NOTE 1 把 `quant_spec` 的幅度上限定在 8 191，故表覆盖全域。
///
/// **不调用宿主 `powf`。** 表值由 ADR-0002 规定的整数判据选出：`v = m × 2^e`
/// 是 f32 当且仅当 `v³ = m³ × 2^(3e)`，而目标满足 `v³ = q⁴`，因此“哪个 f32
/// 最接近 `q^(4/3)`”可以完全在整数域判定——二分出最大的 `m` 使 `m³ × 2^(3e)`
/// 不超过 `q⁴`，再用中点 `(2m+1) × 2^(e−1)` 的立方决定进位，平局取偶尾数。
/// 中间量最大 `2^75`，`u128` 有 53 位余量。
///
/// 让宿主 libm 参与表值选择会把构建机拖进可复现性边界：它的舍入不保证跨平
/// 台、跨版本一致，而 ADR-0002 的核心主张正是位精确可复现。整个位表的
/// SHA-256 因此被冻结，任何偏差立即中止构建。
pub(crate) fn emit_dequant_table() {
    const LINES: u32 = 8192;
    /// 按 `q` 升序连接每项 `to_bits().to_le_bytes()` 后的 SHA-256，见 ADR-0002。
    const EXPECTED_DIGEST: &str =
        "60b7347dff798930b6021357b9c7027234dd3455fac545aa02ff01eb4663a880";

    let mut bits = Vec::with_capacity(LINES as usize);
    for q in 0..LINES {
        bits.push(nearest_cube_root_bits(u128::from(q)));
    }

    let mut blob = Vec::with_capacity(bits.len() * 4);
    for value in &bits {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    let digest = sha256_hex(&blob);
    assert_eq!(
        digest, EXPECTED_DIGEST,
        "反量化表摘要与 ADR-0002 记录不符；生成规则或舍入判据被改动"
    );

    // 结构性断言：摘要只证明字节没变，这几条证明它是个合理的幂函数表。
    assert_eq!(bits[0], 0, "0^(4/3) 应为 +0");
    assert_eq!(bits[1], 0x3F80_0000, "1^(4/3) 应精确为 1.0");
    for window in bits.windows(2) {
        let (low, high) = (window[0], window[1]);
        assert!(low < high, "表应严格递增：0x{low:08X} → 0x{high:08X}");
        assert!(
            high & 0x7F80_0000 != 0x7F80_0000,
            "表项应为有限数：0x{high:08X}"
        );
    }

    let mut generated = String::new();
    generated.push_str(
        "/// `|q|^(4/3)`，下标即 `quant_spec` 的绝对值，见 `TS103190-1:v1.4.1:5.1.3.2`。\n\
         ///\n\
         /// 由构建脚本以整数判据正确舍入生成，位表摘要在构建期核对。\n",
    );
    let _ = writeln!(generated, "pub(crate) static REC_SPEC: [f32; {LINES}] = [");
    for value in &bits {
        let _ = writeln!(generated, "    f32::from_bits(0x{value:08X}),");
    }
    generated.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("dequant_table.rs");
    std::fs::write(&out, generated)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", out.display()));
}

/// 返回最接近 `q^(4/3)` 的 f32 位模式，全整数判定。
fn nearest_cube_root_bits(q: u128) -> u32 {
    if q == 0 {
        return 0;
    }
    let target = q * q * q * q; // q⁴，最大 8191⁴ < 2^52

    // (m × 2^e)³ 与 q⁴ 比较；两侧都左移到整数域，不引入浮点。
    let cube_cmp = |m: u128, e: i32| -> core::cmp::Ordering {
        let (mut lhs, mut rhs) = (m * m * m, target);
        let shift = 3 * e;
        if shift >= 0 {
            lhs <<= shift as u32;
        } else {
            rhs <<= shift.unsigned_abs();
        }
        lhs.cmp(&rhs)
    };

    // 找出使尾数落在 [2^23, 2^24) 的指数。
    let mut exponent = None;
    for candidate in -40..20 {
        if cube_cmp(1 << 23, candidate).is_le() && cube_cmp(1 << 24, candidate).is_gt() {
            exponent = Some(candidate);
            break;
        }
    }
    let exponent = exponent.unwrap_or_else(|| panic!("q={q} 找不到规格化指数"));

    // 二分出最大的 m 使 (m × 2^e)³ ≤ q⁴。
    let (mut low, mut high) = (1u128 << 23, 1u128 << 24);
    while high - low > 1 {
        let mid = (low + high) / 2;
        if cube_cmp(mid, exponent).is_le() {
            low = mid;
        } else {
            high = mid;
        }
    }
    let (mut mantissa, mut exponent) = (low, exponent);

    // 中点 (2m+1) × 2^(e−1) 决定进位；恰在中点时取偶尾数。
    let (mut lhs, mut rhs) = ((2 * mantissa + 1).pow(3), target);
    let shift = 3 * (exponent - 1);
    if shift >= 0 {
        lhs <<= shift as u32;
    } else {
        rhs <<= shift.unsigned_abs();
    }
    let above_midpoint = match lhs.cmp(&rhs) {
        core::cmp::Ordering::Less => true,
        core::cmp::Ordering::Greater => false,
        core::cmp::Ordering::Equal => mantissa % 2 == 1,
    };
    if above_midpoint {
        mantissa += 1;
        if mantissa == 1 << 24 {
            mantissa = 1 << 23;
            exponent += 1;
        }
    }

    let biased = exponent + 23 + 127;
    assert!(
        (1..=254).contains(&biased),
        "q={q} 的指数 {biased} 超出 f32 规格化范围"
    );
    ((biased as u32) << 23) | ((mantissa as u32) & 0x7F_FFFF)
}
