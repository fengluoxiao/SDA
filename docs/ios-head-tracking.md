# iOS AirPods Head Tracking

## Current Native Data Path

Core Motion callbacks now directly call the Rust `trackingProvider` command.
Rust applies the shared Windows helper attitude filter and sends its normalized
yaw quaternion directly to the KU100 renderer command queue. No pose enters JS:
the 20 ms JS timer, raw-pose export and processed-pose setter have been removed.
JS controls only the UI preference and reads diagnostic status. Native lifecycle
and timeout handling still reset the pose without JS participation.

The desktop TypeScript HeadTrackingSession is no longer in the iOS data path.
Its second-stage smoothing/anchor processing is not currently ported to Rust;
do not describe this version as complete desktop end-to-end behavior parity.
The sections below describe prior integration history, not the current path.

The shared header on Playback, Library and Space contains an opt-in tracking
button immediately left of Settings. Its preference is persisted. Settings
contains a recenter action. Tracking starts when the SDA/KU100 engine is ready;
Apple system spatial audio remains system-controlled and is not overridden.

CMHeadphoneMotionManager delivers a normalized quaternion to a new input adapter
in the existing Windows helper's `orientation.rs`. iOS compiles that exact source
file and calls its existing `filter_orientation` for median, discontinuity gates,
stationary lock and dead zone. The adapter replaces AACP packet parsing, bud
selection, count unwrap and count-domain gyro bias estimation: Core Motion does
not expose those raw packet fields. It uses the same ten-sample stable calibration
window and provides at most 30 samples/second, matching the Windows packet cadence.
The filtered provider quaternion is exposed with a session ID and sequence.
Foreground JS samples the latest provider value every 20 ms, timestamps
new samples with performance.now(), and feeds the exact desktop
`apps/web/src/head-tracking-session.ts` implementation. This uses the desktop
yaw-only policy, three-frame median, fixed-forward anchor, smoothingMs=220,
deadZoneDegrees=2.5 and maxDegreesPerSecond=480. Pitch and roll are intentionally
ignored, exactly as on Windows. The stabilized quaternion goes to the native
renderer through a session-guarded `setTrackedHeadPose` bridge. This is not a
Swift reimplementation of the desktop stabilizer.

The native background notification invalidates the motion session, stops
sampling and clears the renderer pose without depending on JavaScript. Active
notification starts a fresh reference. Late callbacks from older sessions are
ignored. Missing samples for one second clear the pose and reference; resumed
samples recenter. Clearing pose follows the renderer's existing short audio
ramp, so "immediate" means on the lifecycle event, not zero audio latency.

Native lifecycle and watchdog resets remain independent of JS. Background,
off/on, provider-session changes and recenter all discard the JS session state.
Native code rejects processed poses from older sessions or while no fresh raw
sample is available. JS scheduling under load can still add latency; the desktop
220 ms smoothing is deliberately retained for behavioral parity.

The earlier iOS-only rendered FIFO reduction and shared HRTF deadband edits have
been removed. The renderer's original policy is retained. Consequently this
change does not promise a low-latency end-to-end audio result or physical-device
parity without listening tests.

## Build and Verification

Rebuild SdaNative.xcframework from the changed Rust sources, then rebuild the
iOS app with the updated podspec and Expo configuration. An existing installed
native binary or Expo Go cannot provide the new pose command. The plist must
contain NSMotionUsageDescription and the pod links CoreMotion.

Automated checks:

- `node scripts/test-ios-head-tracking.mjs` (source contracts only)
- `node scripts/test-ios-session-recovery.mjs` (existing lifecycle contracts)
- `tsc -p apps/mobile/tsconfig.json --noEmit`
- Rust `sda-native` head-pose tests with the `ios-host` feature

Physical-device acceptance remains necessary:

- Grant/deny Motion access and connect a compatible AirPods model.
- Check yaw, pitch and roll independently for correct direction and zero origin.
- Turn the head, background the app, and verify neutral audio with playback intact.
- Foreground with the head turned; the first new frame must become neutral.
- Disconnect/reconnect AirPods; stale audio direction must not persist.
- Verify the header button on all three tabs and the recenter action.
- Verify Apple system spatial output is not affected by SDA tracking controls.

macOS can reuse the quaternion format and shared rendering logic, but this iOS
lifecycle adapter is UIKit-specific; the desktop helper is not modified here.
