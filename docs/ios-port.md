# iOS port

The mobile UI and Rust decoder/KU100 renderer are shared with Android and desktop, including the front-common centre-HRTF vocal clarity correction. Apple only receives the rendered two-channel PCM: AVAssetReader retrieves compressed E-AC-3 access units, not Apple's Atmos downmix. MPEG-H MP4 import uses the existing JS demuxer, MHAS and MP3 use the native decoders. DRM files are unsupported.

## Build without a Mac

Push to fengluoxiao/SDA, branch feat/android-port, or dispatch the iOS port CI workflow. The workflow stages verified HRTF/room assets, builds device and arm64 simulator Rust XCFramework slices, generates the Expo Xcode project, installs pods, builds a Release simulator app and runs a native audio smoke test, then archives an unsigned arm64 device app. Build and simulator logs are retained as Actions artifacts.

Minimum deployment target is iOS 16.0. The SDK-26 job is required. SDK-27 is built only when an actual iOS 27 SDK exists on the hosted image; absence is explicitly reported as NOT validated, not compatibility success. macOS hosted patch versions cannot be fixed to 26.0–26.4; host, Xcode, SDK and runtime versions are recorded.

The unsigned IPA is NOT directly installable. Physical installation requires an Apple signing identity and provisioning profile, or a user-controlled signing workflow. No certificates or credentials are committed. App Store/TestFlight distribution is not configured.

## Audio/lifecycle

AVAudioSourceNode consumes the same Rust stereo FIFO at 48 kHz, planar Float32. AVAudioEngine converts to the hardware route sample rate. It does not request an Apple Atmos renderer. This cannot guarantee that user-enabled OS/AirPods processing is bypassed; listen with OS Spatialize Stereo off when comparing.

Native decode continues independently of JavaScript. Current-track background audio, lock-screen pause/resume and interruptions are implemented. Unplugging pauses playback. In-app preset changes replace the renderer without reopening the stream. Background automatic playlist advance and physical AirPods/device listening remain validation items, not claimed completed features.

Run host callback tests with cargo test --manifest-path crates/sda-native/Cargo.toml --no-default-features --features ios-host --locked ios::tests. Resource staging: node scripts/prepare-ios-assets.mjs. Native Mac build: bash scripts/build-ios-native.sh. Generated Xcode projects, XCFrameworks and assets are ignored rather than committed.

## Automated validation

The iOS bridge tests include FIFO stereo/pause/flush/clock checks, null-pointer error ownership, and a generated E-AC-3 stereo tone through the actual KU100 renderer and pull callback. The tone test verifies nonzero, non-identical left/right output and that a live HRTF preset change preserves decoded and consumed clocks. It is not an Atmos listening comparison. No user recordings are included in CI.

The simulator smoke test explicitly sets SDA_IOS_SMOKE=1 to exercise a bundled generated E-AC-3 M4A via AVAssetReader and the real AVAudioEngine pull callback, including pause/resume and a live preset update. It writes audio-smoke.json with clocks and hardware route sample rate. Ordinary launches do not auto-play this fixture. A simulator callback check does not establish physical-device/AirPods listening quality.

## Verified build (2026-10-02)

GitHub Actions run **36986380540**, source commit **3827fa8ed5701b2cb452bbf33e5ae2cd957ff49c** on fengluoxiao/SDA / feat/android-port passed the SDK-26 job, including Release simulator build, compressed audio playback checks and unsigned device archive. The actual hosted environment was macOS 26.6.2 / Xcode 26.6 / iOS SDK and simulator runtime 26.5. The iOS-27 matrix job reports SDK unavailable; its successful job conclusion is only an explicit skip, NOT an iOS-27 build or compatibility result.

Outputs: SDA-unsigned-ios16-arm64.ipa and SDA-simulator-arm64.zip in the SDA-iOS-SDK26-8 Actions artifact (14-day retention). The final device archive was independently inspected: Info.plist MinimumOSVersion=16.0 and Mach-O minimum iOS version=16.0.0, device families iPhone/iPad, background audio enabled, no provisioning profile or code-signature directory. SDK-16-runtime execution and physical-device listening have not been tested.

Validated checks:
- Apple MP4 reader yields compressed bytes identical to the generated raw E-AC-3 fixture; zero-sample markers are skipped and deferred sample data is materialized.
- Rust callback regression tests pass with real KU100 assets.
- Simulator AVAudioEngine consumes the rendered stereo FIFO at a 48 kHz route, drains the generated track, preserves pause clock and maintains playback generation/clock across live HRTF replacement.
- App starts and presents the shared player UI. iOS safe-area wrapper avoids status-bar/home-indicator overlap without changing Android layout.

CI fixes included CocoaPods public C bridge visibility, explicit arm64 simulator architecture, runtime-compatible phone selection, compressed sample readiness handling and proper failure-cache configuration. Signing, App Store/TestFlight distribution, background playlist advancement and iOS-27 validation remain external/future verification tasks.
