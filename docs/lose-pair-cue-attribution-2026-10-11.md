# Lose失败并非单一对象可修

运行 `scripts/diag-lose-pair-cue-attribution.py`，保留完整元数据和起点历史，将PCM拆成诊断相关pair17/19与rest。两种cue配置各两次实际引擎回放通过，minimumGuard1，合成对完整安全回放最大差分别1.3784e-7、1.2107e-7。不是人声分离，编号没有加入生产逻辑。

结果：
- 126.6s500–1500Hz，pair自身能量变化/full baseline左-12.881%、右-2.682%，rest和pair/rest交叉变化很小，主要变化落在pair内。
- 119.5s同频带，pair左/右变化约-0.609%/-0.566%，rest左-10.710%、右-1.930%，失败主要落在其他混音。
- 127.6s1500–4000Hz，pair左-3.754%/右+0.654%，同时pair/rest交叉项右+1.396%。

说明失败位置不是总由同一对对象造成，不能将pair识别人声后单独调低右源、冻结该对残响，或仅按运动选择作为完整修复。作者PCM与空间元数据保留；下步需联合检查各路与总混合，不以单对象能量代替听感。没有新候选或默认改动。

报告：`C:/Users/legendshop/Downloads/SDA-clarity-validation/lose-pair-attribution/report.json`。
