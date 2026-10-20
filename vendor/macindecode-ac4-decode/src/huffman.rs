//! Huffman 码本解码。
//!
//! `TS103190-1:v1.4.1` 附录 A 的码本以随附 C 文件给出，由 `build.rs` 在构建
//! 时转成解码 trie，见该文件的说明与校验条件。本模块只负责按 trie 走位。
//!
//! 规范中的 `huff_decode(codebook, hcw)` 返回码本内的**符号下标**，不是最终
//! 数值；下标到数值的映射（`cb_mod` / `cb_off` 等）属于各语法元素自身，不在
//! 本模块内完成。

use core::fmt;
use macindecode_ac4_bitstream::reader::{BitReader, ReadError};

/// 码本解码失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HuffmanError {
    /// 读取比特时越过了数据末尾。
    Read(ReadError),
    /// trie 指向了不存在的节点。
    ///
    /// 码本由构建期校验保证是完备前缀码，正常输入不会走到这里；保留该分支
    /// 是为了不在运行期 panic。
    MalformedTable,
}

impl fmt::Display for HuffmanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HuffmanError::Read(error) => write!(f, "{error}"),
            HuffmanError::MalformedTable => write!(f, "Malformed Huffman table"),
        }
    }
}

impl core::error::Error for HuffmanError {}

impl From<ReadError> for HuffmanError {
    fn from(error: ReadError) -> Self {
        HuffmanError::Read(error)
    }
}

/// 一张 Huffman 码本的解码 trie。
///
/// 每个元素是一个内部节点的两条分支，下标 0 对应比特 0。分支值非负时是子
/// 节点下标，为负时 `!value` 是符号下标。根节点固定为第 0 项。
#[derive(Debug)]
pub struct HuffmanTable {
    nodes: &'static [[i16; 2]],
}

impl HuffmanTable {
    /// 仅由已完成结构校验的生成代码调用。
    const fn new(nodes: &'static [[i16; 2]]) -> Self {
        Self { nodes }
    }

    /// 码本内的符号个数。
    ///
    /// 完备前缀码的内部节点数恒为符号数减一，该等式由构建期断言保证。
    #[must_use]
    pub const fn len(&self) -> usize {
        self.nodes.len().saturating_add(1)
    }

    /// 码本是否为空。恒为假，仅为满足 `len` 的惯例而提供。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// 从当前位置解出一个符号下标，按 MSB 优先逐比特下行。
    ///
    /// # Errors
    ///
    /// 数据不足以走完一条码字时返回 [`HuffmanError::Read`]。
    pub fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16, HuffmanError> {
        let mut node = 0usize;
        loop {
            let branches = self.nodes.get(node).ok_or(HuffmanError::MalformedTable)?;
            let bit = usize::from(reader.read_flag()?);
            let next = *branches.get(bit).ok_or(HuffmanError::MalformedTable)?;
            if next < 0 {
                return u16::try_from(!next).map_err(|_| HuffmanError::MalformedTable);
            }
            node = usize::try_from(next).map_err(|_| HuffmanError::MalformedTable)?;
        }
    }
}

/// 规范随附 C 表生成的全部码本。
pub mod tables {
    use super::HuffmanTable;

    include!(concat!(env!("OUT_DIR"), "/huffman_tables.rs"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tables::{ASF_HCB_SCALEFAC, GENERATED_CODEBOOKS};

    /// 构建期若一张码本都没生成，后续测试会假绿。
    #[test]
    fn generates_all_codebooks() {
        assert_eq!(
            GENERATED_CODEBOOKS, 84,
            "规范随附 C 表应给出 84 张 Huffman 码本"
        );
    }

    /// `ASF_HCB_SCALEFAC` 的规模见附录 A 表 A.1。
    #[test]
    fn scalefac_codebook_has_declared_length() {
        assert_eq!(ASF_HCB_SCALEFAC.len(), 121);
    }

    /// 逐比特解码应与码本自身的 (长度, 码字) 一致。
    ///
    /// 这里用附录 A.1 中 `ASF_HCB_SCALEFAC` 的最短码字：下标 60 的码长为 1、
    /// 码字为 0b1，是全表唯一的一比特码。
    #[test]
    fn decodes_single_bit_codeword() {
        let data = [0b1000_0000];
        let mut reader = BitReader::new(&data);
        assert_eq!(ASF_HCB_SCALEFAC.decode(&mut reader).unwrap(), 60);
        assert_eq!(reader.bit_position(), 1);
    }

    /// 解码只应消耗码字本身的比特，不得多读。
    #[test]
    fn leaves_reader_at_codeword_end() {
        // 下标 59 的码长为 3、码字为 0b011；其后补 0b10101 供核对。
        let data = [0b011_10101];
        let mut reader = BitReader::new(&data);
        assert_eq!(ASF_HCB_SCALEFAC.decode(&mut reader).unwrap(), 59);
        assert_eq!(reader.bit_position(), 3);
        assert_eq!(reader.read_bits(5).unwrap(), 0b10101);
    }

    /// 数据不足以走完一条码字时报错，而非返回残缺符号。
    #[test]
    fn reports_truncated_codeword() {
        // 0b0000_0000 走的是长码分支，8 比特不足以到达叶子。
        let data = [0b0000_0000];
        let mut reader = BitReader::new(&data);
        assert!(matches!(
            ASF_HCB_SCALEFAC.decode(&mut reader),
            Err(HuffmanError::Read(_))
        ));
    }

    /// 84 张码本的每个符号都能被其自身码字原样解出，且不多读一比特。
    ///
    /// 这一条不校验表值——表值由构建期的 Kraft 与前缀无关断言、以及
    /// `spec/MANIFEST.json` 记录的成员哈希保证。它校验的是解码器与构造侧
    /// 对比特序和叶子编码的理解一致：任一方向搞反，全部符号都会错位。
    #[test]
    fn every_symbol_round_trips() {
        let mut symbols = 0usize;
        for &(name, table, lengths, codewords) in tables::ALL_CODEBOOKS {
            assert_eq!(table.len(), lengths.len(), "{name} 的符号数与码长表不符");
            for (symbol, (&len, &codeword)) in lengths.iter().zip(codewords).enumerate() {
                // 码字左对齐到 32 比特写入，最长码字 29 比特。
                let buffer = (codeword << (32 - u32::from(len))).to_be_bytes();
                let mut reader = BitReader::new(&buffer);
                let decoded = table.decode(&mut reader).unwrap();
                assert_eq!(
                    usize::from(decoded),
                    symbol,
                    "{name} 的第 {symbol} 个符号解出了 {decoded}"
                );
                assert_eq!(
                    reader.bit_position(),
                    u64::from(len),
                    "{name}[{symbol}] 消耗的比特数与码长不符"
                );
                symbols += 1;
            }
        }
        assert_eq!(symbols, 4_917, "遍历到的符号总数与码本规模不符");
    }
}
