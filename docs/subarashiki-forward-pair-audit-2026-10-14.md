# 前方与其余内容逐对象交叉项定位

上一轮时钟检查是进展；本轮完成5个前方诊断solo及10个其他对象solo的实际full-prefix渲染，全部unityguard，原PCM/元数据/位置不改，反射关闭且匹配standardfallback保留。未写生产规则、未推送或打包。

可复现前方solo脚本：`scripts/diag-subarashiki-forward-pairs.py`。solo输出保存在Downloads `SDA-clarity-validation/subarashiki-front/forward-pair-audit`，每路f32/json/log。scratch sources.pcm明确只保留最后solo，原fullfixture未改。

5前方solo相加对既有前方group最大误差<1e-6。单个中央对象并非负交叉项主要贡献者：
- 700–1500Hz前左16/前右20输入相关0.602，双耳输出相关-0.299；该对交叉项/full混合功率-0.11645。前方组内部整体coherent/independent仍+0.1578dB，不能把一对反号扩大为整组全被抵消。
- 4–8kHz前方16与侧方12交叉项/full功率-0.11979；前方20与侧方23为-0.11023。这两对输入本已负相关（-0.14358/-0.11304），双耳输出负相关增强（-0.32799/-0.29698）。
- 1.5–4kHz主要前方与其他对象交叉项来自16/21、20/11、20/17等，不能冻结/削弱“某一个后方对象”覆盖全部情况。

这些是实际混合数学归因，不是独立vocal测量或错误判定。由于素材已有负相关，不能反相某源或默认自动去相关作为修复，也不能削后方来讨好前方。下一步需核对这些原有相关内容在普通核心下混/空间呈现中的不同系数与相位是否存在可复现实现缺陷，保留作者方向和移动线索。仅知道负交叉项不够支持新默认。

证据：`forward-pair-audit/report.json`、`front-other-pairs.json`。目标保持未完成。
