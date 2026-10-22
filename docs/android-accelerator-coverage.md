# Android 协处理器覆盖与准入边界

目标是按实际可调用的运行时、运算支持和数值/延时验证选择后端，而不是按照品牌或型号打白名单。可见的硬件、可调用的驱动、实际运行的后端是三个不同状态。

| 路线 | 适用范围与约束 | 当前验证状态 |
| --- | --- | --- |
| CPU / NEON | 当前完整浮点 HRTF、动态滤波器、原有精度与最终保护链 | 已用于播放；现有基准与 PCM 回归通过 |
| 公共 NNAPI 设备接口 | 可枚举已注册的高通、联发科及其他厂商 CPU/GPU/专用加速器；没有按厂商过滤 | 已接入 HRTF FP32 复数频域乘加；明确选择单个 ACCELERATOR，整图运算支持检查与实际音频数值/耗时准入；尚无 NPU 真机执行证据 |
| LiteRT NPU / CompiledModel | 官方提供高通 AI Engine Direct、MediaTek NeuroPilot 等集成路线；须满足对应 SoC、系统、运行库和模型约束 | 尚未接入当前音频渲染，不能声明已启用 |
| 高通 Hexagon SDK / FastRPC | 骁龙 660 官方配置含 Hexagon 680 DSP/HVX；自定义 DSP 内核还依赖 ROM、DSP 运行环境及部署条件 | 未做目标手机的自定义浮点音频内核实测 |
| 旧版 TensorFlow Lite Hexagon delegate | 官方历史支持列表含 660，但接口面向 8-bit 量化模型 | 不把它直接作为当前浮点 FFT/HRTF 的兼容实现 |
| MediaTek Neuron Adapter | 官方文档提供第三方 Android 应用入口；部分 Neuron Runtime API 仅供 vendor 分区应用使用 | 尚未完成对应天玑设备运行库及音频运算验证 |

公共设备类型遵守 NDK NeuralNetworksTypes.h：CPU=2，GPU=3，专用机器学习加速器=4。GPU 不计为 NPU；参考 CPU 驱动也不能计为专用加速。枚举在诊断线程之外异步执行一次，不在音频回调或渲染块热路径运行，不做没有音频作用的虚假加速调用。

真正启用音频协作前必须满足：

1. 普通应用能够使用目标公开运行时，不能只检查 vendor 文件存在。
2. 对当前浮点运算及动态滤波器形状取得明确支持，执行日志确认选择的硬件；参考 CPU 回退不能算 NPU 已启用。公开 NNAPI 可以证明提交给哪个设备并禁止框架 CPU 回退，但不能观测厂商驱动内部实现。
3. 同输入 PCM 精度、左右关系、空间运动、延迟、音量和真峰值保护验证合格。
4. 总时间包括准备、搬运、执行和同步，持续吞吐与尾延时均合格，且不会增加首次打开文件的阻塞等待。
5. 运行时不满足条件或设备没有开放入口时，明确标注未启用；不得为了显示 NPU 开关而量化音频、降低 HRTF 精度或运行无关神经网络。

2026-10-22 实现范围：`spectral_accelerator` / `nnapi_spectral` 把连续对象 HRTF 卷积的频域复数乘加交给公开 NNAPI 加速器。一次打包同一块的所有对象及滤波器过渡；CPU 仍负责 FFT/IFFT、对象控制及原有真峰值保护。不是无关模型演示，也不把设备枚举作为启用依据。

- 整图 FP32；`relaxComputationFloat32toFloat16(false)`；不用 INT8/FP16 替换原始音频。分区按原始次序相加。
- 仅选择 NNAPI 类型 `ACCELERATOR=4` 且 feature level >=3 的设备，逐运算检查支持并使用 `createForDevices` 限定一个设备。CPU、GPU、UNKNOWN 和 OTHER 均不冒充 NPU。
- 第一次在后台编译，播放继续走 CPU。先用 2 个实际音频块预热驱动并核对精度，再用 8 个实际块核对精度与性能；预热/核对期播放始终使用同块 CPU 结果。以实际音频块验证：RMS 误差 <= 参考信号的 -120 dB，单个频点误差 <= 参考峰值的 4e-6。打包、提交、执行、同步、取回的时间必须有性能收益，才能采用加速结果。
- 校验期间仍用同块 CPU 频谱生成声音。启用后请求等待时间最多 1 ms 且不超过实测 CPU 计算预算；线程恢复调度后再检查实际时钟，晚于预算的结果一律丢弃；报错/超时即用已保留历史计算当前块，拒绝迟到结果。不会重走音量包络或改变歌曲增益。
- 内存中只保留一个有界批次及一个编译形状；应用自有输入/输出/参考缓冲总量最多 32 MiB；每批最多 256 行（128 个对象各含当前/过渡两行）、4 个卷积分区及 4097 个独立频点，超出时走 CPU。不扫描整首、不保存 PCM 缓存、不为每首歌积累模型文件。驱动内部内存由系统管理。
- 无品牌/型号白名单，同一公开接口覆盖提供此能力的高通、天玑及其他设备。**没有公开驱动、驱动只支持量化模型、精度不合格或传输后更慢时，不启用 NPU。此实现不包含 LiteRT/QNN/NeuroPilot 专属适配器，不能宣称所有具有 NPU 的手机都已覆盖。** 骁龙 660 的 Hexagon/HVX 也不能仅凭硬件名称推断已支持。

诊断 `accelerators.audio` 报告状态/失败原因、明确设备名、图形状、支持的整图运算数、实际执行音频批次和实际用于 PCM 的批次。`npuAudioEnabled` 只在合格结果实际被采用后为 true。`audioBatchesExecuted>0` 但 `audioBatchesApplied=0` 说明实际运算曾委派但未获准用于播放，不可作为加速成功宣传。模拟器只能验证运行库缺失时的 CPU 回退；Qualcomm / MediaTek 的真实执行和端到端收益仍需相应真机诊断证据。

官方参考：

- https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/snapdragon_660_mobile_platform_product_brief.pdf
- https://ai.google.dev/edge/litert/next/npu
- https://github.com/tensorflow/tensorflow/blob/master/tensorflow/lite/g3doc/android/delegates/hexagon.md
- https://developer.mediatek.com/ai/6423c916c612745b3a4baa2d.html
- https://developer.mediatek.com/ai/64254ccbf55b040d6989a99a.html
- https://mediatek.gitlab.io/aiot/doc/aiot-dev-guide/master/sw/yocto/ml-guide/neuron-development.html（此分区限制说明来自 AIoT 平台指南，不能直接视为所有天玑手机的权限结论）
- https://developer.android.com/ndk/guides/neuralnetworks
- https://docs.qualcomm.com/bundle/publicresource/topics/80-70015-265/overview.html

## NNAPI 库生命周期回归（2026-10-22）

Mi Note 3 真机两次崩溃均位于 `sda-npu-audio` 退出时的 `pthread_key_clean_all`，调用目标已被卸载。该系统的 NNAPI 只公开一个 CPU 设备，后端发现无专用加速器后释放 `dlopen` 句柄；库依赖注册的 pthread TLS 清理回调仍会在线程退出时执行，导致 SIGSEGV。

设备枚举和实际执行现在共用 `nnapi_runtime::ProcessLibrary`，使用 Android 公共 `RTLD_NODELETE` 固定运行库及依赖到进程结束。句柄正常释放，模型/执行对象仍正常释放，不为每首歌累计缓存。不能把运行库卸载早于它注册的线程清理回调。

USB 独立进程回归在同一台设备上复现旧策略退出码 139；使用生产加载器的修复策略连续 12 轮加载、枚举、释放句柄和线程退出全部通过。测试不输出音频。此结果验证崩溃修复，不代表设备已获得 NPU 驱动或音频加速。
