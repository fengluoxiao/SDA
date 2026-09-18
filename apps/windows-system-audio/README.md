# Windows 系统音频输入（实验工程）

本分支包含 WaveRT 虚拟输出端点、受限用户态读取器、IEC61937 DD+ 解包、SDA 对象解码和原生双耳渲染接收程序。**目前不是经过安装验收的桌面功能。** 本机已完成编译、离线完整性和原生渲染回放验证，开发包已通过 UAC 安装；设备启动尚未通过，不能宣称真实播放器直通成功。

路径：支持直通的播放器 → SDA 虚拟端点 → 有界原始数据队列 → capture.exe → IEC61937 解包 → SDA E-AC-3/JOC 解码 → PCM + 对象事件 → SdaNativeRenderer → 明确指定的实际输出设备。

只声明本原型支持的 48 kHz 立体声 PCM、DD+ 和 DD+ Atmos（192 kHz 双声道 16-bit 载波）。PCM 输入不会伪装成原始对象。TrueHD/MAT、DTS:X、MPEG-H 尚未接入此系统入口，原有文件解码能力不受影响。发送工具拒绝把压缩载波发往物理耳机。

## 构建与验证

在仓库根目录执行，需要 Python 3.12+、VS 2022 C++ Build Tools、WDK 10.0.26100.0、Rust、Node 和已有 SDA WASM/native 构建：

```powershell
powershell -File apps/windows-system-audio/build-driver.ps1
cargo build --manifest-path tools/windows-audio-probe/Cargo.toml --locked --offline
cargo test --manifest-path tools/windows-audio-probe/Cargo.toml --locked --offline
node apps/windows-system-audio/test.cjs
```

`prepare-driver.py` 从固定 Microsoft Windows-driver-samples 提交 `717778a20ba4dd2440fe609f69153a1f8a64f597` 取源码，在 `tmp/sda-system-audio-driver` 应用明确校验锚点的修改。可用 `--source` 指向现有镜像；默认缺失时克隆到 `tmp/windows-driver-samples`。Microsoft 上游许可证保留在准备目录。SDA 新增源文件采用仓库 Apache-2.0 许可。不会修改上游检出或安装任何驱动。

构建输出位于 `tmp/sda-system-audio-package`。WDK 的开发测试签名不等于发布签名；CAT 需要签名后才能在对应受信任测试环境安装。不要将这一目录视为正式安装包。

在已配置 WDK 开发证书的机器上，构建脚本可附加 `-TestSign`，使用本次 SYS 的同一个 WDK 测试证书签 CAT；不会安装证书或改变系统信任/启动设置。本机输出已使用开发证书签名，不代表获得 Microsoft 发布签名。

## 已验证

- Windows x64 WDK 编译生成 `SdaSystemAudio.sys`，INF 校验通过。
- 实际 Atmos 测试素材：144384 字节 E-AC-3 → 1155072 字节 IEC61937 → 原始载荷逐字节一致。
- 47 个解码帧、15 个对象、705 条对象事件，解码 PCM、对象位置/时间/增益、通道映射和响度元数据的统一 SHA-256 与文件直读一致：`ff85eb713cb564014bc540199c8c41fcc73f7d195d77bd452028db2e9ef52af8`。
- 1/7/997/65536 字节分片、读取记录跨界、暂停中断残缺 burst 后重新开始、非法长度/格式/数据间隙检查。
- 同一记录回放送入实际 SdaNativeRenderer，开启逐对象 HRTF 和连续方向渲染，以零输出音量通过物理 WASAPI 输出完成消费；这验证用户态链路，不证明驱动接收成功。

## 驱动安装与实际收发验证

仅在具备管理员权限和适当开发签名配置的测试系统完成这一阶段。安装脚本会自行调起 UAC，通过后使用 WDK DevCon 注册硬件 ID `Root\SdaSystemAudio`；已有设备则更新，不重复创建。不会改变测试签名或 Secure Boot。

本机安装记录（2026-09-18）：初版包注册成功，但设备报 Code 10 / `0xC0000184`；更新版本后 Windows 报需重启，单独移除设备并停止服务返回 1052，旧内核映像未卸载。`0.1.0.2` 已调整控制设备的创建/移除生命周期，并加入服务 Parameters 下的 `StartupXX`、`StartupResult` 初始化诊断。最新版安装的 UAC 请求被取消，当前设备版本仍为 `0.1.0.1`；需先安装 `0.1.0.2`，再重启加载新版后继续验收；编译成功及 DevCon 的“安装成功”不能代替设备启动成功。

1. 检查开发证书/已签 CAT、安装并确认设备管理器没有 Code 52 等错误。
2. 用 probe 查询新端点，记录格式协商的实际 HRESULT。
3. 管理员终端运行接收器（控制设备 ACL 只允许 SYSTEM/管理员；只允许一个接收器）。指定**实际输出设备 ID**，不允许静默回退或送回虚拟端点。

`powershell -File apps/windows-system-audio/install-driver.ps1` 可在上述测试环境注册设备并自动请求 UAC；`-Remove` 删除该设备实例（保留 DriverStore 包，便于重装）。脚本不修改启动安全设置、不安装证书、不自动重启；UAC 被取消、签名检查或设备启动失败时会报错，需要重启时明确报告。

```powershell
cargo run --manifest-path tools/windows-audio-probe/Cargo.toml --bin sda-windows-audio-probe
node apps/windows-system-audio/receive.cjs --output=<实际输出ID> --seconds=60 --save=tmp/captured.sdac
```

4. 另一终端制作已知载波并发送到 SDA 虚拟端点；制备工具限定 48k、六块独立帧及关联依赖子流，其他排列会明确拒绝。

```powershell
node apps/windows-system-audio/pack-eac3.cjs harletty-bridge/harletty/tests/fixtures/joc_atmos_1s.eac3 tmp/test.spdif
tools/windows-audio-probe/target/debug/send.exe "<SDA虚拟端点ID>" tmp/test.spdif
```

5. 验证录制：`node apps/windows-system-audio/receive.cjs --replay=tmp/captured.sdac --verify-only`；须进一步将实际接收的完整节目区间与原文件解码结果对齐比较。然后测试至少两款真实播放器的 DD+ 直通、暂停、切歌、跳转、异常退出和持续播放。播放器若已输出 PCM，无法从中恢复原对象。

`test.cjs <新文件路径>` 可生成离线 SDAC 记录，使用 `receive.cjs --replay=<路径> --output=<实际设备ID> --volume=0` 复测原生渲染协议。该数据是生成的测试记录，不是驱动抓取证据。

## 生命周期与尚待验收

- 内核回调仅复制原始字节；单次锁内复制最多 16 KiB，队列 2 MiB，不解码、不写音频文件、不等待 Electron。
- 暂停/停止、换流、接收器重开、队列溢出和超过 DMA 环的时间跳变会清空旧数据、改变 epoch。接收器随之重置解包器、解码器、对象源和渲染 FIFO；不会拼接断流前后的压缩片段。
- 输入为保护内容时拒绝用户态读取/保护会话，不提供 DRM 绕过。不是受保护音频路径实现。
- 接收字节位置与原生已消费样本位置分开使用；目前沿用 SysVAD 的软件 DMA 时钟，**还没有完成播放器视频延迟补偿、跨设备时钟漂移、长期稳定性和驱动故障恢复验收**。不能承诺影视音画同步。
- 当前入口是独立命令行程序，没有向桌面菜单暴露未经实机验证的开关；房间、头追和 UI 状态共享仍须在系统入口实机验收后接入。
- 其他格式、驱动服务的普通用户授权、正式驱动签名、安装/卸载和桌面发布均未完成。

研究边界见 [系统输入研究](../../docs/system-atmos-input-research.md) 和 [macOS 研究](../../docs/macos-system-audio-integration.md)。
