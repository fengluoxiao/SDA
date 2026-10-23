//! 把一个 `var_channel_element()` 的全部声道驱动到 A-JOC 的输入。
//!
//! 这一层不新增规范判读，做的是**分派与对齐**：`var_element` 的三段路由已经
//! 回答了「参数在哪」「谁和谁一起驱动」「送进 A-JOC 的第几路」，这里按那三份
//! 答案逐条执行，把每一路的 `Q_out,ASPX` 写进调用方的 A-JOC 输入缓冲。
//!
//! # 三条去向，延迟各不相同
//!
//! | 作业 | 走的通路 | 相对 QMF 分析输出的延迟 |
//! | --- | --- | --- |
//! | [`AspxJob::Mono`] / [`AspxJob::Balanced`] | `aspx::pipeline` 的对应 `_qmf` 入口 | `δ_ASPX = ts_offset_hfgen` |
//! | [`AspxJob::LfeQmf`]（元素取 A-SPX） | [`bypass_frame_qmf`]，即 `Y ≡ 0` 的合并 | 同上 |
//! | [`AspxJob::LfeQmf`] / [`AspxJob::SimpleQmf`]（元素取 SIMPLE） | 只做 QMF 分析 | 无 |
//!
//! **中间那一行是判读**，见 `docs/SPEC_TRACEABILITY.md` 第 7 节：`5.7.6.5.3` 称
//! `δ_ASPX` 是 A-SPX 引入的总延迟，`6.2.10` 又把 LFE 排除在 A-SPX 之外，而
//! `Pseudocode 15` 要把它并回已经吃过这段延迟的输出。这里补上它，`Y ≡ 0` 的
//! 参照通路恰好就是「只延迟、不做带宽扩展」。
//!
//! `Pseudocode 15` 对 LFE 只有直接赋值，没有现场移位；这反而要求它的
//! `Q'_in,AJOC` 在进入该接口前已经对齐。P2 `5.7.3.6.1` 的 dry 分支同样直接使用
//! `x(ts,sb)`，A-JOC decorrelator 的延迟只属于 wet 分量，并非整个输出的统一延迟。
//! P1 `5.7.1` 的全局 6 时隙描述终端 QMF 时间轴；即使它由公共调度施加，也不会
//! 消除 LFE 旁路相对 A-SPX 通路少掉的 `δ_ASPX`。
//!
//! **第三行不能用同一条通路。** SIMPLE 整个元素都不激活 A-SPX（`6.2.10`：
//! 「A-SPX shall be active for all codec modes except for the SIMPLE codec
//! mode」），一路都没有 `δ_ASPX`，也就没有要对齐的对象；让它走 `Y ≡ 0` 的合并
//! 会凭空加进 192 或 384 个样本。
//!
//! # 输出按 A-JOC 的输入顺序摆放
//!
//! `out` 的第 `i` 项是 `Qin_AJOC[i]`，顺序由 [`VarChannelElement::ajoc_input_order`]
//! 给出——**不是传输顺序**。LFE 不占其中任何一项，单独写进 `lfe`。
//!
//! # Codec mode 变化必须显式重置
//!
//! `var_codec_mode` 每帧传输，但规范没有说明 SIMPLE 与 A-SPX 之间变化时，A-SPX
//! 的其余历史该保留、清空还是用 SIMPLE 的 QMF 推进。尤其输出延迟线若原样冻结，
//! 恢复 A-SPX 时会重放切换前的旧时隙。本层用 [`ElementChannelState`] 记住已提交
//! 模式并拒绝隐式变化；调用方只能在随机访问点 [`ElementChannelState::reset`] 后
//! 以新模式重新起解。

use crate::aspx::pipeline::{
    AspxIntermediates, EnvelopeInput, HfAdjustParams, HfGenParams, PipelineError, bypass_frame_qmf,
    frame_with_balanced_scale_factors_qmf, frame_with_scale_factors_pair_qmf,
    frame_with_scale_factors_qmf, prime_control_delay_qmf,
};
use crate::aspx::qmf::{QmfError, QmfSlot, analyse_ac4_pcm, analyse_ac4_pcm_pair};
use crate::aspx::state::AspxChannelState;
use crate::aspx::syntax::{AspxConfig, AspxData};
use crate::aspx::tables::{NUM_QMF_SUBBANDS, num_qmf_timeslots};
use crate::aspx::workspace::{AspxWorkspace, MAX_QMF_TIMESLOTS};
use crate::var_element::{AspxJob, SignalLocation, VarChannelElement};

/// 一条声道在一帧内的 QMF 矩阵，即 A-JOC 的一路输入。
///
/// 按合法帧长的上界留足；本帧实际有效的时隙数是 `pcm.len() / 64`。
pub type QmfChannelFrame = [QmfSlot; MAX_QMF_TIMESLOTS];

/// 一个未填充的 QMF 帧，供调用方预留缓冲。
#[must_use]
pub const fn empty_channel_frame() -> QmfChannelFrame {
    [QmfSlot::zero(); MAX_QMF_TIMESLOTS]
}

/// 驱动一个元素所需的、不在 `aspx_data_*()` 里的三个值。
#[derive(Debug, Clone, Copy)]
pub struct ElementParams {
    /// TOC 的基础采样率是否为 48 kHz 族，喂给 `5.7.6.3.1.4` 的 patch 表推导。
    pub base_samp_freq_48: bool,
    /// 本帧的 `aspx_config` 是否与上一个 I 帧不同，见 `MasterResetTracker`。
    pub master_reset: bool,
    /// 解码器初始化标志，只在编解码启动时为真。
    pub first_frame: bool,
}

/// 元素级驱动的一路跨帧状态。
///
/// A-SPX 本身的十一个状态收在 [`AspxChannelState`]；这里额外记住上一帧的
/// `var_codec_mode`，防止 SIMPLE 与 A-SPX 之间切换时静默沿用不连续的延迟线。
/// 规范把多处历史定义成「上一个 A-SPX interval」，却没有说明中间夹着 SIMPLE
/// 帧时如何衔接；在取得可验证的规则前，模式变化必须由调用方在随机访问点显式
/// [`Self::reset`]，不能由本层猜测性地保留或清空一部分历史。
#[derive(Debug, PartialEq)]
pub struct ElementChannelState {
    aspx: AspxChannelState,
    codec_mode_aspx: Option<bool>,
}

impl ElementChannelState {
    /// 建立全新状态，尚未提交任何 codec mode。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            aspx: AspxChannelState::new(),
            codec_mode_aspx: None,
        }
    }

    /// 丢弃全部历史，允许从新的随机访问点以任一 codec mode 起解。
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// 最近一次成功驱动所提交的 `var_codec_mode`；全新或重置后为 `None`。
    #[must_use]
    pub const fn codec_mode_aspx(&self) -> Option<bool> {
        self.codec_mode_aspx
    }

    /// 只读访问底层 A-SPX 状态，供诊断跨帧历史。
    #[must_use]
    pub const fn aspx(&self) -> &AspxChannelState {
        &self.aspx
    }
}

impl Default for ElementChannelState {
    fn default() -> Self {
        Self::new()
    }
}

/// 驱动失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveError {
    /// 逐路 PCM 的条数与本元素的声道数不符。
    ChannelCountMismatch {
        /// 本元素解出的声道数。
        expected: usize,
        /// 调用方提供的条数。
        provided: usize,
    },
    /// 同一元素的逐路 PCM 帧长不同，不能共享一条 QMF 时间轴。
    FrameLengthMismatch {
        /// 帧长不同的声道下标，按传输顺序。
        channel: usize,
        /// 第 0 路确立的公共帧长。
        expected: usize,
        /// 该路实际给出的帧长。
        provided: usize,
    },
    /// 逐声道跨帧状态不足。
    StateWorkspaceTooSmall {
        /// 本元素需要的状态数。
        needed: usize,
        /// 调用方提供的状态数。
        provided: usize,
    },
    /// 逐声道诊断中间量不足。
    IntermediateWorkspaceTooSmall {
        /// 本元素需要的中间量数。
        needed: usize,
        /// 调用方提供的中间量数。
        provided: usize,
    },
    /// A-JOC 输入缓冲不足以容纳全部全频带信号。
    OutputTooSmall {
        /// 需要的条数，即 `n_fullband_dmx_signals`。
        needed: usize,
        /// 调用方提供的条数。
        provided: usize,
    },
    /// 本元素带 LFE，但调用方没有给出 LFE 的输出缓冲。
    MissingLfeOutput,
    /// `aspx_data_*()` 里没有该路的解析结果。
    MissingAspxData {
        /// A-SPX 数据元素的下标。
        element: usize,
    },
    /// 元素取 A-SPX，却没有可用的 `aspx_config()`。
    MissingAspxConfig,
    /// 同一份跨帧状态遇到不同的 `var_codec_mode`，需要在随机访问点显式重置。
    CodecModeChangeRequiresReset {
        /// 发生变化的声道下标，按传输顺序。
        channel: usize,
        /// 最近一次成功驱动的模式；真为 A-SPX，假为 SIMPLE。
        previous_aspx: bool,
        /// 本帧要求的模式。
        current_aspx: bool,
    },
    /// 解析摘要给出的信号、作业与 A-JOC 落点不自洽。
    InvalidElementRoute,
    /// QMF 分析失败。
    Qmf(QmfError),
    /// A-SPX 通路失败。
    Pipeline(PipelineError),
}

impl From<QmfError> for DriveError {
    fn from(error: QmfError) -> Self {
        Self::Qmf(error)
    }
}

impl From<PipelineError> for DriveError {
    fn from(error: PipelineError) -> Self {
        Self::Pipeline(error)
    }
}

/// 调用方持有的逐帧缓冲与跨帧状态。
///
/// 两份工作区就够：平衡式一次要两条，其余作业一次只用第一条。工作区约 127 KiB
/// 一份，逐声道各留一份既没必要也放不下。
#[derive(Debug)]
pub struct DriveWorkspace<'a> {
    /// 两份 A-SPX 帧内缓冲。
    pub aspx: [&'a mut AspxWorkspace; 2],
    /// 逐声道的跨帧状态，按 [`VarChannelElement::signals`] 的顺序。
    pub states: &'a mut [ElementChannelState],
    /// 逐声道的诊断中间量，长度同 `states`。
    pub intermediates: &'a mut [AspxIntermediates],
}

/// 驱动一个 `var_channel_element()` 的全部声道。
///
/// `pcm` 按 [`VarChannelElement::signals`] 的顺序逐路给出核心带 PCM，条数含
/// LFE。`out` 按 A-JOC 的输入顺序接收全频带各路的 `Q_out,ASPX`，`lfe` 单独接收
/// LFE；全部 PCM 必须共用表 189/192 的同一合法帧长，两处输出的有效时隙数都是
/// `pcm[0].len() / 64`。
///
/// # Errors
///
/// 见 [`DriveError`]。元素级结构错误（帧布局、缓冲容量、路由、缺失配置或数据、
/// codec mode 变化）会在任何状态、工作区与输出改写之前返回。进入底层 A-SPX
/// 通路后若仍返回 [`DriveError::Pipeline`]，此前声道可能已经推进；调用方必须丢弃
/// 本元素的全部输出，重置逐声道状态，并从随机访问点重新起解。
#[expect(
    clippy::too_many_arguments,
    reason = "驱动一个元素就是要同时收下解析结果、逐帧参数、逐路 PCM、三组可变\
              状态与两处输出；聚成上下文结构只会把这一层的真实接线藏起来"
)]
pub fn drive_element(
    element: &VarChannelElement,
    aspx: &[AspxData],
    config: Option<&AspxConfig>,
    params: ElementParams,
    pcm: &[&[f32]],
    workspace: DriveWorkspace<'_>,
    out: &mut [QmfChannelFrame],
    lfe: Option<&mut QmfChannelFrame>,
) -> Result<(), DriveError> {
    let channels =
        usize::from(element.n_dmx_signals).saturating_add(usize::from(element.b_has_lfe));
    if pcm.len() != channels {
        return Err(DriveError::ChannelCountMismatch {
            expected: channels,
            provided: pcm.len(),
        });
    }
    let fullband = usize::from(element.n_dmx_signals);
    let DriveWorkspace {
        aspx: [first_workspace, second_workspace],
        states,
        intermediates,
    } = workspace;
    if states.len() < channels {
        return Err(DriveError::StateWorkspaceTooSmall {
            needed: channels,
            provided: states.len(),
        });
    }
    if intermediates.len() < channels {
        return Err(DriveError::IntermediateWorkspaceTooSmall {
            needed: channels,
            provided: intermediates.len(),
        });
    }
    if out.len() < fullband {
        return Err(DriveError::OutputTooSmall {
            needed: fullband,
            provided: out.len(),
        });
    }
    if element.b_has_lfe && lfe.is_none() {
        return Err(DriveError::MissingLfeOutput);
    }
    if element.codec_mode_aspx && config.is_none() {
        return Err(DriveError::MissingAspxConfig);
    }

    let timeslots = validate_common_frame(pcm)?;
    for (channel, state) in states.iter().take(channels).enumerate() {
        if let Some(previous_aspx) = state.codec_mode_aspx
            && previous_aspx != element.codec_mode_aspx
        {
            return Err(DriveError::CodecModeChangeRequiresReset {
                channel,
                previous_aspx,
                current_aspx: element.codec_mode_aspx,
            });
        }
    }

    // 传输顺序里的第几路 → A-JOC 输入的第几项。LFE 不在其中。
    let slot_of = ajoc_slots(element, channels, fullband)?;
    preflight_jobs(element, aspx, config, params, &slot_of, channels, fullband)?;

    let mut lfe = lfe;

    let mut jobs = element.aspx_jobs().peekable();
    while let Some(job) = jobs.next() {
        match job {
            AspxJob::LfeQmf(signal) => {
                let source = source_of(element, signal)?;
                let Some(target) = lfe.as_deref_mut() else {
                    return Err(DriveError::MissingLfeOutput);
                };
                let input = pcm.get(source).ok_or(DriveError::InvalidElementRoute)?;
                let state = states
                    .get_mut(source)
                    .ok_or(DriveError::InvalidElementRoute)?;
                if element.codec_mode_aspx {
                    // 判读：补上 `δ_ASPX`，与经 A-SPX 的各路对齐。
                    let produced = bypass_frame_qmf(input, &mut state.aspx, first_workspace)?;
                    copy_frame(produced, target);
                } else {
                    // SIMPLE：一路都没有 `δ_ASPX`，不能凭空加。
                    analyse_only(input, &mut state.aspx, first_workspace, target, timeslots)?;
                }
            }
            AspxJob::SimpleQmf(signal) => {
                if let Some(AspxJob::SimpleQmf(second_signal)) = jobs.peek().copied() {
                    let _ = jobs.next();
                    let sources = [
                        source_of(element, signal)?,
                        source_of(element, second_signal)?,
                    ];
                    let inputs = [
                        pcm.get(sources[0])
                            .copied()
                            .ok_or(DriveError::InvalidElementRoute)?,
                        pcm.get(sources[1])
                            .copied()
                            .ok_or(DriveError::InvalidElementRoute)?,
                    ];
                    let Some((first_state, second_state)) = pair_mut(states, sources) else {
                        return Err(DriveError::InvalidElementRoute);
                    };
                    let targets = [
                        validate_target(sources[0], &slot_of, out.len())?,
                        validate_target(sources[1], &slot_of, out.len())?,
                    ];
                    let Some((first_target, second_target)) = pair_mut(out, targets) else {
                        return Err(DriveError::InvalidElementRoute);
                    };
                    analyse_only_pair(
                        inputs,
                        [&mut first_state.aspx, &mut second_state.aspx],
                        [first_workspace, second_workspace],
                        [first_target, second_target],
                        timeslots,
                    )?;
                    continue;
                }
                let source = source_of(element, signal)?;
                let input = pcm.get(source).ok_or(DriveError::InvalidElementRoute)?;
                let state = states
                    .get_mut(source)
                    .ok_or(DriveError::InvalidElementRoute)?;
                let target = target_for(source, &slot_of, out)?;
                analyse_only(input, &mut state.aspx, first_workspace, target, timeslots)?;
            }
            AspxJob::Mono(signal) => {
                if let Some(AspxJob::Mono(second_signal)) = jobs.peek().copied() {
                    let _ = jobs.next();
                    let signals = [signal, second_signal];
                    let sources = [
                        source_of(element, signals[0])?,
                        source_of(element, signals[1])?,
                    ];
                    let (first_element, first_channel) =
                        signals[0].aspx.ok_or(DriveError::InvalidElementRoute)?;
                    let (second_element, second_channel) =
                        signals[1].aspx.ok_or(DriveError::InvalidElementRoute)?;
                    let first_element = usize::from(first_element);
                    let second_element = usize::from(second_element);
                    let first_data =
                        aspx.get(first_element).ok_or(DriveError::MissingAspxData {
                            element: first_element,
                        })?;
                    let second_data =
                        aspx.get(second_element)
                            .ok_or(DriveError::MissingAspxData {
                                element: second_element,
                            })?;
                    let first_inputs =
                        channel_inputs(first_data, config, usize::from(first_channel), params)?;
                    let second_inputs =
                        channel_inputs(second_data, config, usize::from(second_channel), params)?;
                    let inputs = [
                        pcm.get(sources[0])
                            .copied()
                            .ok_or(DriveError::InvalidElementRoute)?,
                        pcm.get(sources[1])
                            .copied()
                            .ok_or(DriveError::InvalidElementRoute)?,
                    ];
                    let Some((first_state, second_state)) = pair_mut(states, sources) else {
                        return Err(DriveError::InvalidElementRoute);
                    };
                    let Some((first_mid, second_mid)) = pair_mut(intermediates, sources) else {
                        return Err(DriveError::InvalidElementRoute);
                    };
                    let produced = frame_with_scale_factors_pair_qmf(
                        inputs,
                        [&first_data.bands, &second_data.bands],
                        [first_inputs.hf_gen, second_inputs.hf_gen],
                        [first_inputs.envelopes, second_inputs.envelopes],
                        [first_inputs.hf_adjust, second_inputs.hf_adjust],
                        [&mut first_state.aspx, &mut second_state.aspx],
                        [first_workspace, second_workspace],
                        [first_mid, second_mid],
                    )?;
                    let targets = [
                        validate_target(sources[0], &slot_of, out.len())?,
                        validate_target(sources[1], &slot_of, out.len())?,
                    ];
                    let Some((first_target, second_target)) = pair_mut(out, targets) else {
                        return Err(DriveError::InvalidElementRoute);
                    };
                    copy_frame(produced[0], first_target);
                    copy_frame(produced[1], second_target);
                    continue;
                }
                let source = source_of(element, signal)?;
                let (element_index, channel) =
                    signal.aspx.ok_or(DriveError::InvalidElementRoute)?;
                let element_index = usize::from(element_index);
                let data = aspx.get(element_index).ok_or(DriveError::MissingAspxData {
                    element: element_index,
                })?;
                let inputs = channel_inputs(data, config, usize::from(channel), params)?;
                let input = pcm.get(source).ok_or(DriveError::InvalidElementRoute)?;
                let state = states
                    .get_mut(source)
                    .ok_or(DriveError::InvalidElementRoute)?;
                let mid = intermediates
                    .get_mut(source)
                    .ok_or(DriveError::InvalidElementRoute)?;
                let produced = frame_with_scale_factors_qmf(
                    input,
                    &data.bands,
                    inputs.hf_gen,
                    inputs.envelopes,
                    inputs.hf_adjust,
                    &mut state.aspx,
                    first_workspace,
                    mid,
                )?;
                let target = target_for(source, &slot_of, out)?;
                copy_frame(produced, target);
            }
            AspxJob::Balanced(pair) => {
                let [left, right] = pair;
                let sources = [source_of(element, left)?, source_of(element, right)?];
                let (element_index, first_channel) =
                    left.aspx.ok_or(DriveError::InvalidElementRoute)?;
                let (right_element, second_channel) =
                    right.aspx.ok_or(DriveError::InvalidElementRoute)?;
                if element_index != right_element || [first_channel, second_channel] != [0, 1] {
                    return Err(DriveError::InvalidElementRoute);
                }
                let element_index = usize::from(element_index);
                let data = aspx.get(element_index).ok_or(DriveError::MissingAspxData {
                    element: element_index,
                })?;
                let first_inputs =
                    channel_inputs(data, config, usize::from(first_channel), params)?;
                let second_inputs =
                    channel_inputs(data, config, usize::from(second_channel), params)?;

                let first_pcm = pcm.get(sources[0]).ok_or(DriveError::InvalidElementRoute)?;
                let second_pcm = pcm.get(sources[1]).ok_or(DriveError::InvalidElementRoute)?;
                let Some((first_state, second_state)) = pair_mut(states, sources) else {
                    return Err(DriveError::InvalidElementRoute);
                };
                let Some((first_mid, second_mid)) = pair_mut(intermediates, sources) else {
                    return Err(DriveError::InvalidElementRoute);
                };
                let produced = frame_with_balanced_scale_factors_qmf(
                    [first_pcm, second_pcm],
                    &data.bands,
                    [first_inputs.hf_gen, second_inputs.hf_gen],
                    [first_inputs.envelopes, second_inputs.envelopes],
                    [first_inputs.hf_adjust, second_inputs.hf_adjust],
                    [&mut first_state.aspx, &mut second_state.aspx],
                    [first_workspace, second_workspace],
                    [first_mid, second_mid],
                )?;
                for (source, frame) in sources.into_iter().zip(produced) {
                    let target = target_for(source, &slot_of, out)?;
                    copy_frame(frame, target);
                }
            }
        }
    }

    // codec mode 与整元素的输出一起提交；任何错误都不改变这个判定，调用方仍能
    // 区分「从未成功」与「同一模式下底层通路部分推进」。
    for state in states.iter_mut().take(channels) {
        state.codec_mode_aspx = Some(element.codec_mode_aspx);
    }

    Ok(())
}

/// 在表 188 的首份控制数据到期前，用 frame-aligned PCM 预热 QMF 历史。
///
/// 路由、输出顺序与 [`drive_element`] 完全相同，但 A-SPX 声道只执行分析、低带
/// 与 `Y = 0` 的输出合并；不会消费包络或推进任何逐控制帧状态。LFE 仍走同一档
/// `delta_ASPX`，SIMPLE 仍只做分析。调用方应把返回的 QMF 帧照常交给终端合成，
/// 使分析与合成两侧都在第一份到期控制之前建立历史。
///
/// `config` 与 `params` 只用于复用 [`drive_element`] 的完整静态预检；本函数不会
/// 提交 `master_reset` 或 `first_frame`。
///
/// # Errors
///
/// 结构错误在任何状态改写之前返回。进入逐声道预热后若底层通路失败，调用方须
/// 与 [`drive_element`] 一样丢弃整条元素状态。
#[expect(
    clippy::too_many_arguments,
    reason = "预热必须与正式驱动共享元素、控制摘要、逐路 PCM、状态及两处输出"
)]
pub fn prime_control_delay_element(
    element: &VarChannelElement,
    aspx: &[AspxData],
    config: Option<&AspxConfig>,
    params: ElementParams,
    pcm: &[&[f32]],
    workspace: DriveWorkspace<'_>,
    out: &mut [QmfChannelFrame],
    lfe: Option<&mut QmfChannelFrame>,
) -> Result<(), DriveError> {
    let channels =
        usize::from(element.n_dmx_signals).saturating_add(usize::from(element.b_has_lfe));
    if pcm.len() != channels {
        return Err(DriveError::ChannelCountMismatch {
            expected: channels,
            provided: pcm.len(),
        });
    }
    let fullband = usize::from(element.n_dmx_signals);
    let DriveWorkspace {
        aspx: [first_workspace, second_workspace],
        states,
        intermediates,
    } = workspace;
    if states.len() < channels {
        return Err(DriveError::StateWorkspaceTooSmall {
            needed: channels,
            provided: states.len(),
        });
    }
    if intermediates.len() < channels {
        return Err(DriveError::IntermediateWorkspaceTooSmall {
            needed: channels,
            provided: intermediates.len(),
        });
    }
    if out.len() < fullband {
        return Err(DriveError::OutputTooSmall {
            needed: fullband,
            provided: out.len(),
        });
    }
    if element.b_has_lfe && lfe.is_none() {
        return Err(DriveError::MissingLfeOutput);
    }
    if element.codec_mode_aspx && config.is_none() {
        return Err(DriveError::MissingAspxConfig);
    }

    let timeslots = validate_common_frame(pcm)?;
    let slot_of = ajoc_slots(element, channels, fullband)?;
    preflight_jobs(element, aspx, config, params, &slot_of, channels, fullband)?;

    let mut lfe = lfe;
    for job in element.aspx_jobs() {
        match job {
            AspxJob::LfeQmf(signal) => {
                let source = source_of(element, signal)?;
                let input = pcm.get(source).ok_or(DriveError::InvalidElementRoute)?;
                let state = states
                    .get_mut(source)
                    .ok_or(DriveError::InvalidElementRoute)?;
                let Some(target) = lfe.as_deref_mut() else {
                    return Err(DriveError::MissingLfeOutput);
                };
                if element.codec_mode_aspx {
                    let produced = bypass_frame_qmf(input, &mut state.aspx, first_workspace)?;
                    copy_frame(produced, target);
                } else {
                    analyse_only(input, &mut state.aspx, first_workspace, target, timeslots)?;
                }
            }
            AspxJob::SimpleQmf(signal) => {
                let source = source_of(element, signal)?;
                let input = pcm.get(source).ok_or(DriveError::InvalidElementRoute)?;
                let state = states
                    .get_mut(source)
                    .ok_or(DriveError::InvalidElementRoute)?;
                let target = target_for(source, &slot_of, out)?;
                analyse_only(input, &mut state.aspx, first_workspace, target, timeslots)?;
            }
            AspxJob::Mono(signal) => {
                let source = source_of(element, signal)?;
                let (element_index, _) = signal.aspx.ok_or(DriveError::InvalidElementRoute)?;
                let element_index = usize::from(element_index);
                let data = aspx.get(element_index).ok_or(DriveError::MissingAspxData {
                    element: element_index,
                })?;
                let input = pcm.get(source).ok_or(DriveError::InvalidElementRoute)?;
                let state = states
                    .get_mut(source)
                    .ok_or(DriveError::InvalidElementRoute)?;
                let produced =
                    prime_control_delay_qmf(input, &data.bands, &mut state.aspx, first_workspace)?;
                let target = target_for(source, &slot_of, out)?;
                copy_frame(produced, target);
            }
            AspxJob::Balanced([left, right]) => {
                let sources = [source_of(element, left)?, source_of(element, right)?];
                let (element_index, _) = left.aspx.ok_or(DriveError::InvalidElementRoute)?;
                let element_index = usize::from(element_index);
                let data = aspx.get(element_index).ok_or(DriveError::MissingAspxData {
                    element: element_index,
                })?;
                let first_pcm = pcm.get(sources[0]).ok_or(DriveError::InvalidElementRoute)?;
                let second_pcm = pcm.get(sources[1]).ok_or(DriveError::InvalidElementRoute)?;
                let Some((first_state, second_state)) = pair_mut(states, sources) else {
                    return Err(DriveError::InvalidElementRoute);
                };
                let first = prime_control_delay_qmf(
                    first_pcm,
                    &data.bands,
                    &mut first_state.aspx,
                    first_workspace,
                )?;
                let target = target_for(sources[0], &slot_of, out)?;
                copy_frame(first, target);
                let second = prime_control_delay_qmf(
                    second_pcm,
                    &data.bands,
                    &mut second_state.aspx,
                    second_workspace,
                )?;
                let target = target_for(sources[1], &slot_of, out)?;
                copy_frame(second, target);
            }
        }
    }

    for state in states.iter_mut().take(channels) {
        state.codec_mode_aspx = Some(element.codec_mode_aspx);
    }
    Ok(())
}

/// 核对逐路 PCM 共用同一个、且属于表 189/192 的帧布局。
fn validate_common_frame(pcm: &[&[f32]]) -> Result<usize, DriveError> {
    let samples = pcm.first().map_or(0, |input| input.len());
    for (channel, input) in pcm.iter().enumerate().skip(1) {
        if input.len() != samples {
            return Err(DriveError::FrameLengthMismatch {
                channel,
                expected: samples,
                provided: input.len(),
            });
        }
    }

    let subbands = usize::from(NUM_QMF_SUBBANDS);
    if samples.checked_rem(subbands) != Some(0) {
        return Err(PipelineError::UnalignedInput { samples }.into());
    }
    let Some(timeslots) = samples.checked_div(subbands) else {
        return Err(PipelineError::UnsupportedFrameLength { samples }.into());
    };
    if timeslots > MAX_QMF_TIMESLOTS {
        return Err(PipelineError::FrameTooLong {
            timeslots,
            capacity: MAX_QMF_TIMESLOTS,
        }
        .into());
    }
    let Ok(frame_len_base) = u16::try_from(samples) else {
        return Err(PipelineError::UnsupportedFrameLength { samples }.into());
    };
    // 查得到就是合法帧长。**不要再拿它与 `timeslots` 比一次**——
    // `num_qmf_timeslots` 算的就是 `frame_len_base / 64`，而这里的 `samples`
    // 与 `timeslots` 同源，那个比较恒假，注入删掉它一条判据都不响。
    if num_qmf_timeslots(frame_len_base).is_none() {
        return Err(PipelineError::UnsupportedFrameLength { samples }.into());
    }
    Ok(timeslots)
}

/// 建立传输顺序到 A-JOC 输入顺序的逆表，并核对它是全频带信号的双射。
fn ajoc_slots(
    element: &VarChannelElement,
    channels: usize,
    fullband: usize,
) -> Result<[usize; crate::var_element::MAX_SIGNALS], DriveError> {
    let mut slot_of = [usize::MAX; crate::var_element::MAX_SIGNALS];
    let mut targets = 0usize;
    for (target, signal) in element.ajoc_input_order().enumerate() {
        let source = source_of(element, signal)?;
        if source >= channels || target >= fullband {
            return Err(DriveError::InvalidElementRoute);
        }
        let slot = slot_of
            .get_mut(source)
            .ok_or(DriveError::InvalidElementRoute)?;
        if *slot != usize::MAX {
            return Err(DriveError::InvalidElementRoute);
        }
        *slot = target;
        targets = targets.saturating_add(1);
    }
    if targets != fullband {
        return Err(DriveError::InvalidElementRoute);
    }
    for source in usize::from(element.b_has_lfe)..channels {
        if slot_of.get(source).copied() == Some(usize::MAX) {
            return Err(DriveError::InvalidElementRoute);
        }
    }
    Ok(slot_of)
}

/// 在第一条声道通路启动前，核对全部作业的静态输入与覆盖关系。
fn preflight_jobs(
    element: &VarChannelElement,
    aspx: &[AspxData],
    config: Option<&AspxConfig>,
    params: ElementParams,
    slot_of: &[usize; crate::var_element::MAX_SIGNALS],
    channels: usize,
    fullband: usize,
) -> Result<(), DriveError> {
    let mut seen = [false; crate::var_element::MAX_SIGNALS];
    for job in element.aspx_jobs() {
        match job {
            AspxJob::LfeQmf(signal) => {
                let source = source_of(element, signal)?;
                if !element.b_has_lfe || source != 0 {
                    return Err(DriveError::InvalidElementRoute);
                }
                mark_source(&mut seen, source, channels)?;
            }
            AspxJob::SimpleQmf(signal) => {
                if element.codec_mode_aspx {
                    return Err(DriveError::InvalidElementRoute);
                }
                let source = source_of(element, signal)?;
                validate_target(source, slot_of, fullband)?;
                mark_source(&mut seen, source, channels)?;
            }
            AspxJob::Mono(signal) => {
                if !element.codec_mode_aspx {
                    return Err(DriveError::InvalidElementRoute);
                }
                let source = source_of(element, signal)?;
                let (element_index, channel) =
                    signal.aspx.ok_or(DriveError::InvalidElementRoute)?;
                let element_index = usize::from(element_index);
                let data = aspx.get(element_index).ok_or(DriveError::MissingAspxData {
                    element: element_index,
                })?;
                channel_inputs(data, config, usize::from(channel), params)?;
                validate_target(source, slot_of, fullband)?;
                mark_source(&mut seen, source, channels)?;
            }
            AspxJob::Balanced([left, right]) => {
                if !element.codec_mode_aspx {
                    return Err(DriveError::InvalidElementRoute);
                }
                let sources = [source_of(element, left)?, source_of(element, right)?];
                let (element_index, first_channel) =
                    left.aspx.ok_or(DriveError::InvalidElementRoute)?;
                let (right_element, second_channel) =
                    right.aspx.ok_or(DriveError::InvalidElementRoute)?;
                if element_index != right_element || [first_channel, second_channel] != [0, 1] {
                    return Err(DriveError::InvalidElementRoute);
                }
                let element_index = usize::from(element_index);
                let data = aspx.get(element_index).ok_or(DriveError::MissingAspxData {
                    element: element_index,
                })?;
                channel_inputs(data, config, usize::from(first_channel), params)?;
                channel_inputs(data, config, usize::from(second_channel), params)?;
                for source in sources {
                    validate_target(source, slot_of, fullband)?;
                    mark_source(&mut seen, source, channels)?;
                }
            }
        }
    }
    if !seen.iter().take(channels).all(|covered| *covered) {
        return Err(DriveError::InvalidElementRoute);
    }
    Ok(())
}

fn mark_source(
    seen: &mut [bool; crate::var_element::MAX_SIGNALS],
    source: usize,
    channels: usize,
) -> Result<(), DriveError> {
    if source >= channels {
        return Err(DriveError::InvalidElementRoute);
    }
    let covered = seen
        .get_mut(source)
        .ok_or(DriveError::InvalidElementRoute)?;
    if *covered {
        return Err(DriveError::InvalidElementRoute);
    }
    *covered = true;
    Ok(())
}

fn validate_target(
    source: usize,
    slot_of: &[usize; crate::var_element::MAX_SIGNALS],
    fullband: usize,
) -> Result<usize, DriveError> {
    slot_of
        .get(source)
        .copied()
        .filter(|target| *target < fullband)
        .ok_or(DriveError::InvalidElementRoute)
}

fn target_for<'a>(
    source: usize,
    slot_of: &[usize; crate::var_element::MAX_SIGNALS],
    out: &'a mut [QmfChannelFrame],
) -> Result<&'a mut QmfChannelFrame, DriveError> {
    let target = validate_target(source, slot_of, out.len())?;
    out.get_mut(target).ok_or(DriveError::InvalidElementRoute)
}

/// 某个落点在 [`VarChannelElement::signals`] 里排第几。
///
/// **查不到没能构造出可达的输入**：`ajoc_slots` 与 `preflight_jobs` 已经把作业、
/// 信号与 A-JOC 落点核对成一一对应，主循环里的每个 `signal` 都来自同一个
/// `signals()`。注入「查不到就退回第 0 路」不会有判据响，属纵深防御——但退回
/// 一个看着合理的下标正是这一层最该避免的写法，故仍然报错。
fn source_of(element: &VarChannelElement, signal: SignalLocation) -> Result<usize, DriveError> {
    transmission_index(element, signal).ok_or(DriveError::InvalidElementRoute)
}

/// 只做 `4.8.3.9` 的 QMF 分析，不经 A-SPX，也不引入 `δ_ASPX`。
fn analyse_only(
    pcm: &[f32],
    state: &mut AspxChannelState,
    workspace: &mut AspxWorkspace,
    out: &mut QmfChannelFrame,
    timeslots: usize,
) -> Result<(), DriveError> {
    let Some(slots) = workspace.q_in.get_mut(..timeslots) else {
        return Err(DriveError::Qmf(QmfError::SlotCountMismatch {
            expected: timeslots,
            provided: MAX_QMF_TIMESLOTS,
        }));
    };
    analyse_ac4_pcm(pcm, &mut state.analysis, slots)?;
    copy_frame(slots, out);
    Ok(())
}

/// 两路 SIMPLE 声道共用垂直 QMF 分析，其余状态与输出仍逐声道独立。
fn analyse_only_pair(
    pcm: [&[f32]; 2],
    states: [&mut AspxChannelState; 2],
    workspaces: [&mut AspxWorkspace; 2],
    out: [&mut QmfChannelFrame; 2],
    timeslots: usize,
) -> Result<(), DriveError> {
    let [first_state, second_state] = states;
    let [first_workspace, second_workspace] = workspaces;
    let [first_out, second_out] = out;
    let first_slots = first_workspace
        .q_in
        .get_mut(..timeslots)
        .ok_or(DriveError::Qmf(QmfError::SlotCountMismatch {
            expected: timeslots,
            provided: MAX_QMF_TIMESLOTS,
        }))?;
    let second_slots = second_workspace
        .q_in
        .get_mut(..timeslots)
        .ok_or(DriveError::Qmf(QmfError::SlotCountMismatch {
            expected: timeslots,
            provided: MAX_QMF_TIMESLOTS,
        }))?;
    analyse_ac4_pcm_pair(
        pcm,
        [&mut first_state.analysis, &mut second_state.analysis],
        [first_slots, second_slots],
    )?;
    copy_frame(first_slots, first_out);
    copy_frame(second_slots, second_out);
    Ok(())
}

/// 把本帧有效的时隙抄进输出缓冲，其余位置清零。
///
/// 清零不是多余的：缓冲跨帧复用，帧长变短时留下的旧尾部会被下游按新长度之外的
/// 位置读到——虽然当前调用方都按有效长度切片，但那是调用方的约定，不是本函数
/// 能保证的。
fn copy_frame(source: &[QmfSlot], target: &mut QmfChannelFrame) {
    for (slot, value) in target.iter_mut().zip(source) {
        *slot = *value;
    }
    for slot in target.iter_mut().skip(source.len()) {
        *slot = QmfSlot::zero();
    }
}

/// 同时取出两个互不相同下标处的可变引用。
fn pair_mut<T>(items: &mut [T], indices: [usize; 2]) -> Option<(&mut T, &mut T)> {
    let [first, second] = indices;
    if first == second {
        return None;
    }
    let (low, high) = if first < second {
        (first, second)
    } else {
        (second, first)
    };
    if high >= items.len() {
        return None;
    }
    let (head, tail) = items.split_at_mut(high);
    let low = head.get_mut(low)?;
    let high = tail.first_mut()?;
    if first < second {
        Some((low, high))
    } else {
        Some((high, low))
    }
}

/// 一路声道的三组逐帧输入。
struct ChannelInputs<'a> {
    hf_gen: HfGenParams<'a>,
    hf_adjust: HfAdjustParams<'a>,
    envelopes: EnvelopeInput<'a>,
}

fn channel_inputs<'a>(
    data: &'a AspxData,
    config: Option<&AspxConfig>,
    channel: usize,
    params: ElementParams,
) -> Result<ChannelInputs<'a>, DriveError> {
    let config = config.ok_or(DriveError::MissingAspxConfig)?;
    let parsed = crate::aspx::pipeline::ChannelInputs::from_parsed(
        data,
        config,
        channel,
        params.base_samp_freq_48,
        params.master_reset,
        params.first_frame,
    )?;
    Ok(ChannelInputs {
        hf_gen: parsed.hf_gen,
        hf_adjust: parsed.hf_adjust,
        envelopes: parsed.envelopes,
    })
}

/// 某个落点在 [`VarChannelElement::signals`] 里排第几。
fn transmission_index(element: &VarChannelElement, signal: SignalLocation) -> Option<usize> {
    element.signals().position(|candidate| candidate == signal)
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "下标由同一用例构造的帧长与频带表派生；越界即是该用例要报告的失败"
)]
mod tests {
    use super::*;
    use crate::aspx::bands::AspxBandTables;
    use crate::aspx::frames::{AspxInterval, AspxIntervalParams};
    use crate::aspx::syntax::{AspxChannelFraming, AspxEnvelopes, AspxHfGen};
    use crate::aspx::tables::NUM_QMF_SUBBANDS;

    extern crate std;
    use std::vec;
    use std::vec::Vec;

    const SUBBANDS: usize = NUM_QMF_SUBBANDS as usize;
    /// 表 192 的第一行：2 048 采样、倍率 2、`ts_offset_hfgen = 6`。
    const SLOTS: usize = 32;
    const FRAME: usize = SLOTS * SUBBANDS;
    /// A-SPX 时隙数，单位是 ATS：`num_ts_in_ats = 2`，故为 QMF 时隙数的一半。
    const ATS: u8 = (SLOTS / 2) as u8;

    fn bands() -> AspxBandTables {
        AspxBandTables::derive(false, 0, 0, 0, 1).expect("频带表")
    }

    fn config() -> AspxConfig {
        AspxConfig {
            quant_mode_env: false,
            start_freq: 0,
            stop_freq: 0,
            master_freq_scale: false,
            interpolation: false,
            preflat: false,
            limiter: false,
            noise_sbg: 0,
            num_env_bits_fixfix: false,
            freq_res_mode: 3,
        }
    }

    /// 占满整帧的 FIXFIX 区间，两个信号包络，全部频率方向差分。
    fn framing() -> AspxChannelFraming {
        let mut params = AspxIntervalParams::fixfix(2);
        params.freq_res = [true, true, false, false, false];
        let interval = AspxInterval::derive(&params, ATS, 0, true, i16::from(ATS)).expect("区间");
        AspxChannelFraming::for_test(params, interval, false, &[false, false], &[false, false])
    }

    fn envelopes(bands: &AspxBandTables, seed: i16) -> AspxEnvelopes {
        let high: Vec<i16> = (0..bands.num_sbg_sig_highres())
            .map(|index| {
                if index == 0 {
                    seed.saturating_add(4)
                } else {
                    0
                }
            })
            .collect();
        let noise: Vec<i16> = (0..bands.num_sbg_noise())
            .map(|index| {
                if index == 0 {
                    seed.saturating_add(2)
                } else {
                    0
                }
            })
            .collect();
        AspxEnvelopes::for_test(&[&high, &high], &[&noise, &noise])
    }

    fn hfgen(bands: &AspxBandTables) -> AspxHfGen {
        let modes = vec![0u8; usize::from(bands.num_sbg_noise())];
        // `add_harmonic` 按高分辨率信号子带组计数，不是按 A-SPX 子带数。
        let harmonics = vec![false; usize::from(bands.num_sbg_sig_highres())];
        AspxHfGen::for_test(&modes, &harmonics)
    }

    /// 一个双声道元素（`aspx_balance = 0`）加一个单声道元素。
    fn aspx_elements(bands: &AspxBandTables) -> Vec<AspxData> {
        let frame = framing();
        vec![
            AspxData::for_test_balanced(
                2,
                Some(false),
                *bands,
                [frame, frame],
                [hfgen(bands), hfgen(bands)],
                [envelopes(bands, 0), envelopes(bands, 1)],
            ),
            AspxData::for_test_balanced(
                1,
                None,
                *bands,
                [frame, frame],
                [hfgen(bands), hfgen(bands)],
                [envelopes(bands, 2), envelopes(bands, 0)],
            ),
        ]
    }

    fn ramp(len: usize, scale: f32) -> Vec<f32> {
        (0..len)
            .map(|index| ((index % 97) as f32 - 48.0) * scale)
            .collect()
    }

    fn params() -> ElementParams {
        ElementParams {
            base_samp_freq_48: true,
            master_reset: false,
            first_frame: true,
        }
    }

    struct Rig {
        states: Vec<ElementChannelState>,
        intermediates: Vec<AspxIntermediates>,
        first: AspxWorkspace,
        second: AspxWorkspace,
    }

    impl Rig {
        fn new(channels: usize) -> Self {
            let mut states = Vec::new();
            states.resize_with(channels, ElementChannelState::new);
            let mut intermediates = Vec::new();
            intermediates.resize_with(channels, AspxIntermediates::default);
            Self {
                states,
                intermediates,
                first: AspxWorkspace::new(),
                second: AspxWorkspace::new(),
            }
        }

        fn workspace(&mut self) -> DriveWorkspace<'_> {
            DriveWorkspace {
                aspx: [&mut self.first, &mut self.second],
                states: &mut self.states,
                intermediates: &mut self.intermediates,
            }
        }
    }

    /// 单独跑一遍 `Y ≡ 0` 的通路，即「只延迟、不做带宽扩展」。
    fn delayed_only(input: &[f32]) -> Vec<QmfSlot> {
        let mut state = AspxChannelState::new();
        let mut workspace = AspxWorkspace::new();
        bypass_frame_qmf(input, &mut state, &mut workspace)
            .expect("参照通路")
            .to_vec()
    }

    /// 裸 QMF 分析，没有任何延迟。
    fn analysed_only(input: &[f32]) -> Vec<QmfSlot> {
        let mut state = AspxChannelState::new();
        let mut slots = vec![QmfSlot::zero(); input.len() / SUBBANDS];
        analyse_ac4_pcm(input, &mut state.analysis, &mut slots).expect("分析");
        slots
    }

    fn sentinel_frame(value: f32) -> QmfChannelFrame {
        let mut frame = empty_channel_frame();
        frame[0].re[0] = value;
        frame[MAX_QMF_TIMESLOTS - 1].im[SUBBANDS - 1] = -value;
        frame
    }

    fn assert_fresh_states(rig: &Rig) {
        for state in &rig.states {
            assert_eq!(state, &ElementChannelState::new());
        }
    }

    /// **LFE 必须和经 A-SPX 的各路走同一条时间轴。**
    ///
    /// 判读见规范可追踪性第 7 节：`δ_ASPX` 是 A-SPX 引入的总延迟，LFE 不经
    /// A-SPX，却要并回一份已经吃过它的输出。判据把同一段 PCM 同时喂给 LFE 与
    /// 各路全频带信号，要求 LFE 的输出**逐位等于**「只延迟」的参照；再要求某
    /// 一路全频带信号的 `Q_out` 减去它的 `Y` 也等于同一份参照——两侧被延迟了
    /// 同样多。
    #[test]
    fn the_lfe_carries_the_same_delay_as_the_aspx_channels() {
        let bands = bands();
        let data = aspx_elements(&bands);
        let config = config();
        // 3 路全频带加 LFE：一个 `aspx_data_2ch()` 加一个 `aspx_data_1ch()`。
        let element = VarChannelElement::for_test(true, Some(false), 3, true, &[false]);
        let signal = ramp(FRAME, 0.5);
        let pcm: Vec<&[f32]> = vec![&signal, &signal, &signal, &signal];

        let mut rig = Rig::new(4);
        let mut out = vec![empty_channel_frame(); 3];
        let mut lfe = empty_channel_frame();
        drive_element(
            &element,
            &data,
            Some(&config),
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            Some(&mut lfe),
        )
        .expect("驱动");

        let delayed = delayed_only(&signal);
        assert_ne!(
            delayed,
            analysed_only(&signal),
            "夹具前提：延迟与不延迟必须可区分"
        );
        assert_eq!(
            &lfe[..SLOTS],
            delayed.as_slice(),
            "LFE 应恰好是延迟后的输入"
        );

        // 最后一条作业是单声道，用的是第一份工作区，其 `Y` 仍在里面。
        //
        // **比较只能落在低带。** `Q_out = Q_in(延迟后) + Y`，而 f32 下
        // `(a + b) - b` 不恒等于 `a`；实测子带 11（即 `sbx`）处两者差 8e-7 的
        // 相对量。低带的 `Y` 恒为零，等式因此是精确的——`5.7.6.4.5` 只写
        // `[sbx, sbx + num_sb_aspx)`。高带另用「`Y` 非零」单独钉住。
        let y = rig.first.y;
        let sbx = usize::from(bands.sbx());
        let last = element
            .ajoc_input_order()
            .position(|signal| signal.aspx == Some((1, 0)))
            .expect("单声道元素应在 A-JOC 输入里");
        for ts in 0..SLOTS {
            for sb in 0..sbx {
                assert_eq!(y[ts].re[sb], 0.0, "低带的 `Y` 应恒为零");
                assert_eq!(
                    out[last][ts].re[sb], delayed[ts].re[sb],
                    "时隙 {ts} 子带 {sb} 的实部"
                );
            }
        }
        let nonzero_y = (0..SLOTS)
            .any(|ts| (sbx..SUBBANDS).any(|sb| y[ts].re[sb] != 0.0 || y[ts].im[sb] != 0.0));
        assert!(
            nonzero_y,
            "夹具前提：`Y` 必须非零，否则这一路根本没走 A-SPX，判据退化成两条旁路相等"
        );
    }

    /// SIMPLE 整个元素都不激活 A-SPX，因此**一格延迟都不该加**。
    ///
    /// `6.2.10`：「A-SPX shall be active for all codec modes except for the
    /// SIMPLE codec mode」。它与 LFE 走的是同一个 `match` 分支，只差元素的
    /// `var_codec_mode`；让它也走 `Y ≡ 0` 的合并会凭空加进 384 个样本。
    #[test]
    fn simple_mode_adds_no_delay_at_all() {
        let element = VarChannelElement::for_test(false, Some(false), 3, true, &[]);
        let signal = ramp(FRAME, 0.25);
        let pcm: Vec<&[f32]> = vec![&signal, &signal, &signal, &signal];

        let mut rig = Rig::new(4);
        let mut out = vec![empty_channel_frame(); 3];
        let mut lfe = empty_channel_frame();
        drive_element(
            &element,
            &[],
            None,
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            Some(&mut lfe),
        )
        .expect("驱动");

        let bare = analysed_only(&signal);
        assert_ne!(
            bare,
            delayed_only(&signal),
            "夹具前提：延迟与不延迟必须可区分，否则本判据什么也没测"
        );
        assert_eq!(&lfe[..SLOTS], bare.as_slice(), "SIMPLE 的 LFE 不得被延迟");
        for (index, frame) in out.iter().enumerate() {
            assert_eq!(&frame[..SLOTS], bare.as_slice(), "第 {index} 路不得被延迟");
        }
    }

    /// 输出按 A-JOC 的输入顺序摆放，不是传输顺序。
    ///
    /// 五路全频带时 `n_offset = 3`，期望顺序 2、3、4、0、1。判据给每一路不同的
    /// PCM，用低带首子带的实部序列当指纹——`Y` 只写 `sb >= sbx`，低带那一格恒
    /// 等于延迟后的输入，因此指纹与该路走哪条通路无关。
    #[test]
    fn the_outputs_land_in_ajoc_input_order() {
        let element = VarChannelElement::for_test(false, Some(false), 5, true, &[]);
        let signals: Vec<Vec<f32>> = (0..6).map(|k| ramp(FRAME, 0.1 + k as f32 * 0.3)).collect();
        let pcm: Vec<&[f32]> = signals.iter().map(Vec::as_slice).collect();

        let mut rig = Rig::new(6);
        let mut out = vec![empty_channel_frame(); 5];
        let mut lfe = empty_channel_frame();
        drive_element(
            &element,
            &[],
            None,
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            Some(&mut lfe),
        )
        .expect("驱动");

        let low_band =
            |frame: &[QmfSlot]| -> Vec<f32> { (0..SLOTS).map(|ts| frame[ts].re[0]).collect() };
        // 期望表照 `Pseudocode 14a` 手推，与 `var_element` 那条判据同源但独立。
        for (slot, source) in [2usize, 3, 4, 0, 1].into_iter().enumerate() {
            // 传输顺序第 0 路是 LFE，故全频带信号 `s` 落在 `pcm[s + 1]`。
            let want = low_band(&analysed_only(&signals[source + 1]));
            assert_eq!(
                low_band(&out[slot][..SLOTS]),
                want,
                "A-JOC 输入第 {slot} 项应取传输顺序的全频带信号 {source}"
            );
        }
    }

    /// 同一元素的全部声道必须共用一份帧布局；不能让短路的尾部被补零后伪装成
    /// 与长路等长。错误在任何分析状态、工作区或输出改写前返回。
    #[test]
    fn different_channel_frame_lengths_are_rejected_before_mutation() {
        let element = VarChannelElement::for_test(false, None, 2, false, &[]);
        let first = ramp(FRAME, 0.1);
        let second = ramp(1024, 0.2);
        let pcm: Vec<&[f32]> = vec![&first, &second];
        let mut rig = Rig::new(2);
        let mut out = vec![sentinel_frame(3.0), sentinel_frame(4.0)];
        let before = out.clone();

        let error = drive_element(
            &element,
            &[],
            None,
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            None,
        )
        .expect_err("逐路帧长不同必须拒绝");

        assert_eq!(
            error,
            DriveError::FrameLengthMismatch {
                channel: 1,
                expected: FRAME,
                provided: 1024,
            }
        );
        assert_eq!(out, before);
        assert_fresh_states(&rig);
        assert_eq!(rig.first.q_in, [QmfSlot::zero(); MAX_QMF_TIMESLOTS]);
    }

    /// SIMPLE 虽不进 A-SPX，也仍受表 189/192 的八档公共帧布局约束。
    #[test]
    fn simple_rejects_an_aligned_but_unsupported_frame_length() {
        let element = VarChannelElement::for_test(false, None, 1, false, &[]);
        let signal = ramp(64, 0.25);
        let pcm: Vec<&[f32]> = vec![&signal];
        let mut rig = Rig::new(1);
        let mut out = vec![sentinel_frame(5.0)];
        let before = out.clone();

        let error = drive_element(
            &element,
            &[],
            None,
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            None,
        )
        .expect_err("64 个样本虽对齐但不是合法 AC-4 帧长");

        assert_eq!(
            error,
            DriveError::Pipeline(PipelineError::UnsupportedFrameLength { samples: 64 })
        );
        assert_eq!(out, before);
        assert_fresh_states(&rig);
    }

    /// 平衡式作业过去在 `split_at_mut(high)` 之前不验容量，短工作区会 panic。
    /// 两类逐声道缓冲现在都在进入作业循环前给出各自的结构化错误。
    #[test]
    fn undersized_balanced_workspaces_return_errors_instead_of_panicking() {
        let bands = bands();
        let data = aspx_elements(&bands);
        let config = config();
        let element = VarChannelElement::for_test(true, None, 2, false, &[true]);
        let first_signal = ramp(FRAME, 0.1);
        let second_signal = ramp(FRAME, 0.2);
        let pcm: Vec<&[f32]> = vec![&first_signal, &second_signal];
        let mut out = vec![empty_channel_frame(); 2];

        let mut short_states = vec![ElementChannelState::new()];
        let mut mids = vec![AspxIntermediates::default(), AspxIntermediates::default()];
        let mut first_workspace = AspxWorkspace::new();
        let mut second_workspace = AspxWorkspace::new();
        let error = drive_element(
            &element,
            &data[..1],
            Some(&config),
            params(),
            &pcm,
            DriveWorkspace {
                aspx: [&mut first_workspace, &mut second_workspace],
                states: &mut short_states,
                intermediates: &mut mids,
            },
            &mut out,
            None,
        )
        .expect_err("短状态工作区必须返回错误");
        assert_eq!(
            error,
            DriveError::StateWorkspaceTooSmall {
                needed: 2,
                provided: 1,
            }
        );

        let mut states = vec![ElementChannelState::new(), ElementChannelState::new()];
        let mut short_mids = vec![AspxIntermediates::default()];
        let error = drive_element(
            &element,
            &data[..1],
            Some(&config),
            params(),
            &pcm,
            DriveWorkspace {
                aspx: [&mut first_workspace, &mut second_workspace],
                states: &mut states,
                intermediates: &mut short_mids,
            },
            &mut out,
            None,
        )
        .expect_err("短中间量工作区必须返回错误");
        assert_eq!(
            error,
            DriveError::IntermediateWorkspaceTooSmall {
                needed: 2,
                provided: 1,
            }
        );
    }

    /// 缺 A-SPX 数据时不能先把排在最前的 LFE 推进一帧，再在全频带声道处失败。
    #[test]
    fn missing_aspx_data_is_preflighted_before_the_lfe_advances() {
        let config = config();
        let element = VarChannelElement::for_test(true, None, 1, true, &[]);
        let lfe_signal = ramp(FRAME, 0.1);
        let fullband_signal = ramp(FRAME, 0.2);
        let pcm: Vec<&[f32]> = vec![&lfe_signal, &fullband_signal];
        let mut rig = Rig::new(2);
        let mut out = vec![sentinel_frame(6.0)];
        let mut lfe = sentinel_frame(7.0);
        let before_out = out.clone();
        let before_lfe = lfe;

        let error = drive_element(
            &element,
            &[],
            Some(&config),
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            Some(&mut lfe),
        )
        .expect_err("缺少 A-SPX 数据必须整元素预检失败");

        assert_eq!(error, DriveError::MissingAspxData { element: 0 });
        assert_eq!(out, before_out);
        assert_eq!(lfe, before_lfe);
        assert_fresh_states(&rig);
    }

    /// 已提供的前置元素合法时，预检仍须继续找到后面的首个缺失下标；不能先驱动
    /// LFE 或前两路全频带信号，再在第二个 A-SPX 元素处失败。
    #[test]
    fn later_missing_aspx_data_reports_its_index_before_any_channel_advances() {
        let bands = bands();
        let data = aspx_elements(&bands);
        let config = config();
        // 三路全频带信号需要一个双声道元素和一个单声道元素；只提供前者。
        let element = VarChannelElement::for_test(true, Some(false), 3, true, &[false]);
        assert_eq!(element.aspx_elements(), 2, "夹具必须需要两个 A-SPX 元素");
        let signal = ramp(FRAME, 0.25);
        let pcm: Vec<&[f32]> = vec![&signal, &signal, &signal, &signal];
        let mut rig = Rig::new(4);
        let mut out = vec![sentinel_frame(8.0); 3];
        let mut lfe = sentinel_frame(9.0);
        let before_out = out.clone();
        let before_lfe = lfe;

        let error = drive_element(
            &element,
            &data[..1],
            Some(&config),
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            Some(&mut lfe),
        )
        .expect_err("缺少后一个 A-SPX 元素必须整元素预检失败");

        assert_eq!(error, DriveError::MissingAspxData { element: 1 });
        assert_eq!(out, before_out);
        assert_eq!(lfe, before_lfe);
        assert_fresh_states(&rig);
    }

    /// SIMPLE 与 A-SPX 的跨帧历史衔接没有规范规则；两边都必须先显式重置，不能
    /// 让恢复 A-SPX 后的输出延迟线重放切换前的旧时隙。
    #[test]
    fn codec_mode_changes_require_an_explicit_random_access_reset() {
        let bands = bands();
        let data = aspx_elements(&bands);
        let mono_data = &data[1..2];
        let config = config();
        let aspx_element = VarChannelElement::for_test(true, None, 1, false, &[]);
        let simple_element = VarChannelElement::for_test(false, None, 1, false, &[]);
        let signal = ramp(FRAME, 0.3);
        let pcm: Vec<&[f32]> = vec![&signal];

        let mut rig = Rig::new(1);
        let mut aspx_control = Rig::new(1);
        let mut out = vec![empty_channel_frame()];
        let mut control_out = vec![empty_channel_frame()];
        for (state, target) in [(&mut rig, &mut out), (&mut aspx_control, &mut control_out)] {
            drive_element(
                &aspx_element,
                mono_data,
                Some(&config),
                params(),
                &pcm,
                state.workspace(),
                target,
                None,
            )
            .expect("A-SPX 首帧");
        }
        assert_eq!(rig.states, aspx_control.states);
        assert_eq!(rig.states[0].codec_mode_aspx(), Some(true));

        out[0] = sentinel_frame(8.0);
        let before = out.clone();
        let error = drive_element(
            &simple_element,
            &[],
            None,
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            None,
        )
        .expect_err("A-SPX 到 SIMPLE 不得隐式切换");
        assert_eq!(
            error,
            DriveError::CodecModeChangeRequiresReset {
                channel: 0,
                previous_aspx: true,
                current_aspx: false,
            }
        );
        assert_eq!(out, before);
        assert_eq!(rig.states, aspx_control.states, "失败不能推进任何历史");

        rig.states[0].reset();
        drive_element(
            &simple_element,
            &[],
            None,
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            None,
        )
        .expect("重置后可从 SIMPLE 起解");
        let mut simple_control = Rig::new(1);
        drive_element(
            &simple_element,
            &[],
            None,
            params(),
            &pcm,
            simple_control.workspace(),
            &mut control_out,
            None,
        )
        .expect("SIMPLE 参照");
        assert_eq!(rig.states, simple_control.states);
        assert_eq!(rig.states[0].codec_mode_aspx(), Some(false));

        out[0] = sentinel_frame(9.0);
        let before = out.clone();
        let error = drive_element(
            &aspx_element,
            mono_data,
            Some(&config),
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            None,
        )
        .expect_err("SIMPLE 到 A-SPX 不得隐式切换");
        assert_eq!(
            error,
            DriveError::CodecModeChangeRequiresReset {
                channel: 0,
                previous_aspx: false,
                current_aspx: true,
            }
        );
        assert_eq!(out, before);
        assert_eq!(rig.states, simple_control.states, "失败不能推进任何历史");

        rig.states[0].reset();
        drive_element(
            &aspx_element,
            mono_data,
            Some(&config),
            params(),
            &pcm,
            rig.workspace(),
            &mut out,
            None,
        )
        .expect("再次重置后可从 A-SPX 起解");
        assert_eq!(rig.states[0].codec_mode_aspx(), Some(true));
    }
}
