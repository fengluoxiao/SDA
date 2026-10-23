//! PCM 帧对齐（`TS103190-1:v1.4.1:5.6`，表 188）。
//!
//! IMDCT 输出在进入 QMF 工具之前要补一段 PCM 延迟，使同一 raw AC-4 frame
//! 里的 QMF 控制数据与对应的 spectral frontend 信号相隔整数个 codec frame。
//! 表 188 同时给出 PCM 延迟 [`FrameAlignment::pcm_delay`] 与控制数据需要保留的
//! 帧数 [`FrameAlignment::control_delay_frames`]。
//!
//! 表 188 按帧率列值；表 83/87 能把当前支持的八种 codec frame length 唯一映射
//! 回同一行延迟。共享同一 frame length 的帧率在表 188 中也共享同一对值，因此
//! 这里以 frame length 为 API，调用方不必再传一份可互相矛盾的帧率。

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "环形缓冲下标始终小于表 188 的最大延迟，且在每次递增后显式回绕"
)]

/// 表 188 的最大 PCM 延迟：100 fps 对应 1 312 个样本。
pub const MAX_PCM_ALIGNMENT_DELAY: usize = 1_312;
/// 表 188 的最大控制延迟：100/120 fps 对应 4 个 codec frame。
pub const MAX_CONTROL_ALIGNMENT_DELAY_FRAMES: usize = 4;

/// 一种 codec frame length 对应的帧对齐参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameAlignment {
    pcm_delay: u16,
    control_delay_frames: u8,
}

impl FrameAlignment {
    /// PCM 信号延迟，单位为时域样本。
    #[must_use]
    pub const fn pcm_delay(self) -> u16 {
        self.pcm_delay
    }

    /// QMF 控制数据需要暂存的 codec frame 数。
    #[must_use]
    pub const fn control_delay_frames(self) -> u8 {
        self.control_delay_frames
    }
}

/// 由表 188 查询帧对齐参数。
///
/// 输入是表 189/192 同一组八种 codec frame length。其他长度返回 `None`，不从
/// 相近帧率猜测延迟。
#[must_use]
pub const fn frame_alignment(frame_length: u16) -> Option<FrameAlignment> {
    let (pcm_delay, control_delay_frames) = match frame_length {
        // 23,976/24 fps。
        1_920 => (288, 1),
        // 25 fps 与 23,4375 fps（48 kHz 音乐）；两行参数相同。
        2_048 => (352, 1),
        // 29,97/30 fps。
        1_536 => (96, 1),
        // 47,952/48 fps。
        960 => (960, 2),
        // 50 fps。
        1_024 => (1_056, 2),
        // 59,94/60 fps。
        768 => (672, 2),
        // 100 fps。
        512 => (1_312, 4),
        // 119,88/120 fps。
        384 => (864, 4),
        _ => return None,
    };
    Some(FrameAlignment {
        pcm_delay,
        control_delay_frames,
    })
}

/// PCM 帧对齐失败原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAlignmentError {
    /// 输入与输出长度不同。
    LengthMismatch { input: usize, output: usize },
    /// 同一状态在未重置时切换到了另一档 PCM 延迟。
    DelayChanged { previous: u16, current: u16 },
}

/// 一路 PCM 的表 188 延迟状态。
///
/// 环形缓冲只使用前 `delay` 项。首次调用之前的历史定义为静音；成功处理后，每
/// 次调用仍产出与输入完全相同数量的样本。帧边界不进入算法，因此即使 PCM 延迟
/// 大于单帧长度（表 188 的 50/100/120 fps 行）也能连续工作。
#[derive(Debug, PartialEq)]
pub struct FrameAlignmentState {
    history: [f32; MAX_PCM_ALIGNMENT_DELAY],
    write: usize,
    filled: usize,
    delay: Option<u16>,
}

impl FrameAlignmentState {
    /// 建立全新状态；帧前历史为静音。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            history: [0.0; MAX_PCM_ALIGNMENT_DELAY],
            write: 0,
            filled: 0,
            delay: None,
        }
    }

    /// 丢弃历史，允许下一次选择任一表 188 延迟档。
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// 延迟一路 PCM，输出长度与输入相同。
    ///
    /// 所有可失败条件都在写输出与推进状态之前核对；失败后可修正参数并原样重试。
    pub fn process(
        &mut self,
        input: &[f32],
        alignment: FrameAlignment,
        output: &mut [f32],
    ) -> Result<(), FrameAlignmentError> {
        if input.len() != output.len() {
            return Err(FrameAlignmentError::LengthMismatch {
                input: input.len(),
                output: output.len(),
            });
        }
        if let Some(previous) = self.delay
            && previous != alignment.pcm_delay
        {
            return Err(FrameAlignmentError::DelayChanged {
                previous,
                current: alignment.pcm_delay,
            });
        }

        let delay = usize::from(alignment.pcm_delay);
        for (source, target) in input.iter().zip(output.iter_mut()) {
            *target = if self.filled < delay {
                0.0
            } else {
                self.history[self.write]
            };
            self.history[self.write] = *source;
            self.write += 1;
            if self.write == delay {
                self.write = 0;
            }
            if self.filled < delay {
                self.filled += 1;
            }
        }
        self.delay = Some(alignment.pcm_delay);
        Ok(())
    }
}

impl Default for FrameAlignmentState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec;
    use std::vec::Vec;

    #[test]
    fn table_188_maps_every_supported_codec_frame_length() {
        for (frame, pcm, control) in [
            (1_920, 288, 1),
            (2_048, 352, 1),
            (1_536, 96, 1),
            (960, 960, 2),
            (1_024, 1_056, 2),
            (768, 672, 2),
            (512, 1_312, 4),
            (384, 864, 4),
        ] {
            let value = frame_alignment(frame).expect("表 188 应有对应行");
            assert_eq!(value.pcm_delay(), pcm, "frame length {frame}");
            assert_eq!(value.control_delay_frames(), control);
        }
        assert_eq!(frame_alignment(2_000), None, "不能按相近帧率猜测");
        assert_eq!(
            [1_920, 2_048, 1_536, 960, 1_024, 768, 512, 384]
                .into_iter()
                .filter_map(frame_alignment)
                .map(|alignment| usize::from(alignment.control_delay_frames()))
                .max(),
            Some(MAX_CONTROL_ALIGNMENT_DELAY_FRAMES),
            "公开上限必须覆盖表 188 的全部行"
        );
    }

    #[test]
    fn consecutive_calls_form_one_sample_accurate_delayed_stream() {
        let alignment = frame_alignment(2_048).expect("23,4375 fps");
        let delay = usize::from(alignment.pcm_delay());
        let input: Vec<f32> = (0..4_096).map(|value| value as f32 + 1.0).collect();
        let mut state = FrameAlignmentState::new();
        let mut actual = Vec::new();

        for block in input.chunks(257) {
            let mut output = vec![-1.0; block.len()];
            state
                .process(block, alignment, &mut output)
                .expect("连续延迟");
            actual.extend(output);
        }

        let expected = (0..input.len())
            .map(|index| {
                index
                    .checked_sub(delay)
                    .and_then(|source| input.get(source).copied())
                    .unwrap_or(0.0)
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn delays_larger_than_one_input_block_carry_across_multiple_calls() {
        let alignment = frame_alignment(512).expect("100 fps");
        assert!(usize::from(alignment.pcm_delay()) > 512);
        let mut state = FrameAlignmentState::new();
        let mut joined = Vec::new();
        for block_index in 0..4usize {
            let input = vec![block_index as f32 + 1.0; 512];
            let mut output = vec![f32::NAN; 512];
            state
                .process(&input, alignment, &mut output)
                .expect("跨多帧延迟");
            joined.extend(output);
        }
        let delay = usize::from(alignment.pcm_delay());
        assert!(joined[..delay].iter().all(|sample| *sample == 0.0));
        assert_eq!(joined[delay], 1.0);
        assert_eq!(joined[delay + 511], 1.0);
        assert_eq!(joined[delay + 512], 2.0);
    }

    #[test]
    fn errors_are_transactional_and_reset_allows_a_new_delay() {
        let first = frame_alignment(2_048).expect("第一档");
        let second = frame_alignment(1_536).expect("第二档");
        let mut state = FrameAlignmentState::new();
        let mut output = [9.0; 2];
        assert_eq!(
            state.process(&[1.0], first, &mut output),
            Err(FrameAlignmentError::LengthMismatch {
                input: 1,
                output: 2
            })
        );
        assert_eq!(output, [9.0; 2]);
        assert_eq!(state, FrameAlignmentState::new());

        state
            .process(&[1.0], first, &mut output[..1])
            .expect("第一档");
        let snapshot = FrameAlignmentState {
            history: state.history,
            write: state.write,
            filled: state.filled,
            delay: state.delay,
        };
        assert_eq!(
            state.process(&[2.0], second, &mut output[..1]),
            Err(FrameAlignmentError::DelayChanged {
                previous: first.pcm_delay(),
                current: second.pcm_delay()
            })
        );
        assert_eq!(state, snapshot);

        state.reset();
        state
            .process(&[2.0], second, &mut output[..1])
            .expect("重置后可切档");
    }
}
