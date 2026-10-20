//! A-SPX 噪声表，取自表 D.2（随附 C 表的 `ASPX_NOISE[512][2]`）。

use super::sha256::sha256_hex;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

const ASPX_NOISE_SHA256: &str = "6ef5390a34b211cf5f190019b8295f349b56eb493f7828b88cbba272890a2e9f";

/// `5.7.6.4.3` 的噪声表：512 个复数，随机相位、平均能量为 1。
///
/// 构建期核对两条**与摘要无关**的结构判据。摘要只保证「和上次一样」，这两条
/// 才说明这 1 024 个数确实是一张可用的噪声表：
///
/// - **逐项单位模**：`re² + im² == 1`。规范只声称「平均能量为 1」，而实测每一
///   项都是单位模（最大偏离 `1,3×10⁻⁶`，与六位小数的舍入量级一致）。
/// - **平均能量为 1**：规范在 `5.7.6.4.3` 与表 D.2 明写的那一条，独立断言。
///
/// # 这两条不是同一条判据
///
/// 平均值被 512 项摊薄。往解析器里注入「第 7 项的实部加 `10⁻⁵`」后：逐项偏离
/// `1,55×10⁻⁵` 超出 `10⁻⁵` 预算而中止构建，同一扰动下平均能量只偏离
/// `1,2×10⁻⁸`，远在 `10⁻⁶` 预算之内。只留规范明写的那条会漏掉三个数量级。
///
/// # 这道门禁没有自动判据
///
/// 构建脚本的 `#[cfg(test)]` 不被 `cargo test` 执行，而放宽这里的预算在数据正确
/// 时不改变任何输出，因此没有任何单元判据会响。上面那次是**手工**的解析器缺陷
/// 注入；改动本文件的预算或判据后要重做一次，不能只看测试全绿。
///
/// 另需注意：`verify_hash` 在解析之前就核对了 C 文件的字节，因此这两条拦的是
/// **解析错误**，不是文件被换掉——后者由 `MANIFEST.json` 的 `member_sha256` 挡。
///
/// # 列序无法由规范判定
///
/// 表 D.2 只给出 `num_columns 2`，正文只说「512 个复数」。哪一列是实部，规范
/// 没有明说；「随机相位」与「平均能量为 1」两条性质在实虚互换下都不变，因此
/// 数据本身也判不出来。这里取 C 的常规写法 `{re, im}`，**没有独立证据**。
/// 互换的后果是每个噪声样本关于 45° 线镜像，统计性质完全相同，只有与参考解码
/// 器逐位对照才可能分辨。
pub(crate) fn emit_aspx_noise(complex: &BTreeMap<String, Vec<[f32; 2]>>) {
    const ROWS: usize = 512;

    let table = complex.get("ASPX_NOISE").unwrap_or_else(|| {
        panic!("规范随附 C 表中没有 ASPX_NOISE；请重新运行 scripts/fetch_specs.py")
    });
    assert_eq!(table.len(), ROWS, "ASPX_NOISE 应有 {ROWS} 行");

    // 六位小数的十进制字面量，单位模的往返误差量级为 1e-6；预算取 1e-5，
    // 能拦住第五位及更高位的抄错，拦不住第六位——这是该判据的已知边界。
    let mut worst = 0.0f64;
    let mut total = 0.0f64;
    for (index, [re, im]) in table.iter().enumerate() {
        let re = f64::from(*re);
        let im = f64::from(*im);
        let energy = re * re + im * im;
        total += energy;
        let deviation = (energy - 1.0).abs();
        assert!(
            deviation <= 1.0e-5,
            "ASPX_NOISE[{index}] 的能量偏离单位模 {deviation:e}，超出预算"
        );
        worst = worst.max(deviation);
    }
    let mean = total / ROWS as f64;
    assert!(
        (mean - 1.0).abs() <= 1.0e-6,
        "ASPX_NOISE 的平均能量 {mean} 不满足规范声称的 1"
    );
    // 逐项判据若被削弱成平均值判据，这一行会立刻显出差距。
    assert!(worst > 0.0, "ASPX_NOISE 全部恰为单位模，预算判据未被执行过");

    let mut blob = Vec::with_capacity(ROWS * 8);
    for [re, im] in table {
        blob.extend_from_slice(&re.to_bits().to_le_bytes());
        blob.extend_from_slice(&im.to_bits().to_le_bytes());
    }
    let digest = sha256_hex(&blob);
    assert_eq!(
        digest, ASPX_NOISE_SHA256,
        "A-SPX 噪声表摘要与冻结值不符：解析或 f32 化发生了变化"
    );

    let mut out = String::with_capacity(ROWS * 40);
    out.push_str("/// `5.7.6.4.3` 表 D.2 的 A-SPX 噪声表，由构建脚本从规范随附 C 表生成。\n");
    out.push_str("///\n/// 每项为 `[实部, 虚部]`，见 build_support/noise.rs 对列序的说明。\n");
    out.push_str("pub(crate) static ASPX_NOISE: [[f32; 2]; 512] = [\n");
    for [re, im] in table {
        let _ = writeln!(
            out,
            "    [f32::from_bits({:#010x}), f32::from_bits({:#010x})],",
            re.to_bits(),
            im.to_bits()
        );
    }
    out.push_str("];\n");

    let path = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("aspx_noise.rs");
    std::fs::write(&path, out)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", path.display()));
}
