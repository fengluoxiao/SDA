# Android PCM 格式错配验证（2026-09-27）

## 已确认的缺陷

`apps/native-renderer/src/aaudio_output.rs` 将 `AAUDIO_FORMAT_PCM_FLOAT` 手写成了 `1`，但 NDK `aaudio/AAudio.h` 和预生成 `ndk-sys` 绑定定义 `PCM_I16=1`、`PCM_FLOAT=2`。旧 writer 向 int16 音频流交付 f32 缓冲，导致位模式被错误解释；减小音量、改变喂数间隔不能纠正这种格式错配。

输出模块现在直接使用 `ndk-sys` 的函数及常量，打开后校验实际格式/通道/采样率。当前支持 f32 stereo 48 kHz；不符合约定时关闭流并报错，不以错误的格式或速度继续写入。短写会保留剩余帧，消费位置仅计入实际接受的音频帧。预缓冲门关闭时不弹出 FIFO。

另一个独立缺陷在 `WavDumpOutput`：`pop_into_f32` 返回帧数，但导出只复制了 `popped` 个样本，应复制 `popped * 2` 个立体声样本。旧导出每块丢掉后半段，不能据此认定 HRTF 或解码器有失真。消费时钟本身以每声道 sample position（帧）计数，仍使用 `popped`，不乘 2。

此前关于 MuMu 回调模式损坏内存的断言也没有成立：旧 sine demo 将“回调结构体地址”传给了要求“函数指针”的 `AAudioStreamBuilder_setDataCallback`。跳进数据段与这处 ABI 声明错误相符，不能据此归罪于 Android 12/MuMu。当前保留阻塞写模式，未重新做回调模式设备实验。

## 修复版验证

- 精确 WAV 回归：2051 个 stereo frames，跨完整/尾块比较每个 i16 样本，无缺帧、无声道错位，通过。
- 消费时钟测试解除 ignore 后通过。
- `sda-native` 本次选定的 6 个功能测试通过；另 3 个原有整曲诊断测试未作为验收门禁运行。
- 既有正弦频谱回归通过。
- 新 `render_file` 示例按解码/消费位置节流整曲：解码帧数 **10,142,208**，消费帧数 **10,142,208**，WAV **40,568,876 bytes**，时长 **211.296 秒**，没有 rejection/queue-full。
- 对同一 E-AC-3 的前 64 秒，FFmpeg 与 SDA 解码的 100ms RMS 包络相关性为 **0.9999999935**；修正后的整曲渲染与 FFmpeg 的 10ms 包络相关性为 **0.9775**（对齐延迟 50ms）。这些是客观检查，不代替最终听感验收。
- APK 已实际安装到 `127.0.0.1:16416`。包内 `.so` 与 staged 文件 SHA-256 一致，标记 `ndk-f32-v1` 存在。
- MuMu 实际打开日志：`format=2(PCM_FLOAT) sample_bytes=4 frame_bytes=8 channels=2 rate=48000 burst=1026`。
- 静音设备验证已观测约 20 秒连续进度：`audio_frames=962388`、`partial_writes=0`、`xruns=0`。
- 随后设备日志获取超时并掉线，重连返回连接被拒绝。因此没有声称 MuMu 整曲完成、重开/停止或听感已验收。需要手动重新打开 MuMu。

IDE build 工具未连接 SDA 项目（只打开了另两个项目），未用于验证；实际 Rust/NDK/Gradle 编译均通过。

## 用户最终试听确认

设备重启后重新安装并打开修复版，用户随后明确反馈“可以了！！！”。因此，用户提供歌曲转为立体声 E-AC-3 后经 SDA 渲染、在 MuMu 播放的听感用例已通过。此确认不补充未采集到的 MuMu 整曲结束日志，也不扩大为 MP3 直接解码、Atmos 对象音频或真机验收。

已将可复用诊断步骤及历史误判整理为项目级 skill：
[`.agents/skills/sda-android-audio-debugging/SKILL.md`](../.agents/skills/sda-android-audio-debugging/SKILL.md)。

## 试听行为

正常打开 SDA Engine 不自动出声。点击“播放歌曲（低音量）”开始，默认应用音量 25%；有停止按钮，离开页面请求停止，由单一 JNI 调用线程释放引擎，worker/writer 收到停止标志退出。测试专用 intent `validationMuted=true` 才会静音自动跑通路。

## 重建

设置 `ANDROID_NDK_HOME`、`ANDROID_HOME`、`JAVA_HOME`，如不在 PATH 再设置 `SDA_GRADLE`，运行：

```sh
node scripts/build-android-engine-demo.mjs
```

脚本先执行实际 cargo-ndk release build，检查退出码，再从 cargo metadata 返回的 target directory 拷贝 `.so`，然后构建 APK。任何失败都会终止，不安装旧产物。校验清单写在 APK 同目录的 `build-verification.json`。

整曲离线导出（输出文件必须不存在）：

```sh
cargo +1.98.0 run --manifest-path crates/sda-native/Cargo.toml --release --no-default-features --example render_file -- <song.eac3> <hrtf-set.json> <output.wav>
```

上述测试只验证立体声 E-AC-3 声床经现有渲染链播放，不代表 MP3 直接解码或 Atmos 对象效果验收。用户原始 MP3 保持不变。
