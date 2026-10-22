# Android 远程诊断

Android 沿用 iOS 的只读诊断接口：`GET /diagnostics`（`GET /` 也可），TCP 端口 `8765`。设置 → 远程诊断 → 远程诊断服务。每次应用重新启动默认关闭；关闭开关或销毁模块会关闭监听及已连接的客户端。

手机和读取端连接同一个 Tailscale 网络后，通过 `http://<手机的 Tailscale IPv4>:8765/diagnostics` 读取。也支持同一局域网的手机 IPv4；设置页面显示本机地址。监听所有本机接口，无中转、无上传、无远程播放控制。访问范围由本机网络和 Tailscale 的访问规则决定。接口没有另外一套身份验证协议，与现有 iOS 接口一致，仅在需要诊断时手动开启。

返回 `schema: 1`，保留 iOS 的 `time`、`playback`、`playing`、`paused`、`background`、`generation`、`route`、`thermalState`、`events` 等常用字段。Android 补充：

- `playback.outputDevice`：AAudio 实际后端、硬件 xrun、缓冲帧数、burst、性能模式和错误码。不支持的硬件 xrun 保留 `null`，不能当作零次欠载。
- `playback.callbackMissingFrames`：软件 FIFO 未能提供的帧数；与硬件 xrun 分开看。
- `renderTotalMicros`、`renderMaxMicros`、`renderWorstStagesMicros`：累计渲染耗时、生命周期最大单块耗时和七阶段最大耗时。阶段顺序见 `renderStageOrder`。这些最大值不是每秒的局部最大值。
- `android`：机型、SoC、系统声明 ABI、实际进程架构、CPU 累计时间、内存、电源/待机状态、热状态，以及选定的音频/JS/UI 线程状态和 nice 值。`rendererArchitecture` 表示实际 Rust 库架构，不能只根据系统机型字符串判断是真机还是模拟器。线程 CPU 时间以系统 tick 计，没有换算成秒。
- `history`：最近 60 个样本，默认约一分钟；包含软件丢帧/硬件 xrun 增量、缓冲量、进度、CPU 累计时间、前后台、暂停和热状态。发生切歌时计数基线重置。
- `events`：最近 96 个播放、暂停、继续、停止、结束、错误及前后台事件。仅内存保存。
- `nativeSnapshotAgeMs`：既有播放状态快照的年龄。`snapshotAgeMs` 为 HTTP 缓存快照的年龄；历史中的 `sampleIntervalMs` 可显示应用被延迟调度的间隔。暂停、停止或尚未开始播放时，应结合 `playing`、`paused` 判断快照是否过期。

开启后每秒在低优先级线程整理快照，每五秒读取一次选定线程的 `/proc/self/task` 状态。复用播放器已经读取的原生状态，不新增 JNI 轮询；HTTP 工作线程只读取缓存，不持有播放器锁，不向音频回调添加网络/磁盘操作。历史数据有固定上限，无持续 PCM、诊断文件或整曲缓存。

不提供音频、歌名、艺术家、封面、文件路径、内容哈希或未经处理的异常描述。原生播放状态及输出设备配置使用字段白名单。HTTP 请求头最多 8 KiB，最多四个并发连接，连接在五秒内关闭；仅接受 GET，无缓存、无 keep-alive。服务刚启动但首个快照尚未完成时返回 503，可稍后重试。

排查“后台点击其他应用控件导致卡顿”时，先开启诊断再开始播放，持续每秒读取一次；卡顿后仍可读取最近一分钟的记录。比较同一 `generation` 内的 xrun、丢帧、FIFO、渲染时间和调度间隔，避免把换曲、暂停或冷启动缓冲算作卡顿。模拟器验证不能替代 Qualcomm / MediaTek 真机复现。
