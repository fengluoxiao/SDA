//! A-JOC 去相关器与瞬态抑制。
//!
//! `TS103190-2:v1.3.1:5.7.3.5` 复用
//! `TS103190-1:v1.4.1:5.7.7.4` 的三组 all-pass IIR 与 15 带 transient
//! ducker，并规定 A-JOC 的七路循环为 D0、D2、D1、D0、D2、D1、D0。
//!
//! # Canonical 状态
//!
//! 表 198 的三个区域都满足 `delay + order = 14`。每个 `(decorrelator,
//! subband)` 因此只保存 14 个复数状态：前 `delay` 个是纯延迟，后 `order` 个
//! 是 direct-form II 的递归状态，不保存 `Pseudocode 111` 直写形式中彼此重复的
//! input/output 历史。递归状态与累加器使用 f64，QMF 边界仍是 f32；这既遵守
//! ADR-0002 的滤波累加策略，也让 canonical 输出收窄后与直写伪码逐位一致。
//!
//! # Ducker 时间轴
//!
//! `Pseudocode 112`–`114` 在每个 QMF 时隙运行一次。每个 decorrelator 独立
//! 保存 15 个参数带的 peak/smooth/diff，跨 AC-4 帧连续；能量始终取该路当前
//! **输入** QMF 的复数模平方，而不是 all-pass 输出。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "表 198 把全部下标锁在 64 子带、14 状态和 15 ducker 带内；\
              IIR 与能量伪码用显式下标和分步四则运算才能固定累加顺序"
)]

use crate::ajoc::bands::AjocBandMap;
use crate::aspx::qmf::{QmfSlot, SUBBANDS};
use crate::aspx::workspace::MAX_QMF_TIMESLOTS;
use crate::spec_tables::ajoc::{
    DECORRELATOR_CYCLE, DECORRELATOR_REGIONS as TABLE_198, TABLE_199, TABLE_200, TABLE_201,
};
use core::fmt;

/// 每个子带的 canonical all-pass 复数状态数。
pub const ALL_PASS_STATE_LEN: usize = 14;

/// transient ducker 使用的 A-CPL 参数频带数。
pub const DUCKER_BANDS: usize = 15;

const ALPHA: f32 = 0.765_928_3;
const ALPHA_SMOOTH: f32 = 0.25;
const ONE_MINUS_ALPHA_SMOOTH: f32 = 0.75;
const GAMMA: f32 = 1.5;
const EPSILON: f32 = 1.0e-9;

const _: () = {
    assert!(TABLE_198[0].2 as usize + TABLE_198[0].3 as usize == ALL_PASS_STATE_LEN);
    assert!(TABLE_198[1].2 as usize + TABLE_198[1].3 as usize == ALL_PASS_STATE_LEN);
    assert!(TABLE_198[2].2 as usize + TABLE_198[2].3 as usize == ALL_PASS_STATE_LEN);
};

/// 三个规范去相关器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorrelatorKind {
    /// D0。
    D0,
    /// D1。
    D1,
    /// D2。
    D2,
}

impl DecorrelatorKind {
    const fn column(self) -> usize {
        match self {
            Self::D0 => 0,
            Self::D1 => 1,
            Self::D2 => 2,
        }
    }
}

/// A-JOC 七路去相关器的规范循环。
pub const AJOC_DECORRELATOR_CYCLE: [DecorrelatorKind; 7] = build_decorrelator_cycle();

const fn build_decorrelator_cycle() -> [DecorrelatorKind; 7] {
    let mut cycle = [DecorrelatorKind::D0; 7];
    let mut index = 0usize;
    while index < DECORRELATOR_CYCLE.len() {
        cycle[index] = match DECORRELATOR_CYCLE[index] {
            0 => DecorrelatorKind::D0,
            1 => DecorrelatorKind::D1,
            2 => DecorrelatorKind::D2,
            _ => panic!("locally generated decorrelator loop contains an invalid index"),
        };
        index += 1;
    }
    cycle
}

/// A-JOC decorrelator 下标对应的 D0/D1/D2。
#[must_use]
pub fn kind_for_ajoc_index(index: usize) -> Option<DecorrelatorKind> {
    AJOC_DECORRELATOR_CYCLE.get(index).copied()
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Complex64 {
    re: f64,
    im: f64,
}

impl Complex64 {
    const ZERO: Self = Self { re: 0.0, im: 0.0 };

    const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    fn scale(self, coefficient: f64) -> Self {
        Self::new(self.re * coefficient, self.im * coefficient)
    }

    fn is_finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AllPassState {
    cells: [Complex64; ALL_PASS_STATE_LEN],
}

impl AllPassState {
    const fn new() -> Self {
        Self {
            cells: [Complex64::ZERO; ALL_PASS_STATE_LEN],
        }
    }

    fn clear(&mut self) {
        self.cells.fill(Complex64::ZERO);
    }

    fn is_finite(&self) -> bool {
        self.cells.iter().copied().all(Complex64::is_finite)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DuckerBandState {
    peak_decay: f32,
    smooth: f32,
    smooth_peak_diff: f32,
}

impl DuckerBandState {
    const ZERO: Self = Self {
        peak_decay: 0.0,
        smooth: 0.0,
        smooth_peak_diff: 0.0,
    };

    fn is_finite(self) -> bool {
        self.peak_decay.is_finite() && self.smooth.is_finite() && self.smooth_peak_diff.is_finite()
    }
}

/// 一路 decorrelator 的跨 AC-4 帧状态。
///
/// A-JOC 的每个并行 decorrelator 必须持有独立实例；共享此结构会把一路瞬态的
/// ducking 历史泄漏到另一条路。
#[derive(Debug, Clone, PartialEq)]
pub struct DecorrelatorState {
    filters: [AllPassState; SUBBANDS],
    ducker: [DuckerBandState; DUCKER_BANDS],
}

impl DecorrelatorState {
    /// 全零 IIR 与 ducker 历史。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            filters: [AllPassState::new(); SUBBANDS],
            ducker: [DuckerBandState::ZERO; DUCKER_BANDS],
        }
    }

    /// 丢弃该 decorrelator 的全部跨帧历史。
    pub fn reset(&mut self) {
        for filter in &mut self.filters {
            filter.clear();
        }
        self.ducker.fill(DuckerBandState::ZERO);
    }

    /// 所有递归与 ducker 状态是否有限。
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.filters.iter().all(AllPassState::is_finite)
            && self.ducker.iter().copied().all(DuckerBandState::is_finite)
    }
}

impl Default for DecorrelatorState {
    fn default() -> Self {
        Self::new()
    }
}

/// 去相关器或 transient ducker 失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorrelatorError {
    /// 帧级入口只接受实际 AC-4 帧可有的 `1..=32` 个 QMF 时隙。
    TimeslotCountOutOfRange { timeslots: usize, limit: usize },
    /// 输入与输出帧的时隙数不同。
    OutputLengthMismatch { input: usize, output: usize },
    /// 输入 QMF 含非有限复数分量。
    NonFiniteInput { subband: usize },
    /// 输入能量收窄为 f32 后非有限。
    NonFiniteEnergy { parameter_band: usize },
    /// ducker 历史或新 gain 非有限。
    NonFiniteDucker { parameter_band: usize },
    /// IIR 或 gain 应用后的输出非有限。
    NonFiniteOutput { subband: usize },
    /// 内建的 15 带映射缺失。
    MissingDuckerBandMap,
}

impl fmt::Display for DecorrelatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimeslotCountOutOfRange { timeslots, limit } => {
                write!(
                    f,
                    "A-JOC decorrelation timeslot count {timeslots} is outside 1..={limit}"
                )
            }
            Self::OutputLengthMismatch { input, output } => {
                write!(
                    f,
                    "A-JOC decorrelation input/output timeslot counts are {input}/{output}"
                )
            }
            Self::NonFiniteInput { subband } => {
                write!(
                    f,
                    "A-JOC decorrelation input subband {subband} is non-finite"
                )
            }
            Self::NonFiniteEnergy { parameter_band } => {
                write!(
                    f,
                    "Energy in A-JOC ducker parameter band {parameter_band} is non-finite"
                )
            }
            Self::NonFiniteDucker { parameter_band } => {
                write!(
                    f,
                    "State in A-JOC ducker parameter band {parameter_band} is non-finite"
                )
            }
            Self::NonFiniteOutput { subband } => {
                write!(
                    f,
                    "A-JOC decorrelation output subband {subband} is non-finite"
                )
            }
            Self::MissingDuckerBandMap => write!(f, "A-JOC lacks the built-in 15-band ducker map"),
        }
    }
}

impl core::error::Error for DecorrelatorError {}

/// 处理一个 QMF 时隙。
///
/// 能量从 `input` 计算；每个子带先经过表 198–201 的 all-pass，再乘当前时隙
/// 更新出的 15 带 gain。成功时完整覆写 `output`。输入非有限时会在任何状态
/// 改写前失败；递归过程中出现的数值失败可能已经推进部分 IIR 状态，帧级调用方
/// 若要求事务语义，应像对象重建入口一样在候选状态上执行。
///
/// # Errors
///
/// 输入、能量、ducker 或输出出现非有限值时返回 [`DecorrelatorError`]。
pub fn process_timeslot(
    kind: DecorrelatorKind,
    input: &QmfSlot,
    state: &mut DecorrelatorState,
    output: &mut QmfSlot,
) -> Result<(), DecorrelatorError> {
    for sb in 0..SUBBANDS {
        if !input.re[sb].is_finite() || !input.im[sb].is_finite() {
            return Err(DecorrelatorError::NonFiniteInput { subband: sb });
        }
    }

    let map = AjocBandMap::for_num_bands(DUCKER_BANDS as u8)
        .ok_or(DecorrelatorError::MissingDuckerBandMap)?;
    let energy = input_energy(input, &map)?;
    let (next_ducker, gains) = update_ducker(&state.ducker, &energy)?;
    let mut candidate = QmfSlot::zero();

    for sb in 0..SUBBANDS {
        let sample = Complex64::new(f64::from(input.re[sb]), f64::from(input.im[sb]));
        let filtered = filter_sample(kind, sb, sample, &mut state.filters[sb]);
        let parameter_band = usize::from(map.column()[sb]);
        let ducked = filtered.scale(f64::from(gains[parameter_band]));
        let re = ducked.re as f32;
        let im = ducked.im as f32;
        if !ducked.is_finite() || !re.is_finite() || !im.is_finite() {
            return Err(DecorrelatorError::NonFiniteOutput { subband: sb });
        }
        candidate.re[sb] = re;
        candidate.im[sb] = im;
    }

    state.ducker = next_ducker;
    *output = candidate;
    Ok(())
}

/// 连续处理一帧 `1..=32` 个 QMF 时隙。
///
/// 状态不会在帧边界重置，因此把连续输入拆成 24/30/32 时隙帧与逐时隙连续调用
/// 具有相同结果。
///
/// # Errors
///
/// 时隙数、输出长度或任一时隙数值无效时返回 [`DecorrelatorError`]。
pub fn process_frame(
    kind: DecorrelatorKind,
    input: &[QmfSlot],
    state: &mut DecorrelatorState,
    output: &mut [QmfSlot],
) -> Result<(), DecorrelatorError> {
    if input.is_empty() || input.len() > MAX_QMF_TIMESLOTS {
        return Err(DecorrelatorError::TimeslotCountOutOfRange {
            timeslots: input.len(),
            limit: MAX_QMF_TIMESLOTS,
        });
    }
    if output.len() != input.len() {
        return Err(DecorrelatorError::OutputLengthMismatch {
            input: input.len(),
            output: output.len(),
        });
    }
    for slot in input {
        for sb in 0..SUBBANDS {
            if !slot.re[sb].is_finite() || !slot.im[sb].is_finite() {
                return Err(DecorrelatorError::NonFiniteInput { subband: sb });
            }
        }
    }
    for (source, target) in input.iter().zip(output) {
        process_timeslot(kind, source, state, target)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct FilterSpec {
    region: usize,
    delay: usize,
    order: usize,
}

fn filter_spec(subband: usize) -> FilterSpec {
    for (region, &(first, last, delay, order)) in TABLE_198.iter().enumerate() {
        if (usize::from(first)..=usize::from(last)).contains(&subband) {
            return FilterSpec {
                region,
                delay: usize::from(delay),
                order: usize::from(order),
            };
        }
    }
    // 调用方只遍历 64 个 QMF 子带；保底值仍满足 14 状态约束。
    FilterSpec {
        region: 2,
        delay: 12,
        order: 2,
    }
}

fn coefficient(region: usize, index: usize, kind: DecorrelatorKind) -> f64 {
    match region {
        0 => TABLE_199[index][kind.column()],
        1 => TABLE_200[index][kind.column()],
        _ => TABLE_201[index][kind.column()],
    }
}

/// Delay + canonical direct-form II；每次只触碰表 198 指定的 14 个状态。
fn filter_sample(
    kind: DecorrelatorKind,
    subband: usize,
    input: Complex64,
    state: &mut AllPassState,
) -> Complex64 {
    let spec = filter_spec(subband);
    let delayed = state.cells[0];
    state.cells.copy_within(1..spec.delay, 0);
    state.cells[spec.delay - 1] = input;

    let mut w = delayed;
    for i in 1..=spec.order {
        let previous = state.cells[spec.delay + i - 1];
        let a = coefficient(spec.region, i, kind);
        w.re -= a * previous.re;
        w.im -= a * previous.im;
    }

    let mut filtered = w.scale(coefficient(spec.region, spec.order, kind));
    for i in 1..=spec.order {
        let previous = state.cells[spec.delay + i - 1];
        let b = coefficient(spec.region, spec.order - i, kind);
        filtered.re += b * previous.re;
        filtered.im += b * previous.im;
    }

    state
        .cells
        .copy_within(spec.delay..spec.delay + spec.order - 1, spec.delay + 1);
    state.cells[spec.delay] = w;
    filtered
}

fn input_energy(
    input: &QmfSlot,
    map: &AjocBandMap,
) -> Result<[f32; DUCKER_BANDS], DecorrelatorError> {
    let mut accumulated = [0.0f64; DUCKER_BANDS];
    for sb in 0..SUBBANDS {
        let re = f64::from(input.re[sb]);
        let im = f64::from(input.im[sb]);
        let magnitude_squared = re * re + im * im;
        let parameter_band = usize::from(map.column()[sb]);
        accumulated[parameter_band] += magnitude_squared;
    }

    let mut energy = [0.0f32; DUCKER_BANDS];
    for parameter_band in 0..DUCKER_BANDS {
        energy[parameter_band] = accumulated[parameter_band] as f32;
        if !energy[parameter_band].is_finite() {
            return Err(DecorrelatorError::NonFiniteEnergy { parameter_band });
        }
    }
    Ok(energy)
}

fn update_ducker(
    previous: &[DuckerBandState; DUCKER_BANDS],
    energy: &[f32; DUCKER_BANDS],
) -> Result<([DuckerBandState; DUCKER_BANDS], [f32; DUCKER_BANDS]), DecorrelatorError> {
    let mut next = [DuckerBandState::ZERO; DUCKER_BANDS];
    let mut gain = [1.0f32; DUCKER_BANDS];
    for parameter_band in 0..DUCKER_BANDS {
        let decayed_peak = ALPHA * previous[parameter_band].peak_decay;
        let peak_decay = if decayed_peak < energy[parameter_band] {
            energy[parameter_band]
        } else {
            decayed_peak
        };

        let mut smooth = ONE_MINUS_ALPHA_SMOOTH * previous[parameter_band].smooth;
        smooth += ALPHA_SMOOTH * energy[parameter_band];
        let mut smooth_peak_diff =
            ONE_MINUS_ALPHA_SMOOTH * previous[parameter_band].smooth_peak_diff;
        smooth_peak_diff += ALPHA_SMOOTH * (peak_decay - energy[parameter_band]);

        let scaled_diff = GAMMA * smooth_peak_diff;
        if scaled_diff > smooth {
            let denominator = GAMMA * (smooth_peak_diff + EPSILON);
            gain[parameter_band] = smooth / denominator;
        }
        next[parameter_band] = DuckerBandState {
            peak_decay,
            smooth,
            smooth_peak_diff,
        };
        if !next[parameter_band].is_finite() || !gain[parameter_band].is_finite() {
            return Err(DecorrelatorError::NonFiniteDucker { parameter_band });
        }
    }
    Ok((next, gain))
}

#[cfg(test)]
#[expect(
    clippy::cast_precision_loss,
    reason = "测试信号下标小于 64，转换为 f32 精确"
)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec;
    use std::vec::Vec;

    #[derive(Clone, Copy)]
    struct DirectPseudocodeState {
        input: [Complex64; ALL_PASS_STATE_LEN],
        output: [Complex64; 7],
    }

    impl DirectPseudocodeState {
        const fn new() -> Self {
            Self {
                input: [Complex64::ZERO; ALL_PASS_STATE_LEN],
                output: [Complex64::ZERO; 7],
            }
        }

        /// Pseudocode 111 按原加法顺序直写，保留独立 input/output 历史。
        fn process(
            &mut self,
            kind: DecorrelatorKind,
            subband: usize,
            input: Complex64,
        ) -> Complex64 {
            let spec = filter_spec(subband);
            let mut output =
                self.input[spec.delay - 1].scale(coefficient(spec.region, spec.order, kind));
            for i in 1..=spec.order {
                let feed_forward = self.input[spec.delay + i - 1].scale(coefficient(
                    spec.region,
                    spec.order - i,
                    kind,
                ));
                let feedback = self.output[i - 1].scale(coefficient(spec.region, i, kind));
                let term =
                    Complex64::new(feed_forward.re - feedback.re, feed_forward.im - feedback.im);
                output.re += term.re;
                output.im += term.im;
            }
            self.input.copy_within(0..ALL_PASS_STATE_LEN - 1, 1);
            self.input[0] = input;
            self.output.copy_within(0..spec.order - 1, 1);
            self.output[0] = output;
            output
        }
    }

    fn sample(time: usize, subband: usize) -> Complex64 {
        let real_code = (time * 29 + subband * 11) % 97;
        let imag_code = (time * 17 + subband * 23) % 89;
        let re = (real_code as f32 - 48.0) * 257.25;
        let im = (imag_code as f32 - 44.0) * -193.5;
        Complex64::new(f64::from(re), f64::from(im))
    }

    fn frame_slot(time: usize) -> QmfSlot {
        let mut slot = QmfSlot::zero();
        for sb in 0..SUBBANDS {
            let value = sample(time, sb);
            slot.re[sb] = value.re as f32;
            slot.im[sb] = value.im as f32;
        }
        slot
    }

    #[test]
    fn table_regions_and_ajoc_cycle_cover_every_boundary() {
        let mut expected_first = 0usize;
        for (region, &(first, last, delay, order)) in TABLE_198.iter().enumerate() {
            assert_eq!(usize::from(first), expected_first);
            assert!(first <= last);
            assert_eq!(usize::from(delay) + usize::from(order), ALL_PASS_STATE_LEN);
            for subband in usize::from(first)..=usize::from(last) {
                assert_eq!(filter_spec(subband).region, region);
            }
            expected_first = usize::from(last) + 1;
        }
        assert_eq!(expected_first, SUBBANDS);
        for index in 0..AJOC_DECORRELATOR_CYCLE.len() {
            assert_eq!(
                kind_for_ajoc_index(index),
                Some(AJOC_DECORRELATOR_CYCLE[index])
            );
        }
        assert_eq!(kind_for_ajoc_index(AJOC_DECORRELATOR_CYCLE.len()), None);
        assert_eq!(
            core::mem::size_of::<AllPassState>(),
            ALL_PASS_STATE_LEN * 16
        );
    }

    #[test]
    fn canonical_state_matches_direct_pseudocode_111_bit_for_bit() {
        for kind in [
            DecorrelatorKind::D0,
            DecorrelatorKind::D1,
            DecorrelatorKind::D2,
        ] {
            for &(first, last, _, _) in &TABLE_198 {
                for subband in [usize::from(first), usize::from(last)] {
                    let mut canonical = AllPassState::new();
                    let mut direct = DirectPseudocodeState::new();
                    for time in 0..192 {
                        let input = sample(time, subband);
                        let actual = filter_sample(kind, subband, input, &mut canonical);
                        let expected = direct.process(kind, subband, input);
                        assert_eq!(
                            (actual.re as f32).to_bits(),
                            (expected.re as f32).to_bits(),
                            "{kind:?} sb={subband} ts={time} real"
                        );
                        assert_eq!(
                            (actual.im as f32).to_bits(),
                            (expected.im as f32).to_bits(),
                            "{kind:?} sb={subband} ts={time} imag"
                        );
                    }
                    assert!(canonical.is_finite());
                }
            }
        }
    }

    #[test]
    fn split_frames_equal_continuous_timeslots_for_all_decorrelators() {
        let input: Vec<QmfSlot> = (0..54).map(frame_slot).collect();
        for kind in [
            DecorrelatorKind::D0,
            DecorrelatorKind::D1,
            DecorrelatorKind::D2,
        ] {
            let mut continuous_state = DecorrelatorState::new();
            let mut continuous = vec![QmfSlot::zero(); input.len()];
            for (source, target) in input.iter().zip(&mut continuous) {
                process_timeslot(kind, source, &mut continuous_state, target)
                    .expect("连续时隙应成功");
            }

            let mut split_state = DecorrelatorState::new();
            let mut split = vec![QmfSlot::zero(); input.len()];
            process_frame(kind, &input[..24], &mut split_state, &mut split[..24])
                .expect("24 时隙帧");
            process_frame(kind, &input[24..], &mut split_state, &mut split[24..])
                .expect("30 时隙帧");

            assert_eq!(split, continuous, "{kind:?} 拆帧改变输出");
            assert_eq!(split_state, continuous_state, "{kind:?} 拆帧改变状态");
            assert!(split_state.is_finite());
        }
    }

    #[test]
    fn ducker_tracks_complex_input_energy_in_all_15_bands() {
        let map = AjocBandMap::for_num_bands(15).expect("15 带映射必须存在");
        let mut input = QmfSlot::zero();
        let mut expected_energy = [0.0f32; DUCKER_BANDS];
        for parameter_band in 0..DUCKER_BANDS {
            let subband = map
                .column()
                .iter()
                .position(|&mapped| usize::from(mapped) == parameter_band)
                .expect("每个参数带至少覆盖一个 QMF 子带");
            let re = parameter_band as f32 + 1.0;
            let im = -(parameter_band as f32 + 2.0);
            input.re[subband] = re;
            input.im[subband] = im;
            expected_energy[parameter_band] = re * re + im * im;
        }

        let mut state = DecorrelatorState::new();
        let mut output = frame_slot(0);
        process_timeslot(DecorrelatorKind::D1, &input, &mut state, &mut output)
            .expect("首时隙应建立全部 ducker 带状态");
        assert_eq!(output, QmfSlot::zero(), "最短 delay 前不得出现输出");
        for (parameter_band, &energy) in expected_energy.iter().enumerate() {
            assert_eq!(
                state.ducker[parameter_band].peak_decay.to_bits(),
                energy.to_bits()
            );
            assert_eq!(
                state.ducker[parameter_band].smooth.to_bits(),
                (0.25 * energy).to_bits()
            );
            assert_eq!(state.ducker[parameter_band].smooth_peak_diff.to_bits(), 0);
        }

        process_timeslot(
            DecorrelatorKind::D1,
            &QmfSlot::zero(),
            &mut state,
            &mut output,
        )
        .expect("后一静音时隙应推进全部带的衰减状态");
        assert_eq!(output, QmfSlot::zero());
        for (parameter_band, &energy) in expected_energy.iter().enumerate() {
            let peak_decay = ALPHA * energy;
            assert_eq!(
                state.ducker[parameter_band].peak_decay.to_bits(),
                peak_decay.to_bits()
            );
            assert_eq!(
                state.ducker[parameter_band].smooth.to_bits(),
                (0.75 * (0.25 * energy)).to_bits()
            );
            assert_eq!(
                state.ducker[parameter_band].smooth_peak_diff.to_bits(),
                (0.25 * peak_decay).to_bits()
            );
        }
    }

    #[test]
    fn transient_ducker_attenuates_the_delayed_tail_and_is_independent() {
        let mut transient_state = DecorrelatorState::new();
        let mut silent_state = DecorrelatorState::new();
        let mut raw_filter = AllPassState::new();
        let mut input = QmfSlot::zero();
        input.re[0] = 1.0;
        let mut observed = 0.0f32;
        let mut raw = 0.0f32;

        for time in 0..8 {
            let source = if time == 0 { input } else { QmfSlot::zero() };
            let mut output = QmfSlot::zero();
            process_timeslot(
                DecorrelatorKind::D0,
                &source,
                &mut transient_state,
                &mut output,
            )
            .expect("瞬态序列应成功");
            let raw_sample = filter_sample(
                DecorrelatorKind::D0,
                0,
                Complex64::new(f64::from(source.re[0]), 0.0),
                &mut raw_filter,
            );
            if time == 7 {
                observed = output.re[0];
                raw = raw_sample.re as f32;
            }

            let mut silent_output = QmfSlot::zero();
            process_timeslot(
                DecorrelatorKind::D0,
                &QmfSlot::zero(),
                &mut silent_state,
                &mut silent_output,
            )
            .expect("独立静音路应成功");
            assert_eq!(silent_output, QmfSlot::zero());
        }

        assert_ne!(raw.to_bits(), 0, "夹具必须产生 delay 后的 all-pass 输出");
        assert!(observed.abs() < raw.abs(), "ducker 必须衰减混响尾部");
        assert_ne!(transient_state.ducker, silent_state.ducker);
        assert!(transient_state.is_finite());
        assert!(silent_state.is_finite());
    }

    #[test]
    fn silence_stays_zero_and_reset_matches_a_fresh_state() {
        let mut state = DecorrelatorState::new();
        let zero = QmfSlot::zero();
        for _ in 0..40 {
            let mut output = frame_slot(0);
            process_timeslot(DecorrelatorKind::D2, &zero, &mut state, &mut output)
                .expect("静音应成功");
            assert_eq!(output, zero);
        }
        assert!(state.is_finite());

        let mut discarded = QmfSlot::zero();
        process_timeslot(
            DecorrelatorKind::D2,
            &frame_slot(9),
            &mut state,
            &mut discarded,
        )
        .expect("非零状态准备");
        state.reset();
        let mut fresh = DecorrelatorState::new();
        let probe = frame_slot(10);
        let mut after_reset = QmfSlot::zero();
        let mut expected = QmfSlot::zero();
        process_timeslot(DecorrelatorKind::D2, &probe, &mut state, &mut after_reset)
            .expect("reset 后处理");
        process_timeslot(DecorrelatorKind::D2, &probe, &mut fresh, &mut expected)
            .expect("全新状态处理");
        assert_eq!(after_reset, expected);
        assert_eq!(state, fresh);
    }

    #[test]
    fn invalid_shapes_and_nonfinite_input_fail_before_state_changes() {
        let mut state = DecorrelatorState::new();
        let before = state.clone();
        assert_eq!(
            process_frame(DecorrelatorKind::D0, &[], &mut state, &mut []),
            Err(DecorrelatorError::TimeslotCountOutOfRange {
                timeslots: 0,
                limit: 32,
            })
        );
        let too_many = vec![QmfSlot::zero(); MAX_QMF_TIMESLOTS + 1];
        let mut too_many_output = too_many.clone();
        assert_eq!(
            process_frame(
                DecorrelatorKind::D0,
                &too_many,
                &mut state,
                &mut too_many_output,
            ),
            Err(DecorrelatorError::TimeslotCountOutOfRange {
                timeslots: 33,
                limit: 32,
            })
        );
        let input = [QmfSlot::zero(); 1];
        assert_eq!(
            process_frame(DecorrelatorKind::D0, &input, &mut state, &mut []),
            Err(DecorrelatorError::OutputLengthMismatch {
                input: 1,
                output: 0,
            })
        );
        let mut bad = QmfSlot::zero();
        bad.im[23] = f32::NAN;
        let mut output = frame_slot(0);
        assert_eq!(
            process_timeslot(DecorrelatorKind::D0, &bad, &mut state, &mut output),
            Err(DecorrelatorError::NonFiniteInput { subband: 23 })
        );
        assert_eq!(state, before);
        assert_eq!(output, frame_slot(0));

        let mut finite_overrange = QmfSlot::zero();
        finite_overrange.re[0] = f32::MAX;
        assert_eq!(
            process_timeslot(
                DecorrelatorKind::D0,
                &finite_overrange,
                &mut state,
                &mut output,
            ),
            Err(DecorrelatorError::NonFiniteEnergy { parameter_band: 0 })
        );
        assert_eq!(state, before);
        assert_eq!(output, frame_slot(0));
    }
}
