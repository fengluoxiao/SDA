# Windows AirPods 头部追踪实施计划

## 目标

为 SDA 双耳渲染加入设备无关的头部姿态输入：将世界坐标中的对象与方向性床声道变换为头部相对坐标，再由既有 VBAP → 固定虚拟音箱 → KU100 BRIR 管线渲染。首期提供模拟姿态和 Electron provider 接口；Windows AirPods 数据源作为显式实验性外部 helper 接入。

## 架构

```text
pose provider (mock / external helper)
  → Electron main / preload
  → App tracking lifecycle
  → SdaPlayer.setHeadPose(...)
  → SpatialRenderer.setHeadPose(...)
  → inverse(head orientation) × source world direction
  → existing VBAP gain batch
  → existing AudioWorklet / fixed BRIR buses
```

头部姿态更新不得重建 ConvolverNode、AudioContext 或 worklet。它只重算既有 source 到固定虚拟音箱总线的增益。

## 坐标与音频规则

- 使用 SDA ADM 坐标：`+x` 右、`+y` 前、`+z` 上。
- 对 world-locked 定位：`sourceHead = inverse(headWorld) × sourceWorld`。
- LFE 保持无方向性，不参与旋转。
- 方向性 bed 与 object 都参与变换；不可继续以 `snapBus` 固定在头部。
- 不把 AirPods/设备姿态直接当作 ADM 姿态：provider 必须先完成设备轴、手性、四元数方向和 recenter 校准。
- 外部姿态为 wall-clock 数据，使用平滑和限频的即时 gain 更新，不冒充 codec sample metadata。

## 分阶段实现

### Phase 1 — SDA pose engine

1. 新增 `head-pose.ts`：纯四元数/向量运算与 ADM 方向变换。
2. 在 `SpatialRenderer` 保留 canonical world source directions；实现 `setHeadPose()` / `clearHeadPose()` / `recenterHeadPose()`。
3. 限频合并 pose 更新，批量发送增益，避免高频 MessagePort 压力。
4. 在 `SdaPlayer` 暴露 pose facade。
5. 为正负 yaw、recenter、对象/bed/LFE、零姿态等添加回归。

### Phase 2 — Electron provider contract

1. 定义窄 IPC 接口：tracking status、pose event、start/stop、recenter。
2. 首个 provider 是 mock/trace provider，用于调试和真实渲染验证。
3. 在 UI 暴露实验性状态、启停和 recenter；姿态超时平滑降级到固定双耳。

### Phase 3 — Windows AirPods helper feasibility

1. 作为独立实验程序研究标准用户态 L2CAP 是否能持续获得 AACP head-tracking packet stream。
2. 以标准化 pose JSON/pipe 接入 SDA，不将蓝牙协议和 renderer 混合。
3. 先验证多适配器、不同固件、长时间流、A2DP 并发、时钟/轴/漂移。
4. 不复制 GPL LibrePods 源码；KMDF driver 只在 Windows 预览安装器中作为明确选择的实验功能分发。

## 非目标

- 不声称与 Apple 系统 Spatial Audio 等价。
- 不默认安装实验性 kernel driver，也不在未经用户勾选和管理员确认时修改 TestSigning。
- 不在 pose 更新时重建双耳卷积图。
- 不使用 raw AirPods 私有 packet 作为未经校准的音频四元数。

## 2026-08-15 Windows 11 实机结论

- 已完成 LibrePods AACP 握手、通知、motion stream 启停包、10 帧校准和姿态映射的独立 Rust helper 移植；JSONL 接口已接入 Electron，renderer 姿态链路可测试。
- 实机已配对并连接 AirPods Pro，Windows 同时显示麦克风和音频已连接；Winsock catalog 也存在 `MSAFD L2CAP [Bluetooth]`。
- 对 PSM `0x1001` 直接连接以及 AACP UUID service lookup 均返回 `WSAENETDOWN (10050)`。这不是 A2DP 未连接。
- 微软官方 `Bluetooth and socket` 文档明确列出应用支持的 Bluetooth Winsock protocol 为 `BTHPROTO_RFCOMM`，未支持 `BTHPROTO_L2CAP`。catalog 中的 L2CAP provider 不能据此视为普通桌面应用可用接口。
- 因此当前用户态 Winsock transport 只能保留为研究原型，不能声称已在原生 Windows 蓝牙栈上完成硬件可用性验证。若要纯 Windows 落地，需要实现、签名并安装 Bluetooth profile kernel driver；SDA 的 JSONL、校准和 renderer 部分可以复用。
- 已实现白名单 KMDF profile driver，并在独立 Windows 预览安装器中提供两个默认关闭的选项：开启 TestSigning、安装驱动。两者互不隐式触发，均通过管理员 PowerShell 执行，且不会自动重启。

官方依据：https://learn.microsoft.com/windows/win32/bluetooth/bluetooth-and-socket
