# Android 协处理器覆盖与准入边界

目标是按实际可调用的运行时、运算支持和数值/延时验证选择后端，而不是按照品牌或型号打白名单。可见的硬件、可调用的驱动、实际运行的后端是三个不同状态。

| 路线 | 适用范围与约束 | 当前验证状态 |
| --- | --- | --- |
| CPU / NEON | 当前完整浮点 HRTF、动态滤波器、原有精度与最终保护链 | 已用于播放；现有基准与 PCM 回归通过 |
| 公共 NNAPI 设备接口 | 可枚举已注册的高通、联发科及其他厂商 CPU/GPU/专用加速器；没有按厂商过滤 | 本项目增加只读异步枚举；只表示设备可见，不表示音频已在 NPU 执行 |
| LiteRT NPU / CompiledModel | 官方提供高通 AI Engine Direct、MediaTek NeuroPilot 等集成路线；须满足对应 SoC、系统、运行库和模型约束 | 尚未接入当前音频渲染，不能声明已启用 |
| 高通 Hexagon SDK / FastRPC | 骁龙 660 官方配置含 Hexagon 680 DSP/HVX；自定义 DSP 内核还依赖 ROM、DSP 运行环境及部署条件 | 未做目标手机的自定义浮点音频内核实测 |
| 旧版 TensorFlow Lite Hexagon delegate | 官方历史支持列表含 660，但接口面向 8-bit 量化模型 | 不把它直接作为当前浮点 FFT/HRTF 的兼容实现 |
| MediaTek Neuron Adapter | 官方文档提供第三方 Android 应用入口；部分 Neuron Runtime API 仅供 vendor 分区应用使用 | 尚未完成对应天玑设备运行库及音频运算验证 |

公共设备类型遵守 NDK NeuralNetworksTypes.h：CPU=2，GPU=3，专用机器学习加速器=4。GPU 不计为 NPU；参考 CPU 驱动也不能计为专用加速。枚举在诊断线程之外异步执行一次，不在音频回调或渲染块热路径运行，不做没有音频作用的虚假加速调用。

真正启用音频协作前必须满足：

1. 普通应用能够使用目标公开运行时，不能只检查 vendor 文件存在。
2. 对当前浮点运算及动态滤波器形状取得明确支持，执行日志确认选择的硬件；参考 CPU 回退不能算 NPU 已启用。
3. 同输入 PCM 精度、左右关系、空间运动、延迟、音量和真峰值保护验证合格。
4. 总时间包括准备、搬运、执行和同步，持续吞吐与尾延时均合格，且不会增加首次打开文件的阻塞等待。
5. 运行时不满足条件或设备没有开放入口时，明确标注未启用；不得为了显示 NPU 开关而量化音频、降低 HRTF 精度或运行无关神经网络。

本文件是已核对的覆盖设计及边界，**不是 NPU 音频后端已经实现或所有机型已经验证的声明**。当前诊断返回 `audioBackend: cpu-native`、`npuAudioEnabled: false`；后续必须在实际音频执行器接入并通过验证后才能改变该状态。

官方参考：

- https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/snapdragon_660_mobile_platform_product_brief.pdf
- https://ai.google.dev/edge/litert/next/npu
- https://github.com/tensorflow/tensorflow/blob/master/tensorflow/lite/g3doc/android/delegates/hexagon.md
- https://developer.mediatek.com/ai/6423c916c612745b3a4baa2d.html
- https://developer.mediatek.com/ai/64254ccbf55b040d6989a99a.html
- https://mediatek.gitlab.io/aiot/doc/aiot-dev-guide/master/sw/yocto/ml-guide/neuron-development.html（此分区限制说明来自 AIoT 平台指南，不能直接视为所有天玑手机的权限结论）
- https://developer.android.com/ndk/guides/neuralnetworks
- https://docs.qualcomm.com/bundle/publicresource/topics/80-70015-265/overview.html
