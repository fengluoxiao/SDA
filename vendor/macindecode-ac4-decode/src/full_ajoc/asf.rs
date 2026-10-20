//! Full A-JOC 的 ASF 反量化、联合声道矩阵与帧级 IMDCT 前端。
//!
//! 这里不保存整文件 PCM sink。每条物理 substream 只拥有逐声道 overlap，所有
//! 临时谱线、IMDCT 工作区和本帧 PCM 由 decoder 复用；成功结果借用到下一次
//! 可变调用。任一声道失败都会丢弃整条 substream 的 ASF 历史，避免半帧 overlap
//! 被后续帧继承。

use crate::{
    asf::{
        MAX_SFB, MAX_WINDOWS,
        imdct::{
            frame::{ChannelSynthesis, FrameSynthesisError, synthesize},
            transform::ImdctWorkspace,
        },
        reconstruct::{ReconstructError, scale_factors, scale_spectrum, ungroup_spectrum},
        spectrum::MAX_SPECTRAL_LINES,
    },
    channel::{ChannelElement, ChannelMatrixError, MAX_ELEMENT_CHANNELS},
    var_element::MAX_CHANNEL_ELEMENTS,
};
use alloc::{boxed::Box, vec::Vec};
use core::fmt;
use macindecode_ac4_bitstream::topology::MAX_SUBSTREAMS;

const MAX_ASF_CHANNELS: usize = MAX_CHANNEL_ELEMENTS.saturating_mul(MAX_ELEMENT_CHANNELS);

/// ASF 帧前端输入。
#[derive(Debug, Clone, Copy)]
pub(super) struct FullAjocAsfFrameInput<'a> {
    /// 当前语法快照实际写入的声道元素。
    pub(super) elements: &'a [ChannelElement],
    /// TOC 推导的 codec frame length。
    pub(super) frame_length: u16,
    /// 物理 substream 的零基下标。
    pub(super) substream_index: u32,
}

/// ASF 前端内部工作区不变量对应的缓冲。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullAjocAsfBuffer {
    /// 每个元素最多三路的缩放谱线。
    ScaledSpectrum,
    /// 窗口解组后的连续谱线。
    UngroupedSpectrum,
    /// 本帧 planar PCM 的声道槽。
    PcmChannels,
    /// 单路 PCM 的样本缓冲。
    PcmSamples,
    /// 与 PCM 一一对应的逐路观察。
    Observations,
    /// 按传输声道顺序隔离的 IMDCT overlap。
    SynthesisStates,
}

/// 非有限值出现的处理阶段。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullAjocAsfStage {
    /// 反量化和联合声道矩阵完成后的谱线。
    ScaledSpectrum,
    /// 纯排列解组后的谱线。
    UngroupedSpectrum,
    /// 帧级 IMDCT 的 PCM 输出。
    Pcm,
}

/// ASF 帧前端失败的稳定分类。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullAjocAsfErrorKind {
    /// 物理 substream 下标超过固定 topology 容量。
    SubstreamIndexOutOfRange { index: u32, limit: usize },
    /// 输入暴露的声道元素超过 `var_channel_element()` 上界。
    TooManyElements { count: usize, limit: usize },
    /// 某个元素没有 1…3 路受支持声道。
    UnsupportedChannelCount { channels: u8 },
    /// 解析工作区缺少声明声道的窗口布局。
    MissingLayout,
    /// 解析工作区缺少声明声道的量化频谱。
    MissingSpectrum,
    /// codec frame length 不能建立 IMDCT overlap。
    UnsupportedFrameLength { frame_length: u16 },
    /// 同一 substream 未经 reset 改变了 codec frame length。
    FrameLengthChanged { previous: u16, current: u16 },
    /// 绝对标度因子还原失败。
    ScaleFactors(ReconstructError),
    /// 谱线反量化或缩放失败。
    ScaleSpectrum(ReconstructError),
    /// MDCT 域联合声道矩阵失败。
    ChannelMatrix(ChannelMatrixError),
    /// 窗口组谱线解组失败。
    UngroupSpectrum(ReconstructError),
    /// 帧级 IMDCT 或 overlap/add 失败。
    Synthesis(FrameSynthesisError),
    /// 数值路径产生非有限值。
    NonFinite {
        stage: FullAjocAsfStage,
        sample: usize,
    },
    /// 预分配容量与已验证的元素形状不一致。
    WorkspaceInvariant {
        buffer: FullAjocAsfBuffer,
        needed: usize,
        available: usize,
    },
}

/// 带物理位置的 ASF 帧前端错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullAjocAsfError {
    substream_index: u32,
    element_index: Option<usize>,
    channel_index: Option<usize>,
    kind: FullAjocAsfErrorKind,
    nonfinite_samples: usize,
}

impl FullAjocAsfError {
    /// 失败所属的物理 substream。
    #[must_use]
    pub const fn substream_index(&self) -> u32 {
        self.substream_index
    }

    /// 失败所属的声道元素；配置级错误没有该位置。
    #[must_use]
    pub const fn element_index(&self) -> Option<usize> {
        self.element_index
    }

    /// 失败所属的元素内声道；元素级错误没有该位置。
    #[must_use]
    pub const fn channel_index(&self) -> Option<usize> {
        self.channel_index
    }

    /// 稳定的结构化失败分类。
    #[must_use]
    pub const fn kind(&self) -> FullAjocAsfErrorKind {
        self.kind
    }

    /// 数值失败所在阶段检测到的非有限样本总数。
    ///
    /// [`FullAjocAsfErrorKind::NonFinite`] 仍保留首个样本位置用于定位；本字段
    /// 另外保存整路扫描得到的数量，使统计调用方不必把“一次失败”误当成
    /// “一个非有限样本”。其他错误返回 `None`。
    #[must_use]
    pub const fn nonfinite_samples(&self) -> Option<usize> {
        match self.kind {
            FullAjocAsfErrorKind::NonFinite { .. } => Some(self.nonfinite_samples),
            _ => None,
        }
    }
}

impl fmt::Display for FullAjocAsfError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "substream {}", self.substream_index)?;
        if let Some(element) = self.element_index {
            write!(formatter, " element {element}")?;
        }
        if let Some(channel) = self.channel_index {
            write!(formatter, " channel {channel}")?;
        }
        write!(formatter, ": {:?}", self.kind)
    }
}

impl core::error::Error for FullAjocAsfError {}

/// 一路 ASF 重建的数值与守恒观察。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FullAjocAsfChannelObservation {
    element_index: usize,
    channel_index: usize,
    scale_factor_bands: usize,
    scale_factor_min: Option<u8>,
    scale_factor_max: Option<u8>,
    scaled_lines: usize,
    scaled_peak: f32,
    scaled_nonzero: usize,
    ungrouped_lines: usize,
    ungrouped_nonzero: usize,
    ungroup_energy_drift: f64,
    pcm_peak: f32,
    input_silent: bool,
    output_silent: bool,
}

impl FullAjocAsfChannelObservation {
    /// 本路来自当前帧第几个声道元素。
    #[must_use]
    pub const fn element_index(self) -> usize {
        self.element_index
    }

    /// 本路在声道元素内的下标。
    #[must_use]
    pub const fn channel_index(self) -> usize {
        self.channel_index
    }

    /// 成功还原绝对值的标度因子带数。
    #[must_use]
    pub const fn scale_factor_bands(self) -> usize {
        self.scale_factor_bands
    }

    /// 本路标度因子的最小值。
    #[must_use]
    pub const fn scale_factor_min(self) -> Option<u8> {
        self.scale_factor_min
    }

    /// 本路标度因子的最大值。
    #[must_use]
    pub const fn scale_factor_max(self) -> Option<u8> {
        self.scale_factor_max
    }

    /// 反量化后实际覆盖的编码谱线数。
    #[must_use]
    pub const fn scaled_lines(self) -> usize {
        self.scaled_lines
    }

    /// 联合声道矩阵后谱线的绝对值峰值。
    #[must_use]
    pub const fn scaled_peak(self) -> f32 {
        self.scaled_peak
    }

    /// 联合声道矩阵后的非零谱线数。
    #[must_use]
    pub const fn scaled_nonzero(self) -> usize {
        self.scaled_nonzero
    }

    /// 解组后送入帧级 IMDCT 的谱线数。
    #[must_use]
    pub const fn ungrouped_lines(self) -> usize {
        self.ungrouped_lines
    }

    /// 解组后的非零谱线数。
    #[must_use]
    pub const fn ungrouped_nonzero(self) -> usize {
        self.ungrouped_nonzero
    }

    /// 解组前后以 `f64` 累加的最大相对能量偏差。
    #[must_use]
    pub const fn ungroup_energy_drift(self) -> f64 {
        self.ungroup_energy_drift
    }

    /// 本路核心带 PCM 的绝对值峰值。
    #[must_use]
    pub const fn pcm_peak(self) -> f32 {
        self.pcm_peak
    }

    /// 本路送入 IMDCT 的谱线是否全零。
    #[must_use]
    pub const fn input_silent(self) -> bool {
        self.input_silent
    }

    /// 本路当前 PCM 帧是否全零。
    #[must_use]
    pub const fn output_silent(self) -> bool {
        self.output_silent
    }
}

/// 一路借用的 ASF 核心带 PCM。
#[derive(Debug, Clone, Copy)]
pub struct FullAjocAsfPcmChannel<'a> {
    observation: FullAjocAsfChannelObservation,
    samples: &'a [f32],
}

impl<'a> FullAjocAsfPcmChannel<'a> {
    /// 本路 ASF 数值与守恒观察。
    #[must_use]
    pub const fn observation(self) -> FullAjocAsfChannelObservation {
        self.observation
    }

    /// 内部 AC-4 标量尺度的核心带 PCM；Scene 出口再统一归一化。
    #[must_use]
    pub const fn samples(self) -> &'a [f32] {
        self.samples
    }
}

/// 一帧借用的 ASF 核心带 PCM 与逐路观察。
#[derive(Debug)]
pub struct DecodedFullAjocAsfFrame<'a> {
    state: &'a mut FullAjocAsfSubstreamState,
    frame_length: usize,
    pcm: &'a [Vec<f32>],
    observations: &'a [FullAjocAsfChannelObservation],
}

impl<'a> DecodedFullAjocAsfFrame<'a> {
    pub(super) fn reset_state(&mut self) {
        self.state.reset();
    }

    pub(super) const fn pcm(&self) -> &'a [Vec<f32>] {
        self.pcm
    }

    /// 本帧成功重建的核心带声道数。
    #[must_use]
    pub const fn channels(&self) -> usize {
        self.observations.len()
    }

    /// 按传输顺序读取一路核心带 PCM。
    #[must_use]
    pub fn channel(&self, index: usize) -> Option<FullAjocAsfPcmChannel<'a>> {
        Some(FullAjocAsfPcmChannel {
            observation: *self.observations.get(index)?,
            samples: self.pcm.get(index)?.get(..self.frame_length)?,
        })
    }

    /// 取得不含 overlap 所有权的只读 observation。
    #[must_use]
    pub const fn observation(&self) -> FullAjocAsfFrameObservation<'a> {
        FullAjocAsfFrameObservation {
            frame_length: self.frame_length,
            pcm: self.pcm,
            observations: self.observations,
        }
    }
}

/// 最近一次成功重建的 ASF 核心带 PCM 与逐路统计只读视图。
///
/// 即使后续 A-SPX/Full 阶段失败，这份前端 observation 仍可由 decoder 读取；
/// 下一次可变调用或显式 reset 会使其失效。
#[derive(Debug, Clone, Copy)]
pub struct FullAjocAsfFrameObservation<'a> {
    frame_length: usize,
    pcm: &'a [Vec<f32>],
    observations: &'a [FullAjocAsfChannelObservation],
}

impl<'a> FullAjocAsfFrameObservation<'a> {
    /// 本帧成功重建的核心带声道数。
    #[must_use]
    pub const fn channels(self) -> usize {
        self.observations.len()
    }

    /// 按传输顺序读取一路核心带 PCM。
    #[must_use]
    pub fn channel(self, index: usize) -> Option<FullAjocAsfPcmChannel<'a>> {
        Some(FullAjocAsfPcmChannel {
            observation: *self.observations.get(index)?,
            samples: self.pcm.get(index)?.get(..self.frame_length)?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct FullAjocAsfObservationDescriptor {
    substream_index: u32,
    frame_length: usize,
    channels: usize,
}

#[derive(Debug)]
struct FullAjocAsfSubstreamState {
    frame_length: Option<u16>,
    synthesis: Vec<Option<ChannelSynthesis>>,
}

impl FullAjocAsfSubstreamState {
    const fn new() -> Self {
        Self {
            frame_length: None,
            synthesis: Vec::new(),
        }
    }

    fn prepare(&mut self, frame_length: u16) -> Result<(), FullAjocAsfErrorKind> {
        if let Some(previous) = self.frame_length {
            if previous != frame_length {
                return Err(FullAjocAsfErrorKind::FrameLengthChanged {
                    previous,
                    current: frame_length,
                });
            }
        } else {
            if ChannelSynthesis::new(frame_length).is_none() {
                return Err(FullAjocAsfErrorKind::UnsupportedFrameLength { frame_length });
            }
            self.frame_length = Some(frame_length);
        }
        if self.synthesis.len() < MAX_ASF_CHANNELS {
            self.synthesis.resize_with(MAX_ASF_CHANNELS, || None);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.frame_length = None;
        for state in &mut self.synthesis {
            *state = None;
        }
    }

    fn channel_mut(
        &mut self,
        slot: usize,
        frame_length: u16,
    ) -> Result<&mut ChannelSynthesis, FullAjocAsfErrorKind> {
        let available = self.synthesis.len();
        let Some(state) = self.synthesis.get_mut(slot) else {
            return Err(FullAjocAsfErrorKind::WorkspaceInvariant {
                buffer: FullAjocAsfBuffer::SynthesisStates,
                needed: slot.saturating_add(1),
                available,
            });
        };
        if state.is_none() {
            *state = ChannelSynthesis::new(frame_length);
        }
        state
            .as_mut()
            .ok_or(FullAjocAsfErrorKind::UnsupportedFrameLength { frame_length })
    }
}

#[derive(Debug)]
struct FullAjocAsfWorkspace {
    scaled: [Vec<f32>; MAX_ELEMENT_CHANNELS],
    ungrouped: Vec<f32>,
    imdct: Box<ImdctWorkspace>,
    pcm: Vec<Vec<f32>>,
    observations: Vec<FullAjocAsfChannelObservation>,
}

impl FullAjocAsfWorkspace {
    fn new() -> Self {
        Self {
            scaled: [Vec::new(), Vec::new(), Vec::new()],
            ungrouped: Vec::new(),
            imdct: Box::new(ImdctWorkspace::new()),
            pcm: Vec::new(),
            observations: Vec::new(),
        }
    }

    fn ensure(&mut self, frame_length: u16) {
        for scaled in &mut self.scaled {
            if scaled.len() < MAX_SPECTRAL_LINES {
                scaled.resize(MAX_SPECTRAL_LINES, 0.0);
            }
        }
        if self.ungrouped.len() < MAX_SPECTRAL_LINES {
            self.ungrouped.resize(MAX_SPECTRAL_LINES, 0.0);
        }
        if self.pcm.len() < MAX_ASF_CHANNELS {
            self.pcm.resize_with(MAX_ASF_CHANNELS, Vec::new);
        }
        let samples = usize::from(frame_length);
        for pcm in &mut self.pcm {
            // reset 后允许用更短的稳定配置复用容量；QMF 组合入口要求每路切片
            // 的逻辑长度与本帧严格一致，truncate 不会改变容量或缓冲地址。
            pcm.resize(samples, 0.0);
        }
        if self.observations.len() < MAX_ASF_CHANNELS {
            self.observations
                .resize(MAX_ASF_CHANNELS, FullAjocAsfChannelObservation::default());
        }
    }
}

/// [`super::FullAjocDecoder`] 内部拥有的 ASF 前端状态与工作区。
#[derive(Debug)]
pub(super) struct FullAjocAsfDecoder {
    substreams: Vec<FullAjocAsfSubstreamState>,
    workspace: FullAjocAsfWorkspace,
    last_observation: Option<FullAjocAsfObservationDescriptor>,
}

impl FullAjocAsfDecoder {
    pub(super) fn new() -> Self {
        Self {
            substreams: Vec::new(),
            workspace: FullAjocAsfWorkspace::new(),
            last_observation: None,
        }
    }

    pub(super) fn reset_substream(&mut self, substream_index: u32) {
        if self
            .last_observation
            .is_some_and(|last| last.substream_index == substream_index)
        {
            self.last_observation = None;
        }
        let slot = usize::try_from(substream_index).unwrap_or(usize::MAX);
        if let Some(state) = self.substreams.get_mut(slot) {
            state.reset();
        }
    }

    pub(super) fn reset(&mut self) {
        self.last_observation = None;
        for state in &mut self.substreams {
            state.reset();
        }
    }

    pub(super) fn last_observation(&self) -> Option<FullAjocAsfFrameObservation<'_>> {
        let last = self.last_observation?;
        Some(FullAjocAsfFrameObservation {
            frame_length: last.frame_length,
            pcm: self.workspace.pcm.get(..last.channels)?,
            observations: self.workspace.observations.get(..last.channels)?,
        })
    }

    pub(super) fn clear_observation(&mut self) {
        self.last_observation = None;
    }

    pub(super) fn prepare_substream(
        &mut self,
        substream_index: u32,
        frame_length: u16,
    ) -> Result<(), FullAjocAsfError> {
        let slot = usize::try_from(substream_index).unwrap_or(usize::MAX);
        if slot >= MAX_SUBSTREAMS {
            return Err(error(
                substream_index,
                None,
                None,
                FullAjocAsfErrorKind::SubstreamIndexOutOfRange {
                    index: substream_index,
                    limit: MAX_SUBSTREAMS,
                },
            ));
        }
        if self.substreams.len() <= slot {
            self.substreams
                .resize_with(slot.saturating_add(1), FullAjocAsfSubstreamState::new);
        }
        let Some(state) = self.substreams.get_mut(slot) else {
            return Err(error(
                substream_index,
                None,
                None,
                FullAjocAsfErrorKind::WorkspaceInvariant {
                    buffer: FullAjocAsfBuffer::SynthesisStates,
                    needed: slot.saturating_add(1),
                    available: self.substreams.len(),
                },
            ));
        };
        state
            .prepare(frame_length)
            .map_err(|kind| error(substream_index, None, None, kind))?;
        self.workspace.ensure(frame_length);
        Ok(())
    }

    pub(super) fn decode_frame<'decoder>(
        &'decoder mut self,
        input: FullAjocAsfFrameInput<'_>,
    ) -> Result<DecodedFullAjocAsfFrame<'decoder>, FullAjocAsfError> {
        self.last_observation = None;
        if let Err(failure) = self.prepare_substream(input.substream_index, input.frame_length) {
            self.reset_substream(input.substream_index);
            return Err(failure);
        }
        let slot = usize::try_from(input.substream_index).unwrap_or(usize::MAX);
        let Self {
            substreams,
            workspace,
            last_observation,
        } = self;
        let available = substreams.len();
        let Some(state) = substreams.get_mut(slot) else {
            return Err(error(
                input.substream_index,
                None,
                None,
                FullAjocAsfErrorKind::WorkspaceInvariant {
                    buffer: FullAjocAsfBuffer::SynthesisStates,
                    needed: slot.saturating_add(1),
                    available,
                },
            ));
        };
        let channels = match drive_frame(state, workspace, input) {
            Ok(channels) => channels,
            Err(failure) => {
                state.reset();
                return Err(failure);
            }
        };
        let Some(pcm) = workspace.pcm.get(..channels) else {
            state.reset();
            return Err(error(
                input.substream_index,
                None,
                None,
                FullAjocAsfErrorKind::WorkspaceInvariant {
                    buffer: FullAjocAsfBuffer::PcmChannels,
                    needed: channels,
                    available: workspace.pcm.len(),
                },
            ));
        };
        let Some(observations) = workspace.observations.get(..channels) else {
            state.reset();
            return Err(error(
                input.substream_index,
                None,
                None,
                FullAjocAsfErrorKind::WorkspaceInvariant {
                    buffer: FullAjocAsfBuffer::Observations,
                    needed: channels,
                    available: workspace.observations.len(),
                },
            ));
        };
        *last_observation = Some(FullAjocAsfObservationDescriptor {
            substream_index: input.substream_index,
            frame_length: usize::from(input.frame_length),
            channels,
        });
        Ok(DecodedFullAjocAsfFrame {
            state,
            frame_length: usize::from(input.frame_length),
            pcm,
            observations,
        })
    }
}

fn drive_frame(
    state: &mut FullAjocAsfSubstreamState,
    workspace: &mut FullAjocAsfWorkspace,
    input: FullAjocAsfFrameInput<'_>,
) -> Result<usize, FullAjocAsfError> {
    if input.elements.len() > MAX_CHANNEL_ELEMENTS {
        return Err(error(
            input.substream_index,
            None,
            None,
            FullAjocAsfErrorKind::TooManyElements {
                count: input.elements.len(),
                limit: MAX_CHANNEL_ELEMENTS,
            },
        ));
    }

    let mut output = 0usize;
    for (element_index, element) in input.elements.iter().enumerate() {
        let channels = usize::from(element.channels());
        if !(1..=MAX_ELEMENT_CHANNELS).contains(&channels) {
            return Err(error(
                input.substream_index,
                Some(element_index),
                None,
                FullAjocAsfErrorKind::UnsupportedChannelCount {
                    channels: element.channels(),
                },
            ));
        }

        let mut written = [None; MAX_ELEMENT_CHANNELS];
        let mut factor_counts = [0usize; MAX_ELEMENT_CHANNELS];
        let mut factor_min = [None; MAX_ELEMENT_CHANNELS];
        let mut factor_max = [None; MAX_ELEMENT_CHANNELS];
        for channel_index in 0..channels {
            let layout = element.layout(channel_index).ok_or_else(|| {
                error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::MissingLayout,
                )
            })?;
            let spectrum = element.spectrum(channel_index).ok_or_else(|| {
                error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::MissingSpectrum,
                )
            })?;
            let factors = scale_factors(spectrum, layout).map_err(|source| {
                error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::ScaleFactors(source),
                )
            })?;
            let (count, low, high) = factor_summary(&factors);
            if let Some(slot) = factor_counts.get_mut(channel_index) {
                *slot = count;
            }
            if let Some(slot) = factor_min.get_mut(channel_index) {
                *slot = low;
            }
            if let Some(slot) = factor_max.get_mut(channel_index) {
                *slot = high;
            }
            let Some(scaled) = workspace.scaled.get_mut(channel_index) else {
                return Err(error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::WorkspaceInvariant {
                        buffer: FullAjocAsfBuffer::ScaledSpectrum,
                        needed: channel_index.saturating_add(1),
                        available: workspace.scaled.len(),
                    },
                ));
            };
            let count = scale_spectrum(spectrum, layout, &factors, scaled).map_err(|source| {
                error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::ScaleSpectrum(source),
                )
            })?;
            if let Some(slot) = written.get_mut(channel_index) {
                *slot = Some(count);
            }
        }

        {
            let [first, second, third] = &mut workspace.scaled;
            let mut spectra = [
                first.as_mut_slice(),
                second.as_mut_slice(),
                third.as_mut_slice(),
            ];
            element
                .apply_channel_matrix(&mut spectra)
                .map_err(|source| {
                    error(
                        input.substream_index,
                        Some(element_index),
                        None,
                        FullAjocAsfErrorKind::ChannelMatrix(source),
                    )
                })?;
        }

        for channel_index in 0..channels {
            let layout = element.layout(channel_index).ok_or_else(|| {
                error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::MissingLayout,
                )
            })?;
            let Some(scaled_lines) = written.get(channel_index).copied().flatten() else {
                return Err(error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::WorkspaceInvariant {
                        buffer: FullAjocAsfBuffer::ScaledSpectrum,
                        needed: 1,
                        available: 0,
                    },
                ));
            };
            let scaled = workspace
                .scaled
                .get(channel_index)
                .and_then(|values| values.get(..scaled_lines))
                .ok_or_else(|| {
                    error(
                        input.substream_index,
                        Some(element_index),
                        Some(channel_index),
                        FullAjocAsfErrorKind::WorkspaceInvariant {
                            buffer: FullAjocAsfBuffer::ScaledSpectrum,
                            needed: scaled_lines,
                            available: workspace.scaled.get(channel_index).map_or(0, Vec::len),
                        },
                    )
                })?;
            let scaled_summary = signal_summary(scaled);
            if let Some(sample) = scaled_summary.first_nonfinite {
                return Err(nonfinite_error(
                    input.substream_index,
                    element_index,
                    channel_index,
                    FullAjocAsfStage::ScaledSpectrum,
                    sample,
                    scaled_summary.nonfinite,
                ));
            }
            let ungrouped_lines = ungroup_spectrum(layout, scaled, &mut workspace.ungrouped)
                .map_err(|source| {
                    error(
                        input.substream_index,
                        Some(element_index),
                        Some(channel_index),
                        FullAjocAsfErrorKind::UngroupSpectrum(source),
                    )
                })?;
            let ungrouped = workspace.ungrouped.get(..ungrouped_lines).ok_or_else(|| {
                error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::WorkspaceInvariant {
                        buffer: FullAjocAsfBuffer::UngroupedSpectrum,
                        needed: ungrouped_lines,
                        available: workspace.ungrouped.len(),
                    },
                )
            })?;
            let ungrouped_summary = signal_summary(ungrouped);
            if let Some(sample) = ungrouped_summary.first_nonfinite {
                return Err(nonfinite_error(
                    input.substream_index,
                    element_index,
                    channel_index,
                    FullAjocAsfStage::UngroupedSpectrum,
                    sample,
                    ungrouped_summary.nonfinite,
                ));
            }

            let synthesis = state
                // overlap 按最终传输声道顺序绑定，而不是按元素槽绑定：逐帧的
                // `var_coding_config` 可以在三声道与 2+1 分组间切换。
                .channel_mut(output, input.frame_length)
                .map_err(|kind| {
                    error(
                        input.substream_index,
                        Some(element_index),
                        Some(channel_index),
                        kind,
                    )
                })?;
            let available = workspace.pcm.len();
            let pcm = workspace.pcm.get_mut(output).ok_or_else(|| {
                error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::WorkspaceInvariant {
                        buffer: FullAjocAsfBuffer::PcmChannels,
                        needed: output.saturating_add(1),
                        available,
                    },
                )
            })?;
            let frame_length = usize::from(input.frame_length);
            let pcm_available = pcm.len();
            let target = pcm.get_mut(..frame_length).ok_or_else(|| {
                error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::WorkspaceInvariant {
                        buffer: FullAjocAsfBuffer::PcmSamples,
                        needed: frame_length,
                        available: pcm_available,
                    },
                )
            })?;
            synthesize(
                synthesis,
                ungrouped,
                layout,
                workspace.imdct.as_mut(),
                target,
            )
            .map_err(|source| {
                error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::Synthesis(source),
                )
            })?;
            let pcm_summary = signal_summary(target);
            if let Some(sample) = pcm_summary.first_nonfinite {
                return Err(nonfinite_error(
                    input.substream_index,
                    element_index,
                    channel_index,
                    FullAjocAsfStage::Pcm,
                    sample,
                    pcm_summary.nonfinite,
                ));
            }

            let observation = FullAjocAsfChannelObservation {
                element_index,
                channel_index,
                scale_factor_bands: factor_counts.get(channel_index).copied().unwrap_or(0),
                scale_factor_min: factor_min.get(channel_index).copied().flatten(),
                scale_factor_max: factor_max.get(channel_index).copied().flatten(),
                scaled_lines,
                scaled_peak: scaled_summary.peak,
                scaled_nonzero: scaled_summary.nonzero,
                ungrouped_lines,
                ungrouped_nonzero: ungrouped_summary.nonzero,
                ungroup_energy_drift: relative_energy_drift(
                    scaled_summary.energy,
                    ungrouped_summary.energy,
                ),
                pcm_peak: pcm_summary.peak,
                input_silent: ungrouped_summary.nonzero == 0,
                output_silent: pcm_summary.nonzero == 0,
            };
            let observation_available = workspace.observations.len();
            let Some(slot) = workspace.observations.get_mut(output) else {
                return Err(error(
                    input.substream_index,
                    Some(element_index),
                    Some(channel_index),
                    FullAjocAsfErrorKind::WorkspaceInvariant {
                        buffer: FullAjocAsfBuffer::Observations,
                        needed: output.saturating_add(1),
                        available: observation_available,
                    },
                ));
            };
            *slot = observation;
            output = output.saturating_add(1);
        }
    }
    Ok(output)
}

fn factor_summary(
    factors: &crate::asf::reconstruct::ScaleFactors,
) -> (usize, Option<u8>, Option<u8>) {
    let mut count = 0usize;
    let mut low: Option<u8> = None;
    let mut high: Option<u8> = None;
    for group in 0..MAX_WINDOWS {
        for sfb in 0..MAX_SFB {
            let Some(value) = factors.get(group, sfb) else {
                continue;
            };
            count = count.saturating_add(1);
            low = Some(low.map_or(value, |current| current.min(value)));
            high = Some(high.map_or(value, |current| current.max(value)));
        }
    }
    (count, low, high)
}

#[derive(Debug, Clone, Copy)]
struct SignalSummary {
    peak: f32,
    nonzero: usize,
    energy: f64,
    first_nonfinite: Option<usize>,
    nonfinite: usize,
}

fn signal_summary(values: &[f32]) -> SignalSummary {
    let mut out = SignalSummary {
        peak: 0.0,
        nonzero: 0,
        energy: 0.0,
        first_nonfinite: None,
        nonfinite: 0,
    };
    for (index, value) in values.iter().copied().enumerate() {
        if !value.is_finite() {
            out.first_nonfinite.get_or_insert(index);
            out.nonfinite = out.nonfinite.saturating_add(1);
            continue;
        }
        out.peak = out.peak.max(value.abs());
        out.nonzero = out.nonzero.saturating_add(usize::from(value != 0.0));
        out.energy += f64::from(value) * f64::from(value);
    }
    out
}

fn relative_energy_drift(before: f64, after: f64) -> f64 {
    if before > 0.0 {
        (after - before).abs() / before
    } else {
        0.0
    }
}

const fn error(
    substream_index: u32,
    element_index: Option<usize>,
    channel_index: Option<usize>,
    kind: FullAjocAsfErrorKind,
) -> FullAjocAsfError {
    FullAjocAsfError {
        substream_index,
        element_index,
        channel_index,
        kind,
        nonfinite_samples: 0,
    }
}

const fn nonfinite_error(
    substream_index: u32,
    element_index: usize,
    channel_index: usize,
    stage: FullAjocAsfStage,
    sample: usize,
    nonfinite_samples: usize,
) -> FullAjocAsfError {
    FullAjocAsfError {
        substream_index,
        element_index: Some(element_index),
        channel_index: Some(channel_index),
        kind: FullAjocAsfErrorKind::NonFinite { stage, sample },
        nonfinite_samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{channel::ChannelContext, huffman::tables::ASF_HCB_1, testutil::BitBuf};
    use macindecode_ac4_bitstream::reader::BitReader;

    fn mono_element() -> ChannelElement {
        let mut payload = BitBuf::new();
        payload.push_mono_data(2);
        let mut element = ChannelElement::new();
        element
            .parse_mono_data(
                &mut BitReader::new(payload.as_slice()),
                ChannelContext {
                    frame_len_base: 2048,
                    sampling_frequency_hz: 48_000,
                },
                false,
            )
            .expect("单声道 ASF 夹具应能解析");
        element
    }

    fn nonzero_mono_element() -> ChannelElement {
        let mut payload = BitBuf::new();
        payload.push(false); // spec_frontend = ASF
        payload.push_long_sf_info(1);
        payload.push_bits(1, 4); // codebook 1
        payload.push_bits(0, 5); // 一个频带
        payload.push_symbol(&ASF_HCB_1, 0);
        payload.push_bits(100, 8); // unity scale factor
        payload.push(false); // b_snf_data_exists

        let mut reader = BitReader::new(payload.as_slice());
        let mut element = ChannelElement::new();
        element
            .parse_mono_data(
                &mut reader,
                ChannelContext {
                    frame_len_base: 2048,
                    sampling_frequency_hz: 48_000,
                },
                false,
            )
            .expect("非零单声道 ASF 夹具应能解析");
        assert_eq!(reader.bit_position(), payload.bit_len() as u64);
        element
    }

    fn input(elements: &[ChannelElement], frame_length: u16) -> FullAjocAsfFrameInput<'_> {
        FullAjocAsfFrameInput {
            elements,
            frame_length,
            substream_index: 0,
        }
    }

    type WorkspaceLayout = [(*const u8, usize); 10];

    fn workspace_layout(decoder: &FullAjocAsfDecoder) -> WorkspaceLayout {
        let [first, second, third] = &decoder.workspace.scaled;
        let first_pcm = decoder.workspace.pcm.first();
        let synthesis = decoder.substreams.first().map(|state| &state.synthesis);
        [
            (first.as_ptr().cast(), first.capacity()),
            (second.as_ptr().cast(), second.capacity()),
            (third.as_ptr().cast(), third.capacity()),
            (
                decoder.workspace.ungrouped.as_ptr().cast(),
                decoder.workspace.ungrouped.capacity(),
            ),
            (
                decoder.workspace.pcm.as_ptr().cast(),
                decoder.workspace.pcm.capacity(),
            ),
            (
                first_pcm.map_or(core::ptr::null(), |pcm| pcm.as_ptr().cast()),
                first_pcm.map_or(0, Vec::capacity),
            ),
            (
                decoder.workspace.observations.as_ptr().cast(),
                decoder.workspace.observations.capacity(),
            ),
            (
                decoder.substreams.as_ptr().cast(),
                decoder.substreams.capacity(),
            ),
            (
                synthesis.map_or(core::ptr::null(), |states| states.as_ptr().cast()),
                synthesis.map_or(0, Vec::capacity),
            ),
            (
                core::ptr::from_ref(decoder.workspace.imdct.as_ref()).cast(),
                1,
            ),
        ]
    }

    #[test]
    fn frontend_returns_borrowed_planar_pcm_and_reuses_buffers() {
        let elements = [mono_element()];
        let mut decoder = super::super::FullAjocDecoder::new();
        decoder
            .prepare_asf_substream(0, 2048)
            .expect("配置期应能预分配 ASF 前端");

        let first = {
            let decoded = decoder
                .decode_asf_frame(input(&elements, 2048))
                .expect("首帧应能重建");
            assert_eq!(decoded.channels(), 1);
            let channel = decoded.channel(0).expect("应有一路 PCM");
            assert_eq!(channel.samples().len(), 2048);
            assert!(channel.samples().iter().all(|sample| *sample == 0.0));
            let observation = channel.observation();
            assert_eq!(observation.element_index(), 0);
            assert_eq!(observation.channel_index(), 0);
            assert_eq!(observation.scaled_lines(), 8);
            assert_eq!(observation.ungrouped_lines(), 2048);
            assert!(observation.input_silent());
            assert!(observation.output_silent());
            channel.samples().as_ptr()
        };

        let second = decoder
            .decode_asf_frame(input(&elements, 2048))
            .expect("次帧应能重建")
            .channel(0)
            .expect("应有一路 PCM")
            .samples()
            .as_ptr();
        assert_eq!(second, first, "稳定解码不得更换帧 PCM 缓冲");
    }

    #[test]
    fn configuration_preallocation_covers_every_stable_asf_buffer() {
        let elements = [nonzero_mono_element()];
        let mut decoder = FullAjocAsfDecoder::new();
        decoder
            .prepare_substream(0, 2048)
            .expect("配置建立时应预分配 ASF 工作区");
        let layout = workspace_layout(&decoder);

        {
            let decoded = decoder
                .decode_frame(input(&elements, 2048))
                .expect("首帧应能重建");
            assert_eq!(decoded.channels(), 1);
        }
        assert_eq!(workspace_layout(&decoder), layout);
        {
            let decoded = decoder
                .decode_frame(input(&elements, 2048))
                .expect("稳定次帧应能重建");
            assert_eq!(decoded.channels(), 1);
        }
        assert_eq!(
            workspace_layout(&decoder),
            layout,
            "稳定解码不得扩容或搬移 ASF 工作区"
        );
    }

    #[test]
    fn frame_length_change_discards_history_before_retry() {
        let elements = [mono_element()];
        let mut decoder = super::super::FullAjocDecoder::new();
        decoder
            .decode_asf_frame(input(&elements, 2048))
            .expect("首帧应能建立 overlap");

        let failure = decoder
            .decode_asf_frame(input(&elements, 1920))
            .expect_err("未经 reset 改帧长必须失败");
        assert_eq!(
            failure.kind(),
            FullAjocAsfErrorKind::FrameLengthChanged {
                previous: 2048,
                current: 1920,
            }
        );
        decoder
            .decode_asf_frame(input(&elements, 2048))
            .expect("失败已丢弃历史，原配置可从新首帧重试");
    }

    #[test]
    fn frontend_observes_the_reconstructed_nonzero_signal() {
        let elements = [nonzero_mono_element()];
        let mut decoder = super::super::FullAjocDecoder::new();
        let decoded = decoder
            .decode_asf_frame(input(&elements, 2048))
            .expect("非零谱应能重建");
        let channel = decoded.channel(0).expect("应有一路 PCM");
        let observation = channel.observation();

        assert_eq!(observation.scale_factor_bands(), 1);
        assert_eq!(observation.scale_factor_min(), Some(100));
        assert_eq!(observation.scale_factor_max(), Some(100));
        assert_eq!(observation.scaled_lines(), 4);
        assert_eq!(observation.scaled_peak(), 1.0);
        assert_eq!(observation.scaled_nonzero(), 4);
        assert_eq!(observation.ungrouped_nonzero(), 4);
        assert_eq!(observation.ungroup_energy_drift(), 0.0);
        assert!(!observation.input_silent());
        assert!(channel.samples().iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn nonfinite_error_keeps_the_first_location_and_total_count() {
        let summary = signal_summary(&[1.0, f32::NAN, f32::INFINITY, -2.0, f32::NEG_INFINITY]);
        assert_eq!(summary.first_nonfinite, Some(1));
        assert_eq!(summary.nonfinite, 3);

        let failure = nonfinite_error(
            2,
            3,
            1,
            FullAjocAsfStage::Pcm,
            summary.first_nonfinite.unwrap_or(usize::MAX),
            summary.nonfinite,
        );
        assert_eq!(
            failure.kind(),
            FullAjocAsfErrorKind::NonFinite {
                stage: FullAjocAsfStage::Pcm,
                sample: 1,
            }
        );
        assert_eq!(failure.nonfinite_samples(), Some(3));
        assert_eq!(
            error(2, Some(3), Some(1), FullAjocAsfErrorKind::MissingLayout).nonfinite_samples(),
            None
        );
    }

    #[test]
    fn public_substream_reset_discards_asf_overlap() {
        let elements = [nonzero_mono_element()];
        let mut decoder = super::super::FullAjocDecoder::new();
        let first: Vec<u32> = decoder
            .decode_asf_frame(input(&elements, 2048))
            .expect("首帧应能重建")
            .channel(0)
            .expect("应有一路 PCM")
            .samples()
            .iter()
            .map(|sample| sample.to_bits())
            .collect();
        let continued: Vec<u32> = decoder
            .decode_asf_frame(input(&elements, 2048))
            .expect("连续帧应能重建")
            .channel(0)
            .expect("应有一路 PCM")
            .samples()
            .iter()
            .map(|sample| sample.to_bits())
            .collect();
        assert_ne!(continued, first, "夹具必须能观察到 overlap 延续");

        decoder.reset_substream(0);
        let restarted: Vec<u32> = decoder
            .decode_asf_frame(input(&elements, 2048))
            .expect("reset 后应从新首帧重建")
            .channel(0)
            .expect("应有一路 PCM")
            .samples()
            .iter()
            .map(|sample| sample.to_bits())
            .collect();
        assert_eq!(restarted, first, "reset 必须清空 ASF overlap");
    }

    #[test]
    fn out_of_range_substream_is_rejected_before_workspace_growth() {
        let elements = [mono_element()];
        let mut decoder = FullAjocAsfDecoder::new();
        let failure = decoder
            .decode_frame(FullAjocAsfFrameInput {
                elements: &elements,
                frame_length: 2048,
                substream_index: u32::try_from(MAX_SUBSTREAMS).unwrap_or(u32::MAX),
            })
            .expect_err("容量外下标必须失败");
        assert!(matches!(
            failure.kind(),
            FullAjocAsfErrorKind::SubstreamIndexOutOfRange { .. }
        ));
        assert!(decoder.substreams.is_empty());
        assert!(decoder.workspace.pcm.is_empty());
    }
}
