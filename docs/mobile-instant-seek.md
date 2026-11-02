# 移动端点击定位

播放页原进度条支持点击或拖动结束后定位，外观、封面与 mini-player 动画保持原有实现。时间显示以原生输出时钟为准；不会只把进度条移到新位置，继续等待从头解码。

## 实现

- iOS / Android 的请求携带歌曲内容标识，拒绝已切换歌曲的旧请求；连续点击由原生输入线程合并到最新位置。
- ALAC / MP3 使用包表定位；iOS 的 ALAC 保留 Apple PCM 解码，只重建目标附近的读取区间，并使用实际首个 PCM 时间戳。
- Atmos 容器使用样本表，裸 EC-3、Audio Vivid、AC-4 使用帧头索引；360RA 使用 MHAS 配置状态和独立帧定位。索引不包含 PCM。
- 只解码、渲染一小段前滚，前滚样本在进入输出 FIFO 前丢弃。保持当前 HRTF/房间资源、歌曲增益和用户设置，清除旧输出与旧对象时间线。
- 渲染队列使用代次检查，已经被新请求取代的渲染块不能重新写入 FIFO。
- 定位不运行整曲响度分析、房间仿真参数计算或 HRTF 资产重载。保持当前歌曲的增益状态。
- 逐歌曲分析结果不落盘、不跨应用启动复用；旧结果读写已移除。定位后的片段也不保存为整曲响度结果。

## 验证与边界

在 Windows 上执行移动端同一原生引擎，提供的 Atmos M4A 在三个位置的优化构建输出回调延迟约为 80–85 ms。这包括目标附近的解码和 HRTF 渲染，尚不包含手机系统、蓝牙与耳机的额外输出延迟，也不代表已安装到 iPhone 实测。

实际 BADDY BADDY ALAC 的多个跳转窗口与连续解码逐样本一致；带 MP3 bit reservoir 的测试也通过。360RA 的 PCM 与连续解码对比通过。Audio Vivid 的自由运行随机噪声填充在随机定位后不保证逐样本一致，因此另核对采样相位、每声道能量和波形相关性，不通过从头解码追赶随机数状态。

文件必须具备有效包边界或可验证的独立帧。无法建立索引的流仍保留原播放读取逻辑，定位会返回实际不支持的错误。iOS 的 Apple 系统空间音频输出路径暂未接入快速定位；当前实现针对 SDA 原生 HRTF / 直接下混输出。Windows / macOS UI 和旧的浏览器 Worker 定位文档不在本次移动端接入范围内。

## 相关检查

- `apps/mobile/test/import-native-queue.test.mjs`：原生定位调用、暂停保留、连续请求覆盖、无 UI 假跳转、无重新导入/整曲分析。
- `crates/sda-native/src/ios.rs` 的 `seek_callback_tests`：真实 FIFO/回调出声、旧音频隔离与暂停后定位。
- `crates/sda-native/src/packet_source.rs`、`mhas_source.rs` 与 `scripts/alac-file-regression/tests/seek.rs`：帧头索引、重采样、真实素材与 EOF 后定位。
- `apps/native-renderer/src/render_command.rs` 的代次测试：取消旧 PCM 后仍保留用户设置。
- 移动端 TypeScript、UI 回归、Android Kotlin 编译及 aarch64 Android Rust 检查。

iOS Swift 集成仍需 Xcode 编译，随后在已安装应用里核对文件类型、暂停/续播和实际设备延迟。
