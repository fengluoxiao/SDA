# Windows 原始空间码流接入：端点探测

独立的第一阶段验证工具，不接入 SDA 播放流程。仅枚举活动的输出端点，调用 WASAPI `IAudioClient::IsFormatSupported` 查询独占格式支持。不会初始化或启动音频流，不会修改默认设备。

Windows 上运行：

```powershell
cargo test --manifest-path tools/windows-audio-probe/Cargo.toml
cargo run --manifest-path tools/windows-audio-probe/Cargo.toml
```

测试三个具体的 48 kHz 内容描述符：E-AC-3 5.1、E-AC-3 Atmos 5.1、MLP MAT 1 7.1。载波格式和编码内容格式分别填写；仅精确 `S_OK` 标记为接受。`0x88890008` 是 `AUDCLNT_E_UNSUPPORTED_FORMAT`。报告含本机端点 ID，分享前请自行删去。

**局限：** 不是所有格式组合的穷举；拒绝某个描述符不能证明整个格式族不可用。接受也不能证明播放器会向该端点直通、能捕获对象，或 SDA 已能接收码流。未测试 MAT 2.x 和 MPEG-H，不能从 GUID 名称推断传输封装。

2026-09-18 本机验证：编译和布局单元测试通过，5 个活动端点对上述 3 个描述符均返回不支持。未安装或修改驱动。

下一阶段需要在独立测试环境验证虚拟 WaveRT 端点：

1. 声明并协商准确的 IEC61937 格式，确认播放器实际提交压缩流。
2. 接收并验证完整 burst、访问单元及断流/跳转边界；对照原文件，不允许将声道 PCM 当成原始对象。
3. 接入 SDA 对应解码器，比较对象数量、位置、时间戳与文件直读结果。
4. 完整性验证通过后，再接主播放器、双耳渲染及实际输出设备。

API 依据和跨平台限制见 [系统输入研究](../../docs/system-atmos-input-research.md)。当前没有虚拟输入驱动或系统对象捕获功能。
