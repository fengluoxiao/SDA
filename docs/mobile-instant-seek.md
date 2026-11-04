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

## 对象显示、切歌与导入性能

对象的仅增益事件不代表移除对象：已有位置保持不变，更新增益和时间；未知位置不会凭空创建对象。定位后继续使用实际解码的对象事件与渲染活动，不把床声道伪装成对象。`seek_callback_tests` 校验真实出声回调时对象快照也已恢复。

移动端原进度条在按下/拖动时用原生驱动弹簧将竖向比例放大至 1.75，释放或手势中断时恢复；不改变封面、时间文字的布局。外观仍需已安装设备核对。

Audio Vivid 和立体声的整曲响度参数属于当前进程中已加载的队列音源；A→B→A、重复播放和定位不会对同一有效音源重复分析。移出队列、文件变化或模块销毁后按归属释放。测量器减少取模、重复分配和对数计算，CNN 减少内层整数索引运算，保留测量政策和浮点累加顺序。235 秒的 Audio Vivid 诊断文件在当前 Windows 机器由 24.57 秒降至 19.05 秒，整曲响度/真峰值结果相同；这不是手机分析耗时，首次分析仍需进一步优化。

360RA/Audio Vivid 的 MP4 提取优先遵循容器索引读取偏移，支持末尾 moov，无需先传完整 mdat。原生导入会话持有一个输入与一个输出句柄，使用最多 1 MiB 的读写块；旧宿主使用 256 KiB 后备。进度桥接限频，阶段切换立即报告。碎片 MP4 不使用普通 MP4 的提前完成条件，保留全部片段。所有临时句柄和未完成输出在失败/取消时关闭回收。

## Android 与 iOS 的暂停语义

Android 的解码循环与 iOS `SdaPlayer.feed` 一致：用户暂停或音频焦点中断时等待，定位期间不以渲染器的临时 `paused` 遥测阻止解码。`SeekReset` 会先把渲染器置为暂停；只有继续提供新位置附近的有限预滚 PCM，才能达到 `StartAt` 的恢复条件。等待这一临时标记会造成解码与渲染相互等待，手动恢复播放也无法解除。

Android `playbackStatus` 与 iOS `playbackSnapshot` 一样返回宿主的暂停意图，避免界面在定位准备期间误显示为用户已暂停。保留原定位索引、节目增益与对象事件，不重新分析整曲，也不持久化渲染音频。

- `python scripts/test-android-seek-feed.py`：在 JVM 上执行从生产 Kotlin 提取的真实解码等待条件；覆盖播放中定位、暂停后定位、恢复播放、连续定位与设备恢复。
- `cargo test --manifest-path crates/sda-native/Cargo.toml --no-default-features --test android_seek_feed -- --test-threads=1`：通过真实解码器、渲染工作线程和 FIFO 消费核对定位后实际出声、采样时钟推进与旧 PCM 隔离；使用仓库合成 ALAC/MP3 素材。设置 `SDA_ATMOS_SEEK_FILE`、`SDA_AV3A_SEEK_FILE`、`SDA_MHAS_SEEK_FILE` 可追加本地沉浸式文件检查，私人音频不进入 CI。
