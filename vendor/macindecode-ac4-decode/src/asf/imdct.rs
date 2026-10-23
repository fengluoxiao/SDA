//! IMDCT 的块切换与窗口序列（`TS103190-1:v1.4.1:5.5`）。
//!
//! 本模块包含表 186 的 KBD α、`5.5.2.2` Step 5/6 的窗口分段、
//! `Pseudocode 61` 的 crate 内 Stockham IFFT，以及完整 IMDCT。
//!
//! 变换涉及的常量——`5.5.2.2` 的旋转因子
//! `−cos(2π(8k+1)/16N)`、IFFT 的 `cos(4πkn/N)`，以及 `5.5.3` 的 KBD 窗（零阶
//! 修正贝塞尔 `I(x) = Σ (x^k / (2^k k!))²` 求和后开方）——无法像 `|q|^(4/3)`
//! 那样化为整数立方比较，后者能成立是因为 `v³ = q⁴` 把问题搬回了整数域，三角
//! 函数没有这样的代数出口。**常量方案见 ADR-0003，IFFT 选型见 ADR-0004**：
//! IFFT 根、前/后旋转与 KBD 窗三张生产表均已由构建期锁定版本的 `libm` 生成
//! 并冻结 SHA-256，运行期只查表。[`transform`] 把六个步骤接合为完整 IMDCT，
//! 工作区与重叠缓冲由调用方提供。
//!
//! 窗口整数部分是变换的控制输入，且自带可闭合的判据——窗口
//! 三段长度之和必须精确等于对应的块长度。

use crate::asf::tables::TRANSFORM_LENGTHS_48;
use crate::spec_tables::asf::KBD_ALPHA_HALVES_48;

// 生产 IFFT 只由尚未接入声道解码路径的完整变换消费；保持 crate 内可见，避免
// 提前冻结 DSP API。
#[allow(dead_code, reason = "等待完整变换接入声道解码路径的 crate 内生产内核")]
pub(crate) mod ifft;

// 两个子模块的说明写在各自文件的 `//!` 头里。此处不再加 `///`：外层文档会与
// 内层文档拼成同一块，而链接随之改在本模块的作用域里解析，子模块自己 `use`
// 进来的类型名会解不出来（实测 3 条 intra-doc 链接因此失效）。
pub mod frame;
pub mod transform;

/// 由构建脚本生成的三角常量表，见 ADR-0003。
#[allow(dead_code, reason = "等待完整变换接入声道解码路径的常量表")]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/imdct_rotation.rs"));
    include!(concat!(env!("OUT_DIR"), "/kbd_windows.rs"));
}

/// 变换长度在十五档表中的行号；表外长度返回 `None`。
fn row_of_transform_length(transform_length: u16) -> Option<usize> {
    TRANSFORM_LENGTHS_48
        .iter()
        .position(|&length| length == transform_length)
}

/// 某变换长度的 KBD α 值，以半整数表示（`6` 即 α = 3）。
///
/// 只覆盖 44,1 kHz 与 48 kHz；96/192 kHz 的长度是这些值的 2 或 4 倍，α 相同，
/// 但那两档采样率在成帧层已被拒绝（`AsfTransformInfo`），此处不做映射。
#[must_use]
pub fn kbd_alpha_halves(transform_length: u16) -> Option<u8> {
    KBD_ALPHA_HALVES_48
        .get(row_of_transform_length(transform_length)?)
        .copied()
}

/// 某变换长度的前/后旋转因子 `[xcos1[k], xsin1[k]]`，`k = 0…N/2−1`。
///
/// `Pseudocode 60` 的 Step 2 与 `Pseudocode 62` 的 Step 4 共用同一组，故只存
/// 一份。表由构建脚本以锁定版本的 `libm` 生成并冻结摘要，运行期只查表。
#[must_use]
#[allow(dead_code, reason = "等待完整变换接入声道解码路径")]
pub(crate) fn rotation_factors(transform_length: u16) -> Option<&'static [[f32; 2]]> {
    let row = row_of_transform_length(transform_length)?;
    let start = usize::from(*generated::IMDCT_ROTATION_OFFSETS.get(row)?);
    let end = usize::from(*generated::IMDCT_ROTATION_OFFSETS.get(row.checked_add(1)?)?);
    generated::IMDCT_ROTATION.get(start..end)
}

/// 某重叠区宽度 `N_W` 的 KBD 左窗，`n = 0…N_W−1`，见 `5.5.3`。
///
/// 参数是 `Nw = min(N, N_prev)` 而非块长：窗形由重叠区宽度决定，`α` 也按它
/// 查表 186。
#[must_use]
#[allow(dead_code, reason = "等待窗口应用接入")]
pub(crate) fn kbd_left_window(taper: u16) -> Option<&'static [f32]> {
    let row = row_of_transform_length(taper)?;
    let start = usize::from(*generated::KBD_LEFT_OFFSETS.get(row)?);
    let end = usize::from(*generated::KBD_LEFT_OFFSETS.get(row.checked_add(1)?)?);
    generated::KBD_LEFT.get(start..end)
}

/// KBD 右窗第 `index` 项，`index = 0…N_W−1` 对应规范的 `n = N_W…2N_W−1`。
///
/// 不单独建表：右窗求和上限为 `2N−n−1`，代入即得左窗的上限 `n`，因此
/// `KBD_RIGHT(N, 2N−1−n) = KBD_LEFT(N, n)`，逆序索引同一张表即可。
#[must_use]
#[allow(dead_code, reason = "等待窗口应用接入")]
pub(crate) fn kbd_right_window_value(taper: u16, index: usize) -> Option<f32> {
    let window = kbd_left_window(taper)?;
    let mirrored = window.len().checked_sub(1)?.checked_sub(index)?;
    window.get(mirrored).copied()
}

/// 一侧变换窗的分段长度，见 `5.5.2.2` Step 5 与 Step 6。
///
/// 窗由三段构成，两端等长：
///
/// | 段 | 长度 | 左窗（Step 5） | 右窗（Step 6） |
/// |---|---|---|---|
/// | 前 | `skip` | 恒 0 | 恒 1 |
/// | 中 | `taper` | `KBD_LEFT(Nw, ·)` | `KBD_RIGHT(Nw, ·)` |
/// | 后 | `skip` | 恒 1 | 恒 0 |
///
/// 两端等长是 `Nskip` 的定义决定的，不是巧合：左窗 `Nskip = (N − Nw)/2`，
/// 三段和恰为 `N`；右窗 `Nskip = (N_prev − Nw)/2`，三段和恰为 `N_prev`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowShape {
    /// 两端各自的长度，即 `Nskip`。
    pub skip: u16,
    /// 中间 KBD 渐变段的长度，即 `Nw`。
    pub taper: u16,
}

impl WindowShape {
    /// 窗覆盖的样本数，`taper + 2 × skip`。
    #[must_use]
    pub const fn len(&self) -> u32 {
        (self.taper as u32).saturating_add((self.skip as u32).saturating_mul(2))
    }

    /// 窗是否不覆盖任何样本。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `Nw = min(N, N_prev)`，Step 5 与 Step 6 共用。
///
/// 规范把它写成两段条件（`N ≤ N_prev` 取 `N`，否则取 `N_prev`），即较短的那个
/// ——相邻两块的重叠区不可能宽于其中较窄者。
const fn taper_length(block: u16, previous: u16) -> u16 {
    if block <= previous { block } else { previous }
}

/// 左窗分段，见 Step 5：`Nskip = (N − Nw) / 2`，三段和为 `N`。
///
/// 返回 `None` 当且仅当 `N − Nw` 为奇数，那意味着块长度不成 2 的倍数关系，
/// 与 `5.5.3` 的「部分块是全块的 2、4、8、16 分之一」矛盾。
#[must_use]
pub fn left_window_shape(block: u16, previous: u16) -> Option<WindowShape> {
    let taper = taper_length(block, previous);
    let spread = block.checked_sub(taper)?;
    if spread % 2 != 0 {
        return None;
    }
    Some(WindowShape {
        skip: spread / 2,
        taper,
    })
}

/// 右窗分段，见 Step 6：`Nskip = (N_prev − Nw) / 2`，三段和为 `N_prev`。
///
/// 与左窗**不对称**：两者的 `Nw` 相同，但铺开的宽度分别由当前块和前一块决定。
/// 因此块长度切换时，同一次重叠相加的两侧窗形状不同，这正是保持时域混叠抵消
/// 所必需的。
#[must_use]
pub fn right_window_shape(block: u16, previous: u16) -> Option<WindowShape> {
    let taper = taper_length(block, previous);
    let spread = previous.checked_sub(taper)?;
    if spread % 2 != 0 {
        return None;
    }
    Some(WindowShape {
        skip: spread / 2,
        taper,
    })
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "下标由同一用例刚断言过的表长派生，越界即是该用例要报告的失败"
)]
mod tests {
    extern crate std;

    use super::*;

    /// 两张表都必须按十五档长度切满，每档的项数由规范定义式给出。
    ///
    /// 偏移表是唯一把连续存储切成十五段的东西；切错会让某档读到相邻档的
    /// 尾部，而值本身仍是合法的余弦或窗值，摘要也照样通过。
    #[test]
    fn generated_tables_are_sliced_into_fifteen_lengths() {
        let (mut rotation_total, mut window_total) = (0usize, 0usize);
        for &length in TRANSFORM_LENGTHS_48.iter() {
            let rotation = rotation_factors(length).expect("表内长度都应有旋转因子");
            assert_eq!(
                rotation.len(),
                usize::from(length) / 2,
                "N={length} 的旋转因子应有 N/2 对"
            );
            rotation_total += rotation.len();

            let window = kbd_left_window(length).expect("表内长度都应有 KBD 窗");
            assert_eq!(
                window.len(),
                usize::from(length),
                "N_W={length} 的左窗应有 N_W 项"
            );
            window_total += window.len();
        }
        assert_eq!(rotation_total, 5_332, "旋转因子总对数");
        assert_eq!(window_total, 10_664, "KBD 窗总项数");

        assert_eq!(rotation_factors(1000), None, "表外长度无旋转因子");
        assert_eq!(kbd_left_window(1000), None, "表外长度无 KBD 窗");
        assert_eq!(kbd_right_window_value(1000, 0), None);
    }

    /// 旋转因子的角度必须是 `2π(8k+1)/16N`，逐档核对首末两项。
    ///
    /// 用容差而非逐位比较：表由 `libm` 生成，此处的参考值来自宿主 `cos`，两者
    /// 可以差一两个 ulp。容差取 `1×10⁻⁶`，比宿主间差异大两个数量级，又比取错
    /// `k` 或写错分母造成的偏差小得多。**具体值由摘要与高精度审计锚定**，本用
    /// 例只确认表里装的是这个角度，不是别的角度。
    #[test]
    fn rotation_factors_follow_the_specified_angle() {
        for &length in TRANSFORM_LENGTHS_48.iter() {
            let rotation = rotation_factors(length).expect("表内长度都应有旋转因子");
            let denominator = 16.0 * f64::from(length);
            for k in [0usize, rotation.len() - 1] {
                let angle = std::f64::consts::TAU * ((8 * k + 1) as f64) / denominator;
                let (sine, cosine) = angle.sin_cos();
                assert!(
                    (f64::from(rotation[k][0]) + cosine).abs() <= 1.0e-6,
                    "N={length}、k={k} 的 xcos1 应为 −cos({angle})"
                );
                assert!(
                    (f64::from(rotation[k][1]) + sine).abs() <= 1.0e-6,
                    "N={length}、k={k} 的 xsin1 应为 −sin({angle})"
                );
            }
        }
    }

    /// KBD 左窗从接近 0 升到接近 1，且满足 Princen-Bradley 恒等式。
    ///
    /// 恒等式在构建期已核对过一遍；这里重跑是因为它检验的是**读出来的**表，
    /// 能一并覆盖偏移切分——切错档时两端不再互补。
    #[test]
    fn kbd_windows_rise_from_zero_to_one_and_stay_complementary() {
        for &taper in TRANSFORM_LENGTHS_48.iter() {
            let window = kbd_left_window(taper).expect("表内长度都应有 KBD 窗");
            let last = window.len() - 1;

            assert!(
                window[0] > 0.0 && window[0] < 0.01,
                "N_W={taper} 的首项应接近 0，实得 {}",
                window[0]
            );
            assert!(
                window[last] > 0.99 && window[last] <= 1.0,
                "N_W={taper} 的末项应接近 1，实得 {}",
                window[last]
            );

            for (n, &value) in window.iter().enumerate() {
                let mirrored = f64::from(window[last - n]);
                let value = f64::from(value);
                assert!(
                    (value * value + mirrored * mirrored - 1.0).abs() <= 1.0e-7,
                    "N_W={taper}、n={n} 的 Princen-Bradley 偏差"
                );
            }
        }
    }

    /// 右窗是左窗的逐位逆序，两端各自对齐。
    #[test]
    fn kbd_right_window_mirrors_the_left() {
        for &taper in TRANSFORM_LENGTHS_48.iter() {
            let window = kbd_left_window(taper).expect("表内长度都应有 KBD 窗");
            for index in 0..window.len() {
                assert_eq!(
                    kbd_right_window_value(taper, index).map(f32::to_bits),
                    Some(window[window.len() - 1 - index].to_bits()),
                    "N_W={taper}、index={index} 的右窗取值"
                );
            }
            assert_eq!(
                kbd_right_window_value(taper, window.len()),
                None,
                "N_W={taper} 越界索引应无值"
            );
        }
    }

    /// 表 186 的五档 α，逐档核对首个长度。
    #[test]
    fn kbd_alpha_follows_table_186() {
        for (length, halves) in [
            (2048u16, 6u8),
            (1920, 6),
            (1536, 6),
            (1024, 8),
            (960, 8),
            (768, 8),
            (512, 9),
            (480, 9),
            (384, 9),
            (256, 10),
            (240, 10),
            (192, 10),
            (128, 12),
            (120, 12),
            (96, 12),
        ] {
            assert_eq!(
                kbd_alpha_halves(length),
                Some(halves),
                "变换长度 {length} 的 α"
            );
        }
        assert_eq!(kbd_alpha_halves(1000), None, "表外长度无 α");
    }

    /// α 随变换长度单调不增：块越短，窗越钝。
    ///
    /// 这条不依赖逐项抄录是否正确，只依赖表 186 的形状；抄错某一档的相对
    /// 大小会立刻违反它。
    #[test]
    fn kbd_alpha_decreases_with_block_length() {
        let mut previous = None;
        for &length in TRANSFORM_LENGTHS_48.iter() {
            let alpha = kbd_alpha_halves(length).expect("表内长度都应有 α");
            if let Some(previous) = previous {
                assert!(
                    alpha >= previous,
                    "长度 {length} 的 α 半整数 {alpha} 应不小于前一项 {previous}"
                );
            }
            previous = Some(alpha);
        }
    }

    /// 窗口三段长度之和必须精确等于对应块长——左窗为 `N`，右窗为 `N_prev`。
    ///
    /// 这是本步能闭合的判据：`Nskip` 由差值折半而来，任何一侧取错块长都会让
    /// 和对不上。遍历 `5.5.3` 允许的全部长度对。
    #[test]
    fn window_segments_span_exactly_one_block() {
        for &block in TRANSFORM_LENGTHS_48.iter() {
            for &previous in TRANSFORM_LENGTHS_48.iter() {
                let left = left_window_shape(block, previous).expect("规范内长度对必须有左窗形状");
                let right =
                    right_window_shape(block, previous).expect("规范内长度对必须有右窗形状");
                assert_eq!(
                    left.len(),
                    u32::from(block),
                    "左窗应覆盖 N = {block}（N_prev = {previous}）"
                );
                assert_eq!(
                    right.len(),
                    u32::from(previous),
                    "右窗应覆盖 N_prev = {previous}（N = {block}）"
                );
                assert_eq!(left.taper, right.taper, "两侧的 Nw 相同");
            }
        }
    }

    /// 块长不变时窗退化为纯 KBD：没有前导零段，也没有平坦段。
    #[test]
    fn equal_block_lengths_give_a_pure_taper() {
        for &length in TRANSFORM_LENGTHS_48.iter() {
            let left = left_window_shape(length, length).expect("等长应有解");
            let right = right_window_shape(length, length).expect("等长应有解");
            assert_eq!(
                left,
                WindowShape {
                    skip: 0,
                    taper: length
                }
            );
            assert_eq!(right, left, "等长时两侧形状相同");
        }
    }

    /// 切换块长时两侧窗形状不同，而 `Nw` 相同。
    ///
    /// 2 048 → 512 的一次切换：当前块 512，重叠区仍只有 512 宽，但右窗要铺满
    /// 前一块的 2 048，因此两端各留 768 的平坦/零段。
    #[test]
    fn switching_block_length_makes_the_two_sides_differ() {
        let left = left_window_shape(512, 2048).expect("有解");
        let right = right_window_shape(512, 2048).expect("有解");

        assert_eq!(
            left,
            WindowShape {
                skip: 0,
                taper: 512
            },
            "短块的左窗无铺开"
        );
        assert_eq!(
            right,
            WindowShape {
                skip: 768,
                taper: 512
            },
            "右窗要铺满前一块的 2 048"
        );
        assert_eq!(right.len(), 2048);

        // 反向切换：当前块长、前块短。
        let left_back = left_window_shape(2048, 512).expect("有解");
        assert_eq!(
            left_back,
            WindowShape {
                skip: 768,
                taper: 512
            },
            "长块的左窗要铺满自身的 2 048"
        );
        assert_eq!(left_back.len(), 2048);
    }

    /// 差值为奇数时无解，而不是悄悄截断。
    ///
    /// `5.5.3` 规定部分块是全块的 2、4、8、16 分之一，差值必为偶数；奇数差
    /// 说明块长度组合本身非法。
    #[test]
    fn odd_spreads_have_no_window_shape() {
        assert_eq!(left_window_shape(97, 96), None);
        assert_eq!(right_window_shape(96, 97), None);
    }
}
