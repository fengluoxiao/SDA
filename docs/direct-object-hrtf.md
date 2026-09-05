# Direct Object HRTF Experiment

The desktop setting `逐对象 HRTF（实验）` selects independent object
convolution instead of object routing through virtual speaker buses. It defaults
to off and persists under `sda-direct-object-hrtf`. The native command is
`setObjectHrtf` with a boolean `enabled`; a running sidecar must acknowledge the
change before the UI saves it. Session reset preserves the selected mode.

Both modes use the selected subject, wet weight, decoded object PCM, object
scalar gain, headphone compensation, EQ, program gain, and output guard. Beds
remain on speaker buses and LFE retains its separate path. Object mute, Solo,
scheduled metadata, and head-relative orientation remain available.

Each object uses a 128-sample partitioned stereo convolver. Direction selection
uses the closest measured direction in the existing selected dataset. Spread
blends the closest three measured filters with unity total weight. This is an
experimental local spread approximation, not Dolby's proprietary renderer or
continuous HRTF interpolation. The existing Web high-resolution option does not
select a different native dataset.

Mode changes fade object excitation between paths over 200 ms and continue
draining both convolution tails. Direction changes crossfade filter outputs over
32 ms using the same input history; retargeting preserves the current blend.
Spatial metadata retains its codec sample timing; the direct filter selection
is updated on the next 128-sample boundary. A reset clears audio history. The
native health event includes `directObjectHrtf` and `objectConvolverCount`.

This experiment does not include the earlier inverse-distance cancellation or
sparse-JOC pedestal subtraction. FFmpeg compatibility stereo was reported as
normal, while the previous B and C experiments still sounded abnormal. The new
switch provides another listening comparison; it does not establish that the
reported modulation has been fixed.
