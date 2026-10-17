# iOS 360RA逐对象 Apple 渲染可行性研究

研究日期：2026-10-02。范围：公开 Apple API + 当前 SDA 源码核对。**研究结论，不是已经完成的功能，也不是实机听感验证。** 本次不修改播放器、不新增签名权限、不替换已通过 CI 的 7.1.4 路由。

## 结论

有公开技术路线：MPEG-H 源 PCM/OAM → PHASE 对象/声道渲染 → Apple 输出。PHASE 接受应用提供的 PCM 和场景位置；它不是任意 Atmos 对象元数据的注入入口。无需伪造 Dolby 标签，也不会变成 Dolby Atmos 内容。不能据此承诺 Apple Music 相同听感、相同混音策略、相同控制中心标识。

相比先渲染到 7.1.4，这条路线可让移动对象在进入 Apple 空间引擎之前保留为独立源。收益是可行性推断，必须做原型与真机 A/B。

## 已确认公开 API

|能力|接口|iOS 可用版本/边界|
|---|---|---|
|逐源 PCM 推送|PHASEPushStreamNodeDefinition / PHASEPushStreamNode.scheduleBuffer|15 起，兼容 SDA 的最低 iOS 16；提供 AVAudioPCMBuffer 与可选 AVAudioTime|
|主动拉取 PCM|PHASEPullStreamNodeDefinition / PHASEPullStreamNode.renderHandler|18 起；回调是高优先级实时线程，禁止锁、分配、解码、JSON、文件读取|
|空间源/监听者|PHASESource / PHASEListener / PHASESpatialMixerDefinition|15 起；transform 指定位置与朝向|
|监听者自动头部跟踪|PHASEListener.automaticHeadTrackingFlags / .orientation|18 起；支持的硬件及 Head Pose capability 是前提|
|个性化空间音频档案|com.apple.developer.spatial-audio.profile-access|Apple entitlement 文档明确包含 PHASE；需要对应 capability 与有效签名/provisioning，不能靠 unsigned IPA 验证|
|PHASE 最近渲染时刻|PHASEEngine.lastRenderTime|**26 起**，不是 iOS 16/18 都能无条件使用|
|纯直达管线|PHASESpatialPipeline(flags: [.directPathTransmission])|15 起；earlyReflections、lateReverb 为可选层，不应在音乐原型中开启|
|声道床|PHASEAmbientMixerDefinition(channelLayout:orientation:)|公开声道布局入口；任意原始 360RA 布局/LFE 行为须另外验证，不等同于已验证支持 11.2|

Apple head-pose entitlement 文档明确把 PHASE automaticHeadTrackingFlags 列为可使用兼容 AirPods 头姿的接口。因此“PHASE 完全不能接入系统头部跟踪”不成立。另一方面，PHASE 路径不等于 AVSampleBufferAudioRenderer 的媒体空间化路径，不能从接口存在推断控制中心模式完全相同。

不能保证任意重新签名工具保留或获得这些权限：必须检查最终签名 entitlements 与 provisioning profile 是否匹配。未签名 CI 包仅能用于构建和模拟器逻辑检查。此研究没有确认用户是否具备相应签名条件。

## SDA 源码核对

- `scripts/prepare-mpegh.mjs` 已在默认 group selection/gain 之后、对象渲染之前抓取源 PCM；同时抓取 OAM offset、azimuth、elevation、radius、gain、spread、diffuseness、screen-relative 与 duration。
- `packages/core/mpegh/bridge.c` 提供分平面 PCM 和每帧多条 OAM；桥接不是只有最终 7.1.4 PCM。
- `crates/sda-native/src/mpegh.rs` 普通源路径已产生对象 channel declaration、ObjectEvent.sample_pos、gain_db 与 ramp_duration；`new_7_1_4` 是另一条上游扬声器渲染路径。
- 当前纯源路径对对象/HOA 混合明确报错；不要声称任意 MPEG-H 内容都能直接变成 PHASE 对象。
- 现有 iOS C ABI 的 speaker host 只导出扬声器 PCM；逐对象原型需要**新增独立源 ABI**，不能调用 speaker read 后把 12 个结果声道伪称原始对象。
- 当前 app.json 未设置上述 capability；podspec 未链接 PHASE。这里只记录需求，没有在研究阶段擅自修改。

## 正确映射与主要风险

### 1. 坐标不能直接复制

SDA OAM 当前使用 x 向右、y 向前、z 向上；源代码位置为 `[-sin(az)*cos(el), cos(az)*cos(el), sin(el)]`。
Apple PHASE 为右手系，Y 向上，-Z 向前。基于两套约定，方向映射应为 `(x,z,-y)`，并用前/后/左/右/上固定信号测试确认。不要额外重复反转左右。

当前 adapter 明确不把 OAM relative radius 当成米（distance_m=None）。原型先采用固定半径和不引入距离衰减的音量策略；有意支持距离之前需完成规范和测试核对。

### 2. 时间同步是最大工程风险

OAM 在帧内有多个 sample_pos 与 ramp_duration；不能每次读到一帧就立即改 transform：提前排队的 PCM 还没播出，对象会提前移动。

PHASE 有 PCM 时间调度接口，但当前查到的 transform setter 不提供同等的 sample-timestamp 调度保证。工程上要以**实际播放时钟**消费 OAM、插值姿态，并量化 update 粒度/输出延迟。不能宣称这种方式天然达到逐采样对象位置同步。

先在 iOS 26 模拟器原型验证 lastRenderTime、同一事件多节点起播、暂停/欠载/恢复，再选择 iOS 18–25 的时钟方案。所有音频节点必须共享同一个时间基准、帧序号和暂停状态；不能每对象独立异步起播。push path 的公共时间参数及 pull path 的实际回调时间需通过 SDK 编译/运行核对后选型。

### 3. 增益只应用一次

源抓取点已处理 group gain；OAM object gain 是单独的元数据。必须用固定增益和阶跃增益夹具核对各级处理，再确定 application point。不要额外开逐源 loudness normalization：PHASEPushStreamNodeDefinition.normalize 默认 false，原型保持 false；必要的源标定应固定且可测，不应抹平对象之间的混音电平。

不要同时用 PCM 乘增益和 PHASE 节点 gain 重复应用同一个 OAM gain。room、near-field、KU100、SDA loudness balance 必须绕过。

### 4. 不重新制造“糊”的环境声

只启用 directPathTransmission；不加入 earlyReflections、lateReverb、遮挡、几何传播滤波或默认距离衰减模型。并检测 Apple 后续媒体空间化是否又处理已双耳化输出，不能通过 KU100 再渲染一次。

个性化档案与头部跟踪是分开的权限/行为；分别测，不用“系统空间音频已开”一个布尔值同时表示全部成功。

### 5. bed、LFE、HOA 和扩展 OAM

- 原始 bed 按真实布局处理；优先实验 ambient mixer，不能凭声道编号猜方向。
- LFE 不适合当作前方全频点声源；需单独核对低频与电平管理，避免截掉或重复叠加。
- 未验证的布局或对象/HOA 混合先回退上游 MPEG-H → 7.1.4，不冒充全保真支持。
- spread/diffuseness/screen anchoring 和 PHASE 的几何/directivity 不是直接等价参数；第一阶段明确标注未完全映射，不能称为完整 MPEG-H 场景保真。

## 推荐落地步骤与验收

1. **隔离原型，不替换现有路线。** 新增源 PCM + OAM ABI、单一 PHASEEngine、多源节点、纯直达输出。先使用项目生成的 motion.mhas 双移动对象夹具，无需用户版权音频。
2. **CI 最小测试。** iOS 16 deployment availability 检查；iOS 26 SDK 真正编译 PHASE；固定前后左右、两对象相反运动、gain 阶跃、EOF、欠载、pause/resume、输出切换。报告 decoded/consumed 样本、对象数、metadata 时刻、时钟漂移，不只报告 App 启动。
3. **布局与不支持输入测试。** bed + objects、LFE、不同 OAM subframes、未知布局及 HOA 明确回退。来源转换需在路由打开前决定，且不同时抢占 singleton MPEG-H decoder。
4. **签名/真机验收。** 有效签名的 iPhone + 兼容 AirPods 上确认个性化档案和头部跟踪分别起效；对比现有 7.1.4、PHASE、Apple Music，响度匹配，不用手机录屏当作耳边最终头部跟踪输出。
5. **再决定是否公开实验入口。** 保留现有 iOS-only 360RA 7.1.4 开关；PHASE 实验入口是另一条明确标识的路由，不悄悄改变它的语义。默认关闭、切换仅影响下一次播放。iOS 16–17 保留现有路线，不能承诺新的自动头跟踪能力。

研究建议：先做上述原型，**不先接入真实 Atmos 编码器，也不伪造 Dolby 标识**。如果签名条件暂时不足，仍可验证编译、逐源流与时钟；个性化/头跟踪与听感必须保持“未验证”。

## 官方依据（2026-10-02 抓取）

Apple 页面动态正文经官方 DocC JSON 核对。原始记录存于 `E:/SDA/tools/phase-research/`，不属于运行时资产。部分普通网页抓取为空，不能从空页面推断结论。

- [PHASEPushStreamNode](https://developer.apple.com/documentation/phase/phasepushstreamnode)
- [PCM scheduleBuffer](https://developer.apple.com/documentation/phase/phasepushstreamnode/schedulebuffer(buffer:time:options:completioncallbacktype:completionhandler:))
- [PHASEPullStreamNode.renderHandler](https://developer.apple.com/documentation/phase/phasepullstreamnode/renderhandler)
- [automaticHeadTrackingFlags](https://developer.apple.com/documentation/phase/phaselistener/automaticheadtrackingflags)
- [PHASEAutomaticHeadTrackingFlags](https://developer.apple.com/documentation/phase/phaseautomaticheadtrackingflags)
- [Head Pose entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.coremotion.head-pose)
- [Spatial Audio Profile entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.spatial-audio.profile-access)
- [PHASEObject.transform](https://developer.apple.com/documentation/phase/phaseobject/transform)
- [PHASESpatialPipeline](https://developer.apple.com/documentation/phase/phasespatialpipeline)
- [PHASEAmbientMixerDefinition](https://developer.apple.com/documentation/phase/phaseambientmixerdefinition)
- [normalize](https://developer.apple.com/documentation/phase/phasepushstreamnodedefinition/normalize)
- [PHASEEngine.lastRenderTime](https://developer.apple.com/documentation/phase/phaseengine/lastrendertime)
- [PHASESpatializationMode](https://developer.apple.com/documentation/phase/phasespatializationmode)

此记录没有 SDK 编译 PHASE 原型，没有新增 CI 运行，没有签名真机验证，也没有宣布 iOS 27 验证通过。


## 隔离原型实现（2026-10-02，验证中）

用户授权尝试后，新增 `Phase360Prototype.swift` 和独立 source-frame C ABI。当前只在明确设置 SDA_IOS_SMOKE=1 的 CI 启动流程中运行，不修改正常播放路由或现有开关。原型是 iOS 26+、纯对象、最多 8 秒/2 MiB 的预载测试，不是全曲生产流播放器。

- 保留真实 PCM、对象 id/channel、OAM samplePos/rampDuration；非实时串行 JSON 交接。
- 同一个 PHASE sound event 中的独立单声道节点统一定时起播。
- 纯直达空间管线、无距离模型、normalize=false；OAM 增益在 PCM 中应用一次。
- render-host clock 驱动控制线程位置更新，输出 CSV 轨迹和更新间隙；不声称逐采样位置同步或已测声学延迟。
- 验证 PHASE dataRendered 回调、engine pause 状态及恢复；不把回调当作耳边听感证明。
- 头部跟踪请求关闭，不添加需要正式签名的能力；个性化档案/真机听感仍未验证。
- bed/LFE/HOA、扩散/尺寸/anchor、欠载、长曲实时流与应用内开关仍未实现，不能称为完整 360RA 播放路线。

本地 ios::tests：5 passed（包括新 source ABI、既有 speaker ABI 与 KU100 host）；Apple SDK 编译与模拟器验证等待 GitHub Actions 结果。
