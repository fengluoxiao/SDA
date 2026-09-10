## 让 native layout selector 恢复 master 的“同房间不同物理设备”语义

目标：保留最终双耳 HRTF/WASAPI 输出，但让 `Dolby 5.1 / 5.1.2 / 5.1.4 / 7.1.2 / 7.1.4 / 9.1.2 / 9.1.4 / 9.1.6` 真正改变房间内的虚拟物理扬声器布局、对象 VBAP 承接、bed 映射与对应 HRTF bus 图，而不是只改变前端房间视图。

### 1. 以 master 的 layout definitions 作为唯一布局真源

- 提取/镜像 `packages/renderer/src/layouts.ts` 中每个布局的 physical speaker 列表、名字、ADM 位置与高度层；不要在 Rust 手写一套与 master 易漂移的坐标。
- 建立受校验的 native layout descriptor：布局 ID、physical speaker positions、可直接对应的 bed channel 语义、LFE exclusions，以及对应的降级/映射规则。
- 为 `auto` 保留播放器现有的 decoded-label/object-content 自动选择；一旦自动检测得到布局，原生 sink 同步设置同一个 resolved layout。

### 2. 把 native 固定 7.1.4 bus 图改为 worker-owned 的可切换 physical speaker 图

- 使 `apps/native-renderer/src/vbap.rs` 和 `bus_renderer.rs` 从编译期固定 `BUS_COUNT = 11` 改为受上限保护的 layout-specific bus configuration。
- 为每个被选中的布局建立该布局实际存在的 non-LFE physical speaker bus，并基于 master 相同的物理方位构造 VBAP solver / fallback。
- 每个 physical speaker bus 都保留自己的 HRTF partitioned convolver；因此切 5.1、5.1.4、7.1.4、9.1.6 时，卷积图的 speaker 数、方位与对象分配都实际改变。
- LFE 继续保持现有独立低通、动态、左右耳分配路径，不进入 VBAP/HRTF bus，也不受 layout speaker-count 误影响。
- native render worker 继续独占 Engine 和 convolution graph；WASAPI callback 保持 FIFO-only，不增加 FFT、source scan、布局重建或锁。

### 3. 让 bed/object routing 随当前布局变化

- object：在 layout 切换后，以当前 object metadata/pose 重新求 master-aligned VBAP gain，使用现有 sample-timeline ramp 过渡，而不是瞬间跳到新布局。
- bed：按 selected layout 的 physical speaker names 映射固定 bed 标签；布局没有该 speaker 时，使用 master 相同的静态 fallback/VBAP 处理，而不是再偷回固定 7.1.4 路由。
- 切 layout 时重新绑定所有已声明 bed source 的 route，同时重算 object route；保留对象的 gain、mute、availability、PCM ring、活动状态和绝对 sample timeline。

### 4. 增加 layout control 的 end-to-end native 协议

- 新增一个版本化 `setLayout` native command，接受严格 allowlist 的 resolved layout ID；从 `SdaPlayer.setLayout()` 透传到 native sink，再经 `App.tsx`、preload、main process 送给 sidecar。
- 保持 main-process control serialization，等待 `setLayout` ACK，避免与 PCM/source declarations 交叉写入。
- 原生 worker 在控制命令中构建/切换新的 bus renderer，并用现有 render epoch + FIFO flush/reheat 边界隔离旧布局尾音；回 ACK 后才让后续 PCM 与 object events 在新布局下运行。
- UI 仍显示原有 selector，不再禁用；当前选择和 `auto` resolved layout 都会传给 native。手动切换与自动检测都立即对听感生效。

### 5. 测试与验证

- Rust 单测：各 layout 的 speaker count/positions、VBAP object gain 随布局改变、bed label direct map/fallback、LFE 恒不进入 bus、layout switch 保留 PCM/object/mute 但更换 routes。
- 协议集成测试：合法/非法 `setLayout` ACK、HRTF ready 后 layout 切换、活动 object/bed PCM 下的新布局正常 render。
- player/desktop contracts：`setLayout` 覆盖 sink、preload、main IPC、command serialization，`auto` layout 解析后同步 native。
- web build/TypeScript、Rust tests、binary protocol、desktop/player contracts 全量执行。
- 构建并重新 stage `SdaNativeRenderer.exe`，重启 Electron dev 实例。

### 刻意保持不变

- 最终输出仍是 native binaural/HRTF；这不是输出设备切换。
- 不恢复 Web Audio 可听渲染作为布局切换的后备。
- 不让 layout 切换工作进入 WASAPI callback。