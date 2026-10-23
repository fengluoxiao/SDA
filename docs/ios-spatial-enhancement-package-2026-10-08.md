# iOS 空间层增强打包记录（2026-10-08）

## 已提交的功能

用户接受通用增强试听后，明确要求打包 iOS。仅选择性提交本任务的 16 个源码、iOS UI/偏好、回归与工作流文件，保留其他桌面二进制、资产删除、远程 Web 资源和用户音频改动，不将其混入此次提交。

- 提交：`008d1a64c016f761492d088503ea43b2fcd6c889`。
- 分支：`feat/android-port`；远端仓库：`fengluoxiao/SDA`。
- commit 带 `[skip ci]`，推送后仅手动 dispatch `ios-port-ci.yml`。
- 运行 ID：`37735294718`；run number：64；已核对运行 HEAD 与请求提交一致。

## iOS 使用入口

“更多设置 → 声音增强 → 空间层增强 · +2/+6”。默认仍关闭；用户开启后以一个共享工作线程命令同时应用 master +2 dB / 辅助空间层最多 +6 dB，关闭恢复两者 0 dB。选择写入 UserDefaults，并在新原生播放恢复。系统空间音频路径不应用本增益，活动系统路径下 UI 开关禁用。

这不是声源响度分类、歌曲/对象规则或独立人声识别；会改变辅助空间内容的相对电平。原始测量、位置、元数据 gain 和源 PCM 数据未被打包代码改写。既有整体峰值保护继续生效。

## 检查与打包状态

- 本地 TypeScript noEmit、iOS UI 回归、空间层 UI/偏好专项检查通过。
- GNU Windows host Rust facade + ios-host check 通过。
- 本地原生 lib 新版本：257 passed、0 failed、27 ignored。
- 云端共享 Rust XCFramework 和前置增强/空间回归已通过，正继续 Xcode App 构建。
- 现有工作流生成未签名 device IPA，最低目标 iOS 16.4；需有效 Apple 签名/配置后安装。

当前不是交付完成。真实 IPA 生成后，检查 Payload、Info.plist、arm64 Mach-O、嵌入 JS/原生增强接口、大小和 SHA256，不能将 SDK 不可用分支的日志包视作应用。

进度：`tmp/ios-delivery/spatial-enhancement-2026-10-08/build-state.json`；来源记录：同目录 `source.json`。最终目录/签名边界待实际产物验证后补充。

## 最终交付：已验证未签名 IPA

运行 37735294718（run 64）已成功。SDK 26 真机归档、模拟器播放和后台冒烟检查通过；SDK 27 分支仅上传 384 字节的不可用诊断 ZIP，不能称为 SDK 27 应用或认证。

- 文件：`E:/SDA/artifacts/ios-spatial-enhancement-37735294718/SDA-spatial-enhancement-SDK26-run64-unsigned.ipa`。
- 大小：16,364,723 字节（约 15.6 MiB）。
- Bundle ID：`app.sda.mobile`；版本 0.1.0，build 1。
- 实际 SDK：iPhoneOS 26.5；最低系统 16.4；arm64。
- SHA256：`fa9bc3a93b30f3d7a4267c11b3c4d54c6552c5419f693e43f5cf53196849d064`。

整个 SDK 26 artifact 的摘要与 GitHub digest 一致；IPA 内部 ZIP CRC 通过。已确认 Payload/Info.plist/arm64 Mach-O、内嵌增强 UI 与原生接口、61 点历史 HRTF manifest。交付 manifest 和 SHA256SUMS/安装说明保存在同一目录。

默认仍关闭；用户在「更多设置 → 声音增强」启用增强后使用 +2/+6 方案。此包未做有效设备签名/配置，需要用户签名后安装，不是 TestFlight/App Store 发布，也不是实机听感验收。
