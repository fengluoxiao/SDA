# Direct Object HRTF Experiment

The desktop setting `逐对象 HRTF（实验）` selects independent object
convolution using the selected virtual speaker layout. It defaults
to off and persists under `sda-direct-object-hrtf`. The native command is
`setObjectHrtf` with a boolean `enabled`; a running sidecar must acknowledge the
change before the UI saves it. Session reset preserves the selected mode.

Both modes use the selected subject, wet weight, decoded object PCM, object
scalar gain, headphone compensation, EQ, program gain, and output guard. Beds
remain on speaker buses and LFE retains its separate path. Object mute, Solo,
scheduled metadata, and head-relative orientation remain available.

Each object uses a 128-sample partitioned stereo convolver. The selected layout's
VBAP weights (including object spread and head pose) combine the virtual speaker
HRTFs into that object's filter. The sum retains the actual speaker amplitudes;
it is not normalized to unity. Thus objects retain independent convolution
histories while layout selection changes their sound as well as the beds.
For static routing, this is equivalent to summing the speaker-filter outputs.
This is not Dolby's proprietary renderer. The existing Web high-resolution option does not
select a different native dataset.

Mode changes fade object excitation between paths over 200 ms and continue
draining both convolution tails. Direction changes crossfade filter outputs over
one render block using the same input history; retargeting preserves the current blend.
Spatial metadata retains its codec sample timing and signaled route-ramp duration.
The engine interpolates Cartesian object position and spread over that duration,
then re-pans the trajectory at render-block boundaries. Speaker gains bridge
only the next trajectory segment, rather than crossfading the two endpoint
speaker vectors for the entire move. The direct filter follows these speaker
weights at each 128-sample boundary, rather than jumping to the position target
with a fixed 32 ms ramp.
This retains block-resolution motion, not sample-exact equivalence to the bus
renderer for moving objects. A reset clears audio history. The
native health event includes `directObjectHrtf` and `objectConvolverCount`.

This experiment does not include the earlier inverse-distance cancellation or
sparse-JOC pedestal subtraction. FFmpeg compatibility stereo was reported as
normal, while the previous B and C experiments still sounded abnormal. The new
switch provides another listening comparison; it does not establish that the
reported modulation has been fixed.

The original experiment bypassed speaker layouts for objects. That behavior was
replaced so both settings honor the same selected virtual room layout. Regression
coverage compares independent-object impulse output against the speaker bus sum
in 5.1.2 and 9.1.6, and verifies that changing layouts changes the output.

Motion regression coverage schedules a left-to-right move with a 500 ms ramp
and checks early, middle, and late ear-energy balance in both modes. Separate
synthetic-signal tests exercise Obj10 through Obj24 individually, covering the
IDs in the current Egaku capture. These tests establish signal routing, not
perceptual audibility of every original stem in a dense mix. Authored silence,
metadata gain, user mute/Solo, and the selected layout's spatial resolution
still apply; quiet objects are not automatically boosted.

A captured Obj14 path from front-left `[-1, 1, 0]` to rear-left `[-1, -1, 0]`
over 1536 samples is covered with the carrier's 577-sample event offset. Its
midpoint must route through `[-1, 0, 0]`; averaging the endpoint speaker gains
fails that check. Late events advance the coordinate trajectory to the current
codec time before rebuilding their route. This fixes a geometric rendering
error, but a 32 ms move followed by a long stationary interval is still a brief
move, not a continuous orbit; the renderer does not invent motion absent from
the decoded metadata.
# 输出音箱监听控制

“声道”浮层列出当前布局的输出音箱，而不是解码器的输入声床标签。
例如输入只有 LFE 声床加 JOC 对象，7.1.4 输出仍显示 12 路音箱。
支持多选 Mute/Solo，Mute 优先；对象面板的源静音继续独立叠加。
切换布局后，只用新布局中存在的 Solo 音箱计算独奏集合。

原生 WASAPI 的 `setSpeakerMutes` 按音箱名称控制路由贡献：总线路径在
音箱卷积前应用 2048 采样平滑电平，逐对象 HRTF 路径将同样的电平乘入
音箱滤波器组合权重。LFE 路径独立应用同样的平滑控制。保留卷积尾声，
不重新归一化剩余音箱，不改对象的坐标、元数据或源静音状态。
该接口需要新版 Electron preload、主进程和原生程序，更新后须重启 Electron。

点击 3D 音箱可加入多选聚焦，再次点击同一个音箱取消它的选择。所有聚焦音箱
保持原路由增益，未选中的音箱（含 LFE）降低 12 dB，并在 3D 中降低到 22%
不透明度。取消最后一个选择或点击面板的“取消聚焦”恢复正常。超过 4 像素的
拖动不触发聚焦。聚焦与声道 Mute/Solo 互斥：
聚焦时禁用声道 Mute/Solo；当前布局有声道 Mute/Solo 时禁用音箱点击聚焦。
必须先手动取消当前状态才能操作另一组，受限点击不清空选择、不切换模式。
播放器和原生命令入口仍防止混合状态同时下发。对象面板的源静音独立保留。
静音音箱在空间视图中隐藏，独奏时仅显示未被手动静音的独奏音箱；面板保留
全部音箱以便恢复。中文位置名、缩写和角度标明当前布局，9.1 布局在面板顶部
提供左前宽 Lw / 右前宽 Rw 的成对控制。静音前宽不等同于切换 7.1：后者会
重新分配对象路由，固定布局静音只移除这两路贡献。
聚焦音箱不存在于当前布局时忽略聚焦；最终总输出仍经过原有峰值保护。

`speaker_monitor_filters_bed_and_object_contributions_in_both_modes` 在
5.1.2、7.1.4、9.1.6 中测试两种对象模式，将音箱 Solo 的稳态输出与仅保留
对应声源路由贡献的参考输出逐采样比较，允许最大绝对误差 `2e-6`。
同一测试也覆盖聚焦，以未选中路由乘以 `10^(-12/20)` 的独立参考验证电平。
