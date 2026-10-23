//! `Pseudocode 61` 的 mixed-radix Stockham IFFT。
//!
//! ADR-0004 选定 radix-4/2/3/5、power-first、正号且无归一化的标量基线。
//! 根表由构建脚本以锁定版本的 `libm` 生成并冻结摘要；运行期只做查表与 f64
//! 四则运算，不分配、不递归，也不使用 FMA。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "十五档固定计划、根表偏移与工作区长度由构建期及全域测试共同约束"
)]

use crate::asf::tables::TRANSFORM_LENGTHS_48;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/ifft_roots.rs"));
}

use generated::{IFFT_ROOT_OFFSETS, IFFT_ROOTS};

pub(crate) const MAX_IFFT_LEN: usize = 1024;
const MAX_STAGES: usize = 5;
const MAX_RADIX: usize = 5;

/// 与 `TRANSFORM_LENGTHS_48` 同序的 power-first 计划；零是未使用的尾部槽位。
const SELECTED_RADICES: [[u8; MAX_STAGES]; 15] = [
    [4, 4, 4, 4, 4], // 1024
    [4, 4, 4, 3, 5], // 960
    [4, 4, 4, 4, 3], // 768
    [4, 4, 4, 4, 2], // 512
    [4, 4, 2, 3, 5], // 480
    [4, 4, 4, 2, 3], // 384
    [4, 4, 4, 4, 0], // 256
    [4, 4, 3, 5, 0], // 240
    [4, 4, 4, 3, 0], // 192
    [4, 4, 4, 2, 0], // 128
    [4, 2, 3, 5, 0], // 120
    [4, 4, 2, 3, 0], // 96
    [4, 4, 4, 0, 0], // 64
    [4, 3, 5, 0, 0], // 60
    [4, 4, 3, 0, 0], // 48
];

const SELECTED_STAGE_COUNTS: [u8; 15] = [5, 5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 3, 3, 3];

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Complex64 {
    pub(crate) re: f64,
    pub(crate) im: f64,
}

impl Complex64 {
    const ZERO: Self = Self { re: 0.0, im: 0.0 };

    pub(crate) const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.re + other.re, self.im + other.im)
    }

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.re * other.re - self.im * other.im,
            self.re * other.im + self.im * other.re,
        )
    }

    #[cfg(test)]
    fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
}

/// 一个最大长度的复数 f64 scratch，固定为 16 KiB。
#[derive(Debug)]
pub(crate) struct IfftWorkspace {
    scratch: [Complex64; MAX_IFFT_LEN],
}

impl IfftWorkspace {
    pub(crate) const fn new() -> Self {
        Self {
            scratch: [Complex64::ZERO; MAX_IFFT_LEN],
        }
    }
}

impl Default for IfftWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfftError {
    UnsupportedLength { length: usize },
}

/// 末级所在的借用缓冲；调用方可直接接后旋转，避免一次全长复制。
#[derive(Debug)]
pub(crate) enum IfftOutput<'a> {
    Input(&'a [Complex64]),
    Scratch(&'a [Complex64]),
}

impl IfftOutput<'_> {
    pub(crate) fn as_slice(&self) -> &[Complex64] {
        match self {
            Self::Input(values) | Self::Scratch(values) => values,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IfftPlan {
    length: usize,
    radices: [u8; MAX_STAGES],
    stages: usize,
}

impl IfftPlan {
    fn selected(length: usize) -> Option<(Self, usize)> {
        let row = row_of_ifft_length(length)?;
        let radices = *SELECTED_RADICES.get(row)?;
        let stages = usize::from(*SELECTED_STAGE_COUNTS.get(row)?);
        Some((
            Self {
                length,
                radices,
                stages,
            },
            row,
        ))
    }

    fn factors(&self) -> &[u8] {
        &self.radices[..self.stages]
    }

    #[cfg(test)]
    fn for_order(length: usize, order: FactorOrder) -> Option<Self> {
        row_of_ifft_length(length)?;
        let mut plan = Self {
            length,
            radices: [0; MAX_STAGES],
            stages: 0,
        };
        let mut remaining = length;

        if order == FactorOrder::OddFirst {
            plan.take_factor(&mut remaining, 5)?;
            plan.take_factor(&mut remaining, 3)?;
        }
        while remaining.is_multiple_of(4) {
            plan.push(4)?;
            remaining /= 4;
        }
        if remaining.is_multiple_of(2) {
            plan.push(2)?;
            remaining /= 2;
        }
        if order == FactorOrder::PowerFirst {
            plan.take_factor(&mut remaining, 3)?;
            plan.take_factor(&mut remaining, 5)?;
        }
        (remaining == 1).then_some(plan)
    }

    #[cfg(test)]
    fn take_factor(&mut self, remaining: &mut usize, radix: usize) -> Option<()> {
        while (*remaining).is_multiple_of(radix) {
            self.push(u8::try_from(radix).ok()?)?;
            *remaining /= radix;
        }
        Some(())
    }

    #[cfg(test)]
    fn push(&mut self, radix: u8) -> Option<()> {
        *self.radices.get_mut(self.stages)? = radix;
        self.stages += 1;
        Some(())
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FactorOrder {
    PowerFirst,
    OddFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputBuffer {
    Input,
    Scratch,
}

fn row_of_ifft_length(length: usize) -> Option<usize> {
    TRANSFORM_LENGTHS_48
        .iter()
        .position(|&transform_length| usize::from(transform_length) / 2 == length)
}

fn roots_for_row(row: usize) -> Option<&'static [[f32; 2]]> {
    let start = usize::from(*IFFT_ROOT_OFFSETS.get(row)?);
    let end = usize::from(*IFFT_ROOT_OFFSETS.get(row.checked_add(1)?)?);
    IFFT_ROOTS.get(start..end)
}

/// 执行正号、无归一化 IFFT，见 `TS103190-1:v1.4.1:Pseudocode 61`。
pub(crate) fn inverse<'a>(
    input: &'a mut [Complex64],
    workspace: &'a mut IfftWorkspace,
) -> Result<IfftOutput<'a>, IfftError> {
    let length = input.len();
    let (plan, row) = IfftPlan::selected(length).ok_or(IfftError::UnsupportedLength { length })?;
    let roots = roots_for_row(row).ok_or(IfftError::UnsupportedLength { length })?;
    debug_assert_eq!(roots.len(), length);

    let location = stockham_ifft(plan, input, &mut workspace.scratch[..length], roots);
    Ok(match location {
        OutputBuffer::Input => IfftOutput::Input(input),
        OutputBuffer::Scratch => IfftOutput::Scratch(&workspace.scratch[..length]),
    })
}

trait RootTable {
    fn root(&self, index: usize) -> Complex64;
}

impl RootTable for [[f32; 2]] {
    fn root(&self, index: usize) -> Complex64 {
        let [real, imaginary] = self[index];
        Complex64::new(f64::from(real), f64::from(imaginary))
    }
}

#[cfg(test)]
impl RootTable for [Complex64] {
    fn root(&self, index: usize) -> Complex64 {
        self[index]
    }
}

/// 一层 mixed-radix Stockham autosort。
fn stockham_stage<R: RootTable + ?Sized>(
    source: &[Complex64],
    destination: &mut [Complex64],
    roots: &R,
    before: usize,
    radix: usize,
) {
    let length = source.len();
    let sections = length / (before * radix);
    let input_stride = length / radix;
    let small_root_stride = length / radix;

    for section in 0..sections {
        for position in 0..before {
            let mut values = [Complex64::ZERO; MAX_RADIX];
            for (branch, value) in values.iter_mut().enumerate().take(radix) {
                let source_index = section * before + position + branch * input_stride;
                let twiddle_index = position * branch * sections % length;
                *value = source[source_index].mul(roots.root(twiddle_index));
            }

            for output_branch in 0..radix {
                let mut sum = Complex64::ZERO;
                for (input_branch, &value) in values.iter().enumerate().take(radix) {
                    let root_index = input_branch * output_branch * small_root_stride % length;
                    sum = sum.add(value.mul(roots.root(root_index)));
                }
                let destination_index =
                    section * before * radix + position + output_branch * before;
                destination[destination_index] = sum;
            }
        }
    }
}

fn stockham_ifft<R: RootTable + ?Sized>(
    plan: IfftPlan,
    input: &mut [Complex64],
    scratch: &mut [Complex64],
    roots: &R,
) -> OutputBuffer {
    let mut before = 1;
    let mut source_is_input = true;

    for &radix in plan.factors() {
        let radix = usize::from(radix);
        if source_is_input {
            stockham_stage(input, scratch, roots, before, radix);
        } else {
            stockham_stage(scratch, input, roots, before, radix);
        }
        before *= radix;
        source_is_input = !source_is_input;
    }

    debug_assert_eq!(before, plan.length);
    if source_is_input {
        OutputBuffer::Input
    } else {
        OutputBuffer::Scratch
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::format;

    const ONE: Complex64 = Complex64::new(1.0, 0.0);

    /// 差分用的 f64 参考根，只服务算法层判据。
    ///
    /// 这里调用宿主 `std` 是有意的：`both_factor_orders_…` 让参考根同时喂给
    /// Stockham 与定义式，差分的是算法而非根值；`production_f32_roots_…` 的
    /// 容差比宿主实现之间的 f64 差异大八个数量级。**生产表的具体值不由本函数
    /// 校验**——那是摘要与 `scripts/check_transform_tables.py` 的职责，测试只
    /// 核对表内的派生关系，因此不把单元测试绑到宿主 `cos`/`sin` 的位模式上。
    fn test_roots(length: usize, output: &mut [Complex64; MAX_IFFT_LEN]) {
        let length_f64 = length as f64;
        let quarter = length / 4;
        output[..length].fill(Complex64::ZERO);

        for offset in 0..quarter {
            let angle = std::f64::consts::TAU * offset as f64 / length_f64;
            let (sin, cos) = angle.sin_cos();
            output[offset] = Complex64::new(cos, canonical_zero(sin));
            output[quarter + offset] = Complex64::new(canonical_zero(-sin), cos);
        }
        output[2 * quarter] = Complex64::new(-1.0, 0.0);
        for exponent in 1..=2 * quarter {
            let first_half = output[exponent];
            output[length - exponent] =
                Complex64::new(first_half.re, canonical_zero(-first_half.im));
        }
    }

    fn canonical_zero(value: f64) -> f64 {
        if value == 0.0 { 0.0 } else { value }
    }

    fn canonical_zero_f32(value: f32) -> f32 {
        if value == 0.0 { 0.0 } else { value }
    }

    /// 定义式故意不复用生产内核的复数乘加原语。
    fn direct_ifft(input: &[Complex64], roots: &[Complex64], destination: &mut [Complex64]) {
        let length = input.len();
        for (time, result) in destination.iter_mut().enumerate() {
            let mut sum_re = 0.0;
            let mut sum_im = 0.0;
            for (frequency, &value) in input.iter().enumerate() {
                let root = roots[frequency * time % length];
                sum_re += value.re * root.re - value.im * root.im;
                sum_im += value.re * root.im + value.im * root.re;
            }
            *result = Complex64::new(sum_re, sum_im);
        }
    }

    fn deterministic_signal(output: &mut [Complex64]) {
        let mut state = 0x8f4d_3c2b_1a09_7865_u64;
        for value in output {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let re = f64::from(((state >> 40) as i32 - 8_388_608) as f32 / 8_388_608.0);
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let im = f64::from(((state >> 40) as i32 - 8_388_608) as f32 / 8_388_608.0);
            *value = Complex64::new(re, im);
        }
    }

    fn result<'a>(
        location: OutputBuffer,
        input: &'a [Complex64],
        scratch: &'a [Complex64],
    ) -> &'a [Complex64] {
        match location {
            OutputBuffer::Input => input,
            OutputBuffer::Scratch => scratch,
        }
    }

    fn assert_complex_close(actual: Complex64, expected: Complex64, context: &str) {
        let error = Complex64::new(actual.re - expected.re, actual.im - expected.im).abs();
        let tolerance = 3.0e-11 * expected.abs().max(1.0);
        assert!(
            error <= tolerance,
            "{context}: actual={actual:?}, expected={expected:?}, error={error:e}, tolerance={tolerance:e}"
        );
    }

    #[test]
    fn selected_plans_match_power_first_factorization_at_all_lengths() {
        for &transform_length in &TRANSFORM_LENGTHS_48 {
            let length = usize::from(transform_length) / 2;
            let (selected, _) = IfftPlan::selected(length).expect("有固定计划");
            let derived = IfftPlan::for_order(length, FactorOrder::PowerFirst).expect("可分解");
            assert_eq!(selected, derived, "M={length} 的固定计划");
            assert_eq!(
                selected
                    .factors()
                    .iter()
                    .map(|&radix| usize::from(radix))
                    .product::<usize>(),
                length
            );
        }
    }

    #[test]
    fn inverse_rejects_lengths_outside_the_ac4_table_without_mutating_input() {
        let mut workspace = IfftWorkspace::new();
        for length in [0, 1, 30, 1000, MAX_IFFT_LEN + 1] {
            let mut input = [Complex64::new(7.0, -3.0); MAX_IFFT_LEN + 1];
            let before = input;
            assert_eq!(
                inverse(&mut input[..length], &mut workspace).unwrap_err(),
                IfftError::UnsupportedLength { length }
            );
            assert_eq!(input, before, "拒绝 M={length} 不得改写输入");
        }
    }

    #[test]
    fn generated_roots_cover_every_length_with_exact_axes_and_conjugates() {
        for (row, &transform_length) in TRANSFORM_LENGTHS_48.iter().enumerate() {
            let length = usize::from(transform_length) / 2;
            let quarter = length / 4;
            let roots = roots_for_row(row).expect("有根表");
            assert_eq!(roots.len(), length);
            for (index, expected) in [
                (0, [1.0f32, 0.0f32]),
                (quarter, [0.0f32, 1.0f32]),
                (2 * quarter, [-1.0f32, 0.0f32]),
                (3 * quarter, [0.0f32, -1.0f32]),
            ] {
                assert_eq!(roots[index][0].to_bits(), expected[0].to_bits());
                assert_eq!(roots[index][1].to_bits(), expected[1].to_bits());
            }
            for exponent in 1..length {
                let conjugate = roots[length - exponent];
                assert_eq!(roots[exponent][0].to_bits(), conjugate[0].to_bits());
                assert_eq!(
                    roots[exponent][1].to_bits(),
                    canonical_zero_f32(-conjugate[1]).to_bits()
                );
            }
        }
    }

    #[test]
    fn production_inverse_is_positive_natural_order_and_not_normalized() {
        let mut workspace = IfftWorkspace::new();
        for &transform_length in &TRANSFORM_LENGTHS_48 {
            let length = usize::from(transform_length) / 2;
            let mut impulse = [Complex64::ZERO; MAX_IFFT_LEN];
            impulse[0] = ONE;
            let output = inverse(&mut impulse[..length], &mut workspace).expect("支持的长度");
            for (index, &value) in output.as_slice().iter().enumerate() {
                assert_complex_close(value, ONE, &format!("M={length}, impulse n={index}"));
            }

            let mut unit_bin = [Complex64::ZERO; MAX_IFFT_LEN];
            unit_bin[1] = ONE;
            let output = inverse(&mut unit_bin[..length], &mut workspace).expect("支持的长度");
            let row = row_of_ifft_length(length).expect("有行");
            for (index, (&actual, root)) in output
                .as_slice()
                .iter()
                .zip(roots_for_row(row).expect("有根表"))
                .enumerate()
            {
                let expected = Complex64::new(f64::from(root[0]), f64::from(root[1]));
                let error = Complex64::new(actual.re - expected.re, actual.im - expected.im).abs();
                assert!(error <= 4.0e-6, "M={length}, n={index}, error={error:e}");
            }
        }
    }

    #[test]
    fn both_factor_orders_match_the_direct_definition_at_all_lengths() {
        let mut seed = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut input = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut scratch = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut expected = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut roots = [Complex64::ZERO; MAX_IFFT_LEN];

        for &transform_length in &TRANSFORM_LENGTHS_48 {
            let length = usize::from(transform_length) / 2;
            deterministic_signal(&mut seed[..length]);
            test_roots(length, &mut roots);
            direct_ifft(&seed[..length], &roots[..length], &mut expected[..length]);

            for order in [FactorOrder::PowerFirst, FactorOrder::OddFirst] {
                input[..length].copy_from_slice(&seed[..length]);
                let plan = IfftPlan::for_order(length, order).expect("可分解");
                let location = stockham_ifft(
                    plan,
                    &mut input[..length],
                    &mut scratch[..length],
                    &roots[..length],
                );
                for (index, (&actual, &reference)) in
                    result(location, &input[..length], &scratch[..length])
                        .iter()
                        .zip(expected.iter())
                        .enumerate()
                {
                    assert_complex_close(
                        actual,
                        reference,
                        &format!("{order:?}, M={length}, n={index}"),
                    );
                }
            }
        }
    }

    #[test]
    fn production_f32_roots_stay_within_the_numeric_budget() {
        let mut seed = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut input = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut expected = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut reference_roots = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut workspace = IfftWorkspace::new();
        let mut worst_normalized = 0.0_f64;
        let mut error_energy = 0.0_f64;
        let mut reference_energy = 0.0_f64;

        for &transform_length in &TRANSFORM_LENGTHS_48 {
            let length = usize::from(transform_length) / 2;
            deterministic_signal(&mut seed[..length]);
            input[..length].copy_from_slice(&seed[..length]);
            test_roots(length, &mut reference_roots);
            direct_ifft(
                &seed[..length],
                &reference_roots[..length],
                &mut expected[..length],
            );

            let output = inverse(&mut input[..length], &mut workspace).expect("支持的长度");
            for (&actual, &reference) in output.as_slice().iter().zip(expected.iter()) {
                let error =
                    Complex64::new(actual.re - reference.re, actual.im - reference.im).abs();
                let reference_magnitude = reference.abs();
                worst_normalized = worst_normalized.max(error / reference_magnitude.max(1.0));
                error_energy += error * error;
                reference_energy += reference_magnitude * reference_magnitude;
            }
        }

        let relative_rms = (error_energy / reference_energy).sqrt();
        assert!(
            worst_normalized <= 4.0e-6,
            "最坏归一误差 {worst_normalized:e} 超出预算"
        );
        assert!(
            relative_rms <= 1.0e-7,
            "相对 RMS 误差 {relative_rms:e} 超出预算"
        );
    }

    #[test]
    fn workspace_is_exactly_one_sixteen_kibibyte_buffer() {
        assert_eq!(core::mem::size_of::<IfftWorkspace>(), 16 * 1024);
    }

    /// `cargo test -p macindecode-ac4-decode --release --features spec-tables ifft_factor_order_benchmark -- --ignored --nocapture`
    #[test]
    #[ignore = "手动运行的 IFFT 因子顺序微基准"]
    fn ifft_factor_order_benchmark() {
        const ROUNDS: usize = 5;
        const ITERATIONS: usize = 200;

        for round in 1..=ROUNDS {
            for order in [FactorOrder::PowerFirst, FactorOrder::OddFirst] {
                let elapsed = benchmark_order(order, ITERATIONS);
                std::eprintln!("round {round}: {order:?} = {elapsed:?}");
            }
        }
    }

    fn benchmark_order(order: FactorOrder, iterations: usize) -> std::time::Duration {
        let mut total = std::time::Duration::ZERO;
        let mut seed = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut input = [Complex64::ZERO; MAX_IFFT_LEN];
        let mut scratch = [Complex64::ZERO; MAX_IFFT_LEN];

        for (row, &transform_length) in TRANSFORM_LENGTHS_48.iter().enumerate() {
            let length = usize::from(transform_length) / 2;
            let plan = IfftPlan::for_order(length, order).expect("可分解");
            let roots = roots_for_row(row).expect("有根表");
            deterministic_signal(&mut seed[..length]);

            let started = std::time::Instant::now();
            for _ in 0..iterations {
                input[..length].copy_from_slice(&seed[..length]);
                let location =
                    stockham_ifft(plan, &mut input[..length], &mut scratch[..length], roots);
                std::hint::black_box(result(location, &input[..length], &scratch[..length]));
            }
            total += started.elapsed();
        }
        total
    }

    /// 第二象限必须由第一象限逐位换位得到：`exp(j(θ+π/2)) = (−sin θ, cos θ)`。
    ///
    /// 这条与 `generated_roots_cover_every_length_with_exact_axes_and_conjugates`
    /// 的轴点、共轭断言合起来，覆盖了构建脚本从四分之一圆铺满整圆的全部规则。
    /// 第一象限的具体值不在此校验：那需要独立的高精度实现，由摘要与
    /// `scripts/check_transform_tables.py` 承担。此处若改用宿主 `cos`/`sin`
    /// 复算并逐位比对，测试就会绑定宿主实现——构建按 `libm` 生成且摘要一致，
    /// 而宿主 `cos` 与 `libm` 只要有一项收窄后不同，失败信息就会指向「量化不
    /// 匹配」，掩盖真实原因。本机实测这两者在这批角度上 f64 有 116/2 636 项
    /// 位模式不同，收窄 f32 后为 0 项，但那是本机的结论，不是契约。
    #[test]
    fn generated_roots_rotate_the_first_quadrant_into_the_second() {
        for (row, &transform_length) in TRANSFORM_LENGTHS_48.iter().enumerate() {
            let length = usize::from(transform_length) / 2;
            let quarter = length / 4;
            let roots = roots_for_row(row).expect("有根表");
            for offset in 1..quarter {
                let [cosine, sine] = roots[offset];
                let [rotated_real, rotated_imaginary] = roots[quarter + offset];
                assert_eq!(
                    rotated_real.to_bits(),
                    canonical_zero_f32(-sine).to_bits(),
                    "M={length}、e={offset} 换位后的实部应为 −sin"
                );
                assert_eq!(
                    rotated_imaginary.to_bits(),
                    cosine.to_bits(),
                    "M={length}、e={offset} 换位后的虚部应为 cos"
                );
            }
        }
    }
}
