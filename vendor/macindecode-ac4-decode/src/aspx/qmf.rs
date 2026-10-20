//! 64 子带复数 QMF 滤波器组（`TS103190-1:v1.4.1:5.7.3`、`5.7.4`）。
//!
//! `Pseudocode 65` 是分析，`Pseudocode 66` 是合成，两者共用表 D.3 的 640 抽头
//! 原型窗 `QWIN`。窗取自规范随附 C 表，不经人工转写；构建期另核对两条与摘要
//! 无关的结构判据，见 `build.rs` 的 `emit_qmf_window`。
//!
//! 分析调制按 ADR-0009 利用子带 `sb` 与 `63-sb` 的精确换位相位成对累加，
//! 合成调制按 ADR-0008 利用输出 `n` 与 `127-n` 的精确负共轭关系成对累加；
//! 两者都共享一半乘法与相位加载，同时保持每个输出内部原有的累加顺序，因此逐位 PCM 不变。
//! 128 点 FFT 加前/后旋转虽更快，但会改变加法结合顺序，已因无法通过逐位门禁而
//! 否决。三角值同样查构建期生成的表，运行期不做求值。
//!
//! 滤波器状态与工作区都由调用方提供，不分配；生存期与 `Ac4DecoderSession` 的
//! 边界一起定，此处只固定「不分配」这一条。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "全部下标由 64/128/640/1280 这几个常量与已核对过的时隙数派生，\
              两条伪码的多重偏移用显式下标比迭代器更贴近原文，便于逐行核对"
)]

use crate::aspx::tables::NUM_QMF_SUBBANDS;

/// 表 D.3 的原型窗抽头数。
pub const NUM_QMF_WIN_COEF: usize = 640;

/// 子带数，`5.7.3.2` 规定恒为 64。
pub(crate) const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;
/// `Pseudocode 65` 折叠后的向量长度。
const FOLDED: usize = 2 * SUBBANDS;
/// `Pseudocode 66` 的合成状态长度。
const SYNTH_STATE: usize = 10 * FOLDED;

/// 本实现为终端 Core PCM 保留的 2 倍表示因子。
///
/// `Pseudocode 62`/`64` 与 `5.5.3` 对该因子的落位并不自洽；生产链按 ADR-0006
/// 的跨条款裁决，在 Core PCM 跨进或跨出绝对标度的 QMF 工具链时做互逆换算。
/// 纯 [`analyse`]/[`synthesise`] 始终保持 `Pseudocode 65`/`66` 的字面量级。
const AC4_PCM_INTERFACE_GAIN: f32 = 2.0;

/// 单位圆表的点数，见 [`modulation`]。
const MODULATION_POINTS: usize = 512;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/qmf_window.rs"));
}

mod generated_modulation {
    include!(concat!(env!("OUT_DIR"), "/qmf_modulation.rs"));
}

pub(crate) use generated::QMF_WINDOW;

/// 一个 QMF 时隙的 64 个复数子带样本。
///
/// 实部虚部分开存放而不是 `[(f32, f32); 64]`：A-SPX 的包络与噪声处理逐子带
/// 遍历实部或模值，分开存能让那些循环连续读。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QmfSlot {
    /// 实部，下标即子带号。
    pub re: [f32; SUBBANDS],
    /// 虚部。
    pub im: [f32; SUBBANDS],
}

impl QmfSlot {
    /// 全零时隙。
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            re: [0.0; SUBBANDS],
            im: [0.0; SUBBANDS],
        }
    }
}

impl Default for QmfSlot {
    fn default() -> Self {
        Self::zero()
    }
}

/// 分析滤波器组的跨帧状态，即 `Pseudocode 65` 的 `qmf_filt`。
#[derive(Debug, PartialEq)]
pub struct QmfAnalysisState {
    filt: [f32; NUM_QMF_WIN_COEF],
}

impl QmfAnalysisState {
    /// 建立全零状态。首帧之前没有信号，等价于前置静音。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            filt: [0.0; NUM_QMF_WIN_COEF],
        }
    }
}

impl Default for QmfAnalysisState {
    fn default() -> Self {
        Self::new()
    }
}

/// 合成滤波器组的跨帧状态，即 `Pseudocode 66` 的 `qsyn_filt`。
///
/// 规范写作 1 280 个样本，其中 1 152 个跨调用保留——即每时隙左移 128 之后
/// 剩下的部分。
#[derive(Debug, PartialEq)]
pub struct QmfSynthesisState {
    filt: [f32; SYNTH_STATE],
}

impl QmfSynthesisState {
    /// 建立全零状态。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            filt: [0.0; SYNTH_STATE],
        }
    }
}

impl Default for QmfSynthesisState {
    fn default() -> Self {
        Self::new()
    }
}

/// QMF 无法执行的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QmfError {
    /// 输入 PCM 的长度不是 64 的整数倍。
    UnalignedInput { samples: usize },
    /// 输出容量与时隙数不符。
    SlotCountMismatch { expected: usize, provided: usize },
}

/// 多声道 QMF 合成的形状不成立。
///
/// 批量入口只在同一条 QMF 时间轴上并排处理声道；所有形状会在推进任一路跨帧
/// 状态之前完整核对。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QmfChannelsError {
    /// 输入、状态与输出的声道数不同。
    ChannelCountMismatch {
        inputs: usize,
        states: usize,
        outputs: usize,
    },
    /// 时隙数无法换算为 PCM 样本数。
    SampleCountOverflow { timeslots: usize },
    /// 某路 QMF 帧不足以覆盖公共时隙数。
    InputTooShort {
        channel: usize,
        needed: usize,
        provided: usize,
    },
    /// 某路 PCM 输出长度不等于公共时隙数乘 64。
    OutputLengthMismatch {
        channel: usize,
        expected: usize,
        provided: usize,
    },
    /// 奇数路批次的末路标量回退失败。
    ScalarTail { channel: usize, source: QmfError },
}

/// `Pseudocode 65` 的分析滤波。
///
/// `pcm` 的长度必须是 64 的整数倍，每 64 个样本产出一个时隙；`slots` 的长度
/// 必须等于时隙数。
///
/// # Errors
///
/// 长度不成立时返回 [`QmfError`]，且不改写状态。
pub fn analyse(
    pcm: &[f32],
    state: &mut QmfAnalysisState,
    slots: &mut [QmfSlot],
) -> Result<(), QmfError> {
    if !pcm.len().is_multiple_of(SUBBANDS) {
        return Err(QmfError::UnalignedInput { samples: pcm.len() });
    }
    let timeslots = pcm.len() / SUBBANDS;
    if slots.len() != timeslots {
        return Err(QmfError::SlotCountMismatch {
            expected: timeslots,
            provided: slots.len(),
        });
    }

    let mut folded = [0.0f64; FOLDED];
    for (ts, slot) in slots.iter_mut().enumerate() {
        // 左移 64，再按「越高下标越旧」喂入新样本。
        state
            .filt
            .copy_within(0..NUM_QMF_WIN_COEF - SUBBANDS, SUBBANDS);
        for sb in 0..SUBBANDS {
            state.filt[SUBBANDS - 1 - sb] = pcm[ts * SUBBANDS + sb];
        }

        // 加窗并折叠成 128 项。
        for (n, cell) in folded.iter_mut().enumerate() {
            let mut sum = 0.0f64;
            for k in 0..5 {
                let index = n + k * FOLDED;
                sum += f64::from(state.filt[index]) * f64::from(QMF_WINDOW[index]);
            }
            *cell = sum;
        }

        modulate_analysis_slot(&folded, slot);
    }
    Ok(())
}

/// 把一个折叠时隙调制成 64 个分析子带。
///
/// 令 `q = 2sb+1`、`m = 2n-1`。镜像子带 `63-sb` 的奇数频率为 `128-q`，故
/// 其相位是 `exp(j·mπ/2) · conj(exp(j·q·m·2π/512))`。`n` 为偶数时
/// 实虚贡献都是原相位贡献换位后取负，`n` 为奇数时只换位。两个子带因而共享
/// `value·cos` 与 `value·sin`；四个累加器仍各自严格按 `n=0..127` 前进。
#[inline(always)]
fn modulate_analysis_slot(folded: &[f64; FOLDED], slot: &mut QmfSlot) {
    for sb in 0..SUBBANDS / 2 {
        let mirrored_sb = SUBBANDS - 1 - sb;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        let (mut mirrored_re, mut mirrored_im) = (0.0f64, 0.0f64);

        for n in (0..FOLDED).step_by(2) {
            let (cos, sin) = analysis_phase(sb, n);
            let real = folded[n] * cos;
            let imaginary = folded[n] * sin;
            re += real;
            im += imaginary;
            mirrored_re += -imaginary;
            mirrored_im += -real;

            let next = n + 1;
            let (cos, sin) = analysis_phase(sb, next);
            let real = folded[next] * cos;
            let imaginary = folded[next] * sin;
            re += real;
            im += imaginary;
            mirrored_re += imaginary;
            mirrored_im += real;
        }

        slot.re[sb] = re as f32;
        slot.im[sb] = im as f32;
        slot.re[mirrored_sb] = mirrored_re as f32;
        slot.im[mirrored_sb] = mirrored_im as f32;
    }
}

/// ARM64 NEON 与 portable x86-64 SSE2 的两路 `f64` 分析 lane。
const ANALYSIS_CHANNEL_LANES: usize = 2;

/// 两声道分析在加窗折叠后的局部 AoSoA。
///
/// 每个 lane 都保留 [`analyse`] 的五抽头求和与 128 项调制累加顺序；这里只把
/// 两条互不相关的声道时间线并排，允许 LLVM 在 safe Rust 下生成垂直 SIMD。
#[repr(C, align(16))]
struct AnalysisChannelPair {
    folded: [[f64; ANALYSIS_CHANNEL_LANES]; FOLDED],
}

impl AnalysisChannelPair {
    const fn new() -> Self {
        Self {
            folded: [[0.0; ANALYSIS_CHANNEL_LANES]; FOLDED],
        }
    }
}

/// 两路声道并排执行镜像子带分析调制。
///
/// 声道 lane 之间没有归约，子带镜像也只共享乘法结果；八个输出累加器都保持
/// `n=0..127` 的定义顺序。固定两 lane 让 LLVM 在 safe Rust 下继续生成 2×f64
/// SIMD，而偶数/奇数 `n` 的显式相邻处理消除了镜像符号的热循环分支。
#[inline(always)]
fn modulate_analysis_slot_pair(
    pair: &AnalysisChannelPair,
    left_slot: &mut QmfSlot,
    right_slot: &mut QmfSlot,
) {
    for sb in 0..SUBBANDS / 2 {
        let mirrored_sb = SUBBANDS - 1 - sb;
        let (mut left_re, mut left_im) = (0.0f64, 0.0f64);
        let (mut left_mirrored_re, mut left_mirrored_im) = (0.0f64, 0.0f64);
        let (mut right_re, mut right_im) = (0.0f64, 0.0f64);
        let (mut right_mirrored_re, mut right_mirrored_im) = (0.0f64, 0.0f64);

        for n in (0..FOLDED).step_by(2) {
            let (cos, sin) = analysis_phase(sb, n);
            let [left, right] = pair.folded[n];
            let left_real = left * cos;
            let left_imaginary = left * sin;
            let right_real = right * cos;
            let right_imaginary = right * sin;
            left_re += left_real;
            left_im += left_imaginary;
            left_mirrored_re += -left_imaginary;
            left_mirrored_im += -left_real;
            right_re += right_real;
            right_im += right_imaginary;
            right_mirrored_re += -right_imaginary;
            right_mirrored_im += -right_real;

            let next = n + 1;
            let (cos, sin) = analysis_phase(sb, next);
            let [left, right] = pair.folded[next];
            let left_real = left * cos;
            let left_imaginary = left * sin;
            let right_real = right * cos;
            let right_imaginary = right * sin;
            left_re += left_real;
            left_im += left_imaginary;
            left_mirrored_re += left_imaginary;
            left_mirrored_im += left_real;
            right_re += right_real;
            right_im += right_imaginary;
            right_mirrored_re += right_imaginary;
            right_mirrored_im += right_real;
        }

        left_slot.re[sb] = left_re as f32;
        left_slot.im[sb] = left_im as f32;
        left_slot.re[mirrored_sb] = left_mirrored_re as f32;
        left_slot.im[mirrored_sb] = left_mirrored_im as f32;
        right_slot.re[sb] = right_re as f32;
        right_slot.im[sb] = right_im as f32;
        right_slot.re[mirrored_sb] = right_mirrored_re as f32;
        right_slot.im[mirrored_sb] = right_mirrored_im as f32;
    }
}

/// 两路相同帧布局的 Core PCM 一起分析到 QMF 域。
///
/// 两路形状会在任一分析状态推进之前完整核对。PCM 到 `f64` 的提升精确，lane
/// 之间没有归约，也不使用 FMA；每路输出与依次调用 [`analyse_ac4_pcm`] 逐位相同。
pub(crate) fn analyse_ac4_pcm_pair(
    pcm: [&[f32]; ANALYSIS_CHANNEL_LANES],
    states: [&mut QmfAnalysisState; ANALYSIS_CHANNEL_LANES],
    slots: [&mut [QmfSlot]; ANALYSIS_CHANNEL_LANES],
) -> Result<(), QmfError> {
    let [left_pcm, right_pcm] = pcm;
    let [left_state, right_state] = states;
    let [left_slots, right_slots] = slots;

    if left_pcm.len() % SUBBANDS != 0 {
        return Err(QmfError::UnalignedInput {
            samples: left_pcm.len(),
        });
    }
    if right_pcm.len() % SUBBANDS != 0 {
        return Err(QmfError::UnalignedInput {
            samples: right_pcm.len(),
        });
    }
    let timeslots = left_pcm.len() / SUBBANDS;
    let right_timeslots = right_pcm.len() / SUBBANDS;
    if right_timeslots != timeslots {
        return Err(QmfError::SlotCountMismatch {
            expected: timeslots,
            provided: right_timeslots,
        });
    }
    for provided in [left_slots.len(), right_slots.len()] {
        if provided != timeslots {
            return Err(QmfError::SlotCountMismatch {
                expected: timeslots,
                provided,
            });
        }
    }

    let mut pair = AnalysisChannelPair::new();
    let pcm_chunks = left_pcm
        .as_chunks::<SUBBANDS>()
        .0
        .iter()
        .zip(right_pcm.as_chunks::<SUBBANDS>().0.iter());
    let slot_pairs = left_slots.iter_mut().zip(right_slots.iter_mut());
    for ((left_pcm, right_pcm), (left_slot, right_slot)) in pcm_chunks.zip(slot_pairs) {
        left_state
            .filt
            .copy_within(0..NUM_QMF_WIN_COEF - SUBBANDS, SUBBANDS);
        right_state
            .filt
            .copy_within(0..NUM_QMF_WIN_COEF - SUBBANDS, SUBBANDS);
        let state_heads = left_state.filt[..SUBBANDS]
            .iter_mut()
            .rev()
            .zip(right_state.filt[..SUBBANDS].iter_mut().rev());
        for ((left_state, right_state), (&left, &right)) in
            state_heads.zip(left_pcm.iter().zip(right_pcm))
        {
            *left_state = left;
            *right_state = right;
        }

        for n in 0..FOLDED {
            let (mut left_sum, mut right_sum) = (0.0f64, 0.0f64);
            for k in 0..5 {
                let index = n + k * FOLDED;
                let window = f64::from(QMF_WINDOW[index]);
                left_sum += f64::from(left_state.filt[index]) * window;
                right_sum += f64::from(right_state.filt[index]) * window;
            }
            pair.folded[n] = [left_sum, right_sum];
        }

        modulate_analysis_slot_pair(&pair, left_slot, right_slot);
    }

    for (left, right) in left_slots.iter_mut().zip(right_slots.iter_mut()) {
        for sb in 0..SUBBANDS {
            left.re[sb] /= AC4_PCM_INTERFACE_GAIN;
            left.im[sb] /= AC4_PCM_INTERFACE_GAIN;
            right.re[sb] /= AC4_PCM_INTERFACE_GAIN;
            right.im[sb] /= AC4_PCM_INTERFACE_GAIN;
        }
    }
    Ok(())
}

#[cfg_attr(feature = "qmf-split-profile", inline(never))]
#[cfg_attr(not(feature = "qmf-split-profile"), inline(always))]
fn advance_synthesis_state(state: &mut QmfSynthesisState) {
    state.filt.copy_within(0..SYNTH_STATE - FOLDED, FOLDED);
    #[cfg(feature = "qmf-split-profile")]
    core::hint::black_box(state.filt[FOLDED]);
}

#[cfg_attr(feature = "qmf-split-profile", inline(never))]
#[cfg_attr(not(feature = "qmf-split-profile"), inline(always))]
fn modulate_synthesis_slot(slot: &QmfSlot, state: &mut QmfSynthesisState) {
    // 复数子带经 N 矩阵回到 128 个实数样本；`1/64` 是规范写在矩阵里的。
    //
    // 成对处理 n 与 127-n。令 q=2sb+1、a=q(2n+1)，两行的指数分别为
    // a-256q ≡ a-256 与 -a，因此相位恰为 -exp(ja) 与 conj(exp(ja))。
    // 一对输出共享 re*cos 和 im*sin；两个累加器仍各自按 sb=0…63 的定义顺序
    // 前进，所以不会改变 f64 加法结合顺序或最终 f32 舍入。
    for n in 0..SUBBANDS {
        let mut low_sum = 0.0f64;
        let mut high_sum = 0.0f64;
        for sb in 0..SUBBANDS {
            let (cos, sin) = modulation(sb, 2 * n as isize + 1);
            let real = f64::from(slot.re[sb]) * cos;
            let imaginary = f64::from(slot.im[sb]) * sin;
            low_sum += imaginary - real;
            high_sum += real + imaginary;
        }
        state.filt[n] = (low_sum / SUBBANDS as f64) as f32;
        state.filt[FOLDED - 1 - n] = (high_sum / SUBBANDS as f64) as f32;
    }
}

/// ARM64 NEON 与 portable x86-64 SSE2 都能一次容纳两个 `f64` lane。
///
/// 这里不用平台 intrinsic：固定宽度、连续的两 lane 算术允许 LLVM 在稳定 Rust
/// 上做 SLP 向量化，同时继续满足 workspace 的 `unsafe_code = "forbid"` 与本 crate
/// 的 `no_std` 边界。
const SYNTHESIS_CHANNEL_LANES: usize = 2;

/// 一个时隙的两路复数子带，转成 `f64` 后按声道 lane 排列。
///
/// 原始 [`QmfSlot`] 是单声道内的实/虚 SoA；终端合成需要跨声道 SIMD，因此只在
/// 这个局部内核里转成 `[subband][channel]`。每个 f32 到 f64 的转换都是精确的，
/// 不引入新的舍入。16 字节对齐让两 lane 可以直接作为一个 128-bit 向量装载。
#[repr(C, align(16))]
struct SynthesisChannelPair {
    re: [[f64; SYNTHESIS_CHANNEL_LANES]; SUBBANDS],
    im: [[f64; SYNTHESIS_CHANNEL_LANES]; SUBBANDS],
}

impl SynthesisChannelPair {
    const fn new() -> Self {
        Self {
            re: [[0.0; SYNTHESIS_CHANNEL_LANES]; SUBBANDS],
            im: [[0.0; SYNTHESIS_CHANNEL_LANES]; SUBBANDS],
        }
    }

    fn load(&mut self, left: &QmfSlot, right: &QmfSlot) {
        for sb in 0..SUBBANDS {
            self.re[sb] = [f64::from(left.re[sb]), f64::from(right.re[sb])];
            self.im[sb] = [f64::from(left.im[sb]), f64::from(right.im[sb])];
        }
    }
}

/// 两路声道并排执行合成调制。
///
/// lane 之间没有归约：每一路仍严格按 `sb=0..63` 前进，乘法、`imaginary-real`、
/// `real+imaginary` 与累加顺序均与 [`modulate_synthesis_slot`] 相同。禁止 FMA 的
/// 数值契约因此不变，区别只有相同相位只查一次、两路独立标量可由编译器装入一个
/// SIMD 向量。
#[inline(always)]
fn modulate_synthesis_slot_pair(
    left: &QmfSlot,
    right: &QmfSlot,
    left_state: &mut QmfSynthesisState,
    right_state: &mut QmfSynthesisState,
    pair: &mut SynthesisChannelPair,
) {
    pair.load(left, right);
    for n in 0..SUBBANDS {
        let (mut left_low, mut left_high) = (0.0f64, 0.0f64);
        let (mut right_low, mut right_high) = (0.0f64, 0.0f64);
        for sb in 0..SUBBANDS {
            let (cos, sin) = modulation(sb, 2 * n as isize + 1);
            let [left_re, right_re] = pair.re[sb];
            let [left_im, right_im] = pair.im[sb];

            let left_real = left_re * cos;
            let left_imaginary = left_im * sin;
            left_low += left_imaginary - left_real;
            left_high += left_real + left_imaginary;

            let right_real = right_re * cos;
            let right_imaginary = right_im * sin;
            right_low += right_imaginary - right_real;
            right_high += right_real + right_imaginary;
        }
        left_state.filt[n] = (left_low / SUBBANDS as f64) as f32;
        left_state.filt[FOLDED - 1 - n] = (left_high / SUBBANDS as f64) as f32;
        right_state.filt[n] = (right_low / SUBBANDS as f64) as f32;
        right_state.filt[FOLDED - 1 - n] = (right_high / SUBBANDS as f64) as f32;
    }
}

#[cfg_attr(feature = "qmf-split-profile", inline(never))]
#[cfg_attr(not(feature = "qmf-split-profile"), inline(always))]
fn accumulate_synthesis_polyphase(
    state: &QmfSynthesisState,
    windowed: &mut [f64; NUM_QMF_WIN_COEF],
    pcm_slot: &mut [f32],
) {
    // 抽取 g、加窗、按 64 相位求和。
    for n in 0..5 {
        for sb in 0..SUBBANDS {
            let low = FOLDED * n + sb;
            let high = FOLDED * n + SUBBANDS + sb;
            windowed[low] = f64::from(state.filt[2 * FOLDED * n + sb]) * f64::from(QMF_WINDOW[low]);
            windowed[high] = f64::from(state.filt[2 * FOLDED * n + 3 * SUBBANDS + sb])
                * f64::from(QMF_WINDOW[high]);
        }
    }
    for (sb, sample) in pcm_slot.iter_mut().enumerate() {
        let mut sum = 0.0f64;
        for n in 0..10 {
            sum += windowed[SUBBANDS * n + sb];
        }
        *sample = sum as f32;
    }
}

/// 已完成形状预检的两声道 AC-4 QMF 合成。
fn synthesise_ac4_pcm_pair(
    slots: [&[QmfSlot]; SYNTHESIS_CHANNEL_LANES],
    states: [&mut QmfSynthesisState; SYNTHESIS_CHANNEL_LANES],
    pcm: [&mut [f32]; SYNTHESIS_CHANNEL_LANES],
) {
    let [left_slots, right_slots] = slots;
    let [left_state, right_state] = states;
    let [left_pcm, right_pcm] = pcm;
    let mut pair = SynthesisChannelPair::new();
    let mut windowed = [0.0f64; NUM_QMF_WIN_COEF];

    let slot_pairs = left_slots.iter().zip(right_slots);
    let pcm_pairs = left_pcm
        .as_chunks_mut::<SUBBANDS>()
        .0
        .iter_mut()
        .zip(right_pcm.as_chunks_mut::<SUBBANDS>().0.iter_mut());
    for ((left, right), (left_pcm_slot, right_pcm_slot)) in slot_pairs.zip(pcm_pairs) {
        advance_synthesis_state(left_state);
        advance_synthesis_state(right_state);
        modulate_synthesis_slot_pair(left, right, left_state, right_state, &mut pair);
        accumulate_synthesis_polyphase(left_state, &mut windowed, left_pcm_slot);
        accumulate_synthesis_polyphase(right_state, &mut windowed, right_pcm_slot);
    }

    for sample in left_pcm.iter_mut().chain(right_pcm) {
        *sample *= AC4_PCM_INTERFACE_GAIN;
    }
}

/// `Pseudocode 66` 的合成滤波。
///
/// # Errors
///
/// 长度不成立时返回 [`QmfError`]，且不改写状态。
#[cfg_attr(feature = "qmf-split-profile", inline(never))]
pub fn synthesise(
    slots: &[QmfSlot],
    state: &mut QmfSynthesisState,
    pcm: &mut [f32],
) -> Result<(), QmfError> {
    if !pcm.len().is_multiple_of(SUBBANDS) {
        return Err(QmfError::UnalignedInput { samples: pcm.len() });
    }
    let timeslots = pcm.len() / SUBBANDS;
    if slots.len() != timeslots {
        return Err(QmfError::SlotCountMismatch {
            expected: timeslots,
            provided: slots.len(),
        });
    }

    let mut windowed = [0.0f64; NUM_QMF_WIN_COEF];
    for (slot, pcm_slot) in slots
        .iter()
        .zip(pcm.as_chunks_mut::<SUBBANDS>().0.iter_mut())
    {
        advance_synthesis_state(state);
        modulate_synthesis_slot(slot, state);
        accumulate_synthesis_polyphase(state, &mut windowed, pcm_slot);
    }
    Ok(())
}

/// 把本解码器重建出的 Core PCM 分析到规范 QMF 量级。
///
/// 本解码器的终端 Core PCM 按 `TS103190-1:v1.4.1:5.5.3` 的块切换示例保留
/// 2 倍表示；若把它不经换算送入 A-SPX/A-JOC，低带直通能量会多 6 dB，而
/// `Pseudocode 82` 传输来的绝对高带目标不随之改变。这里先执行字面的
/// [`analyse`]，再按 ADR-0006 撤销该表示因子。独立使用滤波器组时仍应直接调用
/// [`analyse`]；本换算是对规范内部量级歧义的裁决，不是 `5.5.3` 明文定义的接口规则。
///
/// # Errors
///
/// 与 [`analyse`] 相同。错误返回时不改写状态或输出。
pub fn analyse_ac4_pcm(
    pcm: &[f32],
    state: &mut QmfAnalysisState,
    slots: &mut [QmfSlot],
) -> Result<(), QmfError> {
    analyse(pcm, state, slots)?;
    for slot in slots {
        for value in slot.re.iter_mut().chain(slot.im.iter_mut()) {
            *value /= AC4_PCM_INTERFACE_GAIN;
        }
    }
    Ok(())
}

/// 把终端 QMF 输出合成为本解码器的 Core PCM 呈现量级。
///
/// 这是 [`analyse_ac4_pcm`] 的另一侧：所有 A-SPX/A-JOC QMF 域工具完成后才补回
/// 终端 Core PCM 的 2 倍表示因子。独立使用滤波器组时仍应直接调用
/// [`synthesise`]。证据等级与推翻条件见 ADR-0006。
///
/// # Errors
///
/// 与 [`synthesise`] 相同。错误返回时不改写状态或输出。
pub fn synthesise_ac4_pcm(
    slots: &[QmfSlot],
    state: &mut QmfSynthesisState,
    pcm: &mut [f32],
) -> Result<(), QmfError> {
    synthesise(slots, state, pcm)?;
    for sample in pcm {
        *sample *= AC4_PCM_INTERFACE_GAIN;
    }
    Ok(())
}

/// 在同一条 QMF 时间轴上批量合成多路 AC-4 PCM。
///
/// 相邻两路使用跨声道 AoSoA 内核；奇数路批次的末路回退到
/// [`synthesise_ac4_pcm`]。输入、状态与输出必须逐路对应，输入可以比当前帧更长
/// （例如固定上限的 [`crate::element_drive::QmfChannelFrame`]），但输出必须恰好是
/// `timeslots * 64` 个样本。
///
/// 所有形状会在推进任何状态之前核对。浮点路径保持每路原有的 f64 累加顺序，不
/// 使用 FMA，也不在声道间做归约。
pub(crate) fn synthesise_ac4_pcm_channels<I, O>(
    slots: &[I],
    timeslots: usize,
    states: &mut [QmfSynthesisState],
    pcm: &mut [O],
) -> Result<(), QmfChannelsError>
where
    I: AsRef<[QmfSlot]>,
    O: AsMut<[f32]>,
{
    let channels = slots.len();
    if states.len() != channels || pcm.len() != channels {
        return Err(QmfChannelsError::ChannelCountMismatch {
            inputs: channels,
            states: states.len(),
            outputs: pcm.len(),
        });
    }
    let samples = timeslots
        .checked_mul(SUBBANDS)
        .ok_or(QmfChannelsError::SampleCountOverflow { timeslots })?;
    for (channel, input) in slots.iter().enumerate() {
        let provided = input.as_ref().len();
        if provided < timeslots {
            return Err(QmfChannelsError::InputTooShort {
                channel,
                needed: timeslots,
                provided,
            });
        }
    }
    for (channel, output) in pcm.iter_mut().enumerate() {
        let provided = output.as_mut().len();
        if provided != samples {
            return Err(QmfChannelsError::OutputLengthMismatch {
                channel,
                expected: samples,
                provided,
            });
        }
    }

    let paired_channels = channels / SYNTHESIS_CHANNEL_LANES * SYNTHESIS_CHANNEL_LANES;
    let mut channel = 0;
    while channel < paired_channels {
        let Some(input_pair) = slots.get(channel..channel + SYNTHESIS_CHANNEL_LANES) else {
            return Err(QmfChannelsError::ChannelCountMismatch {
                inputs: channels,
                states: states.len(),
                outputs: pcm.len(),
            });
        };
        let [left_input, right_input] = input_pair else {
            return Err(QmfChannelsError::ChannelCountMismatch {
                inputs: channels,
                states: states.len(),
                outputs: pcm.len(),
            });
        };
        let Some(state_pair) = states.get_mut(channel..channel + SYNTHESIS_CHANNEL_LANES) else {
            return Err(QmfChannelsError::ChannelCountMismatch {
                inputs: channels,
                states: states.len(),
                outputs: pcm.len(),
            });
        };
        let [left_state, right_state] = state_pair else {
            return Err(QmfChannelsError::ChannelCountMismatch {
                inputs: channels,
                states: states.len(),
                outputs: pcm.len(),
            });
        };
        let Some(output_pair) = pcm.get_mut(channel..channel + SYNTHESIS_CHANNEL_LANES) else {
            return Err(QmfChannelsError::ChannelCountMismatch {
                inputs: channels,
                states: states.len(),
                outputs: pcm.len(),
            });
        };
        let [left_output, right_output] = output_pair else {
            return Err(QmfChannelsError::ChannelCountMismatch {
                inputs: channels,
                states: states.len(),
                outputs: pcm.len(),
            });
        };

        synthesise_ac4_pcm_pair(
            [
                &left_input.as_ref()[..timeslots],
                &right_input.as_ref()[..timeslots],
            ],
            [left_state, right_state],
            [left_output.as_mut(), right_output.as_mut()],
        );
        channel += SYNTHESIS_CHANNEL_LANES;
    }

    if channel < channels {
        let input = &slots[channel].as_ref()[..timeslots];
        let output = pcm[channel].as_mut();
        synthesise_ac4_pcm(input, &mut states[channel], output)
            .map_err(|source| QmfChannelsError::ScalarTail { channel, source })?;
    }
    Ok(())
}

/// 分析调制的相位 `exp(j·(π/128)·(sb+0.5)·(2n−1))`。
fn analysis_phase(sb: usize, n: usize) -> (f64, f64) {
    modulation(sb, 2 * (n as isize) - 1)
}

/// 合成调制的相位 `exp(j·(π/128)·(sb+0.5)·(2n−255))`。
#[cfg(test)]
fn synthesis_phase(sb: usize, n: usize) -> (f64, f64) {
    modulation(sb, 2 * (n as isize) - 255)
}

/// `exp(j·(π/128)·(sb+0.5)·m)` 的实部与虚部，`m` 为奇数。
///
/// `(π/128)·(sb+0.5)·m = 2π·(2sb+1)·m / 512`，因此查一张 512 点的单位圆表即
/// 可，运行期不做三角求值。`(2sb+1)·m` 恒为奇数，所以只用得到奇数下标；表仍
/// 按整圈存，见 `build.rs` 的说明。
fn modulation(sb: usize, m: isize) -> (f64, f64) {
    let index = ((2 * sb as isize + 1) * m).rem_euclid(MODULATION_POINTS as isize) as usize;
    let [cos, sin] = generated_modulation::QMF_MODULATION[index];
    (f64::from(cos), f64::from(sin))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::format;
    use std::time::{Duration, Instant};
    use std::vec;
    use std::vec::Vec;

    /// 原型窗的两条结构判据在运行期再核一遍。
    ///
    /// 构建期已经核过同样两条，这里重复是为了让**默认拿到生成结果的读者**也能
    /// 在测试里看到它们：镜像只在 128 的倍数处反号，每个相位的平方和为 1。
    #[test]
    fn the_prototype_window_is_mirrored_and_power_complementary() {
        assert_eq!(QMF_WINDOW.len(), NUM_QMF_WIN_COEF);
        assert_eq!(QMF_WINDOW[0], 0.0);
        for n in 1..NUM_QMF_WIN_COEF {
            let mirrored = QMF_WINDOW[NUM_QMF_WIN_COEF - n];
            let expected = if n % 128 == 0 { -mirrored } else { mirrored };
            assert_eq!(QMF_WINDOW[n], expected, "QWIN[{n}] 的镜像关系");
        }

        let mut worst = 0.0f64;
        for phase in 0..SUBBANDS {
            let sum: f64 = (0..NUM_QMF_WIN_COEF / SUBBANDS)
                .map(|k| {
                    let value = f64::from(QMF_WINDOW[SUBBANDS * k + phase]);
                    value * value
                })
                .sum();
            worst = worst.max((sum - 1.0).abs());
        }
        assert!(worst <= 1.0e-6, "多相功率互补最大偏差 {worst:e}");
    }

    /// 分析接合成的总延迟，单位为样本。
    ///
    /// 不是 `640 − 64 = 576`：`Pseudocode 65` 把一块的第一个样本喂到 `filt[63]`
    /// （块内最旧），而两族调制的指数 `2n−1` 与 `2n−255` 又各带半个样本的偏移。
    /// 64 种块内输入相位的冲激峰值都恰好落在「输入位置 + 577」，主瓣对 1 的
    /// 最大偏差为 `2.4×10⁻⁷`，见
    /// [`all_input_phases_have_symmetric_finite_polyphase_responses`]。
    const ROUND_TRIP_DELAY: usize = 577;

    fn deterministic_signal(count: usize) -> Vec<f32> {
        let mut state = 0x3c6e_f372_fe94_f82b_u64;
        (0..count)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 40) as i32 - 8_388_608) as f32 / 8_388_608.0
            })
            .collect()
    }

    /// `Pseudocode 65` 分析调制的逐子带定义式，作为镜像成对实现的测试 oracle。
    fn modulate_analysis_slot_direct(folded: &[f64; FOLDED], slot: &mut QmfSlot) {
        for sb in 0..SUBBANDS {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (n, &value) in folded.iter().enumerate() {
                let (cos, sin) = analysis_phase(sb, n);
                re += value * cos;
                im += value * sin;
            }
            slot.re[sb] = re as f32;
            slot.im[sb] = im as f32;
        }
    }

    /// 拆分前 `Pseudocode 65` 的字面实现，只用于锁定完整分析状态与输出。
    fn analyse_direct_reference(pcm: &[f32], state: &mut QmfAnalysisState, slots: &mut [QmfSlot]) {
        assert_eq!(pcm.len(), slots.len() * SUBBANDS);
        let mut folded = [0.0f64; FOLDED];
        for (pcm, slot) in pcm.as_chunks::<SUBBANDS>().0.iter().zip(slots) {
            state
                .filt
                .copy_within(0..NUM_QMF_WIN_COEF - SUBBANDS, SUBBANDS);
            for sb in 0..SUBBANDS {
                state.filt[SUBBANDS - 1 - sb] = pcm[sb];
            }
            for (n, cell) in folded.iter_mut().enumerate() {
                let mut sum = 0.0f64;
                for k in 0..5 {
                    let index = n + k * FOLDED;
                    sum += f64::from(state.filt[index]) * f64::from(QMF_WINDOW[index]);
                }
                *cell = sum;
            }
            modulate_analysis_slot_direct(&folded, slot);
        }
    }

    fn deterministic_folded(count: usize) -> Vec<[f64; FOLDED]> {
        let values = deterministic_signal(count * FOLDED);
        values
            .as_chunks::<FOLDED>()
            .0
            .iter()
            .map(|chunk| core::array::from_fn(|index| f64::from(chunk[index])))
            .collect()
    }

    fn finite_edge_folded() -> Vec<[f64; FOLDED]> {
        let values = [
            0.0f32,
            -0.0f32,
            f32::from_bits(1),
            -f32::from_bits(1),
            f32::MIN_POSITIVE,
            -f32::MIN_POSITIVE,
            1.0,
            -1.0,
            f32::MAX,
            -f32::MAX,
        ];
        (0..values.len())
            .map(|offset| {
                core::array::from_fn(|n| f64::from(values[(3 * n + offset) % values.len()]))
            })
            .collect()
    }

    #[test]
    fn analysis_phase_subbands_form_exact_swapped_pairs() {
        for sb in 0..SUBBANDS / 2 {
            let mirrored_sb = SUBBANDS - 1 - sb;
            for n in 0..FOLDED {
                let (cos, sin) = analysis_phase(sb, n);
                let mirrored = analysis_phase(mirrored_sb, n);
                let expected = if n % 2 == 0 { (-sin, -cos) } else { (sin, cos) };
                assert_eq!(
                    mirrored.0.to_bits(),
                    expected.0.to_bits(),
                    "子带 {sb}/{mirrored_sb}、折叠项 {n} 的实相位"
                );
                assert_eq!(
                    mirrored.1.to_bits(),
                    expected.1.to_bits(),
                    "子带 {sb}/{mirrored_sb}、折叠项 {n} 的虚相位"
                );
            }
        }
    }

    #[test]
    fn paired_analysis_modulation_is_bit_exact_to_the_definition() {
        let mut inputs = deterministic_folded(19);
        inputs.extend(finite_edge_folded());
        for (input_index, folded) in inputs.iter().enumerate() {
            let mut direct = QmfSlot::zero();
            let mut paired = QmfSlot::zero();
            modulate_analysis_slot_direct(folded, &mut direct);
            modulate_analysis_slot(folded, &mut paired);
            assert_qmf_slots_bit_exact(
                &[paired],
                &[direct],
                &format!("paired analysis input {input_index}"),
            );
        }
    }

    #[test]
    fn paired_scalar_analysis_matches_the_definition_across_calls() {
        const TIMESLOTS: usize = 19;
        let signal = deterministic_signal(TIMESLOTS * SUBBANDS);
        let mut paired_state = QmfAnalysisState::new();
        let mut direct_state = QmfAnalysisState::new();

        for (call, range) in [0..7, 7..TIMESLOTS].into_iter().enumerate() {
            let sample_range = range.start * SUBBANDS..range.end * SUBBANDS;
            let mut paired = vec![QmfSlot::zero(); range.len()];
            let mut direct = vec![QmfSlot::zero(); range.len()];
            analyse(
                &signal[sample_range.clone()],
                &mut paired_state,
                &mut paired,
            )
            .expect("成对分析应成功");
            analyse_direct_reference(&signal[sample_range], &mut direct_state, &mut direct);
            assert_qmf_slots_bit_exact(&paired, &direct, &format!("调用 {call}"));
            assert_analysis_state_bit_exact(&paired_state, &direct_state, &format!("调用 {call}"));
        }
    }

    /// 拆分前 `Pseudocode 66` 的字面实现，仅用于锁定符号级归因改写的逐位等价性。
    fn synthesise_monolithic_reference(
        slots: &[QmfSlot],
        state: &mut QmfSynthesisState,
        pcm: &mut [f32],
    ) {
        assert_eq!(pcm.len(), slots.len() * SUBBANDS);
        let mut windowed = [0.0f64; NUM_QMF_WIN_COEF];
        for (ts, slot) in slots.iter().enumerate() {
            state.filt.copy_within(0..SYNTH_STATE - FOLDED, FOLDED);
            for n in 0..FOLDED {
                let mut sum = 0.0f64;
                for sb in 0..SUBBANDS {
                    let (cos, sin) = synthesis_phase(sb, n);
                    sum += f64::from(slot.re[sb]) * cos - f64::from(slot.im[sb]) * sin;
                }
                state.filt[n] = (sum / SUBBANDS as f64) as f32;
            }
            for n in 0..5 {
                for sb in 0..SUBBANDS {
                    let low = FOLDED * n + sb;
                    let high = FOLDED * n + SUBBANDS + sb;
                    windowed[low] =
                        f64::from(state.filt[2 * FOLDED * n + sb]) * f64::from(QMF_WINDOW[low]);
                    windowed[high] = f64::from(state.filt[2 * FOLDED * n + 3 * SUBBANDS + sb])
                        * f64::from(QMF_WINDOW[high]);
                }
            }
            for sb in 0..SUBBANDS {
                let mut sum = 0.0f64;
                for n in 0..10 {
                    sum += windowed[SUBBANDS * n + sb];
                }
                pcm[ts * SUBBANDS + sb] = sum as f32;
            }
        }
    }

    fn deterministic_slots(count: usize) -> Vec<QmfSlot> {
        let values = deterministic_signal(count * 2 * SUBBANDS);
        let mut slots = vec![QmfSlot::zero(); count];
        for (index, slot) in slots.iter_mut().enumerate() {
            let start = index * 2 * SUBBANDS;
            slot.re.copy_from_slice(&values[start..start + SUBBANDS]);
            slot.im
                .copy_from_slice(&values[start + SUBBANDS..start + 2 * SUBBANDS]);
        }
        slots
    }

    /// `Pseudocode 66` 合成调制的逐行定义式，作为候选实现的测试 oracle。
    fn modulate_synthesis_slot_direct(slot: &QmfSlot, state: &mut QmfSynthesisState) {
        for n in 0..FOLDED {
            let mut sum = 0.0f64;
            for sb in 0..SUBBANDS {
                let (cos, sin) = synthesis_phase(sb, n);
                sum += f64::from(slot.re[sb]) * cos - f64::from(slot.im[sb]) * sin;
            }
            state.filt[n] = (sum / SUBBANDS as f64) as f32;
        }
    }

    fn finite_edge_slots() -> Vec<QmfSlot> {
        let values = [
            0.0f32,
            -0.0f32,
            f32::from_bits(1),
            -f32::from_bits(1),
            f32::MIN_POSITIVE,
            -f32::MIN_POSITIVE,
            1.0,
            -1.0,
            f32::MAX,
            -f32::MAX,
        ];
        (0..values.len())
            .map(|offset| {
                let mut slot = QmfSlot::zero();
                for sb in 0..SUBBANDS {
                    slot.re[sb] = values[(sb + offset) % values.len()];
                    slot.im[sb] = values[(3 * sb + offset + 1) % values.len()];
                }
                slot
            })
            .collect()
    }

    fn assert_synthesis_head_bit_exact(
        actual: &QmfSynthesisState,
        expected: &QmfSynthesisState,
        context: &str,
    ) {
        for (index, (&actual, &expected)) in actual.filt[..FOLDED]
            .iter()
            .zip(&expected.filt[..FOLDED])
            .enumerate()
        {
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "{context} 的调制输出 {index} 必须逐位相同"
            );
        }
    }

    #[test]
    fn synthesis_phase_rows_form_exact_negated_conjugate_pairs() {
        for n in 0..SUBBANDS {
            for sb in 0..SUBBANDS {
                let root = modulation(sb, 2 * n as isize + 1);
                let low = synthesis_phase(sb, n);
                let high = synthesis_phase(sb, FOLDED - 1 - n);
                assert_eq!(low.0.to_bits(), (-root.0).to_bits());
                assert_eq!(low.1.to_bits(), (-root.1).to_bits());
                assert_eq!(high.0.to_bits(), root.0.to_bits());
                assert_eq!(high.1.to_bits(), (-root.1).to_bits());
            }
        }
    }

    #[test]
    fn paired_modulation_is_bit_exact_to_the_definition() {
        let mut slots = deterministic_slots(19);
        slots.extend(finite_edge_slots());
        for (slot_index, slot) in slots.iter().enumerate() {
            let mut direct = QmfSynthesisState::new();
            let mut paired = QmfSynthesisState::new();
            modulate_synthesis_slot_direct(slot, &mut direct);
            modulate_synthesis_slot(slot, &mut paired);
            assert_synthesis_head_bit_exact(&paired, &direct, &format!("paired slot {slot_index}"));
        }
    }

    fn distinct_channel_pcm(channels: usize, samples: usize) -> Vec<Vec<f32>> {
        let mut out = Vec::with_capacity(channels);
        for channel in 0..channels {
            let mut signal = deterministic_signal(samples);
            if samples != 0 {
                signal.rotate_left((17 * channel) % samples);
            }
            for (index, sample) in signal.iter_mut().enumerate() {
                if (index + 3 * channel) % 5 == 0 {
                    *sample = -*sample;
                }
            }
            out.push(signal);
        }
        out
    }

    fn assert_analysis_state_bit_exact(
        actual: &QmfAnalysisState,
        expected: &QmfAnalysisState,
        context: &str,
    ) {
        for (index, (&actual, &expected)) in actual.filt.iter().zip(&expected.filt).enumerate() {
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "{context} 的分析状态 {index} 必须逐位相同"
            );
        }
    }

    fn assert_qmf_slots_bit_exact(actual: &[QmfSlot], expected: &[QmfSlot], context: &str) {
        assert_eq!(actual.len(), expected.len(), "{context} 的时隙数必须相同");
        for (timeslot, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            for sb in 0..SUBBANDS {
                assert_eq!(
                    actual.re[sb].to_bits(),
                    expected.re[sb].to_bits(),
                    "{context} 时隙 {timeslot} 子带 {sb} 的实部必须逐位相同"
                );
                assert_eq!(
                    actual.im[sb].to_bits(),
                    expected.im[sb].to_bits(),
                    "{context} 时隙 {timeslot} 子带 {sb} 的虚部必须逐位相同"
                );
            }
        }
    }

    #[test]
    fn channel_pair_analysis_matches_scalar_across_calls() {
        const TIMESLOTS: usize = 19;
        let signals = distinct_channel_pcm(2, TIMESLOTS * SUBBANDS);
        let mut pair_states = [QmfAnalysisState::new(), QmfAnalysisState::new()];
        let mut scalar_states = [QmfAnalysisState::new(), QmfAnalysisState::new()];

        for (call, range) in [0..7, 7..TIMESLOTS].into_iter().enumerate() {
            let sample_range = range.start * SUBBANDS..range.end * SUBBANDS;
            let mut pair_slots = [
                vec![QmfSlot::zero(); range.len()],
                vec![QmfSlot::zero(); range.len()],
            ];
            let mut scalar_slots = [
                vec![QmfSlot::zero(); range.len()],
                vec![QmfSlot::zero(); range.len()],
            ];
            let [left_pair_state, right_pair_state] = &mut pair_states;
            let [left_pair_slots, right_pair_slots] = &mut pair_slots;
            analyse_ac4_pcm_pair(
                [
                    &signals[0][sample_range.clone()],
                    &signals[1][sample_range.clone()],
                ],
                [left_pair_state, right_pair_state],
                [left_pair_slots, right_pair_slots],
            )
            .expect("双声道分析应成功");

            for channel in 0..2 {
                analyse_ac4_pcm(
                    &signals[channel][sample_range.clone()],
                    &mut scalar_states[channel],
                    &mut scalar_slots[channel],
                )
                .expect("标量分析应成功");
                assert_qmf_slots_bit_exact(
                    &pair_slots[channel],
                    &scalar_slots[channel],
                    &format!("调用 {call} 声道 {channel}"),
                );
                assert_analysis_state_bit_exact(
                    &pair_states[channel],
                    &scalar_states[channel],
                    &format!("调用 {call} 声道 {channel}"),
                );
            }
        }
    }

    #[test]
    fn channel_pair_analysis_shape_errors_are_transactional() {
        let signals = distinct_channel_pcm(2, SUBBANDS);
        let mut states = [QmfAnalysisState::new(), QmfAnalysisState::new()];
        let sentinel = QmfSlot {
            re: [7.0; SUBBANDS],
            im: [9.0; SUBBANDS],
        };
        let mut outputs = [vec![sentinel], vec![sentinel]];
        let [left_state, right_state] = &mut states;
        let [left_output, right_output] = &mut outputs;
        let error = analyse_ac4_pcm_pair(
            [&signals[0], &signals[1][..SUBBANDS - 1]],
            [left_state, right_state],
            [left_output, right_output],
        )
        .expect_err("未对齐的第二路必须失败");
        assert_eq!(
            error,
            QmfError::UnalignedInput {
                samples: SUBBANDS - 1,
            }
        );
        assert_eq!(outputs, [vec![sentinel], vec![sentinel]]);
        for (channel, state) in states.iter().enumerate() {
            assert_analysis_state_bit_exact(
                state,
                &QmfAnalysisState::new(),
                &format!("失败后的声道 {channel}"),
            );
        }
    }

    fn distinct_channel_slots(channels: usize, timeslots: usize) -> Vec<Vec<QmfSlot>> {
        let mut out = Vec::with_capacity(channels);
        for channel in 0..channels {
            let mut frame = deterministic_slots(timeslots);
            for (timeslot, slot) in frame.iter_mut().enumerate() {
                slot.re.rotate_left((channel + timeslot) % SUBBANDS);
                slot.im.rotate_right((3 * channel + timeslot) % SUBBANDS);
                if channel % 2 != 0 {
                    for value in slot.re.iter_mut().step_by(3) {
                        *value = -*value;
                    }
                }
            }
            out.push(frame);
        }
        out
    }

    fn assert_synthesis_state_bit_exact(
        actual: &QmfSynthesisState,
        expected: &QmfSynthesisState,
        context: &str,
    ) {
        for (index, (&actual, &expected)) in actual.filt.iter().zip(&expected.filt).enumerate() {
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "{context} 的状态 {index} 必须逐位相同"
            );
        }
    }

    #[test]
    fn channel_pair_synthesis_matches_scalar_across_calls_and_tail() {
        const CHANNELS: usize = 5;
        const TIMESLOTS: usize = 19;
        let frames = distinct_channel_slots(CHANNELS, TIMESLOTS);
        let mut batch_states = (0..CHANNELS)
            .map(|_| QmfSynthesisState::new())
            .collect::<Vec<_>>();
        let mut scalar_states = (0..CHANNELS)
            .map(|_| QmfSynthesisState::new())
            .collect::<Vec<_>>();

        for (call, range) in [0..7, 7..TIMESLOTS].into_iter().enumerate() {
            let inputs = frames
                .iter()
                .map(|frame| &frame[range.clone()])
                .collect::<Vec<_>>();
            let samples = range.len() * SUBBANDS;
            let mut batch_pcm = vec![vec![0.0f32; samples]; CHANNELS];
            let mut scalar_pcm = vec![vec![0.0f32; samples]; CHANNELS];

            synthesise_ac4_pcm_channels(&inputs, range.len(), &mut batch_states, &mut batch_pcm)
                .expect("批量合成应成功");
            for channel in 0..CHANNELS {
                synthesise_ac4_pcm(
                    inputs[channel],
                    &mut scalar_states[channel],
                    &mut scalar_pcm[channel],
                )
                .expect("标量合成应成功");
                for (sample, (&batch, &scalar)) in batch_pcm[channel]
                    .iter()
                    .zip(&scalar_pcm[channel])
                    .enumerate()
                {
                    assert_eq!(
                        batch.to_bits(),
                        scalar.to_bits(),
                        "调用 {call} 声道 {channel} 样本 {sample} 必须逐位相同"
                    );
                }
                assert_synthesis_state_bit_exact(
                    &batch_states[channel],
                    &scalar_states[channel],
                    &format!("调用 {call} 声道 {channel}"),
                );
            }
        }
    }

    #[test]
    fn channel_batch_shape_errors_do_not_advance_any_state() {
        let inputs = distinct_channel_slots(2, 1);
        let mut states = vec![QmfSynthesisState::new(), QmfSynthesisState::new()];
        let mut outputs = vec![vec![7.0f32; SUBBANDS], vec![9.0f32; SUBBANDS - 1]];
        let error = synthesise_ac4_pcm_channels(&inputs, 1, &mut states, &mut outputs)
            .expect_err("第二路输出过短必须失败");
        assert_eq!(
            error,
            QmfChannelsError::OutputLengthMismatch {
                channel: 1,
                expected: SUBBANDS,
                provided: SUBBANDS - 1,
            }
        );
        assert_eq!(outputs[0], [7.0; SUBBANDS]);
        assert_eq!(outputs[1], [9.0; SUBBANDS - 1]);
        for (channel, state) in states.iter().enumerate() {
            assert_synthesis_state_bit_exact(
                state,
                &QmfSynthesisState::new(),
                &format!("失败后的声道 {channel}"),
            );
        }
    }

    fn benchmark_modulation(
        label: &str,
        iterations: usize,
        mut run: impl FnMut(&QmfSlot, &mut QmfSynthesisState),
    ) -> Duration {
        let slots = deterministic_slots(19);
        let mut state = QmfSynthesisState::new();
        let started = Instant::now();
        for iteration in 0..iterations {
            let slot = std::hint::black_box(&slots[iteration % slots.len()]);
            run(slot, std::hint::black_box(&mut state));
            std::hint::black_box(&state.filt[..FOLDED]);
        }
        let elapsed = started.elapsed();
        std::eprintln!(
            "{label}: {:.3} ns/slot",
            elapsed.as_nanos() as f64 / iterations as f64
        );
        elapsed
    }

    /// 手动运行：
    /// `cargo test -p macindecode-ac4-decode --release --features audio-decode qmf_paired_modulation_benchmark -- --ignored --nocapture`
    #[test]
    #[ignore = "手动运行的 QMF 成对调制微基准"]
    fn qmf_paired_modulation_benchmark() {
        const ITERATIONS: usize = 20_000;
        let mut direct = Vec::with_capacity(5);
        let mut paired = Vec::with_capacity(5);
        for round in 0..5 {
            if round % 2 == 0 {
                direct.push(benchmark_modulation(
                    "direct",
                    ITERATIONS,
                    modulate_synthesis_slot_direct,
                ));
                paired.push(benchmark_modulation(
                    "paired",
                    ITERATIONS,
                    modulate_synthesis_slot,
                ));
            } else {
                paired.push(benchmark_modulation(
                    "paired",
                    ITERATIONS,
                    modulate_synthesis_slot,
                ));
                direct.push(benchmark_modulation(
                    "direct",
                    ITERATIONS,
                    modulate_synthesis_slot_direct,
                ));
            }
        }
        direct.sort_unstable();
        paired.sort_unstable();

        std::eprintln!(
            "median speedup: paired {:.3}x",
            direct[2].as_secs_f64() / paired[2].as_secs_f64(),
        );
    }

    fn benchmark_scalar_analysis(signals: &[Vec<f32>], iterations: usize) -> Duration {
        let mut states = (0..signals.len())
            .map(|_| QmfAnalysisState::new())
            .collect::<Vec<_>>();
        let mut slots = vec![vec![QmfSlot::zero(); signals[0].len() / SUBBANDS]; signals.len()];
        let started = Instant::now();
        for _ in 0..iterations {
            for channel in 0..signals.len() {
                analyse_ac4_pcm(
                    std::hint::black_box(&signals[channel]),
                    std::hint::black_box(&mut states[channel]),
                    std::hint::black_box(&mut slots[channel]),
                )
                .expect("标量分析应成功");
            }
        }
        std::hint::black_box((states, slots));
        started.elapsed()
    }

    fn benchmark_channel_pair_analysis(signals: &[Vec<f32>], iterations: usize) -> Duration {
        let mut states = (0..signals.len())
            .map(|_| QmfAnalysisState::new())
            .collect::<Vec<_>>();
        let mut slots = vec![vec![QmfSlot::zero(); signals[0].len() / SUBBANDS]; signals.len()];
        let started = Instant::now();
        for _ in 0..iterations {
            for ((input_pair, state_pair), output_pair) in signals
                .as_chunks::<ANALYSIS_CHANNEL_LANES>()
                .0
                .iter()
                .zip(
                    states
                        .as_chunks_mut::<ANALYSIS_CHANNEL_LANES>()
                        .0
                        .iter_mut(),
                )
                .zip(slots.as_chunks_mut::<ANALYSIS_CHANNEL_LANES>().0.iter_mut())
            {
                let [left_input, right_input] = input_pair;
                let [left_state, right_state] = state_pair;
                let [left_output, right_output] = output_pair;
                analyse_ac4_pcm_pair(
                    [
                        std::hint::black_box(left_input),
                        std::hint::black_box(right_input),
                    ],
                    [
                        std::hint::black_box(left_state),
                        std::hint::black_box(right_state),
                    ],
                    [
                        std::hint::black_box(left_output),
                        std::hint::black_box(right_output),
                    ],
                )
                .expect("双声道分析应成功");
            }
        }
        std::hint::black_box((states, slots));
        started.elapsed()
    }

    /// 手动运行：
    /// `cargo test -p macindecode-ac4-decode --release --features audio-decode qmf_channel_pair_analysis_benchmark -- --ignored --nocapture`
    #[test]
    #[ignore = "手动运行的 20 声道 QMF 分析微基准"]
    fn qmf_channel_pair_analysis_benchmark() {
        const CHANNELS: usize = 20;
        const TIMESLOTS: usize = 19;
        const ITERATIONS: usize = 10;
        let signals = distinct_channel_pcm(CHANNELS, TIMESLOTS * SUBBANDS);
        let mut scalar = Vec::with_capacity(5);
        let mut paired = Vec::with_capacity(5);
        for round in 0..5 {
            if round % 2 == 0 {
                scalar.push(benchmark_scalar_analysis(&signals, ITERATIONS));
                paired.push(benchmark_channel_pair_analysis(&signals, ITERATIONS));
            } else {
                paired.push(benchmark_channel_pair_analysis(&signals, ITERATIONS));
                scalar.push(benchmark_scalar_analysis(&signals, ITERATIONS));
            }
        }
        scalar.sort_unstable();
        paired.sort_unstable();
        std::eprintln!(
            "scalar {:.3} ms/frame, channel-pair {:.3} ms/frame, speedup {:.3}x",
            scalar[2].as_secs_f64() * 1_000.0 / ITERATIONS as f64,
            paired[2].as_secs_f64() * 1_000.0 / ITERATIONS as f64,
            scalar[2].as_secs_f64() / paired[2].as_secs_f64(),
        );
    }

    fn benchmark_scalar_channels(frames: &[Vec<QmfSlot>], iterations: usize) -> Duration {
        let mut states = (0..frames.len())
            .map(|_| QmfSynthesisState::new())
            .collect::<Vec<_>>();
        let mut pcm = vec![vec![0.0f32; frames[0].len() * SUBBANDS]; frames.len()];
        let started = Instant::now();
        for _ in 0..iterations {
            for channel in 0..frames.len() {
                synthesise_ac4_pcm(
                    std::hint::black_box(&frames[channel]),
                    std::hint::black_box(&mut states[channel]),
                    std::hint::black_box(&mut pcm[channel]),
                )
                .expect("标量合成应成功");
            }
        }
        std::hint::black_box((states, pcm));
        started.elapsed()
    }

    fn benchmark_channel_pairs(frames: &[Vec<QmfSlot>], iterations: usize) -> Duration {
        let mut states = (0..frames.len())
            .map(|_| QmfSynthesisState::new())
            .collect::<Vec<_>>();
        let mut pcm = vec![vec![0.0f32; frames[0].len() * SUBBANDS]; frames.len()];
        let started = Instant::now();
        for _ in 0..iterations {
            synthesise_ac4_pcm_channels(
                std::hint::black_box(frames),
                frames[0].len(),
                std::hint::black_box(&mut states),
                std::hint::black_box(&mut pcm),
            )
            .expect("批量合成应成功");
        }
        std::hint::black_box((states, pcm));
        started.elapsed()
    }

    /// 手动运行：
    /// `cargo test -p macindecode-ac4-decode --release --features audio-decode qmf_channel_pair_benchmark -- --ignored --nocapture`
    #[test]
    #[ignore = "手动运行的 20 声道 QMF 批量微基准"]
    fn qmf_channel_pair_benchmark() {
        const CHANNELS: usize = 20;
        const TIMESLOTS: usize = 19;
        const ITERATIONS: usize = 20;
        let frames = distinct_channel_slots(CHANNELS, TIMESLOTS);
        let mut scalar = Vec::with_capacity(5);
        let mut paired = Vec::with_capacity(5);
        for round in 0..5 {
            if round % 2 == 0 {
                scalar.push(benchmark_scalar_channels(&frames, ITERATIONS));
                paired.push(benchmark_channel_pairs(&frames, ITERATIONS));
            } else {
                paired.push(benchmark_channel_pairs(&frames, ITERATIONS));
                scalar.push(benchmark_scalar_channels(&frames, ITERATIONS));
            }
        }
        scalar.sort_unstable();
        paired.sort_unstable();
        std::eprintln!(
            "scalar {:.3} ms/frame, channel-pair {:.3} ms/frame, speedup {:.3}x",
            scalar[2].as_secs_f64() * 1_000.0 / ITERATIONS as f64,
            paired[2].as_secs_f64() * 1_000.0 / ITERATIONS as f64,
            scalar[2].as_secs_f64() / paired[2].as_secs_f64(),
        );
    }

    #[test]
    fn split_synthesis_phases_are_bit_exact_to_monolithic_reference() {
        let slots = deterministic_slots(19);
        let mut split_state = QmfSynthesisState::new();
        let mut reference_state = QmfSynthesisState::new();

        for (call_index, frame) in [&slots[..7], &slots[7..]].into_iter().enumerate() {
            let mut split = vec![0.0f32; frame.len() * SUBBANDS];
            let mut reference = vec![0.0f32; frame.len() * SUBBANDS];
            synthesise(frame, &mut split_state, &mut split).expect("拆分实现应成功");
            synthesise_monolithic_reference(frame, &mut reference_state, &mut reference);

            for (sample_index, (split, reference)) in split.iter().zip(&reference).enumerate() {
                assert_eq!(
                    split.to_bits(),
                    reference.to_bits(),
                    "调用 {call_index} 的 PCM 样本 {sample_index} 必须逐位相同"
                );
            }
            for (state_index, (split, reference)) in split_state
                .filt
                .iter()
                .zip(&reference_state.filt)
                .enumerate()
            {
                assert_eq!(
                    split.to_bits(),
                    reference.to_bits(),
                    "调用 {call_index} 的状态 {state_index} 必须逐位相同"
                );
            }
        }
    }

    /// 分析接合成必须重建原信号，只差固定延迟。
    ///
    /// 这是本模块最强的判据：它一次覆盖两条伪码的状态左移、窗下标、折叠与抽取
    /// 顺序、两族调制相位以及 `1/64` 归一化。任何一处出错都会让重建塌掉，而不是
    /// 差一个常数。
    ///
    /// `1e-3` 是这条固定宽带信号的回归预算，不是滤波器组的单一特征值。实测相对
    /// 偏差 `8.7154e-4`；Python 双精度参考实现与未经 f32 舍入的原型窗也得到同一
    /// 数字，说明它不是查表精度造成的，但该值仍依赖输入信号与测量窗口。
    #[test]
    fn analysis_then_synthesis_reconstructs_with_a_fixed_delay() {
        const SLOTS: usize = 40;
        const SAMPLES: usize = SLOTS * SUBBANDS;
        const DELAY: usize = ROUND_TRIP_DELAY;

        let signal = deterministic_signal(SAMPLES);
        let mut analysis = QmfAnalysisState::new();
        let mut synthesis = QmfSynthesisState::new();
        let mut slots = vec![QmfSlot::zero(); SLOTS];
        let mut out = vec![0.0f32; SAMPLES];

        analyse(&signal, &mut analysis, &mut slots).expect("分析应成功");
        synthesise(&slots, &mut synthesis, &mut out).expect("合成应成功");

        let mut worst = 0.0f64;
        let mut energy = 0.0f64;
        for index in DELAY..SAMPLES {
            let expected = f64::from(signal[index - DELAY]);
            let actual = f64::from(out[index]);
            worst = worst.max((actual - expected).abs());
            energy += expected * expected;
        }
        let rms = (energy / (SAMPLES - DELAY) as f64).sqrt();
        assert!(
            worst / rms <= 1.0e-3,
            "QMF 往返最大偏差 {worst:e}（信号 RMS {rms:e}）超出预算"
        );
    }

    /// AC-4 PCM 的 2 倍因子只跨 QMF 域边界撤销并补回；两条规范伪码本身不改。
    #[test]
    fn ac4_pcm_boundary_moves_the_interface_gain_outside_qmf() {
        const SLOTS: usize = 12;
        const SAMPLES: usize = SLOTS * SUBBANDS;

        let signal = deterministic_signal(SAMPLES);
        let mut raw_analysis = QmfAnalysisState::new();
        let mut ac4_analysis = QmfAnalysisState::new();
        let mut raw_slots = vec![QmfSlot::zero(); SLOTS];
        let mut ac4_slots = vec![QmfSlot::zero(); SLOTS];
        analyse(&signal, &mut raw_analysis, &mut raw_slots).expect("规范分析");
        analyse_ac4_pcm(&signal, &mut ac4_analysis, &mut ac4_slots).expect("AC-4 PCM 分析");
        assert_eq!(raw_analysis, ac4_analysis, "接口换算不得改变分析历史");
        for (raw, ac4) in raw_slots.iter().zip(&ac4_slots) {
            for (raw, ac4) in raw
                .re
                .iter()
                .chain(raw.im.iter())
                .zip(ac4.re.iter().chain(ac4.im.iter()))
            {
                assert_eq!(*ac4, *raw / AC4_PCM_INTERFACE_GAIN);
            }
        }

        let mut raw_synthesis = QmfSynthesisState::new();
        let mut ac4_synthesis = QmfSynthesisState::new();
        let mut raw_pcm = vec![0.0f32; SAMPLES];
        let mut ac4_pcm = vec![0.0f32; SAMPLES];
        synthesise(&raw_slots, &mut raw_synthesis, &mut raw_pcm).expect("规范合成");
        synthesise_ac4_pcm(&raw_slots, &mut ac4_synthesis, &mut ac4_pcm).expect("AC-4 PCM 合成");
        assert_eq!(raw_synthesis, ac4_synthesis, "接口换算不得改变合成历史");
        for (raw, ac4) in raw_pcm.iter().zip(ac4_pcm) {
            assert_eq!(ac4, raw * AC4_PCM_INTERFACE_GAIN);
        }
    }

    /// 64 种块内输入相位各有一条有限、对称的多相冲激响应。
    ///
    /// 临界抽取让往返级联以 64 个样本为周期变化，不能把 `signal[0]` 的响应外推成
    /// 单一 LTI 滤波器。对每个输入相位 `p`，结构性抽头都落在
    /// `p + 577 + 128m`（`|m| ≤ 4`）并关于 `p + 577` 逐位对称，但抽头值随 `p`
    /// 变化。全相位实测主瓣对 1 的最大偏差为 `2.4×10⁻⁷`，最大旁瓣
    /// `2.229×10⁻⁴`，离开 128 网格的 f32 噪声不超过 `1.409×10⁻⁸`；网格上
    /// `|m| ≥ 5` 的尾部精确为零。
    ///
    /// 这条判据锁定每个多相分支的延迟、旁瓣位置、对称、有限长度和实现噪声底，
    /// 不从分支对称推出全局线性相位或单一幅频响应。
    #[test]
    fn all_input_phases_have_symmetric_finite_polyphase_responses() {
        const SLOTS: usize = 40;
        const SAMPLES: usize = SLOTS * SUBBANDS;
        /// 调制周期，即两个子带跳。
        const HOP: usize = 2 * SUBBANDS;

        let mut worst_main_error = (0.0f64, 0usize);
        let mut worst_sidelobe = (0.0f64, 0usize, 0usize);
        let mut worst_off_grid = (0.0f64, 0usize, 0usize);

        for phase in 0..SUBBANDS {
            let mut signal = vec![0.0f32; SAMPLES];
            signal[phase] = 1.0;
            let mut analysis = QmfAnalysisState::new();
            let mut synthesis = QmfSynthesisState::new();
            let mut slots = vec![QmfSlot::zero(); SLOTS];
            let mut out = vec![0.0f32; SAMPLES];
            analyse(&signal, &mut analysis, &mut slots).expect("分析应成功");
            synthesise(&slots, &mut synthesis, &mut out).expect("合成应成功");

            let center = ROUND_TRIP_DELAY + phase;
            let peak = out
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.abs().total_cmp(&right.1.abs()))
                .expect("输出非空");
            assert_eq!(peak.0, center, "输入相位 {phase} 的延迟应为 577");

            let main_error = (f64::from(out[center]) - 1.0).abs();
            if main_error > worst_main_error.0 {
                worst_main_error = (main_error, phase);
            }

            for (index, value) in out.iter().enumerate() {
                let distance = index.abs_diff(center);
                if distance % HOP != 0 {
                    let magnitude = f64::from(value.abs());
                    if magnitude > worst_off_grid.0 {
                        worst_off_grid = (magnitude, phase, index);
                    }
                } else if distance > 4 * HOP {
                    assert_eq!(
                        *value, 0.0,
                        "输入相位 {phase}、相对中心 {distance} 处的网格尾部应为零"
                    );
                }
            }

            for offset in (HOP..=4 * HOP).step_by(HOP) {
                let left = out[center - offset];
                let right = out[center + offset];
                assert_eq!(
                    left.to_bits(),
                    right.to_bits(),
                    "输入相位 {phase}、偏移 ±{offset} 的多相抽头应逐位对称"
                );
                let magnitude = f64::from(left).abs();
                if magnitude > worst_sidelobe.0 {
                    worst_sidelobe = (magnitude, phase, offset);
                }
            }
        }

        assert!(
            worst_main_error.0 <= 1.0e-6,
            "输入相位 {} 的主瓣误差 {:e} 超出预算",
            worst_main_error.1,
            worst_main_error.0
        );
        assert!(
            worst_sidelobe.0 <= 2.5e-4,
            "输入相位 {}、偏移 {} 的旁瓣 {:e} 超出预算",
            worst_sidelobe.1,
            worst_sidelobe.2,
            worst_sidelobe.0
        );
        assert!(
            worst_off_grid.0 <= 2.0e-8,
            "输入相位 {}、输出 {} 的离网格噪声 {:e} 超出预算",
            worst_off_grid.1,
            worst_off_grid.2,
            worst_off_grid.0
        );
    }

    /// 静音进、静音出，且不在状态里留下残留。
    #[test]
    fn silence_stays_silent() {
        const SLOTS: usize = 12;
        let mut analysis = QmfAnalysisState::new();
        let mut synthesis = QmfSynthesisState::new();
        let mut slots = vec![QmfSlot::zero(); SLOTS];
        let mut out = vec![0.0f32; SLOTS * SUBBANDS];

        analyse(&vec![0.0f32; SLOTS * SUBBANDS], &mut analysis, &mut slots).expect("分析应成功");
        assert!(
            slots
                .iter()
                .all(|slot| slot.re.iter().chain(slot.im.iter()).all(|&v| v == 0.0)),
            "静音输入应给出静音子带"
        );
        synthesise(&slots, &mut synthesis, &mut out).expect("合成应成功");
        assert!(out.iter().all(|&value| value == 0.0), "静音子带应给出静音");
    }

    /// 非法长度一律拒绝，且不推进状态。
    #[test]
    fn invalid_lengths_are_rejected_without_touching_state() {
        let mut analysis = QmfAnalysisState::new();
        let mut synthesis = QmfSynthesisState::new();
        let mut slots = vec![QmfSlot::zero(); 2];

        assert_eq!(
            analyse(&vec![0.0f32; 100], &mut analysis, &mut slots),
            Err(QmfError::UnalignedInput { samples: 100 })
        );
        assert_eq!(
            analyse(&vec![0.0f32; 3 * SUBBANDS], &mut analysis, &mut slots),
            Err(QmfError::SlotCountMismatch {
                expected: 3,
                provided: 2
            })
        );
        assert!(
            analysis.filt.iter().all(|&value| value == 0.0),
            "拒绝不应写入分析状态"
        );

        let mut pcm = vec![0.0f32; 3 * SUBBANDS];
        assert_eq!(
            synthesise(&slots, &mut synthesis, &mut pcm),
            Err(QmfError::SlotCountMismatch {
                expected: 3,
                provided: 2
            })
        );
        assert!(
            synthesis.filt.iter().all(|&value| value == 0.0),
            "拒绝不应写入合成状态"
        );
    }

    /// 状态跨调用延续：分两次喂入与一次喂入等价。
    #[test]
    fn state_carries_across_calls() {
        const SLOTS: usize = 20;
        const SAMPLES: usize = SLOTS * SUBBANDS;
        let signal = deterministic_signal(SAMPLES);

        let once = {
            let mut state = QmfAnalysisState::new();
            let mut slots = vec![QmfSlot::zero(); SLOTS];
            analyse(&signal, &mut state, &mut slots).expect("分析应成功");
            slots
        };
        let split = {
            let mut state = QmfAnalysisState::new();
            let mut slots = vec![QmfSlot::zero(); SLOTS];
            let (head, tail) = slots.split_at_mut(SLOTS / 2);
            analyse(&signal[..SAMPLES / 2], &mut state, head).expect("前半应成功");
            analyse(&signal[SAMPLES / 2..], &mut state, tail).expect("后半应成功");
            slots
        };
        for (index, (a, b)) in once.iter().zip(split.iter()).enumerate() {
            assert_eq!(a, b, "时隙 {index} 应与一次喂入逐位相同");
        }
    }
}
