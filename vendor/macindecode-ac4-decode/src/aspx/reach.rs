//! A-SPX 解析结果实际触发的解码分支。

use super::{AspxData, IntervalClass, MAX_ASPX_TIMESLOTS};

/// A-SPX 各解码分支在一帧内是否被触发。
///
/// 这是解析结果的只读观察，不是支持凭证。当前 Full 子集只因交织分支拒绝一帧；
/// 其余标志仍供 engine observation 与 CLI census 使用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AspxReach {
    add_harmonic: bool,
    interleaved: bool,
    variable_framing: bool,
    balance: bool,
}

impl AspxReach {
    /// 从四个独立的 A-SPX 分支标志建立观察值。
    #[must_use]
    pub const fn new(
        add_harmonic: bool,
        interleaved: bool,
        variable_framing: bool,
        balance: bool,
    ) -> Self {
        Self {
            add_harmonic,
            interleaved,
            variable_framing,
            balance,
        }
    }

    /// 任一子带组是否请求 `5.7.6.4.4` 音调生成。
    #[must_use]
    pub const fn add_harmonic(self) -> bool {
        self.add_harmonic
    }

    /// 任一 FIC/TIC 掩码是否实际启用。
    #[must_use]
    pub const fn interleaved(self) -> bool {
        self.interleaved
    }

    /// 成帧类别是否至少一次不是 `FIXFIX`。
    #[must_use]
    pub const fn variable_framing(self) -> bool {
        self.variable_framing
    }

    /// 任一 A-SPX 元素是否使用 balance 编码。
    #[must_use]
    pub const fn balance(self) -> bool {
        self.balance
    }

    /// 合并同一 codec frame 内另一条物理 substream 的观察值。
    pub fn merge(&mut self, other: Self) {
        self.add_harmonic |= other.add_harmonic;
        self.interleaved |= other.interleaved;
        self.variable_framing |= other.variable_framing;
        self.balance |= other.balance;
    }
}

/// 扫描一条物理 substream 的 A-SPX 元素，汇总实际触发的解码分支。
///
/// 逐声道、逐有效子带组取或；未填充的工作区槽位不会进入观察结果。
#[must_use]
pub fn collect_aspx_reach(elements: &[AspxData]) -> AspxReach {
    let mut reach = AspxReach::default();
    for data in elements {
        reach.balance |= data.balance == Some(true);
        let groups = usize::from(data.bands.num_sbg_master());
        for channel in 0..usize::from(data.channels) {
            if let Some(framing) = data.framing(channel) {
                reach.variable_framing |= framing.params.int_class != IntervalClass::FixFix;
            }
            let Some(hfgen) = data.hfgen(channel) else {
                continue;
            };
            for group in 0..groups {
                reach.add_harmonic |= hfgen.add_harmonic(group) == Some(true);
                reach.interleaved |= hfgen.fic_used_in_sfb(group) == Some(true);
            }
            for slot in 0..MAX_ASPX_TIMESLOTS {
                reach.interleaved |= hfgen.tic_used_in_slot(slot) == Some(true);
            }
        }
    }
    reach
}
