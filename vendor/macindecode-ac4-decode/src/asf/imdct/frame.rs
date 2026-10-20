//! 帧级合成：把一帧内各块的 IMDCT 输出按窗口顺序拼成 `frame_length` 个样本。
//!
//! `5.5.3` 在块切换示例之后规定：一帧内的各块依次处理进一个 composition
//! buffer，该缓冲至多需要 4 096 个样本（96 kHz 为 8 192，192 kHz 为 16 384）；
//! 处理完当前帧的全部块后，最早的 `frame_length` 个样本交给下一个工具。
//!
//! **本实现不设独立的 composition buffer**，各块直接写入调用方的输出缓冲。
//! 依据不是「块长之和等于 `frame_length`」——那只说明数量对得上，说不了对齐
//! ——而是同一小节的示例所描述的移位寄存器语义：每块输出恒取重叠缓冲的
//! `[0, N)`，随后整体左移 `N`，新块的未加窗后半写到 `[(N_full−N)/2,
//! (N_full+N)/2)`。由此得两条：
//!
//! - **输出时间轴匀速。** 读指针每块前进 `N`，而块中心每块前进
//!   `(N_prev+N)/2`；两者由 `c_i = t_read_i + (N_full+N_i)/2` 联系，差分即
//!   `(N_{i−1}+N_i)/2`，对任意块序列恒成立。所以延迟是常数，与块长无关。
//! - **`N_full` 的缓冲够用。** 右窗先从 `(N_full+min(N,N_prev))/2` 起归零，
//!   当前块的左半随后最多写到 `(N_full+N)/2`，故其右侧恒为零。若下一块更长，
//!   只有新增的右侧区间落在这段零区；左侧区间按设计与仍存活的历史重叠。
//!
//! 两条都由 `transform` 的 `mixed_block_lengths_reconstruct_with_a_constant_delay`
//! 实测：22 个块、5 种切换，重建偏差 1.8e-7，且逐块核对无残留。规范给的
//! 4 096 恰是 `2 × N_full`（96 kHz 的 8 192 与 192 kHz 的 16 384 同样是两倍），
//! 与本实现「`N_full` 重叠缓冲 + `frame_length` 输出缓冲」的总量相同。
//!
//! 一个可观察的后果：帧内靠后的短块，其能量要到**下一帧**才出现在输出里
//! ——写入偏移 `(N_full−N)/2` 最远可达 960（`N = 128`、`N_full = 2 048`），而
//! 读指针每块只前进 `N`。整帧输入非零却输出全零因此是合法情形，不能当判据。
//!
//! 块序列读自 [`AsfWindowLayout`]：窗口 `w` 的变换长度是它所属组的
//! `transform_length`。它与帧长由不同字段决定——前者出自 `asf_transform_info()`
//! 与 `asf_psy_info()`，后者出自表 83——因此两者相等本身就是一条跨层判据。
//!
//! 每个声道各持一份 [`ChannelSynthesis`]；[`ImdctWorkspace`] 只在一次变换内
//! 有效，可在声道间复用。

use crate::asf::framing::AsfWindowLayout;
use crate::asf::imdct::transform::{self, ImdctError, ImdctWorkspace, OverlapBuffer};

/// 一个声道的跨帧合成状态。
#[derive(Debug)]
pub struct ChannelSynthesis {
    overlap: OverlapBuffer,
}

impl ChannelSynthesis {
    /// 以全块长度 `N_full` 建立声道状态。
    #[must_use]
    pub fn new(frame_length: u16) -> Option<Self> {
        Some(Self {
            overlap: OverlapBuffer::new(frame_length)?,
        })
    }

    /// 本声道的全块长度。
    #[must_use]
    pub const fn frame_length(&self) -> u16 {
        self.overlap.frame_length()
    }

    /// 上一块的长度 `N_prev`。
    #[must_use]
    pub const fn previous_block_length(&self) -> u16 {
        self.overlap.previous_length()
    }
}

/// 帧级合成无法完成的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSynthesisError {
    /// 某一块的 IMDCT 失败。
    Block { window: usize, error: ImdctError },
    /// 窗口布局给出的组下标越界。
    InvalidWindow { window: usize },
    /// 解组后的谱线不足以喂满全部窗口。
    NotEnoughLines { needed: usize, provided: usize },
    /// 各块长度之和与 `frame_length` 不符。
    BlockLengthsDoNotSpanFrame { total: usize, frame: usize },
    /// 输出缓冲长度与 `frame_length` 不符。
    OutputLengthMismatch { expected: usize, provided: usize },
}

/// 合成一帧：逐窗口 IMDCT，按顺序写满 `frame_length` 个样本。
///
/// `ungrouped` 是 `5.1.5` 解组后的谱线，按「窗口 → 频率升序」排列，与本模块
/// 逐窗口切片的顺序一致。
pub fn synthesize(
    state: &mut ChannelSynthesis,
    ungrouped: &[f32],
    layout: &AsfWindowLayout,
    workspace: &mut ImdctWorkspace,
    pcm: &mut [f32],
) -> Result<(), FrameSynthesisError> {
    let frame = usize::from(state.overlap.frame_length());
    if pcm.len() != frame {
        return Err(FrameSynthesisError::OutputLengthMismatch {
            expected: frame,
            provided: pcm.len(),
        });
    }

    // 先核对总量再变换：任何一块落地后重叠缓冲就已推进，中途失败会留下半帧
    // 状态，而调用方无从回滚。
    let windows = usize::from(layout.num_windows());
    let mut total = 0usize;
    for window in 0..windows {
        total = total.saturating_add(usize::from(block_length(layout, window)?));
    }
    if total != frame {
        return Err(FrameSynthesisError::BlockLengthsDoNotSpanFrame { total, frame });
    }
    if ungrouped.len() < total {
        return Err(FrameSynthesisError::NotEnoughLines {
            needed: total,
            provided: ungrouped.len(),
        });
    }

    let mut offset = 0usize;
    for window in 0..windows {
        let length = usize::from(block_length(layout, window)?);
        let end = offset.saturating_add(length);
        let (Some(lines), Some(target)) = (ungrouped.get(offset..end), pcm.get_mut(offset..end))
        else {
            return Err(FrameSynthesisError::NotEnoughLines {
                needed: end,
                provided: ungrouped.len(),
            });
        };
        transform::transform(lines, workspace, &mut state.overlap, target)
            .map_err(|error| FrameSynthesisError::Block { window, error })?;
        offset = end;
    }
    Ok(())
}

/// 布局覆盖的样本数，即各窗口变换长度之和。
///
/// 按 `5.5.3`，合法布局的所有部分块尺寸之和应等于表 83 的 `frame_length`。
/// 本函数只计算布局侧的值；若调用方持有 TOC 推导出的帧长，应使用那个独立值
/// 建立 [`ChannelSynthesis`]，由 [`synthesize`] 保留跨层一致性检查。
///
/// # Errors
///
/// 窗口布局给出的组下标越界时返回 [`FrameSynthesisError::InvalidWindow`]。
pub fn spanned_length(layout: &AsfWindowLayout) -> Result<u16, FrameSynthesisError> {
    let mut total = 0u32;
    for window in 0..usize::from(layout.num_windows()) {
        total = total.saturating_add(u32::from(block_length(layout, window)?));
    }
    u16::try_from(total).map_err(|_| FrameSynthesisError::BlockLengthsDoNotSpanFrame {
        total: total as usize,
        frame: 0,
    })
}

/// 窗口 `window` 的变换长度，即它所属组的 `transform_length`。
fn block_length(layout: &AsfWindowLayout, window: usize) -> Result<u16, FrameSynthesisError> {
    let group = layout
        .window_to_group(window)
        .ok_or(FrameSynthesisError::InvalidWindow { window })?;
    layout
        .transform_length(usize::from(group))
        .ok_or(FrameSynthesisError::InvalidWindow { window })
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "下标由同一用例构造的布局与帧长派生，位串与伪随机源的算术在固定小范围内；\
              越界或溢出即是该用例要报告的失败"
)]
mod tests {
    extern crate std;

    use super::*;
    use crate::asf::framing::{AsfFraming, AsfPsyContext, AsfPsyInfo, AsfTransformInfo};
    use crate::asf::tables::{n_msfb_bits_48, num_sfb_48};
    use macindecode_ac4_bitstream::reader::BitReader;
    use std::vec;
    use std::vec::Vec;

    /// 最小的 MSB 优先位串构造器。
    ///
    /// 不复用 `testutil::BitBuf`：那个模块只在 `audio-decode` 下编译，而本模块
    /// 参与默认构建，测试也应当在两种配置下都跑。
    #[derive(Default)]
    struct Bits {
        bytes: Vec<u8>,
        len: usize,
    }

    impl Bits {
        fn push(&mut self, value: u32, width: u32) {
            for shift in (0..width).rev() {
                if self.len.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                if (value >> shift) & 1 == 1 {
                    let index = self.len / 8;
                    self.bytes[index] |= 0x80 >> (self.len % 8);
                }
                self.len += 1;
            }
        }
    }

    /// 按成帧形态构造窗口布局；`grouping` 为假时每个窗口自成一组。
    fn layout_for(frame_len_base: u16, framing: AsfFraming, grouping: bool) -> AsfWindowLayout {
        let mut buf = Bits::default();
        match framing {
            AsfFraming::Long => buf.push(1, 1),
            AsfFraming::Split { first, second } => {
                buf.push(0, 1);
                buf.push(u32::from(first), 2);
                buf.push(u32::from(second), 2);
            }
            AsfFraming::Single { index } => buf.push(u32::from(index), 2),
        }
        let mut reader = BitReader::new(&buf.bytes);
        let transform =
            AsfTransformInfo::parse(&mut reader, frame_len_base, 48_000).expect("成帧应可解析");
        assert_eq!(transform.framing, framing, "解出的成帧应与构造一致");

        // 两个半帧的变换长度不同时各传一个 max_sfb，相同则只传一个。
        let slots: Vec<u8> = match framing {
            AsfFraming::Long => vec![4],
            AsfFraming::Split { first, second } if first != second => vec![first, second],
            AsfFraming::Split { first, .. } => vec![first],
            AsfFraming::Single { index } => vec![index],
        };
        let mut psy_buf = Bits::default();
        for slot in slots {
            let length = transform.transform_length(slot).expect("变换长度");
            let width = n_msfb_bits_48(length).expect("max_sfb 位宽");
            let max_sfb = num_sfb_48(length).expect("num_sfb");
            psy_buf.push(u32::from(max_sfb), u32::from(width));
        }
        for _ in 0..transform.n_grp_bits().expect("分组比特数") {
            psy_buf.push(u32::from(grouping), 1);
        }
        let mut reader = BitReader::new(&psy_buf.bytes);
        let psy = AsfPsyInfo::parse(&mut reader, &transform, AsfPsyContext::default())
            .expect("psy 应可解析");
        AsfWindowLayout::derive(&transform, &psy, false).expect("分组推导应成功")
    }

    /// 前半 8 个 128 点窗口、后半 1 个 1 024 点窗口，各自成组。
    ///
    /// 用不等长布局而非等长布局，是因为等长会让「窗口顺序」与「窗口到组的
    /// 映射」双双退化成恒等——注入实测确认，这两类错误在等长布局上完全不可
    /// 观察，无论比较得多细。
    fn uneven_split_layout() -> AsfWindowLayout {
        let layout = layout_for(
            2048,
            AsfFraming::Split {
                first: 0,
                second: 3,
            },
            false,
        );
        assert_eq!(layout.num_windows(), 9);
        assert_eq!(layout.num_window_groups(), 9);
        assert_eq!(layout.transform_length(0), Some(128));
        assert_eq!(layout.transform_length(8), Some(1024));
        layout
    }

    fn deterministic_lines(count: usize) -> Vec<f32> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        (0..count)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 40) as i32 - 8_388_608) as f32 / 8_388_608.0
            })
            .collect()
    }

    /// 独立地按窗口顺序逐块变换，供帧层测试作参考路径。
    fn transform_frame_block_by_block(
        lines: &[f32],
        layout: &AsfWindowLayout,
        workspace: &mut ImdctWorkspace,
        overlap: &mut OverlapBuffer,
    ) -> Vec<f32> {
        let frame = usize::from(overlap.frame_length());
        let mut pcm = vec![0.0f32; frame];
        let mut offset = 0usize;
        for window in 0..usize::from(layout.num_windows()) {
            let group = layout.window_to_group(window).expect("窗口应有组");
            let length = usize::from(
                layout
                    .transform_length(usize::from(group))
                    .expect("组应有长度"),
            );
            let end = offset + length;
            transform::transform(
                &lines[offset..end],
                workspace,
                overlap,
                &mut pcm[offset..end],
            )
            .expect("块应可变换");
            offset = end;
        }
        assert_eq!(offset, frame, "参考路径应覆盖整帧");
        pcm
    }

    /// 各块长度之和必须精确等于 `frame_length`，这是 `5.5.3` 的明文要求。
    ///
    /// 它同时是成帧层与变换层之间的一致性检查：窗口布局由 `asf_transform_info`
    /// 与 `asf_psy_info` 推出，帧长来自表 83，两者由不同字段决定。
    #[test]
    fn block_lengths_span_exactly_one_frame() {
        for (frame_len_base, layout) in [
            (2048u16, layout_for(2048, AsfFraming::Long, false)),
            (1920, layout_for(1920, AsfFraming::Long, false)),
            (1536, layout_for(1536, AsfFraming::Long, false)),
            (2048, uneven_split_layout()),
            (
                2048,
                layout_for(
                    2048,
                    AsfFraming::Split {
                        first: 1,
                        second: 1,
                    },
                    false,
                ),
            ),
            (
                1024,
                layout_for(1024, AsfFraming::Single { index: 2 }, true),
            ),
        ] {
            let total: usize = (0..usize::from(layout.num_windows()))
                .map(|window| usize::from(block_length(&layout, window).expect("窗口应有长度")))
                .sum();
            assert_eq!(
                total,
                usize::from(frame_len_base),
                "{} 个窗口的块长之和应等于帧长",
                layout.num_windows()
            );
        }
    }

    /// 不等长多组帧：帧层合成与逐块直接变换逐位相同。
    ///
    /// 参考路径独立地按窗口升序取块长，因此窗口顺序、窗口到组的映射与切片
    /// 边界三者中任何一项出错，都会让某个样本位改变。
    #[test]
    fn uneven_frame_matches_block_by_block_transforms() {
        let layout = uneven_split_layout();
        let lines = deterministic_lines(2048);

        let mut workspace = ImdctWorkspace::new();
        let mut state = ChannelSynthesis::new(2048).expect("2048 是合法全块长度");
        let mut framed = vec![0.0f32; 2048];
        synthesize(&mut state, &lines, &layout, &mut workspace, &mut framed)
            .expect("不等长帧应可合成");

        let mut direct_workspace = ImdctWorkspace::new();
        let mut overlap = OverlapBuffer::new(2048).expect("2048 是合法全块长度");
        let direct =
            transform_frame_block_by_block(&lines, &layout, &mut direct_workspace, &mut overlap);

        for (index, (&framed, &raw)) in framed.iter().zip(direct.iter()).enumerate() {
            assert_eq!(framed.to_bits(), raw.to_bits(), "样本 {index} 应逐位相同");
        }
        assert_eq!(
            state.previous_block_length(),
            1024,
            "末块是 1 024，重叠状态应停在它上面"
        );
    }

    /// 连续多帧：重叠状态跨帧延续，帧边界处的块长切换正确衔接。
    #[test]
    fn consecutive_frames_carry_overlap_across_the_boundary() {
        let uneven = uneven_split_layout();
        let long = layout_for(2048, AsfFraming::Long, false);
        let mut workspace = ImdctWorkspace::new();
        let mut state = ChannelSynthesis::new(2048).expect("2048 是合法全块长度");
        let mut direct_workspace = ImdctWorkspace::new();
        let mut direct_overlap = OverlapBuffer::new(2048).expect("2048 是合法全块长度");
        let source = deterministic_lines(4 * 2048);

        for index in 0..4usize {
            let use_long = index % 2 == 1;
            let layout = if use_long { &long } else { &uneven };
            let lines = &source[2048 * index..2048 * (index + 1)];
            let mut pcm = vec![0.0f32; 2048];
            synthesize(&mut state, lines, layout, &mut workspace, &mut pcm)
                .expect("每帧都应可合成");
            assert!(pcm.iter().all(|value| value.is_finite()));

            let direct = transform_frame_block_by_block(
                lines,
                layout,
                &mut direct_workspace,
                &mut direct_overlap,
            );
            for (sample, (&framed, &raw)) in pcm.iter().zip(direct.iter()).enumerate() {
                assert_eq!(
                    framed.to_bits(),
                    raw.to_bits(),
                    "第 {index} 帧样本 {sample} 应与持续状态的逐块路径相同"
                );
            }

            if index > 0 {
                let mut fresh_workspace = ImdctWorkspace::new();
                let mut fresh_state = ChannelSynthesis::new(2048).expect("2048 是合法全块长度");
                let mut fresh = vec![0.0f32; 2048];
                synthesize(
                    &mut fresh_state,
                    lines,
                    layout,
                    &mut fresh_workspace,
                    &mut fresh,
                )
                .expect("同一帧从空状态也应可合成");
                assert_ne!(pcm, fresh, "第 {index} 帧必须能观察到前一帧留下的 overlap");
            }

            assert_eq!(
                state.previous_block_length(),
                if use_long { 2048 } else { 1024 },
                "第 {index} 帧的末块长度"
            );
        }
    }

    /// 非法输入一律拒绝，且不推进重叠状态。
    #[test]
    fn invalid_input_is_rejected_without_touching_state() {
        let layout = layout_for(2048, AsfFraming::Long, false);
        let lines = deterministic_lines(2048);
        let mut workspace = ImdctWorkspace::new();
        let mut state = ChannelSynthesis::new(2048).expect("2048 是合法全块长度");

        let mut short_pcm = vec![0.0f32; 1024];
        assert_eq!(
            synthesize(&mut state, &lines, &layout, &mut workspace, &mut short_pcm),
            Err(FrameSynthesisError::OutputLengthMismatch {
                expected: 2048,
                provided: 1024
            })
        );

        // 布局跨越 2 048，状态却按 1 024 建立：块长之和与帧长不符。
        let mut mismatched = ChannelSynthesis::new(1024).expect("1024 是合法全块长度");
        let mut pcm = vec![0.0f32; 1024];
        assert_eq!(
            synthesize(&mut mismatched, &lines, &layout, &mut workspace, &mut pcm),
            Err(FrameSynthesisError::BlockLengthsDoNotSpanFrame {
                total: 2048,
                frame: 1024
            })
        );

        // 反向的不符：布局只跨 1 024，状态却按 2 048 建立。两侧都要测，
        // 否则把相等判据放宽成任一单向不等式都不会被发现。
        let short_layout = layout_for(1024, AsfFraming::Single { index: 2 }, true);
        let mut pcm = vec![0.0f32; 2048];
        assert_eq!(
            synthesize(
                &mut state,
                &deterministic_lines(1024),
                &short_layout,
                &mut workspace,
                &mut pcm
            ),
            Err(FrameSynthesisError::BlockLengthsDoNotSpanFrame {
                total: 1024,
                frame: 2048
            })
        );

        let mut pcm = vec![0.0f32; 2048];
        assert_eq!(
            synthesize(&mut state, &lines[..100], &layout, &mut workspace, &mut pcm),
            Err(FrameSynthesisError::NotEnoughLines {
                needed: 2048,
                provided: 100
            })
        );

        assert_eq!(state.previous_block_length(), 2048, "拒绝不应推进重叠状态");
        assert_eq!(mismatched.previous_block_length(), 1024);
        assert_eq!(
            ChannelSynthesis::new(300).map(|_| ()),
            None,
            "表外全块长度应被拒绝"
        );
    }
}
