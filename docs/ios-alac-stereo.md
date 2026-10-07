# iOS stereo ALAC routes

Stereo ALAC in unencrypted M4A/MP4 is decoded by AVAssetReader to interleaved
48 kHz Float32 stereo. E-AC-3 remains compressed for the existing Rust decoder.
Non-stereo ALAC is rejected rather than silently downmixed.

## Settings (both default off)

| Stereo upmix | Apple system output | Route |
| --- | --- | --- |
| Off | Off | Stereo PCM to SDA/KU100 |
| On | Off | Shared stereo-to-7.1.4 adapter to SDA/KU100 |
| Off | On | Stereo PCM to Apple sample-buffer renderer |
| On | On | 7.1.4 PCM to Apple sample-buffer renderer |

Preferences are snapshotted at track start. Changing them does not stop or restart
current playback; the next playback uses the selected route. The existing 360RA
system-output preference is independent. System output bypasses SDA HRTF/room.
Actual Apple spatial effects depend on the output device and system controls.

The upmixer ports packages/player/src/alac-stereo-upmix.ts. It distributes stereo
mid/side information; it does not reconstruct original objects or native height
content. UI identifies this explicitly as upmix, not Atmos. SDA channel order has
side surrounds before rears; Apple CICP19 has rears before sides, so the system
adapter explicitly swaps those pairs. Original stereo measures loudness, with one
linked gain for all generated channels and a separate ALAC cache namespace.

No accepted KU100 cue assets, directional renderer tuning, or song-specific
processing was changed.

## Verification

- Six Rust ALAC adapter/ABI tests: passthrough, malformed input, upmix coefficients,
  chunk invariance, channel mapping, linked gain, analysis without output, clock.
- TypeScript and iOS UI/queue/polling/drawable regression scripts.
- Apple CI probe: generates 44.1/48/96 kHz ALAC, verifies stereo identity,
  48 kHz output duration, reader recovery and mono rejection.
- Apple probe and Swift typecheck require macOS CI; Windows host tests do not
  establish Apple device playback or AirPods spatialization correctness.
