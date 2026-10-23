//! 测试用的比特串构造器。
//!
//! 只在 `cfg(test)` 下编译。各 `push_*` 方法对应规范中的一个语法元素，构造
//! 的都是该元素的最短合法形式；判据是「构造长度」与「解析落点」相等，因此
//! 这些方法必须与被测代码各自独立地实现同一段语法。

use crate::aspx::bands::AspxBandTables;
use crate::aspx::codebooks::table_for;
use crate::aspx::tables::{EnvelopeKind, HcbType, StereoMode, get_aspx_hcb};
use crate::huffman::tables::ALL_CODEBOOKS;

/// 定长比特缓冲区。
pub(crate) struct BitBuf {
    bytes: [u8; 4096],
    len: usize,
}

impl BitBuf {
    pub(crate) const fn new() -> Self {
        Self {
            bytes: [0; 4096],
            len: 0,
        }
    }

    /// 已写入的比特数。
    pub(crate) const fn bit_len(&self) -> usize {
        self.len
    }

    pub(crate) fn push(&mut self, bit: bool) {
        let index = self.len / 8;
        let shift = 7usize.saturating_sub(self.len % 8);
        if bit && let Some(slot) = self.bytes.get_mut(index) {
            *slot |= 1u8
                .checked_shl(u32::try_from(shift).unwrap_or(0))
                .unwrap_or(0);
        }
        self.len = self.len.saturating_add(1);
    }

    pub(crate) fn push_bits(&mut self, value: u32, width: u32) {
        for bit in (0..width).rev() {
            self.push((value >> bit) & 1 == 1);
        }
    }

    /// 补足到字节边界。
    pub(crate) fn byte_align(&mut self) {
        while !self.len.is_multiple_of(8) {
            self.push(false);
        }
    }

    /// 追加整字节；调用前应已对齐。
    pub(crate) fn push_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.push_bits(u32::from(byte), 8);
        }
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        let bytes = self.len.div_ceil(8);
        self.bytes.get(..bytes).unwrap_or(&self.bytes)
    }

    pub(crate) fn push_symbol(&mut self, table: &crate::huffman::HuffmanTable, symbol: u16) {
        for &(_, candidate, lengths, codewords) in ALL_CODEBOOKS {
            if !core::ptr::eq(candidate, table) {
                continue;
            }
            let index = usize::from(symbol);
            let (Some(&width), Some(&codeword)) = (lengths.get(index), codewords.get(index)) else {
                panic!("码本没有第 {symbol} 个符号");
            };
            self.push_bits(codeword, u32::from(width));
            return;
        }
        panic!("码本不在 ALL_CODEBOOKS 内");
    }

    /// 15 位 `aspx_config()`。
    pub(crate) fn push_aspx_config(&mut self) {
        self.push(false); // quant_mode_env
        self.push_bits(0, 3);
        self.push_bits(0, 2);
        self.push(true); // 高分辨率模板
        self.push(false);
        self.push(false);
        self.push(false);
        self.push_bits(1, 2); // noise_sbg
        self.push(false);
        self.push_bits(3, 2); // freq_res_mode = 恒高
    }

    pub(crate) fn push_long_sf_info(&mut self, max_sfb: u32) {
        self.push(true);
        self.push_bits(max_sfb, 6);
    }

    pub(crate) fn push_empty_sf_data(&mut self, max_sfb: u32) {
        self.push_bits(0, 4);
        self.push_bits(max_sfb.saturating_sub(1), 5);
        self.push_bits(0, 8);
        self.push(false);
    }

    pub(crate) fn push_mono_data(&mut self, max_sfb: u32) {
        self.push(false); // spec_frontend = ASF
        self.push_long_sf_info(max_sfb);
        self.push_empty_sf_data(max_sfb);
    }

    /// 一个 FIXFIX 单包络的 `aspx_data_1ch()`。
    pub(crate) fn push_aspx_data_1ch(&mut self) {
        self.push_aspx_data_1ch_for_frame(true);
    }

    /// 一个 FIXFIX 单包络的 `aspx_data_1ch()`；交叉偏移只在 I 帧传输。
    pub(crate) fn push_aspx_data_1ch_for_frame(&mut self, b_iframe: bool) {
        self.push_aspx_data_1ch_with_differentials(b_iframe, false);
    }

    /// 与 [`Self::push_aspx_data_1ch_for_frame`] 相同，但把 DF 符号写成零差分，
    /// 使量化包络保持在 F0 的有限范围内，可继续进入数值流水线。
    pub(crate) fn push_drivable_aspx_data_1ch_for_frame(&mut self, b_iframe: bool) {
        self.push_aspx_data_1ch_with_differentials(b_iframe, true);
    }

    fn push_aspx_data_1ch_with_differentials(&mut self, b_iframe: bool, zero_differentials: bool) {
        let Ok(bands) = AspxBandTables::derive(true, 0, 0, 1, 0) else {
            panic!("频带表应可推导");
        };
        let highres = bands.num_sbg_sig_highres();
        let noise_sbg = bands.num_sbg_noise();

        if b_iframe {
            self.push_bits(0, 3); // xover
        }
        self.push(false); // FIXFIX
        self.push_bits(0, 1); // 1 个包络
        self.push(false); // sig_delta_dir
        self.push(false); // noise_delta_dir
        for _ in 0..noise_sbg {
            self.push_bits(0, 2); // tna_mode
        }
        self.push(false); // ah_present
        self.push(false); // fic_present
        self.push(false); // tic_present

        let f0 = get_aspx_hcb(EnvelopeKind::Signal, StereoMode::Level, false, HcbType::F0);
        let df = get_aspx_hcb(EnvelopeKind::Signal, StereoMode::Level, false, HcbType::Df);
        self.push_symbol(table_for(f0), 0);
        let df_symbol = if zero_differentials {
            u16::try_from(table_for(df).len().saturating_sub(1) / 2).unwrap_or(0)
        } else {
            0
        };
        for _ in 1..highres {
            self.push_symbol(table_for(df), df_symbol);
        }
        let nf0 = get_aspx_hcb(EnvelopeKind::Noise, StereoMode::Level, false, HcbType::F0);
        let ndf = get_aspx_hcb(EnvelopeKind::Noise, StereoMode::Level, false, HcbType::Df);
        self.push_symbol(table_for(nf0), 0);
        let ndf_symbol = if zero_differentials {
            u16::try_from(table_for(ndf).len().saturating_sub(1) / 2).unwrap_or(0)
        } else {
            0
        };
        for _ in 1..noise_sbg {
            self.push_symbol(table_for(ndf), ndf_symbol);
        }
    }

    /// `oamd_timing_data()`：隐式偏移、指定块数、每块零 ramp。
    pub(crate) fn push_timing(&mut self, blocks: u32) {
        self.push(false); // oa_sample_offset_type = 隐式
        self.push_bits(blocks, 3); // num_obj_info_blocks，非 minus1
        for _ in 0..blocks {
            self.push_bits(0, 6); // block_offset_factor
            self.push_bits(0, 2); // ramp_duration_code = 0，无后续字段
        }
    }

    /// 一个最简 `object_info_block()`：对象不活动。
    ///
    /// 不活动时基本信息与渲染信息都取缺省，但仍有一位 `b_additional_data`，
    /// 故最短是两位而非一位。
    pub(crate) fn push_inactive_object_block(&mut self) {
        self.push(true); // b_object_not_active
        self.push(false); // b_additional_data
    }

    /// 一个带绝对位置的 `object_info_block()`，对象活动且为动态对象。
    ///
    /// `b_no_delta` 为真时基本与渲染信息的状态位不传输，故两条路径的长度不同。
    /// 基本信息取 `default_metadata`，渲染信息只带位置，区域与其他属性都取
    /// 分组缺省。
    /// `z_sign` 按码流字段取值：**为真表示正 Z**，见 `resolve_position`。
    pub(crate) fn push_absolute_position_block(
        &mut self,
        b_no_delta: bool,
        x: u32,
        y: u32,
        z_sign: bool,
        z: u32,
    ) {
        self.push(false); // b_object_not_active
        if !b_no_delta {
            self.push(false); // basic：非 reuse
        }
        self.push(true); // ObjectBasicInfo 的 default_metadata
        if !b_no_delta {
            self.push(false); // render：非 reuse
            self.push(false); // render：非 part_reuse
        }
        if !b_no_delta {
            self.push(false); // b_diff_pos_coding
        }
        self.push_bits(x, 6);
        self.push_bits(y, 6);
        self.push(z_sign);
        self.push_bits(z, 4);
        self.push(true); // zone 取分组缺省
        self.push(true); // other_properties 取分组缺省
        self.push(false); // b_additional_data
    }

    /// 一个带差分位置的 `object_info_block()`；只能出现在非 I 帧首块之外。
    pub(crate) fn push_differential_position_block(&mut self, dx: u32, dy: u32, dz: u32) {
        self.push(false); // b_object_not_active
        self.push(false); // basic：非 reuse
        self.push(true); // default_metadata
        self.push(false); // render：非 reuse
        self.push(false); // render：非 part_reuse
        self.push(true); // b_diff_pos_coding
        self.push_bits(dx, 3);
        self.push_bits(dy, 3);
        self.push_bits(dz, 3);
        self.push(true); // zone 取分组缺省
        self.push(true); // other_properties 取分组缺省
        self.push(false); // b_additional_data
    }

    /// 一个最简 `ajoc()`：无去相关器、无数据点、单对象不存在。
    pub(crate) fn push_minimal_ajoc(&mut self, num_umx: usize) {
        self.push_bits(0, 3); // ajoc_num_decorr = 0
        for _ in 0..num_umx {
            self.push(false); // ajoc_object_present = 0
        }
        self.push_bits(0, 2); // ajoc_num_dpoints = 0
        self.push(false); // ajoc_b_nodt
    }

    /// 一个最简 `ajoc_dmx_de_data()`：带配置、不传系数、无主对话对象。
    pub(crate) fn push_minimal_dmx_de(&mut self, num_umx: usize) {
        self.push(true); // b_dmx_de_cfg
        self.push(true); // b_keep_dmx_de_coeffs
        self.push_bits(0, 2); // de_max_gain
        for _ in 0..num_umx {
            self.push(false); // de_main_dlg_flag
        }
    }
}
