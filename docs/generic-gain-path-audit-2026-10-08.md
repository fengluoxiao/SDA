# 通用增益与空间线索链路检查（2026-10-08）

## 目标与新增参考

用户要求修复共享链路，而非为某首歌或对象设置默认增益。用户确认 iOS「文件」App 录屏是普通立体声下混，不是 SDA 录屏或 Apple 耳机空间输出。此前诊断 C（整体 +2 dB、Obj12–24 额外 +6 dB）被反馈为最接近；这是整组混音偏好参考，不是独立人声测量或 Apple 算法证据。

本轮没有改生产 DSP、资产、默认策略，没有提交、推送或构建移动端应用。只增加 test-only 增益记录及离线诊断脚本，保留原有工作区修改。

## 实际执行

- 从当前源码编译 GNU Windows release native renderer lib test harness，禁用输出设备 features。新增增益账本只在 `SDA_ATMOS_REPLAY_LEDGER=1` 时输出。
- 使用完整原 PCM、元数据、移动端 HRTF、原声源增益进行三次完整回放：默认、只关闭附加 spatialCues、只把所有对象 ID 加 100。
- 再进行 15 路关闭附加空间线索的独立对象回放，布局及元数据完整保留，只将其他 PCM 槽位置零；与此前原位置独立回放对照。
- 回放覆盖原曲约 53.984–64 秒，统计窗口为 55–63 秒。增益账本为每块结束状态，不冒充逐样本记录或实机捕获。
- 本轮 lib tests：**246 passed、0 failed、25 ignored**。3 次完整与 15 次独立回放均成功；ignored fixture test 在上述回放中显式执行。

## 结果

### 1. 没有在本次离线配置中发现额外标量衰减

默认回放每块的 outputGain、programGain、comparisonGain、monitorGain、guardGain 均为 1。
各声源 gain、targetGain、availability、distanceGain 均为 1。704 条原始元数据事件的 gainDb 均为 0，无显式距离、无非零 extent。

代码核查：`VolumeBalance::route` 对此非立体声 E-AC-3 对象流判定为不 eligible；现有 `mobile_balance_routes_cached_master_gain_and_bypasses_object_programs` 测试覆盖相应策略。此次离线引擎 programEnabled=false，与这首歌正常移动端的旁路策略相符，但不能证明用户手机保存的音量/其他设置或安装版本相同。

共享输出音量曲线为 `clamp(volume, 0, 1)^2`，本次设定为 1 时无衰减。没有把曲线改成线性，也没有默认加 +2 dB。

### 2. 当前回放和已有试听基线一致

当前完整默认回放与此前 source-interaction/full 最大逐样本差为 **2.38419e-7**。
对象全部重编号后最大差为 **2.38419e-7**，差异相对能量约 **-134.07 dB**。本案例未见对象编号影响电平；这不是覆盖所有编号/布局的形式证明。

### 3. 未找到附加空间线索造成约 6 dB 缺失的证据

这里只关闭附加空间线索做诊断，测量 HRIR、插值、原位置和输入相对增益保留，生产配置未动。

关闭线索后完整混音总 RMS 变化为 +0.205 dB。完整混音可能掩盖局部差异，所以又检查各对象：

| 频带 | 15 路对象各自整段能量变化范围 | 有效 40ms 窗变化范围 |
|---|---:|---:|
| 500–1500 Hz | -0.286 至 -0.001 dB | -0.921 至 +0.561 dB |
| 1500–4000 Hz | -0.295 至 +0.033 dB | -1.044 至 +0.269 dB |
| 4000–8000 Hz | -0.039 至 -0.001 dB | -0.093 至 +0.055 dB |

有效窗要求两种输出的频带功率均高于 -60 dBFS，避免静音比值误导。这些是对象宽频带统计，不是目标人声 mask、窄带分析或主观高度判断。关闭线索后的方向/高度变化没有听感验收，不能用总能量接近就认为线索可以移除。

**本轮结果不支持删除既有空间线索，也不支持套用整组 +6 dB 作为通用修复。没有证明目标问题已经解决。**

## 下一步的收敛方向

保留原始测量点、相对增益、空间线索与普通音量平衡。下一阶段优先区分：

1. 原生解码与当前诊断缓存（原 WASM 解码）的 PCM/元数据是否一致；不能仅凭共享代码名认定实际编译输出一致。
2. 沿用户认可的 core SL/SR 与交叉验证 JOC 重建参考追踪目标相关分量，不能把拟合系数或单一对象语义化为人声。
3. 分别检查原始测量响应、连续方向响应及对象合成中的窄带/短时差异；若怀疑方向相关缺陷，用等输入的合成方向扫测复现，不能按歌曲频谱反向设计补偿。
4. 在未建立可复现偏差之前，不默认提高全部上方、后方或低电平对象，不引入语音检测式重混。实际设备版本/设置差异仍待验证。

## 证据与可复现入口

- `scripts/diag-how-love-gain-path.py --harness <当前 test exe>`
- `scripts/diag-how-love-cue-source-transfer.py --harness <同一 test exe>`
- `tmp/how-love-diag/generic-gain-path-audit/report.json`：输入/二进制哈希、增益、元数据与完整对照。
- `tmp/how-love-diag/generic-gain-path-audit/default/render-gain-ledger.json`：各块各源状态。
- `tmp/how-love-diag/generic-gain-path-audit/source-cue-transfer.json`：逐对象/频带、有效 40ms 窗与原 core 重建系数（仅诊断拟合）。
- `tmp/how-love-diag/generic-gain-path-audit/native-tests.log`：当前原生测试日志。

`cues-disabled-solos/sources.pcm` 是循环复用的临时输入，最终仅保留最后一个对象的输入；各对象输出、配置及日志均单独保留。音频和本地 PCM 不应提交到仓库。
