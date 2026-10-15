# Shared C experiment packaging integration

User requested the experimental implementation/assets be included for iOS, Android, Windows and macOS. This is packaging integration, not proof that four device installers were built or sound quality accepted.

## Shared configuration

Existing active.json selects fullheight-triangle-commonphase-542-v1. A new shared staging helper validates response hashes/lengths, exact manifest, triangle hash/order, copies the dense family and original17fallback, and writes render-experiment.json. The source folder retains its older ios-experiments name for compatibility but is now consumed by all four packaging entry points.

- iOS prepare-ios-assets calls shared stage; original ios-experiment marker stays for IPA verification.
- Android Gradle Sync overlays the selected experiment into generated assets and tracks experiment inputs. Kotlin validates manifest-referenced responses and triangle sidecar instead of counting61*_dry.f32 files. CI verifies actual APK experiment marker and response/triangle hashes before upload. Newly built native library includes the same manifest-enabled triangle implementation; do not reuse an old prebuilt.so.
- Windows desktop build-native-renderer compiles the shared Rust renderer, stages existing families then overlays selected KU100dense+original17fallback. beforePack checks experimental assets and renderer existence, refusing missing/mismatched files.
- macOS uses the same native renderer staging and Electron extraResources. Release workflow now explicitly builds helper+renderer, selects aarch64/x86_64 Apple target by DMG architecture, fetches native dependencies and includes them before Electron packaging.

The experimental dense family is selected through the existing KU100dense preset/path. Other human/personal HRTF presets are not forcibly converted to this experiment; do not claim all alternative presets run542. Web-only playback is outside this native four-platform request.

## Executed checks and remaining limits

Four temporary stage layouts compared byte-for-byte for manifest/triangle/response files and17fallback; deliberate response corruption rejected. Shared staging entry/source contract test passed. iOS portable bundle8tests, mobile preset checks, Android ALAC source regression, TypeScript and Node syntax checks passed. These are resource and source tests, not Android Gradle compilation, Windows NSIS, macOS DMG, actual APK/IPA extraction or device listening.

No new Android/Windows/macOS installers triggered in this turn. Existing iOS124 runs an earlier immutable commit and does not receive uncommitted shared-stage/UI changes. Actual installers still require per-platform build and artifact verification before claiming all four are delivered. No new subjective sound acceptance claimed.
