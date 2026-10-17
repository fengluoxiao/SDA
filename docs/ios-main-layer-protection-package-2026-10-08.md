# iOS 主层保护版打包（2026-10-08）

用户明确要求将主层保护候选打包 iOS。此次只发布 6 个相关源码/UI 文案文件，不包含其他桌面资产或用户音频。

- 已测试本地提交：`7d996aa680aee4113165384d098e656f7c2a0302`。
- 云端提交：`fba29d98a5be07e400b77c4df0bfa7e53262d592`。
- 两个提交的完整源码树完全一致；Git 传输两次连接失败后，通过 Git Data API 逐 blob/tree 校验并非强制更新远端。API 时间戳规范化改变 commit SHA，不改变源码内容。随后已导入精确 API commit，并以源码树相同的方式对齐本地分支；工作区文件未重置。
- 原生检查：258 passed、0 failed、27 ignored；TypeScript/iOS 开关/偏好及 mobile facade host check 通过。
- 运行：37746548242，run 65，仅手动启动 ios-port-ci.yml；push commit 带 skip ci，未主动触发 Windows/Android。

增强开关仍为「空间层增强 · +2/+6」，默认关闭，保存/恢复逻辑不变。主层保护修正后，水平前/侧方抵消自动增益，峰值保护前恢复原始基准电平；高低处/后方辅助层保留增强。不识别人声、不按歌曲/时间/对象 ID 处理。真实声音验收与数值/构建检查是不同状态。

当前正在云端构建，尚未交付完成。最终需验证源码 HEAD、Payload/arm64 Mach-O、内嵌 JS 的新版“主层维持原电平”文案、原生增强接口、61 点 HRTF、ZIP CRC、大小及 SHA256。

仍为未签名 IPA，需有效签名/配置后安装。不能把旧 run 64 包或 SDK 27 不可用日志称为新包。进度/来源保存在 `tmp/ios-delivery/main-layer-protection-2026-10-08/`。

## 最终交付

run 65（37746548242）成功，已验证新包而非旧 run 64。

- IPA：`E:/SDA/artifacts/ios-main-layer-protection-37746548242/SDA-main-layer-protection-SDK26-run65-unsigned.ipa`。
- 大小：16,369,268 字节（约 15.6 MiB）。
- Bundle：app.sda.mobile，版本 0.1.0 / build 1。
- arm64，最低 iOS 16.4，实际 SDK iphoneos26.5。
- SHA256：`beb1e12030d92355673ea2d4f6bf887576210beb145a5e3c35b97c39f9a433c7`。

实际 API 发布提交 fba29d9 与本地已测试提交 7d996aa 的源码树完全一致；包内已经匹配新版“主层维持原电平/水平主层抵消自动抬升”文案，原生增强接口、61 点历史 HRTF、完整 Payload 和 arm64 Mach-O 均通过检查。整个 artifact 与 GitHub digest 一致，IPA ZIP CRC 通过。SDK 27 仅为不可用日志，不误报为应用。

新版未签名，需要用户有效签名后安装。入口与偏好保存逻辑不变，新安装默认关闭，已保存的开启偏好保留。旧包保留，未覆盖。模拟器播放/后台通过不等于实际 iPhone 听感验收。
