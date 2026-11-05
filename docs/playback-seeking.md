# 播放进度跳转

## 桌面端原生文件路径

Windows 和 macOS 使用 `sda-desktop-player`，打包后沿用 `SdaNativeRenderer` 文件名。它在同一个 Rust 进程中持有原文件、访问单元索引、解码器、渲染队列和音频输出。Electron/JS 发送打开、暂停、定位、参数设置等控制，接收时钟、标签及对象状态；此路径不创建 WASM 解码 Worker，不把播放 PCM 传给 JS。

进度条支持点击定位、按住拖动后松手定位，以及键盘定位。拖动过程只预览时间。每次定位带独立 epoch；旧会话或旧定位的返回不能覆盖最新进度和空间对象。暂停时定位保持暂停，正在播放时定位自动继续。

| 本地输入 | 原生策略 |
| --- | --- |
| ALAC、MP3、AAC/M4A、16/24-bit FLAC、AIFF、Vorbis | 原生容器/帧定位，短距离恢复解码状态；保持原始采样率信息并转换到 48 kHz 输出 |
| WAV / RF64 / BW64 / ADM | JS 在工作线程中只解析一次文件元数据。Rust 校验实际 PCM 格式、范围和映射，持有对象自动化，按文件偏移直接读取目标附近 PCM。定位不重新解析 ADM/XML |
| E-AC-3 / Atmos、Audio Vivid | 根据容器时间戳或原始帧索引定位，恢复解码器、SRC 相位及对象时间线 |
| MPEG-H / 360RA | 根据 MHAS 独立帧索引恢复配置/场景。支持的非分片 MP4 以虚拟读视图访问原样本，不另行提取音轨文件 |
| TrueHD、DTS、AC-4 | 根据经验证的独立访问单元/容器索引恢复依赖；依赖帧不能被当作独立帧直接播放 |
| SDAR、SDA M4A、SDA WAV 母版 | 复用源文件及其调优/对象资源，原生定位。只有 WAV 母版允许编辑房间；其编辑接口只提供元数据与资源，播放 PCM 仍由 Rust 读取 |

原生能力按文件探测。未支持的码流变体（例如当前 Symphonia 不支持的 32-bit FLAC、缺少可验证访问点的容器）保留原来的兼容播放路径，不假装具备原生跳转。定位耗时仍受文件读取、访问点间隔、解码器预热和设备输出缓冲影响，不保证所有文件固定零延迟。

## 响度、对象和存储

播放前需要的整曲参考由独立原生分析子进程产生，只返回测量值。切歌/取消会结束该进程；定位不会重新分析整曲，也不会用目标附近的局部响度替换已选定增益。支持的 Audio Vivid MP4 可以直接访问原样本或原生拆包，不输出临时音轨。

PCM、渲染音频和整曲分析结果均不写入持久缓存。仅保留当前源的索引、对象/标签元数据、测量标量及有上限的实时缓冲。ADM 元数据分块传送，单块小于控制协议上限，自动化总量有明确上限。停止、替换或释放源时释放文件和状态；SDAR 当前会话需要的房间/HRTF 等临时资源由其持有者清理。

对象事件和音频使用同一绝对 48 kHz 播放时钟。重新定位会恢复中断的对象位置/增益/大小包络和剩余 ramp。原生状态持续推送对象及发声对象 ID，JS 无需靠暂停/恢复触发对象刷新。

## 验证

- `node --test apps/desktop/test/native-file-playback.test.cjs apps/web/test/desktop-progress.test.mjs packages/player/test/native-file-seek.test.mjs`：控制关联、拖动松手、键盘、旧结果隔离、暂停意图、失败回退、停止/释放，以及原生路径不分配解码 Worker。
- 设置 `SDA_NATIVE_PLAYER_TEST` 为当前平台的原生二进制后运行 `node --test apps/desktop/test/native-programme-reference.test.cjs`：真实 ALAC/MP3/Audio Vivid 原生分析、MP4/裸流参考一致、取消和无音频中间文件。
- `cargo test --manifest-path crates/sda-native/Cargo.toml --lib desktop::tests -- --test-threads=1`：真实解码和受控输出回调，包括暂停定位、连续定位、短文件结束和音频位置比较。可用 `SDA_DESKTOP_SEEK_FIXTURES` / `SDA_DESKTOP_PCM_FIXTURES` 提供本地额外样本的 JSON 路径数组。
- `desktop_src::tests`、`desktop_wave::tests`、`mp4_extract::tests`：44.1/96/192 kHz 的时钟/相位/尾部、PCM 文件与对象 ramp 恢复、虚拟压缩视图随机读取与提取结果逐字节一致。
- `seek_resource_tests`（设置 `SDA_SDAR_SEEK_FILE`）：SDAR 定位不重建或遗留当前资源目录。
- `.github/workflows/desktop-native-seek.yml`：Windows x64、macOS x64/arm64 构建并运行原生回归；Windows ARM64 交叉编译二进制及测试程序，不能当作 ARM64 真机测试。

2026-11-05 的本地验证包括 Windows 实际输出设备回调：暂停定位保持 4,000 ms，恢复及多次连续定位正常推进，ADM 对象和发声 ID 在定位后继续出现。生成的 96 kHz TrueHD 和 DTS 样本通过定位/连续解码的音频比较。以上结果不替代 macOS 或移动端真机验收。
