# iOS 真机 IPA 打包流程

按用户要求，`ios-port-ci.yml` 现在只生成真机 IPA。所有旧测试、独立 Swift 探针、模拟器应用构建、模拟器启动/冒烟检查、模拟器 Rust 目标均已从打包工作流移除。仓库中的测试源文件保留供手动运行，不由打包任务调用。

旧流程 #157 在真机归档阶段触发 90 分钟硬超时。#158 的分段记录为：原生回调测试 819 秒、两种架构 Rust 库 857 秒、模拟器应用 924 秒、冒烟 520 秒；因此仅调整测试优化参数仍会超过一小时。

## 当前步骤

资源准备 → 单一 aarch64-apple-ios Release 库 → Expo 项目/CocoaPods → 真机归档 → 移除 CI 音频资源 → IPA/dSYM/构建来源与耗时记录 → 上传。

- `SDA_IOS_INCLUDE_SIMULATOR=0` 只编译真机库。构建脚本未设置此变量时仍支持开发者原有的双平台 XCFramework。
- 保留 pnpm、Rust、CocoaPods 下载和 Xcode 内容寻址编译缓存。源/依赖/工具链仍由各自构建系统识别，真机链接正常执行。
- 正式 Rust opt-level=3、Thin LTO 和已选音频处理保持。TrueHD 依赖使用 16 个 codegen units，和 core 的既有配置一致。
- `build-info.json` 只记录源提交、库摘要、128 路容量与源文件摘要，不执行解码器或测试；明确写入 `testsExecuted=false`、`simulatorBuilt=false`。
- 输出 `BuildTimingSummary` 与 `build-timings.json`，编译任务累计时间和墙钟用时分开理解。纯打包实际分钟数以成功运行的记录为准。

128 路修复的本地验证记录见 `object-source-capacity-2026-10-20.md`。后续 IPA 可以核对实际二进制提交、资源、架构和构建来源；不能把未运行的测试或未做的实机安装标成通过。
