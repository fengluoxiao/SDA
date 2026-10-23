# Accepted G80-derived iOS package

The user stopped further research and explicitly approved packaging the latter
40seconds of G80-then-global-clock-140-180.wav, including the earlier G+80ms
rendering combination. The research goal is paused, not declared scientifically
complete. This change packages that chosen listening version.

## Exact processing

- historicalInterpolationMode: aligned-pairwise. This is the aligned-waveform
  interpolation family with the consistent pairwise clock and no dominant-anchor
  insertion, matching the previously test-selected audition.
- spatialCueWindow: extended80. Keep the80ms spatial residual window.
- Same542-direction bank, including218horizontal-support directions, full height,
  mirrored triangle support and common fractional timing. Independent objects,
  continuous actual directions and existing head tracking stay enabled.
- Standard17-direction companion responses remain identical to the audition.
- No per-song/source-ID rules, playback prescan or analysis/cache files.

The offline-12dB headroom offset and shared export gain were listening/test
controls; they are not written into app output volume. Existing playback volume
and spatial-enhancement preference remain user-controlled. The approved audition
and parity check use spatial enhancement enabled.

## Packaging and verification

Based on fengluoxiao/SDA feat/android-port at b0cd545, the latest successful iOS
build, preserving the existing merged AC-4/head-tracking work. The upstream
cloud-jzh branch is a separate Android line and is not the deployment target.

The active experiment marker specifies aligned-pairwise and extended80. Shared
asset staging and actual iOS bundle validation reject a mismatch or a silently
missing policy field. Test coverage exercises both missing-field failures.

Validation completed:
- All15 HRTF tests passed, including manifest mode, snapshot/layout/gain updates.
  The three packaged DSP source files match the tested source exactly after
  line-ending normalization.
- All9 portable iOS resource tests passed; iOS spatial-enhancement checks passed.
- Release/non-test native-renderer library compilation check passed from this
  isolated packaging checkout. This is a Windows host check, not a local iOS SDK.
- All542dense+17fallback directions match the audition response files bySHA256.
- Actual staged iOS resource manifest, without diagnostic environment selectors,
  reproduces original140-180s versus the approved later-half source at
  max sample error1.4901161193847656e-7, relative residual-130.19495894365005dB.
  Independent15objects, direct/actual-direction flags and sampled guard1 checked.
- Source diff whitespace check passed. iOS compilation/signing/bundle verification
  runs in the explicitly dispatched GitHub workflow after push.

Audition SHA256:
24e52676dce26eb5acaf4b20f97b50c2441d7e38ccfec01d3cf0a88cfdc4fdba

No further auditory research or parameter exploration is part of this packaging
request. Earlier research limitations remain documented; user approval here is
for device packaging of this particular audition, not proof of every research
objective or every track.

## October16 CI repair

Run38763614905 failed compiling native renderer tests on SDK26: directional tests
referenced diagnostic_continuous_cues and SpatialEvent.coordinate_space from the
research checkout, absent in the packaging branch. SDK27 had no installed SDK
and skipped compilation; its green job was not a completed app package.

Remove unavailable research-only test fields/continuous-cue branch while retaining
the shipping moving-cue attenuation checks, both mixers and both layouts. Running
the CI regression groups also exposed a stale historical61 cue test: it compared
unconfigured nearest-speaker mapping with configured actual-direction lookup.
The corrected control removes residual excitation while retaining identical layout
and direction lookup, and separately requires exact equality for stored primary
and companion anchors. Array equality and nonzero residual checks remain strict.

Both changes are inside cfg(test) modules. No production DSP, processing policy,
response asset, or playback preference changed. Test-harness compilation succeeded;
137 distinct native tests from18 CI-related filter groups passed on Windows,
plus moving-source parity using the actual staged aligned-pairwise/extended80/542
manifest. Existing ignored diagnostics remain ignored; Apple-specific execution
still belongs to the newly dispatched macOS CI run.
