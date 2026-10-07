# Shared directional masking compensation — 2026-10-07

## Policy correction - 2026-10-08

The default-on policy below is withdrawn. `SpatialBalance::default` now disables
foreground attenuation: normal playback preserves authored source excitation.
The opt-in implementation remains for diagnostic comparisons, not as an HRTF fix.
The replay harness respects the engine default unless explicitly overridden.
Prior listening approval does not establish faithful rendering when other source
levels change. Independent directional changes are not reverted.

## Previous shipping policy (superseded)

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

## Authored-level baseline checks (2026-10-08)

The default-path regression feeds a front-dominant scene for 1,000 blocks and
requires exact preservation of both front and upper source samples. Diagnostic
opt-in attenuation tests remain explicitly enabled, separate from defaults.

Replayed the How Do I Make You Love Me fixture without setting the balance
environment override: 480,768 stereo frames, finite, peak guard gain 1.0.
Default output differs from the earlier explicit-bypass baseline by at most
2.98023e-7. Separately rendering the original foreground group (LFE, Obj10/11)
and its exact complement (Obj12..24), with unchanged metadata and no gain
adjustment, reconstructs the full render: max absolute residual 2.60770e-7,
relative residual -133.23 dB. All three runs report peak guard gain 1.0.
This excludes significant non-additivity between these groups in this fixture;
it does not validate individual spectral transfer, motion or perceived height.
The target clarity issue remains open, not solved by this policy rollback.

## Subsequent interpolation correction

The authored-level rollback above is retained. A separate measured defect in
the historical KU100 dominant-neighbour interpolation was subsequently fixed,
without a source-level masking policy. See
`hrtf-continuous-interpolation-validation.md` for the boundary, spectral, timing,
full-suite and actual-fixture checks and their listening limitations. This does
not turn the earlier attenuation/remix experiment into a valid HRTF fix.
