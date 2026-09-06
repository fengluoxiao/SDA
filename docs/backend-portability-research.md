# Rust 音频后端跨平台可移植性调研

调查日期：2026-09-06。代码基线：`02a20b0`，分支 `feat/airpods-head-tracking`。

本文是代码审计、官方文档核对和移植设计，不代表已经完成非 Windows 平台移植。本次只添加文档，不改播放行为，也不开始头部追踪修复。

## 1. 结论

**macOS、Linux、iOS、Android 都有明确的技术移植路径，独立对象双耳渲染可以保留。** 最值得复用的是 Rust 空间路由、逐对象卷积、声床混音、房间处理和输出 DSP；需要分别适配的是音频设备、生命周期、文件访问、应用封装和头部姿态来源。

“双耳渲染已经在 Rust”不等于“整套播放器已经是独立 Rust 库”。现在桌面应用选用原生 Rust 作为实际发声后端，但解封装、解码驱动、PCM 提交调度和部分控制仍依赖 TypeScript/Electron；解码算法虽然也是 Rust，目前公开接口主要面向 WASM/JavaScript。

| 平台 | 判断 | 推荐第一条路线 | 主要阻碍 |
| --- | --- | --- | --- |
| macOS | 桌面移植可行，工作量中等 | Electron + 原生 Rust sidecar + CPAL/Core Audio | 可执行文件发现、资源打包、签名、公证、设备格式和切换恢复 |
| Linux | 桌面移植可行，工作量中等 | Electron + 原生 Rust sidecar + CPAL/ALSA，验证 PipeWire 环境 | 发行版依赖、音频路由、设备权限、打包兼容性 |
| iOS/iPadOS | 引擎复用可行，完整应用工作量高 | Rust 库 + Swift 音频会话/输出和应用外壳 | 原生解码调度、后台生命周期、内存和功耗、真机签名测试 |
| Android | 引擎复用可行，完整应用工作量高 | Rust 库 + JNI/Kotlin + Oboe 或经验证的 CPAL Android 输出 | 音频焦点、前台服务、设备差异、NDK/ABI、内存和功耗 |

这里的工作量是相对判断，不是工期承诺。没有目标设备性能测试，不能声称手机支持与当前 Windows 相同的最大对象数、房间卷积长度和持续运行功耗。

建议顺序：**先整理可复用边界，macOS 做桌面先导，Linux 完成第二种桌面输出验证，再推进移动端。** 当前只有 Windows 机器：Linux 可以先用虚拟机/CI 做编译和离线测试；Apple 平台需要补充 Mac/Xcode 构建环境及真实音频设备。模拟器、交叉编译成功都不能代替真机验收。

## 2. 当前架构到底在哪里运行

```mermaid
flowchart LR
    F[文件和应用控制] --> D[TypeScript 解封装]
    D --> W[Worker 内 Rust WASM 解码]
    W --> P[TypeScript 播放调度与短帧合并]
    P --> I[Electron IPC / 二进制 PCM 协议]
    I --> R[Rust 路由与独立对象 HRTF 卷积]
    R --> E[Rust 房间 / 耳机补偿 / EQ / 输出保护]
    E --> Q[已渲染立体声 FIFO]
    Q --> C[CPAL 音频回调]
    C --> O[系统音频设备]
```

控制命令和对象元数据还会经 IPC 送到 Rust，并按采样时钟生效；图示不是说这些信息只包含在 PCM 中。

| 层次 | 代码证据 | 当前状况与移植含义 |
| --- | --- | --- |
| 实际输出后端选择 | [`apps/web/src/App.tsx`](../apps/web/src/App.tsx)，创建播放器时指定 `outputBackend: "native-sidecar"` | 当前桌面实际发声由原生后端承担 |
| Web Audio 备用实现 | [`packages/player/src/player.ts`](../packages/player/src/player.ts)、[`packages/renderer/src/renderer.ts`](../packages/renderer/src/renderer.ts) | 仓库仍保留该实现；当前应用不能据此宣称在没有 sidecar 的普通浏览器中自动恢复播放 |
| 解封装 | [`packages/demux`](../packages/demux/src)，MP4 使用 `mp4box` | MKV/MP4 等容器读取并未整体迁到原生 Rust |
| 解码 | [`packages/core/src/lib.rs`](../packages/core/src/lib.rs)、[`packages/core/Cargo.toml`](../packages/core/Cargo.toml) | TrueHD、E-AC-3/JOC、DTS、ALAC 的 Rust pipeline 已存在；绑定仍引用 `wasm-bindgen`、`js-sys::Float32Array` |
| 播放调度 | [`packages/player/src/decoder.worker.ts`](../packages/player/src/decoder.worker.ts)、[`frame-batcher.ts`](../packages/player/src/frame-batcher.ts)、`player.ts` | 包含背压、ACK、预缓冲、播放时钟和短帧合并，移动端不能只移植最后的卷积器 |
| 空间音频引擎 | [`apps/native-renderer/src`](../apps/native-renderer/src) | `vbap`、`spatial`、`direct_renderer`、`bus_renderer`、`hrtf`、`convolution`、`cinema`、`focus`、`headphone`、`dsp` 可作为共享算法基础 |
| 设备和进程 | [`main.rs`](../apps/native-renderer/src/main.rs)、[`protocol.rs`](../apps/native-renderer/src/protocol.rs) | 引擎与 CPAL 初始化、工作线程、stdin/stdout 协议仍处于同一二进制 crate，尚无稳定 C ABI |
| 桌面接入 | [`apps/desktop/main.cjs`](../apps/desktop/main.cjs)、[`preload.cjs`](../apps/desktop/preload.cjs) | Node 子进程、IPC、文件对话框、设置和资源路径需要平台适配 |
| 房间生成 | [`apps/desktop/room-lab.cjs`](../apps/desktop/room-lab.cjs)、[`scripts/room-simulator.py`](../scripts/room-simulator.py) | Python 离线仿真与 Rust 实时房间渲染是两件事；迁移后者不等于手机可以直接生成房间 |

原生回调主要消费预先渲染的 FIFO，重 DSP 在渲染工作线程上执行。`main.rs` 顶部仍有早期架构注释，判断以 `spawn_render_worker`、`build_stream` 和实际调用为准，不能仅依据注释中的 WASAPI/JSONL 字样推断不可跨平台。

## 3. 已确认的跨平台基础

### 3.1 音频输出依赖已经具备对应后端

项目锁定使用 CPAL 0.15.3。该版本官方 README 和本机 Cargo 依赖源码确认：[S1]

| 系统 | CPAL 0.15.3 对应后端 |
| --- | --- |
| Windows | 默认 WASAPI，可选 ASIO |
| macOS / iOS | Core Audio，依赖 `coreaudio-rs` |
| Linux | ALSA，可选 JACK |
| Android | Oboe，涉及 JNI、NDK 和应用上下文 |

因此并非要为每个平台从零实现音频输出。但 **CPAL 不是完整播放器，也不代办后台会话、音频焦点、签名、沙盒或头部追踪**。其跨平台支持也不保证当前版本在最新移动系统和所有设备上开箱即用。

Linux 当前依赖不能直接描述为“原生 PipeWire 后端”。可以验证 ALSA 默认设备经 PipeWire 兼容配置输出；直接 ALSA 硬件设备、PipeWire 桌面和 JACK 是不同部署路径。PipeWire 文档确认图采样率和 quantum 可改变，兼容客户端可能经过重采样。[S2]

### 3.2 Rust 和算法依赖可覆盖目标架构

Rust 官方平台表包含 Apple、Linux、Android 目标，但 target 支持等级不等于 SDA 已通过编译或实时验证。[S3]

本项目主要算法使用 Rust、`serde`、`rustfft`。本机 `rustfft` 6.4 的默认 feature 包含 AVX、SSE、NEON；E-AC-3 的 `qmf.rs` 已有 `aarch64` NEON 分支和其他架构分支。这是复用基础，不是 ARM 与 x86 输出逐位一致的证明。

首批建议目标：`aarch64-apple-darwin`、`x86_64-unknown-linux-gnu`、`aarch64-apple-ios`、`aarch64-linux-android`。Apple Intel、Linux ARM64 和模拟器目标按实际用户需求追加。Android 先支持 `arm64-v8a`，不先承诺 32 位设备；现有大量 `AtomicU64` 等代码需要结合目标原子能力检查。

## 4. 必须解决的具体问题

### 4.1 启动和打包仍然绑定 Windows

- [`scripts/build-native-renderer.mjs`](../scripts/build-native-renderer.mjs) 在非 `win32` 时直接跳过；产物路径写死为 `.exe`。
- `apps/desktop/main.cjs` 查找 `SdaNativeRenderer.exe`，其他桌面平台需要按平台和架构定位产物。
- [`apps/desktop/package.json`](../apps/desktop/package.json) 虽有 macOS DMG 配置，但原生渲染器等 `extraResources` 放在 `win` 配置内。已有 DMG 配置不代表它包含可运行的原生引擎。
- macOS/Linux 要处理可执行权限、安装后只读资源目录和用户可写设置目录。移动端应将 Rust 嵌入应用，不能照搬 Electron 启动 sidecar 的方式。

### 4.2 48 kHz 假设与重采样

`main()` 仅选取至少双声道、支持 48 kHz 的设备格式；不满足就报错。`focus.rs` 等 DSP 系数也基于 48 kHz，HRTF 资产主要为 48 kHz。

更重要的是，`player.ts::ensureStreamRate` 在原生后端分支直接设置 ready 并返回；未找到该路径完整的输入重采样及 OAMD 时间映射实现。因此不能假设 44.1/96/192 kHz 音源在该原生链路中已被正确处理。这是现有跨采样率能力的待验证点，不是本次已经修复的问题。

建议保持内部渲染时钟 48 kHz，分开处理：

1. **输入侧**：将非 48 kHz 的解码 PCM 重采样到 48 kHz，同时用统一比例和累计相位映射对象事件、ramp、seek 和结束位置，避免逐帧四舍五入造成漂移。
2. **输出侧**：设备不接受 48 kHz 时，将最终双耳输出转换为实际设备格式；回报消费位置时从设备时钟映射回内部时钟，不能混用两种 sample index。
3. **设备切换**：重新查询实际 rate、channel count、buffer duration，重建输出适配层并保持内部音频与元数据同步。

Apple 官方明确说明 `preferredSampleRate`/buffer duration 是偏好，激活后实际值可能不同；路由切换也可能改变这些属性。[S4][S5] Android 同样应读取实际流配置；Oboe 的低延迟建议不能理解为所有设备必然按请求返回。[S8]

### 4.3 解码和调度要有可脱离 JavaScript 的接口

桌面先导版可以继续沿用 Worker/WASM + Electron，但 iOS/Android 的稳定后台播放不能把关键解码供给寄托在 WebView 页面和 JS Worker 的生命周期上。

移动端需要从 `sda-core` 拆出不依赖 JS 数组类型的 Rust 解码接口，复用现有 `Pipeline`、`FrameData` 和 `ObjectEvent`，让原生线程承担读取、解封装、解码、队列和背压。容器层可选择经过验证的库或维护现有最小容器功能的原生实现；本次没有选定新 demux 库，也没有证明系统解码 API 会提供所需的独立 Atmos 对象。

不得用“系统能播放这个文件”代替对象 PCM、对象 ID、位置、增益和时间戳的完整性验证。更换系统解码器可能只得到声道混音，不能无提示牺牲项目的独立对象特点。

TrueHD 40-sample 短帧合并方案也需迁移：合并传输/调度块时，必须保留原始对象事件时间、拓扑和增益边界，不能把所有事件统一挪到块首。

### 4.4 已渲染 FIFO 的延迟与头部追踪

当前常量按 48 kHz 换算：

| 项目 | 采样数 | 时长 |
| --- | --- | --- |
| 卷积分块 `DEFAULT_PARTITION` | 128 | 约 2.67 ms |
| 输出启动阈值 | 8,192 | 约 170.7 ms |
| 已渲染输出 FIFO 目标 | 16,384 | 约 341.3 ms |
| 输出 FIFO 容量 | 32,768 | 约 682.7 ms |

这些是配置值，不是端到端延迟实测值。设备缓冲和蓝牙还会增加延迟。

必须区分“解码后的未渲染 PCM 预读”与“已经做完 HRTF 的立体声预渲染”。前者可以保留足够缓冲；后者已经包含旧姿态，后来的转头无法直接改写它。当前约 341 ms 的预渲染目标明显不适合作为低延迟头部追踪的最终设计。

后续应使控制/姿态尽量在接近实际输出时作用于渲染，分别设置解码和输出缓冲，并测量姿态时间戳到输出响应的延迟。不能只减 CPAL 请求的硬件 buffer，也不能在没有性能余量时盲目缩小全部缓冲。

`App.tsx` 当前还传入 yaw 模式、约 220 ms 平滑等头姿参数；这不是平台移植自动能解决的问题，留待专门追踪任务校正坐标、pitch/roll、时间戳、平滑与重置逻辑。

### 4.5 移动端内存和持续性能

`MAX_PENDING_SAMPLES = 480_000`；每个 `Source` 都创建一个 `AbsolutePcmRing`，槽位保存 `u64 clock` 和 `f32 sample`。按常见 64 位 ABI 的 16 字节槽位估算：

| 源数 | 仅源 PCM 环形缓冲估算 |
| --- | --- |
| 1 | 约 7.32 MiB |
| 16 | 约 117.19 MiB |
| 64（当前上限） | 约 468.75 MiB |

这是结构布局计算，不是进程 RSS 实测，尚未计入解码器、PCM 副本、HRTF、FFT 历史、房间数据和界面。目标 ABI 上应以 `size_of` 和内存分析器复核。

建议让缓冲时长和源容量可配置，控制跨语言 PCM 复制次数，明确资源共享和回收时机。保持对象独立不意味着必须给每路固定分配 10 秒带时钟标签的缓冲。

本次对 `apps/web/public/hrtf*` 目录下直接文件的统计为 1,647 个文件、68,458,322 字节，约 65.29 MiB；这是当前资源目录大小，不是压缩包、全应用体积或加载后的内存。移动版可按所选 HRTF 加载，房间数据应有大小上限及格式版本。

已有 Windows 聚焦修复记录显示：合并前景/背景卷积并清除低通的极小尾值后，聚焦开启/取消的短时复播未新增源缺音或输出 FIFO 缺音；测试阶段平均每 128-sample 渲染块约 1.5 ms。该结果仅说明当时 Windows 测试成功，不能外推到 ARM 手机。手机必须测长时间热降频、功耗、最坏对象运动和房间长度。

### 4.6 实时线程与资源更新

保留现有“命令/解码工作线程 + 渲染线程 + 轻量输出回调”的分层，但进一步审计实时期限：

- 回调不能执行文件 IO、JSON、JNI/Swift 高频对象分配、等待锁或阻塞网络。
- HRTF/房间加载、滤波准备、缓存构建应在非实时路径完成，切换时采用准备好的状态和受控过渡。
- 当前滤波缓存和动态滤波混合存在复制/分配，不能因为代码是 Rust 就声称整个渲染循环已经完全无分配。
- `StereoFifo` 使用 `UnsafeCell` 和 acquire/release 原子，必须验证单生产者/单消费者约束、flush/暂停/重启竞争，特别是 ARM 的内存排序。
- 移动端音频线程优先级、系统中断和输出设备断开处理要由平台层验证；CPAL 抽象不会自动保证所有渲染工作线程具有足够调度优先级。

### 4.7 资产路径和房间仿真

HRTF 当前通过文件路径读取，dense KU100 还依赖同级标准方向目录；不能搬走一个目录后丢掉另一个。平台资源接口应保持这类依赖，或显式将资产集合交给引擎。

macOS/Linux 可将资源放到应用只读目录。iOS 使用 bundle/沙盒；Android 的 APK asset 不应假设是普通可寻址文件，应通过字节读取接口或受控解包接入。导入房间和媒体需适配 iOS 文档访问及 Android 内容 URI/文件描述符。

房间仿真当前要求机器本地 Python、依赖、测量源和路径配置，不包含在通用 Rust 实时引擎中。推荐移动版先支持读取桌面生成的房间档案；手机现场生成房间是后续独立功能，不能默认为此移植已经包含。

## 5. 各平台实施方案

### 5.1 macOS

1. 在 Mac/Xcode 环境构建 Apple Silicon sidecar，按需求补 Intel 或 universal 产物。
2. 改正平台化二进制命名、资源发现和 Electron 打包，验证从安装后的 `.app` 启动，而非只在源码目录运行。
3. 用 CPAL/Core Audio 验证内置输出、USB DAC、蓝牙耳机、默认设备更换和睡眠唤醒；补充错误后的重建状态机。
4. 分别签名应用和内嵌可执行文件，完成 Developer ID、公证和分发检查。公证不是 Mac App Store 审核；若后续走商店，还需另行验证沙盒和文件访问。[S6]
5. 保持 Rust 生成的最终双耳立体声作为输出；不能把接入 Core Audio 描述为自动获得 Apple 自家的 Atmos 解码或空间化算法。

头部追踪后续可考虑 `CMHeadphoneMotionManager`。官方文档标注 iOS 14、macOS 14 起支持，要求查询 `isDeviceMotionAvailable`，并配置 `NSMotionUsageDescription`。[S7] 这条原生 API 路线不同于现有 Windows helper；不保证任意蓝牙耳机都有姿态数据。

### 5.2 Linux

1. 建立本地/CI 构建，安装 ALSA 开发依赖；CPAL 0.15.3 README 明确列出 Debian/Ubuntu 的 `libasound2-dev` 和 Fedora 的 `alsa-lib-devel`。[S1]
2. 平台化启动路径与资源，选择实际维护的包格式，明确 glibc/发行版基线；不能只在开发机链接成功就视为可分发。
3. 分别测试 PipeWire 桌面默认路由、直接 ALSA/USB 和可选 JACK。兼容层的采样率转换、设备独占/占用和 quantum 变化需要实测。
4. 验证没有可用设备、蓝牙切换、设备拔出、睡眠恢复和应用正常退出；Linux 音频权限与 Electron 图形环境分开排障。

桌面 ARM64 也有希望，但编译通过之外必须测 NEON 解码路径和实时负载。Linux 无统一、可保证任意耳机可用的头姿入口，本次不承诺 Windows AirPods helper 可移植。

### 5.3 iOS / iPadOS

1. 把 DSP 做成 Rust 库，通过 C ABI 提供给 Swift；可以产出静态库/XCFramework，区分真机与模拟器 slice。不能把现有 Electron 应用直接部署到 iOS。[S9]
2. Swift 管理 `AVAudioSession` 的 playback 类别、激活/停用、中断、路由改变和实际硬件格式。输出可先验证 CPAL/iOS，或者让平台输出回调消费 Rust PCM；同一链路只设一个输出和会话所有者。
3. 原生线程负责持续读取、解码和供给。配置合法的后台音频能力与锁屏媒体控制；不能依靠持续显示网页或屏幕常亮实现后台播放。[S10]
4. 降低内存上限、限定合理房间/资源尺寸，使用真实 iPhone/iPad 测锁屏、电话、Siri、耳机插拔、应用进入后台及长时间播放。
5. 保持应用代码自包含、使用受支持的应用模型。App Review 2.5.2、2.5.4 对下载执行代码和后台用途的限制需在分发前核对；不是说读取用户音频或下载合法 HRTF 数据本身被禁止。[S11]

原型成功不等于可以上架；资产许可、应用许可、审核和性能是不同的验收项。

### 5.4 Android

1. 构建 `arm64-v8a` 的 Rust `.so`，用 JNI 提供少量批量接口；Kotlin/Java 负责 UI、文件选择、播放服务和通知。
2. CPAL 0.15.3 已使用 Oboe，但需要审计它携带的 Oboe/NDK 版本和应用上下文初始化。若必须升级或直接接 Oboe，先做输出适配器原型，不能只因 Rust 能编译就认定流启动正确。
3. 采用数据回调、低延迟性能模式，读取实际 burst/buffer/rate，响应断开并重建。独占模式是请求，不保证授予。[S8]
4. Android 的低延迟资料面向游戏，其中 `USAGE_GAME` 不是 SDA 音乐播放器必须照抄的配置。媒体播放应使用与产品用途一致的 AudioAttributes，并验证对应的延迟和行为。
5. 实现系统音频焦点、失焦暂停/ducking、耳机拔出和前台媒体播放服务。目标 API 35 及以上请求音频焦点需要位于前台或运行符合要求的前台服务。[S12]
6. 可用 MediaSessionService 管理后台媒体会话，但接入自有 Rust 解码引擎还需要播放器接口适配，不是直接把示例 ExoPlayer 替换成 `.so` 即可。[S13]
7. 检查所有本地共享库的 16 KB page-size 兼容性。官方文档说明 NDK r28 及以上默认产生相应对齐；Rust 链接器、传递依赖和 APK 打包仍需逐项验收，不能仅检查主 `.so`。[S14]

Android 的 `TYPE_HEAD_TRACKER` 是平台传感器类型，不是“任意普通应用必然能拿到所有耳机姿态”的保证。[S15] 后续需要按系统、设备和可访问性调查；手机自身陀螺仪不等于头戴耳机姿态。

## 6. 推荐的共享引擎边界

以下是目标设计，不是已经存在的模块名或接口：

```text
sda-decode       Rust 解码及类型，不依赖 js-sys
sda-media       原生文件/容器与播放调度，保留对象时间线
sda-render      路由、独立对象 HRTF、房间、混音、输出 DSP
sda-host-*      CPAL / Apple / Android 设备和生命周期适配
sda-ffi         面向 Swift/JNI 等宿主的稳定边界
desktop-shell   暂时保留 Electron 和现有 IPC
```

先把现有二进制中的引擎抽成库，让 Windows 继续调用同一实现，再添加其他平台。不要同时重写 UI、解码器和 DSP，以免无法判断移植偏差来自哪一层。

接口必须明确：

- 使用固定宽度整数和明确单位：源 ID、内部 sample clock、事件采样时间、四元数方向和数据布局。
- 提交 PCM 时同步提交/调度该时间段所需的对象声明与事件；保留 ACK、容量背压、EOF、seek 和 discontinuity 语义。
- 错误通过状态码/结构返回；不让 Rust panic 穿越 FFI，不暴露 Rust `Vec`、`String` 的内存布局。
- 区分所有权和寿命：谁分配、谁释放、PCM 是否复制、提交返回后调用者是否可复用内存。
- 音频回调只处理预分配缓冲和确定边界的工作；日志和 UI 遥测走低频通道。
- 姿态接口带单调时间戳和坐标定义，区分耳机姿态与设备姿态；平台控制不能自行改变对象 authored position。
- 现有 sidecar 二进制协议可作为桌面宿主适配，移动端无需为复用协议而把 JSON/管道放进音频实时链路。

## 7. 保持声音与功能一致的验收标准

以下为建议验收任务，未在其他平台执行：

| 维度 | 验证内容 | 通过条件 |
| --- | --- | --- |
| 解码完整性 | 相同授权测试文件的 PCM、对象 ID、稀疏声明、OAMD 时间戳、gain/ramp、EOF | 不丢对象，不把对象退化成固定床层；差异有可解释依据 |
| 跨采样率 | 44.1/48/96 kHz 输入、设备 rate 改变、seek 后恢复 | 时长/音高正确，对象事件与 PCM 不漂移 |
| 空间路由 | 所有已有布局，逐床声道和对象移动，包括前后、上下 | 与 Windows 离线参考方向和增益一致；保留布局差异 |
| 聚焦/solo/mute | 单选、多选、互斥、连续切换、带移动对象 | HRTF 未旁路、无卡顿、取消后无残留额外渲染开销 |
| 资源与房间 | KU100 校准开关、D2 等主体、测量房间、资源缺失 | 使用正确资产，无静默回退到错误方向或其他主体 |
| 数值回归 | 冲激、扫频、持续信号、对象移动、长尾和静音恢复 | 对照误差阈值按测试定义；不要求 SIMD 间所有样本逐位一致 |
| 实时性 | 正常、最坏对象数、频繁路由、长房间和切换 | 测试有效播放区间内源缺音/输出缺音计数不增长，记录 p95/p99/max 耗时 |
| 内存/热状态 | 16 路及目标上限、30 分钟持续播放、后台 | 有可公布的设备预算，内存稳定，无热降频后的持续缺音 |
| 生命周期 | 锁屏、来电、音频焦点、耳机断连、默认设备切换、休眠 | 可预测暂停/恢复，不泄漏会话，不双重输出 |
| 头部追踪 | 三轴/回中、传感器断连、时间同步、转头响应 | 测到真实端到端延迟；不把预渲染延迟误当传感器漂移 |
| 分发 | 安装目录运行、签名、沙盒、Android 16 KB page size | 目标安装包在干净设备能运行，资源和依赖完整 |

先提供离线 `PCM + sample-timed events -> stereo PCM` 测试入口，作为平台无关参考，再对设备输出做 loopback/外部测量。系统空间化、耳机自带效果和蓝牙链路应记录为实验条件，避免双重空间化造成不可比结果。

## 8. 分阶段交付及停止条件

| 阶段 | 可交付成果 | 完成判据 |
| --- | --- | --- |
| P0：接口整理 | 从现有 sidecar 抽出引擎库；统一时钟/所有权；离线测试入口 | Windows 行为与现有测试不退化 |
| P1：macOS 先导 | Apple Silicon 原生后端、完整桌面包、设备生命周期 | 目标 Mac 实机播放和离线对照通过，签名包可启动 |
| P2：Linux | ALSA/PipeWire 桌面验证、明确支持发行版与包 | 干净系统安装及 USB/蓝牙/切换测试通过 |
| P3：移动基础 | 原生解码调度、FFI、输入/输出采样率处理、可配置内存 | 不依赖 WebView 存活也能持续供给音频 |
| P4：移动应用 | iOS 会话/后台、Android 服务/焦点/NDK 集成 | 真机长时间与生命周期测试通过 |
| P5：头姿与优化 | 平台姿态源、低延迟渲染调度、功耗优化 | 定位和真实延迟可测，性能达预算 |

Windows 头部追踪修复可独立推进，不要求先做完移植。但其坐标和时间语义应与共享引擎设计一致。

若手机的目标对象数与房间长度无法满足实时预算，应先优化内存、共享滤波资产、预计算和线程调度；必要时提供明确标注的房间质量档位，保留直接声和对象方向。不允许无提示地关闭对象、合并为立体声或关闭 HRTF 来宣称移植成功。

## 9. 分发与许可边界

这是完整应用移植的一部分，不能由“Rust 可以链接”推出可自由分发：

- `apps/native-renderer/Cargo.toml` 当前声明 `GPL-3.0-or-later`；`sda-core` 及所用本地解码 crate 声明 `Apache-2.0`。尤其移动端嵌入同一应用后，需要明确组合分发的许可履行方式和商店条款；本次未给出法律可分发结论。
- 当前 HRTF manifest 记录 SADIE II V2.2 来源、DOI 和 `Apache-2.0`。发布前应核对所带原始数据及派生资产对应版本的许可/署名，不能将别的房间数据自动归入同一许可。
- 能自行解码 TrueHD/E-AC-3/DTS，不等于获得 Dolby/DTS 品牌认证、专利许可或系统受保护媒体的访问权限。移植功能验证与商业发行资格分别处理。
- 不把 Apple/Android 文档中的原生空间音频能力写成 SDA 已调用该私有算法，也不承诺普通平台 API 一定输出独立 Atmos 对象。

## 10. 本次实际验证范围

已完成：审计当前提交的架构、配置与关键实现；核对本机 CPAL 0.15.3 与 rustfft 6.4 的依赖源码；读取下列官方网页；计算固定缓冲与资产规模；整理已有 Windows 复播测试的边界。

未完成：任何 macOS/iOS/Linux/Android 构建、目标设备音频输出、签名包、真机功耗或跨架构数值对照。本机安装的 Rust target 只有 `x86_64-pc-windows-gnu` 和 `wasm32-unknown-unknown`。本次没有为了写报告安装 SDK、改变工具链或运行新的移植代码。

因此本文结论是“有经过代码和官方接口支持的可行路径”，不是“四个平台已经支持”。

## 11. 官方资料与核对方式

以下资料于 2026-09-06 访问。Apple 的 QA/归档指南用来解释 API 语义，不作为最新最低系统版本或商店政策的唯一依据。Android 主域名连接失败的页面，使用 Google 官方 `developer.android.google.cn` 文档读取英文正文；相关限制以实施时的目标 SDK 和最新官方要求复核。

- [S1] [CPAL 0.15.3 官方 README](https://github.com/RustAudio/cpal/blob/v0.15.3/README.md)、[对应版本 API](https://docs.rs/cpal/0.15.3/cpal/)：平台后端、Linux 依赖、回调模型；同时核对本机该版本 `Cargo.toml`。
- [S2] [PipeWire daemon 官方文档](https://docs.pipewire.org/page_man_pipewire_1.html)：图采样率、quantum 和兼容客户端重采样。
- [S3] [Rust Platform Support](https://doc.rust-lang.org/rustc/platform-support.html)：目标平台与支持分级。
- [S4] [Apple QA1631：Audio Session Preferences](https://developer.apple.com/library/archive/qa/qa1631/_index.html)：偏好值与激活后实际硬件参数的区别。
- [S5] [Apple：Responding to Route Changes](https://developer.apple.com/library/archive/documentation/Audio/Conceptual/AudioSessionProgrammingGuide/HandlingAudioHardwareRouteChanges/HandlingAudioHardwareRouteChanges.html)：路由变化后的采样率、缓冲与通道参数。
- [S6] [Apple：Notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)：签名、公证和分发；核对 Apple 文档 JSON 正文。
- [S7] [Apple：CMHeadphoneMotionManager](https://developer.apple.com/documentation/coremotion/cmheadphonemotionmanager)：可用性检查、坐标轴、隐私说明和支持系统；核对 Apple 文档 JSON 的平台元数据。
- [S8] [Android：Low-latency audio](https://developer.android.google.cn/games/sdk/oboe/low-latency-audio?hl=en)：Oboe、回调、性能模式、实际配置和避免阻塞。该文面向游戏，不能照搬用途属性。
- [S9] [Electron 官方介绍](https://www.electronjs.org/docs/latest/)：支持 Windows、macOS、Linux 桌面应用，不提供 iOS/Android Electron 应用运行时。
- [S10] [Apple：Configuring Audio Settings](https://developer.apple.com/library/archive/documentation/AudioVideo/Conceptual/MediaPlaybackGuide/Contents/Resources/en.lproj/ConfiguringAudioSettings/ConfiguringAudioSettings.html)：播放类别和后台音频能力。
- [S11] [App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)：核对 2.5.2、2.5.4，分发时需复核最新版本。
- [S12] [Android：Manage audio focus](https://developer.android.google.cn/media/optimize/audio-focus?hl=en)：音频焦点及目标 API 35 以上的前台条件。
- [S13] [Android：Background playback with MediaSessionService](https://developer.android.google.cn/media/media3/session/background-playback?hl=en)：媒体会话、服务与 `mediaPlayback` 类型。
- [S14] [Android：Support 16 KB page sizes](https://developer.android.google.cn/guide/practices/page-sizes?hl=en)：NDK、ELF 对齐和原生依赖验证。
- [S15] [Android Sensor：TYPE_HEAD_TRACKER](https://developer.android.google.cn/reference/android/hardware/Sensor#TYPE_HEAD_TRACKER)：头部追踪传感器类型定义；不据此推断具体耳机和应用的访问能力。

相关项目记录：[双耳渲染说明](binaural-rendering.md)、[影院房间渲染](cinema-room-rendering.md)、[房间实验室](room-lab.md)、[高度与对象定位研究](symbol-height-rendering-research.md)。
