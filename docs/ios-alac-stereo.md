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

The upmix preference is applied to the active ALAC session without restarting.
The Apple system-output preference still selects the output transport at track
start. The existing 360RA system-output preference is independent. System output bypasses SDA HRTF/room.
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

## Native ALAC balance startup

The SDA/KU100 ALAC path (both 2.0 and 7.1.4 upmix) now opts into an early,
provisional estimate after seven audible gated windows, approximately one second
of audible material. Measurements are published every eight decoder batches,
so acquisition is not an exact one-second wall-clock promise. Previously it
waited for 57 audible windows (about six seconds), then scheduled another
multi-second gain staircase. Silent introductions could delay this further.

The first estimate uses the renderer's existing 50 ms linked gain ramp at the
source sample position. It does not stop playback or predecode the whole track.
Later estimates retain the conservative update schedule. Full-track cache
requirements, the -18 LUFS / -1 dBTP attenuation-only policy, original stereo
measurement, and the upmix/HRTF coefficients are unchanged. A valid cached
measurement takes priority. Other codecs and Apple system output are unchanged.

Regression coverage includes the previously failing early-estimate case,
silence/quiet inputs, cache priority, reset isolation, native 2.0/7.1.4 PCM gain,
uncached short-track attenuation, and mid-playback disable/re-enable with the
same native handle. These host tests do not replace iPhone listening validation.

## Live ALAC upmix switching

- The active feeder uses the new upmix setting, preserving the reader, playback
  clock, original-stereo loudness measurement and cached gain. Native lookahead
  is bounded to about half a second plus a feed batch, rather than four seconds.
- Dry/wet coefficients ramp over approximately 50 ms (including reverse switches).
  The native ALAC graph reserves 7.1.4 buses once before its first PCM, avoiding
  convolution graph resets on switches. Dry mode has only original L/R samples;
  all ten additional inputs are exactly zero. UI layout follows the selected
  listening mode (2.0 or 7.1.4), not reserved internal graph capacity.
- Apple output stores unmodified stereo in its decoder queue and upmixes at read
  time. Buffers keep their own format and presentation timestamps. On disabling,
  it finishes the fade in 12 channels, then submits actual two-channel buffers.
  The existing renderer/synchronizer is not stopped, flushed or recreated.
- Already rendered/submitted buffers are preserved and must play before the
  change is heard; this is not a zero-latency promise. No lossy re-encoding is
  added. Upmix/HRTF/gain processing itself is not bit-perfect passthrough.
- Rust tests cover repeat/reverse switches, arbitrary chunk sizes, exact dry
  recovery, source-frame counts, queue strides and preserved loudness state.
  Apple format transitions still require macOS/iPhone validation.
