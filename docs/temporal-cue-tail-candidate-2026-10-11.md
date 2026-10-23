# 保留早期附加反射、渐弱拖尾候选

用户要求解决后实际新增test-only `SDA_DIAGNOSTIC_CUE_TAIL_MS`。默认不读该变量（release cfg不包含候选）；保留已缓存bundled profile，再克隆施加时域包络。原直达数组、残响前20ms不变，之后smoothstep到80ms残响幅度0.501187。不改source gain/位置、空间层参数、历史HRIR、额外延迟或用户房间。

不是论文直接推荐20ms、不是听觉阈值；实验逻辑是避免像全局-12那样削最早反射支撑。原profile绝大多数残响能量在20ms内，故候选改动较小，不能以兼容性推断清晰改善。

新增测试通过：两个layout原直达/前20ms逐样本不变，尾部能量不增加、支持不扩大。完整当前native tests266pass0fail28ignored；三首ignored fixture replay显式另外执行通过。候选关闭后How baseline max误差<1e-6。没有移动端实机构建、推送或默认迁移。

`diag-tail-taper-replay.py`执行相同20ms策略：How完整64s、Very66s、Lose139s参数（fixture138.016s）。`diag-candidate-regression-gate.py --tag cue-tail20`三首四频带均通过0.25dB工程风险检查，不是听感验收。

500–1500Hz100ms最大ILD变化：How55–63约0.032dB、Very50–65约0.01984dB、Lose115–251约0.08799dB。候选heatmap已生成并实际查看Lose，变化带较拒绝的全局-12小。仍需要How目标清晰与回归听感确认。

PCM24输出有限/回读/4xpeak检查通过，响度匹配只用于离线试听：
- How-Love-baseline-then-tail20.wav：0–8baseline，9–17candidate，原曲55–63。
- Very-First-Night-baseline-then-tail20.wav：0–15baseline，16–31candidate，原曲50–65。
- Lose-Yourself-baseline-then-tail20.wav：0–16baseline，17–33candidate，原曲115–131。

路径：`C:/Users/legendshop/Downloads/SDA-clarity-validation/tail-taper/audition.json`、`all-tests.log`、`control.json`、`regression-gate/cue-tail20.json`、`cue-tail20-heatmaps`。

状态：新可试听候选，无已证实清晰改善，不标完成。

## 用户验收与共享正式路径接入

用户对上述三首试听反馈「都可以了」，记录为本次How目标及三首回归听感接受。不是所有歌曲/全部设备的普遍保证。

已将同一20–80ms包络迁入共享`spatial_cues::profile`缓存构建，在后方残响平衡及既有频谱整形之后只执行一次；移除test-only环境覆盖，避免重复taper。仅bundled Dolby7.1.4/Sony360RA13 profile经过此函数，用户房间不经过，原direct数组/前20ms仍受测试保护。

实际从更新后的源码重新编译全集成测试：266passed、0failed、28ignored；三首实际fixture replay另行显式通过。`diag-accepted-tail-production-parity.py`清除全部诊断变量，在共享默认路径回放并对照已验收`cue-tail20`：

| 比较 | 最大采样差 | 最大100ms能量差dB |
|---|---:|---:|
| How55–63 | 2.0862e-7 | 2.0605e-7 |
| Very0–65 | 4.1723e-7 | 1.4756e-6 |
| Lose115–251 | 0.00010908 | 0.00018223 |

Lose仍存在浮点归约/联动峰值包络微差，不能宣称逐样本相同；整体差能量-109.03dB。全部4x过采样峰值<0dBFS。正式路径三首四频带风险检查通过，脚本依然不会自动批准默认（听感接受由用户提供，不由脚本推断）。

目标完成证据：用户三首听感通过、默认共享路径与验收输出数值一致范围、原生全集成测试通过；无歌曲名/对象ID/时间窗生产规则，无播放前扫描/分析缓存或新增用户档位。未推送/打包，已安装IPA/APK尚不含本次源码更新。

报告：`tail-taper/production-parity.json`、`tail-taper/production-tests.log`、`regression-gate/accepted-tail-production.json`，均在上述Downloads诊断根下。
