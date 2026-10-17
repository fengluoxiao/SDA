# Android 移植开发计划

调查日期：2026-09-26。代码基线：`7ee75f2`（master）。

本文是可执行的开发计划，不是可行性调研（调研见
[backend-portability-research.md](backend-portability-research.md)、
[mobile-native-module.md](mobile-native-module.md)）。任务拆解到原子粒度：
**每个任务只做一件事、有唯一产出物、有可独立执行的验收标准**，完成后不依赖
"顺手做"的隐式工作即可标记完成。

## 0. 范围与非目标

**目标**：SDA 在 Android（先 arm64-v8a）上实现与桌面一致的核心链路：

```
文件(SAF) → TS 解封装(RN/JS) → JNI 喂数 → Rust sda-native 解码+VBAP/HRTF 卷积
→ AAudio 低延迟输出（PCM 不出 native 层）→ JS 侧 66ms 对象事件 → R3F 3D 视图
```

**非目标（本计划不做）**：
- iOS（另走 AVAudioEnvironmentNode 路线，见 mobile-native-module.md）
- armeabi-v7a / 32 位（代码中有大量 `AtomicU64`，直接放弃）
- WASM 路线在安卓复用（RN 无 wasm JIT，已定论）
- 多任务窗口 / 平板专属布局

**范围基线**：TrueHD / E-AC-3(JOC) / DTS 三条解码管道 + 对象双耳渲染。
AC-4 / IAMF / mPEG-H 不在第一批，链路预留 codec 分发点即可。

## 1. 代码摸底结论（计划的事实依据）

| 事实 | 出处 | 对计划的影响 |
| --- | --- | --- |
| `wasm-bindgen`/`js-sys` 只出现在 `packages/core/src/lib.rs` 和 `vbap.rs` | grep 计数 29 处，集中于 2 文件 | feature 门控成本低，管道 .rs 本身是干净的 |
| `harletty-bridge` 是未初始化的 git 子模块 | `.gitmodules`，eac3 为 path 依赖 | 构建文档/CI 必须先 `submodule update --init` |
| 渲染引擎（vbap/spatial/convolution/hrtf/cinema/focus/headphone/dsp/pcm_ring/stereo_fifo 等 35 个模块）全部在 `apps/native-renderer` 二进制 crate 内 | `apps/native-renderer/src` | 用 lib target 方式抽离，不搬文件 |
| 引擎与 stdin/stdout JSONL 协议、CPAL 初始化耦合在 `main.rs` | portability-research §2 | 协议类型进 lib，IO 留 bin |
| 内部时钟假设 48 kHz；HRTF 资产 48 kHz；`ensureStreamRate` 原生分支未验证重采样 | portability-research §4.2 | 输入重采样 + 输出速率适配是独立任务组 |
| `apps/mobile` 已有 Expo 壳，演示模式运行；`expo-document-picker` 已装 | `apps/mobile` | UI 起点存在，只差原生桥和 R3F |
| cpal 0.15.3 Android 后端存在但未验证；移动文档已定 AAudio 直连 | portability-research §3.1 | AAudio 直连为主路线，cpal 为备选 |
| 桌面播放调度（背压/ACK/预缓冲/时钟/短帧合并）在 TS `packages/player` | portability-research §2 | 时钟与缓冲策略必须移到 native，JS 只做解封装 |

## 2. 里程碑总览

| 里程碑 | 内容 | 完成标志 | 相对规模 |
| --- | --- | --- | --- |
| M0 | 环境与基线 | 工具链出 `.so`，语料与参考 WAV 就绪 | S |
| M1 | sda-native 引擎库（宿主机） | host 测试：解码+渲染产物对齐桌面参考 | L |
| M2 | Android 音频输出 + 桥 | 真机出声（正弦 → 语料双耳） | M |
| M3 | 数据通路（解封装→喂数→时钟→元数据） | 真机完整播放语料，seek/暂停可用 | L |
| M4 | 头部追踪 | 传感器四元数进引擎，转动方向语义正确 | M |
| M5 | UI | R3F 3D 视图 + 完整播放控制 | M |
| M6 | 性能与发布 | 性能矩阵、功耗、签名包 | M |

M1 完全在宿主机（Windows）上进行，不碰安卓，是风险最集中的阶段——
引擎抽离的正确性可以在 host 上用参考 WAV 闭环验证，不要推迟到真机才发现。

依赖关系：M0 → M1 → M2 → M3 → (M4, M5 可并行) → M6。
M2.1–T2.2（AAudio 验证）可与 M1 并行启动。

## 3. 原子任务清单

每条格式：`内容 / 产出物 / 验收标准 / 依赖`。验收标准必须是可执行的检查，
不写"基本可用"这类不可判定的描述。

### M0 环境与基线

- **T0.1 初始化子模块并固化构建前提**
  - 内容：`git submodule update --init`；确认 `packages/core`（sda-core）host `cargo check` 通过（wasm 目标由现有 `scripts/build-core.mjs` 验证）。
  - 产出物：构建前置写入 `docs/android-porting-plan.md` 或新构建文档一节。
  - 验收：新克隆仓库按文档操作后 `cargo check -p sda-core` 通过。
  - 依赖：无。

- **T0.2 安装安卓工具链**
  - 内容：Android Studio + NDK(r27+) + SDK 35；`rustup target add aarch64-linux-android`；安装 `cargo-ndk`。
  - 产出物：本机工具链版本清单（记录到文档）。
  - 验收：新建一次性示例 crate（`crate-type=["cdylib"]`）`cargo ndk -t arm64-v8a build --release` 产出 `libsSample.so`。
  - 依赖：无。

- **T0.3 建立测试语料库**
  - 内容：收集并登记每个 codec 至少一条代表语料：E-AC-3 JOC、TrueHD（含 Atmos 元数据）、DTS:X；记录容器、采样率、对象数、声床布局。放入 `test-assets/`（本地或 LFS，不入库也可，登记路径）。
  - 产出物：语料清单表（文件名/格式/时长/特征）。
  - 验收：清单中每条语料都能在桌面版当前 master 上正常播放。
  - 依赖：T0.1。

- **T0.4 生成桌面参考输出**
  - 内容：用桌面版（native sidecar 链路）对每条语料输出最终双耳 PCM/WAV 作为参考基准。可复用 `apps/native-renderer` 现有 dump/诊断手段。
  - 产出物：`test-assets/reference/<codec>.wav` + 生成命令记录。
  - 验收：参考 WAV 可被后续脚本读取并计算逐样本差异；生成命令可重复执行。
  - 依赖：T0.3。

- **T0.5 登记基准真机**
  - 内容：确定开发主力机（建议骁龙 8 系或天玑 9 系，12GB RAM）+ 一台中端机；记录 SoC/RAM/系统版本/是否支持 USB 多声道输出。
  - 产出物：设备登记表。
  - 验收：两台设备 adb 可用、开发者模式开启、能跑 Expo debug 包。
  - 依赖：无。

- **T0.6 CI 编译门禁骨架**
  - 内容：GitHub Actions 加 job：ubuntu + NDK，先只做 `cargo check --target aarch64-linux-android`（对后续存在的 crate 逐个纳入）。
  - 产出物：`.github/workflows` 新 job。
  - 验收：CI 在当前分支绿（检查目标 crate 列表可暂时为空/仅 hello 示例）。
  - 依赖：T0.2。

### M1 sda-native 引擎库（全部在 host 上完成）

- **T1.1 sda-core feature 门控**
  - 内容：`packages/core/Cargo.toml` 加 `[features] wasm = ["wasm-bindgen", "js-sys"]`（default = ["wasm"] 保持现状）；`lib.rs`、`vbap.rs` 中 wasm-bindgen 代码以 `#[cfg(feature = "wasm")]` 包裹；平台中立代码（各 codec pipeline）不动。
  - 产出物：sda-core 改造 PR。
  - 验收：① `cargo check -p sda-core --no-default-features` host 通过；② `pnpm core:build`（wasm）产物不变。
  - 依赖：T0.1。

- **T1.2 定义 native 侧事件与配置数据结构**
  - 内容：新建 `packages/core/src/events.rs`（或等价模块），把 web 端 `index.ts` 暴露的对象事件（对象 id、位置 xyz、声床、增益包络等）与引擎配置定义为 serde struct/enum，作为 JS 与 native 的共同契约。
  - 产出物：`events.rs` + 与 `index.ts` 字段的一一对应表。
  - 验收：host 测试中同一事件序列经 serde_json 序列化后字段名/单位与 web 端现有 JSONL 协议一致（抽 10 类事件比对）。
  - 依赖：无。

- **T1.3 实现 codec 检测与解码 facade**
  - 内容：新建平台中立 `Decoder` facade：magic bytes 识别 truehd/eac3/dts → 分发到现有 `*_pipeline.rs`；`new(config)` / `feed(&[u8])` / `poll_events()` / `flush()`。
  - 产出物：facade 模块。
  - 验收：host 单测：三条语料的解封装 chunk 依次 feed，产出 PCM 帧序列的 f32 hash 与直接调用各 pipeline 的结果一致。
  - 依赖：T1.1、T1.2、T0.3。

- **T1.4 native-renderer lib 化（不搬文件）**
  - 内容：`apps/native-renderer/Cargo.toml` 增加 `[lib]`（`src/lib.rs` 以 `pub mod` 挂载现有模块），`main.rs` 改为 `use sda_native_renderer::*`。
  - 产出物：lib target 改造 PR。
  - 验收：① `cargo build --release` sidecar 二进制照常产出且 Windows 桌面回归播放 T0.3 语料与改造前一致；② `cargo build --lib` 通过。
  - 依赖：T0.4（回归对照用）。

- **T1.5 协议与引擎解耦**
  - 内容：`protocol.rs` 中纯数据类型（命令/事件结构）移入 lib 并与 IO（stdin/stdout 读写）分离；`main.rs`/`monitor.rs` 等进程层留在 bin 侧依赖。
  - 产出物：模块拆分 PR。
  - 验收：lib 不依赖 `std::io::stdin/stdout`（用 `cargo geiger` 或 grep 断言）；sidecar 行为回归通过（同 T1.4 验收①）。
  - 依赖：T1.4。

- **T1.6 sda-native 聚合 crate 骨架**
  - 内容：新建 crate（如 `crates/sda-native`），依赖 `sda-core --no-default-features` + native-renderer lib；定义 `Engine`：`init(config, hrtf_dir)`、`load_track(codec)`、`start/pause/stop`、`seek(ms)`、`set_head_pose(quat)`、`poll_events()`。
  - 产出物：crate 骨架 + Engine 空实现编译通过。
  - 验收：`cargo check --target aarch64-linux-android -p sda-native` 通过（CI 纳入该 crate）。
  - 依赖：T1.1、T1.5。

- **T1.7 音频输出抽象（AudioSink trait）**
  - 内容：定义 `trait AudioSink { write(stereo_f32); sample_rate(); underruns(); }`；把 native-renderer 的 `stereo_fifo` → CPAL 回调路径改为经 trait；CPAL 实现留 sidecar，另加 `NullSink`（host 测试用）。
  - 产出物：trait + 两个实现。
  - 验收：sidecar 回归（同 T1.4①）；host 测试用 NullSink 跑通语料渲染全链路并产出 WAV dump。
  - 依赖：T1.5、T1.6。

- **T1.8 播放时钟与缓冲策略进 native**
  - 内容：把 `packages/player` 中预缓冲水位、背压/ACK、短帧合并、presentation clock 的语义在 sda-native 内重建（消费 sample 计数为唯一时钟源）；喂数侧暴露 `buffer_water_level()` 供 JS ACK。
  - 产出物：时钟/缓冲模块 + 设计说明（对齐 player.ts 的阈值参数表）。
  - 验收：host 测试：模拟变速喂数（突发/停顿）不 underrun、`position_ms()` 随消费线性推进；seek 后时钟跳变正确。
  - 依赖：T1.7。

- **T1.9 seek 语义实现**
  - 内容：`seek(ms)` = flush 渲染 FIFO + decoder reset + 事件时间基重置；返回值指示是否需要 JS 侧重新定位解封装（多数容器需要）。
  - 产出物：seek 实现 + 文档（native/JS 各自职责边界）。
  - 验收：host 测试：语料 seek 到 30%/60% 后 PCM 输出与桌面版同点位 seek 的参考输出对齐（容差内）。
  - 依赖：T1.8、T0.4。

- **T1.10 事件节流与快照**
  - 内容：对象事件按 66ms 节流聚合成快照（与 web `frame-batcher` 语义一致），`poll_events()` 返回自上次调用以来的快照序列。
  - 产出物：节流模块。
  - 验收：host 测试：1kHz 对象更新输入下，输出快照间隔 ∈ [60, 75]ms，最终状态无损（停止喂入后最后一帧位置正确）。
  - 依赖：T1.2、T1.6。

- **T1.11 采样率审计与输入重采样**
  - 内容：① 审计三条 pipeline 的输出采样率（48k 固定？TrueHD 96k？）登记成表；② 对非 48k 路径实现输入重采样（rubato 或等价），对象事件/ramp 时间戳按统一比例映射（防逐帧舍入漂移）。
  - 产出物：采样率审计表 + 重采样实现。
  - 验收：① 表格覆盖 T0.3 全部语料；② 96k（如可构造）语料 host 测试：10 分钟渲染时长后事件时间戳累计偏差 < 1ms。
  - 依赖：T1.3。

- **T1.12 HRTF 资产 host 加载校验**
  - 内容：sda-native 的 `init(config, hrtf_dir)` 按现有 `verify-hrtf-assets` 规则校验并加载 HRTF。
  - 产出物：加载器 + 校验复用。
  - 验收：对 T0.4 参考生成时的 HRTF 目录：加载成功且 host 渲染输出与参考 WAV 一致；故意换错文件时报错明确。
  - 依赖：T1.6。

- **T1.13 host 端到端金标测试脚本**
  - 内容：`scripts/test/android-engine-host.mjs`：TS demux 离线产出 chunk 文件 → sda-native（NullSink）渲染 → dump WAV → 与 `reference/<codec>.wav` 逐样本差异报告。
  - 产出物：测试脚本 + 首次运行报告。
  - 验收：三条语料差异在既定容差内（建议先统计实际差异定阈值，关注相关性与能量而非逐位相等）；脚本一键可跑。
  - 依赖：T1.9、T1.10、T1.11、T1.12。

### M2 Android 音频输出与原生桥（可与 M1 后半并行）

- **T2.1 AAudio 直连验证（独立 demo）**
  - 内容：一次性 cdylib（不动主工程）：ndk/aaudio 绑定打开 low-latency 流播 440Hz 正弦；记录实际 burst/latency。
  - 产出物：demo crate + 真机测量记录（burst 数值、采样率、buffer capacity）。
  - 验收：真机听到正弦；logcat 无 AAudio 错误；记录数据 ≥ 1 台基准机。
  - 依赖：T0.2、T0.5。

- **T2.2 AudioSink 的 AAudio 实现**
  - 内容：实现 `AudioSink` trait：回调从渲染 FIFO 拉数、underrun 计数、设备采样率协商（设备不接受 48k 时输出侧重采样，复用 T1.11 结论）、设备变更重建。
  - 产出物：AAudio sink 模块（host 不可测部分收敛到薄层）。
  - 验收：真机：sda-native 输出 T1.13 的 WAV dump 经 AAudio 播放正常，underrun 计数在静置播放 5 分钟内为 0。
  - 依赖：T1.7、T2.1。

- **T2.3 Expo android prebuild 与 .so 打包**
  - 内容：`apps/mobile` 走 expo prebuild 生成 `android/`；cargo-ndk 产出 `libsda_native.so` 放入 `jniLibs/arm64-v8a`（x86_64 可选，模拟器冒烟用）；`System.loadLibrary` 验证。
  - 产出物：构建脚本（`scripts/build-android-native.mjs`）+ prebuild 配置。
  - 验收：debug 包真机安装启动，logcat 显示 `.so` 加载成功。
  - 依赖：T1.6、T0.2。

- **T2.4 FFI 边界定稿（决策执行）**
  - 内容：按 §4 决策 D2 执行：uniffi 绑定或手写 JNI，导出 T1.6 Engine 全部方法；错误以结构化错误码返回（不做 panic-跨界）。
  - 产出物：绑定层 + API 文档表（函数/线程模型/错误码）。
  - 验收：Kotlin 单测：init/feed/pollEvents 往返一个真实 chunk；`cargo test` 覆盖 panic 不跨 FFI（catch_unwind 验证）。
  - 依赖：T1.6。

- **T2.5 Expo Module（Kotlin）封装**
  - 内容：`modules/sda-core` ExpoModule：暴露 JS API（init/config/feed/pollEvents/transport 控制/headPose）+ 事件发射（onObjects、onState）。
  - 产出物：Expo Module + TS 类型定义（`apps/mobile/src/native/*`）。
  - 验收：RN 侧可调用全部 API；pollEvents 循环在 JS 定时器下 66ms 稳定（DCD 工具或时间戳日志验证 5 分钟无漂移）。
  - 依赖：T2.3、T2.4。

- **T2.6 线程模型落地**
  - 内容：固定线程规划：AAudio 回调线程（只拉 FIFO）→ 渲染工作线程（卷积）→ 解码工作线程 → JNI 调用线程；明确每根线程职责与优先级（回调线程不动、渲染线程 `SCHED_FIFO`/priority 提升按设备能力）。
  - 产出物：线程模型文档 + 代码落地。
  - 验收：真机播放时 systrace/perfetto 显示音频回调函数耗时 < burst 预算的一半。
  - 依赖：T2.2、T2.5。

- **T2.7 前台服务与音频焦点**
  - 内容：Kotlin：`mediaPlayback` 前台服务 + `AudioFocusRequest`（transient loss 暂停、恢复续播）+ 通知栏控制（播放/暂停）。
  - 产出物：服务实现。
  - 验收：① 锁屏后台播放不中断 ≥ 10 分钟；② 拨打测试电话后自动暂停、挂断后恢复；③ 通知栏按钮生效。
  - 依赖：T2.5。

- **T2.8 真机首个端到端出声（里程碑验收）**
  - 内容：JS 读语料（临时：直接 assets 内置一个 .ec3）→ 简化喂数 → native 解码渲染 → AAudio 出声。
  - 产出物：可演示的 debug 包。
  - 验收：真机双耳听到语料全程播放；与桌面参考 WAV 主观一致（先不要求录音对比，M3 验收做）。
  - 依赖：T2.2、T2.5、T1.13（语料 chunk 已由脚本产出）。

### M3 数据通路完整化

- **T3.1 TS 解封装在 Hermes 下验证（决策执行）**
  - 内容：按 §4 决策 D3：验证 `packages/demux`（mp4box/mkv/adm/bwf/dbmd）在 Hermes + RN 环境运行；Buffer/stream polyfill 按需补。
  - 产出物：验证结论 + polyfill 清单（或失败报告触发 T3.1f）。
  - 验收：RN 内对 T0.3 语料解封装，chunk 字节序列与 node 端同一脚本输出逐字节一致。
  - 依赖：T0.3。
  - **T3.1f（备选，仅当 T3.1 失败）**：Rust 侧实现最小 MP4/MKV 解封装（只解析 T0.3 语料涉及的轨道），走 T1.3 facade 的 codec 检测；验收同 T3.1。规模 L，会显著延后 M3。

- **T3.2 喂数协议与背压闭环**
  - 内容：定稿 chunk 边界（按解码帧/按固定 KB）、feed 批量上限、基于 `buffer_water_level()` 的 JS 侧暂停/恢复阈值；写协议文档。
  - 产出物：喂数协议文档 + JS/native 两侧实现。
  - 验收：语料完整播放一遍：native 侧水位日志无越界、JS 无 OOM（Android Studio memory profiler 峰值 < 200MB）。
  - 依赖：T1.8、T2.5。

- **T3.3 播放状态机**
  - 内容：JS 侧状态机（idle/loading/playing/paused/seeking/error/ended），驱动 UI 与 native 调用；错误码到用户文案映射。
  - 产出物：状态机模块 + 文档。
  - 验收：全状态迁移有单测；真机操作路径覆盖：选文件→播放→暂停→恢复→seek→结束→重播。
  - 依赖：T3.2。

- **T3.4 seek 全链路**
  - 内容：JS demux 定位（按容器索引）→ 丢旧 chunk → native seek → 恢复喂数；进度条按 `position_ms()` 推进。
  - 产出物：seek 流程实现。
  - 验收：语料随机 seek 20 次：出声延迟 < 300ms（记录 P50/P95）、无杂音爆音、时间显示单调连续。
  - 依赖：T1.9、T3.2。

- **T3.5 ADM/元数据通路**
  - 内容：ADM XML/dbmd 解析维持 JS（复用 `packages/demux`），产出布局/zone/对象配置 JSON 传 native（对齐 sidecar 现有配置协议字段）。
  - 产出物：配置传递实现 + 与桌面配置字段对照表。
  - 验收：带 ADM 的语料：对象数、初始位置、声床布局与桌面版加载同文件完全一致（对照截图/导出 JSON diff）。
  - 依赖：T3.2。

- **T3.6 HRTF 资产打包进 APK**
  - 内容：HRTF 资产入 `android/assets`（或首次启动拷贝到 filesDir，按体积与 mmap 需要定），native 按路径加载。
  - 产出物：打包脚本 + 加载路径。
  - 验收：APK 安装后无外部依赖完成 T1.12 同等校验；APK 体积增量记录。
  - 依赖：T1.12、T2.3。

- **T3.7 M3 端到端验收（里程碑验收）**
  - 内容：真机播放三条语料全程，输出经 USB/loopback 或录音采集与桌面参考对比；记录全功能清单结果。
  - 产出物：验收报告（差异指标 + 已知问题清单）。
  - 验收：① 播放/暂停/seek/事件流全部可用；② 输出对比结论"可接受"（容差沿用 T1.13 口径）；③ 已知问题均有 issue。
  - 依赖：T3.3、T3.4、T3.5、T3.6。

### M4 头部追踪

- **T4.1 传感器管线**
  - 内容：Kotlin：`TYPE_ROTATION_VECTOR` → 四元数，采样 ≥ 60Hz，节流后经 `setHeadPose` 进 native；可选 `TYPE_GYROSCOPE` 融合由系统 sensor fusion 承担（不自建滤波）。
  - 产出物：传感器订阅模块 + 频率/延迟记录。
  - 验收：真机 log：四元数流连续无跳变（相邻差值阈值检测 5 分钟无告警）。
  - 依赖：T2.5。

- **T4.2 坐标系语义对齐**
  - 内容：把 Android sensor 轴（右手、y 北 z 天）映射到 ADM 坐标约定；对照桌面 `head-tracking-driver` 的 JSONL 语义与 `docs/head-tracking-helper-jsonl-protocol.md`，写成映射表。
  - 产出物：坐标系映射文档 + 转换单测。
  - 验收：单测覆盖 6 个标准朝向（前/后/左/右/上/下）的期望输出；真机"向左转头→声像向右相对稳定"等主观测试表全过。
  - 依赖：T4.1。

- **T4.3 头追状态 UI 与开关**
  - 内容：头追开关、校准（当前朝向=正前）、漂移重置按钮；遥测数据接性能页。
  - 产出物：UI + 遥测接入。
  - 验收：开关即时生效；校准后正对屏幕时声像居中（主观表）。
  - 依赖：T4.2。

### M5 UI

- **T5.1 R3F 可行性验证**
  - 内容：Expo dev client 安装 `three` + `@react-three/fiber`（确认 Hermes 兼容、必要时禁用不支持的特性）。
  - 产出物：最小旋转立方体场景 demo。
  - 验收：真机 60fps 渲染连续 5 分钟，内存稳定。
  - 依赖：T2.3。

- **T5.2 3D 房间场景移植**
  - 内容：从 web 端移植最小房间场景：房间线框、对象点、listener 标记；对象事件驱动位置（复用 `auto-layout.ts` 思路）。
  - 产出物：RN 场景组件。
  - 验收：演示模式对象运动流畅；接入真实事件流后位置与桌面版同点位一致（录屏对照）。
  - 依赖：T5.1、T2.5。

- **T5.3 播放控制界面**
  - 内容：传输条（播放/暂停/seek/时间）、音量、对象列表（静音/独奏）。
  - 产出物：控制组件。
  - 验收：T3.3 状态机的所有状态均有对应 UI 反馈；对象静音后 native 侧确认增益生效（事件/遥测）。
  - 依赖：T3.3、T5.1。

- **T5.4 文件选择与导入流程**
  - 内容：`expo-document-picker` 选文件 → 拷贝到缓存目录 → 路径交 native/JS demux；大文件流式处理评估。
  - 产出物：导入流程。
  - 验收：> 1GB 文件选择到开始播放全流程成功；重复导入同名文件不冲突。
  - 依赖：T3.1。

- **T5.5 设置页**
  - 内容：输出模式（双耳 / USB 多声道 [视设备能力，可标记实验性]）、头追开关、性能监控页（复用 `performance-sink` 口径：渲染耗时、underrun、水位）。
  - 产出物：设置页。
  - 验收：设置项重启 App 后保持；性能页数据与 logcat/perfetto 观察一致。
  - 依赖：T5.3、T2.6。

### M6 性能、加固与发布

- **T6.1 性能基准矩阵**
  - 内容：真机跑对象数（2/8/16/32/64）× 房间卷积档位的 CPU/内存矩阵（perfetto 采样），对照桌面阈值。
  - 产出物：基准报告 + 推荐档位表。
  - 验收：矩阵覆盖两台基准机；确定 64 对象的可行档位或明确降级点。
  - 依赖：T3.7。

- **T6.2 性能优化（按 T6.1 结果逐项执行）**
  - 每项一个任务、一项验收：T6.2a NEON 路径确认（rustfft NEON 生效验证，对比关闭开关的耗时）；T6.2b 卷积分区长度调参；T6.2c 渲染线程绑核/优先级；T6.2d FIFO/批大小调参。验收统一为：优化点有前后对比数据，热路径无回归。
  - 依赖：T6.1。

- **T6.3 功耗与发热 soak**
  - 内容：基准机 30 分钟连续播放（64 对象推荐档位），记录电池曲线、温控限频、underrun。
  - 产出物：soak 报告。
  - 验收：30 分钟 underrun=0 或有触发条件的明确结论；表面温度与限频记录在案。
  - 依赖：T6.1。

- **T6.4 设备矩阵扩展**
  - 内容：加测 1 台中端机 + 1 台 3–4 年前旗舰；x86_64 模拟器冒烟（仅安装启动+演示模式）。
  - 产出物：矩阵测试报告。
  - 验收：每台设备完成 T3.7 验收清单；失败项按设备记录。
  - 依赖：T3.7。

- **T6.5 崩溃与资源加固**
  - 内容：JNI local ref 泄漏检查（长时间 feed/poll 循环 `adb shell dumpsys meminfo` 对比）、panic hook 上报、ANR 检查、native heap 增长检查。
  - 产出物：加固 checklist + 修复项。
  - 验收：8 小时循环播放/seek 压测脚本无崩溃、native RSS 增长 < 50MB。
  - 依赖：T3.7。

- **T6.6 发布包**
  - 内容：release 签名、ABI split（arm64 优先）、`cargo build --release` 产物纳入构建脚本、版本号策略。
  - 产出物：构建/发布文档 + 签名包。
  - 验收：干净环境按文档从零出包并安装成功。
  - 依赖：T6.4、T6.5。

## 4. 关键决策点

| # | 决策 | 推荐 | 理由与备选 |
| --- | --- | --- | --- |
| D1 | 音频输出技术 | AAudio 直连 Rust（ndk 绑定） | mobile 文档已定；Oboe 需引入 C++ 工具链，cpal Android 后端未验证，留作 T2.2 失败时备选 |
| D2 | FFI 方式 | 先手写 JNI（API 面小且稳定，Engine 约 10 个方法）；若后续 API 膨胀再迁 uniffi | mobile 文档推荐 uniffi；但 Expo Module 本身要写 Kotlin，手写 JNI 少一层代码生成依赖。两方案任务量近似，T2.4 开工前定稿即可 |
| D3 | 解封装位置 | JS（复用 `packages/demux`） | 代码已存在且纯 JS；mp4box 在 Hermes 的兼容性是 T3.1 的验证点，失败才走 T3.1f（Rust 移植，规模 L） |
| D4 | 引擎抽离方式 | native-renderer 加 lib target，不搬文件 | 零路径变更、sidecar 回归成本低；等移动端稳定后再考虑迁出为独立 crate |
| D5 | 播放调度位置 | 时钟/缓冲/背压在 native（Rust），JS 只做解封装与 UI | JNI 往返延迟不适合帧级调度；对齐 portability-research §2 "移动端不能只移植卷积器"的结论 |

## 5. 风险与对策

| 风险 | 触发信号 | 对策 |
| --- | --- | --- |
| 真机算力不足（多对象卷积） | T6.1 矩阵超标 | 降级路径预先设计：减分区长度 → 限对象数 → 关房间卷积只留双耳直渲 |
| mp4box 不可用于 Hermes | T3.1 失败 | T3.1f Rust 解封装（预留 2–3 周级工作量） |
| eac3/dca crate 在 NDK 编译失败（汇编/构建脚本） | T1.6 首次交叉编译 | 提前到 M1 早期验证（T1.6 第一项验收就是 aarch64 check）；必要时上游 patch |
| AAudio 设备碎片化（采样率/burst 差异） | T2.1/T6.4 | 输出重采样层（T1.11 结论复用）+ 设备矩阵覆盖 |
| 子模块/大文件资产入库问题 | T0.1/T3.6 | 语料与 HRTF 走本地登记或 LFS，构建文档写清来源与校验 |
| 工具链版本漂移 | CI 与本机不一致 | T0.6 CI 固定 NDK/rust 版本，文档锁定版本号 |

## 6. 与现有文档的关系

- `backend-portability-research.md`：跨平台调研与代码审计，本计划的 §1 事实依据。
- `mobile-native-module.md`：移动端架构设计（iOS 优先视角），本计划是其 Android 部分（里程碑 4）的落地细化。
- `head-tracking-helper-jsonl-protocol.md`：桌面头追协议，T4.2 坐标系语义的对照来源。
