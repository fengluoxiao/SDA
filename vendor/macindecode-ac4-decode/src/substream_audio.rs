//! `ac4_substream()` 与 `audio_data_ajoc()` 的接合。
//!
//! [`macindecode_ac4_bitstream::audio_substream`] 按 `audio_size` 跳过音频数据直取 metadata；本模块
//! 把跳过的那一段真正解出来，并用 `audio_size` 声明的长度作为落点判据。
//!
//! # 参数从哪里来
//!
//! `6.2.2.2` 的调用是
//! `audio_data_ajoc(n_fullband_upmix_signals, b_static_dmx, n_fullband_dmx_signals,
//! b_lfe, b_audio_ndot)`，而 `6.2.3.4` 的形参表把最后一个写作 `b_iframe`——**该
//! 元素的 I 帧标志就是 `ac4_substream_info_ajoc()` 里的 `b_audio_ndot`**，不是
//! TOC 的 `b_iframe_global`。其余四个实参全部来自同一个 info 元素。
//!
//! `var_channel_element()` 还需要帧长与采样率。帧长由 TOC 的 `fs_index`、
//! `frame_rate_index` 与 presentation 的帧率参数推出；采样率还要叠加 info 元素的
//! `sf_multiplier`。
//!
//! `frame_rate_factor` 大于 1 的情形本模块拒绝，原因见
//! [`SubstreamAudioError::MultiSubstreamFrameRateUnsupported`]；
//! `frame_rate_fraction` 大于 1 时载荷跨多个传输帧，本模块也在调用方重组前拒绝。
//!
//! # 判据的方向性
//!
//! 音频区段的结构是 `audio_data` + `fill_bits`(VAR) + `byte_align`。本模块把
//! 读取器限制在该区段上，因此**多读**必然撞到边界报
//! [`macindecode_ac4_bitstream::reader::ReadError`]；但 `fill_bits` 长度不受约束，**少读**只会表现为
//! 一个偏大的 [`Ac4SubstreamAjoc::fill_bits`]。
//! 这个判据是单向的，与 `docs/SPEC_TRACEABILITY.md` §5.10 记的落点盲区同类：
//! 长度相等不等于语义正确，长度不足也未必被拒。调用方若掌握编码器的填充
//! 约定，应自行对 `fill_bits` 加上限。

use crate::ajoc::{AjocObjectControl, AjocObjectMatrix};
use crate::aspx::syntax::AspxData;
use crate::audio_data::{
    AudioDataAjoc, AudioDataError, AudioDataParams, AudioDataState, AudioDataWorkspace,
    parse_audio_data_ajoc,
};
use crate::channel::{ChannelContext, ChannelElement};
use crate::var_element::MAX_FULLBAND_DMX_SIGNALS;
use core::fmt;
use macindecode_ac4_bitstream::audio_substream::{
    Ac4AudioSubstream, AudioSubstreamError, SubstreamContext,
};
use macindecode_ac4_bitstream::oamd::{OamdMetadataBlock, ObjectDescriptors};
use macindecode_ac4_bitstream::reader::BitReader;
use macindecode_ac4_bitstream::substream::SubstreamInfoAjoc;
use macindecode_ac4_bitstream::toc::Ac4Toc;

/// 接合 `ac4_substream()` 与 `audio_data_ajoc()` 时的失败原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubstreamAudioError {
    /// `ac4_substream()` 的框架或 metadata 解析失败。
    Substream(AudioSubstreamError),
    /// `audio_data_ajoc()` 解析失败。
    AudioData(AudioDataError),
    /// 对象描述构造失败。
    Oamd(macindecode_ac4_bitstream::oamd::OamdError),
    /// TOC 与 `frame_rate_factor` 的组合没有定义帧长。
    FrameLengthUnavailable {
        /// `fs_index`。
        fs_index: u8,
        /// `frame_rate_index`。
        frame_rate_index: u8,
        /// presentation 声明的 `frame_rate_factor`。
        frame_rate_factor: u32,
    },
    /// `bitstream_version` 不为 2，`sus_ver` 无法确定。
    UnsupportedBitstreamVersion {
        /// TOC 中的版本号。
        bitstream_version: u32,
    },
    /// `n_fullband_dmx_signals` 超出本实现上限。
    DownmixSignalsOutOfRange {
        /// info 元素声明的下混信号数。
        declared: u32,
    },
    /// `b_static_dmx` 为真：下混走 `audio_data_chan()`，且 info 元素不携带
    /// `dmx_assignment`，无从构造 core 侧对象描述。
    StaticDownmixUnsupported,
    /// `frame_rate_factor` 不为 1，逐 substream 的 `b_iframe` 无从取得。
    ///
    /// 一个 `ac4_substream_info()` 此时指向 2 或 4 个连续解码的 substream，
    /// 每个都有自己的 `b_audio_ndot`（P1 `4.3.3.7.8`）。[`macindecode_ac4_bitstream::substream`]
    /// 只保留它们的合取——那是随机访问点判定要的量，不是本元素的 I 帧标志。
    /// 用合取值代替会在混合 ndot 的帧上整段错位，故拒绝而非猜测。
    MultiSubstreamFrameRateUnsupported {
        /// presentation 声明的 `frame_rate_factor`。
        frame_rate_factor: u32,
    },
    /// `frame_rate_fraction` 不为 1，当前载荷只是尚未重组的 codec frame 分片。
    FragmentedFrameRateUnsupported {
        /// presentation 声明的 `frame_rate_fraction`。
        frame_rate_fraction: u32,
    },
    /// substream 采样频率超出当前音频解析器支持范围。
    SamplingFrequencyUnsupported {
        /// 由 `fs_index` 与 `sf_multiplier` 推出的实际采样频率。
        sampling_frequency_hz: u32,
    },
}

impl fmt::Display for SubstreamAudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            SubstreamAudioError::Substream(error) => write!(f, "{error}"),
            SubstreamAudioError::AudioData(error) => write!(f, "{error}"),
            SubstreamAudioError::Oamd(error) => write!(f, "{error}"),
            SubstreamAudioError::FrameLengthUnavailable {
                fs_index,
                frame_rate_index,
                frame_rate_factor,
            } => write!(
                f,
                "The combination of fs_index {fs_index}, frame_rate_index {frame_rate_index}, and factor \
                 {frame_rate_factor} has no frame length in Tables 83/84/87"
            ),
            SubstreamAudioError::UnsupportedBitstreamVersion { bitstream_version } => write!(
                f,
                "bitstream_version is {bitstream_version}; sus_ver can only be determined for version 2"
            ),
            SubstreamAudioError::DownmixSignalsOutOfRange { declared } => write!(
                f,
                "n_fullband_dmx_signals is {declared}, exceeding implementation limit {MAX_FULLBAND_DMX_SIGNALS}"
            ),
            SubstreamAudioError::StaticDownmixUnsupported => {
                write!(
                    f,
                    "b_static_dmx is true, routing the downmix through unsupported audio_data_chan"
                )
            }
            SubstreamAudioError::MultiSubstreamFrameRateUnsupported { frame_rate_factor } => {
                write!(
                    f,
                    "frame_rate_factor is {frame_rate_factor}, but per-substream b_iframe was not retained"
                )
            }
            SubstreamAudioError::FragmentedFrameRateUnsupported {
                frame_rate_fraction,
            } => write!(
                f,
                "frame_rate_fraction is {frame_rate_fraction}; payloads spanning transport frames must be reassembled first"
            ),
            SubstreamAudioError::SamplingFrequencyUnsupported {
                sampling_frequency_hz,
            } => write!(
                f,
                "Substream sample rate is {sampling_frequency_hz} Hz; only 44100 Hz and 48000 Hz are supported"
            ),
        }
    }
}

impl core::error::Error for SubstreamAudioError {}

impl From<AudioSubstreamError> for SubstreamAudioError {
    fn from(error: AudioSubstreamError) -> Self {
        SubstreamAudioError::Substream(error)
    }
}

impl From<AudioDataError> for SubstreamAudioError {
    fn from(error: AudioDataError) -> Self {
        SubstreamAudioError::AudioData(error)
    }
}

impl From<macindecode_ac4_bitstream::oamd::OamdError> for SubstreamAudioError {
    fn from(error: macindecode_ac4_bitstream::oamd::OamdError) -> Self {
        SubstreamAudioError::Oamd(error)
    }
}

/// 由 TOC 与 `ac4_substream_info_ajoc()` 推导出的整套解析上下文。
#[derive(Debug, Clone, Copy)]
pub struct AjocSubstreamContext {
    /// `metadata()` 的上下文。
    pub metadata: SubstreamContext,
    /// `audio_data_ajoc()` 的参数。
    pub params: AudioDataParams,
    /// core 侧对象描述，由 `dmx_assignment` 构造。
    pub dmx_objects: ObjectDescriptors,
    /// full 侧对象描述，由 `upmix_assignment` 构造。
    pub umx_objects: ObjectDescriptors,
}

impl AjocSubstreamContext {
    /// 推导一个 A-JOC substream 的解析上下文。
    ///
    /// `frame_rate_factor` 与 `frame_rate_fraction` 取自引用该 group 的
    /// presentation；`b_alternative` 同。`group_num_obj_info_blocks` 是本帧对该
    /// substream group 生效的块数，调用方应先合并 group 级
    /// `oamd_substream()` 的时间数据与其跨帧状态。
    ///
    /// # Errors
    ///
    /// 见 [`SubstreamAudioError`]。`b_static_dmx` 为真时在此处即拒绝，因为 info
    /// 元素在该分支下不传输 `dmx_assignment`。
    pub fn derive(
        toc: &Ac4Toc,
        info: &SubstreamInfoAjoc,
        frame_rate_factor: u32,
        frame_rate_fraction: u32,
        b_alternative: bool,
        group_num_obj_info_blocks: Option<u8>,
    ) -> Result<Self, SubstreamAudioError> {
        if toc.bitstream_version != 2 {
            return Err(SubstreamAudioError::UnsupportedBitstreamVersion {
                bitstream_version: toc.bitstream_version,
            });
        }
        // 因子大于 1 时本元素描述多个 substream，各有自己的 b_audio_ndot；
        // 保留下来的只是它们的合取，不能当作单个元素的 b_iframe。
        if frame_rate_factor != 1 {
            return Err(SubstreamAudioError::MultiSubstreamFrameRateUnsupported {
                frame_rate_factor,
            });
        }
        // 高帧率分数模式把一个 codec frame 分散在 2 或 4 个传输帧中；单个
        // raw_ac4_frame 的 substream 只是分片，不能直接交给音频语法解析器。
        if frame_rate_fraction != 1 {
            return Err(SubstreamAudioError::FragmentedFrameRateUnsupported {
                frame_rate_fraction,
            });
        }
        // b_static_dmx 下 dmx_assignment 不传输，core 侧的对象描述无从构造；
        // 这比在 audio_data_ajoc 内部拒绝更早，也更贴近缺失的原因。
        let Some(dmx_assignment) = info.dmx_assignment else {
            return Err(SubstreamAudioError::StaticDownmixUnsupported);
        };

        let frame_len_base = toc.codec_frame_len_base(frame_rate_factor).ok_or(
            SubstreamAudioError::FrameLengthUnavailable {
                fs_index: toc.fs_index,
                frame_rate_index: toc.frame_rate_index,
                frame_rate_factor,
            },
        )?;
        let base_sampling_frequency_hz = toc.base_sampling_frequency_hz().ok_or(
            SubstreamAudioError::FrameLengthUnavailable {
                fs_index: toc.fs_index,
                frame_rate_index: toc.frame_rate_index,
                frame_rate_factor,
            },
        )?;
        let sampling_frequency_hz = base_sampling_frequency_hz
            .checked_mul(info.sampling_frequency_multiplier())
            .ok_or(SubstreamAudioError::SamplingFrequencyUnsupported {
                sampling_frequency_hz: u32::MAX,
            })?;
        if !matches!(sampling_frequency_hz, 44_100 | 48_000) {
            return Err(SubstreamAudioError::SamplingFrequencyUnsupported {
                sampling_frequency_hz,
            });
        }

        let n_fb_dmx_signals = u8::try_from(info.n_dmx_signals)
            .ok()
            .filter(|&count| count <= MAX_FULLBAND_DMX_SIGNALS)
            .ok_or(SubstreamAudioError::DownmixSignalsOutOfRange {
                declared: info.n_dmx_signals,
            })?;

        Ok(Self {
            metadata: SubstreamContext {
                // 6.3.2.5.4：bitstream_version 为 2 时 sus_ver 恒为 1。
                sus_ver: 1,
                alternative: b_alternative,
                ajoc: true,
                // 6.2.2.2 NOTE 2：A-JOC 的 info 元素不设 channel_mode。
                channel_mode: None,
                // 6.2.2.2 同样把 b_audio_ndot 传给 metadata() 的 b_iframe。
                b_iframe: Some(info.audio_ndot()),
                // A-JOC 的 alternative datasets 位于 audio_data_ajoc()，不在 metadata()。
                alternative_oamd: None,
            },
            params: AudioDataParams {
                context: ChannelContext {
                    frame_len_base,
                    sampling_frequency_hz,
                },
                n_fb_dmx_signals,
                n_fb_upmix_signals: info.n_upmix_signals,
                b_lfe: info.b_lfe,
                // 6.2.2.2 把 b_audio_ndot 传给 6.2.3.4 的 b_iframe 形参。
                b_iframe: info.audio_ndot(),
                b_static_dmx: info.static_dmx,
                b_alternative,
                group_num_obj_info_blocks,
            },
            dmx_objects: ObjectDescriptors::from_ajoc_assignment(dmx_assignment, info.b_lfe)?,
            umx_objects: ObjectDescriptors::from_ajoc_assignment(
                info.upmix_assignment,
                info.b_lfe,
            )?,
        })
    }
}

/// 解析音频数据所需的工作区。
///
/// 对象描述不在此处：它们由 [`AjocSubstreamContext`] 从 info 元素推出。
#[derive(Debug)]
pub struct AjocAudioWorkspace<'a> {
    /// 声道数据元素，至少 [`crate::var_element::MAX_CHANNEL_ELEMENTS`] 个。
    pub elements: &'a mut [ChannelElement],
    /// A-SPX 数据元素，A-SPX 模式下至少 [`crate::var_element::MAX_ASPX_ELEMENTS`] 个。
    pub aspx: &'a mut [AspxData],
    /// A-JOC 逐对象控制信息，至少 `n_fullband_upmix_signals` 个。
    pub controls: &'a mut [AjocObjectControl],
    /// A-JOC 逐对象混合矩阵，长度同上。
    pub matrices: &'a mut [AjocObjectMatrix],
    /// core 侧动态数据的信息块。
    pub dmx_blocks: &'a mut [OamdMetadataBlock],
    /// full 侧动态数据的信息块。
    pub umx_blocks: &'a mut [OamdMetadataBlock],
}

/// 一个 A-JOC `ac4_substream()` 的完整解析结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ac4SubstreamAjoc {
    /// 框架与 metadata。
    pub substream: Ac4AudioSubstream,
    /// `audio_data_ajoc()` 的结果。
    pub audio: AudioDataAjoc,
    /// `audio_data_ajoc()` 消耗的比特数。
    pub audio_data_bits: u64,
    /// 其后的 `fill_bits` 与 `byte_align` 合计位数。
    ///
    /// 规范对该长度不设上限，故它偏大只说明本实现少读了，不构成错误。
    pub fill_bits: u64,
}

/// 解析一个 A-JOC 编码的 `ac4_substream()`，音频数据一并解出。
///
/// `payload` 必须恰好是该 substream 的字节，可由
/// [`macindecode_ac4_bitstream::topology::Ac4Topology::substream_payload`] 取得。
///
/// # Errors
///
/// 见 [`SubstreamAudioError`]。音频数据的读取被限制在 `audio_size` 声明的区段
/// 内，越界即 [`AudioDataError::Read`]。
pub fn parse_substream_ajoc(
    payload: &[u8],
    context: &AjocSubstreamContext,
    state: &mut AudioDataState,
    workspace: AjocAudioWorkspace<'_>,
) -> Result<Ac4SubstreamAjoc, SubstreamAudioError> {
    let substream = Ac4AudioSubstream::parse(payload, context.metadata)?;
    // parse 已核对过 audio_size 不超出载荷，此处的切片必然成立；仍按同一语义
    // 报错，不引入只在不可达路径上出现的新错误码。
    let audio_region =
        substream
            .audio_payload(payload)
            .ok_or(AudioSubstreamError::AudioSizeOutOfRange {
                audio_size: u64::from(substream.audio_size),
                substream_len: payload.len() as u64,
            })?;

    let AjocAudioWorkspace {
        elements,
        aspx,
        controls,
        matrices,
        dmx_blocks,
        umx_blocks,
    } = workspace;

    // 读取器只覆盖音频区段：任何多读都会撞到区段边界，而不会啃进 metadata。
    let mut reader = BitReader::new(audio_region);
    let audio = parse_audio_data_ajoc(
        &mut reader,
        context.params,
        state,
        AudioDataWorkspace {
            elements,
            aspx,
            controls,
            matrices,
            dmx_objects: context.dmx_objects.as_slice(),
            umx_objects: context.umx_objects.as_slice(),
            dmx_blocks,
            umx_blocks,
        },
    )?;

    Ok(Ac4SubstreamAjoc {
        substream,
        audio,
        audio_data_bits: reader.bit_position(),
        fill_bits: reader.remaining_bits(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ajoc::AjocObjectMatrix;
    use crate::testutil::BitBuf;
    use macindecode_ac4_bitstream::oamd::{MAX_OAMD_OBJECTS, OamdError, ObjectType};
    use macindecode_ac4_bitstream::substream::{Ac4SubstreamGroupInfo, SubstreamInfo};

    /// 24 fps、48 kHz、单 presentation 的 TOC。
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

    /// `bed_dyn_obj_assignment()` 的两种最短形式。
    #[derive(Clone, Copy)]
    enum Assignment {
        /// `b_dyn_objects_only = 1`，全部信号都是动态对象。
        DynamicOnly,
        /// `b_ch_assign_code = 1` 加 3 位码；床对象数取自表内数组。
        BedCode(u32),
    }

    impl Assignment {
        fn push(self, buf: &mut BitBuf) {
            match self {
                Assignment::DynamicOnly => buf.push(true),
                Assignment::BedCode(code) => {
                    buf.push(false); // b_dyn_objects_only
                    buf.push(false); // b_isf
                    buf.push(true); // b_ch_assign_code
                    buf.push_bits(code, 3);
                }
            }
        }
    }

    /// 构造并解析一个只含单个 A-JOC substream 的 group，返回其 info 元素。
    ///
    /// 走真实的 `ac4_substream_group_info()` 解析而非直接填结构体：
    /// `b_audio_ndot` 在 `SubstreamInfoAjoc` 里是私有尾部字段，只有解析才能
    /// 设定它，而它正是本模块最容易接错的那个实参。
    fn ajoc_info(
        n_dmx: u32,
        dmx: Assignment,
        n_umx: u32,
        umx: Assignment,
        b_lfe: bool,
        ndot: bool,
        frame_rate_factor: u32,
    ) -> SubstreamInfoAjoc {
        ajoc_info_with_sf_multiplier(
            n_dmx,
            dmx,
            n_umx,
            umx,
            b_lfe,
            ndot,
            (frame_rate_factor, None),
        )
    }

    fn ajoc_info_with_sf_multiplier(
        n_dmx: u32,
        dmx: Assignment,
        n_umx: u32,
        umx: Assignment,
        b_lfe: bool,
        ndot: bool,
        frame_rate_and_sf_multiplier: (u32, Option<u8>),
    ) -> SubstreamInfoAjoc {
        let (frame_rate_factor, sf_multiplier) = frame_rate_and_sf_multiplier;
        let mut buf = BitBuf::new();
        buf.push(false); // b_substreams_present
        buf.push(false); // b_hsf_ext
        buf.push(true); // 单个低频 substream
        buf.push(false); // b_channel_coded
        buf.push(false); // b_oamd_substream
        buf.push(true); // b_ajoc

        buf.push(b_lfe);
        buf.push(false); // b_static_dmx
        buf.push_bits(n_dmx.saturating_sub(1), 4);
        dmx.push(&mut buf);
        buf.push(false); // b_oamd_common_data_present
        if n_umx <= 15 {
            buf.push_bits(n_umx.saturating_sub(1), 4);
        } else {
            buf.push_bits(15, 4);
            // variable_bits(3) 两段：值为 8*c0 + 8 + c1。
            let extra = n_umx.saturating_sub(16);
            let c0 = extra.saturating_sub(8) / 8;
            buf.push_bits(c0, 3);
            buf.push(true);
            buf.push_bits(
                extra.saturating_sub(8).saturating_sub(c0.saturating_mul(8)),
                3,
            );
            buf.push(false);
        }
        umx.push(&mut buf);

        buf.push(sf_multiplier.is_some()); // b_sf_multiplier（fs_index == 1）
        if let Some(multiplier) = sf_multiplier {
            buf.push(multiplier != 0);
        }
        buf.push(false); // b_bitrate_info
        for _ in 0..frame_rate_factor {
            buf.push(ndot); // b_audio_ndot
        }
        buf.push(false); // b_content_type

        let group = Ac4SubstreamGroupInfo::parse(
            &mut BitReader::new(buf.as_slice()),
            2,
            1,
            frame_rate_factor,
        )
        .expect("group 应能解析");
        match group.substreams().first() {
            Some(&SubstreamInfo::Ajoc(info)) => info,
            other => panic!("应得到 A-JOC substream，实际为 {other:?}"),
        }
    }

    /// 一个默认形状：两个床下混信号、三个动态上混对象、带 LFE。
    fn default_info(ndot: bool) -> SubstreamInfoAjoc {
        ajoc_info(
            2,
            Assignment::BedCode(0), // 表内 code 0 对应 2 个床对象
            3,
            Assignment::DynamicOnly,
            true,
            ndot,
            1,
        )
    }

    /// info 元素的五个实参逐一落到 `audio_data_ajoc()` 的形参上。
    ///
    /// 最容易错的是最后一个：`6.2.2.2` 传的是 `b_audio_ndot`，而 `6.2.3.4` 的
    /// 形参名叫 `b_iframe`。取 TOC 的 `b_iframe_global` 会在两者不一致的帧上
    /// 整段错位。
    #[test]
    fn derives_parameters_from_the_info_element() {
        for ndot in [false, true] {
            let info = default_info(ndot);
            assert_eq!(info.audio_ndot(), ndot);

            let context = AjocSubstreamContext::derive(&toc(), &info, 1, 1, false, Some(1))
                .expect("应能推导");
            assert_eq!(context.params.n_fb_dmx_signals, 2);
            assert_eq!(context.params.n_fb_upmix_signals, 3);
            assert!(context.params.b_lfe);
            assert_eq!(
                context.params.b_iframe, ndot,
                "b_iframe 取自 info 元素的 b_audio_ndot"
            );
            assert!(!context.params.b_static_dmx);
            assert_eq!(context.params.context.frame_len_base, 1920);
            assert_eq!(context.params.context.sampling_frequency_hz, 48_000);
            assert_eq!(context.metadata.sus_ver, 1);
            assert!(context.metadata.ajoc);
            assert_eq!(context.metadata.channel_mode, None, "6.2.2.2 NOTE 2");
        }
    }

    /// core 与 full 各自从自己的分配构造描述符，LFE 固定在索引 0。
    #[test]
    fn builds_both_sides_from_their_own_assignment() {
        let context =
            AjocSubstreamContext::derive(&toc(), &default_info(true), 1, 1, false, Some(1))
                .expect("应能推导");

        let dmx = context.dmx_objects.as_slice();
        assert_eq!(dmx.len(), 3, "两个下混信号加 LFE");
        assert!(dmx.first().is_some_and(|item| item.b_lfe));
        assert!(
            dmx.iter()
                .skip(1)
                .all(|item| item.obj_type == ObjectType::Bed && !item.b_lfe),
            "dmx_assignment 声明的是床对象"
        );

        let umx = context.umx_objects.as_slice();
        assert_eq!(umx.len(), 4, "三个上混对象加 LFE");
        assert!(umx.first().is_some_and(|item| item.b_lfe));
        assert!(
            umx.iter()
                .skip(1)
                .all(|item| item.obj_type == ObjectType::Dynamic),
            "upmix_assignment 声明的是动态对象"
        );
    }

    /// 因子大于 1 时一个 info 元素描述多个 substream，本层拒绝而不猜测。
    ///
    /// 拒绝点必须在读取帧长之前：`frame_len_base` 除得开并不代表逐 substream
    /// 的 `b_iframe` 可得，两者是不同的问题。
    #[test]
    fn rejects_multi_substream_frame_rates() {
        let mut base = toc();
        base.frame_rate_index = 2; // 25 fps，因子可取 1、2、4
        for factor in [2u32, 4] {
            let info = ajoc_info(
                1,
                Assignment::DynamicOnly,
                1,
                Assignment::DynamicOnly,
                false,
                true,
                factor,
            );
            assert_eq!(
                AjocSubstreamContext::derive(&base, &info, factor, 1, false, None).unwrap_err(),
                SubstreamAudioError::MultiSubstreamFrameRateUnsupported {
                    frame_rate_factor: factor
                },
                "因子 {factor} 应被拒绝"
            );
        }

        // 因子 1 时该索引的帧长仍取表 83 的原值。
        let info = ajoc_info(
            1,
            Assignment::DynamicOnly,
            1,
            Assignment::DynamicOnly,
            false,
            true,
            1,
        );
        let context =
            AjocSubstreamContext::derive(&base, &info, 1, 1, false, None).expect("因子 1 应能推导");
        assert_eq!(context.params.context.frame_len_base, 2048);
    }

    /// 高帧率分数模式的载荷要先跨传输帧重组，不能按当前 raw frame 直接解析。
    #[test]
    fn rejects_fragmented_frame_rates_before_parsing_audio() {
        let mut base = toc();
        base.frame_rate_index = 6; // 48 fps，可用 fraction 2 还原 24 fps codec frame
        assert_eq!(
            AjocSubstreamContext::derive(&base, &default_info(true), 1, 2, false, None)
                .unwrap_err(),
            SubstreamAudioError::FragmentedFrameRateUnsupported {
                frame_rate_fraction: 2
            }
        );
    }

    /// substream 的采样率由 TOC 基准值与 info 元素的乘子共同决定。
    #[test]
    fn rejects_high_substream_sampling_frequencies_explicitly() {
        for (encoded, multiplier, sampling_frequency_hz) in
            [(0u8, 2u32, 96_000u32), (1, 4, 192_000)]
        {
            let info = ajoc_info_with_sf_multiplier(
                1,
                Assignment::DynamicOnly,
                1,
                Assignment::DynamicOnly,
                false,
                true,
                (1, Some(encoded)),
            );
            assert_eq!(info.sampling_frequency_multiplier(), multiplier);
            assert_eq!(
                AjocSubstreamContext::derive(&toc(), &info, 1, 1, false, None).unwrap_err(),
                SubstreamAudioError::SamplingFrequencyUnsupported {
                    sampling_frequency_hz
                }
            );
        }
    }

    /// `b_static_dmx` 下 info 元素不带 `dmx_assignment`，在推导阶段即拒绝。
    #[test]
    fn rejects_static_downmix_at_derivation() {
        let mut info = default_info(true);
        info.static_dmx = true;
        info.dmx_assignment = None;
        assert_eq!(
            AjocSubstreamContext::derive(&toc(), &info, 1, 1, false, None).unwrap_err(),
            SubstreamAudioError::StaticDownmixUnsupported
        );
    }

    /// 上混对象数超过 OAMD 的固定容量时报错，而不是截断。
    #[test]
    fn rejects_object_count_beyond_capacity() {
        let info = ajoc_info(
            2,
            Assignment::BedCode(0),
            64,
            Assignment::DynamicOnly,
            false,
            true,
            1,
        );
        assert_eq!(info.n_upmix_signals, 64, "变长扩展应还原出 64");
        assert_eq!(
            AjocSubstreamContext::derive(&toc(), &info, 1, 1, false, None).unwrap_err(),
            SubstreamAudioError::Oamd(OamdError::TooManyObjects {
                limit: MAX_OAMD_OBJECTS
            })
        );
    }

    /// 一个 `audio_data_ajoc()`：单个全频带信号加 LFE，A-SPX 模式。
    ///
    /// 与 `audio_data` 模块的落点用例同构，只是这里要放进 `ac4_substream()`。
    fn push_audio_data(buf: &mut BitBuf, umx_blocks: u32) {
        buf.push(false); // b_some_signals_inactive
        buf.push(true); // var_codec_mode = A-SPX
        buf.push_aspx_config();
        buf.push(true); // companding_control(1)
        buf.push_bits(2, 3); // LFE 的 max_sfb
        buf.push_empty_sf_data(2);
        buf.push_mono_data(2);
        buf.push_aspx_data_1ch();
        buf.push(true); // b_dmx_timing
        buf.push_timing(1);
        for _ in 0..2 {
            buf.push_inactive_object_block(); // 一个信号加 LFE
        }
        buf.push(false); // b_oamd_extension_present
        buf.push_minimal_ajoc(1);
        buf.push_minimal_dmx_de(1);
        buf.push(false); // b_umx_timing
        buf.push(true); // b_derive_timing_from_dmx
        for _ in 0..umx_blocks.saturating_mul(2) {
            buf.push_inactive_object_block();
        }
    }

    /// 按字节写出 `metadata()`，全部取最短分支，末尾补齐 `byte_align`。
    fn push_metadata(buf: &mut BitBuf) {
        buf.push(false); // b_more_basic_metadata
        buf.push(false); // b_dialog
        buf.push(false); // b_channels_classifier
        buf.push(false); // b_event_probability
        buf.push_bits(1, 7); // tools_metadata_size_value
        buf.push(false); // b_more_bits
        buf.push(false); // b_de_data_present
        buf.push(false); // b_emdf_payloads_substream
        buf.byte_align();
    }

    /// 组装一个 `ac4_substream()` 载荷。
    ///
    /// `declared_delta` 加到 `audio_size` 上，`written_fill` 是实际补写的填充
    /// 字节数。两者相等时区段恰好装下音频数据与填充。
    fn substream_payload(declared_delta: i32, written_fill: usize) -> BitBuf {
        let mut audio = BitBuf::new();
        push_audio_data(&mut audio, 1);
        audio.byte_align();
        let body = audio.as_slice();

        let declared = i64::try_from(body.len())
            .unwrap_or(0)
            .saturating_add(i64::from(declared_delta));
        let declared = u32::try_from(declared).unwrap_or(0);
        // 声明变短时只写出区段容得下的那部分，metadata 仍紧跟其后。
        let written = usize::try_from(declared).unwrap_or(0).min(body.len());

        let mut buf = BitBuf::new();
        buf.push_bits(declared, 15);
        buf.push(false); // b_more_bits
        buf.push_bytes(body.get(..written).unwrap_or(body));
        for _ in 0..written_fill {
            buf.push_bits(0, 8);
        }
        push_metadata(&mut buf);
        buf
    }

    struct Workspace {
        elements: [ChannelElement; 2],
        aspx: [AspxData; 1],
        controls: [AjocObjectControl; 1],
        matrices: [AjocObjectMatrix; 1],
        dmx_blocks: [OamdMetadataBlock; 4],
        umx_blocks: [OamdMetadataBlock; 4],
    }

    impl Workspace {
        fn new() -> Self {
            Self {
                elements: [ChannelElement::new(), ChannelElement::new()],
                aspx: [AspxData::empty()],
                controls: [AjocObjectControl::default()],
                matrices: [AjocObjectMatrix::new()],
                dmx_blocks: [OamdMetadataBlock::default(); 4],
                umx_blocks: [OamdMetadataBlock::default(); 4],
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

    fn single_signal_context() -> AjocSubstreamContext {
        let info = ajoc_info(
            1,
            Assignment::DynamicOnly,
            1,
            Assignment::DynamicOnly,
            true,
            true,
            1,
        );
        AjocSubstreamContext::derive(&toc(), &info, 1, 1, false, Some(1)).expect("应能推导")
    }

    /// 音频数据恰好填满 `audio_size`，metadata 恰好落在 substream 末尾。
    ///
    /// 这是本层的判据：`audio_size` 与载荷总长是两条彼此独立的长度声明，音频
    /// 数据、填充与 metadata 必须同时对上它们。
    #[test]
    fn audio_data_fills_the_declared_size_exactly() {
        let buf = substream_payload(0, 0);
        let context = single_signal_context();
        let mut workspace = Workspace::new();
        let mut state = AudioDataState::new();

        let parsed = parse_substream_ajoc(buf.as_slice(), &context, &mut state, workspace.borrow())
            .expect("应能解析");

        assert_eq!(
            parsed.audio_data_bits.saturating_add(parsed.fill_bits),
            u64::from(parsed.substream.audio_size).saturating_mul(8),
            "音频数据加填充应恰好等于 audio_size"
        );
        assert!(parsed.fill_bits < 8, "只有 byte_align 的余数");
        assert_eq!(parsed.audio.dmx_num_obj_info_blocks, 1);
        assert_eq!(parsed.audio.umx_num_obj_info_blocks, 1);
        assert_eq!(parsed.audio.dmx_blocks_written(), 2);
        assert_eq!(parsed.audio.umx_blocks_written(), 2);
        assert_eq!(parsed.audio.derive_timing_from_dmx, Some(true));
    }

    /// `fill_bits` 长度不受约束，多补的字节只体现在 `fill_bits` 上。
    ///
    /// 这正是本层判据的单向性：少读不会被拒。
    #[test]
    fn extra_fill_bytes_are_reported_not_rejected() {
        let buf = substream_payload(3, 3);
        let context = single_signal_context();
        let mut workspace = Workspace::new();
        let mut state = AudioDataState::new();

        let parsed = parse_substream_ajoc(buf.as_slice(), &context, &mut state, workspace.borrow())
            .expect("应能解析");

        assert!(
            parsed.fill_bits >= 24,
            "三个填充字节应计入 fill_bits，实际 {}",
            parsed.fill_bits
        );
        assert_eq!(
            parsed.audio_data_bits.saturating_add(parsed.fill_bits),
            u64::from(parsed.substream.audio_size).saturating_mul(8)
        );
    }

    /// 取出音频数据错误里最内层的读取失败。
    ///
    /// 越界发生在哪一段取决于缺掉的那个字节落在哪里，因此不能把断言钉死在
    /// 某一个包装层上。只解一层：更深的嵌套返回 `None`，由断言的错误信息
    /// 报出实际形状。
    fn innermost_read(
        error: SubstreamAudioError,
    ) -> Option<macindecode_ac4_bitstream::reader::ReadError> {
        let SubstreamAudioError::AudioData(audio) = error else {
            return None;
        };
        match audio {
            AudioDataError::Read(error) => Some(error),
            AudioDataError::VarElement(crate::var_element::VarElementError::Read(error)) => {
                Some(error)
            }
            AudioDataError::Ajoc(crate::ajoc::AjocError::Read(error)) => Some(error),
            AudioDataError::AjocDe(crate::ajoc::de::AjocDeError::Read(error)) => Some(error),
            AudioDataError::Oamd(OamdError::Read(error)) => Some(error),
            _ => None,
        }
    }

    /// `audio_size` 少一个字节时，越界必须发生在**区段末尾**而非载荷末尾。
    ///
    /// 只断言「解析失败」抓不到读取器范围写错：读取器若覆盖整个载荷，缺掉的
    /// 那几位会从 metadata 的开头补上，随后仍会在载荷末尾耗尽而报同一类错误。
    /// 两者的区别只在越界位置，故判据取 `bit_position + remaining_bits`——它
    /// 恰是读取器可见的总长度。
    #[test]
    fn short_audio_size_stops_at_the_region_boundary() {
        let buf = substream_payload(-1, 0);
        let context = single_signal_context();
        let mut workspace = Workspace::new();
        let mut state = AudioDataState::new();

        let framing = Ac4AudioSubstream::parse(buf.as_slice(), context.metadata)
            .expect("框架与 metadata 仍应可解");
        let error = parse_substream_ajoc(buf.as_slice(), &context, &mut state, workspace.borrow())
            .expect_err("应在音频区段内越界");

        let Some(macindecode_ac4_bitstream::reader::ReadError::OutOfBounds {
            bit_position,
            remaining_bits,
            ..
        }) = innermost_read(error)
        else {
            panic!("应是音频数据越界，实际为 {error:?}");
        };
        assert_eq!(
            bit_position.saturating_add(remaining_bits),
            u64::from(framing.audio_size).saturating_mul(8),
            "读取器可见范围应恰好是 audio_size，而不是整个 substream 载荷"
        );
        assert_eq!(state, AudioDataState::new(), "失败的元素不得留下跨帧状态");
    }

    /// 连续两帧共用一份跨帧状态：次帧不再传 `aspx_config`。
    ///
    /// 这条链只有接到 `ac4_substream()` 之后才走得通——`aspx_config` 是元素级
    /// 的一份，次帧的 A-SPX 数据要靠上一帧留下的配置才能解。
    #[test]
    fn state_carries_across_two_substreams() {
        let context = single_signal_context();
        let mut workspace = Workspace::new();
        let mut state = AudioDataState::new();

        let first = substream_payload(0, 0);
        parse_substream_ajoc(first.as_slice(), &context, &mut state, workspace.borrow())
            .expect("首帧应能解析");
        let after_first = state;

        let second = substream_payload(0, 0);
        let parsed =
            parse_substream_ajoc(second.as_slice(), &context, &mut state, workspace.borrow())
                .expect("次帧应能解析");
        assert_eq!(parsed.audio.dmx_num_obj_info_blocks, 1);
        assert_eq!(state, after_first, "两帧内容相同，状态应稳定");
    }
}
