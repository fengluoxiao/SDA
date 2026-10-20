# iOS 构建耗时优化

本轮 iOS #157（音频提交 874d90b）的 Host callback tests 阶段实际耗时 2088 秒，即 34 分 48 秒；前一成功构建 #155 同阶段为 886 秒。新增格式回归扩大了未优化 DSP 的执行成本。core、renderer、native 原先还分别写入各自 target，且只缓存 native 的目录。

## 已改动

- CI 测试专用 opt-level=1、16 个 codegen units、line-tables-only、关闭测试 LTO，保留 debug assertions 和 overflow checks。只配置 TEST profile；正式设备/模拟器 `--release` 的 opt-level、LTO 和声学处理不变。native 另补齐 core 已有的 TrueHD `codegen-units=16`，避免单编译单元的内存/编译瓶颈；不降低该依赖的优化级别。
- 三个 manifest 与两种 Apple target 共用 `ios-build/cargo`。Swatinem/rust-cache 映射到该目录，Cargo 仍按目标架构、特性、编译器和配置区分产物。
- `build-ios-native.sh` 从 CARGO_TARGET_DIR 读取两份 Release 库，同时兼容原默认目录、相对路径及包含空格的绝对路径。没有复用旧 IPA 或跳过链接。
- setup-node 使用 pnpm store 缓存；CocoaPods 仅缓存下载，不缓存生成项目或绕过 pod install。缓存按实际 Xcode 指纹和依赖输入区分。
- Xcode 开启自身的内容寻址编译缓存，模拟器与真机仍分别编译、链接和验证。缓存按 Xcode 指纹与依赖输入恢复，每次运行独立保存更新后的内容；编译器缓存大小限制为 2 GB。没有缓存已生成的 App/IPA 或跳过构建。
- xcodebuild 输出 BuildTimingSummary；`build-timings.json` 汇总每份日志的 Cargo 编译和测试执行时间，Xcode 并行任务时间单独标注为累计任务时间，不冒充墙钟耗时。

## 验证与边界

三项工具回归通过，其中打包路径测试分别用默认/相对/含空格绝对目录生成两种 Apple target 的模拟归档，并确认 XCFramework 读取的文件实际存在；另两项覆盖多个 Cargo 运行的时长解析、Xcode 并行任务、失败和不完整日志。工作流 YAML、shell 语法和读流恢复/真机测试资源隔离检查通过。

没有删除格式矩阵、峰值/空间处理测试、模拟器冒烟、真机归档或包体资源检查。本次正在运行的 #157 继续旧流程，不为套用工作流修改而取消重跑。新流程的端到端分钟数需要后续 GitHub 运行的实测；冷缓存和热缓存分开看，不承诺固定三十分钟。

针对 native 四组合矩阵，新 TEST 配置的首次本地依赖编译耗时 7 分 45 秒；测试执行 59.39 秒，2 项通过。首次依赖编译与测试执行分别统计，这不是 GitHub iOS 整体用时或跨硬件提速比。

Xcode 编译缓存开关、CAS 路径与大小限制依据 Apple 的 Swift Build 开源设置定义：`Sources/SWBCore/Specs/CoreBuildSystem.xcspec`、`Sources/SWBCore/Settings/CASOptions.swift`。本次仅采用构建设置，没有引入其实现代码。

新测试配置的完整本地回归通过：core 46 通过 / 1 忽略（3.77 秒）；renderer 385 通过 / 35 忽略（214.34 秒）；native 110 通过 / 4 忽略（156.14 秒）。core 的第 47 项是仅 Release、关闭 debug_assertions 时编译的 Apple 小栈检查；上一轮 Release 回归已执行，本轮保留 CI 原有 debug_assertions 配置，不改变该选择条件。native 在同一目标目录的完整回归编译耗时 0.55 秒，依赖缓存已复用；这不是对后续 GitHub 冷构建用时的承诺。
