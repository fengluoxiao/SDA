# 独立重建数值对照及 trim 语义边界

本轮实际执行本地OpenJOC0.16.0参考工具，并非假称已有参考。源EC3按syncframe边界提取前181秒，5657frame，17378304byte；原完整文件与已有原生PCM未改。

1. 首次调用`--drc disabled`同时`--internal-base-policy codec-core`被参数规则拒绝；不是音频解码错误。
2. codec-core/observed-vendor-compat无覆盖参数拒绝OAMD，truncated5bits0remaining。不能据此判文件损坏/SDA丢失vocal。
3. diagnose-oamd目标4375..5625导出1251观测，OAMDpayload560bits，trim元素32bits，全部相同bitstring`00000101000011001111000111111100`。
4. 单独`--trim-config-count 8`诊断覆盖可使流式PCM导出成功。必须记录这不是规范认证或生产修复；参考summary仍semantic_binding_state=unresolved/authored_object_pcm_admissible=false。
5. 比较15路ordinal重建PCM，原曲140–180秒，对应原生sources.pcm：各路lag0，最低相关0.9999956373，最大能量差约0.00003569dB。Obj10未缩放残差约-53.03dB，Obj11约-50.59dB。没有数值证据支持显著重建音频丢失作为主要发闷原因；不扩大为所有编解码/所有素材正确或真正独立实现认证。

## 必须保留的歧义

raw oracle将固定bit532命名为per-object disable presence，但在global mode2中，此前需要读取逐配置标志。这个固定命名不可当语义证据。

后续已纠正本地truehd旧schema的presence位顺序：ETSI TS 103 420 V1.2.1表34及OpenJOC的trim.rs均为16=center、8=surround、4=height、2=top/bottom、1=listener。32bit元素含1个discard位，然后才是6个控制位。旧手工解析得出的“9成功/8失败”和“surround7/height8”撤销，不作为后续证据。

正确有界解析中，8配置消费31bit，per-object disable presence=0，剩1bit零padding；9配置越界。配置1的presence=6，height code7按表37为−4.5dB，top/bottom sign1/amount1为+0.125，其余配置默认。这个编码含义与独立工具输出吻合，但配置1究竟对应哪个扬声器布局、是否适用于双耳输出，仍未确认。不能把8当通用标准，不能把高度−4.5dB应用于耳机来冒充主唱修复，也不能给SDA默认应用trim override。规范第5.6.5节解释字段，但当前查阅部分未给出配置索引到输出布局的映射。

新增可复现诊断脚本`scripts/diag-oamd-bounded-trim.py`，带各presence位、截断、padding和8/9边界自测，仅离线审核语法，不参与播放器、不生成用户播放缓存、不自动选择配置。

有效结论局限于PCM重建数值高度吻合；对象与trim语义仍不能认证。没有修改生产解码器、声源、gain、相位或方向，没有推送或打包。

证据目录 `C:/Users/legendshop/Downloads/SDA-clarity-validation/subarashiki-front`：
- `openjoc-reference.log`、`openjoc-trim8-diagnostic.log`、`openjoc-trim8-diagnostic/debug/streaming_summary.json`
- `oamd-target-independent.json`、`trim-element-bounded-bits.json`、`trim-element-parse-hypothesis.json`
- `independent-reconstruction-parity.json`
- `trim-presence-order-recheck.json`、`trim-cardinality-sweep.json`（新解释覆盖旧hypothesis）

目标未完成。重建吻合不能替代主唱/后方/移动听感验收，不把参考覆盖参数转成生产解决方案。

## 后续：目标区间对象元数据对照

用原EC3按syncframe边界生成64帧诊断片段，覆盖AU4375..5625（原曲140–180秒），通过上述显式trim8覆盖解码参考元数据，再与完整原生历史的replay/frames.jsonl对照。脚本为`scripts/diag-oamd-reference-position.py`，严格要求本fixture每帧16个OAMD对象、15个动态路、一个更新块，未满足维度则失败，不把空比较当通过。

- 1251帧、18765次动态对象坐标比较，完整覆盖；最大坐标误差9.2291063e-8。
- 每次增益、ramp duration、事件sample位置（含原生QMF577sample延迟）均吻合。
- 另检查起始64帧：960次坐标比较、最大误差8.6522871e-9，增益/ramp/timing吻合。
- 目标区间先取AU4375、4800、5400三个64帧探针检查控制字段：所有channel_lock=false、六个zone均Include、extent全0、warp=None。此控制清单仅覆盖三处探针，不外推全曲。
- 限制：参考采用片段重置状态及专家参数，不认证完整连续参考状态、原作者声源身份或官方双耳渲染。但本范围没有“前后坐标翻错/位置偏移/主唱增益漏读/事件延迟错位”的明显差异证据。

报告：`oamd-full-target-parity-summary.json`、`oamd-window-parity-*.json`、`oamd-position-start-parity.json`、`oamd-target-trim8-independent.json`。三处探针的报告为`oamd-position-target-{4375,4800,5400}.json`。

重新实际查看已有group-cross-heatmap.png：约145–165秒和169–180秒两耳2–5kHz有多处负交叉能量，但不等于漏失独立vocal；原输入的部分对象对已经反相关。结合已有各对象solo和cue on/off对照，下一条调查线应为多对象经过不同方向滤波后的相干叠加/可闻掩蔽，而不是把所有蓝色格子强行补增益或翻相。当前未将任何相位/衰减/修整策略写入正式播放。
