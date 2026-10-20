//! A-SPX 高级频谱扩展。
//!
//! `TS103190-1:v1.4.1` 的 `4.2.12` 给出语法，`4.3.10` 给出语义，`5.7.6.3`
//! 给出频带表与时频矩阵的推导。
//!
//! 推导层不需要 Huffman 码本，但需要用户从官方 PDF 本地生成的表，故位于
//! `spec-tables` feature 下：
//!
//! - [`tables`]：`5.7.6.3.1.1` 的模板子带组表、表 189/192 的时隙换算、
//!   表 126 的间隔类别与 `Pseudocode 79` 的码本选择；
//! - [`bands`]：`Pseudocode 67`–`Pseudocode 70` 的子带组表推导，产出
//!   `num_sbg_sig_highres`、`num_sbg_sig_lowres` 与 `num_sbg_noise`；
//! - [`frames`]：`5.7.6.3.3.1` 的时间轴推导，产出包络边界与逐包络的
//!   `atsg_freqres`。
//!
//! 后两者合起来定出 `aspx_ec_data()` 的循环次数：频带表给出每个包络的子带
//! 组数，成帧给出包络数与各自的频率分辨率。任一处算错都会让熵编码数据错位。
//!
//! 解码层需要表 A.16–A.33 的 Huffman 码本或 QMF 表，因此只在 `audio-decode`
//! feature 下存在（模块名不写成文档链接，默认配置下它们并不存在）：
//!
//! - `syntax`：`4.2.12` 的语法元素与 `codebooks` 的码本查表；
//! - `qmf`：`5.7.3`/`5.7.4` 的 64 子带复数 QMF 分析与合成；
//! - `lowband`：`5.7.6.3.2` 的低带滤波与 QMF 延迟线；
//! - `envelope`：`5.7.6.3.4` 的包络解码，产出量化标度因子 `qscf`；
//! - `dequant`：`5.7.6.3.5` 的反量化与平衡式立体声解码，产出线性标度因子
//!   `scf`；
//! - `patches`：`5.7.6.3.1.4` 的 HF patch 子带组表，说明搬运低带的哪几段；
//! - `limiter`：`5.7.6.3.1.5` 的限幅器子带组表，`5.7.6.4.2.2` 算增益上限用；
//! - `preflatten`：`5.7.6.4.1.2` 的预平坦化，把低带谱斜率翻成增益向量；
//! - `tna`：`5.7.6.4.1.3` 的子带音调噪声比调整数据，逐子带的复数预测系数与
//!   逐噪声子带组的 chirp 因子；
//! - `hfgen`：`5.7.6.4.1.4` 的 HF 信号创建，把低带按 patch 表搬到 A-SPX 范围
//!   并施加前三者，`5.7.6.4.1` 的汇合点；
//! - `hfadjust`：`5.7.6.4.2.1` 的包络估计，产出「子带 × 包络」的七张矩阵；
//! - `hfgain`：`5.7.6.4.2.2` 的补偿增益，把那七张矩阵压成三张调整后的矩阵；
//! - `noisegen`：`5.7.6.4.3` 的噪声发生器，把 `noise_lev_sb_adj` 铺成 `qmf_noise`；
//! - `tonegen`：`5.7.6.4.4` 的音调生成器，把 `sine_lev_sb_adj` 铺成 `qmf_sine`；
//! - `hfassemble`：`5.7.6.4.5` 的高频组装，依次合并 `Q_high`、噪声与正弦并
//!   携带越过帧尾的已组装结果；
//! - `interleave`：`5.7.6.5.3` 的输出合并，把延迟后的 `Q_in` 与 `Y` 相加得到
//!   `Q_out`；交织分量本身不可达而未实现，但合并是 A-SPX 唯一的出口；
//! - `pipeline`：把各段编排到稳定的 `Q_out,ASPX` 借用切片；另保留只供链路终点
//!   使用的 PCM 合成包装器，QMF 域出口不会推进合成状态；
//! - `state`：一条声道的十一个跨帧状态，一起创建、一起重置；
//! - `workspace`：一条声道帧内的七块 QMF 域中转缓冲，按合法配置的上界留足。
//!
//! `5.7.6.3.3.2` 的 tiling（`Pseudocode 78`）没有单独的模块：它只是按
//! `atsg_freqres[atsg]` 在高低分辨率两张表之间选一张，组数这一维已由
//! `envelope` 逐包络携带，边界表这一维要到 `5.7.6.4` 把 `scf` 映射回 QMF
//! 子带时才用得上。

#[cfg(feature = "spec-tables")]
pub mod bands;
#[cfg(feature = "audio-decode")]
pub mod codebooks;
#[cfg(feature = "audio-decode")]
pub mod dequant;
#[cfg(feature = "audio-decode")]
pub mod envelope;
#[cfg(feature = "spec-tables")]
pub mod frames;
#[cfg(feature = "audio-decode")]
pub mod hfadjust;
#[cfg(feature = "audio-decode")]
pub mod hfassemble;
#[cfg(feature = "audio-decode")]
pub mod hfgain;
#[cfg(feature = "audio-decode")]
pub mod hfgen;
#[cfg(feature = "audio-decode")]
pub mod interleave;
#[cfg(feature = "audio-decode")]
pub mod limiter;
#[cfg(feature = "audio-decode")]
pub mod lowband;
#[cfg(feature = "audio-decode")]
pub mod noisegen;
#[cfg(feature = "audio-decode")]
pub mod patches;
#[cfg(feature = "audio-decode")]
pub mod pipeline;
#[cfg(feature = "audio-decode")]
pub mod preflatten;
#[cfg(feature = "audio-decode")]
pub mod qmf;
#[cfg(feature = "audio-decode")]
mod reach;
#[cfg(feature = "audio-decode")]
pub mod state;
#[cfg(feature = "audio-decode")]
pub mod syntax;
pub mod tables;
#[cfg(feature = "audio-decode")]
pub mod tna;
#[cfg(feature = "audio-decode")]
pub mod tonegen;
#[cfg(feature = "audio-decode")]
pub mod workspace;

#[cfg(feature = "audio-decode")]
pub use reach::{AspxReach, collect_aspx_reach};
#[cfg(feature = "audio-decode")]
pub use syntax::{
    AspxChannelFraming, AspxConfig, AspxData, AspxEnvelopes, AspxError, AspxHfGen, AspxState,
    MAX_SBG_PER_ENVELOPE,
};

#[cfg(feature = "spec-tables")]
pub use bands::{AspxBandTables, BandError, MAX_SBG_SIG_LOWRES};
#[cfg(feature = "spec-tables")]
pub use frames::{
    AspxInterval, AspxIntervalParams, FrameError, MAX_ATSG_NOISE, MAX_ATSG_SIG, MAX_REL_BORDERS,
};
pub use tables::{
    AspxCodebook, EnvelopeKind, HcbType, IntervalClass, MAX_ASPX_TIMESLOTS, MAX_SBG_MASTER,
    MAX_SBG_NOISE, NUM_QMF_SUBBANDS, StereoMode, get_aspx_hcb,
};
#[cfg(feature = "spec-tables")]
pub use tables::{
    max_var_border_offset, num_aspx_timeslots, num_qmf_timeslots, num_ts_in_ats, sbg_template,
    template_group_count, ts_offset_hfgen,
};
