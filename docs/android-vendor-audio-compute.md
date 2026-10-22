# Android 厂商音频计算接入与骁龙 660 验证

日期：2026-10-22。改动在独立工作区验证，正式应用尚未替换。最低 Android API 26 保持。

## 已接入的代码

- 旧 Hexagon NN 1.20 浮点 DSP：明确支持旧 Hexagon 680/骁龙 660 的接口。使用原生 Add_f/Mul_f/Sub_f 表达同一 HRTF 复数乘加；输入及输出为 FP32，不经过 TFLite 的 INT8 delegate。运行时先核对真实算子名称，防止版本/枚举不兼容。
- LiteRT 2.3.0 厂商 NPU：使用公开 CompiledModel C API；仅请求 NPU，并同时要求 FullyAccelerated 与 NonCpuFullyAccelerated，拒绝残留 CPU 分区。支持所需公开 QNN/NeuroPilot 插件和运行库，不按手机型号宣称兼容。
- 图不支持或运行库不可用时尝试 NNAPI 专用加速器。结果精度/整体耗时不合格或超时时回到原渲染；不降低原有音频精度门槛。
- 只有实际采用计算结果后才报告启用。旧 DSP 用 dspAudioEnabled，NPU 用 npuAudioEnabled；共用 audioOffloadEnabled。不能把旧 DSP 当成现代专用 NPU。

FFT/IFFT、对象控制、原有音量/空间平衡和真峰值链保持。无整首扫描、PCM 缓存或逐歌模型缓存。首次运行库校验/复制在后台线程进行，渲染继续 CPU；已验证库存于一个应用私有版本目录。LiteRT 还计入 managed tensor buffer 搬运和同步成本，并限制应用自有缓冲预算。

## 已完成的验证

- 真实 Mi Note 3 ，系统公开 SoC ID 317：官方旧 Hexagon interface 独立兼容性探针返回 `publicInterfaceSupported=true interfaceVersion=137729`。这是 adb shell 环境的兼容识别；该项仅为兼容识别，后续实际执行证据见下面新增记录。
- 同一台手机上，LiteRT 的 CPU 测试执行生产 TFL3 图：1/2/3/4 分区的 8×17 形状，以及 2 分区 32×1025 形状。五项相对 RMS/峰值误差均为 0。此测试不证明 NPU 使用。
- 共用频谱/超时/精度/卷积回退回归 17 项通过；两种新增图的分区和左右结果均核对。
- Android arm64 renderer 与完整 native facade 的 cargo check 通过。
- Android SdaCore Kotlin Release 编译通过；没有安装或替换用户当前应用，没有播放测试音。

## 尚未验证及构建门槛

Google 官方 Hexagon 文档明确要求接受高通运行库许可，下载归档包含 `CLICK_THROUGH_LICENSE.txt`。全文保存于 `docs/licenses/qualcomm-hexagon-runtime.txt`。用户已明确接受这份旧版许可并授权加载测试及按条款分发。已运行旧 DSP skel，仍未声称 660 获得实时播放性能收益。现代 QAIRT SDK 也必须完成对应运行库条款审阅与依赖哈希冻结。

为保持现有构建可用，厂商资源打包默认不启用。确认许可并完成真机检查后，使用 `-PsdaVendorAudio=true` 启用；脚本要求 `SDA_ACCEPT_QUALCOMM_RUNTIME_LICENSE=1` 或明确的接受参数。设置该值须来自人的许可接受，不能由代理擅自设置。

`prepare-android-npu.py --legacy-only` 对应旧 Hexagon 资源；当前锁文件已记录旧接口和三种 DSP skel 的分析哈希。现代运行库的完整哈希清单需在许可确认后完成，不能将未完整冻结的现代资源用于发布。脚本只按 HTTP Range 下载 QAIRT 选定 ZIP 成员，拒绝服务器忽略 Range 时下载整个 >2 GiB SDK；下载内容从不作为脚本执行。

接下来需要以应用 UID 加载旧 DSP 并执行真实音频块，验证数据精度、左右声道、总搬运延时及锁屏播放。若驱动无法访问或无法取得性能收益，应报告具体失败原因；兼容识别成功和 CPU 测试成功不能替代这一步。

## 官方依据

- TensorFlow v2.14 Hexagon delegate 文档：明确列出 Snapdragon 660/Hexagon 680，并说明 SDK 许可确认要求；其标准 delegate 仅支持量化模型。
- Android Open Source Project Hexagon NN `ops.def` 与匹配 1.20 公开 headers：存在浮点 Add_f/Mul_f/Sub_f，故本实现直接使用底层浮点图接口。
- Google LiteRT 2.3.0 CompiledModel / Environment / TensorBuffer C headers 与公开 QNN、NeuroPilot 接入示例。

这些 API 的设备范围和真正可达性能仍取决于对应 SoC、系统驱动、SDK 和计算图支持。

## 许可确认后的 660 实测

- 独立 APK `app.sda.dspcheck20261022` 使用普通应用 UID 10601，目标 API 35，通过可选公共 FastRPC 库声明加载生产 Hexagon 后端。
- 5 种形状各执行 10 次；原始输入未变，FP32 精度门槛均通过。节点周期统计非零，确认 DSP 路径执行。
- 32×1025 / 2 分区：平均 DSP 调用约 41.8 ms，标量 CPU 参考约 9.05 ms，DSP 内部累计节点周期约 22.7M。旧浮点图的执行时间不满足播放准入，不应强行启用；实际 CPU 优化路径还可能更快。
- 旧接口没有导出头文件中的 `hexagon_nn_last_execution_cycles`。实现使用已导出的公开 `hexagon_nn_get_perfinfo`，避免把可选接口缺失误报成无法使用 DSP。
- 补充了准入失败后由 worker 释放驱动模型的生命周期处理，保留 NODELETE 运行库策略。失败/慢速候选不会让图模型一直持有。
- 自定义合并内核仅在独立诊断目录尝试，未接入生产代码。公共 unsigned-user-PD 请求被该 ROM 接受，但系统缺少 `fastrpc_shell_unsigned_3`，驱动在初始化中等待；不尝试绕过签名、权限或修改系统固件。这条原型没有进入生产加载路径。尚未执行任何自定义内核代码。

现代 QAIRT 2.50 使用不同的 AI Stack License，不能把用户对旧 Hexagon PKLA 的接受当作接受这份新协议。完整现代资源打包另需明确确认 `SDA_ACCEPT_QAIRT_RUNTIME_LICENSE=1`；本轮只启用已批准的旧库资源。
