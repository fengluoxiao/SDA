# macindecode-ac4-scene

面向 AC-4 Core 与 Full A-JOC 的容器无关、流式渲染前场景 API。该 crate 提供
`Ac4SceneFrame` 数据契约、presentation 选择、整数采样时间线、结构化错误和
`Ac4DecoderSession`，并以借用视图发布对象/LFE PCM 与 OAMD 状态。

成功解码的 AU 还可从 `DecodedAccessUnit::presentation_metadata` 借用所选 presentation 的
完整 processing-metadata payload、当前帧解析视图、有效 DRC 配置和 substream-group gain
码值。该 AU 级侧车只解析、验证和保留原值，不进入 `Ac4SceneFrame`，也不执行 DRC、gain、
pan、downmix、loudness correction 或 renderer 处理。

容器或系统层已经解析的 presentation metadata 可由调用方放入泛型
`PresentationSelectionMetadata<T>`；已选择的
`ScenePresentation::match_selection_metadata` 只按双方唯一的 effective presentation ID
关联，并返回原 entry 的只读视图。没有 ID 时只允许双方各自唯一的无 ID 项回退，重复 ID 或
多路无 ID 保持歧义。metadata 不参与 TOC 驱动的解码配置，因此 `T` 可以直接是
`macindecode-ac4-mp4` 的借用 DSI envelope，未知版本仍保留其原始定界 body，而本 crate
不需要依赖 MP4。opaque body 的身份必须标为 `Unavailable`，不能把尚未解析 ID 猜成明确
无 ID 后走唯一回退；只要集合仍含这类身份不可用项，关联结果就是 `Indeterminate`，不会
把其余已知候选提前宣称为唯一。

```toml
[dependencies]
macindecode-ac4-scene = "0.1.0"
```

默认构建提供 `#![no_std]` 场景模型与控制面。启用完整音频路径：

```toml
[dependencies]
macindecode-ac4-scene = { version = "0.1.0", features = ["audio-decode"] }
```

`audio-decode` 会转发到 `macindecode-ac4-decode/audio-decode`，因此需要用户从
官方 ETSI PDF 本地生成的 Rust 表与外部 C 表。它们不会随 crate 分发；注册表构建
须设置 `MACINDECODE_AC4_SPEC_DIR`。
目录格式与获取方式见
[`macindecode-ac4-decode` 文档](https://docs.rs/macindecode-ac4-decode)。

MSRV 为 Rust 1.98，禁止 unsafe Rust。容器 sample table、priming 与 edit list
换算不属于本 crate，相关功能由 `macindecode-ac4-mp4` 提供。

## License

[MIT](LICENSE)
