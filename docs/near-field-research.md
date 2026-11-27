# 近场、对象遮挡与空间平衡联合调整

日期：2026-11-27。适用于共享 Rust 渲染器，供 iOS、Android、Windows 和 macOS 使用。

## 依据与适用范围

- Duda & Martens (1998), *Range dependence of the response of a spherical head model*, [doi:10.1121/1.423886](https://doi.org/10.1121/1.423886)。刚性球响应随方向和距离变化，近场低频双耳声级差值得保留；除传播时延外，球模型响应接近最小相位。
- Brungart & Rabinowitz (1999), *Auditory localization of nearby sources. Head-related transfer functions*, [doi:10.1121/1.427180](https://doi.org/10.1121/1.427180)。研究范围为头中心距离 0.12–1 m；侧向声源靠近会显著改变低频 ILD，ITD 随距离变化较小。
- *A psychophysical evaluation of near-field head-related transfer functions synthesized using a distance variation function*, [doi:10.1121/1.3081395](https://doi.org/10.1121/1.3081395)。刚性球 DVF 比单纯调音量改善部分近场距离判断，侧方、40 cm 内改善更明显；去掉强度线索后，绝对距离判断依然有限。
- Yu, Wu, Liu & Xie (2018), *Near-field head-related transfer-function measurement and database of human subjects*, [doi:10.1121/1.5027019](https://doi.org/10.1121/1.5027019)。华南理工的 56 人数据库含 0.2–1 m 七个距离、每个距离 685 个方向。这是人体实测研究，不是本次生成数据的来源。目前未取得可再分发的数据和授权，没有把论文或模拟结果冒充该数据库。
- [Steam Audio: occlusion and transmission](https://valvesoftware.github.io/steam-audio/doc/capi/guide.html#occlusion-and-transmission)：几何遮挡、透射需要真实障碍物及材质信息；声音对象的位置不是障碍物网格。

## 联合处理

普通音乐播放和缺少明确效果声明的 authored scene 不再从发声对象的前后重合关系推断遮挡。HRTF 自带的头部声影保持原样，空间平衡的频谱掩蔽检测保持原样。已有文件显式声明 `source-bearing-8deg-v1` 时仍可重现旧艺术效果；新导出的默认快照不再声明开启该效果。

近场替换原来的两耳自由场几何增益加共同低频抬升，使用硬球点源解的零频率极限。耳位置采用半径 a=0.0875 m 的球模型，r 是头中心到声源的米数，mu 是声源方向与耳轴的夹角余弦。球面相对头中心自由场声压为：

`S(q,mu) = sum[n=0..infinity] (2n+1)/(n+1) * q^n * P_n(mu), q=a/r`

生产实现使用该级数生成函数的闭式积分解；分别取 `S(a/r,mu)/S(a/1m,mu)`，再给两耳施加同一个均方归一化系数。它保留模型双耳比值，没有全局低频增益；既有真实米数的距离强度路径继续负责整体响度。对称轴上不凭空制造双耳差异或共同温暖感。

采用 48 kHz、约 700 Hz 一阶低频棚形、20 ms 系数平滑。低通改用双线性形式，在 Nyquist 处有零点，修复旧指数低通的高频直通残留；高频逐步回到原 HRTF，不增加另一套传播/方向延时。这是**刚性球低频 DVF 近似**，不是完整全频 DVF、人体实测近场 HRTF，也不是已经验证的个人 HRTF。700 Hz、20 ms、1 m 参考点和两耳均方归一化是工程选型；论文没有规定这些数值适用于所有耳机或音乐。

最小模拟距离改为 12 cm，覆盖上述研究中的更近距离；更近输入按 12 cm 限制，避免点源逼近球面导致发散。1 m 及之外精确旁路。原测量 HRTF 不一定以 1 m 为实测参考，因此仍不能宣称绝对距离重建准确。

显式有效 `distance_m` 优先于手动坐标比例，快/常规混音器采用同一规则。反射声距离也统一使用真实米数；修复原先以坐标单位或 `distance_m/metres_per_unit` 决定反射量、导致调整手动比例改变同一物理距离的错误。反射比例 `0.25+0.75*clamp(r,0,1)` 是保留的有界工程政策，不是实测房间声学定律。

没有真实距离的普通 Atmos/ADM 仍只具有方向，只有用户明确启用手动映射才推断渲染距离。固定声床、360RA 底部扬声器和 LFE 不因坐标低或贴近原点就套用对象近场。

空间平衡仍按源域原始参考判断相对缺口，输出余量继续在实际混合、近场、节目增益和格式补偿之后测量。耳机音量滑块或峰值保护造成的衰减不能作为新增增益预算；不引入新的公共响度压低或常驻音频缓存。

## 可重复验证

`scripts/test/generate-near-field-reference.py` 采用 SciPy 的球贝塞尔/汉克尔函数求完整有限频率点源解，独立于生产闭式零频解，生成 20 Hz、72 个距离/方向的参考对。48/64 阶收敛后才保存。文件是数学模型计算值，不含第三方人体测量数据；重新生成需要 NumPy/SciPy，运行原生回归不需要 Python。

原生测试覆盖独立参考误差、左右镜像、头追变换、12 cm 至 1 m 连续性、双耳均方增益、高频保留、平滑和静音；同时覆盖普通音乐重叠对象、显式旧效果的兼容、实际 Engine 两种混音器/两种布局、真实距离优先级、手动比例不变性，以及空间平衡、输出余量、峰值撤回与 seek 重置。

模型和 PCM 测试不能替代实体耳机的双盲定位/距离实验。本次不宣称已经对每位听者完成感知验证。

### 本轮结果

- Windows GNU 原生共享核心：`cargo test --manifest-path apps/native-renderer/Cargo.toml --no-default-features --locked --lib near_field -- --nocapture --test-threads=1`，18 项通过。
- 同一命令筛选 `occlusion`：10 项通过；筛选 `spatial_headroom_tests`：3 项通过。共 31 项不同的相关原生测试通过，没有运行解码器、扫描或封面等无关完整测试集。
- `--features caller-participating-dsp --lib near_field_scene_tests`：4 项通过，补充主调线程参与的原生调度路径；这仍是 Windows 主机验证，不是 iOS 真机运行。
- 独立完整球面波解的 72 对低频参考：最大幅度误差 0.00040086 dB；该数字仅针对 20 Hz 参考与零频极限，不是全频误差或人耳定位精度。
- 16,206 个球面位置检查通过；两布局共 8 组快/常规混音与比例/回调对照通过，包含六个对象、三个固定高度/底部声道及 LFE。8 组近场位置与音量平衡状态组合保留实测余量增强、峰值否决和 seek 重置。
- 实际 `exportRenderMetadata` 打包执行验证：新快照遮挡默认关闭，显式旧效果保留，12 声道及原节目补偿值保留。Rust 格式与 Git 空白检查通过。

没有进行本轮四端安装包打包或实体 iPhone/Android/macOS 听测。
