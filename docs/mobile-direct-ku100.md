# 移动端 KU100 纯直达渲染

Android 与 iOS 共用 `apps/mobile/assets/hrtf-mobile-direct`，不再从桌面房间校准资产打包 HRTF。

## 资产

- 128 个不同的 SADIE II D1 KU100 原始 HRIR 测量方向，48 kHz、每耳 256 taps。
- 保留原有对象方向环及声床测量锚点，再使用球面最远点选择填补覆盖空隙。
- 不复制方向、不生成 128 个伪测量，也不从 BRIR 截取直达声。
- 保留原始 HRIR 的双耳时差、声级差和测量扬声器响应；不声称消除原始测量中的所有残余声学影响。
- 不包含 BRIR、房间残差、房间 EQ、早期反射或人工混响尾部。兼容 manifest 的 wet 字段全部指向一个零值文件。
- 原始样本未经独立耳归一化、房间均衡、裁剪或时间对齐。运行时沿用引擎的延迟对齐插值与 KU100 陷波保护。

生成命令（源归档必须匹配 manifest 中的 SHA-256）：

```powershell
node scripts/build-mobile-ku100.mjs tmp/sadie-source/D1.zip
node scripts/prepare-ios-assets.mjs
```

## 对象与声床声道

- 实际方向对象使用同一个 HRIR 网格进行连续方向处理。
- 声床声道同样使用延迟对齐方向插值，不再回退到桌面的房间校准虚拟扬声器资产。
- 离散逐对象路径直接使用原始短 HRIR，不分配 8192-tap 房间尾部滤波器。
- 声道滤波器长度为 260 taps（256 原始 taps 加插值延迟余量）；移动端标记只影响该资产，不改变桌面链路。
- 移动端资产的 wet 参数不参与声音合成，房间配置不被应用。
- 逐对象渲染、实际方向在首次使用和旧设置迁移后默认开启，用户之后仍可改变对象开关。

## 设置和打包

只保留“高解析逐对象”预设，wet 权重固定为 0。Android/iOS 首次迁移重置旧的 room、near、wet 和对象开关。房间与近场界面移除，兼容原生 API 只允许关闭操作。Android Gradle 与 iOS podspec 不打包桌面 HRTF 或 rooms 目录。

已启用的 iOS 系统 360RA 空间音频选项保持原有行为；该系统输出路径旁路 SDA KU100，并非本次 KU100 资产优化的对象。

## 验证

```powershell
node scripts/test/mobile-rendering-presets.test.cjs
node scripts/test-ios-ui.mjs
node scripts/test-ios-playback-polling.mjs
node apps/mobile/node_modules/typescript/bin/tsc -p apps/mobile/tsconfig.json --noEmit
cargo test --manifest-path apps/native-renderer/Cargo.toml --no-default-features --locked hrtf::
```

测试覆盖：128 个唯一测量方向、每个文件 SHA-256、零 wet、iOS 资产一致、设置迁移及打包规则、界面移除、实时预设切换不重置播放状态、对象原始响应、声床插值一致性、拒绝房间处理，以及既有桌面 HRTF 回归。

滤波器/资产尺寸减少不代表已经测得特定 CPU 降幅；真机听感、移动对象动态听测、各设备实时负载仍需要设备验证。
