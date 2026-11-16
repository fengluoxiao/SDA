# 苹果打包环境改为 Xcode 27.1

GitHub 官方 runner-images 主分支当前提供 `xcode-27` ARM64 镜像。其文档列出的宿主为 macOS 27.0.1，预装并默认选择 Xcode 27.1。Xcode 27.1 自带 iOS 27.1 SDK、macOS 27.0 SDK；这几个版本号不能混为一谈。官方没有 `macos-27.1` 的标准 runner 标签，宿主小版本随镜像滚动更新，实际版本记录在每次构建的 `apple-toolchain.json` 中。

## 打包范围

- iOS 真机安装包改为 `xcode-27`，明确选择 Xcode 27.1 并验证实际 iPhoneOS SDK 为 27.1。iOS 最低部署版本继续使用 16.4。应用版本为 0.9.40，iOS 构建号升至 948；Android 构建号仍为 947。
- macOS 的 x64、ARM64 安装包均在 `xcode-27` 构建。Intel 包保留 `x86_64-apple-darwin` 交叉编译目标，ARM64 保留 `aarch64-apple-darwin`。开始编译前用选定 SDK 链接对应架构的 C++/AudioToolbox 探针，检查输出 Mach-O 架构；Intel 任务另检查 Rosetta 工具执行。
- 既有 Windows、Android 和音频参考测量/旧系统验证任务不变。没有重新生成 HRTF 测量资产或修改音频 DSP。

## 选择与溯源

新的选择器只接受匹配的发布版 Xcode 应用目录，校验 `xcodebuild -version` 和 `xcrun` 返回的实际 SDK 版本。不匹配或未安装就失败，不会静默回退到其他版本、跳过验证后继续打包。宿主系统、runner 镜像版本、Xcode 构建号、iOS/macOS SDK 和架构探针结果一并保存。

工作流在检出应用源码后，从该次工作流提交 `GITHUB_WORKFLOW_SHA` 取得选择器。这样即使将来打包旧的固定 `source_ref`，也能使用当前审核的工具链选择逻辑，不混入其他运行时代码。macOS Cargo 缓存键加入 Xcode/SDK 指纹，避免跨工具链复用不适用的编译缓存。

官方依据存档：runner-images `README.md` blob `cbb0ec6433933cd83d6c749bffbb24a2513388be`、`images/macos/xcode-27-arm64-Readme.md` blob `e9ab70d9fcc85fdede8a253ee447b268c88f0c55`。最终镜像状态以实际构建日志为准。
