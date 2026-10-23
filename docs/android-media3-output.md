# Android Media3 PCM output

The Android JNI playback entry now starts Media3 1.5.1 DefaultAudioSink instead
of AAudio. The original E-AC-3/JOC decoder and native KU100 object renderer still
produce 48 kHz interleaved float stereo. Media3 receives AUDIO_RAW / PCM_FLOAT,
with float output explicitly enabled, no EQ, no speed adjustment and no second
Atmos decoder/downmixer. AAudio remains available to native diagnostic clients;
the application's nativeStart entry does not call it.

Media3 operations are serialized on a dedicated HandlerThread. The native output
thread owns a Java global reference, not a MobileEngine pointer, so engine close
can signal shutdown without calling into freed engine memory. Incomplete writes
retain the same ByteBuffer, FIFO flush drops pending device data, and pause/resume
controls the sink. Playback telemetry follows Media3's device playback position,
not the number of frames enqueued. Output errors are surfaced through feedError.

User-required listening conditions: calibrated high-resolution KU100 (61
directions), per-object HRTF, actual directions, room simulation and independent
near-field all ON. Do not disable either near-field or room to manufacture a
comparison. No HRTF asset or tonal correction is changed by this output work.

The workspace already contains the previous frame-router and repeated-direction
fixes. This build retains those changes; it is not a binary-identical DSP baseline
to the older clean-rendering APK. No accepted Windows reference recording exists,
so successful output checks do not establish Windows listening equivalence.

Instrumentation coverage: float sink initialization, paused backpressure,
resuming a pending buffer, device clock advance, flush clock reset, and repeated
release. The application must also be checked for live song playback and enabled
rendering settings on MuMu after installation.

## Verification (2026-09-30)

- Native release and Android assembleRelease passed; git diff --check passed.
- Android instrumentation: 6 passed, 1 existing media-fixture test skipped.
  Media3OutputTest passed on MuMu Android 12.
- Installed `E:/SDA/apk/SDA-android-media3-x86_64.apk`, SHA256
  `BD41D697EB421AD4DD644B0330BC1A384EA0728865C193EA02817419A150C7FA`.
- Played `01. 壱雫空.m4a` in the actual application, observed advancing Media3
  device timestamps, paused with a stable UI position, and resumed playback.
- UI confirmed direct/directional/near-field ON, 1.00 metres per unit, near-field
  studio room selected, 61 directions and 15 independent object convolvers.
- AudioFlinger snapshot: app PID 12365, active track 91, 48000 Hz, stereo mask 3,
  format 5 (PCM_FLOAT), zero per-track underruns at the inspected snapshot.
  This confirms the application output contract, not subjective sound quality.

## Listening follow-up

The user reports that Media3 removed the muffled sound, but left/right remain
clearer than front/back. Clarification: this is front/back ambiguity, not simply
insufficient radial distance. Preserve this accepted output build and do not
alter distance gain or disable the requested room/near-field settings.

Source inspection found the same default HRTF wet weight (0.04), room direct/
early/late gains (0 dB), early boundary (50 ms), near-field scale (1.0), and
disabled extra source diffusion in the mobile and current desktop code. The
existing same-X front/back filter regression passes. These findings do not
establish parity with the user's remembered Windows listening session.

An opt-in renderer test `export_front_back_all_enabled` exports identical seeded
noise at [0,+0.65,0] and [0,-0.65,0], using the actual dense calibrated KU100,
continuous object path and desktop studio room, with near-field enabled. No
production rendering or installed APK was changed for this follow-up.
Outputs: `E:/SDA/tools/front-back-all-on/front.wav` and `rear.wav` (3 s each).
Normalized PCM correlation is 0.291323; RMS 0.013720 / 0.014704. This demonstrates
distinct rendered signals, not perceptual front/back discrimination. The probe
runs the shared renderer on the host and is not a capture of Media3/device output
or an accepted Windows player reference.
