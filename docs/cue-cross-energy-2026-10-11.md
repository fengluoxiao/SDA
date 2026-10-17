# 残响失败的交叉能量归因

`scripts/diag-cue-cross-energy.py` 已运行。由同渲染图、同固定-12dB安全输入、保护gain1的两个输出求cue-free mixture D及baseline residual W。原Y=D+W，候选Z=D+gW，g=10^(-6/20)。因此P(g)=P(D)+g²P(W)+g*2Re(DW*)。这不是语义独立人声；D仍含全部HRTF、所有对象和LFE。

How现有graph-preserving独立cue-off输出对代数D的目标段验证通过（脚本门限1e-5）；频带能量差分恒等式误差<1e-15。实际查看Lose交叉能量图。

Lose126.6s的500–1500Hz：baseline交叉项/直达功率左0.24934、右0.03880；削残响引起的交叉项变化/baseline功率左-0.09483、右-0.01842，残响自身能量变化左-0.03552、右-0.00862。混合左-0.60654dB、右-0.11905dB，右相对左+0.48749dB。不能把原因简化成右耳残响过强后独立右耳削弱。

离线解析扫描额外残响0到-6dB、0.1dB间隔，以How55–63、Very0–65、Lose115–251的100ms500–1500/1500–4000短窗最大R/L变化作设计风险检查。设计预算0.1/0.25/0.5dB分别最多容纳额外约-0.2/-0.5/-1.2dB；用户How认可候选是额外-6dB。预算仅测试约束，非听觉阈值、非清晰度指标；扫描不能证明自适应方法都无解，也不能由接近原波形推断保留候选清晰收益。扫描静音格点出现divide警告，统计严格使用双耳active窗口，不将非活动NaN计入极值。

结论：停止继续用单个全局残响标量折中碰运气。后续设计需考虑附加支路与直达的联合响应/交叉项，不得仅限制残响自身能量。未加入实时分析、归一化或用户缓存，也未改生产默认。仍缺满足How改善及三首听感的候选，不标完成。

输出：`C:/Users/legendshop/Downloads/SDA-clarity-validation/cue-cross-energy/report.json`、`Lose-Yourself.png`、`scalar-risk-boundary.json`。
