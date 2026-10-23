---
name: sda-android-audio-debugging
description: 排查 SDA Android/Rust/JNI/AAudio 播放的电音、爆音、泡泡声、静音、短音频、进度增长但听不清音乐、重开崩溃及旧 APK/so 问题。用于 MuMu 或真机音频联调、PCM 格式与帧单位检查、构建产物核验和听感验收；优先核对 NDK ABI 与输出格式，避免误判为 HRTF、原曲或模拟器问题。
---

# SDA Android 音频诊断

本 skill 服务于本仓库的 Android 音频链路。目标是用可复现证据定位故障、修复并交付安全试听，不把“进程活着”“流打开”“有声音”当作音频正确。

## 开始前

1. 阅读 `docs/android-pcm-format-validation.md`；需要案例证据、历史误判或单位示例时阅读 [references/incident-and-checklist.md](references/incident-and-checklist.md)。路径以项目根目录为基准。
2. 查看工作树，保留用户及其他会话的改动。确认当前源码、设备和构建脚本，不把旧会话里的实现状态当事实。
3. 若应用正在制造电噪声，先停止本测试应用。不要重启模拟器、修改安全软件或切 Android 版本来代替定位。
4. 区分事实与假设。每项假设给出能区分它与其他原因的检查；失败则撤回判断，不叠加未经验证的算法改动。

## 1. 优先检查 PCM 与 FFI 边界

先读 `apps/native-renderer/src/aaudio_output.rs`，对照实际 NDK 的 `aaudio/AAudio.h` 或 `ndk-sys` 绑定：

- `AAUDIO_FORMAT_PCM_I16 = 1`，`AAUDIO_FORMAT_PCM_FLOAT = 2`。使用官方绑定，不手写猜测常量。
- `Vec<f32>` 的字节只能写入实际授予的 float 流；若请求整数流，则先数值转换成整数 PCM，不能只转换指针。
- 打开后记录并校验实际 format、channels、sample rate、burst、每样本/每帧字节数。当前引擎输出约定为 48 kHz、2 声道、f32：每样本 4 字节，每帧 8 字节。
- 格式不匹配则停止并报错，或实现明确的转换层；不允许退回任意设备采样率后以错误速度继续播放。
- `AAudioStreamBuilder_setDataCallback` 接收**函数指针**，不是装着回调的结构体地址。崩溃 PC 位于数据段时，先核对函数签名、指针层级和生命周期，不据此认定 MuMu 实现有 bug。
- JNI 句柄的访问与释放不得并发。错误不能用 UI 的“playing”掩盖；避免 panic 穿越 FFI。

## 2. 明确帧、样本、字节单位

| 量 | stereo f32 | stereo i16 WAV |
| --- | --- | --- |
| N 帧包含的样本 | N × 2 | N × 2 |
| N 帧包含的字节 | N × 8 | N × 4 |
| 48 kHz 下的时长 | N / 48000 秒 | N / 48000 秒 |

`StereoFifo::pop_into_f32` 返回帧数：

```rust
let frames = fifo.pop_into_f32(&mut block, 2);
interleaved.extend_from_slice(&block[..frames * 2]);
consumed_sample_pos.fetch_add(frames as u64, Ordering::Release);
```

消费时钟是每声道样本位置，即帧数，**不乘声道数**。`AAudioStream_write` 参数和返回值也都是帧数；部分写入后保留未写入数据，指针偏移为 `written_frames * channels`，不能丢尾部继续取新块。

100ms、48 kHz、立体声 i16 的包络窗口为 `4800 * 2 * 2 = 19200` 字节。不要把字节窗当样本窗。

## 3. 核对调度与数据完整性

检查 `crates/sda-native/src/lib.rs`、`render_command.rs`、`stereo_fifo.rs`：

- AddSource 与 PcmFrame 使用同一 ID 映射，例如 `bed:FrontLeft` 和 `obj:10`。遇到 rejection，输出实际 key 和失败条件，不猜 ring 已满。
- 喂数依据 `decodedSamplePos - consumedSamplePos` 背压。字段缺失是版本/接口不兼容，不能默认填零后无限喂数。
- 命令队列失败必须报告或保留重试，不能静默丢帧。UI 的解码进度不等于实际消费进度。
- 阻塞 `AudioOutput::run` 在自己的线程中运行，不能阻挡提交命令。停止时通知 worker/writer，释放流。
- 输出消费者确认 flush epoch；预缓冲/暂停门关闭时不弹出 FIFO。仅按实际接受的音频帧推进消费位置。
- 用实际代码核对渲染上限。`coverage.available(..., DEFAULT_PARTITION)` 已有上限，不能宣称它返回数万帧而没有检查实现。

## 4. 先验证测量工具，再隔离失真

按顺序测试，尽量使用同一段源音频：

1. **导出工具**：运行 `apps/native-renderer/tests/wav_output.rs`，逐样本检查跨完整块和尾块的 stereo 数据、WAV 长度和时钟。
2. **解码**：以 FFmpeg 独立解码同一个码流作为参考，核对声道标签、采样率、总帧数、非有限值、削波比例以及对齐后的波形/包络。峰值稍大于 1 不能单独证明持续电噪声来自削波；低峰值也不能证明音质正确。
3. **渲染**：用 `crates/sda-native/examples/render_file.rs` 导出完整曲目，检查解码/消费/导出帧数一致和无 rejection。再做时延对齐后的频谱、波形或包络对照。
4. **输出**：核对设备实际格式及写入计数、短写、欠载、音轨状态。主机文件正常不代表设备输出正常。

无 HRTF 不等于旁路：当前 `render_chunk` 在 `bus_renderer.is_none()` 时直接返回静音；默认 `VbapSolver::new()` 也已有 7.1.4 布局。需要隔离卷积时用恒等脉冲响应或明确的旁路，不用全零输出证明其他层无错。

包络相关性必须使用**相同内容、正确单位、正确时延**。单频测试通过仅证明该路径下该信号通过，不推出所有曲目和设备路径已保真。

## 5. 构建及安装闭环

确认这些文件仍存在后使用已有脚本，不重复手搭流程：

- 构建：`scripts/build-android-engine-demo.mjs`
- 测试：`apps/native-renderer/tests/wav_output.rs`
- 完整导出：`crates/sda-native/examples/render_file.rs`
- 实际播放：`apps/android-engine-demo/android/app/src/main/java/com/sda/engine/MainActivity.kt`

```sh
# 在项目根目录；先配置实际工具链路径。
export ANDROID_NDK_HOME='<installed-ndk>'
export ANDROID_HOME='<android-sdk>'
export JAVA_HOME='<jdk>'
export SDA_GRADLE='<gradle-or-gradle.bat>'
export MACINDECODE_AC4_SPEC_DIR="$PWD/tmp/MacinDecode-AC4-Core/spec"
node scripts/build-android-engine-demo.mjs

cargo +1.98.0 test --manifest-path apps/native-renderer/Cargo.toml --no-default-features --test wav_output
cargo +1.98.0 test --manifest-path crates/sda-native/Cargo.toml --no-default-features playback_status_tracks_consumed_clock
cargo +1.98.0 run --manifest-path crates/sda-native/Cargo.toml --release --no-default-features --example render_file -- '<input.eac3>' '<hrtf-set.json>' '<new-output.wav>'
```

- `cargo check` 不产出可部署 `.so`。构建失败后不复制旧 so、不安装旧 APK。shell 用 `set -euo pipefail` 或逐个检查真实退出码，别让 `tail/grep` 的成功覆盖编译失败。
- 用 `cargo metadata` 查询 target directory，考虑 `CARGO_TARGET_DIR`。核验编译库、staged 库、APK 内库及实际设备日志的版本标记；检查 APK 确实包含目标 ABI 和新资产。
- `build-verification.json` 在 APK 输出目录记录摘要。AGP 若 strip 符号导致哈希不同，则与 stripped 中间产物或 ELF build ID 核对，不能直接误判。
- HRTF 清单引用相对 FIR 文件，需要完整目录。自定义 `jniLibs.srcDir` 相对 Gradle 模块解析；以实际 APK 内容为证，不能断言自定义目录一律不支持。
- 用户歌曲及其转码资产仅作授权的本地测试；未经要求不要提交、推送或上传歌曲。提交代码不等于授权推送。

## 6. MuMu 验证与用户听感

本机由用户指定：MuMu Android 12 / API 32，优先 x86_64；**不要启动官方 `emulator.exe`**。adb 通常不在 PATH：

```sh
ADB='C:/Users/jzh/AppData/Local/Android/Sdk/platform-tools/adb.exe'
"$ADB" connect 127.0.0.1:16416
"$ADB" -s 127.0.0.1:16416 install -r '<verified.apk>'
"$ADB" -s 127.0.0.1:16416 shell am start -W -n com.sda.engine/.MainActivity
"$ADB" -s 127.0.0.1:16416 logcat -d -s SdaAAudio SdaEngine AndroidRuntime
```

固定 serial `127.0.0.1:16416`，不混用 5557。连接被拒绝时提醒用户手动打开 MuMu；重连不成功就如实说明阻塞，不循环重启系统。

先静音检查数据链（demo 支持 `--ez validationMuted true`），正常启动保持待播放状态，让用户主动点击低音量播放；停止、离开页面和重开也要检查。避免自动循环纯音或突然播放，用户已多次被吓到。

最终汇报包含：

- 已证实的根因与修改，而非猜测。
- 具体通过/失败/忽略的测试，实际设备格式和产物标识。
- 听感是否经用户明确确认，以及尚未验证的范围。

例如：“已确认整数/浮点格式错配并修复；本机整曲帧数一致；MuMu 日志授予 PCM_FLOAT(2)。APK 已安装且不会自动播放。请点击低音量播放确认原曲清晰度。真机及 Atmos 对象效果未验收。”

只有用户明确表示音质恢复（本次为“可以了！！！”）才标记该听感用例通过。它不等于 MP3 直接解码、所有音源、Atmos、真机或全部移植任务通过。
