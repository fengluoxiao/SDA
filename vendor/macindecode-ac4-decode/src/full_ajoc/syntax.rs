//! Full A-JOC 音频语法的有状态、无 sink 帧入口。
//!
//! `parse_substream_ajoc()` 本身由调用方提供状态与六组工作区；CLI survey 过去
//! 因而同时拥有了解码状态和统计职责。本层把这些所有权收回 decoder：稳定配置
//! 下复用同一批缓冲。输入切片耗尽时保留整条物理 substream 的已提交状态，供
//! 调用方补全后重试；其他失败会使其失效。成功结果只借用到下一次可变调用。

use super::{AspxBlocker, FullAjocBlocker, SupportedAjocFullFrame, SupportedAspxFrame};
use crate::{
    ajoc::{AjocObjectControl, AjocObjectMatrix},
    aspx::syntax::{AspxConfig, AspxData},
    audio_data::AudioDataState,
    channel::ChannelElement,
    substream_audio::{
        Ac4SubstreamAjoc, AjocAudioWorkspace, AjocSubstreamContext, SubstreamAudioError,
        parse_substream_ajoc,
    },
    var_element::{MAX_ASPX_ELEMENTS, MAX_CHANNEL_ELEMENTS},
};
use alloc::vec::Vec;
use core::fmt;
use macindecode_ac4_bitstream::{
    audio_substream::AudioSubstreamError,
    emdf::EmdfError,
    oamd::{MAX_OBJ_INFO_BLOCKS, OamdError, OamdMetadataBlock},
    reader::ReadError,
    topology::MAX_SUBSTREAMS,
};

/// Full 音频语法帧的输入。
#[derive(Debug, Clone, Copy)]
pub struct FullAjocSyntaxFrameInput<'a> {
    /// 由 topology 精确定位的一条 A-JOC `ac4_substream()` 载荷。
    pub payload: &'a [u8],
    /// 同一 TOC、substream info 与 group OAMD 时间推导出的解析上下文。
    pub context: AjocSubstreamContext,
    /// 物理 substream 的零基下标。
    pub substream_index: u32,
    /// 调用方当前输出范围内实际参与解码的物理 A-JOC substream 数。
    ///
    /// 全帧巡检应传入该 raw frame 的总数；已经完成 presentation 选择的会话应只
    /// 传入所选 presentation 引用的数量，不能把未选 presentation 的独立音频源
    /// 混入当前输出门禁。
    pub physical_substreams: usize,
}

/// 音频语法工作区中发生不一致的缓冲类别。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullAjocSyntaxBuffer {
    ChannelElements,
    AspxElements,
    ObjectControls,
    Matrices,
    CoreOamdBlocks,
    FullOamdBlocks,
}

impl fmt::Display for FullAjocSyntaxBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ChannelElements => "channel elements",
            Self::AspxElements => "A-SPX elements",
            Self::ObjectControls => "A-JOC object controls",
            Self::Matrices => "A-JOC matrices",
            Self::CoreOamdBlocks => "core OAMD blocks",
            Self::FullOamdBlocks => "full OAMD blocks",
        })
    }
}

/// Full A-JOC 音频语法入口的结构化失败。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullAjocSyntaxError {
    /// 物理 substream 下标超出固定 topology 容量。
    SubstreamIndexOutOfRange { index: u32, limit: usize },
    /// `ac4_substream()` 或 `audio_data_ajoc()` 解析失败。
    Decode {
        substream_index: u32,
        error: SubstreamAudioError,
    },
    /// 预分配工作区与成功解析摘要不一致，属于内部不变量失败。
    WorkspaceInvariant {
        buffer: FullAjocSyntaxBuffer,
        needed: usize,
        available: usize,
    },
}

impl fmt::Display for FullAjocSyntaxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::SubstreamIndexOutOfRange { index, limit } => {
                write!(
                    formatter,
                    "Substream {index} exceeds audio-syntax state capacity {limit}"
                )
            }
            Self::Decode {
                substream_index,
                error,
            } => write!(formatter, "Substream {substream_index}: {error}"),
            Self::WorkspaceInvariant {
                buffer,
                needed,
                available,
            } => write!(
                formatter,
                "{buffer} workspace requires {needed} entries, but only {available} were preallocated"
            ),
        }
    }
}

impl FullAjocSyntaxError {
    /// 当前失败是否由外层 `ac4_substream()` 切片在框架完成前耗尽导致。
    ///
    /// 这覆盖 `audio_size` 超过当前切片、框架或 metadata 解析返回的
    /// [`ReadError::OutOfBounds`]，以及内嵌 EMDF 尚未读到完整终止符。
    /// `audio_data_ajoc()` 的读取器已经限制在声明的 `audio_size` 区段内；其内部越界说明
    /// 区段或语法损坏，追加切片尾部不能恢复。是否能把同一 AU 补全后重试，仍由掌握
    /// 外层定界信息的调用方决定。
    #[must_use]
    pub fn is_input_exhausted(&self) -> bool {
        match self {
            Self::Decode { error, .. } => substream_input_exhausted(error),
            _ => false,
        }
    }
}

impl core::error::Error for FullAjocSyntaxError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Decode { error, .. } => Some(error),
            _ => None,
        }
    }
}

fn substream_input_exhausted(error: &SubstreamAudioError) -> bool {
    match error {
        SubstreamAudioError::Substream(error) => audio_substream_input_exhausted(error),
        _ => false,
    }
}

fn audio_substream_input_exhausted(error: &AudioSubstreamError) -> bool {
    match error {
        AudioSubstreamError::Read(error) => read_input_exhausted(error),
        AudioSubstreamError::Emdf(error) => emdf_input_exhausted(error),
        AudioSubstreamError::Oamd(error) => oamd_input_exhausted(error),
        AudioSubstreamError::AudioSizeOutOfRange { .. } => true,
        _ => false,
    }
}

fn oamd_input_exhausted(error: &OamdError) -> bool {
    matches!(error, OamdError::Read(error) if read_input_exhausted(error))
}

fn emdf_input_exhausted(error: &EmdfError) -> bool {
    match error {
        EmdfError::Read(error) => read_input_exhausted(error),
        EmdfError::MissingTerminator { .. } => true,
        EmdfError::TooManyPayloads { .. } | EmdfError::PayloadTooLarge { .. } => false,
    }
}

fn read_input_exhausted(error: &ReadError) -> bool {
    matches!(error, ReadError::OutOfBounds { .. })
}

/// 一帧成功解析出的借用快照。
///
/// `parsed` 是定长摘要；大体积 ASF 频谱、A-SPX 控制、矩阵与 OAMD 块仍由
/// decoder 工作区拥有。所有切片都精确裁到本帧实际写入的长度。
#[derive(Debug)]
pub struct DecodedFullAjocSyntaxFrame<'a> {
    state: &'a mut AudioDataState,
    parsed: Ac4SubstreamAjoc,
    context: AjocSubstreamContext,
    elements: &'a [ChannelElement],
    aspx: &'a [AspxData],
    aspx_config: Option<AspxConfig>,
    object_controls: &'a [AjocObjectControl],
    matrices: &'a [AjocObjectMatrix],
    dmx_blocks: &'a [OamdMetadataBlock],
    umx_blocks: &'a [OamdMetadataBlock],
    aspx_support: Result<SupportedAspxFrame, AspxBlocker>,
    full_support: Result<SupportedAjocFullFrame, FullAjocBlocker>,
}

impl<'a> DecodedFullAjocSyntaxFrame<'a> {
    pub(super) fn reset_state(&mut self) {
        *self.state = AudioDataState::new();
    }

    #[must_use]
    pub const fn parsed(&self) -> Ac4SubstreamAjoc {
        self.parsed
    }

    #[must_use]
    pub const fn context(&self) -> AjocSubstreamContext {
        self.context
    }

    #[must_use]
    pub const fn elements(&self) -> &'a [ChannelElement] {
        self.elements
    }

    #[must_use]
    pub const fn aspx(&self) -> &'a [AspxData] {
        self.aspx
    }

    /// 当前帧解析完成后生效的 `aspx_config()`，非 I 帧沿用前一 I 帧的值。
    #[must_use]
    pub const fn aspx_config(&self) -> Option<AspxConfig> {
        self.aspx_config
    }

    #[must_use]
    pub const fn object_controls(&self) -> &'a [AjocObjectControl] {
        self.object_controls
    }

    #[must_use]
    pub const fn matrices(&self) -> &'a [AjocObjectMatrix] {
        self.matrices
    }

    #[must_use]
    pub const fn dmx_blocks(&self) -> &'a [OamdMetadataBlock] {
        self.dmx_blocks
    }

    #[must_use]
    pub const fn umx_blocks(&self) -> &'a [OamdMetadataBlock] {
        self.umx_blocks
    }

    pub const fn aspx_support(&self) -> Result<SupportedAspxFrame, AspxBlocker> {
        self.aspx_support
    }

    pub const fn full_support(&self) -> Result<SupportedAjocFullFrame, FullAjocBlocker> {
        self.full_support
    }

    /// 取得不含可变语法状态所有权的只读 observation。
    ///
    /// 该视图与当前借用帧具有相同生命周期，可交给只做统计的调用方；需要在
    /// Full 下游失败后读取同一前端时，使用
    /// [`super::FullAjocDecoder::last_syntax_observation`]。
    #[must_use]
    pub const fn observation(&self) -> FullAjocSyntaxObservation<'a> {
        FullAjocSyntaxObservation {
            parsed: self.parsed,
            context: self.context,
            elements: self.elements,
            aspx: self.aspx,
            aspx_config: self.aspx_config,
            object_controls: self.object_controls,
            matrices: self.matrices,
            dmx_blocks: self.dmx_blocks,
            umx_blocks: self.umx_blocks,
            aspx_support: self.aspx_support,
            full_support: self.full_support,
        }
    }
}

/// 最近一次成功解析的 Full A-JOC 语法、控制与 raw OAMD 只读视图。
///
/// Full DSP 可以在语法与 ASF 已经成功后因未支持分支或重建错误而失败。decoder
/// 仍保留这份只读 observation，使 trace/census 不必为失败路径再维护第二套解析
/// 状态。视图借用 decoder 工作区，仅在下一次可变调用前有效。
#[derive(Debug, Clone, Copy)]
pub struct FullAjocSyntaxObservation<'a> {
    parsed: Ac4SubstreamAjoc,
    context: AjocSubstreamContext,
    elements: &'a [ChannelElement],
    aspx: &'a [AspxData],
    aspx_config: Option<AspxConfig>,
    object_controls: &'a [AjocObjectControl],
    matrices: &'a [AjocObjectMatrix],
    dmx_blocks: &'a [OamdMetadataBlock],
    umx_blocks: &'a [OamdMetadataBlock],
    aspx_support: Result<SupportedAspxFrame, AspxBlocker>,
    full_support: Result<SupportedAjocFullFrame, FullAjocBlocker>,
}

impl<'a> FullAjocSyntaxObservation<'a> {
    #[must_use]
    pub const fn parsed(self) -> Ac4SubstreamAjoc {
        self.parsed
    }

    #[must_use]
    pub const fn context(self) -> AjocSubstreamContext {
        self.context
    }

    #[must_use]
    pub const fn elements(self) -> &'a [ChannelElement] {
        self.elements
    }

    #[must_use]
    pub const fn aspx(self) -> &'a [AspxData] {
        self.aspx
    }

    #[must_use]
    pub const fn aspx_config(self) -> Option<AspxConfig> {
        self.aspx_config
    }

    #[must_use]
    pub const fn object_controls(self) -> &'a [AjocObjectControl] {
        self.object_controls
    }

    #[must_use]
    pub const fn matrices(self) -> &'a [AjocObjectMatrix] {
        self.matrices
    }

    #[must_use]
    pub const fn dmx_blocks(self) -> &'a [OamdMetadataBlock] {
        self.dmx_blocks
    }

    #[must_use]
    pub const fn umx_blocks(self) -> &'a [OamdMetadataBlock] {
        self.umx_blocks
    }

    pub const fn aspx_support(self) -> Result<SupportedAspxFrame, AspxBlocker> {
        self.aspx_support
    }

    pub const fn full_support(self) -> Result<SupportedAjocFullFrame, FullAjocBlocker> {
        self.full_support
    }
}

#[derive(Debug, Clone, Copy)]
struct FullAjocSyntaxObservationDescriptor {
    substream_index: u32,
    parsed: Ac4SubstreamAjoc,
    context: AjocSubstreamContext,
    element_count: usize,
    aspx_count: usize,
    aspx_config: Option<AspxConfig>,
    control_count: usize,
    dmx_block_count: usize,
    umx_block_count: usize,
    aspx_support: Result<SupportedAspxFrame, AspxBlocker>,
    full_support: Result<SupportedAjocFullFrame, FullAjocBlocker>,
}

#[derive(Debug)]
struct FullAjocSyntaxWorkspace {
    elements: Vec<ChannelElement>,
    aspx: Vec<AspxData>,
    controls: Vec<AjocObjectControl>,
    matrices: Vec<AjocObjectMatrix>,
    dmx_blocks: Vec<OamdMetadataBlock>,
    umx_blocks: Vec<OamdMetadataBlock>,
}

impl FullAjocSyntaxWorkspace {
    const fn new() -> Self {
        Self {
            elements: Vec::new(),
            aspx: Vec::new(),
            controls: Vec::new(),
            matrices: Vec::new(),
            dmx_blocks: Vec::new(),
            umx_blocks: Vec::new(),
        }
    }

    fn ensure(&mut self, context: &AjocSubstreamContext) {
        if self.elements.len() < MAX_CHANNEL_ELEMENTS {
            self.elements
                .resize_with(MAX_CHANNEL_ELEMENTS, ChannelElement::new);
        }
        if self.aspx.len() < MAX_ASPX_ELEMENTS {
            self.aspx.resize_with(MAX_ASPX_ELEMENTS, AspxData::empty);
        }

        // `ObjectDescriptors` 已在 context 推导时核对固定容量；从同一份描述符
        // 反推 usize 数量，避免窄 usize 目标上把转换失败误作超大扩容请求。
        let controls = context
            .umx_objects
            .as_slice()
            .len()
            .saturating_sub(usize::from(context.params.b_lfe));
        if self.controls.len() < controls {
            self.controls
                .resize_with(controls, AjocObjectControl::default);
        }
        if self.matrices.len() < controls {
            self.matrices.resize_with(controls, AjocObjectMatrix::new);
        }

        let dmx_blocks = context
            .dmx_objects
            .as_slice()
            .len()
            .saturating_mul(MAX_OBJ_INFO_BLOCKS);
        if self.dmx_blocks.len() < dmx_blocks {
            self.dmx_blocks
                .resize_with(dmx_blocks, OamdMetadataBlock::default);
        }
        let umx_blocks = context
            .umx_objects
            .as_slice()
            .len()
            .saturating_mul(MAX_OBJ_INFO_BLOCKS);
        if self.umx_blocks.len() < umx_blocks {
            self.umx_blocks
                .resize_with(umx_blocks, OamdMetadataBlock::default);
        }
    }

    fn borrow(&mut self) -> AjocAudioWorkspace<'_> {
        AjocAudioWorkspace {
            elements: &mut self.elements,
            aspx: &mut self.aspx,
            controls: &mut self.controls,
            matrices: &mut self.matrices,
            dmx_blocks: &mut self.dmx_blocks,
            umx_blocks: &mut self.umx_blocks,
        }
    }
}

/// [`super::FullAjocDecoder`] 内部拥有的音频语法状态与工作区。
#[derive(Debug)]
pub(super) struct FullAjocSyntaxDecoder {
    states: Vec<AudioDataState>,
    workspace: FullAjocSyntaxWorkspace,
    last_observation: Option<FullAjocSyntaxObservationDescriptor>,
}

impl FullAjocSyntaxDecoder {
    pub(super) const fn new() -> Self {
        Self {
            states: Vec::new(),
            workspace: FullAjocSyntaxWorkspace::new(),
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
        let index = usize::try_from(substream_index).unwrap_or(usize::MAX);
        if let Some(state) = self.states.get_mut(index) {
            *state = AudioDataState::new();
        }
    }

    pub(super) fn reset(&mut self) {
        self.last_observation = None;
        self.states.fill(AudioDataState::new());
    }

    pub(super) fn last_observation(&self) -> Option<FullAjocSyntaxObservation<'_>> {
        let last = self.last_observation?;
        Some(FullAjocSyntaxObservation {
            parsed: last.parsed,
            context: last.context,
            elements: self.workspace.elements.get(..last.element_count)?,
            aspx: self.workspace.aspx.get(..last.aspx_count)?,
            aspx_config: last.aspx_config,
            object_controls: self.workspace.controls.get(..last.control_count)?,
            matrices: self.workspace.matrices.get(..last.control_count)?,
            dmx_blocks: self.workspace.dmx_blocks.get(..last.dmx_block_count)?,
            umx_blocks: self.workspace.umx_blocks.get(..last.umx_block_count)?,
            aspx_support: last.aspx_support,
            full_support: last.full_support,
        })
    }

    pub(super) fn clear_observation(&mut self) {
        self.last_observation = None;
    }

    pub(super) fn prepare_substream(
        &mut self,
        substream_index: u32,
        context: &AjocSubstreamContext,
    ) -> Result<(), FullAjocSyntaxError> {
        let slot = usize::try_from(substream_index).unwrap_or(usize::MAX);
        if slot >= MAX_SUBSTREAMS {
            return Err(FullAjocSyntaxError::SubstreamIndexOutOfRange {
                index: substream_index,
                limit: MAX_SUBSTREAMS,
            });
        }
        self.workspace.ensure(context);
        if self.states.len() <= slot {
            self.states
                .resize(slot.saturating_add(1), AudioDataState::new());
        }
        Ok(())
    }

    pub(super) fn decode_frame<'decoder>(
        &'decoder mut self,
        input: FullAjocSyntaxFrameInput<'_>,
    ) -> Result<DecodedFullAjocSyntaxFrame<'decoder>, FullAjocSyntaxError> {
        self.decode_frame_with_policy(input, true)
    }

    pub(super) fn decode_complete_frame<'decoder>(
        &'decoder mut self,
        input: FullAjocSyntaxFrameInput<'_>,
    ) -> Result<DecodedFullAjocSyntaxFrame<'decoder>, FullAjocSyntaxError> {
        self.decode_frame_with_policy(input, false)
    }

    fn decode_frame_with_policy<'decoder>(
        &'decoder mut self,
        input: FullAjocSyntaxFrameInput<'_>,
        preserve_input_exhaustion: bool,
    ) -> Result<DecodedFullAjocSyntaxFrame<'decoder>, FullAjocSyntaxError> {
        self.last_observation = None;
        self.prepare_substream(input.substream_index, &input.context)?;
        let slot = usize::try_from(input.substream_index).unwrap_or(usize::MAX);

        let Self {
            states,
            workspace,
            last_observation,
        } = self;
        let Some(state) = states.get_mut(slot) else {
            return Err(FullAjocSyntaxError::SubstreamIndexOutOfRange {
                index: input.substream_index,
                limit: MAX_SUBSTREAMS,
            });
        };
        let parsed =
            match parse_substream_ajoc(input.payload, &input.context, state, workspace.borrow()) {
                Ok(parsed) => parsed,
                Err(error) => {
                    if !preserve_input_exhaustion || !substream_input_exhausted(&error) {
                        *state = AudioDataState::new();
                    }
                    return Err(FullAjocSyntaxError::Decode {
                        substream_index: input.substream_index,
                        error,
                    });
                }
            };
        let aspx_config = state.var_channel.config();

        let element_count = usize::from(parsed.audio.var_element.channel_elements());
        let aspx_count = usize::from(parsed.audio.var_element.aspx_elements());
        let control_count =
            usize::try_from(parsed.audio.ajoc.num_umx_signals).unwrap_or(usize::MAX);
        let dmx_block_count = parsed.audio.dmx_blocks_written();
        let umx_block_count = parsed.audio.umx_blocks_written();

        let slices = (
            workspace.elements.get(..element_count),
            workspace.aspx.get(..aspx_count),
            workspace.controls.get(..control_count),
            workspace.matrices.get(..control_count),
            workspace.dmx_blocks.get(..dmx_block_count),
            workspace.umx_blocks.get(..umx_block_count),
        );
        let (elements, aspx, object_controls, matrices, dmx_blocks, umx_blocks) = match slices {
            (
                Some(elements),
                Some(aspx),
                Some(object_controls),
                Some(matrices),
                Some(dmx_blocks),
                Some(umx_blocks),
            ) => (
                elements,
                aspx,
                object_controls,
                matrices,
                dmx_blocks,
                umx_blocks,
            ),
            _ => {
                *state = AudioDataState::new();
                return Err(workspace_invariant(
                    workspace,
                    element_count,
                    aspx_count,
                    control_count,
                    dmx_block_count,
                    umx_block_count,
                ));
            }
        };

        let frame_length = input.context.params.context.frame_len_base;
        let aspx_support = AspxBlocker::check(&parsed.audio.var_element, frame_length);
        let full_support =
            SupportedAjocFullFrame::check(&parsed, &input.context, input.physical_substreams);
        *last_observation = Some(FullAjocSyntaxObservationDescriptor {
            substream_index: input.substream_index,
            parsed,
            context: input.context,
            element_count,
            aspx_count,
            aspx_config,
            control_count,
            dmx_block_count,
            umx_block_count,
            aspx_support,
            full_support,
        });
        Ok(DecodedFullAjocSyntaxFrame {
            state,
            parsed,
            context: input.context,
            elements,
            aspx,
            aspx_config,
            object_controls,
            matrices,
            dmx_blocks,
            umx_blocks,
            aspx_support,
            full_support,
        })
    }
}

fn workspace_invariant(
    workspace: &FullAjocSyntaxWorkspace,
    elements: usize,
    aspx: usize,
    controls: usize,
    dmx_blocks: usize,
    umx_blocks: usize,
) -> FullAjocSyntaxError {
    let candidates = [
        (
            FullAjocSyntaxBuffer::ChannelElements,
            elements,
            workspace.elements.len(),
        ),
        (
            FullAjocSyntaxBuffer::AspxElements,
            aspx,
            workspace.aspx.len(),
        ),
        (
            FullAjocSyntaxBuffer::ObjectControls,
            controls,
            workspace.controls.len(),
        ),
        (
            FullAjocSyntaxBuffer::Matrices,
            controls,
            workspace.matrices.len(),
        ),
        (
            FullAjocSyntaxBuffer::CoreOamdBlocks,
            dmx_blocks,
            workspace.dmx_blocks.len(),
        ),
        (
            FullAjocSyntaxBuffer::FullOamdBlocks,
            umx_blocks,
            workspace.umx_blocks.len(),
        ),
    ];
    let (buffer, needed, available) = candidates
        .into_iter()
        .find(|(_, needed, available)| needed > available)
        .unwrap_or((FullAjocSyntaxBuffer::ChannelElements, elements, 0));
    FullAjocSyntaxError::WorkspaceInvariant {
        buffer,
        needed,
        available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{huffman::tables::ASF_HCB_1, testutil::BitBuf};
    use macindecode_ac4_bitstream::{
        reader::BitReader,
        substream::{Ac4SubstreamGroupInfo, SubstreamInfo, SubstreamInfoAjoc},
        toc::Ac4Toc,
    };

    fn toc() -> Ac4Toc {
        Ac4Toc {
            bitstream_version: 2,
            sequence_counter: 1,
            wait_frames_code: None,
            fs_index: 1,
            frame_rate_index: 1,
            iframe_global: true,
            n_presentations: 1,
            payload_base: 0,
            bits_consumed: 0,
        }
    }

    /// 单一动态下混信号、单一动态上混对象、无 LFE 的 A-JOC info。
    fn single_signal_info(audio_ndot: bool) -> SubstreamInfoAjoc {
        let mut bits = BitBuf::new();
        bits.push(false); // b_substreams_present
        bits.push(false); // b_hsf_ext
        bits.push(true); // 单个低频 substream
        bits.push(false); // b_channel_coded
        bits.push(false); // b_oamd_substream
        bits.push(true); // b_ajoc
        bits.push(false); // b_lfe
        bits.push(false); // b_static_dmx
        bits.push_bits(0, 4); // 一个 dmx signal
        bits.push(true); // dmx_assignment：dynamic only
        bits.push(false); // b_oamd_common_data_present
        bits.push_bits(0, 4); // 一个 umx object
        bits.push(true); // upmix_assignment：dynamic only
        bits.push(false); // b_sf_multiplier
        bits.push(false); // b_bitrate_info
        bits.push(audio_ndot); // b_audio_ndot
        bits.push(false); // b_content_type

        let group = Ac4SubstreamGroupInfo::parse(&mut BitReader::new(bits.as_slice()), 2, 1, 1)
            .expect("A-JOC group 应能解析");
        match group.substreams().first() {
            Some(&SubstreamInfo::Ajoc(info)) => info,
            other => panic!("应得到 A-JOC info，实际为 {other:?}"),
        }
    }

    fn push_audio_data(
        bits: &mut BitBuf,
        b_iframe: bool,
        nonzero_spectrum: bool,
        position_x: Option<u32>,
        active_companding: bool,
        b_alternative: bool,
    ) {
        bits.push(false); // b_some_signals_inactive
        bits.push(true); // var_codec_mode = A-SPX
        if b_iframe {
            bits.push_aspx_config();
        }
        bits.push(false); // companding_control(1)：b_compand_on[0]
        bits.push(active_companding); // b_compand_avg
        if nonzero_spectrum {
            bits.push(false); // spec_frontend = ASF
            bits.push_long_sf_info(1);
            bits.push_bits(1, 4); // codebook 1
            bits.push_bits(0, 5); // 一个频带
            bits.push_symbol(&ASF_HCB_1, 0);
            bits.push_bits(100, 8); // unity scale factor
            bits.push(false); // b_snf_data_exists
        } else {
            bits.push_mono_data(2);
        }
        bits.push_drivable_aspx_data_1ch_for_frame(b_iframe);
        bits.push(true); // b_dmx_timing
        bits.push_timing(1);
        if let Some(x) = position_x {
            bits.push_absolute_position_block(b_iframe, x, 2, true, 3);
        } else {
            bits.push_inactive_object_block();
        }
        if b_alternative {
            bits.push(false); // b_ducking_disabled
            bits.push_bits(0, 2); // object_sound_category
            bits.push_bits(0, 2); // n_alt_data_sets
        }
        bits.push(false); // b_oamd_extension_present
        bits.push_minimal_ajoc(1);
        bits.push_minimal_dmx_de(1);
        bits.push(false); // b_umx_timing
        bits.push(true); // b_derive_timing_from_dmx
        if let Some(x) = position_x {
            bits.push_absolute_position_block(b_iframe, x, 4, true, 5);
        } else {
            bits.push_inactive_object_block();
        }
        if b_alternative {
            bits.push(false); // b_ducking_disabled
            bits.push_bits(0, 2); // object_sound_category
            bits.push_bits(0, 2); // n_alt_data_sets
        }
    }

    fn push_metadata(bits: &mut BitBuf) {
        bits.push(false); // b_more_basic_metadata
        bits.push(false); // b_dialog
        bits.push(false); // b_channels_classifier
        bits.push(false); // b_event_probability
        bits.push_bits(1, 7); // tools_metadata_size_value
        bits.push(false); // b_more_bits
        bits.push(false); // b_de_data_present
        bits.push(false); // b_emdf_payloads_substream
        bits.byte_align();
    }

    fn payload_for_frame_with_audio_size_delta(
        b_iframe: bool,
        nonzero_spectrum: bool,
        position_x: Option<u32>,
        declared_delta: i32,
    ) -> BitBuf {
        payload_for_frame_with_audio_size_delta_and_companding(
            b_iframe,
            nonzero_spectrum,
            position_x,
            false,
            declared_delta,
        )
    }

    fn payload_for_frame_with_audio_size_delta_and_companding(
        b_iframe: bool,
        nonzero_spectrum: bool,
        position_x: Option<u32>,
        active_companding: bool,
        declared_delta: i32,
    ) -> BitBuf {
        let mut audio = BitBuf::new();
        push_audio_data(
            &mut audio,
            b_iframe,
            nonzero_spectrum,
            position_x,
            active_companding,
            false,
        );
        audio.byte_align();
        let audio = audio.as_slice();
        let declared = i64::try_from(audio.len())
            .unwrap_or(0)
            .saturating_add(i64::from(declared_delta));
        let declared = u32::try_from(declared).unwrap_or(0);
        let written = usize::try_from(declared).unwrap_or(0).min(audio.len());

        let mut payload = BitBuf::new();
        payload.push_bits(declared, 15);
        payload.push(false); // b_more_bits
        payload.push_bytes(audio.get(..written).unwrap_or(audio));
        push_metadata(&mut payload);
        payload
    }

    fn payload_for_frame_with_oamd_position(
        b_iframe: bool,
        nonzero_spectrum: bool,
        position_x: Option<u32>,
    ) -> BitBuf {
        payload_for_frame_with_audio_size_delta(b_iframe, nonzero_spectrum, position_x, 0)
    }

    fn payload_for_frame_with_spectrum(b_iframe: bool, nonzero_spectrum: bool) -> BitBuf {
        payload_for_frame_with_oamd_position(b_iframe, nonzero_spectrum, None)
    }

    fn payload_for_frame_with_active_companding(b_iframe: bool) -> BitBuf {
        payload_for_frame_with_audio_size_delta_and_companding(b_iframe, true, None, true, 0)
    }

    fn payload_for_frame(b_iframe: bool) -> BitBuf {
        payload_for_frame_with_spectrum(b_iframe, false)
    }

    fn alternative_payload() -> BitBuf {
        let mut audio = BitBuf::new();
        push_audio_data(&mut audio, true, false, None, false, true);
        audio.byte_align();
        let audio = audio.as_slice();

        let mut payload = BitBuf::new();
        payload.push_bits(u32::try_from(audio.len()).unwrap_or(0), 15);
        payload.push(false); // b_more_bits
        payload.push_bytes(audio);
        push_metadata(&mut payload);
        payload
    }

    fn payload() -> BitBuf {
        payload_for_frame(true)
    }

    fn context() -> AjocSubstreamContext {
        AjocSubstreamContext::derive(&toc(), &single_signal_info(true), 1, 1, false, Some(1))
            .expect("单信号上下文应可推导")
    }

    fn alternative_context() -> AjocSubstreamContext {
        AjocSubstreamContext::derive(&toc(), &single_signal_info(true), 1, 1, true, Some(1))
            .expect("alternative 单信号上下文应可推导")
    }

    fn dependent_context() -> AjocSubstreamContext {
        AjocSubstreamContext::derive(&toc(), &single_signal_info(false), 1, 1, false, Some(1))
            .expect("非 I 帧单信号上下文应可推导")
    }

    fn input_with_context<'a>(
        payload: &'a [u8],
        context: AjocSubstreamContext,
    ) -> FullAjocSyntaxFrameInput<'a> {
        FullAjocSyntaxFrameInput {
            payload,
            context,
            substream_index: 0,
            physical_substreams: 1,
        }
    }

    fn input<'a>(payload: &'a [u8]) -> FullAjocSyntaxFrameInput<'a> {
        input_with_context(payload, context())
    }

    fn audio_input_with_context<'a>(
        payload: &'a [u8],
        context: AjocSubstreamContext,
        lfe_position: Option<u32>,
    ) -> super::super::FullAjocAudioFrameInput<'a> {
        audio_input_with_provenance(
            payload,
            context,
            lfe_position,
            super::super::FullAjocFrameProvenance::new(0),
        )
    }

    fn audio_input_with_provenance<'a>(
        payload: &'a [u8],
        context: AjocSubstreamContext,
        lfe_position: Option<u32>,
        provenance: super::super::FullAjocFrameProvenance,
    ) -> super::super::FullAjocAudioFrameInput<'a> {
        super::super::FullAjocAudioFrameInput {
            syntax: input_with_context(payload, context),
            provenance,
            lfe_position,
            mode: super::super::FullAjocDecodeMode::RequireFull,
        }
    }

    fn audio_input<'a>(payload: &'a [u8]) -> super::super::FullAjocAudioFrameInput<'a> {
        audio_input_with_context(payload, context(), None)
    }

    type AudioFrameSnapshot = (
        super::super::FullAjocObservation,
        Option<super::super::FullAjocFrameProvenance>,
        Vec<u32>,
        Vec<u32>,
        Vec<u32>,
    );

    fn audio_frame_snapshot(
        decoded: &super::super::DecodedFullAjocAudioFrame<'_>,
    ) -> AudioFrameSnapshot {
        let bits = |samples: &[f32]| samples.iter().map(|sample| sample.to_bits()).collect();
        (
            decoded.output().observation(),
            decoded
                .output()
                .aligned_side_information()
                .map(|aligned| aligned.provenance()),
            bits(
                decoded
                    .frontend()
                    .asf()
                    .channel(0)
                    .expect("应有一路 ASF PCM")
                    .samples(),
            ),
            bits(
                decoded
                    .output()
                    .diagnostic_channel(0)
                    .expect("应有一路诊断 PCM")
                    .samples(),
            ),
            bits(
                decoded
                    .output()
                    .reconstructed_channel(0)
                    .expect("应有一路对象 PCM")
                    .samples(),
            ),
        )
    }

    type WorkspaceLayout = (
        (*const ChannelElement, usize),
        (*const AspxData, usize),
        (*const AjocObjectControl, usize),
        (*const AjocObjectMatrix, usize),
        (*const OamdMetadataBlock, usize),
        (*const OamdMetadataBlock, usize),
    );

    fn workspace_layout(decoder: &FullAjocSyntaxDecoder) -> WorkspaceLayout {
        (
            (
                decoder.workspace.elements.as_ptr(),
                decoder.workspace.elements.capacity(),
            ),
            (
                decoder.workspace.aspx.as_ptr(),
                decoder.workspace.aspx.capacity(),
            ),
            (
                decoder.workspace.controls.as_ptr(),
                decoder.workspace.controls.capacity(),
            ),
            (
                decoder.workspace.matrices.as_ptr(),
                decoder.workspace.matrices.capacity(),
            ),
            (
                decoder.workspace.dmx_blocks.as_ptr(),
                decoder.workspace.dmx_blocks.capacity(),
            ),
            (
                decoder.workspace.umx_blocks.as_ptr(),
                decoder.workspace.umx_blocks.capacity(),
            ),
        )
    }

    #[test]
    fn decoded_view_exposes_only_this_frames_written_workspaces() {
        let payload = payload();
        let mut decoder = super::super::FullAjocDecoder::new();
        decoder
            .prepare_syntax_substream(0, &context())
            .expect("配置建立时应能预分配语法工作区");

        let first_pointers = {
            let decoded = decoder
                .decode_syntax_frame(input(payload.as_slice()))
                .expect("首帧应能解析");
            assert_eq!(decoded.elements().len(), 1);
            assert_eq!(decoded.aspx().len(), 1);
            assert!(decoded.aspx_config().is_some());
            assert_eq!(decoded.object_controls().len(), 1);
            assert_eq!(decoded.matrices().len(), 1);
            assert_eq!(decoded.dmx_blocks().len(), 1);
            assert_eq!(decoded.umx_blocks().len(), 1);
            assert_eq!(decoded.parsed().audio.dmx_blocks_written(), 1);
            assert_eq!(decoded.parsed().audio.umx_blocks_written(), 1);
            assert_eq!(
                decoded.context().params.context.sampling_frequency_hz,
                48_000
            );
            assert!(decoded.aspx_support().is_ok());
            assert!(decoded.full_support().is_ok());
            (
                decoded.elements().as_ptr(),
                decoded.aspx().as_ptr(),
                decoded.object_controls().as_ptr(),
                decoded.matrices().as_ptr(),
                decoded.dmx_blocks().as_ptr(),
                decoded.umx_blocks().as_ptr(),
            )
        };

        let second_pointers = {
            let decoded = decoder
                .decode_syntax_frame(input(payload.as_slice()))
                .expect("相同配置的次帧应复用工作区");
            (
                decoded.elements().as_ptr(),
                decoded.aspx().as_ptr(),
                decoded.object_controls().as_ptr(),
                decoded.matrices().as_ptr(),
                decoded.dmx_blocks().as_ptr(),
                decoded.umx_blocks().as_ptr(),
            )
        };
        assert_eq!(second_pointers, first_pointers, "稳定解码不得扩容");
    }

    #[test]
    fn alternative_oamd_is_observable_but_cannot_issue_a_full_credential() {
        let payload = alternative_payload();
        let mut decoder = super::super::FullAjocDecoder::new();
        let decoded = decoder
            .decode_syntax_frame(input_with_context(
                payload.as_slice(),
                alternative_context(),
            ))
            .expect("alternative OAMD 应保留为语法观察");

        assert_eq!(
            decoded.full_support(),
            Err(super::super::FullAjocBlocker::AlternativeObjectMetadata)
        );
        assert_eq!(
            decoded
                .parsed()
                .audio
                .dmx_oamd
                .alternative()
                .expect("core header 应保留")
                .n_data_sets,
            0
        );
        assert_eq!(
            decoded
                .parsed()
                .audio
                .umx_oamd
                .alternative()
                .expect("full header 应保留")
                .n_data_sets,
            0
        );
    }

    #[test]
    fn decoded_view_carries_the_aspx_config_into_a_dependent_frame() {
        let iframe = payload_for_frame(true);
        let dependent = payload_for_frame(false);
        let mut decoder = super::super::FullAjocDecoder::new();

        let iframe_config = decoder
            .decode_syntax_frame(input_with_context(iframe.as_slice(), context()))
            .expect("I 帧应建立 A-SPX 配置")
            .aspx_config()
            .expect("I 帧快照应带出配置");
        let decoded = decoder
            .decode_syntax_frame(input_with_context(
                dependent.as_slice(),
                dependent_context(),
            ))
            .expect("非 I 帧应沿用 A-SPX 配置");

        assert!(!decoded.context().params.b_iframe);
        assert_eq!(decoded.aspx_config(), Some(iframe_config));
    }

    #[test]
    fn frontend_combines_syntax_oamd_and_borrowed_asf_pcm() {
        let payload = payload();
        let mut decoder = super::super::FullAjocDecoder::new();
        decoder
            .prepare_frontend_substream(0, &context())
            .expect("配置建立时应能预分配完整前端");

        let decoded = decoder
            .decode_frontend_frame(input(payload.as_slice()))
            .expect("语法与 ASF 前端应在同一事务中成功");
        assert_eq!(decoded.syntax().elements().len(), 1);
        assert_eq!(decoded.syntax().dmx_blocks().len(), 1);
        assert_eq!(decoded.syntax().umx_blocks().len(), 1);
        assert_eq!(decoded.asf().channels(), 1);
        let pcm = decoded.asf().channel(0).expect("应有一路核心带 PCM");
        assert_eq!(pcm.observation().element_index(), 0);
        assert_eq!(pcm.observation().channel_index(), 0);
        assert_eq!(pcm.samples().len(), 1920);
        assert!(pcm.samples().iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn audio_frame_combines_frontend_table_188_and_full_outputs() {
        let payload = payload();
        let mut decoder = super::super::FullAjocDecoder::new();
        let decoded = decoder
            .decode_audio_frame(audio_input(payload.as_slice()))
            .expect("完整 Full 音频帧事务应成功");

        assert_eq!(decoded.frontend().syntax().dmx_blocks().len(), 1);
        assert_eq!(decoded.frontend().syntax().umx_blocks().len(), 1);
        assert_eq!(decoded.frontend().asf().channels(), 1);
        assert!(decoded.output().observation().warmup());
        assert!(decoded.output().aligned_side_information().is_none());
        assert!(!decoded.output().observation().reconstructed());
        assert_eq!(decoded.output().diagnostic_channels(), 1);
        assert_eq!(decoded.output().reconstructed_channels(), 1);
        for channel in [
            decoded
                .output()
                .diagnostic_channel(0)
                .expect("应有一路诊断 PCM"),
            decoded
                .output()
                .reconstructed_channel(0)
                .expect("预热期也应有一路对象 PCM"),
        ] {
            assert_eq!(channel.samples().len(), 1920);
            assert!(channel.samples().iter().all(|sample| sample.is_finite()));
        }
    }

    #[test]
    fn downstream_failure_keeps_the_same_frontend_observations_until_reset() {
        let payload = payload();
        let mut decoder = super::super::FullAjocDecoder::new();
        let mut input = audio_input(payload.as_slice());
        input.syntax.physical_substreams = 2;

        let error = decoder
            .decode_audio_frame(input)
            .expect_err("多个 Full substream 应在前端完成后 fail closed");
        assert!(matches!(
            error,
            super::super::FullAjocAudioFrameError::Decode(_)
        ));

        let syntax = decoder
            .last_syntax_observation()
            .expect("下游失败不得丢失同次调用的语法 observation");
        assert_eq!(syntax.parsed().audio.dmx_blocks_written(), 1);
        assert_eq!(syntax.parsed().audio.umx_blocks_written(), 1);
        assert_eq!(syntax.object_controls().len(), 1);
        assert_eq!(syntax.matrices().len(), 1);
        assert_eq!(syntax.dmx_blocks().len(), 1);
        assert_eq!(syntax.umx_blocks().len(), 1);

        let asf = decoder
            .last_asf_observation()
            .expect("下游失败不得丢失同次调用的 ASF observation");
        assert_eq!(asf.channels(), 1);
        assert_eq!(
            asf.channel(0).expect("应有一路核心带 PCM").samples().len(),
            1920
        );

        decoder.reset_substream(0);
        assert!(decoder.last_syntax_observation().is_none());
        assert!(decoder.last_asf_observation().is_none());
    }

    #[test]
    fn observe_full_blocker_preserves_frontend_history_for_dependent_census() {
        let iframe = payload_for_frame_with_active_companding(true);
        let dependent = payload_for_frame_with_active_companding(false);
        let reference_pcm = |decoder: &mut super::super::FullAjocDecoder,
                             payload: &[u8],
                             context: AjocSubstreamContext| {
            decoder
                .decode_frontend_frame(input_with_context(payload, context))
                .expect("census 前端应不受 active companding 下游门禁影响")
                .asf()
                .channel(0)
                .expect("应有一路核心带 PCM")
                .samples()
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        };
        let mut reference = super::super::FullAjocDecoder::new();
        let expected_iframe = reference_pcm(&mut reference, iframe.as_slice(), context());
        let expected_dependent =
            reference_pcm(&mut reference, dependent.as_slice(), dependent_context());
        assert_ne!(
            expected_dependent, expected_iframe,
            "反例必须能观察到第二帧的 ASF overlap 延续"
        );

        let mut decoder = super::super::FullAjocDecoder::new();

        for (label, payload, context, expected_pcm) in [
            (
                "I 帧",
                iframe.as_slice(),
                context(),
                expected_iframe.as_slice(),
            ),
            (
                "依赖帧",
                dependent.as_slice(),
                dependent_context(),
                expected_dependent.as_slice(),
            ),
        ] {
            let mut input = audio_input_with_context(payload, context, None);
            input.mode = super::super::FullAjocDecodeMode::ObserveFull;
            let error = decoder
                .decode_audio_frame(input)
                .expect_err("active companding 必须被 A-SPX 门禁拒绝");
            let super::super::FullAjocAudioFrameError::Decode(error) = error else {
                panic!("{label} 必须先成功解析前端，再由 A-SPX 门禁拒绝")
            };
            assert!(error.detail().contains("companding"));

            {
                let syntax = decoder
                    .last_syntax_observation()
                    .expect("下游 blocker 后应保留当前语法 observation");
                assert!(
                    syntax.aspx_config().is_some(),
                    "{label} 应带有生效的 A-SPX 配置"
                );
            }
            {
                let asf = decoder
                    .last_asf_observation()
                    .expect("下游 blocker 后应保留当前 ASF observation");
                let actual = asf
                    .channel(0)
                    .expect("应有一路核心带 PCM")
                    .samples()
                    .iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected_pcm, "{label} 必须保留同源 ASF overlap");
            }
        }
    }

    #[test]
    fn mutable_preparation_invalidates_frontend_observations_before_workspace_resize() {
        let payload = payload();
        let mut decoder = super::super::FullAjocDecoder::new();
        decoder
            .decode_audio_frame(audio_input(payload.as_slice()))
            .expect("完整帧应建立两份前端 observation");
        assert!(decoder.last_syntax_observation().is_some());
        assert!(decoder.last_asf_observation().is_some());

        let mut shorter_toc = toc();
        shorter_toc.frame_rate_index = 3;
        let shorter_context = AjocSubstreamContext::derive(
            &shorter_toc,
            &single_signal_info(true),
            1,
            1,
            false,
            Some(1),
        )
        .expect("应能推导 1536 样本的另一条 substream");
        assert_eq!(shorter_context.params.context.frame_len_base, 1536);

        decoder
            .prepare_frontend_substream(1, &shorter_context)
            .expect("为另一条较短 substream 预分配应成功");
        assert!(
            decoder.last_syntax_observation().is_none(),
            "可变预分配后不得暴露旧语法快照"
        );
        assert!(
            decoder.last_asf_observation().is_none(),
            "共享 PCM 工作区改变长度后不得暴露旧 ASF 快照"
        );
    }

    #[test]
    fn classifies_inline_emdf_exhaustion_as_retryable() {
        let wrap = |error| FullAjocSyntaxError::Decode {
            substream_index: 0,
            error: SubstreamAudioError::Substream(AudioSubstreamError::Emdf(error)),
        };
        let retryable = [
            EmdfError::Read(ReadError::OutOfBounds {
                requested_bits: 8,
                bit_position: 42,
                remaining_bits: 0,
            }),
            EmdfError::MissingTerminator {
                bit_position: 42,
                remaining_bits: 3,
            },
        ];
        for error in retryable {
            assert!(wrap(error).is_input_exhausted(), "{error:?}");
        }

        let terminal = [
            EmdfError::Read(ReadError::ValueOverflow { bit_position: 42 }),
            EmdfError::TooManyPayloads {
                limit: 32,
                bit_position: 42,
            },
            EmdfError::PayloadTooLarge {
                declared: 0x0100_0000,
                limit: 0x00ff_ffff,
                bit_position: 42,
            },
        ];
        for error in terminal {
            assert!(!wrap(error).is_input_exhausted(), "{error:?}");
        }
    }

    #[test]
    fn classifies_nested_oamd_read_exhaustion_as_retryable() {
        let wrap = |error| FullAjocSyntaxError::Decode {
            substream_index: 0,
            error: SubstreamAudioError::Substream(AudioSubstreamError::Oamd(error)),
        };
        assert!(
            wrap(OamdError::Read(ReadError::OutOfBounds {
                requested_bits: 1,
                bit_position: 42,
                remaining_bits: 0,
            }))
            .is_input_exhausted()
        );
        assert!(
            !wrap(OamdError::AdditionalDataUnderflow {
                declared_bytes: 1,
                used_bits: 10,
            })
            .is_input_exhausted()
        );
    }

    #[test]
    fn audio_frame_input_exhaustion_preserves_all_history_for_retry() {
        let iframe = payload_for_frame_with_spectrum(true, true);
        let dependent = payload_for_frame_with_spectrum(false, true);
        let iframe_provenance = super::super::FullAjocFrameProvenance::new(70);
        let dependent_provenance = super::super::FullAjocFrameProvenance::new(71);

        let mut reference = super::super::FullAjocDecoder::new();
        reference
            .decode_audio_frame(audio_input_with_provenance(
                iframe.as_slice(),
                context(),
                None,
                iframe_provenance,
            ))
            .expect("参考解码器的 I 帧应成功");
        let expected = {
            let decoded = reference
                .decode_audio_frame(audio_input_with_provenance(
                    dependent.as_slice(),
                    dependent_context(),
                    None,
                    dependent_provenance,
                ))
                .expect("参考解码器的依赖帧应成功");
            audio_frame_snapshot(&decoded)
        };

        let mut retried = super::super::FullAjocDecoder::new();
        retried
            .decode_audio_frame(audio_input_with_provenance(
                iframe.as_slice(),
                context(),
                None,
                iframe_provenance,
            ))
            .expect("重试路径的 I 帧应成功");
        let truncated = dependent.as_slice().get(..4).expect("依赖帧载荷至少四字节");
        let error = retried
            .decode_audio_frame(audio_input_with_provenance(
                truncated,
                dependent_context(),
                None,
                dependent_provenance,
            ))
            .expect_err("截断的依赖帧必须失败");
        assert!(
            error.is_input_exhausted(),
            "完整入口必须把可重试的输入耗尽与坏帧区分开"
        );
        assert!(
            retried.last_syntax_observation().is_none(),
            "失败调用不得把上一帧语法 observation 冒充为当前帧"
        );
        assert!(
            retried.last_asf_observation().is_none(),
            "语法尚未完成时不得暴露上一帧 ASF observation"
        );

        let actual = {
            let decoded = retried
                .decode_audio_frame(audio_input_with_provenance(
                    dependent.as_slice(),
                    dependent_context(),
                    None,
                    dependent_provenance,
                ))
                .expect("补全同一依赖帧后应能重试");
            audio_frame_snapshot(&decoded)
        };

        assert_eq!(actual, expected, "截断重试不得改变 PCM、控制或 AU 来源");
    }

    #[test]
    fn complete_audio_frame_input_exhaustion_discards_all_history() {
        let iframe = payload_for_frame_with_spectrum(true, true);
        let dependent = payload_for_frame_with_spectrum(false, true);
        let mut decoder = super::super::FullAjocDecoder::new();

        decoder
            .decode_complete_audio_frame(audio_input_with_context(
                iframe.as_slice(),
                context(),
                None,
            ))
            .expect("完整 I 帧应建立语法、ASF、FIFO 与 Full 历史");
        assert!(!decoder.downstream_is_fresh(0));

        let truncated = dependent.as_slice().get(..4).expect("依赖帧载荷至少四字节");
        let error = decoder
            .decode_complete_audio_frame(audio_input_with_context(
                truncated,
                dependent_context(),
                None,
            ))
            .expect_err("声明为完整的截断载荷必须按坏帧失效");
        assert!(error.is_input_exhausted());
        assert!(
            decoder.downstream_is_fresh(0),
            "完整输入入口必须立即切断 QMF、控制 FIFO 与 Full 历史"
        );

        let next_error = decoder
            .decode_complete_audio_frame(audio_input_with_context(
                dependent.as_slice(),
                dependent_context(),
                None,
            ))
            .expect_err("坏帧后的依赖帧不得沿用此前 I 帧语法配置");
        assert!(matches!(
            next_error,
            super::super::FullAjocAudioFrameError::Syntax(FullAjocSyntaxError::Decode {
                error: SubstreamAudioError::AudioData(
                    crate::audio_data::AudioDataError::VarElement(
                        crate::var_element::VarElementError::MissingAspxConfig
                    )
                ),
                ..
            })
        ));
    }

    #[test]
    fn audio_region_exhaustion_discards_all_history_as_a_bad_frame() {
        let iframe = payload_for_frame_with_spectrum(true, true);
        let short_audio_region = payload_for_frame_with_audio_size_delta(false, true, None, -1);
        let dependent = payload_for_frame_with_spectrum(false, true);
        let mut decoder = super::super::FullAjocDecoder::new();

        decoder
            .decode_audio_frame(audio_input_with_context(iframe.as_slice(), context(), None))
            .expect("I 帧应建立语法、ASF、QMF 与 Full 历史");
        assert!(!decoder.downstream_is_fresh(0));

        let error = decoder
            .decode_audio_frame(audio_input_with_context(
                short_audio_region.as_slice(),
                dependent_context(),
                None,
            ))
            .expect_err("声明过短的 audio_size 必须在音频区段内失败");
        assert!(matches!(
            &error,
            super::super::FullAjocAudioFrameError::Syntax(FullAjocSyntaxError::Decode {
                error: SubstreamAudioError::AudioData(_),
                ..
            })
        ));
        assert!(
            !error.is_input_exhausted(),
            "audio_size 区段内部越界不能靠追加 substream 尾部恢复"
        );
        assert!(
            decoder.downstream_is_fresh(0),
            "坏帧必须切断 ASF/QMF、控制 FIFO 与 Full 历史"
        );

        let next_error = decoder
            .decode_audio_frame(audio_input_with_context(
                dependent.as_slice(),
                dependent_context(),
                None,
            ))
            .expect_err("坏帧之后的依赖帧不得沿用此前 I 帧配置");
        assert!(matches!(
            next_error,
            super::super::FullAjocAudioFrameError::Syntax(FullAjocSyntaxError::Decode {
                error: SubstreamAudioError::AudioData(
                    crate::audio_data::AudioDataError::VarElement(
                        crate::var_element::VarElementError::MissingAspxConfig
                    )
                ),
                ..
            })
        ));
    }

    #[test]
    fn partial_frame_entries_cut_existing_qmf_and_full_history() {
        let payload = payload();
        let mut decoder = super::super::FullAjocDecoder::new();

        decoder
            .decode_audio_frame(audio_input(payload.as_slice()))
            .expect("完整帧应建立 QMF/Full 历史");
        assert!(!decoder.downstream_is_fresh(0));
        decoder
            .decode_frontend_frame(input(payload.as_slice()))
            .expect("前端帧应成功");
        assert!(
            decoder.downstream_is_fresh(0),
            "未推进 QMF 的 frontend-only 帧必须切断已有下游历史"
        );

        decoder
            .decode_audio_frame(audio_input(payload.as_slice()))
            .expect("切断后应能重新建立 QMF/Full 历史");
        assert!(!decoder.downstream_is_fresh(0));
        decoder
            .decode_syntax_frame(input(payload.as_slice()))
            .expect("语法帧应成功");
        assert!(
            decoder.downstream_is_fresh(0),
            "未推进 ASF/QMF 的 syntax-only 帧必须切断已有下游历史"
        );
    }

    #[test]
    fn table_188_aligns_raw_oamd_and_provenance_with_output_pcm() {
        let first_payload = payload_for_frame_with_oamd_position(true, false, Some(7));
        let second_payload = payload_for_frame_with_oamd_position(true, false, Some(19));
        let third_payload = payload_for_frame_with_oamd_position(true, false, Some(31));
        let first_provenance = super::super::FullAjocFrameProvenance::new(40)
            .with_source_sample_start(-1_920)
            .with_presentation_sample_start(0)
            .with_priming_samples(1_920)
            .with_random_access_hint(true)
            .with_discontinuity(true);
        let second_provenance = super::super::FullAjocFrameProvenance::new(41)
            .with_source_sample_start(0)
            .with_presentation_sample_start(1_920);
        let mut decoder = super::super::FullAjocDecoder::new();

        let (first_dmx, first_umx, first_audio) = {
            let decoded = decoder
                .decode_audio_frame(audio_input_with_provenance(
                    first_payload.as_slice(),
                    context(),
                    None,
                    first_provenance,
                ))
                .expect("首帧应能解码");
            assert!(decoded.output().observation().warmup());
            assert!(
                decoded.output().aligned_side_information().is_none(),
                "首份控制到期前不得伪造 side information"
            );
            (
                decoded.frontend().syntax().dmx_blocks().to_vec(),
                decoded.frontend().syntax().umx_blocks().to_vec(),
                decoded.frontend().syntax().parsed().audio,
            )
        };

        let (second_dmx, second_umx, second_audio) = {
            let decoded = decoder
                .decode_audio_frame(audio_input_with_provenance(
                    second_payload.as_slice(),
                    context(),
                    None,
                    second_provenance,
                ))
                .expect("第二帧应能解码");
            let current_dmx = decoded.frontend().syntax().dmx_blocks();
            let current_umx = decoded.frontend().syntax().umx_blocks();
            assert_ne!(current_dmx, first_dmx, "测试载荷必须能区分前后 OAMD");
            assert_ne!(current_umx, first_umx, "测试载荷必须能区分前后 OAMD");

            let aligned = decoded
                .output()
                .aligned_side_information()
                .expect("第二帧应取到首帧的控制快照");
            assert_eq!(aligned.provenance(), first_provenance);
            assert_eq!(aligned.provenance().access_unit_index(), 40);
            assert_eq!(aligned.provenance().source_sample_start(), Some(-1_920));
            assert_eq!(aligned.provenance().presentation_sample_start(), Some(0));
            assert_eq!(aligned.provenance().priming_samples(), Some(1_920));
            assert_eq!(aligned.provenance().random_access_hint(), Some(true));
            assert!(aligned.provenance().discontinuity());
            assert_eq!(aligned.dmx_oamd_blocks(), first_dmx);
            assert_eq!(aligned.umx_oamd_blocks(), first_umx);
            assert_eq!(aligned.dmx_oamd_timing(), first_audio.dmx_timing);
            assert_eq!(aligned.umx_oamd_timing(), first_audio.umx_timing);
            assert_eq!(
                aligned.dmx_effective_oamd_timing().effective(),
                first_audio.dmx_timing
            );
            assert!(
                aligned
                    .dmx_effective_oamd_timing()
                    .updated_in_source_access_unit()
            );
            assert_eq!(
                aligned.umx_effective_oamd_timing().effective(),
                first_audio.dmx_timing,
                "fixture 的 Full timing 应由 Core timing 派生"
            );
            assert!(
                aligned
                    .umx_effective_oamd_timing()
                    .updated_in_source_access_unit()
            );
            assert_eq!(
                aligned.derive_timing_from_dmx(),
                first_audio.derive_timing_from_dmx
            );
            assert_eq!(
                aligned.dmx_num_obj_info_blocks(),
                first_audio.dmx_num_obj_info_blocks
            );
            assert_eq!(
                aligned.umx_num_obj_info_blocks(),
                first_audio.umx_num_obj_info_blocks
            );
            assert_eq!(aligned.frame_length(), 1_920);
            assert_eq!(aligned.sampling_frequency_hz(), 48_000);
            assert_eq!(aligned.pcm_alignment_delay_samples(), 288);
            assert_eq!(aligned.control_alignment_delay_frames(), 1);
            assert!(decoded.output().observation().reconstructed());
            (
                current_dmx.to_vec(),
                current_umx.to_vec(),
                decoded.frontend().syntax().parsed().audio,
            )
        };

        {
            let decoded = decoder
                .decode_audio_frame(audio_input_with_provenance(
                    third_payload.as_slice(),
                    context(),
                    None,
                    super::super::FullAjocFrameProvenance::new(42),
                ))
                .expect("第三帧应能解码");
            let aligned = decoded
                .output()
                .aligned_side_information()
                .expect("第三帧应取到第二帧的控制快照");
            assert_eq!(aligned.provenance(), second_provenance);
            assert_eq!(aligned.dmx_oamd_blocks(), second_dmx);
            assert_eq!(aligned.umx_oamd_blocks(), second_umx);
            assert_eq!(aligned.dmx_oamd_timing(), second_audio.dmx_timing);
            assert_eq!(aligned.umx_oamd_timing(), second_audio.umx_timing);
            assert_eq!(
                aligned.dmx_effective_oamd_timing().effective(),
                second_audio.dmx_timing
            );
            assert_eq!(
                aligned.umx_effective_oamd_timing().effective(),
                second_audio.dmx_timing
            );
            assert_eq!(
                aligned.derive_timing_from_dmx(),
                second_audio.derive_timing_from_dmx
            );
            assert_eq!(
                aligned.dmx_num_obj_info_blocks(),
                second_audio.dmx_num_obj_info_blocks
            );
            assert_eq!(
                aligned.umx_num_obj_info_blocks(),
                second_audio.umx_num_obj_info_blocks
            );
        }

        decoder.reset();
        let decoded = decoder
            .decode_audio_frame(audio_input_with_provenance(
                first_payload.as_slice(),
                context(),
                None,
                super::super::FullAjocFrameProvenance::new(43),
            ))
            .expect("reset 后应能从 warm-up 重起");
        assert!(decoded.output().observation().warmup());
        assert!(
            decoded.output().aligned_side_information().is_none(),
            "reset 后不得泄漏旧 OAMD 或 provenance"
        );
    }

    #[test]
    fn stable_audio_frames_reuse_all_borrowed_output_buffers() {
        let payload = payload();
        let mut decoder = super::super::FullAjocDecoder::new();
        let decode_layout = |decoder: &mut super::super::FullAjocDecoder| {
            let decoded = decoder
                .decode_audio_frame(audio_input(payload.as_slice()))
                .expect("稳定完整帧应能解码");
            (
                decoded.output().observation(),
                (
                    decoded.frontend().syntax().elements().as_ptr(),
                    decoded.frontend().syntax().aspx().as_ptr(),
                    decoded
                        .frontend()
                        .asf()
                        .channel(0)
                        .expect("应有一路 ASF PCM")
                        .samples()
                        .as_ptr(),
                    decoded
                        .output()
                        .diagnostic_channel(0)
                        .expect("应有一路诊断 PCM")
                        .samples()
                        .as_ptr(),
                    decoded
                        .output()
                        .reconstructed_channel(0)
                        .expect("应有一路对象 PCM")
                        .samples()
                        .as_ptr(),
                ),
            )
        };

        let first = decode_layout(&mut decoder);
        let second = decode_layout(&mut decoder);
        let third = decode_layout(&mut decoder);
        assert!(first.0.warmup());
        assert!(second.0.reconstructed());
        assert!(third.0.reconstructed());
        assert_eq!(second.1, first.1, "第二帧不得扩容任一借用缓冲");
        assert_eq!(third.1, first.1, "控制到期后也不得扩容任一借用缓冲");
    }

    #[test]
    fn downstream_failure_rolls_back_syntax_and_asf_frontends() {
        let iframe = payload_for_frame_with_spectrum(true, true);
        let dependent = payload_for_frame(false);
        let decode_asf = |decoder: &mut super::super::FullAjocDecoder| {
            decoder
                .decode_audio_frame(audio_input(iframe.as_slice()))
                .expect("非零完整帧应能解码")
                .frontend()
                .asf()
                .channel(0)
                .expect("应有一路核心带 PCM")
                .samples()
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        };

        let mut reference = super::super::FullAjocDecoder::new();
        let fresh = decode_asf(&mut reference);

        let mut decoder = super::super::FullAjocDecoder::new();
        assert_eq!(decode_asf(&mut decoder), fresh);
        let failure = decoder
            .decode_audio_frame(audio_input_with_context(
                iframe.as_slice(),
                context(),
                Some(0),
            ))
            .expect_err("无 LFE 元素不得接受 LFE 插回位置");
        assert!(matches!(
            failure,
            super::super::FullAjocAudioFrameError::Decode(_)
        ));

        let failure = decoder
            .decode_audio_frame(audio_input_with_context(
                dependent.as_slice(),
                dependent_context(),
                None,
            ))
            .expect_err("失败帧解析出的 I 帧配置不得遗留");
        assert!(matches!(
            failure,
            super::super::FullAjocAudioFrameError::Syntax(_)
        ));
        assert_eq!(
            decode_asf(&mut decoder),
            fresh,
            "失败事务必须同时切断 ASF overlap"
        );
    }

    #[test]
    fn syntax_only_frame_invalidates_existing_asf_overlap() {
        let payload = payload_for_frame_with_spectrum(true, true);
        let decode_pcm = |decoder: &mut super::super::FullAjocDecoder| {
            decoder
                .decode_frontend_frame(input(payload.as_slice()))
                .expect("非零 ASF 前端应能重建")
                .asf()
                .channel(0)
                .expect("应有一路核心带 PCM")
                .samples()
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        };

        let mut reference = super::super::FullAjocDecoder::new();
        let fresh = decode_pcm(&mut reference);
        let continued = decode_pcm(&mut reference);
        assert_ne!(continued, fresh, "夹具必须能观察到 overlap 延续");

        let mut decoder = super::super::FullAjocDecoder::new();
        assert_eq!(decode_pcm(&mut decoder), fresh);
        decoder
            .decode_syntax_frame(input(payload.as_slice()))
            .expect("syntax-only 帧应能解析");
        let restarted = decode_pcm(&mut decoder);
        assert_eq!(restarted, fresh, "syntax-only 帧必须使旧 ASF overlap 失效");
    }

    #[test]
    fn asf_failure_discards_the_frontends_inherited_syntax_state() {
        let iframe = payload_for_frame(true);
        let dependent = payload_for_frame(false);
        let mut decoder = super::super::FullAjocDecoder::new();
        decoder
            .prepare_asf_substream(0, 2048)
            .expect("测试先绑定一份冲突的 ASF 帧长");

        let failure = decoder
            .decode_frontend_frame(input_with_context(iframe.as_slice(), context()))
            .expect_err("ASF 帧长冲突必须让组合事务失败");
        assert!(matches!(
            failure,
            super::super::FullAjocFrontendError::Asf(_)
        ));

        let failure = decoder
            .decode_frontend_frame(input_with_context(
                dependent.as_slice(),
                dependent_context(),
            ))
            .expect_err("前一事务解析出的 I 帧配置不得在失败后遗留");
        assert!(matches!(
            failure,
            super::super::FullAjocFrontendError::Syntax(_)
        ));
    }

    #[test]
    fn input_exhaustion_preserves_syntax_state_and_workspace_for_retry() {
        let iframe = payload_for_frame(true);
        let dependent = payload_for_frame(false);
        let mut decoder = FullAjocSyntaxDecoder::new();
        decoder
            .prepare_substream(0, &context())
            .expect("配置建立时应能预分配语法工作区");
        let layout = workspace_layout(&decoder);
        let iframe_config = {
            let decoded = decoder
                .decode_frame(input_with_context(iframe.as_slice(), context()))
                .expect("首帧应建立语法状态");
            assert!(decoded.parsed().audio.var_element.codec_mode_aspx);
            decoded.aspx_config().expect("I 帧应提交 A-SPX 配置")
        };
        let committed = decoder.states.first().copied().expect("应有状态槽位");
        assert_ne!(committed, AudioDataState::new(), "夹具必须建立可继承状态");

        let truncated = dependent.as_slice().get(..4).expect("依赖帧载荷至少四字节");
        let error = decoder
            .decode_frame(input_with_context(truncated, dependent_context()))
            .expect_err("截断载荷必须失败");
        assert!(error.is_input_exhausted());
        assert_eq!(decoder.states.first(), Some(&committed));
        assert_eq!(
            workspace_layout(&decoder),
            layout,
            "成功帧与截断重试都不得改变预分配容量或地址"
        );

        let decoded = decoder
            .decode_frame(input_with_context(
                dependent.as_slice(),
                dependent_context(),
            ))
            .expect("补全同一依赖帧后应沿用已提交状态");
        assert_eq!(decoded.aspx_config(), Some(iframe_config));
    }

    #[test]
    fn semantic_syntax_failure_still_discards_inherited_state() {
        let payload = payload();
        let mut decoder = FullAjocSyntaxDecoder::new();
        decoder
            .decode_frame(input(payload.as_slice()))
            .expect("首帧应建立语法状态");

        let mut invalid_context = context();
        invalid_context.params.b_static_dmx = true;
        let error = decoder
            .decode_frame(input_with_context(payload.as_slice(), invalid_context))
            .expect_err("未覆盖的 static downmix 必须失败");
        assert!(!error.is_input_exhausted());
        assert_eq!(decoder.states.first(), Some(&AudioDataState::new()));
    }

    #[test]
    fn out_of_range_substream_never_allocates_a_state_slot() {
        let payload = payload();
        let mut decoder = FullAjocSyntaxDecoder::new();
        let error = decoder
            .decode_frame(FullAjocSyntaxFrameInput {
                substream_index: u32::try_from(MAX_SUBSTREAMS).unwrap_or(u32::MAX),
                ..input(payload.as_slice())
            })
            .expect_err("容量外下标必须在扩容前拒绝");
        assert_eq!(
            error,
            FullAjocSyntaxError::SubstreamIndexOutOfRange {
                index: u32::try_from(MAX_SUBSTREAMS).unwrap_or(u32::MAX),
                limit: MAX_SUBSTREAMS,
            }
        );
        assert!(decoder.states.is_empty());
    }
}
