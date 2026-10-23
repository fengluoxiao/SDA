//! `5.5.2.2` 的完整 IMDCT：前旋转、IFFT、后旋转、展开加窗与重叠相加。
//!
//! 六个步骤直译 `Pseudocode 60`–`64`。旋转因子与 KBD 窗查 ADR-0003 冻结的
//! 生产表，IFFT 走 ADR-0004 的 Stockham 内核，运行期不做任何三角求值。
//!
//! **工作区与重叠缓冲都由调用方提供。** 两者的生存期不同：工作区只在一次
//! 变换内有效，可在声道间复用；重叠缓冲是跨帧状态，每个声道各持一份。谁来
//! 持有它们取决于 `Ac4DecoderSession` 的边界，那还没有定，因此这里只固定
//! 「不分配」这一条，把归属留给上层。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "全部下标由块长派生，而块长已被 rotation_factors 限定在十五档之内；\
              Step 5/6 的多重偏移用显式下标比迭代器更贴近伪码，便于逐行核对"
)]

use super::{
    WindowShape, kbd_left_window, left_window_shape, right_window_shape, rotation_factors,
};
use crate::asf::imdct::ifft::{self, Complex64, IfftError, IfftWorkspace, MAX_IFFT_LEN};
use crate::asf::tables::transform_length_48;

/// 最长变换长度 `N`，即 48 kHz 的全块。
pub(crate) const MAX_TRANSFORM_LENGTH: usize = 2 * MAX_IFFT_LEN;

/// 一次 IMDCT 的工作区，固定 80 KiB，可在声道间复用。
#[derive(Debug)]
pub struct ImdctWorkspace {
    ifft: IfftWorkspace,
    /// `Pseudocode 60` 的 `Z[k]`，随后原地充当 IFFT 输入。
    prerotated: [Complex64; MAX_IFFT_LEN],
    /// `Pseudocode 62` 的 `y[n]`。
    postrotated: [Complex64; MAX_IFFT_LEN],
    /// `Pseudocode 63` 的 `x[n]`，长 `2N`；前 `N` 个已加左窗。
    unfolded: [f64; 2 * MAX_TRANSFORM_LENGTH],
}

impl ImdctWorkspace {
    /// 建立一个全零工作区。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ifft: IfftWorkspace::new(),
            prerotated: [Complex64::new(0.0, 0.0); MAX_IFFT_LEN],
            postrotated: [Complex64::new(0.0, 0.0); MAX_IFFT_LEN],
            unfolded: [0.0; 2 * MAX_TRANSFORM_LENGTH],
        }
    }
}

impl Default for ImdctWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

/// 一个声道的跨帧重叠缓冲，见 `5.5.2.1` 的 `overlap`。
///
/// 长度恒为 `N_full`，与块长无关；块长变化时由 `nskip` 居中对齐。缓冲里存的
/// 是**未加窗**的上一块后半，右窗在下一次变换的 Step 6 才应用——规范
/// `5.5.2.1` 明确「The delayed samples from the previous block, which have not
/// been windowed, are used by the overlap/add step」。
#[derive(Debug)]
pub(crate) struct OverlapBuffer {
    samples: [f32; MAX_TRANSFORM_LENGTH],
    frame_length: u16,
    previous_length: u16,
}

impl OverlapBuffer {
    /// 以全块长度 `N_full` 建立空缓冲。
    ///
    /// `previous_length` 初值取 `N_full`：首块之前没有信号，缓冲全零，右窗
    /// 加权乘零仍是零，因此该初值不影响首块输出，但它必须是个合法块长，否则
    /// 窗形状无解。
    pub(crate) fn new(frame_length: u16) -> Option<Self> {
        transform_length_48(frame_length, 4)?;
        Some(Self {
            samples: [0.0; MAX_TRANSFORM_LENGTH],
            frame_length,
            previous_length: frame_length,
        })
    }

    /// 全块长度 `N_full`，即表 83 的 `frame_length`。
    pub(crate) const fn frame_length(&self) -> u16 {
        self.frame_length
    }

    /// 上一块的长度 `N_prev`。
    pub(crate) const fn previous_length(&self) -> u16 {
        self.previous_length
    }

    /// 缓冲中当前存放的、尚未加窗的上一块后半。
    #[cfg(test)]
    fn delayed(&self) -> &[f32] {
        let skip = (usize::from(self.frame_length) - usize::from(self.previous_length)) / 2;
        &self.samples[skip..skip + usize::from(self.previous_length)]
    }
}

/// IMDCT 无法执行的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImdctError {
    /// 块长不在 `5.5.3` 的十五档之内。
    UnsupportedLength { length: usize },
    /// 块长超过全块长度。
    BlockLongerThanFrame { block: u16, frame: u16 },
    /// 块长不属于该全块长度在表 100/103 中的变换族。
    BlockNotValidForFrame { block: u16, frame: u16 },
    /// 输出缓冲长度与块长不符。
    OutputLengthMismatch { expected: usize, provided: usize },
    /// 与上一块的长度差为奇数，窗无法居中对齐。
    IncompatibleBlockLengths { block: u16, previous: u16 },
    /// IFFT 拒绝了该长度。
    Ifft(IfftError),
}

impl From<IfftError> for ImdctError {
    fn from(error: IfftError) -> Self {
        Self::Ifft(error)
    }
}

/// 左窗第 `position` 个系数，见 Step 5：前 `skip` 个为 0，末 `skip` 个为 1。
fn left_window_value(shape: WindowShape, taper: &[f32], position: usize) -> f64 {
    let skip = usize::from(shape.skip);
    if position < skip {
        return 0.0;
    }
    match taper.get(position - skip) {
        Some(&value) => f64::from(value),
        None => 1.0,
    }
}

/// 右窗第 `position` 个系数，见 Step 6：前 `skip` 个为 1，末 `skip` 个为 0。
///
/// 渐变段是左窗的逆序，见 ADR-0003 第 4 条。
fn right_window_value(shape: WindowShape, taper: &[f32], position: usize) -> f64 {
    let skip = usize::from(shape.skip);
    if position < skip {
        return 1.0;
    }
    let Some(offset) = position.checked_sub(skip) else {
        return 1.0;
    };
    match taper
        .len()
        .checked_sub(1)
        .and_then(|last| last.checked_sub(offset))
    {
        Some(mirrored) => taper.get(mirrored).map_or(0.0, |&value| f64::from(value)),
        None => 0.0,
    }
}

/// 对一个块执行 IMDCT 与重叠相加，输出 `N` 个 PCM 样本。
///
/// `lines` 是 `5.5.2.1` 的 `s_IMDCT,ch`，其长度即块长 `N`。
pub(crate) fn transform(
    lines: &[f32],
    workspace: &mut ImdctWorkspace,
    overlap: &mut OverlapBuffer,
    pcm: &mut [f32],
) -> Result<(), ImdctError> {
    let length = lines.len();
    if pcm.len() != length {
        return Err(ImdctError::OutputLengthMismatch {
            expected: length,
            provided: pcm.len(),
        });
    }
    let block = u16::try_from(length).map_err(|_| ImdctError::UnsupportedLength { length })?;
    let rotation = rotation_factors(block).ok_or(ImdctError::UnsupportedLength { length })?;
    if block > overlap.frame_length {
        return Err(ImdctError::BlockLongerThanFrame {
            block,
            frame: overlap.frame_length,
        });
    }
    if !(0..=4).any(|index| transform_length_48(overlap.frame_length, index) == Some(block)) {
        return Err(ImdctError::BlockNotValidForFrame {
            block,
            frame: overlap.frame_length,
        });
    }

    let previous = overlap.previous_length;
    let incompatible = ImdctError::IncompatibleBlockLengths { block, previous };
    let left = left_window_shape(block, previous).ok_or(incompatible)?;
    let right = right_window_shape(block, previous).ok_or(incompatible)?;
    let taper = kbd_left_window(left.taper).ok_or(ImdctError::UnsupportedLength {
        length: usize::from(left.taper),
    })?;

    let half = length / 2;
    let quarter = length / 4;
    let ImdctWorkspace {
        ifft,
        prerotated,
        postrotated,
        unfolded,
    } = workspace;

    // Step 2：前旋转，`Z[k] = (X[N−2k−1] + j X[2k]) × (xcos1[k] + j xsin1[k])`。
    for (k, slot) in prerotated.iter_mut().take(half).enumerate() {
        let [cosine, sine] = rotation[k];
        let (cosine, sine) = (f64::from(cosine), f64::from(sine));
        let high = f64::from(lines[length - 2 * k - 1]);
        let low = f64::from(lines[2 * k]);
        *slot = Complex64::new(high * cosine - low * sine, low * cosine + high * sine);
    }

    // Step 3：`N/2` 点复数 IFFT，正号且不归一化。
    let transformed = ifft::inverse(&mut prerotated[..half], ifft)?;
    let transformed = transformed.as_slice();

    // Step 4：后旋转并除以 `N`；归一化只在这里出现一次。
    let scale = 1.0 / (length as f64);
    for (n, slot) in postrotated.iter_mut().take(half).enumerate() {
        let [cosine, sine] = rotation[n];
        let (cosine, sine) = (f64::from(cosine), f64::from(sine));
        let value = transformed[n];
        *slot = Complex64::new(
            (value.re * cosine - value.im * sine) * scale,
            (value.im * cosine + value.re * sine) * scale,
        );
    }

    // Step 5：展开、左半加窗、去交织。后 `N` 个样本不加窗，留给下一块的右窗。
    for n in 0..quarter {
        let window = |position: usize| left_window_value(left, taper, position);
        unfolded[2 * n] = postrotated[quarter + n].im * window(2 * n);
        unfolded[2 * n + 1] = -postrotated[quarter - n - 1].re * window(2 * n + 1);
        unfolded[half + 2 * n] = postrotated[n].re * window(half + 2 * n);
        unfolded[half + 2 * n + 1] = -postrotated[half - n - 1].im * window(half + 2 * n + 1);
        unfolded[length + 2 * n] = postrotated[quarter + n].re;
        unfolded[length + 2 * n + 1] = -postrotated[quarter - n - 1].im;
        unfolded[3 * half + 2 * n] = -postrotated[n].im;
        unfolded[3 * half + 2 * n + 1] = postrotated[half - n - 1].re;
    }

    // Step 6：右半加窗与重叠相加。
    let frame = usize::from(overlap.frame_length);
    let skip = (frame - length) / 2;
    let skip_previous = (frame - usize::from(previous)) / 2;

    for n in 0..usize::from(previous) {
        let weight = right_window_value(right, taper, n) as f32;
        overlap.samples[skip_previous + n] *= weight;
    }
    for n in 0..length {
        let sum = f64::from(overlap.samples[skip + n]) + unfolded[n];
        overlap.samples[skip + n] = sum as f32;
    }
    // `Pseudocode 64` 本身没有写 2 倍因子，但紧随其后的 `5.5.3` 块切换示例在
    // 三次重叠相加中都明确要求 factor of 2。本实现据此为终端 Core PCM 选择
    // `2/N` 尺度，同时让 overlap 继续保存 Pseudocode 63 的未缩放后半；进入
    // 绝对标度 QMF 工具链时的表示换算与证据边界见 ADR-0006。
    for (sample, &sum) in pcm.iter_mut().zip(overlap.samples.iter().take(length)) {
        *sample = 2.0 * sum;
    }
    for n in 0..skip {
        overlap.samples[n] = overlap.samples[length + n];
    }
    for n in 0..length {
        overlap.samples[skip + n] = unfolded[length + n] as f32;
    }

    overlap.previous_length = block;
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "下标由同一用例构造的块长与缓冲长度派生，越界即是该用例要报告的失败"
)]
mod tests {
    extern crate std;

    use super::*;
    use crate::asf::tables::TRANSFORM_LENGTHS_48;
    use std::vec;
    use std::vec::Vec;

    /// 正向 MDCT 的定义式，测试专用 oracle，不做归一化。
    ///
    /// 与 `Pseudocode 60`–`63` 无任何共享代码：它是一重求和，而生产实现是
    /// 旋转加 Stockham IFFT。两者只应在数学上一致。
    fn forward_mdct(windowed: &[f64], length: usize) -> Vec<f32> {
        (0..length)
            .map(|k| {
                let mut sum = 0.0;
                for (n, &value) in windowed.iter().enumerate() {
                    sum += value * mdct_phase(n, k, length).cos();
                }
                sum as f32
            })
            .collect()
    }

    /// 逆向 IMDCT 的定义式，含规范 `Pseudocode 62` 的 `1/N`。
    fn direct_imdct(lines: &[f32], length: usize) -> Vec<f64> {
        let scale = 1.0 / (length as f64);
        (0..2 * length)
            .map(|n| {
                let mut sum = 0.0;
                for (k, &line) in lines.iter().enumerate() {
                    sum += f64::from(line) * mdct_phase(n, k, length).cos();
                }
                sum * scale
            })
            .collect()
    }

    fn mdct_phase(n: usize, k: usize, length: usize) -> f64 {
        let length = length as f64;
        core::f64::consts::PI / length * ((n as f64) + 0.5 + length / 2.0) * ((k as f64) + 0.5)
    }

    /// 等长块的 2N 分析窗：左窗接右窗，后者是前者的逆序。
    fn symmetric_analysis_window(block: u16) -> Vec<f64> {
        let taper: Vec<f64> = kbd_left_window(block)
            .expect("表内长度都应有 KBD 窗")
            .iter()
            .map(|&value| f64::from(value))
            .collect();
        let mut window = taper.clone();
        window.extend(taper.iter().rev().copied());
        window
    }

    fn deterministic_signal(count: usize) -> Vec<f64> {
        let mut state = 0x8f4d_3c2b_1a09_7865_u64;
        (0..count)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                f64::from(((state >> 40) as i32 - 8_388_608) as f32 / 8_388_608.0)
            })
            .collect()
    }

    /// 返回允许该变换长度的最短 `N_full`。
    fn smallest_frame_for(block: u16) -> u16 {
        [384u16, 512, 768, 960, 1024, 1536, 1920, 2048]
            .into_iter()
            .find(|&frame| (0..=4).any(|index| transform_length_48(frame, index) == Some(block)))
            .expect("十五档变换长度都应属于至少一个合法 N_full")
    }

    /// 分析—合成往返在等长块上以单位增益完美重建。
    ///
    /// 这是本模块最强的判据：它一次覆盖前旋转、IFFT、后旋转、展开、加窗与
    /// 重叠相加，任何一步的相位、索引或符号出错都会让重建塌掉，而不是差一个
    /// 常数。
    ///
    /// `Pseudocode 62` 的内部样本除以 `N`；本实现依据 `5.5.3` 的重叠相加示例，
    /// 为终端 Core PCM 使用 factor of 2，两者合起来是标准 `2/N` IMDCT。该跨条款
    /// 解释及 QMF 边界见 ADR-0006。每档都通过表 99/100/103 选择合法的 `N_full`，
    /// 比较时扣除 `nskip` 的居中延迟。
    #[test]
    fn analysis_synthesis_reconstructs_at_unity_gain() {
        const FRAMES: usize = 8;

        for &block in TRANSFORM_LENGTHS_48.iter() {
            let length = usize::from(block);
            let frame_length = smallest_frame_for(block);
            let delay = (usize::from(frame_length) - length) / 2;
            let window = symmetric_analysis_window(block);
            let signal = deterministic_signal((FRAMES + 1) * length);

            let mut workspace = ImdctWorkspace::new();
            let mut overlap = OverlapBuffer::new(frame_length).expect("全块长度应受支持");
            let mut output = vec![0.0f32; FRAMES * length];

            for frame in 0..FRAMES {
                let segment: Vec<f64> = (0..2 * length)
                    .map(|index| signal[frame * length + index] * window[index])
                    .collect();
                let lines = forward_mdct(&segment, length);
                let mut pcm = vec![0.0f32; length];
                transform(&lines, &mut workspace, &mut overlap, &mut pcm).expect("等长块应可变换");
                output[frame * length..(frame + 1) * length].copy_from_slice(&pcm);
            }

            // 首帧没有前一块可重叠；短块还需扣除 N_full 居中产生的延迟。
            let mut worst = 0.0f64;
            for index in delay + length..FRAMES * length {
                let expected = signal[index - delay];
                let actual = f64::from(output[index]);
                worst = worst.max((actual - expected).abs());
            }
            assert!(
                worst <= 2.0e-5,
                "N={block} 的重建最大偏差 {worst:e} 超出预算"
            );
        }
    }

    /// 未加窗的后半 `x[N..2N]` 必须等于 IMDCT 定义式。
    ///
    /// 完美重建覆盖的是整条链路的**乘积**，窗与变换的错误可以互相掩盖；这条
    /// 只看变换本身：`Pseudocode 63` 的后 `N` 个样本不加窗，正好是纯变换输出。
    #[test]
    fn unwindowed_second_half_matches_the_direct_definition() {
        for &block in TRANSFORM_LENGTHS_48.iter() {
            let length = usize::from(block);
            let lines: Vec<f32> = deterministic_signal(length)
                .into_iter()
                .map(|value| value as f32)
                .collect();

            let mut workspace = ImdctWorkspace::new();
            let mut overlap =
                OverlapBuffer::new(smallest_frame_for(block)).expect("全块长度应受支持");
            let mut pcm = vec![0.0f32; length];
            transform(&lines, &mut workspace, &mut overlap, &mut pcm).expect("应可变换");

            let reference = direct_imdct(&lines, length);
            let mut worst = 0.0f64;
            for (index, &stored) in overlap.delayed().iter().enumerate() {
                worst = worst.max((f64::from(stored) - reference[length + index]).abs());
            }
            assert!(
                worst <= 5.0e-6,
                "N={block} 的未加窗后半与定义式最大偏差 {worst:e}"
            );
        }
    }

    /// 全零谱线产生全零输出，且不污染重叠缓冲。
    #[test]
    fn silent_spectrum_stays_silent() {
        let block = 256u16;
        let length = usize::from(block);
        let mut workspace = ImdctWorkspace::new();
        let mut overlap = OverlapBuffer::new(2048).expect("全块长度应受支持");
        let mut pcm = vec![1.0f32; length];

        transform(
            &vec![0.0f32; length],
            &mut workspace,
            &mut overlap,
            &mut pcm,
        )
        .expect("应可变换");
        assert!(pcm.iter().all(|&value| value == 0.0), "静音谱应产生静音");
        assert!(
            overlap.delayed().iter().all(|&value| value == 0.0),
            "静音谱不应在重叠缓冲留下残留"
        );
    }

    /// 块长切换后重叠缓冲按新长度居中对齐，且状态随之推进。
    #[test]
    fn switching_block_length_recenters_the_overlap_buffer() {
        let mut workspace = ImdctWorkspace::new();
        let mut overlap = OverlapBuffer::new(2048).expect("全块长度应受支持");
        assert_eq!(overlap.previous_length(), 2048);

        for &block in &[2048u16, 512, 512, 2048] {
            let length = usize::from(block);
            let lines: Vec<f32> = deterministic_signal(length)
                .into_iter()
                .map(|value| value as f32)
                .collect();
            let mut pcm = vec![0.0f32; length];
            transform(&lines, &mut workspace, &mut overlap, &mut pcm)
                .expect("2 的幂之间的切换应合法");
            assert_eq!(overlap.previous_length(), block);
            assert_eq!(overlap.delayed().len(), length);
            assert!(
                pcm.iter().all(|value| value.is_finite()),
                "N={block} 的输出应全部有限"
            );
        }
    }

    /// 非法输入一律拒绝，且不改写重叠缓冲。
    #[test]
    fn invalid_input_is_rejected_without_touching_state() {
        let mut workspace = ImdctWorkspace::new();
        let mut overlap = OverlapBuffer::new(1024).expect("全块长度应受支持");

        let mut pcm = vec![0.0f32; 300];
        assert_eq!(
            transform(&vec![0.0f32; 300], &mut workspace, &mut overlap, &mut pcm),
            Err(ImdctError::UnsupportedLength { length: 300 }),
            "表外块长应被拒绝"
        );

        let mut pcm = vec![0.0f32; 2048];
        assert_eq!(
            transform(&vec![0.0f32; 2048], &mut workspace, &mut overlap, &mut pcm),
            Err(ImdctError::BlockLongerThanFrame {
                block: 2048,
                frame: 1024
            }),
            "块长超过全块应被拒绝"
        );

        let mut pcm = vec![0.0f32; 100];
        assert_eq!(
            transform(&vec![0.0f32; 256], &mut workspace, &mut overlap, &mut pcm),
            Err(ImdctError::OutputLengthMismatch {
                expected: 256,
                provided: 100
            }),
            "输出长度不符应被拒绝"
        );

        assert_eq!(overlap.previous_length(), 1024, "拒绝不应推进状态");
        assert!(
            overlap.delayed().iter().all(|&value| value == 0.0),
            "拒绝不应写入重叠缓冲"
        );

        assert_eq!(
            OverlapBuffer::new(300).map(|_| ()),
            None,
            "表外全块长度应被拒绝"
        );
        assert_eq!(
            OverlapBuffer::new(120).map(|_| ()),
            None,
            "部分块长度不得冒充 N_full"
        );

        let mut pcm = vec![0.0f32; 960];
        assert_eq!(
            transform(&vec![0.0f32; 960], &mut workspace, &mut overlap, &mut pcm),
            Err(ImdctError::BlockNotValidForFrame {
                block: 960,
                frame: 1024
            }),
            "属于其他变换族的块长应被拒绝"
        );
        assert_eq!(overlap.previous_length(), 1024, "拒绝不应推进状态");
    }

    /// `N_full` 只改变输出延迟，不改变内容。
    ///
    /// 短块（`N < N_full`）时 Step 6 必须走搬移分支，而 `N = N_full` 时
    /// `nskip = 0` 让该分支空转。两条路径给出的样本必须**逐位相同**，只差
    /// `nskip` 的整体延迟。
    ///
    /// 这条判据不依赖分析侧几何，因此能覆盖完美重建够不到的地方：把 Step 6
    /// 的搬移与写入顺序颠倒，在 `nskip = 0` 时完全无害，只有短块路径看得见。
    #[test]
    fn full_block_length_only_delays_the_output() {
        const FRAMES: usize = 10;

        for (block, baseline, frame, delay) in [
            (512u16, 512u16, 2048u16, 768usize),
            (128, 512, 2048, 768),
            (96, 384, 1536, 576),
        ] {
            let length = usize::from(block);
            let lines: Vec<Vec<f32>> = (0..FRAMES)
                .map(|index| {
                    deterministic_signal((index + 3) * length)
                        .into_iter()
                        .skip(index * length)
                        .take(length)
                        .map(|value| value as f32)
                        .collect()
                })
                .collect();

            let run = |frame_length: u16| -> Vec<f32> {
                let mut workspace = ImdctWorkspace::new();
                let mut overlap = OverlapBuffer::new(frame_length).expect("全块长度应受支持");
                let mut out = vec![0.0f32; FRAMES * length];
                for (index, block_lines) in lines.iter().enumerate() {
                    let mut pcm = vec![0.0f32; length];
                    transform(block_lines, &mut workspace, &mut overlap, &mut pcm)
                        .expect("应可变换");
                    out[index * length..(index + 1) * length].copy_from_slice(&pcm);
                }
                out
            };

            let aligned = run(baseline);
            let centred = run(frame);
            for index in delay..FRAMES * length {
                assert_eq!(
                    centred[index].to_bits(),
                    aligned[index - delay].to_bits(),
                    "N={block}、N_full={baseline}→{frame}：样本 {index} 应只增加延迟 {delay}"
                );
            }
        }
    }

    /// 混合块长序列的往返重建，延迟必须**恒定**。
    ///
    /// 这条判据回答一个此前只靠推理下过结论的问题：`overlap` 的写入偏移是
    /// `(N_full − N)/2`，随块长变化，而输出恒取 `[0, N)`。若读写指针的相对
    /// 位移不自洽，块长一变时间轴就会错位，重建会在切换点塌掉。
    ///
    /// 分析侧按块中心 `c_i − c_{i−1} = (N_{i−1}+N_i)/2` 推进，左半用
    /// `left_window_shape(N_i, N_{i−1})`，右半用 `right_window_shape(N_{i+1}, N_i)`
    /// ——后者正是下一块做重叠相加时施加的右窗，两侧必须是同一个窗才谈得上时域
    /// 混叠抵消。序列覆盖 2 048→1 024、1 024→128、128→128、128→1 024、
    /// 1 024→2 048 五种切换，含表 187 的 `1 024|8*128` 与 `8*128|1 024` 两行。
    ///
    /// **它抓不到窗形状本身的错。** 分析侧复用同两个形状函数，形状一改两侧
    /// 一起改，重建照样成立——实测把 `Nw` 截到 1 024 时本判据全过，只有
    /// `equal_block_lengths_give_a_pure_taper` 与等长重建拦下。这里锁的是
    /// `transform` 对形状的**用法**：写入偏移、搬移量与输出取址。
    #[test]
    fn mixed_block_lengths_reconstruct_with_a_constant_delay() {
        const FRAME: u16 = 2048;
        const PAD: usize = 4096;

        let mut blocks: Vec<u16> = vec![2048, 2048, 1024];
        blocks.extend(core::iter::repeat_n(128u16, 16));
        blocks.extend([1024, 2048, 2048]);
        let total: usize = blocks.iter().map(|&block| usize::from(block)).sum();

        // 窗中心逐块推进；`c_0 = N_0` 让首块的 2N 窗从 PAD 处起。
        let mut centres = Vec::with_capacity(blocks.len());
        let mut centre = usize::from(blocks[0]);
        centres.push(centre);
        for pair in blocks.windows(2) {
            centre += (usize::from(pair[0]) + usize::from(pair[1])) / 2;
            centres.push(centre);
        }

        let signal = deterministic_signal(2 * PAD + centres[centres.len() - 1] + total);

        let mut workspace = ImdctWorkspace::new();
        let mut overlap = OverlapBuffer::new(FRAME).expect("2 048 是合法全块长度");
        let mut output = Vec::with_capacity(total);
        for (index, &block) in blocks.iter().enumerate() {
            let length = usize::from(block);
            let previous = if index == 0 { FRAME } else { blocks[index - 1] };
            let next = blocks.get(index + 1).copied().unwrap_or(block);
            let left = left_window_shape(block, previous).expect("左窗形状");
            let right = right_window_shape(next, block).expect("右窗形状");
            let left_taper = kbd_left_window(left.taper).expect("左窗渐变段");
            let right_taper = kbd_left_window(right.taper).expect("右窗渐变段");

            let start = PAD + centres[index] - length;
            let windowed: Vec<f64> = (0..2 * length)
                .map(|n| {
                    let weight = if n < length {
                        left_window_value(left, left_taper, n)
                    } else {
                        right_window_value(right, right_taper, n - length)
                    };
                    signal[start + n] * weight
                })
                .collect();

            let lines = forward_mdct(&windowed, length);
            let mut pcm = vec![0.0f32; length];
            transform(&lines, &mut workspace, &mut overlap, &mut pcm).expect("块序列应合法");
            output.extend_from_slice(&pcm);

            // `N_full` 长的缓冲够用、不需要另设 composition buffer，靠的是这条
            // 不变式：每块结束后，下标 `≥ (N_full + N)/2` 的样本恒为零。右窗
            // 归零段的起点正是 `(N_full + min(N, N_prev))/2`，与下一块相加区间
            // 的右端衔接，因此新块再长也只会落进已归零的区域，撞不上更早的残留。
            // 若不成立，长块就会把陈旧样本加进输出，那才真需要更大的缓冲。
            let live = (usize::from(FRAME) + length) / 2;
            assert!(
                overlap.samples[live..].iter().all(|&value| value == 0.0),
                "第 {index} 块（N={block}）之后，下标 {live} 起应无残留"
            );
        }

        // 读指针每块前进 N，起点是 `c_0 − (N_full + N_0)/2`；首块即全块时它退化
        // 为 0，此处 N_0 = N_full = 2 048 故延迟为 0，输出下标即信号下标。
        let delay = (usize::from(FRAME) - usize::from(blocks[0])) / 2;
        let warmup = 2 * usize::from(blocks[0]);
        let mut worst = 0.0f64;
        let mut worst_at = 0usize;
        for index in warmup..total - usize::from(FRAME) {
            let expected = signal[PAD + index - delay];
            let error = (f64::from(output[index]) - expected).abs();
            if error > worst {
                worst = error;
                worst_at = index;
            }
        }
        assert!(
            worst <= 3.0e-5,
            "混合块长重建最大偏差 {worst:e}（样本 {worst_at}）超出预算"
        );
    }

    /// `5.5.3` 的文字示例逐项对上：`frame_length = 1 920`，块序列 480、480、960。
    ///
    /// 这是规范给出的**唯一一个完整数值例子**，且用自然语言写成，与
    /// `Pseudocode 63`/`64` 相互独立。它同时钉住三样东西：短块的重叠缓冲偏移
    /// 是 720、切换到长块后变成 480；块三的左窗是「前 240 个为 0，中间 480 个
    /// 用 KBD left 480，其后 240 个不变」的扩展窗；每次重叠相加都带 factor of 2。
    #[test]
    fn matches_the_worked_example_in_clause_5_5_3() {
        const FRAME: u16 = 1920;

        // 前两块 480：前一块不更短，故左窗是未修改的 KBD left 480。
        for previous in [FRAME, 480] {
            let left = left_window_shape(480, previous).expect("应有左窗");
            assert_eq!(
                left,
                WindowShape {
                    skip: 0,
                    taper: 480
                },
                "N_prev={previous} 时块 480 的左窗应是未修改的 KBD left 480"
            );
        }
        // 块一的右窗覆盖上一完整块：720 个 1 + KBD right 480 + 720 个 0。
        assert_eq!(
            right_window_shape(480, FRAME).expect("应有右窗"),
            WindowShape {
                skip: 720,
                taper: 480
            }
        );
        // 块二、三的右窗都作用在上一块的 480 个样本上，同样未修改。
        assert_eq!(
            right_window_shape(480, 480).expect("应有右窗"),
            WindowShape {
                skip: 0,
                taper: 480
            }
        );
        assert_eq!(
            right_window_shape(960, 480).expect("应有右窗"),
            WindowShape {
                skip: 0,
                taper: 480
            }
        );

        // 块三 960，前一块 480 更短：左窗是「240 个 0 + KBD left 480 + 240 个 1」。
        let extended = left_window_shape(960, 480).expect("应有左窗");
        assert_eq!(
            extended,
            WindowShape {
                skip: 240,
                taper: 480
            },
            "块 960 的左窗应是扩展到 960 的 KBD left 480"
        );
        assert_eq!(extended.len(), 960, "扩展窗仍须铺满整块");

        // 重叠缓冲的偏移：短块 720，长块 480，与示例中的 offset 一致。
        assert_eq!((usize::from(FRAME) - 480) / 2, 720);
        assert_eq!((usize::from(FRAME) - 960) / 2, 480);

        // 文字示例逐块给出的几何；全部数字直接写出，不调用生产形状 helper。
        struct WorkedBlock {
            length: usize,
            left_zero: usize,
            previous_length: usize,
            previous_offset: usize,
            right_one: usize,
            current_offset: usize,
            move_count: usize,
        }

        let blocks = [
            WorkedBlock {
                length: 480,
                left_zero: 0,
                previous_length: 1920,
                previous_offset: 0,
                right_one: 720,
                current_offset: 720,
                move_count: 720,
            },
            WorkedBlock {
                length: 480,
                left_zero: 0,
                previous_length: 480,
                previous_offset: 720,
                right_one: 0,
                current_offset: 720,
                move_count: 720,
            },
            WorkedBlock {
                length: 960,
                left_zero: 240,
                previous_length: 480,
                previous_offset: 720,
                right_one: 0,
                current_offset: 480,
                move_count: 480,
            },
        ];

        // 示例的上一帧含完整块。注入非零存量，才能实际观察块一的扩展右窗；
        // OverlapBuffer::new() 的全零初值会把任何右窗错误都乘没。
        let mut workspace = ImdctWorkspace::new();
        let mut overlap = OverlapBuffer::new(FRAME).expect("1 920 是合法全块长度");
        let initial: Vec<f32> = deterministic_signal(2 * usize::from(FRAME))
            .into_iter()
            .skip(usize::from(FRAME))
            .map(|value| value as f32)
            .collect();
        overlap.samples[..usize::from(FRAME)].copy_from_slice(&initial);
        let mut reference = overlap.samples;
        let taper = kbd_left_window(480).expect("示例使用 KBD 480 窗");

        for (block_index, geometry) in blocks.iter().enumerate() {
            let length = geometry.length;
            let lines: Vec<f32> = deterministic_signal((block_index + 2) * length)
                .into_iter()
                .skip((block_index + 1) * length)
                .take(length)
                .map(|value| value as f32)
                .collect();
            let unfolded = direct_imdct(&lines, length);

            // 独立直写示例的右窗：前 right_one 个为 1，随后 KBD right 480，余下为 0。
            for n in 0..geometry.previous_length {
                let weight = if n < geometry.right_one {
                    1.0
                } else if n < geometry.right_one + taper.len() {
                    taper[taper.len() - 1 - (n - geometry.right_one)]
                } else {
                    0.0
                };
                reference[geometry.previous_offset + n] *= weight;
            }

            // 独立直写左窗与重叠：前 left_zero 个为 0，随后 KBD left 480，余下为 1。
            for n in 0..length {
                let weight = if n < geometry.left_zero {
                    0.0
                } else if n < geometry.left_zero + taper.len() {
                    f64::from(taper[n - geometry.left_zero])
                } else {
                    1.0
                };
                let sum = f64::from(reference[geometry.current_offset + n]) + unfolded[n] * weight;
                reference[geometry.current_offset + n] = sum as f32;
            }
            let expected_pcm: Vec<f32> = reference[..length]
                .iter()
                .map(|&sample| 2.0 * sample)
                .collect();

            // 示例的搬移与后半存储同样使用硬编码 offset。
            for n in 0..geometry.move_count {
                reference[n] = reference[length + n];
            }
            for n in 0..length {
                reference[geometry.current_offset + n] = unfolded[length + n] as f32;
            }

            let mut pcm = vec![0.0f32; length];
            transform(&lines, &mut workspace, &mut overlap, &mut pcm)
                .expect("示例中的块序列必须合法");

            let mut worst_pcm = 0.0f64;
            for (&actual, &expected) in pcm.iter().zip(expected_pcm.iter()) {
                assert!(actual.is_finite(), "块 {} 的 PCM 必须有限", block_index + 1);
                worst_pcm = worst_pcm.max((f64::from(actual) - f64::from(expected)).abs());
            }
            assert!(
                worst_pcm <= 2.0e-5,
                "块 {} 的 PCM 与文字示例参考路径偏差 {worst_pcm:e}",
                block_index + 1
            );

            let mut worst_overlap = 0.0f64;
            for (&actual, &expected) in overlap.samples.iter().zip(reference.iter()) {
                assert!(
                    actual.is_finite(),
                    "块 {} 的 overlap 必须有限",
                    block_index + 1
                );
                worst_overlap = worst_overlap.max((f64::from(actual) - f64::from(expected)).abs());
            }
            assert!(
                worst_overlap <= 1.0e-5,
                "块 {} 的 overlap 与文字示例参考路径偏差 {worst_overlap:e}",
                block_index + 1
            );

            assert_eq!(
                overlap.previous_length(),
                u16::try_from(length).expect("示例块长可表示为 u16")
            );
            assert_eq!(overlap.delayed().len(), length);
        }
    }

    /// 工作区与重叠缓冲的大小固定，不随块长变化。
    #[test]
    fn buffers_have_fixed_size() {
        assert_eq!(core::mem::size_of::<ImdctWorkspace>(), 80 * 1024);
        assert_eq!(
            core::mem::size_of::<OverlapBuffer>(),
            MAX_TRANSFORM_LENGTH * 4 + 4
        );
    }
}
