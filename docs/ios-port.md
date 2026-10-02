# iOS port (initial implementation)

The mobile UI and Rust decoder/KU100 renderer are shared with Android and desktop, including the front-common centre-HRTF vocal clarity correction. Apple only receives the rendered two-channel PCM: AVAssetReader retrieves compressed E-AC-3 access units, not Apple's Atmos downmix. MPEG-H MP4 import uses the existing JS demuxer, MHAS and MP3 use the native decoders. DRM files are unsupported.

## Build without a Mac

Push to fengluoxiao/SDA, branch feat/android-port, or dispatch the iOS port CI workflow. The workflow stages verified HRTF/room assets, builds device and arm64 simulator Rust XCFramework slices, generates the Expo Xcode project, installs pods, builds a Release simulator app and attempts a launch smoke test, then archives an unsigned arm64 device app. Build and simulator logs are retained as Actions artifacts.

Minimum deployment target is iOS 16.0. The SDK-26 job is required. SDK-27 is built only when an actual iOS 27 SDK exists on the hosted image; absence is explicitly reported as NOT validated, not compatibility success. macOS hosted patch versions cannot be fixed to 26.0–26.4; host, Xcode, SDK and runtime versions are recorded.

The unsigned IPA is NOT directly installable. Physical installation requires an Apple signing identity and provisioning profile, or a user-controlled signing workflow. No certificates or credentials are committed. App Store/TestFlight distribution is not configured.

## Audio/lifecycle

AVAudioSourceNode consumes the same Rust stereo FIFO at 48 kHz, planar Float32. AVAudioEngine converts to the hardware route sample rate. It does not request an Apple Atmos renderer. This cannot guarantee that user-enabled OS/AirPods processing is bypassed; listen with OS Spatialize Stereo off when comparing.

Native decode continues independently of JavaScript. Current-track background audio, lock-screen pause/resume and interruptions are implemented. Unplugging pauses playback. In-app preset changes replace the renderer without reopening the stream. Background automatic playlist advance and physical AirPods/device listening remain validation items, not claimed completed features.

Run host callback tests with cargo test --manifest-path crates/sda-native/Cargo.toml --no-default-features --features ios-host --locked ios::tests. Resource staging: node scripts/prepare-ios-assets.mjs. Native Mac build: bash scripts/build-ios-native.sh. Generated Xcode projects, XCFrameworks and assets are ignored rather than committed.
