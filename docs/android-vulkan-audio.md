# Android 系统 Vulkan 音频计算

使用系统 libvulkan.so 与 Vulkan 1.0 Compute，默认先尝试 GPU；不可用时保留系统 NNAPI 和原生 CPU 回退。默认安装包不打入 QNN、NeuroPilot、LiteRT 或旧 Hexagon 运行库，应用也不联网下载驱动。NDK 头文件与构建期着色器编译器不进入 APK。

GPU 只计算现有连续对象 HRTF 的 FP32 复数频域乘加。FFT/IFFT、对象状态、空间平衡、音量平衡和输出保护保持原路径。着色器由 glslc 在构建时生成 SPIR-V 1.0，检查进仓；运行时仅由系统驱动创建管线。precise/NoContraction 保留乘法、加减和分区累加顺序，不引入 FP16 或量化。

沿用现有实际音频块精度及端到端耗时准入。包括 CPU 打包、GPU 上传、同步、计算、结果复制和整体渲染耗时，合格且实际采用结果后才报告 gpuAudioEnabled；不把 GPU 记为 NPU。精度/时限失败保留同块 CPU 结果，不改变响度。

独立计算 worker 创建专用 VkDevice/队列，优先选仅计算队列，不依赖 UI、窗口或画面循环。持久映射 HOST_VISIBLE 存储缓冲区；非一致内存显式 flush/invalidate。CPU 数据包与 GPU 分配合计最多 32 MiB，一次仅一个有界批次。Fence 超时后，缓冲继续由 worker 持有，直至设备空闲才能释放；音频线程不等设备空闲。

设备/编译/内存能力不足会回退。系统驱动保持进程映射，拒绝 Vulkan CPU/已知软件设备。不同厂商和旧 ROM 的效果取决于实际驱动，不以枚举成功代替真实音频采用。

本轮按照用户要求不运行自动测试、不播放测试音频；用户后来明确授权安装到 USB 连接的小米 Note 3。安装与启动检查不代表已确认 GPU 实际执行或有性能收益。设备播放后的诊断以 audio.backendDetails、gpuAudioEnabled、audioBatchesExecuted、audioBatchesApplied 和失败原因区分真实状态。
