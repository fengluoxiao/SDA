# 小幅头动、先到声、空间宽度和视听坐标审计

基线：已接受的 iOS 0.9.40 / 991，源码 `bbd7db465a7de2a4c216422924cf3e4af9ccc525`。
本轮依据研究检查实现和边界，不更换声音模型或增加试听参数。

## 四个方向

| 方向 | 研究依据 | 本次结果 |
|---|---|---|
| 小幅头动与前后辨识 | [McLachlan 等，2023](https://doi.org/10.3389/fnins.2023.1027827) | 检查原生路由与最终姿态消费。新增验证：前、后对象保持位置不变时，±0.01°、±0.1°、±0.5°姿态及回中仍更新 HRTF 路由。原生最终姿态无需等下一条消息即可生效的回归通过。数字描述软件输入，不是耳机传感器精度或人耳定位阈值。 |
| 先到声效应 | [Litovsky 等，1999](https://doi.org/10.1121/1.427914) | 检查首达、短反射、尾部能量和残差合入；已有回归通过。保留现有直达/反射参数，没有依据综述增加混响或改变先到声权重。 |
| 声源宽度与包围感 | [Bradley、Soulodre，1995](https://doi.org/10.1121/1.411951) | 验证宽、高、深的面积路径与扩散路径、零面积旁路、总线功率、扩散通道去相关及实际 PCM 差异。现有连续渲染宽度曲线有明确的稳定听感设计说明，未将其与另一实现的差异直接判作缺陷。 |
| 视觉与声音方向 | [Alais、Burr，2004](https://doi.org/10.1016/j.cub.2004.01.029) | 发现并修复共享 JS 球面坐标转换在较大坐标上的仰角错误。验证 ADM、Three、同一姿态下 VR 相机逆旋转的方向关系，以及场景 seek 更新。 |

## 已修复：距离上限改变仰角

`packages/renderer/src/coords.ts` 的 `admToSpherical` 先将向量长度限制到 4，再将这个值用于 `asin(z/distance)`。当坐标长度大于 4 时，计算的仰角会变大，严重时直接变成 ±90°。

例子：`[0,8,8]` 的方向应为仰角 45°。旧 JS 结果为 90°，而原生 `spatial::adm_to_spherical` 已使用实际向量长度，返回 45°。直接 ADM→Three 与 ADM→球面→Three 两种路径因此也不一致。

修复将实际长度 `radius` 与有上限的返回值 `distance` 分开。方向使用实际长度，返回距离继续限制到 4。原有坐标轴、参考距离上限和零向量行为保持。

修复前新增的两项回归均失败：

- 同一向量缩放后的方向不变性。
- 参考立方体外，直接 ADM 和球面转换路径的场景方向一致性。

修复后均通过。没有修改原生 HRTF、近场、空间平衡、声源宽度、混响或耳机补偿参数。

## 验证

JS 定向命令：

```sh
node --test scripts/test/spatial-visual-cues.test.cjs apps/mobile/test/cardboard-vr.test.mjs apps/mobile/test/scene-performance.test.mjs
```

32 项通过，覆盖坐标缩放、直接/球面路径、iOS/Android 的两种横屏方向、小幅头动、头部姿态和屏幕旋转、左右眼及镜片映射、相机恢复、对象移动/seek、60/120 Hz 模型与场景生命周期。共享坐标模块的 TypeScript 检查通过。

原生定向命令：

```sh
cargo test --manifest-path apps/native-renderer/Cargo.toml --no-default-features --locked --lib -- small_head_turns throttled_final_pose clear_pose_removes source_extent::tests temporal_window_preserves_direct tail_taper_preserves_direct bundled_cues_preserve_direct integrated_bank_keeps_the_original coordinate_scale_and_display head_tracking_bed_regression --nocapture --test-threads=1
```

14 项通过，0 失败，1 项离线性能基准按原有标记未运行。覆盖面积路由、扩散能量/线性叠加、直达和反射、宽高区分、小角度 HRTF 路由、回中、最终姿态消费、世界固定声床与 LFE。

打包流程继续保持 package-only；未添加测试步骤，也没有运行完整解码/扫描/封面测试集。

## 验证边界

场景显示的是文件对象元数据中心，VR 相机仍为独立视觉控制；没有新增手机姿态接管音频、改动文件的渲染变换或修改回中策略。坐标方向验证基于相同输入与相同姿态，不代表两个独立传感器的绝对校准已经一致。

本轮使用本地原生和 Three 数学/组件模型。未宣称已完成实机镜片校准、实际 60/120 fps 测量、真人前后辨识或先到声听觉实验，也没有进行设备视觉验收或打包。
