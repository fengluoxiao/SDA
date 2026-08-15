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
4. 不复制 GPL LibrePods 源码；不捆绑 KMDF driver。只有 transport 可靠且许可/安全评审通过后，才考虑分发更深集成。

## 非目标

- 不声称与 Apple 系统 Spatial Audio 等价。
- 不将实验性 kernel driver 加入 SDA installer。
- 不在 pose 更新时重建双耳卷积图。
- 不使用 raw AirPods 私有 packet 作为未经校准的音频四元数。
