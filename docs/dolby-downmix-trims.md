# Dolby downmix trims 接入说明

## 范围与数据来源

本次把 E-AC-3 / Atmos JOC 与 TrueHD Atmos 的 OAMD trim 从解码端接到共享原生渲染器。Windows、macOS、Android、iOS 使用同一套 Rust 元数据结构和处理阶段。Web/Electron 的 WASM 解码接口同时提供这些元数据；本文的音频处理验证针对原生渲染后端，浏览器独立 Web Audio 后端不在本次等价性验证范围。

解析规则对照 ETSI TS 103 420 V1.2.1 的 5.5.12、5.6.5 和表 32–39；九组配置数量对应 ETSI TS 103 190-2 V1.2.1 的 6.3.9.10.4。配置顺序对照 DAMF 的 No/Some/Many Surrounds 与 No/Some/Many Heights 分类。不会根据歌曲名、对象编号或 payload 长度猜测配置数量。

参考资料：

- ETSI TS 103 420 V1.2.1：<https://www.etsi.org/deliver/etsi_ts/103400_103499/103420/01.02.01_60/ts_103420v010201p.pdf>
- ETSI TS 103 190-2 V1.2.1：6.3.9.10.4。
- truehdd 的 DAMF 分类转换：<https://github.com/truehdd/truehdd/blob/master/src/damf.rs>

`crates/sda-render-metadata` 是独立 Apache-2.0 共享结构，不依赖播放器、音频设备或具体解码器，便于渲染器以后独立使用。

## 已传递的字段

解码帧的 `downmixTrims` 是带采样时间戳的更新列表。每条更新包含 `samplePos` 和 `trims`：

| 字段 | 含义 |
| --- | --- |
| `warpMode` | 0 保留位置；1 在渲染坐标中将 Y 乘 2。原始对象坐标和运动目标不改写 |
| `globalMode` | 0 使用渲染器默认；1 禁用 trim；2 逐配置控制 |
| `configurations` | 九组布局分类的模式及显式参数 |
| `centreDb` / `surroundDb` / `heightDb` | 中心、环绕、高度 trim，单位 dB；null 表示没有传输该显式数值 |
| `topBottomY` / `listenerY` | 上下平面、听众平面的前后平衡；正值指向后方 |
| `disabledObjects` | 将 OAMD 索引转换为实际动态对象 ID 后的 trim 禁用列表 |
| `disabledBeds` | 保留床声道角色名的 trim 禁用列表；渲染器识别 TrueHD / E-AC-3 的等价角色别名 |

配置本身的 `mode` 同样为 0 默认、1 禁用、2 自定义。禁用只跳过 trim，不会删除或静音对应对象；对象原来的 gain 仍然有效。LFE 旁路该处理。

## 布局选择

配置索引为 `环绕档位 + 3 × 高度档位`：无对应扬声器为 0，两只以内为 1，多于两只为 2。

| 虚拟/物理目标布局 | 配置索引 |
| --- | ---: |
| 2.0 / 2.1 | 0 |
| 5.1 | 1 |
| 7.1 | 2 |
| 5.1.2 | 4 |
| 7.1.2 | 5 |
| 5.1.4 | 7 |
| 7.1.4 / 9.1.4 / 9.1.6 | 8 |

耳机最终输出两个声道，并不意味着自动选择 2.0 配置。若当前虚拟渲染布局是 7.1.4，仍选择第 8 组。改变目标布局会重新选择并平滑更新增益。

## SDA 的渲染策略

显式自定义 trim 在源域应用一次，位于对象 gain 之后、空间增强与声学渲染之前。同一路音频的直达、反射、低频等后续路径因此获得一致的 authored trim；不会把同一份 trim 对各个后续路径重复应用。

SDA 用固定 7.1.4 参考路由计算中心、环绕、高度的贡献。源增益是各参考路由增益平方加权后的能量比平方根，即 `sqrt(sum(route² × trim_linear²) / sum(route²))`。同一来源使用相同的增益，无论走逐对象 HRTF、虚拟扬声器还是多对象并行混音。前后 balance 使用同一参考路由上的衰减平衡，不以总体响度目标抵消作者的参数。增益和方向权重依据场景坐标，不随听众转头重新分组。

这是 SDA 原有声像框架内对 authored 控制的实现，不是 Dolby Renderer 的私有渲染方程或官方比特精确认证。公开规范提供字段与属性含义；不能把本次测试扩展成与 Dolby 所有自动 trim 策略等价的结论。

`default` 保留的是 SDA 当前默认 trim 策略，不能解释成码流明确写入了 0 dB。未传输的参数保持 null；不会为其伪造自定义数值。普通兼容床 PCM 的选择会清除对象 trim 更新，避免对编码器已经生成的兼容下混重复处理。现有音量平衡、空间平衡与最终峰值保护继续各自工作。

E-AC-3 按其 ETSI 表拒绝保留码值，码值 14 为 −16 dB。vendored TrueHD 0.6.3 保留上游自己的 −15 dB 查表，只修正 trim presence 的位顺序，未把两个输入格式的查表擅自合并。

## 时间、重置与容量

- trim 与对象元数据使用相同的节目采样时钟。E-AC-3 的条件化路径包括对应的解码时延补偿。
- 解码帧携带当前状态及原始生效时间，保证跳转、分包变化和播放器重建不会丢掉 trim。
- 帧合并会保留自动化更新并去除重复状态；需要 PCM 重采样时，trim 时间戳同步转换。
- 新的增益目标通过 128 个采样平滑过渡；无 metadata 和默认 trim 的中性路径保持逐采样不变。
- 切换 HRTF 或对象渲染路径保留文件参数；换曲/节目 reset 清除旧时间线。
- E-AC-3 新增 trim 解析限制在 element 的真实边界内，保留值/截断不会越界读取下一个 element。可选 trim 元素不再重置对象的差分元数据状态。
- 单个命令和时间线均有容量上限，避免无限堆积。

## 本次验证

- E-AC-3 完整库测试，包括新增 trim 位顺序、表值、禁用标志、截断/越界与跨帧状态。
- TrueHD 实际解码示例、trim presence 位测试；修正了低编号有效 presentation 被调用方忽略的问题。
- 共享结构 JSON 往返、非法值与布局选择。
- 原生实际 PCM 的中心/环绕/高度增益、LFE/逐对象 bypass、默认逐采样一致、时间戳/过渡、HRTF 切换/节目 reset、warp 与等效目标方向的 PCM 一致。
- 快速混音测试使用 8 路合成对象，确认真实进入源优先混音分支；额外静音测试对象只存在于测试中。
- TypeScript 帧合并、SRC、类型检查及 Electron IPC 语法检查。
- 移动引擎完整回归中，独占 MPEG-H 解码桥的测试串行复核；HRTF 切换测试按当前资源清单的方向数量检查，避免继续硬编码已替换的 KU100 17/61 方向。

真实素材只读验证使用《壱雫空》的开头流拷贝片段：共 65 帧、99,840 样本，15 个动态对象 ID（10–24）全部保留；读取到 globalMode=2、warpMode=0，九组配置分别为禁用或默认，没有显式非零 dB 数值。8,192 字节与 127 字节两种输入分包的 PCM 哈希、对象事件和 trim 元数据完全一致。此结果证明该片段的数据传递，不代表发现了需要额外增加音量的参数。

本机 Windows 使用原有 release 优化级别编译。尚未在实体 iOS/Android 设备上进行新版本安装试听；已有 0.9.3A2 手机安装包不会因源码更新自动获得此功能。

复核入口：

```text
cargo test --manifest-path scripts/eac3-unit-tests/Cargo.toml --locked
cargo test --manifest-path vendor/truehd/Cargo.toml trim
cargo test --manifest-path crates/sda-render-metadata/Cargo.toml
cargo test --manifest-path packages/core/Cargo.toml --no-default-features
cargo test --manifest-path apps/native-renderer/Cargo.toml --no-default-features dolby_trim
cargo test --manifest-path crates/sda-native/Cargo.toml --no-default-features dolby
```
