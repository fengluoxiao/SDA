# iOS Core ML 音频计算后端

最低部署保持 iOS 16.4，iPhone 8 继续使用原生 CPU/Accelerate 路径。不能根据 A11 存在 Neural Engine 就宣称第三方 App 获得 ANE 加速。

## 实际链路

`directional::finish_sources` 将当前块及滤波器过渡的 HRTF 频谱打包，交由共用的有界计算执行器。在独立 utility 线程中使用公开 Core ML `cpuAndNeuralEngine` 计算复数乘加；FFT/IFFT、空间控制、前景、低频、音量及真峰值保护保留在原渲染器。原有 iOS 异步头部滤波器准备策略保持。

模型用原创 Rust 代码依照 Apple coremltools 9.0 公开 protobuf 格式生成，仅包含加、减、乘拓扑及 FP32 输入/输出类型，没有歌曲、HRIR、个人数据或训练权重。每个分区按原来的次序累加。使用输入直连数组和可复用输出背板；输出形状、类型、步长和长度均检查。公开接口异常转换成失败，不跨越 C ABI 抛出异常。

## 准入与回退

- 使用现有真实音频块的 2 次预热和 8 次核对。核对期间播放始终使用同块 CPU 结果。
- 保留相对 RMS -120 dB、单频点相对峰值 4e-6 的精度限制，以及计入打包/同步/清空/线程执行成本的整块耗时检查，不为适应内部 FP16 降低标准。
- 请求等待最多 1 ms，并检查实际完成时钟。失败/晚到时使用已保留历史计算当前块，不复用上一块或下一块的结果。
- 不等待整首预扫描；不保存 PCM 缓存。只保留一个模型形状，模型目录为应用缓存中的 `sda-coreml-spectral-v1`；新形状和关闭清理旧目录。Core ML 框架自身内存由系统管理。
- GPU 始终排除，避免后台 GPU 命令限制。系统空间音频路径仍由原有系统实现处理。

## 诊断语义

`accelerators` 被纳入 iOS 远程诊断：

- `coremlAudioEnabled`：当前有经精度和整块耗时核对的 Core ML 结果实际用于渲染。
- `audioBatchesSubmitted/Executed/Applied`：真实音频图提交、成功预测和采用的批次数，不能当成 ANE 硬件运行次数。
- `backendDetails.neuralEngineAvailable`：iOS 17+ 公开设备列表，旧系统为 unknown/null。存在不等于被使用。
- `requestedComputeUnits=cpuAndNeuralEngine`：允许 Core ML 在 CPU 和 ANE 上执行，允许框架内部 CPU 回退。
- `npuAudioEnabled=false`、`neuralEngineExecutionVerified=false`：公开 API 没有逐次预测硬件计数，本实现不伪造 ANE 使用证明。实际硬件路径需要在对应 iPhone 上通过 Core ML Instruments 等工具核验。

若内部 FP16 精度、内存搬运开销或预测时间不合格，会明确报告原因并保持原生 CPU 渲染。此接入不保证每台手机获得 ANE 加速或性能收益。

## 构建和验证

`ios-host` 启用 renderer 的 `coreml-audio`，由 native facade 编译 Objective-C++ 桥并链接公开 CoreML/Foundation。podspec 加入 CoreML；原生部署目标与应用一致为 16.4。其他平台/独立 renderer 不会产生未提供桥的链接依赖。

`Core ML audio backend check` 工作流在 Apple runner 上：运行生产模型编码测试、实际 Core ML CPUOnly 数值/边界/输入不变回归、实际 32×1025 场景形状预测，以及 CPUAndNeuralEngine 可用时的结果记录；另编译 iOS 16.4 桥和实际 iOS Rust 集成。Mac/CPUOnly 成功不等于 iPhone ANE 实测通过。

官方依据：
- https://developer.apple.com/documentation/coreml/mlcomputeunits/cpuandneuralengine
- https://developer.apple.com/documentation/coreml/mlmodel/availablecomputedevices-6klyt
- https://developer.apple.com/documentation/coreml/mlpredictionoptions/outputbackings
- https://apple.github.io/coremltools/docs-guides/source/typed-execution.html
- https://github.com/apple/coremltools/tree/9.0/mlmodel/format
