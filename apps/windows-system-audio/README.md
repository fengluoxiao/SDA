# Windows 系统音频输入（实验工程）

本分支包含 WaveRT 虚拟输出端点、受限用户态读取器、IEC61937 DD+ 解包、SDA 对象解码和原生双耳渲染接收程序。本机已接入开发版桌面并安装 0.1.0.7（PnP Code 0），完成多声道 PCM 到实际 Realtek 输出的录回检查、共享输入的处理后回环采集检查。尚未完成第三方播放器对象直通和长期稳定性验收。

路径：支持直通的播放器 → SDA 虚拟端点 → 有界原始数据队列 → capture.exe → IEC61937 解包 → SDA E-AC-3/JOC 解码 → PCM + 对象事件 → SdaNativeRenderer → 明确指定的实际输出设备。

只声明本原型支持的 48 kHz PCM（1/2/5.1/7.1/7.1.4，16/24/32-bit 整数或 32-bit float）、DD+ 和 DD+ Atmos（192 kHz 双声道 16-bit 载波）。PCM 输入不会伪装成原始对象。TrueHD/MAT、DTS:X、MPEG-H 尚未接入此系统入口，原有文件解码能力不受影响。发送工具拒绝把压缩载波发往物理耳机。

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

本机安装记录（2026-09-18）：0.1.0.4 已安装并加载，PnP Code 0，全部 Startup 阶段成功。此前 Code 10 / `0xC0000184` 来自保留了 `ENDPOINT_LOOPBACK_SUPPORTED`、但回环流数量为零的矛盾配置；已移除该能力标志。同时移除上游启动演示调用，避免覆盖真实端点注册错误。

实机验收：测试发送器经 WASAPI 独占向 SDA 虚拟设备发送 1155072 字节 IEC61937 载波，管理员读取器实际抓取 1220304 字节 SDAC 记录。发送、抓取均退出 0，队列溢出为 0；抓取数据解出 47 帧、15 个对象、705 条事件，PCM 与对象元数据统一 SHA-256 与上文原文件参考值完全一致。实际抓取数据进入原生逐对象 HRTF 回放程序，指定 Realtek 输出、音量 0，完成 47 帧协议处理且无解码诊断；这不是主观听音验收，也不代表第三方播放器或持续实时渲染已通过。

本机测试证据位于忽略目录 `tmp/live-driver-test.log`、`tmp/live-driver-capture.sdac`、`tmp/live-driver-render.log`，不包含在仓库分发中。

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
- Windows 开发版左侧新增“系统音频”面板，开始/停止接收、状态和帧/对象计数；通过 UAC 启动受限读取器，经随机命名且令牌认证的本机管道送入桌面现有原生渲染器。沿用输出设备及开始时的耳廓、房间/监听、方向/近场设置。打开文件或切换输出时停止系统输入；虚拟输入作为输出时拒绝启动，防止反馈。
- 桌面适配层已使用实机抓取的 SDAC 验证 47 帧、15 对象及渲染拒绝错误传播。桌面 UAC 管道到持续实际听音、3D 对象可视化和头追联动尚未完成验收；发布包暂不开放启动。
- 其他格式、驱动服务的普通用户授权、正式驱动签名、安装/卸载和桌面发布均未完成。

研究边界见 [系统输入研究](../../docs/system-atmos-input-research.md) 和 [macOS 研究](../../docs/macos-system-audio-integration.md)。

## 系统默认输入与远程采集

- Windows 默认输出可以保持 SDA 虚拟设备，SDA 输出设置选实际耳机/音箱。
- 驱动的硬件回环 pin 与输入队列分开。原生渲染器在消费最终双耳音频时复制一份，经令牌验证的本机 TCP、现有 UAC 双向管道、`capture --return` 写入 `SDA_RETURN`。回环只读这份处理后的音频，不读取原始载波，不回灌渲染输入。
- 音频回调不做网络/驱动 IO；队列有界，过期返回数据转为静音。停止系统接收、管道断开会清空回送数据。控制设备仍只允许管理员/SYSTEM，写入长度有上限，不处理受保护内容。
- **WASAPI 独占输入仍会拒绝 Windows 共享回环采集**（实测 `AUDCLNT_E_DEVICE_IN_USE`）。需要远程软件同时采集时，播放器使用共享 PCM 输出；Windows 声音设置的虚拟设备配置需选所需的多声道布局，否则音频引擎可能先混成双声道。DD+ 独占直通的本地播放与远程软件回环不能据此承诺同时工作。
- 实机测试将 7.1 / 7.1.4 的隔离声道测试音经驱动、桌面解码、原生渲染送至 Realtek，并录到非零音频。另在共享输入下同时录到 Realtek 和虚拟设备回环，用户已确认远程声音恢复；不代表所有远程软件和所有独占组合都通过。
- 系统输入模式的声道 Mute / Solo / 聚焦直接使用同一原生音箱控制接口，不依赖文件 Player。全部 Mute 的实际 Realtek 录音中段为零，取消后恢复，类型检查通过。

探针：`send-pcm <SDA设备ID> <2|6|8|12> [--shared]`；`loopback <设备ID> <1..60秒> <原始PCM输出路径>`。后者输出格式见 stderr，不要假定总是双声道 float。


## Discrete PCM input layouts (driver 0.1.0.9)

All 13 SDA renderer layouts have an explicit input channel contract in `layouts.json`. The virtual endpoint advertises 48 kHz PCM16/24/32 and float32 for their channel counts, up to 24 channels. Standard nonzero Windows speaker masks retain their own ordering; they are never relabelled by the discrete selection.

For zero-mask (`dwChannelMask = 0`) discrete streams, select the input layout in SDA before starting reception. The selection stays fixed for that capture session, independently of live output-layout changes. A missing layout or mismatched channel count is rejected rather than guessed. Senders must use the listed channel order. Changing this selection does not make an ordinary 7.1 player emit 20 or 24 channels and does not add an upmixer. Windows' shared audio engine may still mix to its configured format; exact multichannel support must be negotiated by the sender.

| Layout | Channels | Input order |
|---|---:|---|
| 2.1 | 3 | FrontLeft, FrontRight, LFE |
| 2.0 | 2 | FrontLeft, FrontRight |
| 5.1 | 6 | FrontLeft, FrontRight, Center, LFE, SurroundLeft, SurroundRight |
| 5.1.2 | 8 | FrontLeft, FrontRight, Center, LFE, SurroundLeft, SurroundRight, TopMiddleLeft, TopMiddleRight |
| 5.1.4 | 10 | FrontLeft, FrontRight, Center, LFE, SurroundLeft, SurroundRight, TopFrontLeft, TopFrontRight, TopRearLeft, TopRearRight |
| 7.1.2 | 10 | FrontLeft, FrontRight, Center, LFE, SurroundLeft, SurroundRight, RearLeft, RearRight, TopMiddleLeft, TopMiddleRight |
| 7.1.4 | 12 | FrontLeft, FrontRight, Center, LFE, SurroundLeft, SurroundRight, RearLeft, RearRight, TopFrontLeft, TopFrontRight, TopRearLeft, TopRearRight |
| 9.1.2 | 12 | FrontLeft, FrontRight, Center, LFE, WideLeft, WideRight, SurroundLeft, SurroundRight, RearLeft, RearRight, TopMiddleLeft, TopMiddleRight |
| 9.1.4 | 14 | FrontLeft, FrontRight, Center, LFE, WideLeft, WideRight, SurroundLeft, SurroundRight, RearLeft, RearRight, TopFrontLeft, TopFrontRight, TopRearLeft, TopRearRight |
| 9.1.6 | 16 | FrontLeft, FrontRight, Center, LFE, WideLeft, WideRight, SurroundLeft, SurroundRight, RearLeft, RearRight, TopFrontLeft, TopFrontRight, TopMiddleLeft, TopMiddleRight, TopRearLeft, TopRearRight |
| 360RA-13 | 13 | FrontLeft, FrontRight, Center, SurroundLeft, SurroundRight, UpperFrontLeft, UpperFrontRight, UpperCenter, UpperRearLeft, UpperRearRight, LowerFrontLeft, LowerFrontRight, LowerCenter |
| 22.2 | 24 | I_M_L060, I_M_R060, I_M_000, LFE, I_M_L135, I_M_R135, I_M_L030, I_M_R030, I_M_180, LFE2, I_M_L090, I_M_R090, I_U_L045, I_U_R045, I_U_000, I_T_000, I_U_L135, I_U_R135, I_U_L090, I_U_R090, I_U_180, I_L_000, I_L_L045, I_L_R045 |
| 11.1.8 | 20 | FrontLeft, FrontRight, Center, LFE, WideLeft, WideRight, SurroundLeft, SurroundRight, RearLeft, RearRight, TopFrontLeft, TopFrontRight, TopRearLeft, TopRearRight, Surround1Left, Surround1Right, FrontHeightLeft, FrontHeightRight, RearHeightLeft, RearHeightRight |

Validation: `node apps/windows-system-audio/test-discrete-layouts.cjs` checks every channel with isolated impulses, all four sample formats, fragmented records, and parity with the renderer layout catalog. `send-pcm <endpoint> <count> --discrete --check` checks the installed driver's exact descriptor; without `--check` it sends one second per channel. A format-support result alone does not prove audible end-to-end playback.


### Windows shared-format negotiation verification

On the current Windows machine, PolicyConfig can publish all counts, but actual shared-client initialization succeeds only through 12 channels. 13/14/16/20/24-channel zero-mask shared streams return E_INVALIDARG. SDA therefore leaves the existing working Windows mix unchanged for those layouts and supports them through exclusive discrete input only. Do not claim that every ordinary shared-mode player can see and render these layouts. 7.1.4 publishes the standard 0x2d63f mask and 12 channels; actual shared tone submission succeeded. The device format uses integer PCM32 and the mix format float32: setting both to float had failed initialization despite SetDeviceFormat returning success.

The format helper uses the undocumented IPolicyConfig compatibility interface, checks the result, and restores the previous format if the requested format is not retained. It does not elevate. It changes only the SDA endpoint, not physical output devices. This is format negotiation, not lossless object decoding or upmixing.


### Speaker mask consistency (Microsoft documentation review)

A 12-channel GetMixFormat result alone is insufficient to verify layout publication. On this machine the endpoint's PKEY_AudioEndpoint_PhysicalSpeakers still reported 0x63f (7.1) while the mix mask was 0x2d63f (7.1.4). The configuration helper now synchronizes PhysicalSpeakers using IMMDevice::OpenPropertyStore / IPropertyStore::SetValue and Commit, verifies both properties, and restores them on failure. Verified 0x2d63f and successful shared loopback initialization after the correction. No player settings were inspected or changed for this fix.

The earlier >12 shared-client failures are observations of this experimental endpoint/configuration, not a documented universal Windows channel limit. Microsoft states that a shared client should accept GetMixFormat; these failures still require endpoint/format investigation.

References:
- https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-getmixformat
- https://learn.microsoft.com/en-us/windows/win32/coreaudio/pkey-audioendpoint-physicalspeakers
- https://learn.microsoft.com/en-us/windows/win32/api/mmreg/ns-mmreg-waveformatextensible
- https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/ksproperty-audio-channel-config

Speaker masks identify positions, not a decoder or height-generation algorithm. A 5.1 PCM stream does not contain four extra height channels. Recovering Atmos positions requires an object-aware decode/render path or original encoded-stream delivery, not just publishing a larger device layout.

### Desktop DD+ / Atmos bitstream reception

The desktop system-audio panel now offers **DD+ / Atmos 原始码流** and **PCM / 自动识别**. Explicit bitstream mode leaves the Windows PCM mix/layout unchanged: the sender negotiates the encoded WASAPI stream. PCM input is discarded with a visible waiting message, rather than being mistaken for a successful Atmos render. The connection remains open and recovers when the sender switches to DD+. The automatic mode retains the existing PCM layout configuration and format detection.

For PotPlayer, select the SDA virtual endpoint with its WASAPI renderer and enable E-AC-3 / DD+ passthrough in the active audio decoder. PCM 7.1.4 selection is not a substitute for passthrough. This is not automatic configuration of third-party players; the current PotPlayer configuration has not been verified. TrueHD / DTS:X passthrough is not implemented. The receiver remains experimental/development-only.

Decoded object PCM and position events go to the same native renderer protocol as file playback. Active object counts use mapped source IDs, because object declarations are not repeated in every frame. A plain DD+ frame without object sources is not labelled Atmos. Physical output remains shared; exclusive encoded input can still prevent third-party loopback capture of the virtual endpoint.

Validation (2026-09-18): `node apps/desktop/test-system-audio-bitstream.cjs` checks PCM rejection, incomplete-burst seek reset and automatic recovery on the same connection. Two decoded passes (94 native batches) match the direct-file decoder's PCM, IDs, sample clocks and metadata. Optional argument `<captured.sdac>` verifies a real driver capture against the same fixture. This machine's fresh driver capture matched all 47 reference batches (15 objects / 705 events, no overflow), and replay through the native HRTF renderer at volume zero completed without decoder diagnostics. These are fixture/protocol checks, not PotPlayer or subjective listening acceptance. Local evidence: `tmp/verify-bitstream-live.log`, `tmp/verify-bitstream-live.sdac`, `tmp/verify-bitstream-native.log`.

Driver 0.1.0.10 adds DD+ carrier mask variants (unspecified, stereo, 5.1 back/side and 7.1), and accepts 2/6/8-channel encoded-content descriptors at 48 kHz. All still use the same two-channel 192 kHz IEC carrier; none are PCM or additional codec support. The hardware loopback list remains PCM-only. Before this change, the installed driver rejected stereo/unspecified mask probes with AUDCLNT_E_UNSUPPORTED_FORMAT. This is a confirmed format-compatibility gap, not yet proof of PotPlayer's exact rejected descriptor.

On 2026-09-18, the new package built, passed INF validation, was test-signed and installed. Windows setupapi.dev.log reported PNP_VetoDevice / CR_REMOVE_VETOED and required reboot; the old loaded driver still rejects the new formats. PotPlayer's renderer was set to the SDA endpoint with exclusive mode and EAC3 FFmpeg passthrough, but playback still fell back to PCM before reboot. End-to-end PotPlayer acceptance remains pending; do not report it fixed based on the installed package version alone.

Post-reboot verification (2026-09-18): driver 0.1.0.10 now accepts both the stereo and unspecified DD+ carrier masks. The endpoint ID changed after replacement, so the PotPlayer WASAPI passthrough renderer was rebound to the current SDA endpoint. Its current configuration had EAC3 passthrough disabled; enabled FFmpeg Pass Through for EAC3, IEC61937 on, compatibility connection mode off, and exclusive WASAPI output to SDA. Other codec passthrough options were left unchanged. Actual playback of `15. Ether.m4a` then produced EAC3 input, 15 object sources plus LFE, and 844 decoded frames; native output was active with HRTF ready and zero reported underruns at that checkpoint. This confirms the real PotPlayer-to-SDA object/render path, but is not a subjective listening, long-duration stability, or UU loopback acceptance test.

### Dual endpoints for exclusive bitstream and remote capture (0.1.0.11)

The earlier workaround changing Windows default to physical output is removed: it bypassed SDA for applications following the default device.

Keep the original SDA endpoint as Windows default. Point the player's exclusive DD+ renderer to **SDA Spatial Bitstream Input - Dedicated**. SDA renders to the selected physical shared output and returns that rendered stereo into the default SDA endpoint's hardware loopback. Dedicated exclusive input no longer occupies the default endpoint's shared audio engine. Receiver startup does not change the default device.

The driver prioritizes an active encoded sender over shared PCM and returns to an already running PCM sender when encoded playback stops. This is arbitration, not simultaneous mixing of applications: shared PCM is not mixed into an active encoded programme; bitstream-only mode still rejects PCM. Protected input remains blocked. Independent PortCls miniports share the bounded rendered-return ring.

Installing this package may require reboot. Build success does not prove live loopback or UU client acceptance.

0.1.0.11 validation: WDK build, INF validation, catalog generation and test signing passed. A host-compiled harness exercised the actual SelectInput / SdaCaptureState / Close / Protected functions extracted from SdaCapture.cpp: encoded priority, concurrent PCM start without stealing the source, pause/resume/close fallback and protected-state transitions passed. Existing system-input PCM format-change and DD+ seek/recovery/object-parity tests passed. Installation has been launched through install-driver.ps1; live dual-endpoint enumeration and UU acceptance are still pending. The prior single-endpoint return test is not evidence of dual-endpoint driver acceptance.

Installation result at 2026-09-18 16:05:27: package 0.1.0.11 published as oem172.inf. SetupAPI completed successfully but reported PNP_VetoDevice / CR_REMOVE_VETOED and "Reboot needed to complete driver update". Only the previous endpoint is currently enumerated, so dual-endpoint loopback/UU verification must continue after a user-initiated reboot. Web TypeScript noEmit check also passed. Next steps: enumerate the dedicated endpoint, restore the original SDA system default, bind PotPlayer's WASAPI device to Dedicated, start the receiver, and verify original-endpoint loopback during exclusive DD+ playback plus Solo/Mute and actual remote receipt.

Post-reboot dual-endpoint verification (2026-09-18): both endpoints enumerate. Windows initially gave them identical sample names and selected Dedicated as the default. `configure-endpoints.ps1` identifies them by driver topology, gives them distinct device descriptions, and moves console/multimedia defaults from Dedicated to System / Remote only when they currently point to Dedicated. `-DiscoverOnly` performs no mutations; receiver discovery uses the topology IDs, not ambiguous friendly names.

PotPlayer's live WASAPI settings still selected the default device, producing AUDCLNT_E_DEVICE_IN_USE (0x8889000A) on System / Remote. Selected Dedicated explicitly, disabled secondary output, saved, then stopped and reopened the file to release the old graph. Dedicated now reports the player's exclusive session; System / Remote initializes shared loopback successfully while SDA continues rendering 15 Atmos objects. A three-second, 48 kHz capture contained rendered stereo in channels 0/1 (peaks 0.437/0.450, RMS 0.094/0.093); the remaining ten mix channels were zero. Windows default remains SDA System / Remote. This verifies concurrent object rendering and default-endpoint capture, not actual UU client receipt or subjective listening. Remote listening and Solo/Mute acceptance remain to be confirmed.

User acceptance (2026-09-18): after the endpoint rebind and playback graph restart, the user confirmed UU remote sound works. Solo/Mute was not separately re-tested during this final routing check.
