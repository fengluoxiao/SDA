# Android / Windows audio investigation — 2026-09-29

## Reference mismatch confirmed by listening (latest finding)

The user identified 2:15–2:32 of the second track as the clearest failure:
two orbiting objects have clear front/back distinction in their Windows player.
Decoded metadata identifies these as objects 12 and 14 (163 / 162 position
changes in that interval). Android MediaExtractor extraction for this entire
track passed the existing instrumentation SHA-256 comparison (all 3 tests).

A decoded window starts at frame 6,239,232 (129.984 seconds), with 24 seconds
of PCM and rebased events. Its first 5.016 seconds serve as preroll. Isolating
objects 12/14 gives exactly identical PCM between the staged Windows executable
and the Windows-host renderer; the Android executable differs by only
max 4.10e-7 / RMS 1.58e-8 during 135–152 seconds. Full mix window RMS difference
is 0.00080335, and Android continuous-from-start vs the Windows window is
0.00230774. These are diagnostic comparisons, not perceptual acceptance.

**The user listened to the exported staged-Windows full-mix reference and
explicitly said it also lacks the effect of their actual Windows player.**
Therefore the staged executable harness is NOT an accepted listening reference.
Do not infer successful porting from matching it. The next priority is locating
and capturing the user's actual Windows playback chain, including its decoder,
renderer executable, assets and effective startup commands.

Artifacts: `E:/SDA/tools/mygo-orbit-*.f32`, `mygo-154-android.f32`,
`mygo-orbit-frames.jsonl`, and `E:/SDA/apk/audio-comparison/` WAVs. The window
probe supports `SDA_PROBE_START_SECONDS` for `dump-frames`; normal long probes
now drain presentation metadata to avoid overflowing the diagnostic queue.

An optional `pcm-diagnostic` Cargo feature adds an actual-app, pre-AAudio PCM
capture, armed explicitly by Android property `debug.sda.pcm_capture=135,152`.
It buffers the chosen interval and writes on a separate thread. It is absent
from default builds. The first capture reached all 816,000 frames but its file
write failed because adb had created an external directory owned by shell.
The Kotlin host now asks Android to provision its own external-files directory.
The corrected capture succeeded: all 816,000 stereo frames from the actual APK
are saved in `E:/SDA/tools/mygo-apk-135-152.f32`. Compared with the Android
offline probe started from the beginning of the track, max/RMS differences are
2.38e-7 / 2.06e-8. This closes the actual-app-vs-probe gap for this interval;
it does not establish parity with the user's unavailable Windows installation.
The diagnostic property is disabled and the subsequent room APK is built
without the diagnostic feature. The user does not currently have the Windows
package available and advised against rebuilding the Windows application.

The user subsequently requested Windows room simulation assets on Android.
That implementation and validation are documented in `android-windows-room.md`.

Status: a KU100 dominant-HRIR selection discrepancy has been reproduced and
fixed. The reported listening difference is not yet fully resolved; a smaller
measured residual remains on the user's selected track. Earlier doll-only
results below must not be generalized to other tracks.

## Confirmed finding on the user's second track

The user identified `01. 壱雫空.m4a`. Its first 12 seconds were decoded once and
the same PCM/object events were replayed into the existing staged Windows
`SdaNativeRenderer.exe` and the mobile renderer. Windows float PCM was captured
from its loopback-only remote mirror before device processing, with local
playback muted. The staged executable matches Git blob
`1fab67017b4c1057561bf4efc474dac27e6ea61e` from commit `7ee75f2`.

This actual binary comparison revealed a difference that the earlier
same-source MobileEngine comparisons could not reveal. Impulse isolation at
Cartesian `[1,1,1]` reproduced it with stationary objects too: the two
directional HRIR responses differed by about 44.76% relative L2 norm.

`directional::Grid::interpolate` selected its dominant measurement by exact
floating-point weight order. At symmetric positions between measurements,
platform rounding could select a different anchor. KU100's notch guard can
replace the interpolation with that entire anchor response, turning tiny
numerical differences into large filter differences. The fix treats weights
within `1e-6` as tied (directions enter through f32 math), and consistently
selects the higher manifest index, preserving the old exact-tie convention.
It is confined to the KU100 notch-guard path.

| Second track, 576,000 frames | Maximum sample error vs staged Windows | RMS error |
| --- | ---: | ---: |
| Android before fix | 0.2351635098 | 0.0175663901 |
| Android after fix | 0.0168894529 | 0.0014866717 |

RMS error decreased by approximately 91.54%; that is a numerical comparison,
not a percentage measurement of perceived sound quality. A residual remains
(largely associated with object 22 in isolation), so this is not a claim of
complete Windows parity. Fixed Android vs fixed Windows-host MobileEngine
max/RMS differences are `8.49e-7` / `7.52e-8`.

Three new regression tests pass, including real dense KU100 HRIR continuity
under one-f32-ulp perturbations around both upper-front symmetric directions.
The broader directional suite had one existing failing shared-reflections /
near-field / speaker-focus test. Restoring the original dominant selector for
that test reproduced exactly the same failure (`0.0040866397`), so it was not
introduced by this change. The unmodified test is retained. The GNU build
script now links ASIO's C++ runtime only with `cpal-output`, allowing the
renderer-only test build without desktop audio libraries.

Additional diagnostic artifacts in `E:/SDA/tools/` include
`mygo-frames.jsonl`, `mygo-android.f32`, `mygo-staged-renderer.f32`,
`mygo-fixed-android.f32`, `probe-staged-renderer.py`, `dominant-tests.log`, and
`directional-baseline-test.log`. Availability/arrival experiments were reverted;
they did not explain this discrepancy.

## Matched comparison settings

The user confirmed the same version, per-object rendering, actual-direction
rendering, and high-resolution KU100 (61 directions), with other effects off.
The tests use calibrated KU100, wet weight 0.04, 7.1.4, 48 kHz stereo and unity
volume. The desktop E-AC-3 automatic-layout code also selects 7.1.4.

## Input and assets

- Source: `C:/Users/legendshop/Downloads/1-01 doll.m4a`.
- Extracted E-AC-3: `E:/SDA/tools/doll-demux.ec3`, 17,200,128 bytes.
- SHA-256: `f09a8a0a5923bd6fd599ae63fafc47f5585278a2a278b239ff0e91a6b91c25fa`.
- Android MediaExtractor bytes previously matched desktop extraction.
- All 36 standard and 123 dense HRTF asset files match byte-for-byte between
  web public assets, desktop staged assets and Android source assets.

## Pre-device PCM measurements

`crates/sda-native/examples/pcm_probe.rs` captures float stereo from the render
FIFO without AAudio, WASAPI, system mixing or emulator processing. Each output
contains exactly 576,000 frames (12 seconds), starting at codec sample zero.

| Comparison | Maximum absolute sample error | RMS error | Correlation |
| --- | ---: | ---: | ---: |
| Windows MobileEngine vs Android MobileEngine | 4.8428774e-8 | 3.1544291e-9 | 0.9999999999999131 |
| Windows MobileEngine vs desktop-feeder reproduction | 1.8626451e-8 | 1.4585574e-9 | 0.9999999999999842 |

Mobile reference RMS is 0.0074193782; peak is approximately 0.068906, with no
clipping. The desktop-feeder reproduction compacted 5,599 of 5,625 events.

The optional `desktop-feeder` probe argument uses independent native Engine
setup, timestamped source declarations, channel-index bed IDs, object-channel
mapping/retirement, JSON event conversion, and desktop repeated-target event
compaction. It does **not** run the actual Electron player, WASM decoder,
transport frame batching, persisted desktop settings or output device. Its
result rules out these reproduced feeder differences for this segment only.

Artifacts under `E:/SDA/tools/`:

- `windows-dense.f32`, `android-dense.f32`, `desktop-feeder-dense.f32`
- `probe-windows.log`, `probe-android.log`, `probe-desktop-feeder.log`
- `probe-host-build.log`, `probe-android-build.log`

Example invocation after building the example:

```text
pcm_probe INPUT.ec3 HRTF.json OUTPUT.f32 12
pcm_probe INPUT.ec3 HRTF.json OUTPUT.f32 12 desktop-feeder
```

## Runtime observations and remaining checks

MuMu settings showed per-object and actual-direction enabled. AAudio reported
float stereo at 48 kHz, zero partial writes and zero xruns. Media stream volume
was 100/100. These checks do not measure the samples after emulator/host mixing.

The full Electron listening session has not been captured. Subsequent testing
of the existing staged native executable is described above. Continue tracing
the remaining object-22 rendering residual; do not attribute this proven
pre-device discrepancy to MuMu or claim full parity from the doll result.
