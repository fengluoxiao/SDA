# 公开研究与实现核对

应用户要求实际网上搜索；读Google发现页、Acta Acustica原文、arXiv摘要、EBU BEAR仓库及direct_diffuse_split.cpp。未把搜索AI概览当作证据；Harvard ADS出现人工验证，未绕过。

## 已核实来源

1. Bischof, Aublin, Seeber (2023), *Fast processing models effects of reflections on binaural unmasking*, DOI10.1051/aacus/2023005。
   https://acta-acustica.edpsciences.org/articles/aacus/full_html/2023/01/aacus210053/aacus210053.html
   原文摘要、引言、模型、讨论和结论：低频harmonic complex tone在噪声中，前方目标加入早期反射可改善检测，侧向60deg未见同样收益；反射改变ILD/IPD，不能把所有反射当作有害。12ms短窗提取、再积分的模型比长窗直接平均更准确。模型使用分别提供的目标/遮蔽信号，不能以整首歌曲混音直接得到目标人声清晰度或充当生产分离器。

2. Remaggi, Jackson, Wang (2019), *Modeling the Comb Filter Effect and Interaural Coherence for Binaural Source Separation*。
   https://arxiv.org/abs/1910.02127
   阅读摘要：利用直达与首个早期反射信息、时频IPD和耳间相干建模以改善分离。这是双耳源分离研究，不是通用零代价EQ补丁；没有完整重现论文或声称实现。

3. EBU BEAR真实开源渲染器，Apache2.0，实时C++，基于EAR/libear。
   https://github.com/ebu/bear
   https://github.com/ebu/bear/blob/main/visr_bear/src/direct_diffuse_split.cpp
   已看源码：独立输出direct_gains_out/diffuse_gains_out，将两组路由增益分开。该文件本身不实现去相关，也不证明ADM diffuse支路等同于SDA附加残响。仓库README仍标pre-release，不能称即插即用已验证修复。

## 对当前工程的约束及下一步

- 不再统一削全部残响：用户How接受、Lose拒绝，且实际频谱显示不同耳的干涉变化。
- 不以整段响度/相关性代替目标清晰度；用短时频带ILD/IPD/相干作为风险筛选，并先保持已认可原基线。
- 可研究限定在附加残响支路的去相关/时序设计，必须先对合成相关输入和镜像源验证染色/左右比例，再对三首完整回放；直达HRTF、作者增益与位置不改。
- 去相关可能改变音色、空间感和定位；不会直接套入生产。BEAR源码和论文是候选设计依据，不是证明能解决How顶部内容。
- 当前只完成研究核对，没有安装BEAR、搬运代码、增加用户侧分析缓存、改默认或发布。

## 研究后的首轮合成筛选

已运行 `scripts/diag-residual-allpass-safety.py`：直达保持不变，只在附加残响左右耳使用相同一阶全通。测试中心/侧向/反射方向不同、5/17/43/75ms四种延迟。参数是合成压力用例，不是论文推荐值、三首实际测量或听觉阈值。

- bypass输出逐频点无变化。
- 两种全通均验证残响自身幅度和复耳间比保持在1e-12误差内。
- 但混合后的500–4000Hz最大ILD变化：a=.35约1.34dB，a=-.65约6.99dB；最大耳幅响应变化分别1.17/6.54dB。

这验证了“全通不改残响幅度/两耳同滤波，所以混音定位不会变”的推断是错的。只改变残响相位仍能改变与直达声的干涉。否决直接插入该全通作为保证平衡的修复；未修改DSP、未交新的失败试听。不能由这些合成极值断言所有去相关设计或实际三首都同样失败。

报告：`C:/Users/legendshop/Downloads/SDA-clarity-validation/residual-allpass-synthetic-safety.json`。
