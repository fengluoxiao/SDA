# 独立核心参考条件与短窗映射

上一轮逐对象交叉能量定位属于进展。本轮检验“固定映射残差只是时变映射”的假设，并修正参考解码条件；未修改默认、推送或打包。

脚本：`scripts/diag-subarashiki-core-mapping-stability.py [--drc-off]`，19s全局训练/最后19s验证留2s空隙；局部250ms窗内前90ms拟合，隔10ms后验证150ms。不是交错相邻样本假称独立验证。局部邻近数据仍依赖，不能证明跨曲泛化或语义主唱分离。

固定映射到FL/FR/C/SL/SR residual dB约-17.81/-18.10/-40.14/-9.79/-9.75；局部映射约-16.47/-17.78/-43.32/-10.75/-6.75。局部FL/FR没有改善，SR更差，不能按预期假设将固定残差归为正常时变系数。也不能因拟合失败判定JOC丢音；任意短窗gain模型可能不覆盖真正解码重建结构。

发现此前FFmpeg原始命令未显式关DRC。查询本地FFmpeg7.1 decoder=eac3帮助确认drc_scale默认1，而SDA不应用DRC。重新解码源EC3前182秒，显式`-drc_scale 0`后以同一独立LFE时间对齐窗口检查：6channel最大采样差0.01676，相对差能量-48.16dB，整窗功率差-0.00172dB；stereo最大差0.02293，-47.84dB，整窗功率差-0.00187dB。显式off重复解码逐样本一致，因此不是随机noise变化；但小整体差异不足以认定明显主唱改善来自DRC。

用DRC-off核心再计算映射，FL/FR/SL/SR结论相同，C固定残差改善到-44.28dB。后续独立比较明确使用DRCoff参考。不新增DRC/压伴奏/EQ来匹配参考。

当前有效下一动作：独立JOC重建实现/规范矩阵对同一窗口核对，从PCM内容和OAMD绑定证明或排除解码偏差；不能把core简单下混注入空间输出，也不能把拟合系数用作歌曲/对象选择器。

报告目录：`C:/Users/legendshop/Downloads/SDA-clarity-validation/subarashiki-front/core-downmix`，`mapping-stability.json`、`mapping-stability-drc-off.json`、`drc-reference-condition.json`。参考PCM及日志保留为测试产物，不进入用户播放缓存。
