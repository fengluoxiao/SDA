# Full-mix front/back investigation

User feedback after the Media3 change: single-source front/back probes are
distinguishable, but all channels playing together obscure front/back. This is
not evidence of missing rear KU100 assets or a radial distance mapping fault.
Keep the accepted Media3 APK and all requested spatial features enabled.

## Real-song checks

The opt-in `mix_parity_probe::real_song_all_enabled_mix_equals_independent_channels`
test replays decoded second-song PCM and object events with calibrated dense
KU100, per-object and actual-direction rendering, desktop studio room and
independent near-field enabled. It compares the parallel mixer with the general
mixer, then compares the full 16-channel mix with 16 independent channel renders
summed before one shared output peak guard.

Input: first 160 frames (5.12 seconds) of
`E:/SDA/tools/mygo-orbit-frames.jsonl`. This is a bounded offline fixture check,
not an end-to-end capture of the Windows player or Android Media3 output.

- Parallel versus general maximum sample error: 2.9802322e-7.
- Full mix versus independently rendered sum after the shared master guard:
  maximum sample error 3.5762787e-7. Test passed.
- Before accounting for the guard, maximum difference was 0.014854789, with
  full-mix peak 0.8912509 and independently summed peak 0.9061057. The guard's
  maximum measured attenuation was 0.1436 dB over approximately 0.208 seconds.
  This is the expected shared output protection, not missing object PCM.
- No active pairwise object occlusion was observed in this replay. A separate
  metadata/PCM activity scan of the existing opening and orbit fixtures also
  found no eligible shadowing pairs.

These checks find no duplicate mixing, cross-object filter sharing, or unexpected
nonlinear collapse in the inspected interval. They do **not** disprove the user's
perceptual complaint, prove all songs/intervals correct, or establish parity with
the remembered Windows playback. No verified Windows listening reference exists.
Do not disable room/near-field, boost rear objects, or weaken the peak guard and
describe that as a demonstrated porting fix.

No production rendering changes or new APK were made for this investigation.
Outputs: `E:/SDA/tools/multichannel-audit/{all,sum,general}.f32`; test log:
`E:/SDA/tools/test-multichannel-mix.log`.

## WASM decoder and all-enabled platform comparison (September 30)

Built the current `packages/core` source for `wasm32-unknown-unknown` with its
locked dependencies, and generated Node bindings with wasm-bindgen 0.2.127.
This is the decoder used by the Windows frontend, not the old checked-in test
WASM. The full Windows application was not rebuilt. The isolated build and
bindings are under `E:/SDA/tools/wasm-decoder-current`.

`E:/SDA/tools/compare-wasm-decoder.cjs` decodes `mygo.ec3` independently using
WASM and compares every channel sample and parsed metadata field against the
native decoder's JSONL export. It rebases both frame and event clocks for the
orbit fixture and compares labels and object-channel declarations as well.

| Window | Frames / scalar samples | Max PCM error | RMS PCM error | Metadata differences |
| --- | --- | --- | --- | --- |
| 0–12 seconds | 375 / 9,216,000 | 2.086162567e-7 | 6.079820035e-9 | 0 |
| 129.984–153.984 seconds | 750 / 18,432,000 | 2.384185791e-7 | 5.854293520e-9 | 0 |

Reports: `wasm-native-decoder-parity.json` and `wasm-native-orbit-parity.json`
under `E:/SDA/tools`. These measurements find no meaningful WASM/native object
separation, interchannel phase, gain, mapping or timing difference in the
inspected windows.

The diagnostic `pcm_probe` now accepts `SDA_PROBE_NEAR_FIELD=1` in both mobile
startup and desktop-feeder modes. This closes a gap in prior probes that enabled
the room but omitted independent near-field. With the same dense calibrated
KU100, studio room, near-field at 1 metre/unit, both object switches enabled and
zero head yaw, the first 12 seconds rendered on Android x86_64 and the Windows
host differ by max 7.897615433e-7 / RMS 7.784447867e-8. Android runs the newly
built standalone probe; the installed Media3 APK is unchanged. This is a
pre-output render comparison, not a Media3 capture.

The previously staged Windows sidecar was also replayed with both room and
near-field enabled (both commands acknowledged). It still differs from the
current Windows-host mobile path: first-12-second max 0.01688987 / RMS
0.0014875844. The same-code platform parity therefore must not be reported as
complete parity with that binary, much less the user's unavailable Windows
listening reference. The residual is not confined to the initial fade.

No saved SDA profile directory was found in the current Windows user's Roaming
app data. The exact effective settings of the remembered Windows playback remain
unverified. This investigation has not established a perceptual fix.

New all-enabled listening files are under
`E:/SDA/apk/audio-comparison/all-enabled/`: `windows-staged-all-enabled.wav` and
`android-all-enabled.wav`. Both preserve 48 kHz float stereo, all channels,
original gain and 135–152 seconds of the second song. Windows uses the decoded
window with 5.016 seconds of preroll; Android decodes/renders continuously from
the song start. Their 17-second max/RMS difference is 0.02620757 / 0.002304757.
The Android file is the standalone renderer output before Media3. The Windows
file is the staged sidecar output before device processing. Neither is a capture
of the user's remembered Windows application session.
