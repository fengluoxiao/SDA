# 输出链及完整受试者HRTF隔离试听

目标仍未完成。本轮生成诊断试听，不是通用修复或发布候选；不得直接替换正式默认。

## 输出链检查

HEAD48771dd当前源码：Engine::new的HeadphoneCompensation为单位FIR旁路。iOS SdaPlayer初始化未调用加载耳机补偿命令；近场默认false，初始化照偏好发near命令。update_occlusion让所有源openness=[1,1]，音频对象不会被当成物理障碍。主渲染输出为合并后耳机FIR→binaural EQ→增强shelf→volume/program/master→linked guard。

这是源码路径检查，不是用户设备状态抓取；不能排除设备偏好、系统耳机处理或旧包差异。当前离线harness不加载耳机FIR/room/near；没有发现额外人声低通的证据。

## 隔离实验

原生harness完整回放181秒，源仍原15路JOC PCM+LFE，保留所有对象位置/增益/运动。统一用工程已有的完整H13人类受试者HRTF，不做前方混合、对象替换或普通下混注入；wet=0、spatialCues=false，空间增强保留+1.5dB总辅助上限与相同输出shelf；空间平衡与front_common关闭；presentation−12dB保证guard=1。

首轮将historicalInterpolation=true用于H13被NativeHrtfSet规则拒绝（KU100专用）。保留失败日志，没有修改loader白名单；使用H13受支持默认插值historicalInterpolation=false重跑成功。因此此对照同时改变完整受试者响应与插值，不可宣称只分离个人耳形变量。

使用`diag-hrtf-manifest-variant.py`保留hrtf-h13-dense及匹配hrtf-h13回退目录，绝对资产路径；只在诊断目录改processing，正式H13及KU100资产不变。H13原版manifest未声明spatialCues，显式关闭以和KU100 direct-only控制一致。

## 输出及质量检查

`scripts/diag-subarashiki-subject-response.py`检查输出8689152sample、minimumGuard=1、wet0、direct、无frontCommon；提取140–180秒，以离线LUFS匹配作试听控制，再统一安全gain（不是播放器归一化）。写PCM24/48kHz WAV，有限性、4倍重采样峰值<1及文件写回验证由共享write helper执行。

匹配后H13相对KU100能量：700–1500Hz−1.013dB、1.5–4kHz+.650dB、4–8kHz+1.370dB、8–16kHz+2.104dB。整段频带ILD变化最大约.227dB，不代表短时定位/后方高度通过。已实际查看subject-spectral-delta.png，两耳中高频普遍红色、频率细节不均匀，不是一次固定高架EQ即可等价。

数值不证明主唱清楚。试听用来判断完整空间滤波是否有听感差异，不能作为直接调音模板、不能针对这首选择H13。未经用户听感、连续运动、头追和三首回归，不合入正式。

证据/试听：`C:/Users/legendshop/Downloads/SDA-clarity-validation/subarashiki-front/subject-response-audit/`。A为KU100直达HRTF（不是带附加空间线索的正式版本），B为完整H13直达对照。连续文件0–40秒A、40–41秒静音、41–81秒B。render.log为初次失败，render-supported-profile.log为成功，report.json保存配置/试听写回验证及频带结果。正式渲染器默认、头追及已认可歌曲没有改动；未发布。

## 后续实际运动及三曲回放：不能直接合入

运行`diag-subject-motion-regression.py --songs`对两个完整受支持HRTF、相同原始fixture与metadata、cue-off进行实际原生回放，所有输出finite、guard=1：

- cross-front：12秒固定x=.5、y从负到正运动；4–8kHz两套subject间100ms ILD最大差11.416dB、P95为9.676dB；y≈0前后±.5秒最大差2.358dB。
- vertical：12秒固定x=.5/y=.8、z由负到正；4–8kHz最大差4.453dB、P95为4.320dB，z≈0邻域也达到4.453dB。
- How Do I Make You Love Me?：完整回放前64秒，比较55–63秒。4–8kHz最大ILD差1.942dB、P95 .979dB。
- The Very First Night：完整回放前66秒，比较0–65秒。4–8kHz最大ILD差2.071dB、P95 .734dB。
- Lose Yourself to Dance：完整回放前139秒。4–8kHz最大ILD差10.289dB、P95 2.931dB；另对115–139秒come on片段单独检查，两套都活动的100ms窗口最大差7.731dB（115.35秒）。不能声称覆盖用户原请求1:55–4:11整个范围。

`diag-subject-motion-curves.py`额外实际画出、查看`motion-and-lose-ild.png`，不是只报最大数值。前后曲线均连续但方位频带响应明显不同，左右总段平均近似相同不能证明定位保持。垂直曲线在中间段有约4dB偏差；Lose高频短时轨迹不同。

这些是两个不同人/假头HRTF的差别，不直接证明H13渲染算法错误，数值也不是主观阈值。但无法证明保留已认可的后方/高度及左右听感，不满足本目标的直接替换证据门槛。因此H13仍仅诊断，不更改正式默认、也不以它更亮而选为发布方案。

证据：`C:/Users/legendshop/Downloads/SDA-clarity-validation/subject-motion-regression/songs-report.json`、`motion-and-lose-curves.json`、五组原生回放log、`motion-and-lose-ild.png`。各原fixture新增subject-regression-ku100/h13输出，未覆盖已有基线。尚无用户试听反馈、头追设备验收或通用改善证据，目标继续。
