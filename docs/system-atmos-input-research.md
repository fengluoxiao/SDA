# Windows 系统音频与空间码流接入：Atmos / DTS:X / 360RA

日期：2026-09-18。范围：通用第三方应用输入，不限 PotPlayer。仅调研，未实现驱动，也未验证播放器格式协商。

macOS 的独立交接见 [macOS 系统音频与空间码流接入](macos-system-audio-integration.md)。两端共用解码/对象事件模型，设备层不能直接照搬。

## 结论

保留对象的候选方案是支持编码码流的 SDA 虚拟音频接收端：应用主动直通完整 Atmos 码流，驱动向 SDA 用户态服务交付原始数据，SDA 解封装、解码并渲染。它不是普通立体声/7.1 虚拟声卡，也不是 WASAPI loopback。

微软文档证明 Windows 存在对应格式描述与音频驱动开发机制；这些依据不等于一个 SysVAD 示例稍作修改就能兼容所有播放器。必须验证非 PCM 格式协商、数据完整性、位置时钟和应用实际选择的输出格式。

| 应用交付的数据 | SDA 能否得到对象 | 接入边界 |
| --- | --- | --- |
| E-AC-3 JOC / DD+ Atmos 原始直通 | 保留完整载荷后，可交给现有解码路径验证 | 新增 IEC 61937 接收、解包与实时输入桥 |
| TrueHD Atmos 直通 | 候选可行，需恢复完整 TrueHD 流 | 处理对应 MAT/IEC 封装、同步和预热 |
| Dolby MAT 2.0/2.1 | 不可据现有 TrueHD 支持直接宣称可用 | MAT 2.x 可携带 LPCM，须单独研究元数据和解码支持 |
| 普通立体声 / 多声道 PCM | 没有原始对象 | 可做虚拟音箱、房间与双耳渲染 |
| 应用提交给 Windows 的空间对象 | 未找到通用的跨进程对象捕获接口 | ISpatialAudioClient 是提交/渲染接口，不是其他应用对象的监听接口 |

## 微软文档核对

1. [IEC 61937 格式表示](https://learn.microsoft.com/en-us/windows/win32/coreaudio/representing-formats-for-iec-61937-transmissions)
   - 列出 `KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_DIGITAL_PLUS_ATMOS`、`DOLBY_MLP`、`DOLBY_MAT20`、`DOLBY_MAT21`，以及 DTS-HD/DTS:X 格式。
   - 传输载波格式不等于内容格式。文档的 48 kHz DD+ 示例使用 192 kHz、双通道传输；不能把传输缓冲当作可听双声道 PCM。
   - MAT 1.0 和 MAT 2.x 分别列出；后者可携带 LPCM，不能笼统按 TrueHD 解包。
2. [IAudioClient::IsFormatSupported](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-isformatsupported)
   - 应用在初始化前协商格式。共享模式与独占模式的格式支持判断不同。
   - 虚拟端点必须实际接收其宣称支持的格式；不能只改设备名称或显示 Atmos 标签。
3. [WASAPI Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
   - 回环用于捕获渲染端点的播放流/系统混音，只支持共享模式，不能用来截获独占直通流。
   - 按进程选择捕获对象也不能恢复已经被混音丢失的空间对象元数据。
4. [Microsoft Spatial Sound](https://learn.microsoft.com/en-us/windows/win32/coreaudio/spatial-sound) 与 [ISpatialAudioClient](https://learn.microsoft.com/en-us/windows/win32/api/spatialaudioclient/nn-spatialaudioclient-ispatialaudioclient)
   - 应用提交静态/动态对象的音频与位置，系统选择空间渲染输出。
   - 文档也描述了 Media Foundation 的 Atmos 播放集成，但这不等于向第三方开放对象解码输出。
   - 本次查阅未找到第三方注册成任意应用通用空间对象捕获器的公开接口；不能承诺虚拟 PCM 声卡能接走这些对象。
5. [SysVAD 官方示例](https://github.com/microsoft/Windows-driver-samples/tree/main/audio/sysvad)
   - 可参考其 WDM/WaveRT 虚拟端点、缓冲与时钟结构；不是现成 Atmos 接收驱动。
6. [APO 架构](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/audio-processing-object-architecture)
   - SFX/MFX/EFX 是音频处理插入位置。普通 APO 并不提供恢复原始 Atmos 对象的通用机制。

## 建议原型

应用码流直通 → SDA 虚拟编码音频端点 → 有界传输缓冲 → 用户态接收服务 → IEC/MAT 解包 → SDA 解码器 → 声床/对象 PCM 与对象事件 → SDA 原生渲染 → 实际耳机。

- 第一阶段只声明 DD+ / E-AC-3 直通支持；保留 JOC 数据，不转码为普通 AC-3。兼容常规 DD+ 子类型的播放器，不能只接受 Atmos 专用 GUID。
- 驱动只负责协商、传输和时钟。解码、房间/HRTF 运算放用户态，不放内核回调中。
- 压缩载荷不进行音量乘法、重采样、声道混合；音量放在解码后处理。
- 第二阶段增加 TrueHD 对应封装；MAT 2.x 独立立项，未验证前不向系统宣称支持。
- 独占编码输入不允许混入系统提示音；需要定义其他应用 PCM 的独立端点/混音策略。
- SDA 输出明确选择物理设备，防止再次送入自身虚拟端点。
- 处理暂停/恢复、停止、断流、格式切换和跳转后的重新同步；旧缓冲必须丢弃，不能播放上一段音频。
- 普通端点接收流未必附带影片绝对时间或 seek 命令，不能把字节输入当作播放器控制协议。依靠驱动流状态和流内同步恢复；音画同步依赖可信消费位置、时钟及延迟报告。
- 对受保护播放链路、浏览器是否愿意直通、系统实时 MAT 编码是否接受该端点，分别验证，不承诺所有软件通用。

## SDA 当前基础与缺口

`packages/core/src/lib.rs` 已提供 `Pipeline::push`、有状态 `SdaDecoder` 和帧/对象事件输出，包含 E-AC-3、TrueHD 等路径。这提供解码复用基础，但不证明所有 Atmos 变体均支持。

`apps/native-renderer/src/protocol.rs` 已接收声源 PCM 和对象事件；仍需新增系统输入生产者、码流解包、格式生命周期和时钟桥。不能只把原始码流送进当前 PCM 命令。

验收应比较同一文件直接播放和虚拟端点直通：解包载荷完整性、解码 PCM/对象事件时间轴、对象数量/位置、跳转后元数据恢复、长时间音画同步，以及 ASIO/WASAPI 输出和设备断开。先证明 DD+ Atmos 端到端对象保留，再扩大范围。

## 360RA / MPEG-H 补充核查

2026-09-18 直接核对微软公开 SDK 头文件，发现 **Windows 已定义 MPEG-H 的 IEC 61937 子格式**。此前仅阅读 Learn 的格式列表并不足以确定完整支持范围。

来源：[微软 win32metadata 的 ksmedia.h](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/RecompiledIdlHeaders/shared/ksmedia.h)。其中包含：

- `KSDATAFORMAT_SUBTYPE_IEC61937_MPEGH_LEVEL1_LC` 至 `LEVEL5_LC`。
- `KSDATAFORMAT_SUBTYPE_IEC61937_MPEGH_LEVEL1_BL` 至 `LEVEL5_BL`。
- 例如 LC Level 1 为 `000110bf-0cea-0010-8000-00aa00389b71`，BL Level 4 为 `000240bf-0cea-0010-8000-00aa00389b71`。

这些定义证明系统有用于表示 MPEG-H 编码传输的格式标识，不证明系统自带对象解码器，也不证明任何播放器或驱动已实现相应直通。

播放器侧核查（本次读取的上游分支，非所有播放器的穷尽调查）：

| 来源 | 查到的事实 |
| --- | --- |
| [mpv audio-spdif 文档](https://github.com/mpv-player/mpv/blob/master/DOCS/man/options.rst) | 列出 ac3、dts、dts-hd、eac3、truehd、dsd，未列 MPEG-H |
| [FFmpeg spdifenc.c](https://github.com/FFmpeg/FFmpeg/blob/master/libavformat/spdifenc.c) | 编码格式分支有 AC3/EAC3、MP1/2/3、DTS、AAC、TRUEHD/MLP，未见 MPEG-H 分支；MP1/2/3 不是 MPEG-H |
| [LAV Audio Bitstream.cpp](https://github.com/Nevcairiel/LAVFilters/blob/master/decoder/LAVAudio/Bitstream.cpp) | 码流媒体类型构造分支为 AC3、EAC3、TRUEHD、DTS，未见 MPEG-H |
| [Fraunhofer MPEG-H decoder](https://github.com/Fraunhofer-IIS/mpeghdec) | 提供 MPEG-H 解码实现；解码能力本身不能证明播放器实现 Windows 原始码流直通 |

因此应将 MPEG-H 纳入统一原始码流入口的设计，但不能宣传“任意 Windows 360RA 播放器选择 SDA 后就能传对象”。本次没有确认可直接使用的通用 MPEG-H 直通播放器。

可行验证路线：先编写最小发送程序，按照 MPEG-H 相应 IEC 61937 封装及 SDK 子格式协商发送已知内容，验证 SDA 虚拟端点数据完整性和对象恢复；再对接支持这种输出的应用。封装规范、LC/BL 与内容 level、初始化配置和重同步均须核实，不能把 M4A 文件字节直接写入音频缓冲。如果现有播放器没有直通能力，需要补播放器输出模块，或提供原始帧/必要配置/时间戳的插件接口。已输出双耳 PCM 的 360RA 只能作为 PCM 接收。

## Windows 实施交接

### 两类输入与统一传输契约

系统 PCM 与原始编码输入共用生命周期，但必须明确区分内容类型。建议新增如下版本化消息，以下是待实现设计，并非现有 IPC：

| 消息 | 必需信息 / 行为 |
| --- | --- |
| Open | 协议版本、streamId、epoch、PCM/编码类别、codec、封装、配置字节、采样率；PCM 附通道布局，编码内容附 profile/level |
| Data | streamId、epoch、递增 sequence、长度、载荷；PTS/timebase 若源可提供，设备输入则提供帧位置与单调 host time 并标记时间来源 |
| Discontinuity | 切歌、跳转、格式变化或丢包；递增 epoch，清空旧 PCM/对象事件并重新预热 |
| State / Close | 启动、暂停、停止、结束；排空或丢弃须显式区分 |
| Feedback | 已接收/已解码/已消费位置分别报告，队列水位、欠载数和总延迟；不能把收到数据当作已经听到 |

载波时钟、内容采样率和实际耳机时钟分别记录。缓冲有上限；满时提供背压或明确报溢出，不能悄悄丢掉压缩帧继续播放。驱动回调不等待 Electron，不分配大块内存，不运行解码器。跨进程共享缓冲和控制管道限制为本机授权客户端，并校验长度、格式和序列。

### 工程入口

- Windows 设备层：以 SysVAD 为参考新建独立工程，先验证 PCM，再逐个验证编码 pin 数据范围、独占格式查询、传输周期和位置报告。SysVAD 没有替 SDA 完成这些功能。
- 用户态接收层：新增原始流生产者。首个原型可在 worker 复用现有解码器，不必一次性将所有解码器迁入 Rust。
- Atmos/DTS：复用 `packages/core/src/lib.rs` 的流式路径。
- 360RA：复用 `packages/core/src/mpegh.ts` 和 `packages/core/mpegh/bridge.c`；当前不是直接使用上述 Rust Pipeline。`mha1` 原始 AU 需要 `mhaC` 配置，MHAS 则按包连续输入。现有混合 objects/HOA 等限制必须保留，不能宣称所有 MPEG-H 都可恢复对象。
- 调度参考 `packages/player/src/player.ts` 的 `NativeRendererSink`；原生入口为 `apps/native-renderer/src/main.rs` 与 `protocol.rs`。新增输入不能改变现有文件播放状态机的语义。
- UI 分开显示“系统 PCM”和“空间码流”，实际接收到支持的对象事件后才显示对象模式。显示真实 codec，不以应用名推断格式。

### 里程碑与验收门槛

1. PCM 端点：立体声和已知 7.1 通道顺序、重采样、无回授、退出恢复。此阶段不算对象接入完成。
2. DD+ Atmos：已知文件通过最小发送器直通，接收载荷与源音轨一致，解码后的对象事件/PCM 与直接播放对齐。
3. TrueHD / DTS:X：分别验证封装、间断恢复和完整扩展数据，不能仅验证 AC-3/DTS core。
4. MPEG-H：校验 profile/level 和完整初始化信息，分别验证 MHAS / 原始 AU 来源，对照已知移动对象样本。
5. 应用互操作：至少两个独立应用；记录其版本、实际选择的格式、是否直通和是否需要插件。MAT 2.x 独立验收。
6. 持续运行至少 30 分钟，包含暂停、连续跳转、切音轨、采样率变化、蓝牙断连/重连、SDA 意外退出与恢复；记录延迟和漂移，不仅主观听音。

发布还需完成驱动签名、安装/卸载和系统兼容测试。这是工程交付条件，不属于本次调研已完成事项。

## API 证据边界与下一阶段

WASAPI 的 `IAudioClient::IsFormatSupported` / `Initialize` 和 `IAudioRenderClient` 用于格式协商及应用提交数据；IEC 61937 描述结构和子格式标识用于声明编码传输；自定义驱动负责接收。原始对象来自 SDA 自己的解码器，不是 WASAPI 提供的对象读取接口。`ISpatialAudioClient` 是应用向系统提交空间对象的接口，也不能用来读取其他应用的对象。

macOS 同样有 Core Audio 流格式查询/设置、AudioDevice IO 回调和 AudioServerPlugIn 虚拟端点机制，且 macOS 15 已确认 Dolby HDMI 原始直通。不能把“Windows 有这些接口”与“Windows 已证实能接收对象”画等号，也不能提前否定 macOS；详见配套 Mac 文档的 API 对照。

Windows 开发分支首先进行无副作用的端点编码格式探测和最小码流收发原型。探测返回支持，只说明端点接受格式查询，不说明播放器会选择直通，更不说明已经收到或解码对象。先完成完整载荷和对象时间线对照，再扩展完整产品功能。

2026-09-18 Windows 分支已加入 [可构建的实验工程](../apps/windows-system-audio/README.md)：WaveRT 接收驱动、受限读取器、DD+ IEC61937 解包和原生渲染接收程序。离线载荷、对象事件和 PCM 对照已通过；尚未完成管理员安装后的实际播放器直通验收，不应将这部分写成已发布的系统对象捕获功能。
