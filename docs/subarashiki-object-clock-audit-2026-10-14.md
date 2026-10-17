# 主唱目标续查：对象路径时钟一致性

上一轮整组与反射旁路回放属于进展。本轮实际检查当前离线引擎，未修改生产参数、推送或打包。

可复现入口：`scripts/diag-object-clock-consistency.py --harness <当前test executable>`。

在稳定初始化之后提交同一已知脉冲，依次通过正前方、前左45°、前右45°、后上45°的真实单对象路径；附加反射关闭，原直达HRTF保留。导出同一Grid的滤波器，以互相关查延迟并拟合共同标量，不用原始HRIR峰值猜测路径延迟。

- 四个方向、左右两耳的额外pipeline delay全部2288sample。
- 拟合脉冲响应残差相对能量约-129.8到-140dB。
- 输出保护gain1，已知脉冲0.01乘现有EAC3presentation+2dB得到拟合标量约0.0125893；不存在单独前方电平或相位补丁。

结论：这些静态方向的单对象实际链路未复现额外延迟差；不能任意将不同方向测量的物理ITD/频率相位归零。检查不覆盖整个移动调度、多个对象间作者相位、手机硬件输出或独立主唱听感，不能扩大为所有路径正确。

同时核对片段140–180s的相同输入分组等权单声道和：1.5–4k/4–8k输入coherent-vs-independent分别约-0.0603/-0.0644dB，空间输出约-0.3463/-1.0676dB。等权源和不是作者下混、输出rest含LFE，不能把两者差值直接作为应补的EQ/相位。只能说明负交叉项与空间合成有关，仍需进一步复现能区分合法HRTF效果与错误实现的故障。

输出：
- `C:/Users/legendshop/Downloads/SDA-clarity-validation/subarashiki-front/object-clock-audit/report.json`
- `C:/Users/legendshop/Downloads/SDA-clarity-validation/subarashiki-front/forward-group-audit/input-output-cross-review.json`

目标未完成，没有新的已证实改善候选；保持原声源、位置、后方/高度和头追，不通过盲目相位操作消除空间线索。
