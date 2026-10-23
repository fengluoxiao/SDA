//! A-SPX 信号与噪声包络的解码（`TS103190-1:v1.4.1:5.7.6.3.4`）。
//!
//! `Pseudocode 80`（信号）与 `Pseudocode 81`（噪声）把码流里的差分符号累加成
//! 量化标度因子 `qscf`。两者共用同一套方向语义：
//!
//! - `aspx_*_delta_dir == 0`（**频率**方向）：`qscf[sbg] = Σ_{i≤sbg} δ·data[i]`，
//!   即从最低子带组起的前缀和，不引用任何历史；
//! - `aspx_*_delta_dir == 1`（**时间**方向）：`qscf[sbg] = prev[sbg] + δ·data[sbg]`，
//!   其中 `prev` 是上一个包络；本区间首个包络则取**上一区间最后一个包络**。
//!
//! `δ` 在 `ch == 1 且 aspx_balance == 1` 时为 2，否则为 1。
//!
//! 信号侧多一层：两个包络的频率分辨率可能不同，`sbg` 要经
//! `sbg_idx_high2low`/`sbg_idx_low2high` 映射之后才对得上。噪声侧恒用
//! `sbg_noise`，没有这一层。
//!
//! 跨区间历史因此是必需状态，且信号侧还要记住上一区间最后一个包络的分辨率
//! ——`freq_res_prev` 决定用哪个方向的映射。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "下标由已核对过的子带组数与包络数派生；两条伪码的映射用显式下标\
              比迭代器更贴近原文，便于逐行核对"
)]

use crate::aspx::bands::AspxBandTables;
use crate::aspx::frames::{MAX_ATSG_NOISE, MAX_ATSG_SIG};
use crate::aspx::syntax::MAX_SBG_PER_ENVELOPE;
use crate::aspx::tables::{EnvelopeKind, MAX_SBG_NOISE};

/// 噪声子带组数的上限。
const NOISE_GROUPS: usize = MAX_SBG_NOISE as usize;

/// 高低分辨率子带组之间的双向下标映射（`Pseudocode 80` 开头）。
///
/// `high2low[sbg]` 给出高分辨率组 `sbg` 落在哪个低分辨率组里；
/// `low2high[sbg_low]` 给出低分辨率组 `sbg_low` 的起始高分辨率组。
#[derive(Debug, Clone, Copy)]
pub struct SbgIndexMap {
    high2low: [u8; MAX_SBG_PER_ENVELOPE],
    low2high: [u8; MAX_SBG_PER_ENVELOPE],
    num_high: u8,
    num_low: u8,
    num_noise: u8,
}

impl SbgIndexMap {
    /// 按 `Pseudocode 80` 的循环推导映射。
    ///
    /// 低分辨率边界是高分辨率边界的子集，因此扫一遍高分辨率边界、在命中低分辨率
    /// 边界时推进 `sbg_low` 即可，无需搜索。
    ///
    /// # Errors
    ///
    /// 组数超出容量，或边界表取不到时返回 [`EnvelopeError`]。
    pub fn derive(bands: &AspxBandTables) -> Result<Self, EnvelopeError> {
        let num_high = usize::from(bands.num_sbg_sig_highres());
        let num_low = usize::from(bands.num_sbg_sig_lowres());
        if num_high > MAX_SBG_PER_ENVELOPE || num_low > MAX_SBG_PER_ENVELOPE {
            return Err(EnvelopeError::TooManyGroups {
                high: num_high,
                low: num_low,
            });
        }
        let mut map = Self {
            high2low: [0; MAX_SBG_PER_ENVELOPE],
            low2high: [0; MAX_SBG_PER_ENVELOPE],
            num_high: bands.num_sbg_sig_highres(),
            num_low: bands.num_sbg_sig_lowres(),
            num_noise: bands.num_sbg_noise(),
        };
        let mut low = 0usize;
        for high in 0..num_high {
            let next_low_border = bands
                .sig_lowres_border(low + 1)
                .ok_or(EnvelopeError::MissingBorder { index: low + 1 })?;
            let high_border = bands
                .sig_highres_border(high)
                .ok_or(EnvelopeError::MissingBorder { index: high })?;
            if next_low_border == high_border {
                low += 1;
                map.low2high[low] = u8::try_from(high).unwrap_or(u8::MAX);
            }
            map.high2low[high] = u8::try_from(low).unwrap_or(u8::MAX);
        }
        Ok(map)
    }

    /// 高分辨率组 `sbg` 所属的低分辨率组。
    #[must_use]
    pub fn high_to_low(&self, sbg: usize) -> Option<u8> {
        if sbg >= usize::from(self.num_high) {
            return None;
        }
        self.high2low.get(sbg).copied()
    }

    /// 噪声子带组数，来自 `5.7.6.3.1.3` 的 `sbg_noise`。
    #[must_use]
    pub const fn noise_groups(&self) -> u8 {
        self.num_noise
    }

    /// 低分辨率组 `sbg` 的起始高分辨率组。
    #[must_use]
    pub fn low_to_high(&self, sbg: usize) -> Option<u8> {
        if sbg >= usize::from(self.num_low) {
            return None;
        }
        self.low2high.get(sbg).copied()
    }
}

/// 跨 A-SPX 区间的包络历史。
///
/// 时间方向的差分会跨区间边界引用上一区间的最后一个包络，因此这三样都要留：
/// 信号标度因子、那个包络的频率分辨率、噪声标度因子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeHistory {
    sig: [i16; MAX_SBG_PER_ENVELOPE],
    sig_freq_res: bool,
    noise: [i16; NOISE_GROUPS],
    /// 是否已有上一区间。首个区间没有历史，时间方向此时无从引用。
    primed: bool,
}

impl EnvelopeHistory {
    /// 建立空历史。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sig: [0; MAX_SBG_PER_ENVELOPE],
            sig_freq_res: false,
            noise: [0; NOISE_GROUPS],
            primed: false,
        }
    }

    /// 是否已有上一区间的包络。
    #[must_use]
    pub const fn is_primed(&self) -> bool {
        self.primed
    }
}

impl Default for EnvelopeHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// 包络解码无法完成的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeError {
    /// 子带组数超出定长容量。
    TooManyGroups { high: usize, low: usize },
    /// 子带组边界表取不到该下标。
    MissingBorder { index: usize },
    /// 包络数超出定长容量。
    TooManyEnvelopes { signal: usize, noise: usize },
    /// 信号或噪声包络为空。
    ///
    /// `AspxInterval` 保证两类包络都至少有一个；接受空切片会让单一的跨区间
    /// 历史标志无法说明究竟哪一侧已经初始化。
    MissingEnvelopes { signal: usize, noise: usize },
    /// 时间方向差分引用了不存在的历史。
    ///
    /// 首个 A-SPX 区间没有上一区间；此时首个包络若声明时间方向，码流要么
    /// 从非起解点开始，要么解析已经错位。不能假定历史为零继续。
    MissingHistory { envelope: usize },
    /// 输出容量不足。
    OutputTooSmall { needed: usize, provided: usize },
    /// 量化标度因子的乘加超出 `i16`。
    ScaleFactorOverflow {
        kind: EnvelopeKind,
        envelope: usize,
        group: usize,
    },
}

/// 一个声道在本区间内解出的量化标度因子。
#[derive(Debug, Clone, Copy)]
pub struct EnvelopeScaleFactors {
    sig: [[i16; MAX_SBG_PER_ENVELOPE]; MAX_ATSG_SIG],
    sig_groups: [u8; MAX_ATSG_SIG],
    num_sig: u8,
    noise: [[i16; NOISE_GROUPS]; MAX_ATSG_NOISE],
    noise_groups: u8,
    num_noise: u8,
}

impl EnvelopeScaleFactors {
    /// 全零结果。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sig: [[0; MAX_SBG_PER_ENVELOPE]; MAX_ATSG_SIG],
            sig_groups: [0; MAX_ATSG_SIG],
            num_sig: 0,
            noise: [[0; NOISE_GROUPS]; MAX_ATSG_NOISE],
            noise_groups: 0,
            num_noise: 0,
        }
    }

    /// 第 `env` 个信号包络第 `sbg` 组的 `qscf_sig_sbg`。
    #[must_use]
    pub fn sig(&self, env: usize, sbg: usize) -> Option<i16> {
        if env >= usize::from(self.num_sig) || sbg >= usize::from(*self.sig_groups.get(env)?) {
            return None;
        }
        self.sig.get(env)?.get(sbg).copied()
    }

    /// 第 `env` 个噪声包络第 `sbg` 组的 `qscf_noise_sbg`。
    #[must_use]
    pub fn noise(&self, env: usize, sbg: usize) -> Option<i16> {
        if env >= usize::from(self.num_noise) || sbg >= usize::from(self.noise_groups) {
            return None;
        }
        self.noise.get(env)?.get(sbg).copied()
    }

    /// 信号与噪声包络数。
    #[must_use]
    pub const fn counts(&self) -> (u8, u8) {
        (self.num_sig, self.num_noise)
    }

    /// 第 `env` 个信号包络的子带组数，取决于该包络的 `atsg_freqres`。
    #[must_use]
    pub fn sig_group_count(&self, env: usize) -> Option<u8> {
        if env >= usize::from(self.num_sig) {
            return None;
        }
        self.sig_groups.get(env).copied()
    }

    /// 噪声子带组数，逐包络相同（`sbg_noise` 不随包络变化）。
    #[must_use]
    pub const fn noise_group_count(&self) -> u8 {
        self.noise_groups
    }
}

impl Default for EnvelopeScaleFactors {
    fn default() -> Self {
        Self::new()
    }
}

/// 一个包络的差分输入，即码流解出的符号与方向。
#[derive(Debug, Clone, Copy)]
pub struct EnvelopeDeltas<'a> {
    /// `aspx_data_sig[atsg]` 或 `aspx_data_noise[atsg]`。
    pub data: &'a [i16],
    /// `aspx_*_delta_dir[atsg]`：假为频率方向，真为时间方向。
    pub time_direction: bool,
    /// `atsg_freqres[atsg]`；噪声侧忽略。
    pub high_resolution: bool,
}

/// 解码一个声道在本区间的信号与噪声包络。
///
/// `delta_is_two` 对应 `Pseudocode 80`/`81` 开头的
/// `ch == 1 && aspx_balance == 1`。
/// `signal` 与 `noise` 都必须至少包含一个包络，这与 `AspxInterval` 的合法
/// 取值域一致。标度因子用 checked `i16` 乘加；规范没有饱和语义。
///
/// # Errors
///
/// 见 [`EnvelopeError`]。任一条不成立时都不改写历史。
pub fn decode(
    signal: &[EnvelopeDeltas<'_>],
    noise: &[EnvelopeDeltas<'_>],
    map: &SbgIndexMap,
    delta_is_two: bool,
    history: &mut EnvelopeHistory,
    out: &mut EnvelopeScaleFactors,
) -> Result<(), EnvelopeError> {
    if signal.is_empty() || noise.is_empty() {
        return Err(EnvelopeError::MissingEnvelopes {
            signal: signal.len(),
            noise: noise.len(),
        });
    }
    if signal.len() > MAX_ATSG_SIG || noise.len() > MAX_ATSG_NOISE {
        return Err(EnvelopeError::TooManyEnvelopes {
            signal: signal.len(),
            noise: noise.len(),
        });
    }
    for (index, envelope) in signal.iter().enumerate() {
        let groups = if envelope.high_resolution {
            usize::from(map.num_high)
        } else {
            usize::from(map.num_low)
        };
        if envelope.data.len() < groups {
            return Err(EnvelopeError::OutputTooSmall {
                needed: groups,
                provided: envelope.data.len(),
            });
        }
        if index == 0 && envelope.time_direction && !history.primed {
            return Err(EnvelopeError::MissingHistory { envelope: 0 });
        }
    }
    let noise_groups = usize::from(map.num_noise).min(NOISE_GROUPS);
    for (index, envelope) in noise.iter().enumerate() {
        if envelope.data.len() < noise_groups {
            return Err(EnvelopeError::OutputTooSmall {
                needed: noise_groups,
                provided: envelope.data.len(),
            });
        }
        if index == 0 && envelope.time_direction && !history.primed {
            return Err(EnvelopeError::MissingHistory { envelope: 0 });
        }
    }

    let delta = if delta_is_two { 2i16 } else { 1 };
    let mut result = EnvelopeScaleFactors::new();
    result.num_sig = u8::try_from(signal.len()).unwrap_or(u8::MAX);
    result.num_noise = u8::try_from(noise.len()).unwrap_or(u8::MAX);

    // 信号包络：`Pseudocode 80`。
    let mut previous_res = history.sig_freq_res;
    for (index, envelope) in signal.iter().enumerate() {
        let groups = if envelope.high_resolution {
            usize::from(map.num_high)
        } else {
            usize::from(map.num_low)
        };
        result.sig_groups[index] = u8::try_from(groups).unwrap_or(u8::MAX);

        if envelope.time_direction {
            for sbg in 0..groups {
                // 分辨率相同则直接对位；不同则按 `Pseudocode 80` 的两个方向映射。
                let mapped = if envelope.high_resolution == previous_res {
                    sbg
                } else if envelope.high_resolution {
                    usize::from(map.high2low[sbg])
                } else {
                    usize::from(map.low2high[sbg])
                };
                let previous = if index == 0 {
                    history.sig[mapped]
                } else {
                    result.sig[index - 1][mapped]
                };
                result.sig[index][sbg] = checked_accumulate(
                    previous,
                    delta,
                    envelope.data[sbg],
                    EnvelopeKind::Signal,
                    index,
                    sbg,
                )?;
            }
        } else {
            let mut running = 0i16;
            for sbg in 0..groups {
                running = checked_accumulate(
                    running,
                    delta,
                    envelope.data[sbg],
                    EnvelopeKind::Signal,
                    index,
                    sbg,
                )?;
                result.sig[index][sbg] = running;
            }
        }
        previous_res = envelope.high_resolution;
    }

    // 噪声包络：`Pseudocode 81`，没有分辨率映射。
    result.noise_groups = u8::try_from(noise_groups).unwrap_or(u8::MAX);
    for (index, envelope) in noise.iter().enumerate() {
        if envelope.time_direction {
            for sbg in 0..noise_groups {
                let previous = if index == 0 {
                    history.noise[sbg]
                } else {
                    result.noise[index - 1][sbg]
                };
                result.noise[index][sbg] = checked_accumulate(
                    previous,
                    delta,
                    envelope.data[sbg],
                    EnvelopeKind::Noise,
                    index,
                    sbg,
                )?;
            }
        } else {
            let mut running = 0i16;
            for sbg in 0..noise_groups {
                running = checked_accumulate(
                    running,
                    delta,
                    envelope.data[sbg],
                    EnvelopeKind::Noise,
                    index,
                    sbg,
                )?;
                result.noise[index][sbg] = running;
            }
        }
    }

    // 全部成功后才推进历史。
    if let Some(last) = signal.len().checked_sub(1) {
        history.sig = result.sig[last];
        history.sig_freq_res = signal[last].high_resolution;
    }
    if let Some(last) = noise.len().checked_sub(1) {
        history.noise = result.noise[last];
    }
    history.primed = true;
    *out = result;
    Ok(())
}

fn checked_accumulate(
    previous: i16,
    delta: i16,
    value: i16,
    kind: EnvelopeKind,
    envelope: usize,
    group: usize,
) -> Result<i16, EnvelopeError> {
    let overflow = EnvelopeError::ScaleFactorOverflow {
        kind,
        envelope,
        group,
    };
    let scaled = delta.checked_mul(value).ok_or(overflow)?;
    previous.checked_add(scaled).ok_or(overflow)
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "下标由同一用例构造的组数与包络数派生，越界即是该用例要报告的失败；\
              逐组比对时显式下标比 zip 更贴近伪码的 sbg 语义"
)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec;
    use std::vec::Vec;

    /// 用实测出现过的三种几何之一建表：`start_freq = 0`、`stop_freq = 0`。
    fn tables() -> AspxBandTables {
        AspxBandTables::derive(true, 0, 0, 1, 0).expect("测试频带表应可推导")
    }

    fn deltas<'a>(data: &'a [i16], time: bool, high: bool) -> EnvelopeDeltas<'a> {
        EnvelopeDeltas {
            data,
            time_direction: time,
            high_resolution: high,
        }
    }

    fn zero_noise(map: &SbgIndexMap) -> Vec<i16> {
        vec![0; usize::from(map.noise_groups())]
    }

    /// 映射必须与低分辨率边界是高分辨率边界子集这一事实自洽。
    ///
    /// 三条结构判据都不依赖本模块的推导过程：`high2low` 单调不减且以 0 起、
    /// `low2high` 严格递增，以及两者互为逆——`high2low[low2high[l]] == l`。
    #[test]
    fn the_index_map_is_consistent_with_the_border_tables() {
        let bands = tables();
        let map = SbgIndexMap::derive(&bands).expect("映射应可推导");
        let high = usize::from(bands.num_sbg_sig_highres());
        let low = usize::from(bands.num_sbg_sig_lowres());
        assert!(high >= low && low > 0, "高分辨率组数不应少于低分辨率");

        assert_eq!(map.high_to_low(0), Some(0));
        let mut previous = 0u8;
        for sbg in 0..high {
            let mapped = map.high_to_low(sbg).expect("高分辨率组应有映射");
            assert!(mapped >= previous, "high2low 应单调不减");
            assert!(usize::from(mapped) < low, "映射结果应落在低分辨率组内");
            previous = mapped;
        }
        assert_eq!(
            previous,
            u8::try_from(low - 1).expect("组数可表示"),
            "应覆盖到最后一组"
        );

        let mut last = None;
        for sbg in 0..low {
            let start = map.low_to_high(sbg).expect("低分辨率组应有起点");
            if let Some(previous) = last {
                assert!(start > previous, "low2high 应严格递增");
            }
            last = Some(start);
            assert_eq!(
                map.high_to_low(usize::from(start)),
                Some(u8::try_from(sbg).expect("组数可表示")),
                "low2high 与 high2low 应互逆"
            );
            // 起点处的边界必须相等，这是映射的定义。
            assert_eq!(
                bands.sig_highres_border(usize::from(start)),
                bands.sig_lowres_border(sbg),
                "第 {sbg} 个低分辨率组的起点边界应与高分辨率一致"
            );
        }
    }

    /// 频率方向就是前缀和，且**不引用任何历史**。
    ///
    /// 把历史换成完全不同的值，结果必须逐位不变——否则说明频率方向误读了历史。
    #[test]
    fn the_frequency_direction_is_a_prefix_sum_independent_of_history() {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let groups = usize::from(map.num_high);
        let data: Vec<i16> = (0..groups).map(|index| (index as i16) - 3).collect();
        let noise = zero_noise(&map);

        let run = |seed: i16| {
            let mut history = EnvelopeHistory::new();
            history.sig = [seed; MAX_SBG_PER_ENVELOPE];
            history.noise = [seed; NOISE_GROUPS];
            history.primed = true;
            let mut out = EnvelopeScaleFactors::new();
            decode(
                &[deltas(&data, false, true)],
                &[deltas(&noise, false, false)],
                &map,
                false,
                &mut history,
                &mut out,
            )
            .expect("频率方向应成功");
            (0..groups)
                .map(|sbg| out.sig(0, sbg).expect("应有结果"))
                .collect::<Vec<_>>()
        };

        let expected: Vec<i16> = data
            .iter()
            .scan(0i16, |sum, value| {
                *sum += value;
                Some(*sum)
            })
            .collect();
        assert_eq!(run(0), expected, "频率方向应是前缀和");
        assert_eq!(run(1000), expected, "频率方向不得引用历史");
    }

    /// 时间方向在包络之间与跨区间边界上都取「上一个包络」。
    #[test]
    fn the_time_direction_chains_within_and_across_intervals() {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let groups = usize::from(map.num_high);
        let base: Vec<i16> = (0..groups).map(|index| (index as i16) % 5 - 2).collect();
        let step: Vec<i16> = vec![2; groups];
        let noise = zero_noise(&map);

        let mut history = EnvelopeHistory::new();
        let mut out = EnvelopeScaleFactors::new();
        // 首个区间：第一个包络必须走频率方向，第二个走时间方向。
        decode(
            &[deltas(&base, false, true), deltas(&step, true, true)],
            &[deltas(&noise, false, false)],
            &map,
            false,
            &mut history,
            &mut out,
        )
        .expect("首个区间应成功");
        for sbg in 0..groups {
            let first = out.sig(0, sbg).expect("首包络");
            assert_eq!(
                out.sig(1, sbg),
                Some(first + 2),
                "区间内时间方向应逐包络累加"
            );
        }
        let last: Vec<i16> = (0..groups)
            .map(|sbg| out.sig(1, sbg).expect("末包络"))
            .collect();
        assert!(history.is_primed());

        // 下一个区间：首个包络走时间方向，应接上一区间的末包络。
        let mut next = EnvelopeScaleFactors::new();
        decode(
            &[deltas(&step, true, true)],
            &[deltas(&noise, false, false)],
            &map,
            false,
            &mut history,
            &mut next,
        )
        .expect("下一区间应成功");
        for sbg in 0..groups {
            assert_eq!(
                next.sig(0, sbg),
                Some(last[sbg] + 2),
                "跨区间应接上一区间的最后一个包络"
            );
        }
    }

    /// `delta = 2` 只在平衡式立体声的第二声道生效。
    #[test]
    fn the_balance_channel_doubles_every_delta() {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let groups = usize::from(map.num_high);
        let data: Vec<i16> = vec![3; groups];
        let noise = zero_noise(&map);

        let run = |doubled: bool| {
            let mut history = EnvelopeHistory::new();
            let mut out = EnvelopeScaleFactors::new();
            decode(
                &[deltas(&data, false, true)],
                &[deltas(&noise, false, false)],
                &map,
                doubled,
                &mut history,
                &mut out,
            )
            .expect("应成功");
            out.sig(0, groups - 1).expect("末组")
        };
        assert_eq!(run(true), run(false) * 2, "delta = 2 应让整条链翻倍");
    }

    /// 分辨率在包络之间双向切换时，时间方向按 `Pseudocode 80` 映射取值。
    #[test]
    fn resolution_changes_map_the_previous_envelope_in_both_directions() {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let high = usize::from(map.num_high);
        let low = usize::from(map.num_low);
        assert!(high > low, "本用例需要高低分辨率组数不同");

        let first: Vec<i16> = (0..high).map(|index| index as i16 + 1).collect();
        let second: Vec<i16> = vec![0; low];
        let third: Vec<i16> = vec![0; high];
        let noise = zero_noise(&map);
        let mut history = EnvelopeHistory::new();
        let mut out = EnvelopeScaleFactors::new();
        decode(
            &[
                deltas(&first, false, true),
                deltas(&second, true, false),
                deltas(&third, true, true),
            ],
            &[deltas(&noise, false, false)],
            &map,
            false,
            &mut history,
            &mut out,
        )
        .expect("分辨率切换应成功");

        // 第二个包络是低分辨率、时间方向、差分全零，因此每组应等于上一包络
        // 中 `low2high[sbg]` 处的值。
        for sbg in 0..low {
            let source = usize::from(map.low_to_high(sbg).expect("起点"));
            assert_eq!(
                out.sig(1, sbg),
                out.sig(0, source),
                "低分辨率组 {sbg} 应取高分辨率组 {source} 的值"
            );
        }
        // 第三个包络切回高分辨率，应按 high2low 复制第二个低分辨率包络。
        for sbg in 0..high {
            let source = usize::from(map.high_to_low(sbg).expect("所属低分辨率组"));
            assert_eq!(
                out.sig(2, sbg),
                out.sig(1, source),
                "高分辨率组 {sbg} 应取低分辨率组 {source} 的值"
            );
        }
    }

    /// 首个区间的第一个包络若声明时间方向，必须拒绝而不是假定历史为零。
    #[test]
    fn a_time_delta_without_history_is_rejected() {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let groups = usize::from(map.num_high);
        let data: Vec<i16> = vec![1; groups];
        let noise_zero = zero_noise(&map);
        let mut history = EnvelopeHistory::new();
        let mut out = EnvelopeScaleFactors::new();
        assert_eq!(
            decode(
                &[deltas(&data, true, true)],
                &[deltas(&noise_zero, false, false)],
                &map,
                false,
                &mut history,
                &mut out
            ),
            Err(EnvelopeError::MissingHistory { envelope: 0 })
        );
        assert!(!history.is_primed(), "拒绝不应推进历史");

        let noise: Vec<i16> = vec![1; usize::from(map.noise_groups())];
        assert_eq!(
            decode(
                &[deltas(&data, false, true)],
                &[deltas(&noise, true, true)],
                &map,
                false,
                &mut history,
                &mut out
            ),
            Err(EnvelopeError::MissingHistory { envelope: 0 }),
            "噪声侧同样不得假定历史"
        );
        assert!(!history.is_primed());
    }

    /// 噪声包络没有分辨率映射，但方向语义与信号一致。
    #[test]
    fn noise_envelopes_chain_without_a_resolution_map() {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let groups = usize::from(map.noise_groups());
        assert!(groups > 0);
        let sig: Vec<i16> = vec![0; usize::from(map.num_high)];
        let first: Vec<i16> = (0..groups).map(|index| index as i16 + 1).collect();
        let step: Vec<i16> = vec![-1; groups];

        let mut history = EnvelopeHistory::new();
        let mut out = EnvelopeScaleFactors::new();
        decode(
            &[deltas(&sig, false, true)],
            &[deltas(&first, false, true), deltas(&step, true, true)],
            &map,
            false,
            &mut history,
            &mut out,
        )
        .expect("噪声包络应成功");

        let expected: Vec<i16> = first
            .iter()
            .scan(0i16, |sum, value| {
                *sum += value;
                Some(*sum)
            })
            .collect();
        for sbg in 0..groups {
            assert_eq!(out.noise(0, sbg), Some(expected[sbg]), "频率方向应是前缀和");
            assert_eq!(
                out.noise(1, sbg),
                Some(expected[sbg] - 1),
                "时间方向应接上一个噪声包络"
            );
        }
    }

    /// 容量与长度不足一律拒绝，且不推进历史。
    #[test]
    fn invalid_input_is_rejected_without_touching_history() {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let groups = usize::from(map.num_high);
        let full: Vec<i16> = vec![1; groups];
        let short: Vec<i16> = vec![1; groups - 1];
        let noise = zero_noise(&map);
        let mut history = EnvelopeHistory::new();
        let mut out = EnvelopeScaleFactors::new();

        assert_eq!(
            decode(
                &[],
                &[deltas(&noise, false, false)],
                &map,
                false,
                &mut history,
                &mut out
            ),
            Err(EnvelopeError::MissingEnvelopes {
                signal: 0,
                noise: 1
            })
        );
        assert_eq!(
            decode(
                &[deltas(&full, false, true)],
                &[],
                &map,
                false,
                &mut history,
                &mut out
            ),
            Err(EnvelopeError::MissingEnvelopes {
                signal: 1,
                noise: 0
            })
        );

        assert_eq!(
            decode(
                &[deltas(&short, false, true)],
                &[deltas(&noise, false, false)],
                &map,
                false,
                &mut history,
                &mut out
            ),
            Err(EnvelopeError::OutputTooSmall {
                needed: groups,
                provided: groups - 1
            })
        );

        let many: Vec<EnvelopeDeltas<'_>> = (0..MAX_ATSG_SIG + 1)
            .map(|_| deltas(&full, false, true))
            .collect();
        assert_eq!(
            decode(
                &many,
                &[deltas(&noise, false, false)],
                &map,
                false,
                &mut history,
                &mut out
            ),
            Err(EnvelopeError::TooManyEnvelopes {
                signal: MAX_ATSG_SIG + 1,
                noise: 1
            })
        );
        assert!(!history.is_primed(), "拒绝不应推进历史");
    }

    /// 规范没有饱和语义；乘法或累加越界都必须失败且不提交历史。
    #[test]
    fn scale_factor_overflow_is_rejected_without_touching_history() {
        let map = SbgIndexMap::derive(&tables()).expect("映射");
        let mut signal_overflow = vec![0; usize::from(map.num_high)];
        signal_overflow[0] = i16::MAX;
        let signal_zero = vec![0; usize::from(map.num_high)];
        let mut noise_overflow = zero_noise(&map);
        noise_overflow[0] = i16::MAX;
        let noise_zero = zero_noise(&map);

        for (signal, noise, kind) in [
            (&signal_overflow, &noise_zero, EnvelopeKind::Signal),
            (&signal_zero, &noise_overflow, EnvelopeKind::Noise),
        ] {
            let mut history = EnvelopeHistory::new();
            let mut out = EnvelopeScaleFactors::new();
            assert_eq!(
                decode(
                    &[deltas(signal, false, true)],
                    &[deltas(noise, false, false)],
                    &map,
                    true,
                    &mut history,
                    &mut out
                ),
                Err(EnvelopeError::ScaleFactorOverflow {
                    kind,
                    envelope: 0,
                    group: 0
                })
            );
            assert!(!history.is_primed(), "溢出不应推进历史");
            assert_eq!(out.counts(), (0, 0), "溢出不应提交部分输出");
        }

        // 另钉住乘法成功但与跨区间历史相加越界的路径。
        let mut history = EnvelopeHistory::new();
        history.sig[0] = i16::MAX;
        history.sig_freq_res = true;
        history.primed = true;
        let mut step = signal_zero;
        step[0] = 1;
        let mut out = EnvelopeScaleFactors::new();
        assert_eq!(
            decode(
                &[deltas(&step, true, true)],
                &[deltas(&noise_zero, false, false)],
                &map,
                false,
                &mut history,
                &mut out
            ),
            Err(EnvelopeError::ScaleFactorOverflow {
                kind: EnvelopeKind::Signal,
                envelope: 0,
                group: 0
            })
        );
        assert_eq!(history.sig[0], i16::MAX, "加法溢出不应改写历史");
        assert_eq!(out.counts(), (0, 0), "加法溢出不应提交部分输出");
    }
}
