# Windows room assets on Android

Android now packages the current Windows catalog's 7.1.4 room: SDA Near-field
Control Room (6 × 5 × 3.2 m, studio). The build reads the Windows catalog and
copies its asset and license notices; it does not generate another response.
Other layouts are excluded because mobile playback currently uses 7.1.4.

The settings sheet offers room bypass and the near-field studio. Selection is
persisted, applied before playback starts, and can change during playback.
Live changes report success only after the renderer acknowledges the new graph.
Failures retain the previous selection. The native graph construction is shared
with the Windows `setCinema` command, using the same default cinema parameters
and the selected KU100 direct response.

The Kotlin asset loader verifies the compressed desktop SHA-256 and the
decompressed content ID before use. Corrupt caches are restored from packaged
assets. Archives are packaged with a `.gz.bin` suffix because Android's asset
compiler otherwise automatically decompresses `.gz` and removes the suffix.

Validation (2026-09-29):

- TypeScript check and Android release build pass.
- All three Android room instrumentation tests pass: archive/cache integrity,
  bypass, and rejection of an unknown/path-traversal room ID.
- Shared native protocol tests: 11 pass, one diagnostic test ignored.
- Actual Windows room-response replay and the shared mobile graph both render
  the second track's orbit window. Room on/off RMS difference is 0.06709;
  the room path changes the audio, not merely the displayed setting.
- MuMu: room selection survives process restart; startup with room enabled
  succeeds; switching off/on during playback is acknowledged and playback
  continues. Left playing the second song with the studio enabled.

APK: `E:/SDA/apk/SDA-android-windows-room-x86_64.apk`
SHA-256: `61D7D90BCDFA950BD99AE8FCD62B939D81A8BF2B633085084D95437838B0F752`

This completes room integration, not proof that the original listening mismatch
is resolved. The user rejected the staged-Windows reference as unlike their
remembered Windows playback, and no accepted Windows installation is available.
See `android-audio-parity-investigation.md` for that investigation.
