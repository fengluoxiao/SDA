//! 生成数学常量表，并消费用户本地生成的 PDF 表与规范随附 C 表。
//!
//! `TS103190-1:v1.4.1` 附录 A.0 规定全部 Huffman 码本以随附文件
//! `ts_10319001v010401p0.zip` 给出，PDF 正文只列出码本名称与长度。因此码本
//! 数值不经人工转写：`scripts/fetch_specs.py` 按 `spec/MANIFEST.json` 的
//! `member_sha256` 校验并释出 C 文件，本脚本在构建时解析它。
//!
//! ETSI 文件及其表值不由本项目重分发，故 PDF 表由
//! `scripts/generate_spec_tables.py` 写入被忽略的 `spec/generated/`，本脚本校验
//! 摘要后复制到 `OUT_DIR`。默认构建不需要这些文件；`spec-tables` 消费 PDF
//! 表，`audio-decode` 在其基础上再消费随附 C 表。
//! 工作区构建默认读取仓库的 `spec/`；从注册表使用本 crate 时，通过
//! `MACINDECODE_AC4_SPEC_DIR` 指向由同一获取流程准备好的目录。
//!
//! 构建期校验（任一不成立即中止构建）：
//!
//! - `_LEN` 与 `_CW` 等长；
//! - Kraft 等式 `Σ 2^-len == 1`，即码本是完备前缀码，长度表无错漏；
//! - 码字集合前缀无关，且码字不超出其声明长度；
//! - 完备前缀码的内部节点数恒为符号数减一。
//!
//! 前缀无关同时确认了比特序：把码字按位反转（即 LSB 优先）解释时，84 张
//! 码本无一保持前缀无关，因此 MSB 优先是由数据判定的，不是假设。

// 构建脚本运行在宿主机构建期，panic 就是它报告错误的方式——构建会因此中止
// 并打印位置。这与 no_std 运行期禁止 panic 的约束是两回事，故在此放开。
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

mod build_support;

use build_support::math::{
    emit_dequant_table, emit_ifft_roots, emit_imdct_rotation, emit_kbd_windows,
};
use build_support::noise::emit_aspx_noise;
use build_support::qmf::{emit_qmf_modulation, emit_qmf_window};
use build_support::sha256::verify_expected_hash;
use build_support::spec::{
    TABLE_FILES, emit, parse_arrays, parse_complex_arrays, parse_float_arrays,
};
use build_support::spec_lock::{
    PART1_TABLES_C_SHA256, PART2_TABLES_C_SHA256, PDF_TABLES_RS_SHA256,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

const SPEC_DIR_ENV: &str = "MACINDECODE_AC4_SPEC_DIR";
const PDF_TABLES_FILE: &str = "generated/ts103190_pdf_tables.rs";

fn main() {
    // 反量化表是纯数学量，与 ETSI 附件无关，因此不受 audio-decode 约束，
    // 也不受分发限制。它必须在 feature 判断之前生成。
    emit_dequant_table();
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SPEC_TABLES");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_AUDIO_DECODE");
    let spec_tables = std::env::var_os("CARGO_FEATURE_SPEC_TABLES").is_some();
    let audio_decode = std::env::var_os("CARGO_FEATURE_AUDIO_DECODE").is_some();
    if !spec_tables {
        assert!(!audio_decode, "audio-decode 必须包含 spec-tables feature");
        return;
    }

    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-env-changed={SPEC_DIR_ENV}");
    let spec_dir = match std::env::var_os(SPEC_DIR_ENV) {
        Some(path) if path.is_empty() => panic!("{SPEC_DIR_ENV} 不能为空"),
        Some(path) => PathBuf::from(path),
        None => crate_dir.join("../../spec"),
    };

    let pdf_tables_path = spec_dir.join(PDF_TABLES_FILE);
    println!("cargo:rerun-if-changed={}", pdf_tables_path.display());
    let pdf_tables = std::fs::read(&pdf_tables_path).unwrap_or_else(|error| {
        panic!(
            "读取 {} 失败：{error}\n\
             `spec-tables` 需要用户从官方 ETSI PDF 本地生成表。请依次运行：\n\
             python3 -m pip install -r scripts/requirements-spec.txt\n\
             scripts/fetch_specs.py\n\
             scripts/generate_spec_tables.py\n\
             注册表构建还需将 {SPEC_DIR_ENV} 指向准备好的 spec 目录",
            pdf_tables_path.display()
        )
    });
    verify_expected_hash(PDF_TABLES_FILE, PDF_TABLES_RS_SHA256, &pdf_tables);
    let pdf_tables_text = String::from_utf8(pdf_tables)
        .unwrap_or_else(|error| panic!("{PDF_TABLES_FILE} 不是 UTF-8：{error}"));
    let transform_lengths = parse_usize_array(&pdf_tables_text, "TRANSFORM_LENGTHS_48");
    let alpha_halves = parse_u8_array(&pdf_tables_text, "KBD_ALPHA_HALVES_48");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let pdf_tables_out = out_dir.join("ts103190_pdf_tables.rs");
    std::fs::write(&pdf_tables_out, pdf_tables_text.as_bytes()).unwrap_or_else(|error| {
        panic!("写入 {} 失败：{error}", pdf_tables_out.display());
    });

    // 这些表是纯数学量，但其支持长度与 KBD alpha 来自本地 PDF 表。
    emit_ifft_roots(&transform_lengths);
    emit_imdct_rotation(&transform_lengths);
    emit_kbd_windows(&transform_lengths, &alpha_halves);

    if !audio_decode {
        return;
    }

    // QMF 调制相位本身是数学量，仅完整音频路径需要。
    emit_qmf_modulation();

    let mut arrays: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let mut floats: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    let mut complex: BTreeMap<String, Vec<[f32; 2]>> = BTreeMap::new();
    for name in TABLE_FILES {
        let path = spec_dir.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = std::fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "读取 {} 失败：{error}\n\
                 规范随附的 C 表不在 crate 内；请先运行 scripts/fetch_specs.py，\
                 或将 {SPEC_DIR_ENV} 指向已准备好的规范表目录",
                path.display()
            )
        });
        let expected = match name {
            "ts_103190_tables.c" => PART1_TABLES_C_SHA256,
            "ts_103190_tables_part2.c" => PART2_TABLES_C_SHA256,
            _ => panic!("没有为 {name} 锁定摘要"),
        };
        verify_expected_hash(name, expected, &bytes);
        let text = String::from_utf8_lossy(&bytes);
        parse_arrays(&text, &mut arrays);
        parse_float_arrays(&text, &mut floats);
        parse_complex_arrays(&text, &mut complex);
    }

    emit_qmf_window(&floats);
    emit_aspx_noise(&complex);

    let generated = emit(&arrays);
    let out = out_dir.join("huffman_tables.rs");
    std::fs::write(&out, generated).unwrap_or_else(|error| {
        panic!("写入 {} 失败：{error}", out.display());
    });
}

fn parse_usize_array(source: &str, name: &str) -> Vec<usize> {
    array_body(source, name)
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|_| panic!("{name} 含非整数值 {value:?}"))
        })
        .collect()
}

fn parse_u8_array(source: &str, name: &str) -> Vec<u8> {
    array_body(source, name)
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|_| panic!("{name} 含非 u8 值 {value:?}"))
        })
        .collect()
}

fn array_body<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("const {name}:");
    let declaration = source
        .find(&marker)
        .unwrap_or_else(|| panic!("本地生成表中找不到 {name}"));
    let tail = &source[declaration..];
    let start = tail
        .find("= [")
        .unwrap_or_else(|| panic!("{name} 没有数组起点"))
        + 3;
    let end = tail[start..]
        .find("];")
        .unwrap_or_else(|| panic!("{name} 没有数组终点"));
    &tail[start..start + end]
}
