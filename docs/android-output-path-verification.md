# Android output-path verification (2026-09-30)

The current APK, with dense calibrated KU100, direct objects, actual direction,
near-field at 1 m/unit and studio room enabled, played MyGO from the beginning.
A one-shot optional capture in Media3Output records accepted float stereo buffers
in memory and writes them on another thread after 12 seconds. No per-block file IO.
The marker is consumed once, ordinary playback does not capture. Startup flushes
are retained; subsequent flushes cancel the capture.

At 2¨C12 seconds, actual APK submission versus the pre-build staged Windows renderer:
RMS 8.468818330982905e-8, max 7.748603820800781e-7, fitted gain 1.0000000796865562.
Against the final Android offline probe: RMS 3.322632318059974e-8.
This extends previous offline-only checks to actual APK demux, rendering and JNI submission.
The staged Windows executable remains an unaccepted subjective reference.

Windows WASAPI loopback of the actual Android playback used the current default
ToDesk Virtual Audio endpoint (44.1 kHz). Android AudioFlinger reports 48 kHz float
stereo processing, a 16-bit stereo HAL, unity per-track gains and zero effect chains.
No system settings were altered. Analysis resamples the source using scipy
resample_poly(147,160, Kaiser beta 12), then aligns integer and fractional delay.
Over seconds 2¨C11, relative waveform error is 0.00035614138 (0.035614%).
Fitted channel matrix is approximately [[1.00001618, -0.00000271],
[-0.00000981, 1.00000830]]. Most band power differences below 16 kHz are <0.002 dB;
16¨C20 kHz differs by approximately +0.28 dB, a small measured resampling difference.
These figures do not show a substantial loss of stereo information in this output path.

Independent instrumented replay of the SAME submitted PCM:
- Media3: relative output error 0.00035613934.
- Direct AudioTrack float stereo 48 kHz: 0.00035613588.
- Windows WASAPI float 44.1 kHz replay after explicit reference resampling:
  relative error 9.40164794e-8 (this is not the complete Windows player).
Media3 and direct AudioTrack both completed playback assertions. Replacing Media3
with AudioTrack is not supported as a remedy by these results. AAudio was not retested.

Validation: full instrumented suite 8 tests, 0 failures, 1 skipped (optional input
fixture); explicit capture startup-flush/stereo-byte-integrity test passed again;
Media3 and AudioTrack comparison tests passed separately. Release build passed.
Diagnostic APK JNI hash exactly equals the preceding elevated-tie-fix APK JNI hash.
No rendering algorithm, effect setting or backend selection changed in this task.

Artifact: E:/SDA/apk/SDA-android-output-diagnostic-x86_64.apk
SHA256 AF5588452EC44B4431F99C03B3A0F2BC23BAB678EF688F54FF213B76A3711EDB
Raw captures and analysis scripts/logs: E:/SDA/tools/mygo-*-loopback*,
mygo-apk-media3-submitted.f32, mygo-apk-media3-settings.json,
analyze-apk-loopback.py and capture-wasapi-loopback.py.

Limitations: one song, opening 12 seconds, this machine's current virtual endpoint;
not all songs, not the 2:15 orbit passage, not another physical phone or a recording
at the listener's ears. The original front/back subjective complaint is NOT declared
resolved. User subsequently authorized building the full current Windows application
to establish an actual available listening reference.
