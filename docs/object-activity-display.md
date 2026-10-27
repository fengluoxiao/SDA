# Object activity display

Desktop and the iOS/Android native renderer use the same source-signal policy. The renderer detects post-source-gain/post-mute object activity, holds the native signal for 200 ms, and publishes snapshots only when their sample position has been consumed by the output device. Position or movement does not gate a sounding source.

`RuntimeTelemetry::sounding_object_ids()` exposes this consumed activity to the mobile status JSON as `soundingObjectIds`. The audio output callback never takes the activity mutex. The renderer worker publishes changed snapshots; foreground UI polling reads at most 128 IDs. Pause/shutdown returns no activity, and reset/seek clears stale activity. The existing desktop event bridge is preserved.

Both surfaces reuse `packages/player/src/object-activity.ts`: immediate full brightness on signal, a 100 ms display hold and 250 ms fade after signal stops. Display time is monotonic and independent of the paused playback clock. Full object positions and audio data remain available separately. Both surfaces offer a show-all override; silent sources are dimmed in that mode.

Mobile applies opacity directly to existing Three.js materials during scene frames. It does not rebuild the player UI at animation frequency. Hidden/background scenes stop drawing, source changes reset display history, and counts distinguish displayed sources from declared geometry. Existing native modules or system-owned output without source activity retain their position view instead of inferring sound from movement.

Tests compare mobile projection to the shared desktop filter, execute actual mobile dot/material frame updates, check full geometry and native activity forwarding separately, retain background/rendering guards, and verify renderer activity waits for DAC consumption and excludes muted sources. Production bundles must exclude Expo Go artwork fixtures.
