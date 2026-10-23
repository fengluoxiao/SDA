# How Love 附加残响候选（2026-10-11）

此前续查是进展：找到了并修复了峰值保护停滞，完成三首回放；但它不解释 How Love 目标段。本轮继续实际相位核对和不同机制的候选渲染，没有将完成目标缩小为测试通过。

## 相位/群延迟核对

`scripts/diag-how-love-filter-phase.py` 使用 2498 个近似运动方向、与 spectral bank 一致的 2048FFT，比较输出 vs 加权原测量解缠相位。

500–4000 Hz 按实际 B2 源频谱加权，纯相位复响应误差相对参考能量为 -73.71 dB；相位误差超过 0.1rad 的参考能量比例约 7.16e-8。频谱凹口附近的群延迟导数极值不等同于额外整体时延。这是实现参考核对，不是 Apple、 authored voice 或声学真值，不据此做全局相位重塑。

输出：`C:/Users/legendshop/Downloads/SDA-clarity-validation/moving-filters/phase-delay-check.json`。

## 先排除错误试听

开始尝试残响 -6dB 时检查到当前 manifest 已有 `spatialCueGain=0.5011872336272722`（即 -6dB）。平铺临时 manifest 的目录还失去 `hrtf-dense` 推断出的标准 `hrtf` fallback，导致输出变化不只是残响。

`cue-minus6-candidate/report.json` 已显式标记 `validForAudition=false` 并记录原因，不交付这组错误候选、不删除历史证据。

## 有效的 -12dB 候选

可复现入口：`scripts/diag-cue-residual-three-song.py --harness <test exe>`。

- 当前残响 -6dB vs 候选残响 -12dB，即仅将附加残响再乘 0.501187。
- 原直达 HRIR、位置、原 PCM 元数据、辅助总增益 +1.5/master0/mainRestore0/finalTrim0 均保持。
- 保留临时 `assets/hrtf-dense` 与匹配的 `assets/hrtf` fallback 结构。先以不改变 cue gain 的 relocation control 完整回放 How Love；与原基线最大差 2.68221e-7，验证目录搬迁没有再更换渲染图。
- 同一配置用于三首，完整渲染 How 至 64s、Very 至66s、Lose 138.016s重置fixture。未引入生产歌曲名或对象 ID 条件。
- 各试听仅用离线单一 LUFS matching 标量去除响度偏好；不是生产归一化或播放分析。

| 歌曲完整回放 | 500–1500 Hz功率变化 dB | 1500–4000 Hz功率变化 dB | R/L功率比变化 dB |
|---|---:|---:|---:|
| How Love | -0.08090 | -0.00356 | +0.03556 |
| Very First Night | -0.10060 | -0.01584 | +0.03566 |
| Lose Yourself | -0.00179 | +0.00766 | +0.01387 |

这不是“完全不改变听感”：附加空间线索确实变化，可能影响空间感。仅说明没有用整体/辅助声源放大制造清晰假象；回归听感仍待验收。

所有输出 PCM24 回读、有限值、4x 过采样峰值检查通过。How 候选 55–63s峰值 -4.937dBFS，minimumGuard1；Very 候选0–65s峰值 -0.854dBFS；Lose试听16s峰值 -0.651dBFS。未声称 sample-peak guard 是 true-peak limiter。

输出目录：`C:/Users/legendshop/Downloads/SDA-clarity-validation/cue-minus12-valid-candidate`。

- `How-Love-A-then-B-continuous.wav`：0–8s现有残响-6dB，8–9s静音，9–17s候选残响-12dB；两段对应相同原曲55–63s。
- `Very-First-Night-{baseline,residual-minus12-matched}.wav`：原曲0–65s。
- `Lose-Yourself-{baseline,residual-minus12-matched}.wav`：完整重置fixture相对1–17s，不在未知原点情况下冒充精确原曲时间。
- `report.json`、`How-Love-continuous.json`：测量、hash、试听位置与限制。

状态：离线候选，未改正式默认、未推送/打包；不能从功率变化证明目标更清晰，也不能凭三首数值测试保证所有歌曲。需要听感接受才能保留该方向。

## 最新听感回归：候选否决

用户对26秒current/-9/-12连续试听反馈「18s可以」，记录为How目标段-12dB通过，不扩大为三首通过。随后明确反馈「lose yourself右边大声了」。因此整体-12dB残响方案未满足三首保持听感的条件，不升级默认，不采用歌曲名/对象ID/单独右耳压低补丁。

针对实际交付Lose原曲115–131s（native origin5471232）核对100ms窗口：500–1500Hz最大右相对左增加约0.4875dB（126.6s），其中左变化-0.6065dB、右-0.1191dB；1500–4000Hz最大右相对左增加约0.2396dB（127.6s），其中右本身增加0.0737dB。整段平均值不能覆盖这些局部变化，也不能以这些混合频带统计代替独立come-on测量或否定用户听感。

结论：统一降低附加残响并不保证统一改善；直接声音和残响相位关系不同，两耳受影响不同。原来通用+1.5声源上限与残响-6基线继续保留。下一候选不能只继续减残响强度，并必须在交试听前检查已失败的局部左右变化。

证据：`C:/Users/legendshop/Downloads/SDA-clarity-validation/lose-minus12-user-rejection-audit.json`。

## 后续短时回归及归因（同日）

上一轮产生候选、渲染和报告属于进展，但未完成听感验收。本轮未叠加新 DSP，实际细化候选风险：

- `diag-cue-candidate-local-regression.py`：100ms窗口，按频带过滤完整回放（非截段后才滤波），活动门限为最大功率-30dB及绝对1e-10。
- 500–1500Hz局部 R/L变化：How最大0.3324dB；Very最大1.1157dB（34.3s），其50–65s历史侧唱区域最大0.5434dB（63.2s）；Lose最大1.4899dB（重置fixture相对115.3s）。不误把Lose相对115.3s标为原曲115.3s。
- `diag-cue-limiter-attribution.py` 将两配置都在保护前降低12dB，六次回放minimumGuard全部1，再恢复固定标量做比较。500–1500Hz guard-unity R/L最大变化分别0.3324/1.1157/1.4899dB。说明局部左右变化主要来自残响干涉，不是限幅器造成；限幅对该频带局部比值差的最大额外贡献为Very0.02745/Lose0.12504dB。
- 这不是“负面听感已证实”，但否定了“只因整段响度差小就能保证平衡不变”。残响-12候选暂不升级默认；无已证实目标改善，不能用候选通过回放替代听感验收。

进一步保存相同dense/standard图、当前增益的cue-off How回放，minimumGuard1。验证 `minus12 = cueOff + (baselineMinus6-cueOff)*10^(-6/20)` 最大误差2.43207e-7，确认有效候选确实只缩放附加残响，不是换掉原直达滤波。55–63s cue-off相对现有的500–1500Hz功率差仅-0.09717dB，1500–4000Hz为-0.00656dB；不证明去残响会恢复目标内容，也不作新的无依据放大。

### 旧证据边界更正

发现旧 `diag-how-love-gain-path.py`、`diag-how-love-cue-source-transfer.py` 也生成/使用平铺manifest。其旧cue-only对照混入失去标准fallback的改变，因此旧 `source-cue-transfer.json` 与基于旧solo的 `cue-coherent-mix-check.json` 不能再用于cue-only归因。输入增益账本/重编号/原HRTF默认基线对照不因此全部失效。

已修复脚本：新增诊断helper `diag-hrtf-manifest-variant.py`，保持fallback目录结构、清除旧诊断环境，元数据要求fallback验证标识和manifest hash，拒绝旧报告。没有覆盖旧输出或假称重跑旧整套solo。本轮新的graph-preserving cue-off/线性残响验证为有效独立证据。

输出：
- `C:/Users/legendshop/Downloads/SDA-clarity-validation/cue-minus12-valid-candidate/local-regression.json/.png`（图像已实际查看）。
- `C:/Users/legendshop/Downloads/SDA-clarity-validation/cue-limiter-attribution/report.json`。
- `C:/Users/legendshop/Downloads/SDA-clarity-validation/cue-bypass-valid/linear-residual-validation.json`。
