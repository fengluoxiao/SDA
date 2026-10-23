# Android / iOS playback and settings parity — 2026-10-16

This Android build starts from the accepted iOS commit `5c3fd08` and keeps decoder
submodule `2411ed0`. G80, the 542-direction bank including 218 horizontal support
directions, full-height support, mirrored triangles, common timing, front-ear
policy, and the integrated JOC conditioning remain the same shared implementation.

## Android changes

- Expose spatial enhancement and live spatial-cue strength, with the same default
  values as iOS (enhancement off; cues -6 dB), validation and saved preferences.
- Prepare cue filters without holding the decoder lock. Validate engine identity,
  generation and layout before applying; always release the prepared handle.
  No transport restart, file analysis, or loudness-matching gain is introduced.
- Restore saved cue strength when opening playback or replacing the HRTF preset.
- Add playback-mode and representative-color settings. Apply the existing shared
  MD3 light/dark schemes to Android backgrounds and controls. Direction text uses
  the active resource manifest instead of the obsolete hard-coded 128 label.
- Accept normalized/raw AC-4 input and report the active source codec to the UI.
  Apple's system-output / AirPods-specific controls remain platform-specific.
- Build both ARM64 and x86_64 native engines. Align their ELF load segments to
  16 KiB, compatible with both Android page-size configurations.
- Repair current Expo/Gradle compatibility: use the Expo version catalog,
  ProviderFactory execution, and source-map paths local to the checkout drive.
  Metro watches the physical dependency directory when node_modules is linked.
- Enforce consistent LF endings for shared audio metadata and notices so packages
  produced on Windows and macOS contain identical resource bytes.

## Validation

The shared cue transport/concurrency/failure tests pass on both iOS and Android.
Enhancement controls preserve transport and handle failure/busy/system-route
states. Character-theme defaults and ten MD3 text contrast schemes pass.
AC-4 framing, bounded import writes, cleanup, E-AC-3 fallback and both mobile raw
entry points pass. iOS spatial-enhancement and mobile rendering-preset tests pass.
AAudio/Media3 output-policy tests confirm that app-rendered binaural output does
not request another system spatialization pass. TypeScript checks pass.

The actual reference is iOS Actions run `38859092153`, SDK26 artifact
`12030651234`, rather than a separate local asset directory. The final APK check
compares every HRTF-family resource and rendering preset to that IPA. Native
library verification compares executable code and constants (plus build IDs when present) against
the newly built ARM64/x86_64 libraries after Android packaging strips symbols.

Local build/verification logs: `C:/sda-android-build/`.
Build command: `assembleRelease -PreactNativeArchitectures=arm64-v8a,x86_64` after
building the shared native crate for both targets with Android NDK 27.1.
Local-only Java temporary-directory, repository-mirror and cross-drive Kotlin
cache settings are in the external build script, not application preferences.


## Restored original Android presentation and stereo upmix

The referenced AndroidLiquidGlass implementation was not present in the accepted
iOS packaging commit. The native Chrome/Symbol modules, Backdrop catalog controls,
platform tab adapter, mini-player, glass settings/back buttons and MD3 settings
were restored from that existing implementation and are now included in this
branch. Backdrop 1.0.6 uses the matching Kotlin/Compose 2.3.10 toolchain. Its license
and local-modification notice are packaged. Research-only -0.75 dB recipe text and
preview mock data were not imported. Existing iOS UI regressions pass with explicit
platform-aware status-bar and safe-area assertions.

Android ALAC file decoding now uses the same live AlacPcm adapter as iOS. The
native file path recognizes actual ALAC metadata, requires stereo input, decodes
incrementally, resamples to 48 kHz when needed, and feeds the existing renderer.
The switch is persisted and read during playback; it changes the shared 50 ms
adapter ramp without reopening the file or resetting transport. Local files are
reused, and cleanup deletes only source copies owned by this decoder. Non-ALAC
container files retain the existing decoder path. The JOC decoder and HRTF assets
are unchanged by this host integration.

Validation on MuMu x86_64:
- Four tests of the actual shared ALAC adapter passed: reversible live switching,
  chunk invariance, channel order, unchanged dry stereo, mono height rejection,
  and malformed-input rejection.
- The actual file decoder passed complete 44.1 kHz / 35-second and 48 kHz /
  300-second stereo ALAC fixtures. Fixture codec/rate/duration were independently
  checked through the shared MP4 demuxer.
- The APK played the 48 kHz fixture. Switching upmix off preserved the progressing
  playback clock; AAudio counters continued with zero observed xruns. The prior
  enabled preference and emulator music volume were restored after testing.
- The broad native test executable hit a Windows NDK linker crash. A focused
  Android executable using the exact same alac_pcm.rs/mp3.rs files and real
  sda-core FrameData was built and executed for the above six tests; no substitute
  DSP implementation was used.

The initial smoke test exposed desktop Three.js being resolved through a reused
root node_modules directory. The mobile pinned dependencies are now resolved
correctly, and a bundling guard verifies React, React Native, Three and Fiber
versions before producing another APK. The final native glass UI starts normally.
