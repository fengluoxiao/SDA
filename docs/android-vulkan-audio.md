# Android 系统 Vulkan 音频计算

使用系统 libvulkan.so 与 Vulkan 1.0 Compute，默认先尝试 GPU；不可用时保留系统 NNAPI 和原生 CPU 回退。默认安装包不打入 QNN、NeuroPilot、LiteRT 或旧 Hexagon 运行库，应用也不联网下载驱动。NDK 头文件与构建期着色器编译器不进入 APK。

GPU 只计算现有连续对象 HRTF 的 FP32 复数频域乘加。FFT/IFFT、对象状态、空间平衡、音量平衡和输出保护保持原路径。着色器由 glslc 在构建时生成 SPIR-V 1.0，检查进仓；运行时仅由系统驱动创建管线。precise/NoContraction 保留乘法、加减和分区累加顺序，不引入 FP16 或量化。

沿用现有实际音频块精度及端到端耗时准入。包括 CPU 打包、GPU 上传、同步、计算、结果复制和整体渲染耗时，合格且实际采用结果后才报告 gpuAudioEnabled；不把 GPU 记为 NPU。精度/时限失败保留同块 CPU 结果，不改变响度。

独立计算 worker 创建专用 VkDevice/队列，优先选仅计算队列，不依赖 UI、窗口或画面循环。持久映射 HOST_VISIBLE 存储缓冲区；非一致内存显式 flush/invalidate。CPU 数据包与 GPU 分配合计最多 32 MiB，一次仅一个有界批次。Fence 超时后，缓冲继续由 worker 持有，直至设备空闲才能释放；音频线程不等设备空闲。

设备/编译/内存能力不足会回退。系统驱动保持进程映射，拒绝 Vulkan CPU/已知软件设备。不同厂商和旧 ROM 的效果取决于实际驱动，不以枚举成功代替真实音频采用。

本轮按照用户要求不运行自动测试、不播放测试音频；用户后来明确授权安装到 USB 连接的小米 Note 3。安装与启动检查不代表已确认 GPU 实际执行或有性能收益。设备播放后的诊断以 audio.backendDetails、gpuAudioEnabled、audioBatchesExecuted、audioBatchesApplied 和失败原因区分真实状态。

启动检查发现 C++ 标准库静态链接遗漏的异常展开符号后，桥接改用 POSIX 时钟和 C 内存分配，不依赖 C++ STL/ABI 运行库；Android facade 的最终链接启用 --no-undefined。安装检查需要重新确认修正包能够启动，仍不自动开始播放。

## 2026-10-23 首次实机检查

小米 Note 3 / Adreno 512 能创建并执行 Vulkan 音频管线，初始实现完成两批真实音频、未采用 GPU 结果；第三批在上传阶段超过 1 ms，按保护条件回退 CPU。播放输出暂未出现缺帧。后续优先 HOST_CACHED 映射，保留非一致内存的显式 flush/invalidate，并增加阶段耗时与主机线程 CPU 时间。GPU 资格核对阶段播放 CPU 结果，使用有界 100 ms 测量窗口获取完整成本；原有精度、20% 性能余量和正式采用时的最多 1 ms 截止条件不变。当前结果不能据此宣称取得加速收益。

## 2026-10-23 实机验证结果

验证设备为 Mi Note 3 / Snapdragon 660 / Adreno 512，Android API 35，Vulkan API 1.1.87（driverVersion 原始值 2149183488）。测试安装包源码为 d076a9f，Universal 包约 112.36 MiB，未包含厂商 NPU 运行库。

- 独立硬件检查使用生产桥接与同一 SPIR-V：9 组形状通过，覆盖 1/2/3/4 分区、8～256 行、17～2049 频点、正常和低幅度输入。核对输出的相对 RMS/峰值误差测得为 0。复现程序为 scripts/test-vulkan-audio.cpp，链接 src/vulkan_spectral.cpp，通过参数提供 vulkan_spectral.spv；不产生音频输出。
- 共享精度、整块盈利判定、时限隔离及回退回归 18 项通过。
- 360RA 群青：代表性核对批次 CPU 乘加 0.596 ms，GPU 往返 12.917 ms，另有打包 0.773 ms、结果拷贝 0.295 ms；CPU 整个渲染块 2.529 ms。GPU 三批通过精度、实际采用 0 批。
- E-AC3 天球のMúsica：代表性核对批次 CPU 乘加 0.766 ms，GPU 往返 6.602 ms，另有打包 0.639 ms、结果拷贝 0.338 ms；CPU 整个渲染块 1.921 ms。GPU 三批通过精度、实际采用 0 批。
- GPU 往返包含提交调度、上传、GPU 等待与读回，不能当作纯 GPU 内核时长。当前映射实际选中 flags 15；公开内存类型比较没有找到更快的可用选择。
- 自动 CPU 回退状态下，切换到设置界面滚动、锁屏、暂停恢复未出现回调缺帧或设备欠载增量。新 generation 内暂停位置保持 41,570 ms，恢复后推进到 48,840 ms；暂停前后渲染增益均为 0 dB。没有播放报错。此结果不证明 GPU 持续采用时的播放稳定性。

结论：GPU 能执行且精度合格，但当前小型频域乘加的数据打包、往返和等待开销在这台设备上超过 CPU，保护逻辑正确保留 CPU 输出。不能宣称已取得 GPU 加速收益，也不能将该结果推及其他高通或天玑设备。后续性能工作需要减少往返或合并更长处理链；本次不降低精度或正式播放的性能/截止要求。原始测试记录保存在任务产物中。
