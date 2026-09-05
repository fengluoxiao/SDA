# SDA 双耳渲染设计文档

> 基于 Dolby / Apple 官方文档与开源实现（EBU BEAR）调研整理。
> 本文档是 `packages/renderer` 及移动端原生渲染的设计依据。
> **2026-08 更新：§1–§5 描述的管线已全部落地实现**（见各节「实现」注记）。

## 1. SDA 总体架构：虚拟扬声器 + BRIR 卷积

以下是 SDA 当前采用的虚拟扬声器总线架构。它保留每个对象的 PCM、位置、增益和 Solo 控制，
再把对象分配到固定方向总线后卷积；这与直接为每个对象位置计算 HRTF 不同。
Dolby 公开资料强调独立动态对象的位置、大小和距离会影响双耳处理，并指出固定扬声器轨道
会限制空间分辨率、可能产生响度叠加。不能据此声称 Dolby 内部采用 SDA 的固定总线算法。
BEAR 是另一个可研究的开源实现，也不构成与 Dolby 或 Apple 算法等价的证据。

```
对象 PCM + 元数据 (x,y,z,gain,size)
  │
  ├─ (1) Panning: 对象位置 → N 个虚拟扬声器的增益向量
  │      VBAP 3D (Pulkki 1997)；size/spread → 向最近扬声器群扩散
  ├─ (2) 虚拟扬声器总线: 各对象按增益求和到 N 条总线（AudioWorklet）
  │      （复杂度与对象数解耦，卷积次数恒为 N × 2 耳）
  ├─ (3) 双耳化: 每条总线卷积对应位置的「干 HRIR ↔ 湿 BRIR 混合」IR
  │      → ConvolverNode（FFT 卷积，浏览器原生加速）
  ├─ (4) SDA 距离层: inverse 距离衰减（参考距离内不衰减）
  │      → SDA near/mid/far 使用不同房间尾声权重（见 §3）
  └─ (5) 头追(可选/v2): 旋转世界坐标→头部坐标，重算各扬声器相对方位
```

SDA 选择固定总线的工程考虑：
- 卷积数量由布局决定；逐对象卷积的成本随对象数增长，实际性能需要按平台测量。
- Web Audio PannerNode 不直接提供对象 size/spread 或房间响应，仍需应用补充。
- 床声道与动态对象可共用总线输出；逐对象 HRTF 也可与床声道处理共存。

这种选择有空间采样与频响上的取舍。总线到双耳的相干叠加不保证单位功率 VBAP
在耳机上保持相同响度，不能将固定总线称为已验证的 Dolby 等效渲染。

SDA 里 PannerNode HRTF 仅作为**无 IR 数据时的降级方案**（对 N 条虚拟扬声器总线各用一个 PannerNode，而非每对象一个）。

> **实现**：worklet 总线混合 + 每源低通在
> `packages/renderer/worklet/sda-renderer.worklet.js`；双耳图（splitter →
> 每总线 ConvolverNode → merger）在 `packages/renderer/src/renderer.ts`；
> SDA 将 LFE 等量直送双耳、不做方向卷积。Apple 的 Dolby 插件文档注明 LFE
> 的 Binaural Render Mode 固定为 Off，但这不说明厂商内部全部低频处理步骤。

### 1.1 床声道：音箱吸附 + 上混扩展（AVR 语义）

床声道（5.1/7.1 等固定声道）与对象走不同平移路径，对齐真实 AVR/物理系统：

- **吸附（direct）**：床标签命中布局音箱 → 直送该音箱总线（增益 1.0），不用
  VBAP 摊到相邻音箱 —— 真实 7.1 系统放 5.1 内容，Ls 就是从侧环绕一只箱子出声。
  布局里没有该音箱（如 7.1 内容选 5.1 布局）才回退 VBAP 平移。
- **上混扩展（DSU 静态近似，仅多声道物理输出）**：所选布局大于内容床时把床
  填满布局——侧环绕馈后环 0.5（5.1 → 7.1.x 后环不再沉默，合成声像略后移，
  恰好贴近 5.1 环绕 ±110° 的制作位）、前左/右馈前宽 0.35（9.1 布局拉开前声场）。
  目标总线被真实床声道占用时馈送撤回（7.1 内容的后环不吃 5.1 式馈送）。
  顶层不做静态派生（环境声提取超出渲染器职责）。
  **为什么只限物理输出**：馈送是相干拷贝。物理后环在真实房间里被房间反射
  去相关，听着是"填满"；双耳里 BRIR(100°) + 0.5·BRIR(140°) 在鼓膜处同相
  叠加 → 梳状滤波 + 声像向中间涂抹，整个声场挤成一团。AVR 上混器对派生
  声道做去相关（Dolby DSU 的 decorrelator 正是为此存在），虚拟音箱域没有
  这个环节——也不需要有：吸附已把床放到混音师本来的位置，多一路相干
  拷贝只会弄脏声像。
- 三种输出模式共用吸附（增益向量层）；扩展只在多声道模式的增益向量里叠加：
  双耳 = 吸附总线各自卷积（床在精确位置，无涂抹）；多声道 = 扩展后总线直出
  物理声道。

多声道直出额外做两件物理对齐：`destination.channelCount` 显式设为布局
音箱数（否则浏览器默认 2 声道降混），总线按 WASAPI/HDMI 通道掩码顺序重排
（7.1：FL FR C LFE **BL BR SL SR**，后环在侧环前；不重排则 Windows 多声道
设备上侧环/后环互换）。

## 2. 虚拟扬声器布局（Dolby 声道身份 + ITU 标称方向）

Genelec 的《Immersive Solutions Guide》**没有发布专属扬声器角度或通道顺序**。它给出
不同房间规模的设备组合与典型听音距离，并要求摆位后用 GLM 校准每只音箱的电平、
飞行时间、相位和房间频响。因此 SDA 不存在一套笼统的“真力 The Ones + 7370A
布局”：Dolby 官方安装图定义 `.2/.4/.6` 与 Wide 的声道身份，固定虚拟方向则选在
ITU-R BS.2051-3 允许范围内。

| 位置 | SDA 标称方位角 | 标称仰角 | 规范依据与出现布局 |
|---|---:|---:|---|
| L / R | ±30° | 0° | ITU；全部布局 |
| C | 0° | 0° | ITU；全部布局 |
| LFE | 不适用 | 不适用 | 无空间角度；不参与卷积，直送低频总线 |
| Ls / Rs | ±110° | 0° | ITU 5.1 允许 ±100..120°；5.1 / 5.1.x |
| Ls / Rs | ±100° | 0° | ITU 7.1 允许 ±85..110°；7.1.x / 9.1.x |
| Lrs / Rrs | ±140° | 0° | ITU 7.1 允许 ±120..150°；7.1.x / 9.1.x |
| Lw / Rw（前宽） | ±60° | 0° | Dolby Wide 身份；SDA 标称角；9.1.x |
| Ltf / Rtf（前顶） | ±45° | +45° | ITU 允许 ±30..45°、+30..55°；`.4`、`.6` |
| Ltm / Rtm（中顶） | ±90° | +45° | Dolby `.2` / `.6` Top Middle；ITU upper-side 对应位置 |
| Ltr / Rtr（后顶） | ±135° | +45° | ITU 允许 ±100..150°、+30..55°；`.4`、`.6` |

因此 Dolby `.2 = 中顶`，`.4 = 前顶 + 后顶`，`.6 = 前顶 + 中顶 + 后顶`。5.1 的
±110° 与 7/9 声道的侧环 ±100° 使用独立工作总线和独立 IR，不能因声道标签同名
就在固定拓扑中合并。Dolby 9.1.x 在 L/R 和侧环绕之间增加 Wide；SDA 为它选择 ±60°
标称方向，但 Dolby 当前安装图没有用正文数字声明该角度，Genelec也没有声明。ITU
System G 的 `Lsc/Rsc` 是屏幕边缘声道，不是 Dolby Wide，二者不能互换。

Genelec 官方《Immersive Solutions Guide》（印刷页10–14）记录的示例监听系统是：

| 系统 | 典型距离 | LCR | 环绕 | 顶层 | LFE |
|---|---:|---|---|---|---|
| Compact 5.1.4 | 0.7 m | 8330 / 8331 | 8330 / 8331 | 8330 / 8331 | 1 × 7360 |
| Small 7.1.2 | 1.2 m | 8340 / 8341 | 8340 / 8341 | 8331 / 8341 | 1 × 7370 |
| Small 7.1.4 | 2.0 m | 8351 | 8340 / 8341 | 8341 | 1 × 7380 |

7370只出现在该指南的Small 7.1.2示例；指南没有发布“The Ones + 7370A 9.1.x”配方。
真实房间还必须由 GLM 在摆位后校准，不能把上述型号表当成双耳IR的逐布局EQ。

三维 VBAP 只使用扬声器方向凸包的实际三角面，不能让任意三个音箱跨越穹顶组面；
否则顶层对象会被错误分配到无关的地面扬声器。实现与布局定义分别在
`packages/renderer/src/vbap.ts`、`packages/renderer/src/layouts.ts`。

官方依据：
- Genelec《Immersive Solutions Guide》：https://assets.ctfassets.net/4zjnzn055a4v/4dG9V4oK3gUEsqXodIqRaJ/02ee79e6e3fd962e0d40ab14b86c8320/Genelec_Immersive_Solutions_Guide_-_2021.pdf
- ITU-R BS.2051-3：https://www.itu.int/dms_pubrec/itu-r/rec/bs/R-REC-BS.2051-3-202205-I!!PDF-E.pdf
- Dolby 5.1.2 / 7.1.2 / 7.1.4 / 9.1.4 / 9.1.6 官方安装图：https://www.dolby.com/about/support/guide/speaker-setup-guides/

## 3. Dolby 侧：binauralRenderMode 与 near/mid/far

元数据位于 BWF `dbmd` chunk 的 Dolby Atmos Supplemental Metadata segment
（DAMF/ADM profile, EBU Tech 3285 Suppl. 6）。取值（经开源解析器证实，
注意顺序不是按距离递增）：

| 值 | 名称 | 含义 |
|---|---|---|
| 0x00 | BYPASS | 不做双耳处理 |
| 0x01 | NEAR | 较近的距离模型；不推定物理米数 |
| 0x02 | FAR | 远场，房间感/混响最多 |
| 0x03 | MID | 中等距离模型，Dolby 插件默认档 |
| 0x04 | NOT_INDICATED | 未指定，渲染器用默认 |

**公开语义**：Near/Mid/Far 表示不同的距离模型；Off 关闭该声道/对象的双耳处理。
Dolby 将其定义为不能自动化的节目级元数据，Apple Logic Pro 允许对各对象和床声道分别设置。
这些设置不影响 Apple Renderer。已核对的公开资料没有给出精确物理距离、内部 BRIR 集
或干湿混合公式，以下权重属于 SDA 的应用设计，不能作为 Dolby 算法参数引用。

**SDA 的实现**（`packages/renderer/src/hrtf.ts`）：calibration v1 将目标方向的
**干 HRIR 直达路径**与同一个 KU100 房间的**校准 BRIR 房间尾声**离线对齐：
`wet = dry + roomTail`。运行时使用 `IR = dry + w·roomTail`，所以 Near/Mid/Far
只改变房间反射比例，不改变已校准的直达参考、共同 TOF、ILD 或 ITD。

| 档位 | 湿声权重 w | 听感 |
|---|---:|---|
| near 近（默认） | 0.04 | 少量早期 BRIR 线索，兼顾对象清晰度与后方/顶层外化 |
| mid 中 | 0.2 | 轻度房间感 |
| far 远 | 0.45 | 明显外化和房间感 |

默认 Near 只混入少量 BRIR 早期线索：后方和顶层定位需要这些外化 cue，但权重不足以让
房间尾音掩盖对象；需要更强外化时才由显式 Mid/Far 引入更多房间反射。

### 3.1 双耳输出标定

KU100 calibration v1 把 17 个虚拟音箱的双耳共同到达时间对齐到同一参考，并用
500–2000 Hz、4 ms 直达窗口把每方向参考电平对齐到稳健中位数。每方向只施加一组
左右共用的延时和标量，因此保留方向性 ILD/ITD；房间响应修正同样使用左右共用、
固定长度的线性相位 FIR，1/3 倍频程平滑且限制在 ±3 dB。运行时不再对校准 IR 做
逐方向总能量归一，否则会破坏已校准的直达/房间标尺。VBAP 仍把每个对象的虚拟
音箱增益向量归一到单位功率。布局只选择同一房间中的已校准音箱子集，不按布局中
所有潜在音箱数量施加 `sqrt(5/N)` 一类总增益。

这些归一化不保证经过头部/耳廓频响后的主观响度等于未空间化立体声。SDA 因此在全部
虚拟扬声器卷积、LFE 汇总和最终双耳 merger **之后**加固定 `+6 dB` makeup gain，
再依次经过用户 master 音量和可选节目 dialnorm；最终与立体声路径共用
stereo-linked lookahead limiter，再进入输出模式 master。

该值是 SDA 针对 AirPods/耳机主观响度的应用级标定点，**不是** Dolby、Apple 或
Genelec 发布的固定双耳增益，也不是 KU100 补偿/耳机 profile。它不改 HRIR/BRIR
资产、每方向能量归一化、LFE 带内增益或用户音量控制。

`+6 dB` 缩小了多总线同相峰值的余量，因此双耳与立体声路径在模式交叉淡化后共用
一个 ceiling `-1 dBFS`、约 `5 ms` lookahead 的 stereo-linked limiter。左右耳/声道
共享同一 gain envelope，lookahead attack 和约 `100 ms` release 都连续，避免峰值阈值
跨越造成单样本台阶。该节点没有 oversampling，**不是** true-peak limiter；极端
inter-sample peak 的保护不等同于专业 true-peak 限制器。

配合每源距离处理（`renderer.ts applyGains`）：
1. **归一化对象距离**：ADM 距离 1 是虚拟音箱环。环内维持 0 dB；环外按
   SDA 选用的 inverse 定律 `gain = 1 / normalizedAdmDistance`，不会由用户切换
   near/mid/far 而改变已制作对象的相对平衡。
2. **不从 ADM 半径推导空气吸收**：OAMD 的归一化位置没有可靠的物理米制含义，
   所以不自动低通对象。空气吸收只能在输入包含明确物理距离元数据时再启用，避免
   将正常的沉浸声对象渲染得发闷。

Apple 的通用空间音频 API 提供 inverse 距离衰减选项，但不能由此推断 Apple Music
会以同样方式解释 ADM/OAMD 归一化距离。

> 注意：制作端可给每个 3D object 和 surround-bed 子声道分别指定 Binaural Render
> Mode；Apple Logic Pro 明确写明 “Each object and surround bed channel can be set to a
> specific binaural render mode”，Steinberg Nuendo 也提供逐 bed subchannel 设置界面。
> 这些是 ADM/DBMD master 的静态渲染元数据，不是 E-AC-3 OAMD 的对象位置字段。
>
> Dolby 公开 DBMD supplemental parser 能读取 ordinal mode table，却不暴露 ordinal
> 到 bed instance/channel、ADM element ID 或 3D object ID 的身份映射。因此 SDA 只把
> DBMD 表显示为未绑定诊断，不按 JOC 动态对象声明顺序猜测绑定；床和对象都不会被
> 错套模式。MKV、MP4、raw EC-3/TrueHD 等发行输入若没有 DBMD/ADM 身份映射，则保留
> 应用默认 Near。本次 `KiLLKiSS.m4a` 全曲只含 EMDF 1（节目响度）、2（节目信息）、
> 11（OAMD）和 14（JOC），没有可恢复的 bed/object Binaural Render Mode 表。

参考：
- Apple Logic Pro《Dolby Atmos plug-in overview》：https://support.apple.com/guide/logicpro/dolby-atmos-plug-in-overview-lgcpad99a338/mac
- Apple Logic Pro《Binaural render modes》：https://support.apple.com/guide/logicpro/binaural-render-modes-lgcp789f000d/mac
- Apple Logic Pro《监听格式》：https://support.apple.com/zh-cn/guide/logicpro/lgcp179f27c1/mac
- Dolby《What is Binaural Render Mode and how do the settings affect my mix?》：https://professionalsupport.dolby.com/s/article/What-is-Binaural-Render-Mode-and-how-do-the-settings-affect-my-mix?language=en_US
- Dolby《Why should I mix with dynamic objects when I can use 7.1.4 or 9.1.6 tracks?》：https://professionalsupport.dolby.com/s/article/Why-should-I-mix-with-dynamic-objects-when-I-can-use-7-1-4-or-9-1-6-tracks?language=en_US
- Steinberg Nuendo《Binaural Render Mode for Beds Dialog》：https://archive.steinberg.help/nuendo/v11/en/cubase_nuendo/topics/surround_sound/surround_sound_adm_authoring_binaural_render_mode_for_beds_r.html
- Dolby DBMD parser public mirror：https://github.com/Harrie2019/dbmd-atmos-parser
- ETSI TS 103 420 V1.2.1 §5.5.9 / Annex C
- 《Dolby Atmos Binaural Settings Plug-in Guide》(professional.dolby.com, PDF)

## 3.5 低频与 LFE

Genelec 的 85 Hz bass management 是**物理多声道监听**中主箱配合 73 系列低音炮
的校准选项；它不应直接套用到耳机虚拟扬声器。把每个主声道 85 Hz 以下的内容抽出并
汇到等量双耳的单声道总线，会抹掉主声道在低频和低中频的 HRTF/BRIR 方向线索，使
整体变厚、变糊。

SDA 双耳路径因此采用以下规则：

1. **主声道全频空间化**：每个非 LFE 虚拟扬声器完整进入其方向的 HRIR/BRIR 卷积，
   不做主声道低频重定向。
2. **LFE 独立处理**：原始 LFE 走 LR4 低通 **120 Hz**、耳机路径带内 **unity**，再经仅作用于
   LFE 支路的 peak compressor 后等量直送双耳；它不参与方向卷积，也不接收主声道低频。
   该节点只约束会主导最终双耳 safety compressor 的 LFE 瞬态，避免低频驱动全频 gain
   reduction 而掩盖低中频和方向线索；它不是 true-peak limiter，也不替代 LFE 的规范增益。

这保留内容本来的方向信息，同时符合 LFE 的独立通道语义。若未来支持物理监听输出的
“small speaker + subwoofer”校准，应只在 `multichannel` 输出图中实现，不进入双耳
渲染。

参考：
- Genelec 8331A / 8341A / 8351B / 7370A 官网规格页（genelec.com）
- Genelec GLM 多声道监听设置指南（物理 bass management）
- ITU-R BS.775-3（LFE 20–120 Hz，+10 dB 带内增益）

## 3.6 耳机回放补偿（仅真实测量曲线）

KU100 的 HRIR/BRIR 已包含耳廓、头影与房间方向线索，不能用一个猜测的低/中/高 EQ
“修平人头麦”。耳机补偿只能放在所有虚拟音箱双耳化并汇成最终 L/R 信号**之后**，
校正用户耳机的回放响应，而不是改变每只虚拟音箱或 KU100 测量资产。

SDA 默认且当前唯一 profile 是 `无补偿`，即最终双耳 merger 到输出标定的 literal
bypass。此前的 `AirPods Pro 2（ANC，平均测量近似）` 已撤回：它没有独立左右耳原始
测量或针对 KU100 双耳 programme 的 balance 验证，尽管 FIR 文件可以写成相同，仍可能按
节目左右频谱差异改变主观声像中心，不能被作为可靠的耳机补偿发布。

未来 profile 必须同时包含：耳机型号/版本、公开或可审计来源、目标曲线说明、**独立的
left/right 原始测量来源**、夹具与 ANC/耳塞/固件/贴合状态、已验证的左右通道映射和平衡
证明、左右 FIR 与采样率。桌面版的本地档案另外要求左右 `.f32` 资产的 tap count 和 SHA-256，
并在导入与每次读取时重验；完整包格式和 AirPods Pro 2 测量门槛见
[`airpods-pro-2-local-calibration.md`](airpods-pro-2-local-calibration.md)。合格 profile 仍只位于最终
双耳 merger 后的独立 L/R FIR；它不改变每方向 HRIR/BRIR、对象/床层增益或 LFE 支路，
立体声和多声道输出也不会经过该层。耳机 EQ 不承担全局响度补偿、动态压缩或声像修复。

## 4. Apple 侧：移动端原生渲染可用 API（Expo 原生模块设计依据）

以下是应用可用的空间音频 API，不是 Apple Music/Apple Renderer 内部算法说明。
Apple 明确区分 Dolby Renderer 与用于 Apple Music/Apple TV 耳机监听的 Apple Renderer。

### AVAudioEnvironmentNode（iOS，最贴切）
- 隐式听者：`listenerPosition` / `listenerAngularOrientation` / `listenerVectorOrientation`
- **`isListenerHeadTrackingEnabled`（iOS 18+）**：一行接入 AirPods 头追
- 距离衰减：`AVAudioEnvironmentDistanceAttenuationParameters`
  （exponential/inverse/linear + rolloffFactor + referenceDistance + maximumDistance）
- `reverbParameters`（factory presets）→ 应用可选的房间感处理，不等同于 Dolby Far
- 每输入总线 `renderingAlgorithm`：
  `equalPowerPanning`（最便宜）/ `sphericalHead` / **`HRTF`/`HRTFHQ`（双耳）** / `soundField` / `auto`
- **关键限制：空间化只作用于 mono 输入** → 每个对象挂一路 mono
  AVAudioPlayerNode —— 恰好与"每对象 PCM + 元数据"模型一一对应

### PHASE（iOS 15+，备选/游戏向）
- `PHASESource`(3D 位置 + 体积形状) / `PHASEListener` / `PHASESoundEvent`
- `PHASESpatialPipeline` flags: directPathTransmission / earlyReflections /
  lateReverb —— 直达声/早期反射/晚期混响分层
- 距离模型 `PHASEGeometricSpreadingDistanceModelParameters`:
  **rolloffFactor 1.0 = 距离每加倍衰减 6 dB**（物理正确值，TS 实现同款默认）
- `PHASEEngine.outputSpatializationMode = alwaysUseBinaural`
- 个性化空间音频（扫耳建模）：系统自动应用，需 entitlement
  `com.apple.developer.spatial-audio.profile-access`；
  头追 entitlement `com.apple.developer.coremotion.head-pose`
- 自研渲染的游戏需 Info.plist `AVGameBypassSystemSpatialAudio = true` 防双重空间化

### 系统级（AVPlayer）
iOS 15+ 系统对 Atmos 内容自动做头追双耳；`AVAudioSession.SpatialExperience`
（iOS 18.1+）声明 bypassed/fixed/headTracked。SDA 自研渲染时设为 bypassed。

### Android
无系统级对象渲染 API → 走自研管线：Rust 侧实现 VBAP + 分区卷积
（与 TS 渲染器同算法，可共享 HRTF 数据），经 AAudio 低延迟输出。
可选头追：Android HeadTracker API（`android.hardware.SensorManager` 头追或
`android.media.audiofx`？v2 再定）。

## 5. HRTF 数据集：SADIE II KU100（已落地）

**格式标准：SOFA（AES69）**，netCDF-4 容器；但 SADIE II 同时发布 **WAV 版本**，
SDA 直接用 WAV（Node 脚本零依赖解析，不需要 Python/netCDF）。

| 数据集 | 内容 | 许可 |
|---|---|---|
| **SADIE II D1 (York)** | **Neumann KU100 假头**：HRIR（消声室）+ BRIR（房间）+ 耳机 EQ | **Apache-2.0** → **首选，干湿两全** |
| MIT KEMAR | 710 位置，44.1kHz，仅 HRIR | "free with no restrictions on use"（要求引用）→ 备选（只有干声） |
| CIPIC | 45 真人，1.27 m | 免费 license agreement，条款需核实 |
| LISTEN (IRCAM) | ~50 被试 | Chromium HRTF 的源头；许可需核实 |
| TH Köln / HUTUBS | 96 头高分辨率 | CC 系；BEAR 的 HRIR_FULL2DEG 即此 |
| BBC R&D (BEAR 默认) | 录音室 KU100 BRIR | 见 ebu/bear 仓库 |

**选 SADIE II KU100 的理由**：① Apache-2.0 可随应用自由再发布；② 同一假头
同时有 HRIR 和 BRIR —— SDA near/mid/far 的房间尾声混合方案（§3）需要
成对的干湿数据；③ KU100 与 SDA 3D 视图里的假头模型一致。

**转换与校准管线**（`scripts/build-hrtf.mjs`、`scripts/build-calibrated-hrtf.mjs`）：
校验官方 `D1.zip` SHA-256 → 从 WAV 文件名解析方向 → 记录 17 个目标音箱与最近
BRIR 实测点的 provenance → 生成原始基线 → 对双耳共同 TOF、500–2000 Hz 直达
参考电平和低分辨率 BRIR/HRIR 房间传递比做逐音箱校准 → 只写 staging → 资产契约
和 5/7/9 Group 验收通过后发布。运行时文件为 f32le `[leftIR][rightIR]` 拼接；
calibration v1 manifest 明确禁用峰值归一和运行时逐 IR 总能量归一。

> ⚠️ 方位角约定：SDA 为 + = 左（ADM/ITU），多数数据集为 + = 右，
> 脚本默认 `--flip-az` 取反匹配（选对称侧的测量点，声道不互换）。

- SOFA 规范: https://www.sofaconventions.org/
- 数据集索引: https://www.sofacoustics.org/data/database/
- SADIE II: https://doi.org/10.5281/zenodo.12092466 （D1.zip = KU100 全部 WAV，
  `azi_<方位角,小数>_ele_<仰角,小数>.wav` 逗号小数命名，方位角 + = 左与 ADM 一致；
  引用：DOI 10.3390/app8112029）
- MIT KEMAR: https://sound.media.mit.edu/resources/KEMAR.html （备选，仅干声 HRIR）
- BEAR: https://github.com/ebu/bear （IR 处理: doc/ir_processing.md）

## 6. Web Audio PannerNode HRTF 能力与局限（降级方案依据）

- `panningModel: "HRTF"` 输出恒为立体声，内置通用 KEMAR 类 HRTF
- 距离仅改增益（`distanceModel` + rolloffFactor），**不改 HRTF 本身**
  （无近/远场音色、无混响）→ 需要自有距离层（§3）补偿
- 位置是 AudioParam 可平滑 —— 对元数据流友好
- 无个性化 HRTF、无内建头追（需自更新 `AudioListener` 朝向）
- 规范: https://webaudio.github.io/web-audio-api/#dom-pannernode-panningmodel

## 7. 坐标系约定（实现期最大的坑）

| 系统 | 约定 |
|---|---|
| ADM / OAMD 事件流 | 笛卡尔 [x,y,z]：x+ = 右，y+ = 前，z+ = 上（单位立方体） |
| ADM 极坐标（VBAP 内部） | 方位角 0°=前，+ = 左；仰角 + = 上 |
| Web Audio / three.js | 右手系：x+ = 右，y+ = 上，z+ = 朝听者（听者面朝 -z） |

转换（`packages/renderer/src/coords.ts`）：
- ADM → WebAudio: `(x, y, z) → (-x, z, -y)`
- 事件流里的距离 = √(x²+y²+z²)，1 = 扬声器环半径

> ⚠️ 待验证：ADM 笛卡尔 X 轴方向（+X = 左还是右）在不同资料中表述不一，
> 以上取 harletty-bridge 注释与 ITU-R BS.2076 通行解释（+X = 左）。
> 用已知内容（如 Atmos 演示片"从左到右飞的直升机"）做视觉校验，
> 若反了只需翻转 `coords.ts` 一处。

## 8. 待核实清单（调研遗留）

1. Dolby near/mid/far 的精确距离和内部滤波实现未由已核对的公开资料确定，不使用猜测值。
2. CIPIC / SADIE II / TH Köln 许可条文细节
3. `AVAudioSession.SpatialExperience` 枚举确切 case 名
4. ITU-R BS.2051 扬声器角度表原文（付费标准，本文用行业通行值）

2026-09-05 的对象起伏排查、官方资料边界和可听对照见
[`egaku-object-envelope-investigation.md`](egaku-object-envelope-investigation.md)。
