//! 子带音调噪声比调整数据（`TS103190-1:v1.4.1:5.7.6.4.1.3`）。
//!
//! 两步走的第一步：在 `Q_low` 的每个 QMF 子带信号内做线性预测，产出复数滤波
//! 系数 `alpha0`/`alpha1`（`Pseudocode 86`–`87`）；第二步的实际调整在
//! `5.7.6.4.1.4` 的 HF 信号创建里执行。本模块另外给出控制调整强度的 chirp
//! 因子（`Pseudocode 88`、表 195）。
//!
//! 子带信号是复数，因此协方差矩阵与滤波系数都是复数。协方差按 ADR-0002 第 2
//! 条在 f64 里累加，系数写回时收窄为 f32。
//!
//! # 时间轴的衔接
//!
//! `Pseudocode 86` 先把 `Q_low` 向前延长 `ts_offset_hfadj = 4` 个时隙，取的是
//! **上一区间 `Q_low_prev` 的 `[N−4, N)`**（`N = num_qmf_timeslots`），不是它的
//! 最后 4 项——`Q_low_prev` 末尾还有 `ts_offset_hfgen` 项。这个偏移恰好接得上：
//! `Q_low[0..ts_offset_hfgen]` 本身就是 `Q_low_prev[N..N+ts_offset_hfgen]`（见
//! [`super::lowband`] 的延迟线），于是
//!
//! ```text
//! Q_low_ext = Q_low_prev[N−4 .. N+ts_offset_hfgen] ++ Q_low[ts_offset_hfgen ..]
//! ```
//!
//! 在时间上连续无重叠。取错成「末尾 4 项」会让 `ts_offset_hfgen` 个时隙被算
//! 两遍，协方差随之偏移。
//!
//! `Q_low_ext` 不物化：`num_qmf_timeslots` 最大 32 时它是 42 个 `QmfSlot`，约
//! 21 KiB，对 `no_std` 的栈过大。改为在取样时直接做下标映射，代价是一次分支。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "下标由 64 个子带、表 192 的两档延迟与已校验的时隙数派生；\
              `Pseudocode 86` 的三段偏移用显式下标比迭代器更贴近原文"
)]

use crate::aspx::qmf::QmfSlot;
use crate::aspx::tables::{MAX_SBG_NOISE, NUM_QMF_SUBBANDS};

/// `Pseudocode 86` 的 `ts_offset_hfadj`，单位为 QMF 时隙。
///
/// 协方差的最远回看是 `ts − 2·i`，`i` 最大为 2，而 `ts` 从本常量起步——4 正是
/// 让 `ts − 4` 不越过 `Q_low_ext` 左端的最小值。
pub const TS_OFFSET_HFADJ: usize = 4;

/// 子带数，`5.7.3.2` 规定恒为 64。
const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;

/// `Pseudocode 87` 的 `EPSILON_INV`，即 `pow(2,-20)`。
///
/// 名字里的 INV 与取值方向相反，这是规范原文；它作为 `1/(1+EPSILON_INV)` 出现，
/// 把 `denom` 从 Cauchy–Schwarz 的等号情形推开一点点。写成两个整数相除让编译期
/// 折叠出精确值，`2^-20` 在 f64 上可精确表示。
const EPSILON_INV: f64 = 1.0 / 1_048_576.0;

/// `Pseudocode 87` 的稳定性上界：任一系数的模达到它，两个系数一起置零。
const MAX_COEFFICIENT_MAGNITUDE: f64 = 4.0;

/// 表 195 的 `tabNewChirp`。
///
/// **第一维是当前 `aspx_tna_mode`，第二维是 `aspx_tna_mode_prev`**，与表 195 的
/// 行列相反：`Pseudocode 88` 写的是
/// `tabNewChirp[aspx_tna_mode[sbg]][aspx_tna_mode_prev[sbg]]`，而表的行标是
/// prev、列标是当前。表本身不对称（`prev=None, cur=Moderate` 为 0,9，反过来是
/// 0,0），因此转置与否是可判定的，不是无关紧要的写法差异。
const NEW_CHIRP: [[f32; 4]; 4] = [
    // 当前 None：取表的 None 列
    [0.0, 0.6, 0.0, 0.0],
    // 当前 Light：取表的 Light 列
    [0.6, 0.75, 0.75, 0.75],
    // 当前 Moderate：取表的 Moderate 列
    [0.9, 0.9, 0.9, 0.9],
    // 当前 Heavy：取表的 Heavy 列
    [0.98, 0.98, 0.98, 0.98],
];

/// `Pseudocode 88` 的平滑：新值低于旧值时用 3/4，否则用 29/32。
///
/// 两组权重都是 2 的负幂之和，在 f32 上精确；和恒为 1。
const DECAY_WEIGHTS: (f32, f32) = (0.75, 0.25);
const RISE_WEIGHTS: (f32, f32) = (0.90625, 0.09375);

/// `Pseudocode 88` 的静默阈值 `0,015625`，即 `2^-6`。
const CHIRP_FLOOR: f32 = 0.015_625;

/// f64 复数。
///
/// 只实现协方差与解算需要的运算。`asf::imdct::ifft` 的同名类型是 IFFT 内核
/// 专用（只有加与乘），提升其可见性会把变换的内部约定散到 A-SPX 里。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    const ZERO: Self = Self { re: 0.0, im: 0.0 };

    const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// `self * cplx_conj(other)`，即协方差的累加项。
    fn mul_conj(self, other: Self) -> Self {
        Self::new(
            self.re * other.re + self.im * other.im,
            self.im * other.re - self.re * other.im,
        )
    }

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.re * other.re - self.im * other.im,
            self.re * other.im + self.im * other.re,
        )
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.re + other.re, self.im + other.im)
    }

    fn sub(self, other: Self) -> Self {
        Self::new(self.re - other.re, self.im - other.im)
    }

    const fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }

    /// `abs(z)^2`。`Pseudocode 87` 只用到模的平方，不必开方。
    fn norm_sqr(self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    /// 除以一个实数。`denom` 与 `cov[1][1]` 在 `Pseudocode 87` 里都是实数。
    ///
    /// 真除而不是乘以倒数：`a·(1/b)` 比 `a/b` 多一次舍入，而原文写的是 `/=`。
    fn div_real(self, divisor: f64) -> Self {
        Self::new(self.re / divisor, self.im / divisor)
    }

    fn is_finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}

/// 单个子带的协方差矩阵。
///
/// `Pseudocode 86` 的 `j` 从 1 起，因此第 0 列不存在——`Pseudocode 87` 用到的
/// 五个元素（`[0][1]`、`[0][2]`、`[1][1]`、`[1][2]`、`[2][2]`）都在 `i ∈ 0..3`
/// 与 `j ∈ 1..3` 之内。`[2][1]` 算了但没人用，保留是为了照抄原文的双重循环。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Covariance {
    /// `cov[i][j]`，第一维是 `i`，第二维是 `j − 1`。
    entries: [[Complex; 2]; 3],
}

impl Covariance {
    const fn get(&self, i: usize, j: usize) -> Complex {
        self.entries[i][j - 1]
    }
}

/// `Pseudocode 87` 的一对复数滤波系数。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Coefficient {
    /// 实部。
    pub re: f32,
    /// 虚部。
    pub im: f32,
}

impl Coefficient {
    /// 零系数。`Pseudocode 87` 的三处置零分支与 `Pseudocode 89` 的判据都用它。
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };

    fn from_complex(value: Complex) -> Self {
        Self {
            re: value.re as f32,
            im: value.im as f32,
        }
    }
}

/// 逐子带的线性预测系数。
#[derive(Debug)]
pub struct TnaFilters {
    alpha0: [Coefficient; SUBBANDS],
    alpha1: [Coefficient; SUBBANDS],
    subbands: u8,
}

impl TnaFilters {
    /// 建立空结果。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            alpha0: [Coefficient::ZERO; SUBBANDS],
            alpha1: [Coefficient::ZERO; SUBBANDS],
            subbands: 0,
        }
    }

    /// 第 `sb` 个子带的 `(alpha0, alpha1)`；超出已填充范围时为 `None`。
    #[must_use]
    pub fn coefficients(&self, sb: usize) -> Option<(Coefficient, Coefficient)> {
        if sb >= usize::from(self.subbands) {
            return None;
        }
        Some((self.alpha0[sb], self.alpha1[sb]))
    }

    /// 已填充的子带数，即 `sba`。
    #[must_use]
    pub const fn subbands(&self) -> u8 {
        self.subbands
    }

    /// 把前 `subbands` 个子带填成同一对系数，供 `hfgen` 的判据隔离单个抽头。
    #[cfg(test)]
    pub(crate) fn fill_for_test(&mut self, subbands: u8, alpha0: Coefficient, alpha1: Coefficient) {
        for sb in 0..usize::from(subbands) {
            self.alpha0[sb] = alpha0;
            self.alpha1[sb] = alpha1;
        }
        self.subbands = subbands;
    }
}

impl Default for TnaFilters {
    fn default() -> Self {
        Self::new()
    }
}

/// 跨 A-SPX 区间的时隙历史。
///
/// 保存上一区间 `Q_low_prev[N−4, N)` 的四个时隙。首个区间之前没有信号，全零即
/// 等价于前置静音。
#[derive(Debug, PartialEq)]
pub struct TnaDelay {
    tail: [QmfSlot; TS_OFFSET_HFADJ],
    /// 已写入的真实历史时隙数，不含前置静音。
    filled: u8,
}

impl TnaDelay {
    /// 建立空状态。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tail: [QmfSlot::zero(); TS_OFFSET_HFADJ],
            filled: 0,
        }
    }

    /// 已保存的历史时隙数；首个区间之前为 0。
    #[must_use]
    pub const fn history(&self) -> u8 {
        self.filled
    }

    /// 直接改写第 `ts` 个历史时隙，供 `hfgen` 的判据构造已知的 `Q_low_ext`。
    #[cfg(test)]
    pub(crate) fn tail_mut(&mut self, ts: usize) -> &mut QmfSlot {
        &mut self.tail[ts]
    }

    /// 把本区间的 `[N−4, N)` 存成下一区间的历史。
    ///
    /// **推进必须排在本区间的全部取样之后。** `Pseudocode 86` 的协方差与
    /// `Pseudocode 89` 的 HF 生成读的是同一个 `Q_low_ext`；若在两者之间推进，
    /// 后一个拿到的前四个时隙会变成本区间的尾部，整条时间轴错位一整个区间。
    /// 推进因此不藏在任何一个消费者内部。
    ///
    /// # Errors
    ///
    /// 见 [`TnaError`]。任一条不成立时都不改写状态。
    pub fn advance(&mut self, q_low: &[QmfSlot], num_qmf_timeslots: usize) -> Result<(), TnaError> {
        if num_qmf_timeslots < TS_OFFSET_HFADJ {
            return Err(TnaError::IntervalTooShort {
                timeslots: num_qmf_timeslots,
            });
        }
        if q_low.len() < num_qmf_timeslots {
            return Err(TnaError::LowBandTooShort {
                timeslots: num_qmf_timeslots,
                provided: q_low.len(),
            });
        }
        for ts in 0..TS_OFFSET_HFADJ {
            self.tail[ts] = q_low[num_qmf_timeslots - TS_OFFSET_HFADJ + ts];
        }
        self.filled = TS_OFFSET_HFADJ as u8;
        Ok(())
    }
}

impl Default for TnaDelay {
    fn default() -> Self {
        Self::new()
    }
}

/// 跨 A-SPX 区间的 chirp 状态。
///
/// `Pseudocode 88` 的 `prev_chirp_array` 与 `aspx_tna_mode_prev`，首个区间都取零。
#[derive(Debug, PartialEq)]
pub struct ChirpState {
    previous: [f32; MAX_SBG_NOISE as usize],
    modes: [u8; MAX_SBG_NOISE as usize],
}

impl ChirpState {
    /// 建立空状态：chirp 全零，mode 全 `None`。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            previous: [0.0; MAX_SBG_NOISE as usize],
            modes: [0; MAX_SBG_NOISE as usize],
        }
    }
}

impl Default for ChirpState {
    fn default() -> Self {
        Self::new()
    }
}

/// 音调噪声比调整数据无法计算的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TnaError {
    /// `ts_offset_hfgen` 不是表 192 规定的 3 或 6。
    UnsupportedOffset { offset: usize },
    /// `Q_low` 短于 `num_qmf_timeslots`，推不出 `ts_offset_hfgen`。
    LowBandTooShort { timeslots: usize, provided: usize },
    /// 区间短于 [`TS_OFFSET_HFADJ`]，取不出 `Q_low_prev[N−4, N)`。
    ///
    /// 表 189 的八档 `num_qmf_timeslots` 下限为 6，合法输入永远够长；这条只挡
    /// 住调用方切错了片。
    IntervalTooShort { timeslots: usize },
    /// `sba` 超出 64 个 QMF 子带。
    SubbandOutOfRange { sba: usize },
    /// 噪声子带组数超出表 126 的上限。
    NoiseGroupOutOfRange { groups: usize },
    /// 输出缓冲长度与噪声子带组数不一致。
    OutputLengthMismatch { expected: usize, provided: usize },
    /// `aspx_tna_mode` 超出表 131 的 2 位取值。
    ModeOutOfRange { group: usize, mode: u8 },
    /// 解算结果不是有限值。
    ///
    /// `Pseudocode 87` 的 `|alpha| ≥ 4` 置零规则挡得住溢出成 `±∞` 的系数，却挡
    /// 不住 `NaN`——`NaN >= 4` 为假，系数会原样流进 HF 生成并污染整条链。
    NonFiniteCoefficient { subband: usize },
}

/// `Q_low_ext` 的只读视图。
///
/// `Pseudocode 86` 与 `Pseudocode 89` 用的是同一条延长后的时间轴，因此视图公开
/// 出去：前 [`TS_OFFSET_HFADJ`] 个时隙来自上一区间的 `Q_low_prev[N−4, N)`，其余
/// 是本区间。
///
/// 视图**不推进状态**。`TnaDelay` 要等本区间的两处消费都取完样才能前进，见
/// [`TnaDelay::advance`]。
#[derive(Debug, Clone, Copy)]
pub struct ExtendedLowBand<'a> {
    delay: &'a TnaDelay,
    q_low: &'a [QmfSlot],
}

impl<'a> ExtendedLowBand<'a> {
    /// 由上一区间的延迟与本区间的 `Q_low` 组成视图。
    #[must_use]
    pub const fn new(delay: &'a TnaDelay, q_low: &'a [QmfSlot]) -> Self {
        Self { delay, q_low }
    }

    /// `num_ts_ext`，即 `num_qmf_timeslots + ts_offset_hfgen + ts_offset_hfadj`。
    #[must_use]
    pub const fn timeslots(&self) -> usize {
        self.q_low.len() + TS_OFFSET_HFADJ
    }

    /// 视图是否为空。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.q_low.is_empty()
    }

    /// `Q_low_ext[sb][ts]` 的实部与虚部；越界时为 `None`。
    #[must_use]
    pub fn sample(&self, sb: usize, ts: usize) -> Option<(f32, f32)> {
        if sb >= SUBBANDS {
            return None;
        }
        let slot = if ts < TS_OFFSET_HFADJ {
            self.delay.tail.get(ts)?
        } else {
            self.q_low.get(ts - TS_OFFSET_HFADJ)?
        };
        Some((slot.re[sb], slot.im[sb]))
    }

    fn complex(&self, sb: usize, ts: usize) -> Complex {
        let (re, im) = self.sample(sb, ts).unwrap_or((0.0, 0.0));
        Complex::new(f64::from(re), f64::from(im))
    }
}

/// `Pseudocode 86` 的协方差矩阵。
fn covariance(ext: ExtendedLowBand<'_>, sb: usize, num_ts_ext: usize) -> Covariance {
    let mut out = Covariance::default();
    for i in 0..3 {
        for j in 1..3 {
            let mut sum = Complex::ZERO;
            let mut ts = TS_OFFSET_HFADJ;
            while ts < num_ts_ext {
                let lhs = ext.complex(sb, ts - 2 * i);
                let rhs = ext.complex(sb, ts - 2 * j);
                sum = sum.add(lhs.mul_conj(rhs));
                ts += 2;
            }
            out.entries[i][j - 1] = sum;
        }
    }
    out
}

/// `Pseudocode 87` 的解算。
///
/// 规范原文把第二行写成 `abs(cov[1][2])`，漏了 `[sb]` 一维。同一段落里其余六处
/// 都带 `[sb]`，且 `cov` 在 `Pseudocode 86` 中就是三维的，因此这是排印脱漏，按
/// `cov[sb][1][2]` 实现。
fn solve(cov: &Covariance) -> (Complex, Complex) {
    let c11 = cov.get(1, 1);
    let c12 = cov.get(1, 2);
    let c22 = cov.get(2, 2);

    let denom = c22.mul(c11).sub(Complex::new(
        c12.norm_sqr() * (1.0 / (1.0 + EPSILON_INV)),
        0.0,
    ));

    // `cov[i][i]` 是 `Σ z·conj(z)`，每一项的虚部都精确为 0，因此 denom 是实数。
    // 规范的 `denom == 0` 是复数相等，这里等价于实部为零。
    let alpha1 = if denom == Complex::ZERO {
        Complex::ZERO
    } else {
        let numerator = cov.get(0, 1).mul(c12).sub(cov.get(0, 2).mul(c11));
        numerator.div_real(denom.re)
    };

    let alpha0 = if c11 == Complex::ZERO {
        Complex::ZERO
    } else {
        let numerator = Complex::ZERO.sub(cov.get(0, 1)).add(alpha1.mul(c12.conj()));
        numerator.div_real(c11.re)
    };

    (alpha0, alpha1)
}

/// 「若 alpha0 与 alpha1 的模有任一达到 4，两者一起置零。」
///
/// 比较用模的平方，省一次开方且不改变判定：两侧都非负时平方保序。溢出成 `±∞`
/// 的系数在这里一并被挡下——`inf` 的模平方仍是 `inf`，比较为真。
fn clamp_unstable(alpha0: Complex, alpha1: Complex) -> (Complex, Complex) {
    let limit = MAX_COEFFICIENT_MAGNITUDE * MAX_COEFFICIENT_MAGNITUDE;
    if alpha0.norm_sqr() >= limit || alpha1.norm_sqr() >= limit {
        return (Complex::ZERO, Complex::ZERO);
    }
    (alpha0, alpha1)
}

/// `Pseudocode 86`–`87` 的逐子带线性预测。
///
/// `ext` 是本区间的 `Q_low_ext`，其中 `Q_low` 长度为
/// `num_qmf_timeslots + ts_offset_hfgen`。本函数只读视图，不推进延迟状态；
/// 调用方应在预测与 HF 生成都取样完成后调用 [`TnaDelay::advance`]。
///
/// # Errors
///
/// 见 [`TnaError`]。任一条不成立时都不改写 `out`。
pub fn prediction_filters(
    ext: ExtendedLowBand<'_>,
    num_qmf_timeslots: usize,
    sba: u8,
    out: &mut TnaFilters,
) -> Result<(), TnaError> {
    let q_low_len = ext.timeslots() - TS_OFFSET_HFADJ;
    if num_qmf_timeslots < TS_OFFSET_HFADJ {
        return Err(TnaError::IntervalTooShort {
            timeslots: num_qmf_timeslots,
        });
    }
    if q_low_len < num_qmf_timeslots {
        return Err(TnaError::LowBandTooShort {
            timeslots: num_qmf_timeslots,
            provided: q_low_len,
        });
    }
    let offset = q_low_len - num_qmf_timeslots;
    if !matches!(offset, 3 | 6) {
        return Err(TnaError::UnsupportedOffset { offset });
    }
    let sba = usize::from(sba);
    if sba > SUBBANDS {
        return Err(TnaError::SubbandOutOfRange { sba });
    }

    let num_ts_ext = num_qmf_timeslots + offset + TS_OFFSET_HFADJ;
    let mut alpha0 = [Coefficient::ZERO; SUBBANDS];
    let mut alpha1 = [Coefficient::ZERO; SUBBANDS];
    for sb in 0..sba {
        let cov = covariance(ext, sb, num_ts_ext);
        let (mut a0, mut a1) = solve(&cov);
        if !a0.is_finite() || !a1.is_finite() {
            return Err(TnaError::NonFiniteCoefficient { subband: sb });
        }
        (a0, a1) = clamp_unstable(a0, a1);
        alpha0[sb] = Coefficient::from_complex(a0);
        alpha1[sb] = Coefficient::from_complex(a1);
    }

    out.alpha0 = alpha0;
    out.alpha1 = alpha1;
    out.subbands = sba as u8;
    Ok(())
}

/// 核对逐噪声子带组的 `aspx_tna_mode` 数量上限与取值范围。
///
/// # Errors
///
/// 数量超过表 126 上限，或任一模式超出表 131 的 2 位取值时返回 [`TnaError`]。
pub(crate) fn validate_chirp_modes(modes: &[u8]) -> Result<(), TnaError> {
    if modes.len() > MAX_SBG_NOISE as usize {
        return Err(TnaError::NoiseGroupOutOfRange {
            groups: modes.len(),
        });
    }
    for (group, &mode) in modes.iter().enumerate() {
        if usize::from(mode) >= NEW_CHIRP.len() {
            return Err(TnaError::ModeOutOfRange { group, mode });
        }
    }
    Ok(())
}

/// `Pseudocode 88` 与表 195 的 chirp 因子。
///
/// `modes` 是本区间逐噪声子带组的 `aspx_tna_mode`，`out` 与之等长。成功时
/// `state` 前进到本区间。
///
/// # Errors
///
/// 见 [`TnaError`]。任一条不成立时都不改写 `state` 与 `out`。
pub fn chirp_factors(
    modes: &[u8],
    state: &mut ChirpState,
    out: &mut [f32],
) -> Result<(), TnaError> {
    if modes.len() > MAX_SBG_NOISE as usize {
        return Err(TnaError::NoiseGroupOutOfRange {
            groups: modes.len(),
        });
    }
    if out.len() != modes.len() {
        return Err(TnaError::OutputLengthMismatch {
            expected: modes.len(),
            provided: out.len(),
        });
    }
    validate_chirp_modes(modes)?;

    for (group, &mode) in modes.iter().enumerate() {
        let previous_mode = state.modes[group];
        let previous = state.previous[group];
        let mut new_chirp = NEW_CHIRP[usize::from(mode)][usize::from(previous_mode)];
        let (fresh, stale) = if new_chirp < previous {
            DECAY_WEIGHTS
        } else {
            RISE_WEIGHTS
        };
        new_chirp = fresh * new_chirp + stale * previous;
        out[group] = if new_chirp < CHIRP_FLOOR {
            0.0
        } else {
            new_chirp
        };
    }

    // `prev_chirp_array` 是「上一区间计算出的 chirp factor」，而 chirp factor 就是
    // `chirp_arr`——静默阈值已经作用于它。因此存回的是截断后的输出，不是平滑的
    // 中间值：一旦落到阈值以下，下一区间的平滑要从 0 起步而不是从残值起步。
    for (group, &mode) in modes.iter().enumerate() {
        state.modes[group] = mode;
        state.previous[group] = out[group];
    }
    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "判据取的都是在 f32/f64 上精确可表示的值，容差会让它们变松"
)]
mod tests {
    use super::*;

    /// 表 189 的一档小区间，配表 192 的 `ts_offset_hfgen = 3`。
    const N: usize = 8;
    const HFGEN: usize = 3;
    /// `ts` 从 4 起步长 2 走到 `num_ts_ext = 15`：{4,6,8,10,12,14}。
    const TERMS: f64 = 6.0;

    /// 把逐时隙的复数序列铺进 `state.tail` 与 `q_low`，使
    /// `Q_low_ext[sb][t]` 恰为 `samples[t]`。
    fn spread(samples: &[(f32, f32)], sb: usize) -> (TnaDelay, [QmfSlot; N + HFGEN]) {
        assert_eq!(samples.len(), N + HFGEN + TS_OFFSET_HFADJ);
        let mut state = TnaDelay::new();
        for (ts, &(re, im)) in samples.iter().take(TS_OFFSET_HFADJ).enumerate() {
            state.tail[ts].re[sb] = re;
            state.tail[ts].im[sb] = im;
        }
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        for (k, &(re, im)) in samples.iter().skip(TS_OFFSET_HFADJ).enumerate() {
            q_low[k].re[sb] = re;
            q_low[k].im[sb] = im;
        }
        (state, q_low)
    }

    fn constant(value: (f32, f32)) -> [(f32, f32); N + HFGEN + TS_OFFSET_HFADJ] {
        [value; N + HFGEN + TS_OFFSET_HFADJ]
    }

    /// `z[t] = 1 + i·t`。
    ///
    /// 协方差元素带非零虚部，是 `i^t` 所不具备的：那条序列在偶数 `ts` 上让每个
    /// 元素都退化成实数，于是「取不取共轭」在结果上看不出来。解析值全是小整数，
    /// f32 与 f64 都精确。
    fn linear_ramp() -> [(f32, f32); N + HFGEN + TS_OFFSET_HFADJ] {
        let mut out = [(0.0, 0.0); N + HFGEN + TS_OFFSET_HFADJ];
        for (t, slot) in out.iter_mut().enumerate() {
            *slot = (1.0, t as f32);
        }
        out
    }

    /// `z[t] = i^t`，四个取值 1、i、−1、−i 在 f32 上全部精确。
    fn quarter_turn() -> [(f32, f32); N + HFGEN + TS_OFFSET_HFADJ] {
        let cycle = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)];
        let mut out = [(0.0, 0.0); N + HFGEN + TS_OFFSET_HFADJ];
        for (t, slot) in out.iter_mut().enumerate() {
            *slot = cycle[t % 4];
        }
        out
    }

    #[test]
    fn the_accumulator_visits_every_second_timeslot_from_the_delay() {
        // 全 1 输入下每一项都是 1·conj(1) = 1，于是 cov 的每个元素都等于项数。
        // 项数写成字面量而不是从实现的常量推导：起点 4 与步长 2 是被检验的对象。
        let (state, q_low) = spread(&constant((1.0, 0.0)), 0);
        let cov = covariance(
            ExtendedLowBand::new(&state, &q_low),
            0,
            N + HFGEN + TS_OFFSET_HFADJ,
        );
        for i in 0..3 {
            for j in 1..3 {
                assert_eq!(cov.get(i, j), Complex::new(TERMS, 0.0), "cov[{i}][{j}]");
            }
        }
    }

    #[test]
    fn the_covariance_matches_the_hand_computed_values() {
        // z[t] = 1 + i·t 在 ts ∈ {4,6,8,10,12,14} 上逐项手算：
        //   cov[1][1] = Σ(1 + (ts−2)²) = 370        cov[2][2] = Σ(1 + (ts−4)²) = 226
        //   cov[1][2] = Σ(1 + (ts−2)(ts−4)) + 2i·6 = 286 + 12i
        //   cov[0][1] = Σ(1 + ts(ts−2))   + 2i·6 = 454 + 12i
        //   cov[0][2] = Σ(1 + ts(ts−4))   + 4i·6 = 346 + 24i
        // 少一次共轭时连 cov[1][1] 都不再是实数（−358 + 84i），差异一目了然。
        let (state, q_low) = spread(&linear_ramp(), 6);
        let cov = covariance(
            ExtendedLowBand::new(&state, &q_low),
            6,
            N + HFGEN + TS_OFFSET_HFADJ,
        );
        assert_eq!(cov.get(1, 1), Complex::new(370.0, 0.0));
        assert_eq!(cov.get(2, 2), Complex::new(226.0, 0.0));
        assert_eq!(cov.get(1, 2), Complex::new(286.0, 12.0));
        assert_eq!(cov.get(0, 1), Complex::new(454.0, 12.0));
        assert_eq!(cov.get(0, 2), Complex::new(346.0, 24.0));
    }

    #[test]
    fn the_diagonal_is_real_and_nonnegative() {
        let (state, q_low) = spread(&linear_ramp(), 3);
        let cov = covariance(
            ExtendedLowBand::new(&state, &q_low),
            3,
            N + HFGEN + TS_OFFSET_HFADJ,
        );
        for i in 1..3 {
            let diagonal = cov.get(i, i);
            // `Σ z·conj(z)` 的每一项虚部都精确为 0，累加不会引入虚部。
            assert_eq!(diagonal.im, 0.0, "cov[{i}][{i}] 应是实数");
            assert!(diagonal.re >= 0.0, "cov[{i}][{i}] 应非负");
        }
    }

    #[test]
    fn the_off_diagonal_pair_is_hermitian() {
        // `cov[2][1]` 是 `Pseudocode 86` 算了却没人用的那个元素，正好用来检验
        // 共轭对称：`cov[1][2] = conj(cov[2][1])`。
        let (state, q_low) = spread(&linear_ramp(), 7);
        let cov = covariance(
            ExtendedLowBand::new(&state, &q_low),
            7,
            N + HFGEN + TS_OFFSET_HFADJ,
        );
        assert_ne!(
            cov.get(1, 2).im,
            0.0,
            "信号退化成实数时这条判据不成其为判据"
        );
        assert_eq!(cov.get(1, 2), cov.get(2, 1).conj());
    }

    #[test]
    fn the_covariance_satisfies_cauchy_schwarz() {
        let (state, q_low) = spread(&linear_ramp(), 1);
        let cov = covariance(
            ExtendedLowBand::new(&state, &q_low),
            1,
            N + HFGEN + TS_OFFSET_HFADJ,
        );
        let product = cov.get(1, 1).re * cov.get(2, 2).re;
        assert!(
            cov.get(1, 2).norm_sqr() <= product * (1.0 + 1e-12),
            "|cov[1][2]|² 应不超过 cov[1][1]·cov[2][2]"
        );
    }

    #[test]
    fn a_single_complex_tone_is_predicted_exactly() {
        // z[t] = i^t 是纯单频。解析地：cov[1][1] = cov[2][2] = M，
        // cov[0][1] = cov[1][2] = −M，cov[0][2] = M，于是 alpha1 的分子
        // (−M)(−M) − (M)(M) 精确为 0，alpha0 = −(−M)/M = 1。
        //
        // 协方差矩阵在这里虽是奇异的，但 alpha1 分子也解析为零；有无正则化
        // 输出都相同。正则化由下面手工装配、分子非零的用例单独钉住。
        let (state, q_low) = spread(&quarter_turn(), 2);
        let cov = covariance(
            ExtendedLowBand::new(&state, &q_low),
            2,
            N + HFGEN + TS_OFFSET_HFADJ,
        );
        assert_eq!(cov.get(1, 1), Complex::new(TERMS, 0.0));
        assert_eq!(cov.get(0, 1), Complex::new(-TERMS, 0.0));
        assert_eq!(cov.get(0, 2), Complex::new(TERMS, 0.0));

        let (alpha0, alpha1) = solve(&cov);
        assert_eq!(alpha1, Complex::ZERO, "单频的 alpha1 分子解析为零");
        assert_eq!(alpha0, Complex::new(1.0, 0.0));
    }

    /// 手工装配一个协方差矩阵。
    ///
    /// `solve` 是纯函数，喂它一个不必来自真实信号的矩阵，才能把「奇异」与
    /// 「分子非零」这两件在真实信号上必然同时发生的事拆开：Cauchy–Schwarz 取
    /// 等号当且仅当两路信号线性相关，而那时 alpha1 的分子解析地为零。
    fn assemble(
        c01: Complex,
        c02: Complex,
        c11: Complex,
        c12: Complex,
        c22: Complex,
    ) -> Covariance {
        let mut cov = Covariance::default();
        cov.entries[0][0] = c01;
        cov.entries[0][1] = c02;
        cov.entries[1][0] = c11;
        cov.entries[1][1] = c12;
        cov.entries[2][1] = c22;
        cov
    }

    #[test]
    fn the_regularisation_keeps_a_singular_matrix_invertible() {
        // 故意不引用实现的 EPSILON_INV，避免实现与期望同时漂移。
        const SPEC_EPSILON_INV: f64 = 1.0 / 1_048_576.0;

        // |cov[1][2]|² = 4 = cov[1][1]·cov[2][2]，恰好取到 Cauchy–Schwarz 的
        // 等号：没有 1/(1+EPSILON_INV) 时 denom 精确为 0，alpha1 走 0 分支。
        // 分子 cov[0][1]·cov[1][2] − cov[0][2]·cov[1][1] = 2 ≠ 0，于是正则化
        // 生效与否给出的 alpha1 相差一个 1/ε 量级的因子。
        let cov = assemble(
            Complex::new(1.0, 0.0),
            Complex::ZERO,
            Complex::new(4.0, 0.0),
            Complex::new(2.0, 0.0),
            Complex::new(1.0, 0.0),
        );
        let (_, alpha1) = solve(&cov);
        let expected = 2.0 * (1.0 + SPEC_EPSILON_INV) / (4.0 * SPEC_EPSILON_INV);
        assert!(
            (alpha1.re - expected).abs() <= expected * 1e-9,
            "alpha1 {} 应为 {expected}；为 0 说明 denom 没有被正则化推离奇异",
            alpha1.re
        );
    }

    #[test]
    fn alpha0_conjugates_the_cross_term() {
        // alpha1 必须非零、cov[1][2] 必须非实，漏掉 cplx_conj 才看得出来。
        let c01 = Complex::new(1.0, 0.0);
        let c11 = Complex::new(2.0, 0.0);
        let c12 = Complex::new(1.0, 1.0);
        let cov = assemble(c01, Complex::ZERO, c11, c12, Complex::new(1.0, 0.0));
        let (alpha0, alpha1) = solve(&cov);
        assert_ne!(alpha1, Complex::ZERO, "本用例需要非零的 alpha1");
        assert_ne!(c12.im, 0.0, "本用例需要非实的 cov[1][2]");

        // alpha0 = (−cov[0][1] + alpha1·conj(cov[1][2])) / cov[1][1]，逐项按
        // Pseudocode 87 重算，共轭写成显式的取负虚部。
        let cross = Complex::new(
            alpha1.re * c12.re + alpha1.im * c12.im,
            alpha1.im * c12.re - alpha1.re * c12.im,
        );
        let expected = Complex::new((-c01.re + cross.re) / c11.re, (-c01.im + cross.im) / c11.re);
        assert!(
            (alpha0.re - expected.re).abs() <= expected.re.abs() * 1e-9,
            "alpha0 实部 {} 应为 {}",
            alpha0.re,
            expected.re
        );
        assert!(
            (alpha0.im - expected.im).abs() <= expected.im.abs() * 1e-9,
            "alpha0 虚部 {} 应为 {}",
            alpha0.im,
            expected.im
        );
    }

    #[test]
    fn the_magnitude_limit_is_exactly_four() {
        // 上界写字面 4，且取模恰好 4 与恰好在其下的两侧：把界挪到别处，两条断言
        // 至少有一条不成立。
        const SPEC_LIMIT: f32 = 4.0;
        let at_limit = Complex::new(f64::from(SPEC_LIMIT), 0.0);
        assert_eq!(
            clamp_unstable(at_limit, Complex::ZERO),
            (Complex::ZERO, Complex::ZERO),
            "模恰为 4 应置零（规范说的是 greater than or equal to）"
        );
        // 模为 3 与 4 之间：√(2²+2²) = 2,828…
        let below = Complex::new(2.0, 2.0);
        assert!(below.norm_sqr() < f64::from(SPEC_LIMIT) * f64::from(SPEC_LIMIT));
        assert_eq!(
            clamp_unstable(below, Complex::ZERO),
            (below, Complex::ZERO),
            "模低于 4 应原样保留"
        );
        // 越界的是 alpha1 时，alpha0 同样要被清掉。
        assert_eq!(
            clamp_unstable(below, at_limit),
            (Complex::ZERO, Complex::ZERO),
            "任一越界则两者一起置零"
        );
    }

    #[test]
    fn silence_yields_zero_coefficients() {
        let (state, q_low) = spread(&constant((0.0, 0.0)), 0);
        let cov = covariance(
            ExtendedLowBand::new(&state, &q_low),
            0,
            N + HFGEN + TS_OFFSET_HFADJ,
        );
        let (alpha0, alpha1) = solve(&cov);
        assert_eq!(alpha0, Complex::ZERO);
        assert_eq!(alpha1, Complex::ZERO);
    }

    #[test]
    fn coefficients_at_or_beyond_the_magnitude_limit_are_zeroed() {
        // 上界写成字面 4，不引用实现的常量：自证循环会让这条判据失效。
        const SPEC_LIMIT: f64 = 4.0;
        let mut state = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        // 让 |alpha0| 明确越界：一个几何增长的序列会给出远大于 4 的预测增益。
        let mut value = 1.0f32;
        for ts in 0..TS_OFFSET_HFADJ {
            state.tail[ts].re[0] = value;
            value *= 8.0;
        }
        for slot in &mut q_low {
            slot.re[0] = value;
            value *= 8.0;
        }
        let cov = covariance(
            ExtendedLowBand::new(&state, &q_low),
            0,
            N + HFGEN + TS_OFFSET_HFADJ,
        );
        let (alpha0, alpha1) = solve(&cov);
        assert!(
            alpha0.norm_sqr().sqrt() >= SPEC_LIMIT || alpha1.norm_sqr().sqrt() >= SPEC_LIMIT,
            "本用例应当越界，否则它检验不到置零规则"
        );

        let mut out = TnaFilters::new();
        prediction_filters(ExtendedLowBand::new(&state, &q_low), N, 1, &mut out).expect("应能解算");
        assert_eq!(
            out.coefficients(0),
            Some((Coefficient::ZERO, Coefficient::ZERO)),
            "任一系数越界时两个系数一起置零"
        );
    }

    #[test]
    fn the_delay_line_carries_the_interval_before_the_hfgen_tail() {
        // Q_low_prev 的 [N−4, N) 与 [N, N+hfgen) 填成不同的值：取错成「末尾
        // 4 项」会拿到后者。两次调用后核对状态里存的是前者。
        let mut state = TnaDelay::new();
        let mut q_low = [QmfSlot::zero(); N + HFGEN];
        for (k, slot) in q_low.iter_mut().enumerate() {
            slot.re[0] = if k < N { 1.0 } else { 9.0 };
        }
        state.advance(&q_low, N).expect("应能推进");
        for ts in 0..TS_OFFSET_HFADJ {
            assert_eq!(
                state.tail[ts].re[0], 1.0,
                "历史应取 [N−4, N)，而不是含 hfgen 尾巴的末尾 4 项"
            );
        }
    }

    #[test]
    fn splitting_a_signal_in_two_reproduces_the_single_pass_covariance() {
        // 把同一条信号一次算完，与分两个区间连续送入，第二个区间的协方差必须
        // 相同——这是延迟线接得上的行为判据，不看任何内部下标。
        let sb = 5;
        let mut long = [QmfSlot::zero(); 2 * N + HFGEN];
        for (k, slot) in long.iter_mut().enumerate() {
            slot.re[sb] = (k as f32) * 0.25 - 1.0;
            slot.im[sb] = 1.0 - (k as f32) * 0.125;
        }

        // 连续两个区间：第一个区间的 Q_low 是 long[0..N+HFGEN]，第二个是
        // long[N..2N+HFGEN]，二者在 hfgen 处重叠，正如延迟线的定义。
        let mut first = [QmfSlot::zero(); N + HFGEN];
        first.copy_from_slice(&long[..N + HFGEN]);
        let mut second = [QmfSlot::zero(); N + HFGEN];
        second.copy_from_slice(&long[N..]);

        let mut state = TnaDelay::new();
        state.advance(&first, N).expect("第一区间应能推进");
        let split = covariance(
            ExtendedLowBand::new(&state, &second),
            sb,
            N + HFGEN + TS_OFFSET_HFADJ,
        );

        // 参照：直接把 long 的对应窗口铺成一次调用的 Q_low_ext。
        let mut reference_state = TnaDelay::new();
        for ts in 0..TS_OFFSET_HFADJ {
            reference_state.tail[ts] = long[N - TS_OFFSET_HFADJ + ts];
        }
        let reference = covariance(
            ExtendedLowBand::new(&reference_state, &second),
            sb,
            N + HFGEN + TS_OFFSET_HFADJ,
        );
        for i in 0..3 {
            for j in 1..3 {
                assert_eq!(split.get(i, j), reference.get(i, j), "cov[{i}][{j}]");
            }
        }
    }

    #[test]
    fn rejected_input_leaves_the_output_untouched() {
        let (state, q_low) = spread(&quarter_turn(), 0);
        let mut out = TnaFilters::new();
        prediction_filters(ExtendedLowBand::new(&state, &q_low), N, 3, &mut out)
            .expect("哨兵结果应可生成");
        let subbands = out.subbands();
        let snapshot: [Option<(Coefficient, Coefficient)>; 3] = [
            out.coefficients(0),
            out.coefficients(1),
            out.coefficients(2),
        ];

        for error in [
            prediction_filters(ExtendedLowBand::new(&state, &q_low), 2, 3, &mut out),
            prediction_filters(ExtendedLowBand::new(&state, &q_low[..2]), N, 3, &mut out),
            prediction_filters(ExtendedLowBand::new(&state, &q_low), N - 1, 3, &mut out),
            prediction_filters(ExtendedLowBand::new(&state, &q_low), N, 65, &mut out),
        ] {
            assert!(error.is_err(), "该输入应被拒绝");
        }
        assert_eq!(out.subbands(), subbands, "已填充子带数被改写");
        for (sb, expected) in snapshot.iter().enumerate() {
            assert_eq!(&out.coefficients(sb), expected, "子带 {sb} 的系数被改写");
        }
    }

    #[test]
    fn the_chirp_table_is_indexed_by_current_mode_first() {
        // 表 195 在 (prev=None, cur=Moderate) 处是 0,9，转置位置是 0,0。初始状态
        // 的 prev 就是 None，因此一次调用即可区分两种索引顺序。
        //
        // 期望值按表 195 与 Pseudocode 88 手算，两个因子都是规范里的字面量，
        // 不引用实现的常量：
        //   new_chirp = 0,9；0,9 ≥ 0（prev_chirp）→ 取 29/32 一支
        //   0,90625 × 0,9 + 0,09375 × 0
        //
        // 乘积不写成十进制 0,815625：0,9 在 f32 上不精确，那个十进制字面量落在
        // 相邻的另一个 f32 上，差一个 ulp。转置一旦写反结果是 0，两者相差远不止
        // 一个 ulp，判据的鉴别力不受此影响。
        const MODERATE: u8 = 2;
        let mut state = ChirpState::new();
        let mut out = [0.0f32];
        chirp_factors(&[MODERATE], &mut state, &mut out).expect("应能计算");
        assert_eq!(out[0], 0.906_25_f32 * 0.9_f32);
    }

    #[test]
    fn the_transposed_direction_gives_a_different_value() {
        // 反向 (prev=Moderate, cur=None) 在表 195 里是 0,0。走到这里需要两个区间。
        const MODERATE: u8 = 2;
        const NONE: u8 = 0;
        let mut state = ChirpState::new();
        let mut out = [0.0f32];
        chirp_factors(&[MODERATE], &mut state, &mut out).expect("第一区间");
        let previous = out[0];
        chirp_factors(&[NONE], &mut state, &mut out).expect("第二区间");
        // new_chirp = 0,0 < prev → 取 3/4 一支：0,75 × 0 + 0,25 × 0,815625
        assert_eq!(out[0], 0.25 * previous);
    }

    #[test]
    fn the_two_smoothing_branches_use_their_own_weights() {
        // 上升沿用 29/32 + 3/32，下降沿用 3/4 + 1/4。权重写成字面分数。
        const HEAVY: u8 = 3;
        const NONE: u8 = 0;
        let mut state = ChirpState::new();
        let mut out = [0.0f32];
        chirp_factors(&[HEAVY], &mut state, &mut out).expect("第一区间");
        let first = out[0];
        assert_eq!(first, 0.906_25 * 0.98, "上升沿");

        // 第二区间：cur=None、prev=Heavy → 表 195 给 0,0，低于 prev，走下降沿。
        chirp_factors(&[NONE], &mut state, &mut out).expect("第二区间");
        assert_eq!(out[0], 0.25 * first, "下降沿");
    }

    #[test]
    fn values_below_the_floor_become_exactly_zero_and_reset_the_smoothing() {
        // 反复送 None 会让 chirp 按 1/4 衰减，越过 2^-6 后必须变成精确的 0，
        // 而且状态存的是这个 0——下一区间的平滑从 0 起步，不是从残值起步。
        const SPEC_FLOOR: f32 = 0.015_625;
        const MODERATE: u8 = 2;
        const NONE: u8 = 0;
        let mut state = ChirpState::new();
        let mut out = [0.0f32];
        chirp_factors(&[MODERATE], &mut state, &mut out).expect("第一区间");
        let mut seen_zero = false;
        for _ in 0..8 {
            let before = out[0];
            chirp_factors(&[NONE], &mut state, &mut out).expect("后续区间");
            if before < SPEC_FLOOR / 0.25 && out[0] == 0.0 {
                seen_zero = true;
                break;
            }
        }
        assert!(seen_zero, "持续 None 应当落到阈值以下");
        // 再走一个区间：从 0 起步，0,75 × 0 + 0,25 × 0 = 0。
        chirp_factors(&[NONE], &mut state, &mut out).expect("阈值之后");
        assert_eq!(out[0], 0.0, "状态存的应是截断后的 0");
    }

    #[test]
    fn the_floor_sits_exactly_at_two_to_the_minus_six() {
        // 阈值写字面 2^-6，并从两侧各取一个恰好跨界的点。规范的判定是严格小于，
        // 因此等于阈值的那个点必须保留。
        //
        // 直接设状态：cur=None 时 new_chirp = 0，低于任何非零 prev，走 1/4 一支，
        // 于是输出恰为 prev/4——把 prev 取成 4·2^-6 与略小于它即可跨界。
        const SPEC_FLOOR: f32 = 0.015_625;
        const NONE: u8 = 0;
        for (previous, expected, note) in [
            (4.0 * SPEC_FLOOR, SPEC_FLOOR, "恰好等于阈值应保留"),
            (4.0 * SPEC_FLOOR - f32::EPSILON, 0.0, "略低于阈值应归零"),
        ] {
            let mut state = ChirpState::new();
            state.previous[0] = previous;
            let mut out = [0.0f32];
            chirp_factors(&[NONE], &mut state, &mut out).expect("应能计算");
            assert_eq!(out[0], expected, "{note}");
        }
    }

    #[test]
    fn an_out_of_range_mode_is_rejected() {
        let mut state = ChirpState::new();
        let mut out = [0.0f32; 2];
        assert_eq!(
            chirp_factors(&[0, 4], &mut state, &mut out),
            Err(TnaError::ModeOutOfRange { group: 1, mode: 4 })
        );
        assert_eq!(out, [0.0, 0.0], "被拒绝的输入不应改写输出");
    }

    #[test]
    fn output_length_mismatch_is_precise_and_atomic() {
        let mut state = ChirpState::new();
        let mut seeded = [0.0f32; 2];
        chirp_factors(&[2, 3], &mut state, &mut seeded).expect("哨兵状态应可生成");
        let previous = state.previous;
        let modes = state.modes;
        let mut out = [-1.0f32];

        assert_eq!(
            chirp_factors(&[1, 2], &mut state, &mut out),
            Err(TnaError::OutputLengthMismatch {
                expected: 2,
                provided: 1,
            })
        );
        assert_eq!(state.previous, previous, "失败不应改写 chirp 历史");
        assert_eq!(state.modes, modes, "失败不应改写 mode 历史");
        assert_eq!(out, [-1.0], "失败不应改写输出缓冲");
    }

    #[test]
    fn every_table_195_entry_matches_the_specification() {
        // 表 195 逐格照抄，行是 prev、列是当前；实现的 NEW_CHIRP 是它的转置。
        const TABLE_195: [[f32; 4]; 4] = [
            [0.0, 0.6, 0.9, 0.98],
            [0.6, 0.75, 0.9, 0.98],
            [0.0, 0.75, 0.9, 0.98],
            [0.0, 0.75, 0.9, 0.98],
        ];
        for prev in 0..4 {
            for current in 0..4 {
                assert_eq!(
                    NEW_CHIRP[current][prev], TABLE_195[prev][current],
                    "prev={prev} cur={current}"
                );
            }
        }
    }
}
