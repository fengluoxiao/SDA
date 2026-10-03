# Dolby front-common center HRTF rendering

User listening decision (2026-10-02): the full-KU100 comparison that moves only
shared front-left/right content to center was substantially clearer and approved.
This changes that shared component's direction, not the HRTF measurement assets.

## Routing contract

- Applies to Dolby object programs using continuous directional HRTF on the shared
  native engine (Android and desktop), not ordinary stereo, MPEG-H or hardware output.
- Find a unique stationary pair at ADM front corners [-1,1,0] and [1,1,0] by
  geometry, never by decoder object IDs. Beds/ambiguous pairs are not transformed.
- Once associated, retain the pair through front-plane motion. Release it for
  rear/elevated/extended/diffuse/explicit-distance/exclusion-zone or removed sources.
- For post-envelope excitation L/R: M=(L+R)/2. Original convolvers receive L-M and
  R-M. M alone goes through full center KU100 directional HRTF at [0,1,0]. Thus
  pure differential content is unchanged; other objects and LFE retain their paths.
  Common content includes instruments, not a separated vocal stem.
- Replace the pair's common reflection excitation with center excitation. Do not
  stack another room path. Near-field and listener pose use the regular center path.
- All three dry paths use the same completed convolution partition, including
  parallel mixing and partial audio callbacks. Existing envelopes, sample clock,
  hardware trims, output EQ/compensation and peak protection remain authoritative.
- One broadband scalar matches group level. Start from coherent pair/center HRIR
  energy, then use accumulated common-input spectral power to estimate reference
  group energy. This is slowly slewed (one-second time constant), not per-band EQ
  or a fast vocal AGC. Silence does not update the estimate. The estimate resets
  on program/seek/HRTF/layout renderer resets; no song-fitted gain is hard-coded.
- Input-routing enable/bypass has a 512-sample fade; existing center tails drain.
  Speaker solo/mute monitoring bypasses the transform. No transport restart.

## Deliberate difference from the approved offline control

The offline control used one broadband RMS gain fitted over song seconds45-75.
Live playback instead estimates that same kind of group level causally from the
actual common excitation and neutral-frame HRTF transfers. It cannot know future
music; gain during startup or a new spectrum need not equal the offline fitted
constant. Real implementation audio must be checked against that reference.

## Validation

Tests live in apps/native-renderer/src/front_common.rs: decomposition, coherent
normalization, silence stability, ID-independent pair discovery, pure Side under
both mixers, ambiguous/non-Dolby bypass, partition timing, front motion, bypass
clock, and reset. Local copyrighted recordings/decoded PCM/replays remain outside
Git under E:/SDA/tools/blinding-lights-audit; they are not shipped as assets.
