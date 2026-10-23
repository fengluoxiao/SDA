//! `var_channel_element()` 的组装。
//!
//! `TS103190-2:v1.3.1` 的 `6.2.4.4` 给出语法，`6.3.5.5`–`6.3.5.6` 给出语义。
//! 它由 `6.2.3.4` 的 `audio_data_ajoc()` 在 `b_static_dmx` 为假时调用，是本
//! 编码链落到 `audio_size` 的核心元素。
//!
//! 声道数据元素与 A-SPX 数据元素**分两段传输**：先是全部 `mono_data()` 与
//! `*_channel_data()`，再是全部 `aspx_data_*()`。两段的元素数不同，故各自
//! 计数。
//!
//! 工作区由调用方提供：一个 [`ChannelElement`] 约 60 KiB，最坏情形需要九
//! 个，放在栈上并不现实。调用方可以复用同一组缓冲跨帧解析。

use crate::aspx::{
    AspxReach, collect_aspx_reach,
    syntax::{AspxConfig, AspxData, AspxError, AspxState},
};
use crate::channel::{ChannelContext, ChannelElement, ChannelError, CompandingControl};
use core::fmt;
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

/// `n_fullband_dmx_signals` 的上界。
///
/// `6.3.2.8.4` 的 `n_fullband_dmx_signals_minus1` 占 4 位，故该值为 16。
pub const MAX_FULLBAND_DMX_SIGNALS: u8 = 16;

/// 一个 `var_channel_element()` 内的最大声道数据元素数。
///
/// 十六个全频带信号取偶数分支得八个 `two_channel_data()`，加上 LFE 的
/// `mono_data(1)` 共九个；十五个信号取奇数分支得六加二再加 LFE，同为九个。
pub const MAX_CHANNEL_ELEMENTS: usize = 9;

/// 一个 `var_channel_element()` 内的最大 A-SPX 数据元素数。
///
/// `n_pairs` 至多为 8，奇数分支再加一个 `aspx_data_1ch()`；两者不会同时取
/// 到上界，故为 8。
pub const MAX_ASPX_ELEMENTS: usize = 8;

/// `var_channel_element()` 解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarElementError {
    /// 读取比特时越过了数据末尾。
    Read(ReadError),
    /// 声道数据元素解析失败。
    Channel(ChannelError),
    /// A-SPX 数据元素解析失败。
    Aspx(AspxError),
    /// `n_fullband_dmx_signals` 越界。
    SignalCountOutOfRange {
        /// 传入的值。
        n_dmx_signals: u8,
        /// 规范上界。
        limit: u8,
    },
    /// 调用方提供的声道元素工作区不足。
    ChannelWorkspaceTooSmall {
        /// 本元素需要的个数。
        needed: usize,
        /// 实际提供的个数。
        provided: usize,
    },
    /// 调用方提供的 A-SPX 工作区不足。
    AspxWorkspaceTooSmall {
        /// 本元素需要的个数。
        needed: usize,
        /// 实际提供的个数。
        provided: usize,
    },
    /// 非 I 帧却没有可沿用的 `aspx_config()`。
    ///
    /// `5.7.6.3.1.0` 指出，码流被切在非 I 帧处时解码器无法复原高频，直到
    /// 收到新的 A-SPX 头。此处显式报错而非按缺省值猜测。
    MissingAspxConfig,
}

impl fmt::Display for VarElementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VarElementError::Read(error) => write!(f, "{error}"),
            VarElementError::Channel(error) => write!(f, "{error}"),
            VarElementError::Aspx(error) => write!(f, "{error}"),
            VarElementError::SignalCountOutOfRange {
                n_dmx_signals,
                limit,
            } => write!(
                f,
                "n_fullband_dmx_signals {n_dmx_signals} exceeds limit {limit}"
            ),
            VarElementError::ChannelWorkspaceTooSmall { needed, provided } => write!(
                f,
                "Element requires {needed} channel workspaces, but only {provided} were provided"
            ),
            VarElementError::AspxWorkspaceTooSmall { needed, provided } => write!(
                f,
                "Element requires {needed} A-SPX workspaces, but only {provided} were provided"
            ),
            VarElementError::MissingAspxConfig => {
                write!(f, "Non-I-frame has no prior aspx_config to continue")
            }
        }
    }
}

impl core::error::Error for VarElementError {}

impl From<ReadError> for VarElementError {
    fn from(error: ReadError) -> Self {
        VarElementError::Read(error)
    }
}

impl From<ChannelError> for VarElementError {
    fn from(error: ChannelError) -> Self {
        VarElementError::Channel(error)
    }
}

impl From<AspxError> for VarElementError {
    fn from(error: AspxError) -> Self {
        VarElementError::Aspx(error)
    }
}

/// `var_channel_element()` 的跨帧状态。
///
/// `aspx_config()` 每个元素一份、只在 I 帧传输；逐 A-SPX 元素的
/// `aspx_xover_subband_offset` 与 `previous_stop_pos` 各自延续。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarChannelState {
    config: Option<AspxConfig>,
    aspx: [AspxState; MAX_ASPX_ELEMENTS],
}

impl VarChannelState {
    /// 一个尚未收到任何 I 帧的初始状态。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            config: None,
            aspx: [AspxState::new(); MAX_ASPX_ELEMENTS],
        }
    }

    /// 当前沿用的 `aspx_config()`。
    #[must_use]
    pub const fn config(&self) -> Option<AspxConfig> {
        self.config
    }
}

impl Default for VarChannelState {
    fn default() -> Self {
        Self::new()
    }
}

/// `var_channel_element()` 的调用参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarChannelParams {
    /// 声道元素的解码上下文。
    pub context: ChannelContext,
    /// `n_fullband_dmx_signals`，**不含 LFE**——`6.2.3.4` 传入的实参即
    /// `n_fb_dmx_signals`。
    pub n_dmx_signals: u8,
    /// 是否带 LFE，对应 `6.2.3.4` 的 `b_lfe`。
    pub b_has_lfe: bool,
    /// 是否为 I 帧；决定 `aspx_config()` 与若干可变边界是否出现。
    pub b_iframe: bool,
}

/// 调用方提供的工作区。
///
/// 一个 [`ChannelElement`] 约 60 KiB，最坏情形需要
/// [`MAX_CHANNEL_ELEMENTS`] 个，放在栈上并不现实；两组缓冲都可跨帧复用。
#[derive(Debug)]
pub struct VarChannelWorkspace<'a> {
    /// 声道数据元素，按传输顺序填充。
    pub elements: &'a mut [ChannelElement],
    /// A-SPX 数据元素，按传输顺序填充。
    pub aspx: &'a mut [AspxData],
}

/// 一个 `var_channel_element()` 的解析结果摘要。
///
/// 逐元素的数据留在调用方提供的工作区里；本结构记录该元素的形态，以及不能从
/// 可复用工作区事后重取的驱动决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarChannelElement {
    /// `var_codec_mode`，见表 77：假为 Simple，真为 A-SPX。
    pub codec_mode_aspx: bool,
    /// `companding_control()`，仅 A-SPX 且信号数不超过 5 时传输。
    pub companding: Option<CompandingControl>,
    /// `var_coding_config`，见 `6.3.5.6`；仅奇数分支且信号数大于 1 时传输。
    ///
    /// 假走 `two_channel_data()` 加 `mono_data(0)`，真走
    /// `three_channel_data()`。
    pub coding_config: Option<bool>,
    /// `n_fullband_dmx_signals`，不含 LFE。
    pub n_dmx_signals: u8,
    /// 是否带 LFE。
    pub b_has_lfe: bool,
    channel_elements: u8,
    aspx_elements: u8,
    /// 逐 A-SPX 元素的 `aspx_balance`；第 `e` 位对应第 `e` 个元素。
    ///
    /// 在解析成功时从同一份工作区固化，避免工作区复用或传错同形结果后静默改变
    /// [`Self::aspx_jobs`] 的驱动决策。
    balanced_aspx: u8,
    /// 本元素解析完成时固化的 A-SPX 分支可达性。
    ///
    /// 支持门禁读取这份摘要，而不是再次信任可能已经复用或错配的外部工作区。
    aspx_reach: AspxReach,
}

impl VarChannelElement {
    /// 本元素实际解析的声道数据元素个数。
    #[must_use]
    pub const fn channel_elements(&self) -> u8 {
        self.channel_elements
    }

    /// 本元素实际解析的 A-SPX 数据元素个数。
    #[must_use]
    pub const fn aspx_elements(&self) -> u8 {
        self.aspx_elements
    }

    /// 本元素实际触发的 A-SPX 解码分支。
    #[must_use]
    pub const fn aspx_reach(&self) -> AspxReach {
        self.aspx_reach
    }

    /// `b_isodd`，即信号数是否为奇数。
    #[must_use]
    pub const fn is_odd(&self) -> bool {
        self.n_dmx_signals % 2 == 1
    }

    /// `n_pairs`，即 `floor(n_dmx_signals / 2)`。
    #[must_use]
    pub const fn pairs(&self) -> u8 {
        self.n_dmx_signals / 2
    }

    /// 逐路已解码声道在两段传输里的落点，按传输顺序。
    ///
    /// 先给出 LFE（若有），再给出 `n_fullband_dmx_signals` 路全频带信号。
    /// 迭代出的条目数恒等于 `n_dmx_signals + b_has_lfe`。
    ///
    /// **`coding_config = true` 的奇数尾部，两段分组不一样，这正是需要这张表的
    /// 原因。** 此时声道侧最后三路走一个 `three_channel_data()`，A-SPX 侧却恒
    /// 走一个 `aspx_data_2ch()` 加一个 `aspx_data_1ch()`。以无 LFE、
    /// `n_dmx_signals = 9` 为例，信号 6、7、8 在声道侧属同一个元素，在 A-SPX
    /// 侧分属第 3 与第 4 个元素。`coding_config = false` 时两侧尾部同为 `2 + 1`；
    /// 有 LFE 时它只占声道侧的第一个元素，不占 A-SPX 元素，元素下标仍会偏移。
    /// 因而不能按元素下标一一对应，否则会静默地把参数配错。
    #[must_use]
    pub fn signals(&self) -> SignalLocations {
        // 与 `parse_var_channel_element` 的三条分支同构：LFE 先行，随后是若干
        // 双声道元素，最后是奇数分支的尾巴。
        let tail: &[u8] = if !self.is_odd() {
            &[]
        } else if self.n_dmx_signals == 1 {
            &[1]
        } else if self.coding_config == Some(true) {
            &[3]
        } else {
            &[2, 1]
        };
        let doubles = if self.is_odd() {
            self.pairs().saturating_sub(1)
        } else {
            self.pairs()
        };

        let mut widths = [0u8; MAX_CHANNEL_ELEMENTS];
        let mut used = 0usize;
        if self.b_has_lfe {
            if let Some(slot) = widths.get_mut(used) {
                *slot = 1;
            }
            used = used.saturating_add(1);
        }
        for width in core::iter::repeat_n(2u8, usize::from(doubles)).chain(tail.iter().copied()) {
            if let Some(slot) = widths.get_mut(used) {
                *slot = width;
            }
            used = used.saturating_add(1);
        }

        SignalLocations {
            widths,
            used: u8::try_from(used).unwrap_or(u8::MAX),
            has_aspx: self.codec_mode_aspx,
            lfe_first: self.b_has_lfe,
            element: 0,
            channel: 0,
            signal: 0,
        }
    }

    /// 逐路或逐对给出本元素该按哪个 A-SPX 入口驱动。
    ///
    /// 作业逐条覆盖 [`Self::signals`] 的每一路，一路不漏、一路不重。
    ///
    /// **要点是 `aspx_balance`。** 它逐 A-SPX 数据元素传输，为真时两路的 `qscf`
    /// 必须按 `Pseudocode 84` 一起反量化；拆成两次单路驱动算出的标度因子是错
    /// 的，而两路照样各有输出、各有峰值，落点与统计判据一概看不出来。因此这个
    /// 决定不能留给调用方按元素下标猜，也不能从可跨帧复用的工作区重新读取；解析
    /// 成功时已把逐元素标志固化在本摘要里，由这里连同信号落点一起给出。
    #[must_use]
    pub fn aspx_jobs(&self) -> AspxJobs {
        // 平衡式的一对恒是同一个 A-SPX 元素的 `ch == 0` 与 `ch == 1`，且在信号
        // 顺序上相邻。配对条件把这三点全查一遍，其中后两点**没能构造出可达的
        // 输入**：`balanced_aspx` 的位只在 `parse_2ch` 那一轮置起，为真的元素
        // 恒有 2 路，而映射把这 2 路连着发出。注入它们不会有判据响，属纵深防御
        // 而非判据缺口。
        const FILLER: AspxJob = AspxJob::LfeQmf(SignalLocation {
            channel_element: 0,
            channel_in_element: 0,
            aspx: None,
        });
        let mut jobs = [FILLER; MAX_SIGNALS];
        let mut used = 0usize;
        let mut signals = self.signals();
        let mut pending = signals.next();
        while let Some(first) = pending {
            let second = signals.next();
            let paired = matches!(
                (first.aspx, second.and_then(|slot| slot.aspx)),
                (Some((left, 0)), Some((right, 1)))
                    if left == right
                        && self.aspx_is_balanced(left)
            );
            let job = match (paired, second) {
                (true, Some(second)) => {
                    pending = signals.next();
                    AspxJob::Balanced([first, second])
                }
                _ => {
                    pending = second;
                    match first.aspx {
                        Some(_) => AspxJob::Mono(first),
                        None if self.is_lfe(first) => AspxJob::LfeQmf(first),
                        None => AspxJob::SimpleQmf(first),
                    }
                }
            };
            if let Some(slot) = jobs.get_mut(used) {
                *slot = job;
            }
            used = used.saturating_add(1);
        }

        AspxJobs {
            jobs,
            used,
            next: 0,
        }
    }

    /// 供其他模块的判据组装一个已解析的元素。
    ///
    /// `channel_elements`/`aspx_elements`/`balanced_aspx` 按解析器的同一套规则
    /// 推出，不由调用方任意指定——否则用它搭出来的夹具可以自相矛盾，而本模块
    /// 那几条路由判据恰恰是靠这几个量互相钉住的。
    #[cfg(test)]
    pub(crate) fn for_test(
        codec_mode_aspx: bool,
        coding_config: Option<bool>,
        n_dmx_signals: u8,
        b_has_lfe: bool,
        balanced: &[bool],
    ) -> Self {
        let pairs = n_dmx_signals / 2;
        let is_odd = n_dmx_signals % 2 == 1;
        let aspx_elements = if codec_mode_aspx {
            pairs.saturating_add(u8::from(is_odd))
        } else {
            0
        };
        let mut balanced_aspx = 0u8;
        let mut balance_reached = false;
        for (index, flag) in balanced.iter().enumerate().take(usize::from(pairs)) {
            if *flag {
                balanced_aspx |= 1u8 << index;
                balance_reached = true;
            }
        }
        let channel_elements = if is_odd && n_dmx_signals > 1 && coding_config == Some(true) {
            channel_element_count(n_dmx_signals, b_has_lfe).saturating_sub(1)
        } else {
            channel_element_count(n_dmx_signals, b_has_lfe)
        };
        Self {
            codec_mode_aspx,
            companding: None,
            coding_config,
            n_dmx_signals,
            b_has_lfe,
            channel_elements: u8::try_from(channel_elements).unwrap_or(u8::MAX),
            aspx_elements,
            balanced_aspx,
            aspx_reach: AspxReach::new(false, false, false, balance_reached),
        }
    }

    /// 送进 A-JOC 之前的输入重排，见 P2 `5.7.2.1` 的 `Pseudocode 14a`。
    ///
    /// 返回第 `i` 个 `Qin_AJOC` 该取哪一路，按 A-JOC 的输入顺序；条目数恒为
    /// `n_fullband_dmx_signals`。
    ///
    /// **A-JOC 的输入顺序不是传输顺序。** `m'_fb > 3` 时最后 `n_offset` 路要挪
    /// 到最前面，`n_offset` 为 3（奇数）或 2（偶数）。这是 `Pseudocode 14a`
    /// 直接规定的矩阵置换；接口只把这些输入写作通用的 `Qin_AJOC`，不应再借
    /// `Pseudocode 15` 给它们补 L/R/C 标签。后者是独立的**输出**后处理：三个
    /// `b_reconstruction_contains_*` 由 full-decode 的上混床分配派生，只决定 LFE
    /// 插回重建对象序列的位置。搞错输入置换不会有任何一处报错，只是每个对象都
    /// 从错的下混信号重建。
    ///
    /// **LFE 不在其中。** `5.7.2.1` NOTE 1 说得很直白：它不由 A-JOC 处理，而是
    /// 在输出侧重新并入。`Pseudocode 14a` 的 `Q'in[i + 1]` 正是跳过它，与
    /// [`Self::signals`] 把 LFE 放在第 0 位互为印证——两处独立说明了同一件事。
    #[must_use]
    pub fn ajoc_input_order(&self) -> AjocInputOrder {
        const FILLER: SignalLocation = SignalLocation {
            channel_element: 0,
            channel_in_element: 0,
            aspx: None,
        };
        // `signals()` 的顺序就是 `Q'in_AJOC`；去掉 LFE 后剩下的就是 `m'_fb` 路
        // 全频带信号，下标与伪码里减去 LFE 之后的下标一致。
        let mut fullband = [FILLER; MAX_FULLBAND_DMX_SIGNALS as usize];
        let mut count = 0usize;
        for signal in self.signals() {
            if self.is_lfe(signal) {
                continue;
            }
            if let Some(slot) = fullband.get_mut(count) {
                *slot = signal;
            }
            count = count.saturating_add(1);
        }

        let mut order = [FILLER; MAX_FULLBAND_DMX_SIGNALS as usize];
        // 伪码的 `m'_fb > 3` 在边界上是**惰性**的：`n_offset = 2 + m'_fb mod 2`
        // 在 `m'_fb` 取 2 或 3 时恰好等于 `m'_fb` 本身，两个循环双双退化成恒等，
        // 与 else 分支给出同一结果。逐个合法取值核对过 1…16，阈值改成 2 或 4
        // 之中的 2 逐项相同（改成 4 则不同，`m'_fb == 4` 会漏掉重排）。这道判断
        // 真正防的是 `m'_fb < 2` 时 C 里 `m' - n_offset` 的下溢。**注入把它写成
        // 2 不会有判据响，那不是判据缺口。**
        if count > 3 {
            let n_offset = 2usize.saturating_add(count % 2);
            for i in 0..n_offset {
                if let (Some(slot), Some(value)) = (
                    order.get_mut(i),
                    fullband.get(count.saturating_sub(n_offset).saturating_add(i)),
                ) {
                    *slot = *value;
                }
            }
            for i in 0..count.saturating_sub(n_offset) {
                if let (Some(slot), Some(value)) =
                    (order.get_mut(n_offset.saturating_add(i)), fullband.get(i))
                {
                    *slot = *value;
                }
            }
        } else {
            for i in 0..count {
                if let (Some(slot), Some(value)) = (order.get_mut(i), fullband.get(i)) {
                    *slot = *value;
                }
            }
        }

        AjocInputOrder {
            order,
            used: count,
            next: 0,
        }
    }

    fn aspx_is_balanced(&self, element: u8) -> bool {
        if usize::from(element) >= MAX_ASPX_ELEMENTS {
            return false;
        }
        self.balanced_aspx & (1u8 << element) != 0
    }

    /// LFE 恒是第 0 个声道数据元素，且那个元素恒只有一路。
    ///
    /// 后半条来自 `6.2.4.4`：带 LFE 时开头是一个 `mono_data(1)`，故
    /// [`Self::signals`] 给它的宽度恒为 1。再加一条
    /// `channel_in_element == 0` 是恒真的——注入删掉它无判据响，那不是判据
    /// 缺口，是那一条本来就没在判什么。
    fn is_lfe(&self, signal: SignalLocation) -> bool {
        self.b_has_lfe && signal.channel_element == 0
    }
}

/// 一路已解码声道在 `var_channel_element()` 两段传输里的落点。
///
/// 由 [`VarChannelElement::signals`] 给出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalLocation {
    /// 声道数据元素的下标，按传输顺序，即
    /// [`VarChannelWorkspace::elements`] 的下标。
    pub channel_element: u8,
    /// 该声道数据元素内部的声道号。
    pub channel_in_element: u8,
    /// A-SPX 参数的落点：[`VarChannelWorkspace::aspx`] 的下标与元素内声道号。
    ///
    /// LFE 恒为 `None`——`aspx_data_*()` 只覆盖 `n_fullband_dmx_signals` 路，
    /// LFE 不在其中，因此它**不占** A-SPX 的下标；`var_codec_mode` 取 Simple
    /// 时同样全为 `None`，那种流一个 A-SPX 元素也没有。
    pub aspx: Option<(u8, u8)>,
}

/// 一路或一对声道该按哪个 A-SPX 入口驱动。
///
/// 由 [`VarChannelElement::aspx_jobs`] 给出，逐条覆盖本元素的**全部**已解码
/// 声道，一路不漏、一路不重。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspxJob {
    /// LFE：做 QMF 分析，但不进入 A-SPX 或 A-JOC。
    ///
    /// 该 QMF 矩阵按 P2 `5.7.2.3` `Pseudocode 15` 留待重新插入 A-JOC 输出。
    ///
    /// **并回去之前要先补 `ts_offset_hfgen` 个 QMF 时隙的延迟。** `5.7.6.5.3`
    /// 把 `δ_ASPX = ts_offset_hfgen` 称作 A-SPX 引入的总延迟，而 `4.8.3.11.1`
    /// 与 `6.2.10` 把 LFE 排除在 A-SPX 之外；它要并回的那份 A-JOC 输出却是从
    /// `Q_out,ASPX` 一路下来的，已经吃过这段延迟。不补就快 192 或 384 个样本。
    ///
    /// 规范没有明写这一条，属判读，已登记在 `docs/SPEC_TRACEABILITY.md` 第 7
    /// 节。`Pseudocode 15` 对 LFE 只有直接赋值，没有现场移位，因此要求这里产出的
    /// `Q'_in,AJOC` 已经对齐；P2 `5.7.3.6.1` 的 dry 分支也在同一 `ts` 直接使用
    /// A-JOC 输入，decorrelator 的延迟只属于 wet 分量，不是统一输出延迟。
    ///
    /// P1 `5.7.1` 的全局 6 时隙是另一层的终端 QMF 时间轴问题。公共延迟不会消除
    /// LFE 与 A-SPX 通路之间的 `δ_ASPX` 差值；短帧下该全局时间轴如何实现仍未决，
    /// 但不再决定本分支该补 3 还是 6 个时隙。
    LfeQmf(SignalLocation),
    /// `var_codec_mode` 取 SIMPLE 的全频带信号。
    ///
    /// 核心带 PCM 仍须按 P2 `4.8.3.9` 做 QMF 分析并进入 A-JOC；这里只跳过
    /// A-SPX，因而既不做带宽扩展，也**不该**承受 `5.7.6.5.3` 的
    /// `delta_ASPX`。
    SimpleQmf(SignalLocation),
    /// 单路驱动，走 `aspx::pipeline` 的单声道入口。
    Mono(SignalLocation),
    /// `aspx_balance = 1` 的一对，必须一起交给平衡式入口。
    ///
    /// `Pseudocode 84` 要把两路的 `qscf` 放在一起反量化，拆成两次单路驱动得到
    /// 的标度因子是错的——而两路照样各有输出、各有峰值，落点与统计判据一概
    /// 看不出来。数组按元素内声道号排列，第 0 项是 `ch == 0`。
    Balanced([SignalLocation; 2]),
}

/// 一个 `var_channel_element()` 内的最大已解码声道数。
///
/// 十六路全频带下混信号加一路 LFE。
pub const MAX_SIGNALS: usize = MAX_FULLBAND_DMX_SIGNALS as usize + 1;

/// [`VarChannelElement::aspx_jobs`] 的迭代器。
///
/// 作业在构造时一次算完，`used` 之后的位置是无效后缀，不参与迭代也不参与
/// 比较——故本类型不实现 `PartialEq`。
#[derive(Debug, Clone)]
pub struct AspxJobs {
    jobs: [AspxJob; MAX_SIGNALS],
    used: usize,
    next: usize,
}

impl Iterator for AspxJobs {
    type Item = AspxJob;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.used {
            return None;
        }
        let job = self.jobs.get(self.next).copied()?;
        self.next = self.next.saturating_add(1);
        Some(job)
    }
}

/// [`VarChannelElement::ajoc_input_order`] 的迭代器。
///
/// 顺序在构造时一次算完，`used` 之后的位置是无效后缀，不参与迭代也不参与
/// 比较——故本类型不实现 `PartialEq`。
#[derive(Debug, Clone)]
pub struct AjocInputOrder {
    order: [SignalLocation; MAX_FULLBAND_DMX_SIGNALS as usize],
    used: usize,
    next: usize,
}

impl Iterator for AjocInputOrder {
    type Item = SignalLocation;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.used {
            return None;
        }
        let signal = self.order.get(self.next).copied()?;
        self.next = self.next.saturating_add(1);
        Some(signal)
    }
}

/// [`VarChannelElement::signals`] 的迭代器。
#[derive(Debug, Clone)]
pub struct SignalLocations {
    /// 逐声道数据元素的声道数，按传输顺序。
    widths: [u8; MAX_CHANNEL_ELEMENTS],
    /// `widths` 中有效的前缀长度。
    used: u8,
    has_aspx: bool,
    lfe_first: bool,
    element: u8,
    channel: u8,
    /// 已发出的**全频带**信号数，LFE 不计入——A-SPX 的下标由它给出。
    signal: u8,
}

impl Iterator for SignalLocations {
    type Item = SignalLocation;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.element >= self.used {
                return None;
            }
            let width = self
                .widths
                .get(usize::from(self.element))
                .copied()
                .unwrap_or(0);
            if self.channel >= width {
                self.element = self.element.saturating_add(1);
                self.channel = 0;
                continue;
            }
            let is_lfe = self.lfe_first && self.element == 0;
            let aspx = if self.has_aspx && !is_lfe {
                let signal = self.signal;
                self.signal = self.signal.saturating_add(1);
                Some((signal / 2, signal % 2))
            } else {
                None
            };
            let out = SignalLocation {
                channel_element: self.element,
                channel_in_element: self.channel,
                aspx,
            };
            self.channel = self.channel.saturating_add(1);
            return Some(out);
        }
    }
}

/// 解析 `var_channel_element()`，见 P2 `6.2.4.4`。
///
/// 参数见 [`VarChannelParams`]，工作区见 [`VarChannelWorkspace`]；两组缓冲
/// 按传输顺序依次填充，实际用到的个数见
/// [`VarChannelElement::channel_elements`] 与
/// [`VarChannelElement::aspx_elements`]。
///
/// # Errors
///
/// 见 [`VarElementError`]。
pub fn parse_var_channel_element(
    reader: &mut BitReader<'_>,
    params: VarChannelParams,
    state: &mut VarChannelState,
    workspace: VarChannelWorkspace<'_>,
) -> Result<VarChannelElement, VarElementError> {
    let VarChannelParams {
        context,
        n_dmx_signals,
        b_has_lfe,
        b_iframe,
    } = params;
    let VarChannelWorkspace { elements, aspx } = workspace;
    if n_dmx_signals == 0 || n_dmx_signals > MAX_FULLBAND_DMX_SIGNALS {
        return Err(VarElementError::SignalCountOutOfRange {
            n_dmx_signals,
            limit: MAX_FULLBAND_DMX_SIGNALS,
        });
    }

    let is_odd = n_dmx_signals % 2 == 1;
    let pairs = n_dmx_signals / 2;

    // 在读取任何比特前核对不依赖码流分支的声道工作区。
    let needed_channels = channel_element_count(n_dmx_signals, b_has_lfe);
    if elements.len() < needed_channels {
        return Err(VarElementError::ChannelWorkspaceTooSmall {
            needed: needed_channels,
            provided: elements.len(),
        });
    }
    let needed_aspx = usize::from(pairs).saturating_add(usize::from(is_odd));

    // var_codec_mode 是本元素第一位。先在读取器副本上探测，只有实际进入
    // A-SPX 分支才要求相应工作区；容量不足时原读取器仍停在元素开头。
    let mut next_reader = (*reader).clone();
    let codec_mode_aspx = next_reader.read_flag()?;
    if codec_mode_aspx && aspx.len() < needed_aspx {
        return Err(VarElementError::AspxWorkspaceTooSmall {
            needed: needed_aspx,
            provided: aspx.len(),
        });
    }
    *reader = next_reader;

    // 配置与逐 A-SPX 元素状态组成同一份跨帧状态。所有解析都先写入副本，
    // 仅在整个 var_channel_element 成功后提交，避免失败帧留下混合状态。
    let mut next_state = *state;

    let mut companding = None;
    if codec_mode_aspx {
        if b_iframe {
            next_state.config = Some(AspxConfig::parse(reader)?);
        }
        if n_dmx_signals <= 5 {
            companding = Some(CompandingControl::parse(reader, n_dmx_signals)?);
        }
    }
    // A-SPX 数据元素在本元素末尾才出现，但配置的缺失要尽早暴露。
    let config = if codec_mode_aspx {
        Some(
            next_state
                .config
                .ok_or(VarElementError::MissingAspxConfig)?,
        )
    } else {
        None
    };

    let mut used = 0usize;
    if b_has_lfe {
        let Some(slot) = elements.get_mut(used) else {
            return Err(VarElementError::ChannelWorkspaceTooSmall {
                needed: needed_channels,
                provided: elements.len(),
            });
        };
        slot.parse_mono_data(reader, context, true)?;
        used = used.saturating_add(1);
    }

    let mut coding_config = None;
    if is_odd {
        if n_dmx_signals == 1 {
            used = parse_into(elements, used, needed_channels, |element| {
                element.parse_mono_data(reader, context, false)
            })?;
        } else {
            for _ in 0..pairs.saturating_sub(1) {
                used = parse_into(elements, used, needed_channels, |element| {
                    element.parse_two_channel_data(reader, context)
                })?;
            }
            let three = reader.read_flag()?;
            coding_config = Some(three);
            if three {
                used = parse_into(elements, used, needed_channels, |element| {
                    element.parse_three_channel_data(reader, context)
                })?;
            } else {
                used = parse_into(elements, used, needed_channels, |element| {
                    element.parse_two_channel_data(reader, context)
                })?;
                used = parse_into(elements, used, needed_channels, |element| {
                    element.parse_mono_data(reader, context, false)
                })?;
            }
        }
    } else {
        for _ in 0..pairs {
            used = parse_into(elements, used, needed_channels, |element| {
                element.parse_two_channel_data(reader, context)
            })?;
        }
    }

    let mut aspx_used = 0usize;
    let mut balanced_aspx = 0u8;
    if let Some(config) = config {
        for _ in 0..pairs {
            let (Some(slot), Some(element_state)) =
                (aspx.get_mut(aspx_used), next_state.aspx.get_mut(aspx_used))
            else {
                return Err(VarElementError::AspxWorkspaceTooSmall {
                    needed: needed_aspx,
                    provided: aspx.len(),
                });
            };
            *slot = AspxData::parse_2ch(
                reader,
                &config,
                element_state,
                context.frame_len_base,
                b_iframe,
            )?;
            if slot.balance == Some(true) {
                balanced_aspx |= 1u8 << aspx_used;
            }
            aspx_used = aspx_used.saturating_add(1);
        }
        if is_odd {
            let (Some(slot), Some(element_state)) =
                (aspx.get_mut(aspx_used), next_state.aspx.get_mut(aspx_used))
            else {
                return Err(VarElementError::AspxWorkspaceTooSmall {
                    needed: needed_aspx,
                    provided: aspx.len(),
                });
            };
            *slot = AspxData::parse_1ch(
                reader,
                &config,
                element_state,
                context.frame_len_base,
                b_iframe,
            )?;
            aspx_used = aspx_used.saturating_add(1);
        }
    }

    let active_aspx = aspx
        .get(..aspx_used)
        .ok_or(VarElementError::AspxWorkspaceTooSmall {
            needed: aspx_used,
            provided: aspx.len(),
        })?;
    let out = VarChannelElement {
        codec_mode_aspx,
        companding,
        coding_config,
        n_dmx_signals,
        b_has_lfe,
        channel_elements: u8::try_from(used).unwrap_or(u8::MAX),
        aspx_elements: u8::try_from(aspx_used).unwrap_or(u8::MAX),
        balanced_aspx,
        aspx_reach: collect_aspx_reach(active_aspx),
    };
    *state = next_state;
    Ok(out)
}

/// 取出第 `index` 个工作区并对其执行 `body`，返回推进后的下标。
fn parse_into(
    elements: &mut [ChannelElement],
    index: usize,
    needed: usize,
    body: impl FnOnce(&mut ChannelElement) -> Result<(), ChannelError>,
) -> Result<usize, VarElementError> {
    let provided = elements.len();
    let Some(element) = elements.get_mut(index) else {
        return Err(VarElementError::ChannelWorkspaceTooSmall { needed, provided });
    };
    body(element)?;
    Ok(index.saturating_add(1))
}

/// `6.2.4.4` 的分支下，该配置需要多少个声道数据元素。
fn channel_element_count(n_dmx_signals: u8, b_has_lfe: bool) -> usize {
    let lfe = usize::from(b_has_lfe);
    let pairs = usize::from(n_dmx_signals / 2);
    if n_dmx_signals.is_multiple_of(2) {
        return lfe.saturating_add(pairs);
    }
    if n_dmx_signals == 1 {
        return lfe.saturating_add(1);
    }
    // 奇数分支：pairs-1 个双声道，再加两个（双声道加单声道）或一个（三声道）。
    lfe.saturating_add(pairs.saturating_sub(1))
        .saturating_add(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aspx::bands::AspxBandTables;
    use crate::aspx::codebooks::table_for;
    use crate::aspx::tables::{EnvelopeKind, HcbType, StereoMode, get_aspx_hcb};
    use crate::huffman::tables::ALL_CODEBOOKS;

    extern crate std;
    use std::format;
    use std::vec;
    use std::vec::Vec;

    const CONTEXT: ChannelContext = ChannelContext {
        frame_len_base: 2048,
        sampling_frequency_hz: 48_000,
    };

    struct BitBuf {
        bytes: [u8; 4096],
        len: usize,
    }

    impl BitBuf {
        const fn new() -> Self {
            Self {
                bytes: [0; 4096],
                len: 0,
            }
        }

        fn push(&mut self, bit: bool) {
            let index = self.len / 8;
            let shift = 7usize.saturating_sub(self.len % 8);
            if bit && let Some(slot) = self.bytes.get_mut(index) {
                *slot |= 1u8
                    .checked_shl(u32::try_from(shift).unwrap_or(0))
                    .unwrap_or(0);
            }
            self.len = self.len.saturating_add(1);
        }

        fn push_bits(&mut self, value: u32, width: u32) {
            for bit in (0..width).rev() {
                self.push((value >> bit) & 1 == 1);
            }
        }

        /// 恰好覆盖已写入比特的切片。
        ///
        /// 用整个数组会让越界读取悄悄读到填充的零；取精确长度才能把「读多
        /// 了」暴露成 `ReadError`。
        fn as_slice(&self) -> &[u8] {
            let bytes = self.len.div_ceil(8);
            self.bytes.get(..bytes).unwrap_or(&self.bytes)
        }

        /// 15 位 `aspx_config()`：高分辨率模板、起止皆 0、`freq_res_mode` 取 3。
        fn push_aspx_config(&mut self) {
            self.push_aspx_config_with_quant(false);
        }

        /// 与 [`Self::push_aspx_config`] 相同，但可指定包络量化模式。
        fn push_aspx_config_with_quant(&mut self, quant_mode_env: bool) {
            self.push(quant_mode_env);
            self.push_bits(0, 3); // start_freq
            self.push_bits(0, 2); // stop_freq
            self.push(true); // master_freq_scale
            self.push(false); // interpolation
            self.push(false); // preflat
            self.push(false); // limiter
            self.push_bits(1, 2); // noise_sbg
            self.push(false); // num_env_bits_fixfix
            self.push_bits(3, 2); // freq_res_mode = 恒高分辨率
        }

        /// `sf_info()`：长帧、单组，`max_sfb` 由参数给出。
        ///
        /// 不含 `spec_frontend`——那一位属于 `mono_data(0)`，由调用方单独写。
        fn push_long_sf_info(&mut self, max_sfb: u32) {
            self.push(true); // b_long_frame
            self.push_bits(max_sfb, 6); // n_msfb_bits(2048) = 6
        }

        /// 一个最简 `sf_data(ASF)`：单个无码本区段，无标度因子与噪声填充。
        fn push_empty_sf_data(&mut self, max_sfb: u32) {
            self.push_bits(0, 4); // sect_cb = 0
            self.push_bits(max_sfb.saturating_sub(1), 5); // sect_len
            self.push_bits(0, 8); // reference_scale_factor
            self.push(false); // b_snf_data_exists
        }

        /// `mono_data(0)`：一位 `spec_frontend` 选中 ASF，随后是成帧与谱数据。
        fn push_mono_data(&mut self, max_sfb: u32) {
            self.push(false); // spec_frontend = ASF
            self.push_long_sf_info(max_sfb);
            self.push_empty_sf_data(max_sfb);
        }

        /// `three_channel_data()`：一份 `sf_info()`、4 位矩阵码、两份
        /// `chparam_info()`（`max_sfb` 为 0 故无频带数据），再三份 `sf_data()`。
        fn push_three_channel_data(&mut self, max_sfb: u32) {
            self.push_long_sf_info(max_sfb);
            self.push_bits(0, 4); // chel_matsel
            self.push_bits(0, 2); // chparam_info[0] 的 sap_mode
            self.push_bits(0, 2); // chparam_info[1] 的 sap_mode
            for _ in 0..3 {
                self.push_empty_sf_data(max_sfb);
            }
        }

        /// `two_channel_data()`：两份独立 `sf_info()` 加两份 `sf_data()`。
        fn push_two_channel_data(&mut self, max_sfb: u32) {
            self.push(false); // b_enable_mdct_stereo_proc = 0
            self.push_long_sf_info(max_sfb);
            self.push_long_sf_info(max_sfb);
            self.push_empty_sf_data(max_sfb);
            self.push_empty_sf_data(max_sfb);
        }

        fn push_symbol(&mut self, table: &crate::huffman::HuffmanTable, symbol: u16) {
            for &(_, candidate, lengths, codewords) in ALL_CODEBOOKS {
                if !core::ptr::eq(candidate, table) {
                    continue;
                }
                let index = usize::from(symbol);
                let (Some(&width), Some(&codeword)) = (lengths.get(index), codewords.get(index))
                else {
                    panic!("码本没有第 {symbol} 个符号");
                };
                self.push_bits(codeword, u32::from(width));
                return;
            }
            panic!("码本不在 ALL_CODEBOOKS 内");
        }

        /// 一个 FIXFIX 单包络的 `aspx_data_1ch()` 或 `aspx_data_2ch()`。
        fn push_aspx_data(&mut self, two_channels: bool) {
            self.push_aspx_data_with(two_channels, 0, true);
        }

        /// 同上，但可指定 `aspx_xover_subband_offset` 与是否为 I 帧。
        ///
        /// 非 I 帧不传输交叉偏移，解析器须沿用该元素**自己**上一帧的值；
        /// 偏移不同则 `num_sbg_sig_highres` 不同，符号数随之改变。
        fn push_aspx_data_with(&mut self, two_channels: bool, xover: u8, b_iframe: bool) {
            self.push_aspx_data_full(two_channels, xover, b_iframe, true);
        }

        /// 同上，但可指定 `aspx_balance`。
        ///
        /// 为假时两路各自成帧、各带一份 `tna_mode`，且两路都用
        /// [`StereoMode::Level`] 的码本；为真时共用一份成帧与 `tna_mode`，第二
        /// 路改用 [`StereoMode::Balance`]。
        fn push_aspx_data_full(
            &mut self,
            two_channels: bool,
            xover: u8,
            b_iframe: bool,
            balance: bool,
        ) {
            let Ok(bands) = AspxBandTables::derive(true, 0, 0, 1, xover) else {
                panic!("频带表应可推导");
            };
            let highres = bands.num_sbg_sig_highres();
            let noise_sbg = bands.num_sbg_noise();
            let balance = two_channels && balance;

            if b_iframe {
                self.push_bits(u32::from(xover), 3);
            }
            self.push(false); // 声道 0：FIXFIX
            self.push_bits(0, 1); // 1 个包络
            if two_channels {
                self.push(balance);
                if !balance {
                    self.push(false); // 声道 1：FIXFIX
                    self.push_bits(0, 1); // 1 个包络
                }
            }
            let channels = if two_channels { 2 } else { 1 };
            for _ in 0..channels {
                self.push(false); // sig_delta_dir[0]
                self.push(false); // noise_delta_dir[0]
            }
            // `aspx_balance` 为真时两路共用一份 `tna_mode`，为假时各带一份。
            let chirp_sets = if balance { 1 } else { channels };
            for _ in 0..usize::from(noise_sbg).saturating_mul(chirp_sets) {
                self.push_bits(0, 2); // tna_mode
            }
            if two_channels {
                self.push(false); // ah_left
                self.push(false); // ah_right
            } else {
                self.push(false); // ah_present
            }
            self.push(false); // fic_present
            self.push(false); // tic_present

            let stereos: &[StereoMode] = match (two_channels, balance) {
                (true, true) => &[StereoMode::Level, StereoMode::Balance],
                (true, false) => &[StereoMode::Level, StereoMode::Level],
                (false, _) => &[StereoMode::Level],
            };
            for &stereo in stereos {
                let f0 = get_aspx_hcb(EnvelopeKind::Signal, stereo, false, HcbType::F0);
                let df = get_aspx_hcb(EnvelopeKind::Signal, stereo, false, HcbType::Df);
                self.push_symbol(table_for(f0), 0);
                for _ in 1..highres {
                    self.push_symbol(table_for(df), 0);
                }
            }
            for &stereo in stereos {
                let f0 = get_aspx_hcb(EnvelopeKind::Noise, stereo, false, HcbType::F0);
                let df = get_aspx_hcb(EnvelopeKind::Noise, stereo, false, HcbType::Df);
                self.push_symbol(table_for(f0), 0);
                for _ in 1..noise_sbg {
                    self.push_symbol(table_for(df), 0);
                }
            }
        }
    }

    fn workspaces() -> ([ChannelElement; 3], [AspxData; 3]) {
        (
            [
                ChannelElement::new(),
                ChannelElement::new(),
                ChannelElement::new(),
            ],
            [AspxData::empty(), AspxData::empty(), AspxData::empty()],
        )
    }

    /// 一次实际解析的结果：形态摘要，加两段各自逐元素的真实声道数。
    ///
    /// 两组宽度取自解析器填好的工作区，不是由参数重算的——[`SignalLocation`]
    /// 要对照的就是它们。
    struct Shape {
        element: VarChannelElement,
        channel_widths: Vec<u8>,
        aspx_widths: Vec<u8>,
        aspx: Vec<AspxData>,
    }

    /// 本次形态的四个自由度，见 [`every_shape`]。
    #[derive(Debug, Clone, Copy)]
    struct Axes {
        n_dmx: u8,
        lfe: bool,
        three: bool,
        balance: bool,
    }

    /// 构造并解析一个完整的 `var_channel_element()`，落点必须与构造长度相等。
    ///
    /// `three` 只在奇数分支且信号数大于 1 时被写进码流，`balance` 只对双声道
    /// A-SPX 元素有意义，其余形态下这两个参数无效。
    fn parse_shape_at(axes: Axes) -> Shape {
        let balances = [axes.balance; MAX_ASPX_ELEMENTS];
        parse_shape_with_balances(axes, &balances)
    }

    /// 同上，但逐双声道 A-SPX 元素指定各自的 `aspx_balance`。
    fn parse_shape_with_balances(axes: Axes, balances: &[bool]) -> Shape {
        let Axes {
            n_dmx,
            lfe,
            three,
            balance: _,
        } = axes;
        let mut buf = BitBuf::new();
        buf.push(true); // var_codec_mode = A-SPX
        buf.push_aspx_config();
        if n_dmx <= 5 {
            if n_dmx > 1 {
                buf.push(true); // companding sync_flag
            }
            buf.push(true); // b_compand_on[0]，全开故不传 b_compand_avg
        }
        if lfe {
            buf.push_bits(2, 3); // sf_info_lfe() 的 max_sfb，n_msfbl_bits = 3
            buf.push_empty_sf_data(2);
        }
        let is_odd = n_dmx % 2 == 1;
        let pairs = n_dmx / 2;
        if is_odd && n_dmx == 1 {
            buf.push_mono_data(2);
        } else if is_odd {
            for _ in 0..pairs.saturating_sub(1) {
                buf.push_two_channel_data(2);
            }
            buf.push(three); // var_coding_config
            if three {
                buf.push_three_channel_data(2);
            } else {
                buf.push_two_channel_data(2);
                buf.push_mono_data(2);
            }
        } else {
            for _ in 0..pairs {
                buf.push_two_channel_data(2);
            }
        }
        for pair in 0..pairs {
            let balance = balances
                .get(usize::from(pair))
                .copied()
                .expect("每个双声道 A-SPX 元素都须指定 balance");
            buf.push_aspx_data_full(true, 0, true, balance);
        }
        if is_odd {
            buf.push_aspx_data_full(false, 0, true, false);
        }
        let expected = buf.len;

        let mut elements = Vec::new();
        elements.resize_with(MAX_CHANNEL_ELEMENTS, ChannelElement::new);
        let mut aspx = vec![AspxData::empty(); MAX_ASPX_ELEMENTS];
        let mut state = VarChannelState::new();
        let mut reader = BitReader::new(buf.as_slice());
        let element = parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: n_dmx,
                b_has_lfe: lfe,
                b_iframe: true,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        )
        .unwrap_or_else(|error| panic!("{axes:?} 应能解析：{error:?}"));
        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "{axes:?} 的落点"
        );

        let aspx_used = usize::from(element.aspx_elements());
        // 夹具前提：双声道元素的 `aspx_balance` 必须真的取到了要求的那一档，
        // 否则「平衡式一对不许拆」的判据两侧跑的是同一条码流。
        for (index, data) in aspx.iter().take(aspx_used).enumerate() {
            let expected = (data.channels == 2).then(|| {
                balances
                    .get(index)
                    .copied()
                    .expect("每个双声道 A-SPX 元素都须指定 balance")
            });
            assert_eq!(data.balance, expected, "{axes:?} 的 aspx_balance");
        }
        assert_eq!(
            element.aspx_reach(),
            collect_aspx_reach(
                aspx.get(..aspx_used)
                    .expect("已解析 A-SPX 数不得超过工作区"),
            ),
            "解析摘要必须固化实际使用的 A-SPX 工作区"
        );

        Shape {
            channel_widths: elements
                .iter()
                .take(usize::from(element.channel_elements()))
                .map(ChannelElement::channels)
                .collect(),
            aspx_widths: aspx
                .iter()
                .take(aspx_used)
                .map(|data| data.channels)
                .collect(),
            aspx: aspx.into_iter().take(aspx_used).collect(),
            element,
        }
    }

    /// 全部合法形态：信号数 1…16 × 有无 LFE × 奇数分支的两种编码配置 ×
    /// 双声道元素的两档 `aspx_balance`。
    fn every_shape() -> impl Iterator<Item = Axes> {
        (1..=MAX_FULLBAND_DMX_SIGNALS).flat_map(|n_dmx| {
            [false, true].into_iter().flat_map(move |lfe| {
                // `var_coding_config` 只在奇数且大于 1 时进入码流；`aspx_balance`
                // 只在有双声道 A-SPX 元素时进入。取不到的那一维跑两遍只是同一
                // 条码流跑两次，白费而已。
                let threes: &[bool] = if n_dmx % 2 == 1 && n_dmx > 1 {
                    &[false, true]
                } else {
                    &[false]
                };
                let balances: &[bool] = if n_dmx >= 2 { &[false, true] } else { &[false] };
                threes.iter().flat_map(move |three| {
                    balances.iter().map(move |balance| Axes {
                        n_dmx,
                        lfe,
                        three: *three,
                        balance: *balance,
                    })
                })
            })
        })
    }

    /// 信号映射必须把两段传输各自铺满，一路不漏、一路不重。
    ///
    /// 判据是**双射**，不是逐条抄一遍：期望序列由解析器填好的工作区生成
    /// （逐元素的真实声道数），映射给出的下标序列必须与之逐项相等。任一段的
    /// 分组写错都会在这里错位。
    #[test]
    fn the_signal_map_covers_both_transmissions_exactly_once() {
        for axes in every_shape() {
            let shape = parse_shape_at(axes);
            let label = format!("{axes:?}");
            let located: Vec<_> = shape.element.signals().collect();

            assert_eq!(
                located.len(),
                usize::from(axes.n_dmx) + usize::from(axes.lfe),
                "{label}：条目数应为信号数加 LFE"
            );

            let expected_channels: Vec<(u8, u8)> = shape
                .channel_widths
                .iter()
                .enumerate()
                .flat_map(|(element, width)| {
                    (0..*width)
                        .map(move |channel| (u8::try_from(element).unwrap_or(u8::MAX), channel))
                })
                .collect();
            let got_channels: Vec<(u8, u8)> = located
                .iter()
                .map(|slot| (slot.channel_element, slot.channel_in_element))
                .collect();
            assert_eq!(got_channels, expected_channels, "{label}：声道侧");

            let expected_aspx: Vec<(u8, u8)> = shape
                .aspx_widths
                .iter()
                .enumerate()
                .flat_map(|(element, width)| {
                    (0..*width)
                        .map(move |channel| (u8::try_from(element).unwrap_or(u8::MAX), channel))
                })
                .collect();
            let got_aspx: Vec<(u8, u8)> = located.iter().filter_map(|slot| slot.aspx).collect();
            assert_eq!(got_aspx, expected_aspx, "{label}：A-SPX 侧");
        }
    }

    /// LFE 占一个声道数据元素，但**不占** A-SPX 的下标。
    ///
    /// 这是这张表最容易错的一处：A-SPX 只覆盖 `n_fullband_dmx_signals` 路，
    /// 把 LFE 也算进去会让其余每一路都串一格。串位后的音频照样像回事，没有
    /// 任何落点或统计判据会响。
    #[test]
    fn the_lfe_takes_a_channel_element_but_no_aspx_slot() {
        for axes in every_shape().filter(|axes| !axes.lfe) {
            let label = format!("{axes:?}");
            let without: Vec<_> = parse_shape_at(axes).element.signals().collect();
            let with: Vec<_> = parse_shape_at(Axes { lfe: true, ..axes })
                .element
                .signals()
                .collect();

            let (head, rest) = with.split_first().expect("带 LFE 时至少有一条");
            assert_eq!(head.channel_element, 0, "{label}：LFE 应是第 0 个声道元素");
            assert_eq!(head.channel_in_element, 0, "{label}：LFE 元素只有一路");
            assert_eq!(head.aspx, None, "{label}：LFE 没有 A-SPX 参数");

            let bare: Vec<_> = without.iter().map(|slot| slot.aspx).collect();
            let shifted: Vec<_> = rest.iter().map(|slot| slot.aspx).collect();
            assert_eq!(shifted, bare, "{label}：A-SPX 侧不得因 LFE 而移位");

            for (plain, moved) in without.iter().zip(rest) {
                assert_eq!(
                    moved.channel_element,
                    plain.channel_element.saturating_add(1),
                    "{label}：声道侧应恰好后移一个元素"
                );
                assert_eq!(
                    moved.channel_in_element, plain.channel_in_element,
                    "{label}：元素内的声道号不该变"
                );
            }
        }
    }

    /// `var_coding_config` 只改声道侧的分组，A-SPX 侧一位不动。
    ///
    /// 奇数分支上末三路走一个 `three_channel_data()` 还是
    /// `two_channel_data()` 加 `mono_data(0)`，是声道侧的事；A-SPX 侧恒是一个
    /// `aspx_data_2ch()` 加一个 `aspx_data_1ch()`。两侧若被写成按下标对应，
    /// 这条判据会响。
    #[test]
    fn the_three_channel_branch_moves_only_the_channel_side() {
        for axes in
            every_shape().filter(|axes| axes.n_dmx % 2 == 1 && axes.n_dmx > 1 && !axes.three)
        {
            let label = format!("{axes:?}");
            let split: Vec<_> = parse_shape_at(axes).element.signals().collect();
            let merged: Vec<_> = parse_shape_at(Axes {
                three: true,
                ..axes
            })
            .element
            .signals()
            .collect();

            let split_aspx: Vec<_> = split.iter().map(|slot| slot.aspx).collect();
            let merged_aspx: Vec<_> = merged.iter().map(|slot| slot.aspx).collect();
            assert_eq!(merged_aspx, split_aspx, "{label}：A-SPX 侧必须一致");

            let split_channels: Vec<_> = split
                .iter()
                .map(|slot| (slot.channel_element, slot.channel_in_element))
                .collect();
            let merged_channels: Vec<_> = merged
                .iter()
                .map(|slot| (slot.channel_element, slot.channel_in_element))
                .collect();
            assert_ne!(
                merged_channels, split_channels,
                "{label}：三声道分支必须改变声道侧的分组，否则本判据什么也没测"
            );
        }
    }

    /// 逐条展平后的作业必须与信号映射逐项相等：一路不漏、一路不重。
    ///
    /// 平衡式把两路并成一条作业，因此「条数」不再等于声道数，只有展平后才能
    /// 比。这条同时挡住「并对时吞掉一路」与「把不该并的并了」。
    #[test]
    fn the_jobs_cover_every_channel_exactly_once() {
        for axes in every_shape() {
            let shape = parse_shape_at(axes);
            let label = format!("{axes:?}");
            let jobs: Vec<_> = shape.element.aspx_jobs().collect();

            let flattened: Vec<SignalLocation> = jobs
                .iter()
                .flat_map(|job| match *job {
                    AspxJob::LfeQmf(slot) | AspxJob::SimpleQmf(slot) | AspxJob::Mono(slot) => {
                        vec![slot]
                    }
                    AspxJob::Balanced(pair) => pair.to_vec(),
                })
                .collect();
            let located: Vec<SignalLocation> = shape.element.signals().collect();
            assert_eq!(flattened, located, "{label}：作业展平后应等于信号映射");
        }
    }

    /// `aspx_balance` 决定并不并对，而并对与否只由它决定。
    ///
    /// 这是本层唯一会静默出错的判断：`Pseudocode 84` 要求两路 `qscf` 一起反
    /// 量化，拆开算出的标度因子是错的，但两路照样各有输出、各有峰值，落点与
    /// 统计判据一概看不出来。判据两侧的码流真的不同——`parse_shape_at` 已把
    /// 「双声道元素的 `aspx_balance` 取到要求的那一档」钉成前置断言。
    #[test]
    fn a_balanced_pair_is_never_split_and_an_unbalanced_one_is_never_merged() {
        for axes in every_shape().filter(|axes| axes.n_dmx >= 2) {
            let shape = parse_shape_at(axes);
            let label = format!("{axes:?}");
            let jobs: Vec<_> = shape.element.aspx_jobs().collect();

            // 逐 A-SPX 元素统计它被排成了几条作业、哪一种。
            let mut seen: Vec<Vec<AspxJob>> = vec![Vec::new(); shape.aspx_widths.len()];
            for job in &jobs {
                let element = match *job {
                    AspxJob::LfeQmf(_) | AspxJob::SimpleQmf(_) => continue,
                    AspxJob::Mono(slot) => slot.aspx.map(|(element, _)| element),
                    AspxJob::Balanced([first, _]) => first.aspx.map(|(element, _)| element),
                };
                let Some(element) = element else { continue };
                if let Some(slot) = seen.get_mut(usize::from(element)) {
                    slot.push(*job);
                }
            }

            for (element, width) in shape.aspx_widths.iter().enumerate() {
                let scheduled = seen.get(element).map(Vec::as_slice).unwrap_or(&[]);
                match (*width, axes.balance) {
                    (2, true) => {
                        assert!(
                            matches!(scheduled, [AspxJob::Balanced(_)]),
                            "{label}：第 {element} 个 A-SPX 元素为平衡式，必须并成一条作业，实得 {scheduled:?}"
                        );
                    }
                    (2, false) => {
                        assert!(
                            matches!(scheduled, [AspxJob::Mono(_), AspxJob::Mono(_)]),
                            "{label}：第 {element} 个 A-SPX 元素非平衡式，必须拆成两条单路作业，实得 {scheduled:?}"
                        );
                    }
                    _ => {
                        assert!(
                            matches!(scheduled, [AspxJob::Mono(_)]),
                            "{label}：第 {element} 个 A-SPX 元素只有一路，必须是单路作业，实得 {scheduled:?}"
                        );
                    }
                }
            }
        }
    }

    /// A-SPX 激活时，只有 LFE 被单独留出，其余各路一律要经 A-SPX。
    ///
    /// Simple 模式的那一半在 [`simple_mode_maps_every_channel_but_no_aspx_slot`]
    /// 里——`every_shape` 全是 A-SPX 流，在这里写它等于没测。
    #[test]
    fn the_lfe_is_the_only_separate_job_while_aspx_is_active() {
        for axes in every_shape() {
            let shape = parse_shape_at(axes);
            let label = format!("{axes:?}");
            let jobs: Vec<_> = shape.element.aspx_jobs().collect();

            let lfe: Vec<_> = jobs
                .iter()
                .filter_map(|job| match *job {
                    AspxJob::LfeQmf(slot) => Some(slot),
                    _ => None,
                })
                .collect();
            assert!(
                jobs.iter().all(|job| !matches!(job, AspxJob::SimpleQmf(_))),
                "{label}：A-SPX 激活时不得排出 SIMPLE 的 QMF-only 作业"
            );
            if axes.lfe {
                assert_eq!(lfe.len(), 1, "{label}：只有 LFE 该被单独留出");
                let head = lfe.first().expect("刚断言过恰有一条");
                assert_eq!(
                    (head.channel_element, head.channel_in_element, head.aspx),
                    (0, 0, None),
                    "{label}：单独留出的必须是第 0 个声道元素的 LFE"
                );
                assert!(
                    matches!(jobs.first(), Some(AspxJob::LfeQmf(_))),
                    "{label}：LFE 排在最前"
                );
            } else {
                assert!(lfe.is_empty(), "{label}：无 LFE 时不该有 LFE 作业");
            }
        }
    }

    /// A-JOC 输入顺序必须是全频带信号的一个置换，且 LFE 不在其中。
    ///
    /// 结构性判据：条数恒为 `n_fullband_dmx_signals`，每一路恰好出现一次。
    /// 它挡住「丢一路」「重一路」「把 LFE 也送进去」，但挡不住整体顺序错——
    /// 那由下一条按伪码逐项列出的期望表挡。
    #[test]
    fn the_ajoc_input_is_a_permutation_of_the_fullband_signals() {
        for axes in every_shape() {
            let shape = parse_shape_at(axes);
            let label = format!("{axes:?}");
            let order: Vec<_> = shape.element.ajoc_input_order().collect();
            assert_eq!(
                order.len(),
                usize::from(axes.n_dmx),
                "{label}：A-JOC 输入条数应为全频带信号数"
            );

            let mut sorted = order.clone();
            sorted.sort_by_key(|slot| (slot.channel_element, slot.channel_in_element));
            // LFE 恒在第 0 位，跳掉它剩下的就是全频带那几路。
            let mut fullband: Vec<_> = shape
                .element
                .signals()
                .skip(usize::from(axes.lfe))
                .collect();
            fullband.sort_by_key(|slot| (slot.channel_element, slot.channel_in_element));
            assert_eq!(sorted, fullband, "{label}：应恰好是全频带那几路的置换");
        }
    }

    /// 逐项核对 `Pseudocode 14a`：最后 2 或 3 路领头，其余顺次跟上。
    ///
    /// **期望表是照伪码手推的，不引用实现里的 `n_offset` 算式**——引用它等于
    /// 拿实现验实现。以 `n_dmx = 9`、带 LFE 为例：`m' = 10`、`n_offset = 3`，
    /// 前三项取 `Q'in[7..10]`（即信号 6、7、8），其余取 `Q'in[1..7]`（信号
    /// 0…5）。三路及以下不重排。
    #[test]
    fn the_ajoc_input_follows_pseudocode_14a() {
        // (信号数, 有无 LFE, 期望的全频带信号序号)
        let expected: [(u8, bool, &[u8]); 8] = [
            (1, false, &[0]),
            (1, true, &[0]),
            (3, false, &[0, 1, 2]),
            (3, true, &[0, 1, 2]),
            (4, false, &[2, 3, 0, 1]),
            (5, false, &[2, 3, 4, 0, 1]),
            (6, true, &[4, 5, 0, 1, 2, 3]),
            (9, true, &[6, 7, 8, 0, 1, 2, 3, 4, 5]),
        ];
        for (n_dmx, lfe, want) in expected {
            let axes = Axes {
                n_dmx,
                lfe,
                three: false,
                balance: false,
            };
            let shape = parse_shape_at(axes);
            let label = format!("{axes:?}");

            // 传输顺序里的全频带信号，LFE 已被去掉。
            let fullband: Vec<_> = shape.element.signals().skip(usize::from(lfe)).collect();
            assert_eq!(
                fullband.len(),
                usize::from(n_dmx),
                "{label}：夹具前提——去掉 LFE 后应剩下 n_dmx 路"
            );

            let got: Vec<_> = shape.element.ajoc_input_order().collect();
            let expected: Vec<_> = want
                .iter()
                .map(|index| {
                    *fullband
                        .get(usize::from(*index))
                        .expect("期望表的下标应在范围内")
                })
                .collect();
            assert_eq!(got, expected, "{label}：A-JOC 输入顺序");
        }
    }

    /// LFE 只是被剔除，不改变全频带各路之间的相对顺序。
    ///
    /// `Pseudocode 14a` 的两条 `b_has_lfe` 分支只差一个 `+ 1` 的读取偏移；若把
    /// 那个偏移漏掉或多加，带 LFE 的流会整体串一格。判据拿同一个 `n_dmx` 的
    /// 有无 LFE 两份比较**相对**顺序，因此对偏移敏感而对绝对下标不敏感。
    #[test]
    fn the_lfe_only_drops_out_of_the_ajoc_input() {
        for axes in every_shape().filter(|axes| !axes.lfe) {
            let label = format!("{axes:?}");
            let bare = parse_shape_at(axes);
            let with = parse_shape_at(Axes { lfe: true, ..axes });

            let rank = |shape: &Shape| -> Vec<usize> {
                let fullband: Vec<_> = shape
                    .element
                    .signals()
                    .skip(usize::from(shape.element.b_has_lfe))
                    .collect();
                shape
                    .element
                    .ajoc_input_order()
                    .map(|slot| {
                        fullband
                            .iter()
                            .position(|candidate| *candidate == slot)
                            .expect("A-JOC 输入必须来自全频带那几路")
                    })
                    .collect()
            };
            assert_eq!(rank(&bare), rank(&with), "{label}：LFE 不该改变相对顺序");

            let lfe_slot = with.element.signals().next().expect("带 LFE 时至少有一路");
            assert!(
                with.element.ajoc_input_order().all(|slot| slot != lfe_slot),
                "{label}：LFE 不得出现在 A-JOC 输入里"
            );
        }
    }

    /// 驱动决策必须绑定成功解析时的摘要，不能被之后复用的工作区改写。
    ///
    /// 两份结果的 A-SPX 形状完全相同，只有 `aspx_balance` 相反；若
    /// `aspx_jobs()` 仍从调用方传入的切片读标志，换成另一帧的工作区后会静默把
    /// 一条 `Balanced` 改成两条 `Mono`。现在方法不再接收工作区，且即使原工作区
    /// 的公开字段随后被改写，已解析摘要也必须保持原决策。
    #[test]
    fn balance_decision_is_bound_to_the_parse_summary() {
        let axes = Axes {
            n_dmx: 5,
            lfe: true,
            three: false,
            balance: true,
        };
        let mut balanced = parse_shape_at(axes);
        let plain = parse_shape_at(Axes {
            balance: false,
            ..axes
        });
        assert_eq!(
            balanced.aspx_widths, plain.aspx_widths,
            "夹具前提：两份解析结果形状相同"
        );

        let expected: Vec<_> = balanced.element.aspx_jobs().collect();
        let other: Vec<_> = plain.element.aspx_jobs().collect();
        assert_ne!(expected, other, "夹具前提：两档 balance 的作业确实不同");

        for data in &mut balanced.aspx {
            data.balance = data.balance.map(|value| !value);
        }
        assert_eq!(
            balanced.element.aspx_jobs().collect::<Vec<_>>(),
            expected,
            "工作区复用或改写不得反向改变已解析摘要的作业"
        );
    }

    /// balance 位逐 A-SPX 元素固化，不能把「任一元素为真」误记成整帧全真。
    #[test]
    fn each_aspx_element_keeps_its_own_balance_decision() {
        let shape = parse_shape_with_balances(
            Axes {
                n_dmx: 6,
                lfe: false,
                three: false,
                balance: false,
            },
            &[true, false, true],
        );
        let signals: Vec<_> = shape.element.signals().collect();
        let [first_a, first_b, second_a, second_b, third_a, third_b] = signals.as_slice() else {
            panic!("夹具前提：六路信号应逐项落下")
        };
        assert_eq!(
            shape.element.aspx_jobs().collect::<Vec<_>>(),
            vec![
                AspxJob::Balanced([*first_a, *first_b]),
                AspxJob::Mono(*second_a),
                AspxJob::Mono(*second_b),
                AspxJob::Balanced([*third_a, *third_b]),
            ],
            "三个 A-SPX 元素应各自遵守自己的 balance 位"
        );
    }

    /// Simple 模式没有 A-SPX 元素：映射的 A-SPX 侧整列为空。
    ///
    /// 全部信号都须做 QMF 分析；LFE 不进入 A-JOC、留待在 QMF 域重新插入，其余
    /// 全频带信号进入 A-JOC。SIMPLE 只是不做带宽扩展，也**不该**承受
    /// `delta_ASPX`。
    #[test]
    fn simple_mode_maps_every_channel_but_no_aspx_slot() {
        let mut buf = BitBuf::new();
        buf.push(false); // var_codec_mode = Simple
        buf.push_bits(2, 3); // LFE
        buf.push_empty_sf_data(2);
        buf.push(false); // var_coding_config = 两声道加单声道
        buf.push_two_channel_data(2);
        buf.push_mono_data(2);

        let (mut elements, mut aspx) = workspaces();
        let mut state = VarChannelState::new();
        let mut reader = BitReader::new(buf.as_slice());
        let element = parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: 3,
                b_has_lfe: true,
                b_iframe: true,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        )
        .expect("应能解析");
        assert_eq!(reader.bit_position(), u64::try_from(buf.len).unwrap_or(0));
        assert_eq!(element.aspx_elements(), 0, "夹具前提：Simple 模式无 A-SPX");

        let located: Vec<_> = element.signals().collect();
        assert_eq!(located.len(), 4, "一路 LFE 加三路全频带");
        assert!(
            located.iter().all(|slot| slot.aspx.is_none()),
            "Simple 模式不得给出 A-SPX 下标"
        );
        let channels: Vec<_> = located
            .iter()
            .map(|slot| (slot.channel_element, slot.channel_in_element))
            .collect();
        assert_eq!(channels, vec![(0, 0), (1, 0), (1, 1), (2, 0)]);

        // 作业侧：LFE 与 SIMPLE 全频带信号虽然都只做 QMF 分析，之后却分别走
        // 「留待重新插入」与「进入 A-JOC」两种动作。
        let jobs: Vec<_> = element.aspx_jobs().collect();
        let mut signals = located.into_iter();
        let lfe = signals.next().expect("夹具前提：第一路是 LFE");
        let expected: Vec<_> = core::iter::once(AspxJob::LfeQmf(lfe))
            .chain(signals.map(AspxJob::SimpleQmf))
            .collect();
        assert_eq!(jobs, expected, "SIMPLE 全频带信号仍须进入 QMF/A-JOC");
    }

    /// `channel_element_count()` 必须与 `6.2.4.4` 的三条分支逐一吻合。
    #[test]
    fn element_count_matches_every_branch() {
        // 偶数：n_pairs 个双声道。
        assert_eq!(channel_element_count(2, false), 1);
        assert_eq!(channel_element_count(16, false), 8);
        assert_eq!(channel_element_count(16, true), 9);
        // 单信号：一个 mono_data(0)。
        assert_eq!(channel_element_count(1, false), 1);
        assert_eq!(channel_element_count(1, true), 2);
        // 其余奇数：n_pairs-1 个双声道，再加两个或一个。
        assert_eq!(channel_element_count(5, false), 3);
        assert_eq!(channel_element_count(9, true), 6);
        assert_eq!(channel_element_count(15, true), 9);
        // 上界不得超过工作区常量。
        for n in 1u8..=MAX_FULLBAND_DMX_SIGNALS {
            assert!(
                channel_element_count(n, true) <= MAX_CHANNEL_ELEMENTS,
                "{n} 个信号的元素数超过 {MAX_CHANNEL_ELEMENTS}"
            );
            let aspx = usize::from(n / 2).saturating_add(usize::from(n % 2 == 1));
            assert!(
                aspx <= MAX_ASPX_ELEMENTS,
                "{n} 个信号的 A-SPX 元素数超过 {MAX_ASPX_ELEMENTS}"
            );
        }
    }

    /// 单信号带 LFE 的 A-SPX 元素落点必须与构造长度相等。
    ///
    /// 这是全链路判据：一个 `mono_data(1)`、一个 `mono_data(0)` 与一个
    /// `aspx_data_1ch()` 串联，任一段的宽度或循环次数写错都会偏移。
    #[test]
    fn single_signal_with_lfe_lands_exactly() {
        let mut buf = BitBuf::new();
        buf.push(true); // var_codec_mode = A-SPX
        buf.push_aspx_config();
        // n_dmx_signals = 1 <= 5，故有 companding_control(1)：不传 sync_flag。
        buf.push(true); // b_compand_on[0]
        buf.push_bits(2, 3); // LFE 的 max_sfb（n_msfbl_bits 为 3）
        buf.push_empty_sf_data(2);
        buf.push_mono_data(2);
        buf.push_aspx_data(false);
        let expected = buf.len;

        let (mut elements, mut aspx) = workspaces();
        let mut state = VarChannelState::new();
        let mut reader = BitReader::new(buf.as_slice());
        let element = parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: 1,
                b_has_lfe: true,
                b_iframe: true,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        )
        .expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );
        assert!(element.codec_mode_aspx);
        assert_eq!(element.channel_elements(), 2);
        assert_eq!(element.aspx_elements(), 1);
        assert_eq!(element.coding_config, None, "单信号不传 var_coding_config");
        assert!(element.companding.is_some());
        assert!(state.config().is_some());
    }

    /// 两个信号不带 LFE：偶数分支，一个 `two_channel_data()` 加一个
    /// `aspx_data_2ch()`。
    #[test]
    fn even_branch_lands_exactly() {
        let mut buf = BitBuf::new();
        buf.push(true); // A-SPX
        buf.push_aspx_config();
        buf.push(true); // companding sync_flag（num_chan = 2 > 1）
        buf.push(true); // b_compand_on[0]
        buf.push_two_channel_data(2);
        buf.push_aspx_data(true);
        let expected = buf.len;

        let (mut elements, mut aspx) = workspaces();
        let mut state = VarChannelState::new();
        let mut reader = BitReader::new(buf.as_slice());
        let element = parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: 2,
                b_has_lfe: false,
                b_iframe: true,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        )
        .expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );
        assert_eq!(element.channel_elements(), 1);
        assert_eq!(element.aspx_elements(), 1);
        assert_eq!(element.pairs(), 1);
        assert!(!element.is_odd());
    }

    /// `var_coding_config` 为真时走 `three_channel_data()`，为假时走
    /// `two_channel_data()` 加 `mono_data(0)`。
    #[test]
    fn coding_config_selects_between_three_channel_and_split() {
        for three in [false, true] {
            let mut buf = BitBuf::new();
            buf.push(false); // var_codec_mode = Simple，跳过全部 A-SPX
            // n_dmx_signals = 3：n_pairs = 1，故 pairs-1 = 0 个前置双声道。
            buf.push(three); // var_coding_config
            if three {
                // three_channel_data()：一份 sf_info、4 位矩阵码、两份
                // chparam_info（sap_mode 各 2 位、max_sfb 为 0 故无频带数据）、
                // 三份 sf_data。
                buf.push_long_sf_info(2);
                buf.push_bits(0, 4); // chel_matsel
                buf.push_bits(0, 2); // chparam_info[0] 的 sap_mode
                buf.push_bits(0, 2); // chparam_info[1] 的 sap_mode
                buf.push_empty_sf_data(2);
                buf.push_empty_sf_data(2);
                buf.push_empty_sf_data(2);
            } else {
                buf.push_two_channel_data(2);
                buf.push_mono_data(2);
            }
            let expected = buf.len;

            let (mut elements, mut aspx) = workspaces();
            let mut state = VarChannelState::new();
            let mut reader = BitReader::new(buf.as_slice());
            let element = parse_var_channel_element(
                &mut reader,
                VarChannelParams {
                    context: CONTEXT,
                    n_dmx_signals: 3,
                    b_has_lfe: false,
                    b_iframe: true,
                },
                &mut state,
                VarChannelWorkspace {
                    elements: &mut elements,
                    aspx: &mut aspx,
                },
            )
            .expect("应能解析");

            assert_eq!(
                reader.bit_position(),
                u64::try_from(expected).unwrap_or(0),
                "var_coding_config = {three} 的落点"
            );
            assert_eq!(element.coding_config, Some(three));
            assert_eq!(element.channel_elements(), if three { 1 } else { 2 });
            assert_eq!(element.aspx_elements(), 0, "Simple 模式不带 A-SPX");
            assert!(element.companding.is_none(), "Simple 模式不带压扩控制");
        }
    }

    /// Simple 模式不包含 A-SPX 数据，调用方无需为未出现的元素预留工作区。
    #[test]
    fn simple_mode_accepts_empty_aspx_workspace() {
        let mut buf = BitBuf::new();
        buf.push(false); // var_codec_mode = Simple
        buf.push_mono_data(2);
        let expected = buf.len;

        let mut elements = [ChannelElement::new()];
        let mut aspx: [AspxData; 0] = [];
        let mut state = VarChannelState::new();
        let mut reader = BitReader::new(buf.as_slice());
        let element = parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: 1,
                b_has_lfe: false,
                b_iframe: true,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        )
        .expect("Simple 模式不应要求 A-SPX 工作区");

        assert_eq!(reader.bit_position(), u64::try_from(expected).unwrap_or(0));
        assert!(!element.codec_mode_aspx);
        assert_eq!(element.channel_elements(), 1);
        assert_eq!(element.aspx_elements(), 0);
        assert!(
            matches!(
                element.aspx_jobs().collect::<Vec<_>>().as_slice(),
                [AspxJob::SimpleQmf(_)]
            ),
            "无 LFE 的 SIMPLE 首路不得误标成 LFE"
        );
    }

    /// 信号数超过 5 时不传输 `companding_control()`。
    ///
    /// 六个信号走偶数分支：三个 `two_channel_data()` 加三个
    /// `aspx_data_2ch()`，中间没有压扩控制。多读或少读那一段都会改变落点。
    #[test]
    fn companding_is_omitted_above_five_signals() {
        let mut buf = BitBuf::new();
        buf.push(true); // var_codec_mode = A-SPX
        buf.push_aspx_config();
        // n_dmx_signals = 6 > 5，此处没有 companding_control()。
        for _ in 0..3 {
            buf.push_two_channel_data(2);
        }
        for _ in 0..3 {
            buf.push_aspx_data(true);
        }
        let expected = buf.len;

        let (mut elements, mut aspx) = workspaces();
        let mut state = VarChannelState::new();
        let mut reader = BitReader::new(buf.as_slice());
        let element = parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: 6,
                b_has_lfe: false,
                b_iframe: true,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        )
        .expect("应能解析");

        assert_eq!(
            reader.bit_position(),
            u64::try_from(expected).unwrap_or(0),
            "落点应与构造长度相等"
        );
        assert_eq!(element.companding, None, "六个信号不得传输压扩控制");
        assert_eq!(element.channel_elements(), 3);
        assert_eq!(element.aspx_elements(), 3);
    }

    /// 每个 A-SPX 数据元素的交叉偏移必须**各自**跨帧沿用。
    ///
    /// `aspx_xover_subband_offset` 由每个 `aspx_data_*()` 单独传输（表
    /// 51/52），只在 I 帧出现。若整个 `var_channel_element()` 共用一份状态，
    /// 非 I 帧的前两个元素会错用最后一个元素的偏移——偏移决定
    /// `num_sbg_sig_highres`，符号数随之改变，落点立刻偏移。
    #[test]
    fn each_aspx_element_carries_its_own_crossover_across_frames() {
        let offsets: [u8; 3] = [0, 1, 2];

        let mut iframe = BitBuf::new();
        iframe.push(true); // var_codec_mode = A-SPX
        iframe.push_aspx_config();
        for _ in 0..3 {
            iframe.push_two_channel_data(2);
        }
        for &xover in &offsets {
            iframe.push_aspx_data_with(true, xover, true);
        }

        let (mut elements, mut aspx) = workspaces();
        let mut state = VarChannelState::new();
        let mut reader = BitReader::new(iframe.as_slice());
        parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: 6,
                b_has_lfe: false,
                b_iframe: true,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        )
        .expect("I 帧应能解析");
        assert_eq!(
            reader.bit_position(),
            u64::try_from(iframe.len).unwrap_or(0),
            "I 帧落点"
        );
        for (index, &xover) in offsets.iter().enumerate() {
            let Some(data) = aspx.get(index) else {
                panic!("应有第 {index} 个 A-SPX 元素");
            };
            assert_eq!(
                data.bands.num_sbg_sig_highres(),
                22u8.saturating_sub(xover),
                "第 {index} 个元素的高分辨率组数"
            );
        }

        // 非 I 帧：既没有 aspx_config 也没有交叉偏移，三个元素各自沿用。
        let mut inter = BitBuf::new();
        inter.push(true); // var_codec_mode = A-SPX
        for _ in 0..3 {
            inter.push_two_channel_data(2);
        }
        for &xover in &offsets {
            inter.push_aspx_data_with(true, xover, false);
        }

        let mut reader = BitReader::new(inter.as_slice());
        parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: 6,
                b_has_lfe: false,
                b_iframe: false,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        )
        .expect("非 I 帧应能解析");
        assert_eq!(
            reader.bit_position(),
            u64::try_from(inter.len).unwrap_or(0),
            "非 I 帧落点：三个元素应各自沿用自己的交叉偏移"
        );
        for (index, &xover) in offsets.iter().enumerate() {
            let Some(data) = aspx.get(index) else {
                panic!("应有第 {index} 个 A-SPX 元素");
            };
            assert_eq!(
                data.bands.num_sbg_sig_highres(),
                22u8.saturating_sub(xover),
                "第 {index} 个元素沿用后的高分辨率组数"
            );
        }
    }

    /// 后续 A-SPX 元素失败时，配置与已成功元素的状态都不得提前提交。
    #[test]
    fn failed_later_aspx_element_does_not_commit_var_state() {
        let mut iframe = BitBuf::new();
        iframe.push(true); // var_codec_mode = A-SPX
        iframe.push_aspx_config();
        iframe.push(true); // companding sync_flag
        iframe.push(true); // 共用的 b_compand_on
        for _ in 0..2 {
            iframe.push_two_channel_data(2);
        }
        iframe.push_aspx_data_with(true, 0, true);
        iframe.push_aspx_data_with(true, 1, true);

        let (mut elements, mut aspx) = workspaces();
        let mut state = VarChannelState::new();
        let mut reader = BitReader::new(iframe.as_slice());
        parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: 4,
                b_has_lfe: false,
                b_iframe: true,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        )
        .expect("基准 I 帧应能解析");
        let before = state;

        let mut damaged = BitBuf::new();
        damaged.push(true); // var_codec_mode = A-SPX
        damaged.push_aspx_config_with_quant(true); // 与基准配置不同
        damaged.push(true); // companding sync_flag
        damaged.push(true); // 共用的 b_compand_on
        for _ in 0..2 {
            damaged.push_two_channel_data(2);
        }
        damaged.push_aspx_data_with(true, 3, true); // 第一元素完整且状态不同
        // 第二个 aspx_data_2ch() 缺失，外层解析必须失败且不提交任何状态。

        let mut reader = BitReader::new(damaged.as_slice());
        let result = parse_var_channel_element(
            &mut reader,
            VarChannelParams {
                context: CONTEXT,
                n_dmx_signals: 4,
                b_has_lfe: false,
                b_iframe: true,
            },
            &mut state,
            VarChannelWorkspace {
                elements: &mut elements,
                aspx: &mut aspx,
            },
        );
        assert!(result.is_err(), "缺失第二个 A-SPX 元素必须报错");
        assert_eq!(state, before, "失败元素不得修改整份跨帧状态");
    }

    /// 工作区不足时必须在读取任何比特前拒绝。
    #[test]
    fn rejects_insufficient_workspaces_before_reading() {
        let buf = BitBuf::new();
        let mut state = VarChannelState::new();
        let mut aspx = [AspxData::empty()];

        let mut elements = [ChannelElement::new()];
        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_var_channel_element(
                &mut reader,
                VarChannelParams {
                    context: CONTEXT,
                    n_dmx_signals: 4,
                    b_has_lfe: true,
                    b_iframe: true,
                },
                &mut state,
                VarChannelWorkspace {
                    elements: &mut elements,
                    aspx: &mut aspx,
                },
            ),
            Err(VarElementError::ChannelWorkspaceTooSmall {
                needed: 3,
                provided: 1
            })
        );
        assert_eq!(reader.bit_position(), 0);

        let mut elements = [
            ChannelElement::new(),
            ChannelElement::new(),
            ChannelElement::new(),
        ];
        let mut aspx_mode = BitBuf::new();
        aspx_mode.push(true); // var_codec_mode = A-SPX
        let mut reader = BitReader::new(aspx_mode.as_slice());
        assert_eq!(
            parse_var_channel_element(
                &mut reader,
                VarChannelParams {
                    context: CONTEXT,
                    n_dmx_signals: 4,
                    b_has_lfe: true,
                    b_iframe: true,
                },
                &mut state,
                VarChannelWorkspace {
                    elements: &mut elements,
                    aspx: &mut aspx,
                },
            ),
            Err(VarElementError::AspxWorkspaceTooSmall {
                needed: 2,
                provided: 1
            })
        );
        assert_eq!(reader.bit_position(), 0);
    }

    /// 信号数越界必须在读取任何比特前拒绝。
    #[test]
    fn rejects_signal_count_out_of_range() {
        let buf = BitBuf::new();
        let mut state = VarChannelState::new();
        let (mut elements, mut aspx) = workspaces();
        for count in [0u8, 17, 255] {
            let mut reader = BitReader::new(buf.as_slice());
            assert_eq!(
                parse_var_channel_element(
                    &mut reader,
                    VarChannelParams {
                        context: CONTEXT,
                        n_dmx_signals: count,
                        b_has_lfe: false,
                        b_iframe: true,
                    },
                    &mut state,
                    VarChannelWorkspace {
                        elements: &mut elements,
                        aspx: &mut aspx,
                    },
                ),
                Err(VarElementError::SignalCountOutOfRange {
                    n_dmx_signals: count,
                    limit: MAX_FULLBAND_DMX_SIGNALS,
                })
            );
            assert_eq!(reader.bit_position(), 0);
        }
    }

    /// 非 I 帧缺少可沿用的 `aspx_config()` 时必须报错。
    #[test]
    fn rejects_missing_config_on_non_iframe() {
        let mut buf = BitBuf::new();
        buf.push(true); // var_codec_mode = A-SPX
        buf.push(true); // companding_control(1) 的 b_compand_on[0]
        let (mut elements, mut aspx) = workspaces();
        let mut state = VarChannelState::new();
        let mut reader = BitReader::new(buf.as_slice());
        assert_eq!(
            parse_var_channel_element(
                &mut reader,
                VarChannelParams {
                    context: CONTEXT,
                    n_dmx_signals: 1,
                    b_has_lfe: false,
                    b_iframe: false,
                },
                &mut state,
                VarChannelWorkspace {
                    elements: &mut elements,
                    aspx: &mut aspx,
                },
            ),
            Err(VarElementError::MissingAspxConfig)
        );
    }
}
