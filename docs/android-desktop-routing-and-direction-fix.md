# Windows 提交路径与方向滤波器调查

**用户听感否决：**用户试听本次 APK 后反馈“更糊了”。已在 MuMu 回退到 `SDA-android-clean-rendering-x86_64.apk`（SHA-256 `6e3a4a90ccad92969f1f312d25d1cddccb7c05f2c2ced4679fe55aeb62d44037`）。本页改动与测试仅保留为待调查记录，不得把这次 APK 当作已接受的音质修复版本。

## 确认并修复的缺陷

`ContinuousSource::schedule` 把“与待应用方向相同”误当成“不需要更新”，清除了 `pending`。静音时 `finish` 不装载滤波器；下一块重新发声若重复同一目标，就可能沿用旧滤波器或零滤波器。现在保留相同的待应用目标，并以实际已应用的方向决定是否取消／重排更新。

回归测试使用真实 KU100：先静音预备，再发声，中间静音并从前方切换到后方。每块重复通知与仅在目标改变时通知的音频应相同。原实现失败（最大样本差异 0.050703272），修复后通过。原有小角度累计更新阈值测试通过。早期八次连续调度的合成测试也复现过，但引擎实际每个卷积块只调度一次，不能把八次调度描述成引擎常规行为。

## Android 提交路径对齐

新 `frame_router.rs` 使用 Windows 播放器的对象 ID／通道映射优先级、按通道索引识别声床、声明时间戳、对象退出时序，以及 decoder.worker 的重复目标去重。映射异常明确报错。新增三项测试覆盖标签与映射冲突、对象退出与重新进入、重叠插值期间不得去重以及无效映射。全部 19 项 mobile 单元测试通过。

第二首前 12 秒的映射本身没有错位，5625 条事件中可去掉 5567 条已完成的重复目标。仅此提交路径修正对整首 189 秒的 PCM RMS 差异为 3.0757e-8，不能解释发糊。

## 实际歌曲影响与限制

方向更新修复对第二首整曲 189 秒的最大样本变化为 0.0014390、RMS 变化为 3.0888e-6；只有 720 帧的差异超过 1e-5，说明影响局限于少量瞬态。它是确定的缺陷，但不是已证实的“所有歌曲整体发糊”根因。

提取仓库已有的 `7ee75f2^` 与 `25b5207` Windows 成品侧车运行，没有编译 Windows 应用。与当前侧车对照第二首前 12 秒，4–10 kHz 相对 100–1000 Hz 的能量差分别约 0.07 dB、0.75 dB，旧版本未显示整体更强的此频段。该测试只覆盖指定片段，且没有用户认可的实际 Windows 会话作为参考，不能据此否认用户听感。

正式 APK：`E:/SDA/apk/SDA-android-direction-update-fix-x86_64.apk`。SHA-256：`ac307014e7eb4381e709a387e09610e9f5e10d3cce030026451d79a389c20de3`。

数据：`E:/SDA/tools/mygo-direction-fixed-android.f32`、`direction-fix-song-result.json`、`direction-silent-before.log`、`direction-silent-final.log`、`mygo-before-spatial-staged-renderer.f32`、`mygo-ku100-sky-staged-renderer.f32`。未用高频 EQ 掩盖问题，整体发糊仍未解决。
