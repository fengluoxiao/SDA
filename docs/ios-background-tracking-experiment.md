# Background Head Tracking Experiment

## Current Behavior

The user confirmed background tracking works on their device. The separate
experimental toggle has been removed. The header's native SwiftUI Menu now offers
Fixed and Head Tracking with a selected checkmark. Head Tracking permits motion
updates during running KU100 background playback; Fixed stops sampling and resets
pose. Pause/interruption/stale-data safeguards remain. The following procedure
documents the earlier experiment, not the current settings layout.

Settings / Remote Diagnostics contains an opt-in Background Tracking Experiment
toggle, off by default. It does not add PiP, silent audio, location services or
any new background entitlement. Enable the main head-tracking control as well.

Only running SDA/KU100 audio permits background motion processing. Pause,
interruption, stopped output or missing samples cause neutral reset. With the
experiment off, entering background stops and resets tracking as before.

Physical iPhone and compatible AirPods are required:

1. Start the diagnostics receiver and configure the phone's receiver/token.
2. Enable remote sending, main head tracking and the background experiment.
3. Play a track through SDA/KU100, wait for calibration, then switch apps.
4. Turn the head for 30 seconds. Repeat with the screen locked for 30 seconds.
5. Inspect `motion.sampleTime`, `motion.backgroundCallbacks` and `background`.
   Continuously increasing timestamps and counts while background is true prove
   delivered samples. Repeated snapshots with unchanged values do not.
6. Pause in background and verify neutral audio; disable the experiment and
   verify the original immediate background reset behavior.

CI/source checks cannot prove background Core Motion delivery. No successful
physical-device result is claimed until these measurements are collected.
