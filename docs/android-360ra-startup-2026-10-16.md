# Android 360RA startup and compact glass footer (2026-10-16)

## Failure and fix

The installed Android APK reproduced `spatial cue layout changed; retry` on
`夜に駆ける (360 Reality Audio).m4a` with the saved spatial cue setting at -3 dB.
`ensureEngine("360RA-13")` used to restore the prepared 13-channel cue filters
before `nativeStart`, while the renderer still had its initial 7.1.4 layout.
The renderer correctly rejected that update. The packaged default -6 dB
skips restoration, explaining why the problem depends on saved settings.

Restore cues after a successful `nativeStart`, before opening/feeding the
source. Start queues SetLayout; the acknowledged cue update follows on the
same FIFO render command queue. Retain the layout mismatch guard and close
the engine if initialization fails. No HRTF resources, DSP coefficients,
codec logic, native engine binaries or iOS paths changed.

## Footer

- Mini: 52 -> 48 dp; cover: 34 -> 32 dp; playback icon: 28 -> 24 dp.
- Tabs: 64 -> 56 dp; icons: 25 -> 23 dp.
- Reserved height: 146 -> 128 dp with mini; 86 -> 74 dp without it.
- Stack: 8 top + optional (48 mini + 6 gap) + 56 tabs + 10 bottom.
- Update both backdrop sampling origins and both inner glass layers.
- Keep 48 dp playback/tab touch areas, native refraction and glass animations.

## Verification

- Release APK built for arm64-v8a and x86_64; signature and 16 KiB ZIP alignment passed.
- 628 packaged rendering resources match the approved iOS IPA from run
  38859092153 byte for byte; packaged native code/constants match staged libraries.
- Android chrome/layout, settings, live cue settings, enhancement, ALAC wiring
  tests and mobile TypeScript check passed. Shared asset staging checks passed.
- MuMu API 35, x86_64: original failure captured before installation. The same
  360RA track started with the same saved -3 dB setting after installation;
  playback clock advanced beyond three minutes. Live switch to 0 dB succeeded.
- Resized footer visually checked in library and player; expanding the mini
  and starting playback through its button worked.
- Restored the original -3 dB setting after testing. System volume was temporarily
  muted for automation, then changed externally to 100; that external setting
  was left intact. Concurrent device interaction interrupted the additional
  restart/codec-switch checks, which are not claimed as passed.
- Runtime log has no fatal exception or layout-mismatch failure. A later AAudio
  counter recorded one xrun; this is not a zero-xrun certification or a listening
  comparison. Physical ARM64 phone playback has not been exercised here.

Local evidence: `C:/sda-android-build/360ra-before.png`,
`360ra-player.png`, `360ra-live-zero-check.png`, `qa-cues-restored.png`,
`360ra-fixed-logcat.txt`, and `package-verification.json`.
