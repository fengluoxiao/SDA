# iOS 个人化 HRTF：公开接口核验与接入边界

核验日期：2026-11-19。本文是可行性与接入设计，**不是已上线的扫耳功能，也不是已验证个人化生效的声明**。

## 当前结论

Face ID 的 LocalAuthentication 只返回认证结果，应用拿不到认证中的人脸／深度模型。TrueDepth 相机可以由 AVCapture 独立申请相机权限并采集同步图像和深度；这与 Face ID 认证是不同接口。ARKit 的 ARFaceGeometry 是脸部网格，公开文档没有承诺它提供能用于声学求解的完整头部和耳廓几何。ARFaceTrackingConfiguration 在新设备上的支持也不等同于存在 TrueDepth 深度相机，必须单独检查设备与深度格式。

Apple 的个人化空间音频设置确实分别采集正面、左耳与右耳。iOS/iPadOS 18、macOS 15 起，`com.apple.developer.spatial-audio.profile-access` entitlement 可将系统设置中的个人化档案应用于 AVAudioEngine、AUSpatialMixer、PHASE 输出。需要真实签名与匹配的 provisioning profile；unsigned IPA／模拟器不能证明这项权限和个人档案有效。

这提供了**由系统使用档案**的公开路线。所核验接口没有提供将该档案的 HRTF 系数直接导出给 SDA Rust 卷积器的能力。当前 SDA 已经在原生渲染器生成双声道；仅增加 entitlement，或在这段双声道上再叠一次空间化，不能当作逐对象个人化已完成。

## 路线 A：由 Apple 引擎使用系统个人档案

输入应保留独立单声道源 PCM 与对象位置，交给 AVAudioEnvironmentNode／PHASE 完成空间化，而不是提交已经双耳化的立体声。AVAudioEnvironmentNode 只对单声道连接的源实施空间化，不能把多声道总线直接当任意对象场景输入。

现有 `Phase360Prototype.swift` 是隔离原型。推进为正式路由前，还需解决实际播放时钟下的对象位置调度、seek 与旧队列丢弃、LFE／底部扬声器、混合格式、输出恢复，以及与 SDA 空间平衡、整体音量和头部追踪的边界。保留 SDA Spatial 作为产品名称不能消除更换底层空间引擎带来的声音变化。

设备验收必须对比同一路由在系统档案启用／停用时的实际耳边输出或定位表现，记录签名权限、设备、耳机与用户设置。`isSpatialAudioEnabled`、命令成功、编译成功都不能单独证明个人档案已生效。

## 路线 B：保留 SDA 渲染器，独立生成个人 HRTF

可研究的完整链路是：前脸与两耳多视角 RGB／深度采集 → 有尺度的完整头耳三维重建 → 耳廓网格质量检查 → 声学求解或经过验证的估计模型 → SOFA 个人 HRTF → SDA 原生渲染与定位试听。

Mesh2HRTF 是已经存在的开源声学工具箱，其 Mesh2Input／NumCalc／Output2HRTFs 能从三维模型与声学设置计算 HRTF，输出 SOFA；它不是手机扫脸即出结果的现成 SDK。官方说明要求合格网格，并涉及内存与计算成本。仓库标注 EUPL-1.2，实际分发和集成时需按所用版本核对许可证。

本次没有证明 TrueDepth 耳部扫描的精度足够，没有引入估计模型，也没有跑过用户头耳网格的声学求解。不能把头宽简单缩放、几档高频 EQ 或通用 HRTF 换名成“扫耳个人化”。现有四项试听参数仍是用户偏好。

实施验收顺序：

1. 独立采集原型先验证两耳细节、尺度、遮挡与重复扫描一致性；不合格时给出重新采集结果，不能用补洞模型假装测量成功。
2. 用已知网格／参考 HRTF 验证求解链路、方向约定、左右耳、采样率、ITD 与频谱；随后才评估手机采集数据。
3. 在相同耳机、响度匹配下，盲测正前／正后、头顶／下方及斜后方的混淆率，比较通用与个人档案。未改善的候选不能自动替换。
4. 个人 HRTF 仅是小型校准数据，播放时使用现有原生解码与卷积，不缓存整首歌曲。原始图像与网格需有单独的保存／删除策略；上传计算必须向用户说明。

耳机频响与佩戴差异是另外一项校准；头耳照片不能测出任意耳机在用户耳边的实际响应。

## 对现有 HRTF 测量工具的限制

`scripts/apple-spatial-reference.swift` 使用 AVAudioEnvironmentNode 的 offline manual rendering 测量通用系统响应。Apple entitlement 文档并未保证个人档案可通过 offline rendering 导出。未经有效签名真机的档案开关对照，不能把该工具的输出称为个人 HRTF，也不能把该推测当正式方案。

## 官方与上游资料

- [Apple：Personalized Spatial Audio 设置，包含正面与双耳采集](https://support.apple.com/en-us/102596)
- [Apple：Spatial Audio Profile entitlement，列出适用 API 与系统版本](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.spatial-audio.profile-access)
- [Apple：LocalAuthentication，应用不能访问底层认证数据](https://developer.apple.com/documentation/localauthentication)
- [Apple：TrueDepth capture device，同步深度与图像](https://developer.apple.com/documentation/avfoundation/avcapturedevice/devicetype-swift.struct/builtintruedepthcamera)
- [Apple：ARFaceGeometry 的脸部网格范围](https://developer.apple.com/documentation/arkit/arfacegeometry)
- [Apple：ARFaceTrackingConfiguration 的硬件与隐私说明](https://developer.apple.com/documentation/arkit/arfacetrackingconfiguration)
- [Apple：AVAudioEnvironmentNode 的输入与空间化限制](https://developer.apple.com/documentation/avfaudio/avaudioenvironmentnode)
- [Mesh2HRTF：三维模型到 SOFA 的公开工具链与参考文献](https://github.com/Any2HRTF/Mesh2HRTF)
