# How Love 清晰度续查（2026-10-11）

本轮没有增益预设/EQ/空间位置修改，没有推送或打包。新增峰值保护数值收敛候选修复，详见第 4 节，不能误称正式版本已修复。用户侧不增加预扫描、分析缓存或等待；以下文件全是离线测试产物。

## 1. 窄带凹陷是否承载实际内容

可复现入口：`scripts/diag-how-love-narrow-filter-energy.py --audit <moving-filters目录>`。

- 覆盖 B2 的 11 个对象，55–63 秒、2498 个近似运动方向；16384 FFT，源局部 2048 Hann 窗。
- 参考为原 61 个 HRTF 的加权测量功率，不是 Apple 输出或声学真值。轨迹来自元数据斜坡近似，不冒充逐样本引擎轨迹。
- 48 个方向/耳组合出现小于 -1 dB 的窄带差异；实际声源加权后，500–4000 Hz 总差异为 +0.000001924 dB。
- 凹陷频点承载该频带独立路径参考能量的约 1.0384e-9。这里是频带能量比例，不是语义人声比例。
- 单对象频带差异范围约 -0.0000684～+0.0003184 dB。

结论：目前不能把约 2 kHz 的窄凹陷当成人声不清晰的原因；不据此增加补偿 EQ。

输出：`C:/Users/legendshop/Downloads/SDA-clarity-validation/moving-filters/all-source-weighted-narrow-check.json` 及同目录 `source-weighted-narrow-heatmap.png/.npz`。图像用 PIL 绘制，已实际查看；工作站无 matplotlib，首次绘图失败后修复并重新执行成功。

## 2. 当前方案的相关内容叠加

可复现入口：`scripts/diag-how-love-coherence-transfer.py --refresh --harness <test exe>`。

- 清除所有旧测试环境变量，当前 master=0、辅助总上限=1.5、mainRestore=0，重新回放 full + 16 路独立输出。
- 全部回放通过，独立路 minimumGuard=1；混音 STFT 加法残差最大 2.91697e-8。
- 500–1500 Hz 原始单声道 PCM 直接叠加 vs 独立能量之和为 +2.3235 dB，双耳渲染后为 -0.0294 dB。
- 1500–4000 Hz 相应为 +2.6502 dB 与 +0.8113 dB。
- 最大相关性变化仍是前方 Obj10/11；用户此前明确目标在 B2，不能把前方配对当作目标，不能据此开启已有 front-common 重居中实验。
- B2 内 Obj13/18 在 500–1500 Hz 的相关性从 +0.5190 变为 -0.2143，但其负交叉能量仅约整个输出的 0.6974%。这既可能来自合法双耳空间滤波，也不是证明目标声被错误抵消。

输出：`C:/Users/legendshop/Downloads/SDA-clarity-validation/coherence-transfer/report.json`、`pair-coherence.png`。`current-replay` 中保存这轮输出与日志，循环结束恢复完整输入，避免留下最后一路 solo 输入污染后续测试。

## 3. Lose 已认可参考的微小回归差异

- 旧 `total-lift-no-trim.f32` 实际含历史 -0.75 dB 输出 trim；先乘回 10^(0.75/20) 后才比较。文件名和旧 JSON 不足以表明 trim。
- 比较当前 verified，最大差约 0.00015422；整段相对残差 -105.34 dB。
- 差异集中在相对 fixture 的 1.5、30.9、130.5 秒附近，不是全曲电平重混。
- 使用同一测试二进制再次回放，最大差仍有 1.7464e-5：不宣称逐样本确定。
- 对最大差窗口拟合逐样本双耳共用比例后，残差最大约 1.92e-7（其他两窗约 2.33e-7、2.65e-7）。峰值保护账本显示最大差附近 guard 从接近 1 到 0.7624，再恢复到 0.9520。

结论：证据支持微小双耳共用峰值包络变化，而非独立右侧声源增益变化；尚未证明哈希归约顺序是唯一原因，不能用这份分析宣布已经完全复现旧参考。

新增 `dsp::tests::peak_guard_small_input_rounding_does_not_shift_binaural_image`，检查 44.1/48/96kHz、触发保护的相关双耳输入，微小输入舍入不破坏双耳比例/有限值和有界差异。该测试发现失败，随后按第 4 节修复。

## 4. 实际发现的峰值包络浮点停滞

- 原实现 `gain=max(target,gain+step)` 在差一个 float ulp、step=差值/lookahead 时，`gain+step` 舍入为 gain。本次测试明确构造 gain=0.75、target=0.74999994，旧实现无法到达 target。
- attack 分支持续命中，无法进入 hold/release。相关双耳合成波加约 1e-7 比例扰动时，旧集成测试 96kHz 输出差最大达 0.14167298（失败），不是微小偏差。
- 候选：只在 next_gain==gain 时置 gain=attack_target，允许下一次进入后续阶段。没有改变 ceiling、release_coeff、lookahead、双耳共用策略。
- 新增精确停滞复现测试 `peak_guard_completes_an_attack_smaller_than_one_gain_ulp`。
- 保留旧实现的离线拷贝运行两个新测试，两项均失败；修复后独立 rustc DSP wrapper 的 8 项测试全通过。完整引擎编译/三首回放需另行核验，不用 wrapper 冒充全集成验证。
- How Love 当前目标片段 minimumGuard=1，因此该修复不是顶部清晰度已解决的证据；这是自主检查时发现的独立通用稳定性缺陷。

离线回归入口：`scripts/diag-peak-guard-three-song-regression.py --harness <修复后 test exe>`。

### 集成与三首回放结果

修复后的完整引擎测试二进制：264 passed、0 failed、28 ignored；三个 fixture ignored replay 均另外显式执行通过。该次 cargo 编译早于新增的精确 ulp 构造测试，后者已在直接引用同一 dsp.rs 的独立 wrapper 中通过，不混报集成测试数量。

| 回放窗口 | 最大采样差 | 整窗能量差 dB | 最大 100ms 能量差 dB | 扣除双耳共用比例后的最大残差 |
|---|---:|---:|---:|---:|
| How Love 55–63s | 2.3842e-7 | 4.69e-9 | 1.4311e-7 | 1.7478e-7 |
| Very First Night 0–65s | 2.8908e-5 | 4.97e-7 | 9.0414e-5 | 2.7292e-7 |
| Lose 完整重置 fixture | 6.1095e-5 | 3.64e-7 | 0.00012662 | 3.4232e-7 |

比较对象为本轮前的 `production-total-lift-verified`；随机归约微差与修复效应未完全分离。不声称逐样本完全相同或客观证明听感不变。未发现独立耳比例或歌曲整体电平显著改变。How Love 目标段仍基本相同，不作为新的“更清晰试听”反复交给用户。

结果：`C:/Users/legendshop/Downloads/SDA-clarity-validation/peak-stall-regression/report.json`；全集成日志 `native-tests-peak-fixed.log`。

## 当前边界

这轮排除了一个实际内容几乎不命中的补偿方向，并建立了当前预设的相关性图。没有新听感改善候选得到证实，How Love 顶部内容清晰度仍未完成验收。不能把频谱、测试通过或整体相关性当作人声清晰度验收，也不再要求用户重新辨认 E/F。
