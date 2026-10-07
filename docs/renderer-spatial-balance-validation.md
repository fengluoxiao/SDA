# Shared directional masking compensation — 2026-10-07

## Shipping policy

Enable the listener-accepted H policy in `Engine::new` via `SpatialBalance::default`.
All native hosts inherit it; no iOS-only DSP and no song, timestamp or object-ID rules.
It attenuates eligible front excitation by at most 6 dB when upper-source energy is
masked, with 150 ms attack / 800 ms release. It never boosts source excitation,
changes source positions, or modifies the restored 61-direction HRTF/spatial cues.
This is relative-mix compensation, not a demonstrated HRTF calibration correction.
Fixed bus beds, legacy/direct convolution and hardware rendering are not covered.
Front-common, speaker solo and mute modes do not request compensation.

## Local validation

- Native library: 240 passed, 0 failed, 22 ignored, serial GNU Windows host run.
- Six policy tests include default-on, bounded attenuation, silence/front-only
  bypass, synthetic source preservation and full-sphere geometric symmetry.
- 2,664 directions at 5-degree intervals, elevations -90..90, azimuths -180..175:
  finite non-silent interpolated responses. Sampled frequencies 500..4000 Hz.
  Largest loss versus weighted measured-neighbour power: horizontal front 0 dB,
  rear 0.421 dB, left/right 0.654 dB each, upper 2.049 dB, lower 3.146 dB.
  Lower worst sample: azimuth -105, elevation -85, 4000 Hz. This metric is not
  a perceptual clarity test; lower-region response needs separate investigation.
- Ether decoded PCM/metadata excerpt: both replays pass, no nonfinite samples.
  Enabled output RMS changes -3.19/-3.22 dB, correlation .9821/.9824.
- Existing 360RA decoded PCM/metadata excerpt: both replays pass, no nonfinite
  samples. RMS changes +.140/+.125 dB, correlation .9817/.9815. This harness uses
  its existing wet/peak-guard policy, so output RMS is not source attenuation.
- These are offline numerical regression checks, NOT listening approval of the
  other songs or physical iOS validation. The new policy intentionally changes
  relative mix on other eligible scenes; do not claim bit-identical music output.
- Previously accepted H audition and its coefficients are preserved. No HRTF
  asset edits; no other platform packages requested or produced by this change.

## Reproduce

`cargo test --manifest-path apps/native-renderer/Cargo.toml --lib --no-default-features -- --test-threads=1`

For the ignored `audit_interpolated_voice_band` test set `SDA_SPECTRAL_AUDIT_OUT`
to an output JSON path. Local music fixtures are not committed. Atmos replay uses
`SDA_ATMOS_REPLAY_SPATIAL_BALANCE=0/1`; MPEG-H probe uses
`SDA_SONG_PROBE_SPATIAL_BALANCE=0/1`. Set front-common off for the accepted policy.
