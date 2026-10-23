# macindecode-ac4-decode

SDA local fork: reconstructed-source capacity is 128. Upstream revision, retained license and changes are documented in [SDA-PATCHES.md](SDA-PATCHES.md).

`#![no_std]` 的 Dolby AC-4 音频数值重建：ASF 反量化与 IMDCT、A-SPX 频谱扩展、
A-JOC 对象重建、QMF 分析/合成，以及把它们编排到表 188 时间轴的统一 Full engine。
语法与元数据解析由 [`macindecode-ac4-bitstream`] 提供，容器解析由
`macindecode-ac4-mp4` 提供。

本 crate 是独立开源实现，与 Dolby Laboratories 不存在隶属、赞助或认可关系；
相关商标仅用于说明兼容性。

```toml
[dependencies]
macindecode-ac4-decode = "0.1.0"
```

## 边界

依赖方向是单向的 syntax → decode → scene，见 ADR-0011 与 ADR-0013。本 crate 依赖
`macindecode-ac4-bitstream`，反向不成立。

**一切需要 ETSI 规范表的处理都在这里**，包括 Huffman 码本机制与两处 Huffman 编码的
元数据解码——`drc_gains` 的 presentation DRC gain set 与 `dialog_enhancement` 的 DE
参数。这两处按规范属于元数据而不是 DSP，把它们放在这里是因为它们消费的是同一批
随附 C 表：规范表流水线与冻结摘要因此只有一份真相源。默认 feature 下不含任何规范表，
`macindecode-ac4-bitstream` 也因此不需要构建脚本。

## `spec-tables` 与 `audio-decode`

`spec-tables` 启用 ASF 成帧/IMDCT、A-SPX 表与 A-JOC 参数表；其数值由用户从
官方 ETSI PDF 本地生成。`audio-decode` 会自动包含 `spec-tables`，并额外消费
ETSI 随附的 Huffman/QMF C 表。仓库检出中运行：

```bash
python3 -m pip install -r scripts/requirements-spec.txt
./scripts/fetch_specs.py
./scripts/generate_spec_tables.py
cargo build -p macindecode-ac4-decode --features spec-tables
cargo build -p macindecode-ac4-decode --features audio-decode
```

从注册表作为依赖构建时，请使用与 crate 版本相同的仓库 tag 运行上述脚本，再把
生成好的 `spec/` 目录通过绝对路径传给 `MACINDECODE_AC4_SPEC_DIR`。`spec-tables`
只需要第一项；`audio-decode` 需要全部三项：

- `generated/ts103190_pdf_tables.rs`
- `ts_103190_tables.c`
- `ts_103190_tables_part2.c`

```bash
MACINDECODE_AC4_SPEC_DIR=/absolute/path/to/spec \
  cargo build --features macindecode-ac4-decode/audio-decode
```

构建脚本使用 crate 内置摘要校验实际读入的三个文件；缺失、版本不匹配或内容变化
都会直接终止构建。官方 PDF、C 表和生成文件均保持在用户本地。

MSRV 为 Rust 1.98，禁止 unsafe Rust。完整设计、支持矩阵和规范追踪见
[项目仓库](https://github.com/SakuzyPeng/MacinDecode-AC4-Core)。

[`macindecode-ac4-bitstream`]: https://docs.rs/macindecode-ac4-bitstream

## License

[MIT](LICENSE)
