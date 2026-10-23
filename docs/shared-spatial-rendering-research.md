# Shared spatial rendering: feasibility investigation

## Scope and recommendation

Investigate improvements to spatial perception and layout coverage, not elimination of the 7.45e-9 sample discrepancy. Keep the accepted 7.1.4 and 360RA-13 outputs as reference regressions. Do not modify the currently building iOS renderer to include untested DSP.

Priority 1: a codec-independent directional residual provider shared by every native platform. Current spatial_cues::profile supports only two layouts although vbap::LayoutId supports 15. The restriction is in profile selection, not inherent in the decoder or mobile OS. The local SADIE D1 archive contains 50 BRIR WAV directions at 48 kHz, with elevations from -90 to +90 degrees. This supports prototyping broader coverage, not a claim of exact measurements at every speaker direction.

## Proposed architecture (not implemented)

1. Normalize decoder metadata into an explicitly documented coordinate space. Preserve distinctions between direction-only coordinates, normalized room positions and physical metres. Room and listener anchors must remain distinct.
2. Preserve the existing per-object actual-direction dry HRIR path and original channel/source gains.
3. Build an offline directional short-residual database from the licensed measurements. Preserve interaural time differences; perform pair-common arrival alignment rather than aligning ears independently. Audit energy, phase and unsupported interpolation regions. Do not simply linearly blend raw unaligned BRIRs.
4. Resolve requested speaker azimuth/elevation independently of codec or layout brand. Precompute filter banks when configuring layouts, not inside the audio callback. Retain the two accepted profiles as exact overrides initially, rather than silently changing their sound.
5. Keep early residual rendering shared across sources using existing bus convolution. Benchmark object-direction residual interpolation as a separate experiment; avoid one long room convolution per object.
6. Keep spatialization disabled for explicit unprocessed stereo/bypass modes. A two-channel recording without spatial metadata does not acquire genuine height objects from adding a response. Do not conflate virtual-speaker externalization with source separation or recovered Atmos information.
7. Do not require a flag whose semantics are mobile-only for cross-platform adoption. A future common capability/configuration should be asset-validated and explicitly opted into by Windows, Android and iOS; maintain backwards compatibility. No such migration is included in the current iOS build.

## Other feasible experiments

- Front/back and elevation: evaluate listener-compatible HRTFs with level-matched blind trials. Increasing direction count alone is not a substitute for matching the listener. Do not change the currently approved 128-direction default without evidence.
- Dynamic head pose: the core already accepts a head quaternion. Evaluate timestamped pose, coordinate calibration, smoothing and dry/reflected-field consistency through one cross-platform interface. Hardware acquisition is platform-specific and must remain optional. No sensor-less simulation or universal support claim.
- Weak harmonies: keep the full-minus-muted versus solo additivity regression, then measure time/frequency masking and transient preservation. Do not globally raise a selected vocal object or use a song-name special case. A contribution existing numerically does not prove it remains perceptually distinct in the full mix.
- Distance: separate front/back discrimination, externalization and metric distance. Reflection parameters may improve externalization, but do not reconstruct true authored metres where metadata contains only normalized coordinates.

## Primary evidence inspected

1. EBU EAR source: ear/core/point_source.py and allocentric.py. It provides separate point-source/allocentric panning and layout-specific allocentric positions. This supports preserving coordinate semantics, not copying a diagnostic 7.1.4 panner onto all codecs.
   https://github.com/ebu/ebu_adm_renderer/blob/master/ear/core/point_source.py
   https://github.com/ebu/ebu_adm_renderer/blob/master/ear/core/allocentric.py
2. SOFA SimpleFreeFieldHRIR specification: source positions, FIR samples and Data.Delay are distinct fields; delay is in samples. HRIR measurements are not arbitrary room simulation data.
   https://www.sofaconventions.org/mediawiki/index.php/SimpleFreeFieldHRIR
3. libmysofa README: filter lookup returns left/right impulse responses and separate delays; floating-point lookup delays are in seconds, unlike the SOFA stored delay field. Import normalization and interpolation are explicit choices. This is a reference for data handling, not a planned mandatory dependency.
   https://github.com/hoene/libmysofa
4. Valve Steam Audio C API: binaural point-source rendering, interpolation options and a separately described reflection effect/mixer. Documentation notes interpolation CPU cost and a possible convolution performance benefit from the reflection mixer. This supports evaluating split direct/shared-reflection architecture; it does not prove a drop-in engine replacement would improve music.
   https://valvesoftware.github.io/steam-audio/doc/capi/binaural-effect.html
   https://valvesoftware.github.io/steam-audio/doc/capi/reflections-effect.html
5. Brimijoin, Boyd and Akeroyd (2013), The contribution of head movement to the externalization and internalization of sounds. Their experiments found world-fixed virtual signals more frequently externalized than head-fixed signals. The result motivates testing head tracking, not a predicted percentage improvement for SDA music playback.
   DOI: 10.1371/journal.pone.0083068; full text PMC3846779.
   https://europepmc.org/articles/PMC3846779
6. Jenny and Reuter (2020), Usability of Individualized Head-Related Transfer Functions in Virtual Reality. Their audiovisual VR study supports perceptual benefits from individualized HRTFs, with task-specific results and limitations. It is not evidence that an arbitrary alternative HRTF will suit this user better than KU100.
   DOI: 10.2196/17576; full text PMC7509635.
   https://europepmc.org/articles/PMC7509635

The search tool returned no usable results in this session, so primary pages/source files and Europe PMC full-text XML were retrieved directly and saved locally in E:/SDA/artifacts/spatial-shared-research. Statements above distinguish source findings from proposed engineering decisions.

## Acceptance gates before rollout

- All 15 supported layouts: correct speaker coordinates, negative elevations retained, LFE isolated, no wrong-layout fallback.
- Exact measured directions retain original dry samples and interaural delays. Interpolated directions have finite nonzero energy and continuous motion; report spectral/ITD errors rather than a single waveform similarity score.
- Existing 7.1.4 and 360RA-13 reference replays stay within established floating-point tolerance with identical clocks, volume and flags.
- Full mix minus muted-source output equals its isolated contribution for the linear path. Separately audit nonlinear limiter/balance behavior.
- Seek, pause, preset and layout changes: bounded tail handling, no stale profiles, no clicks or blocking work in callbacks.
- Device benchmarks: median/tail render time versus actual callback budget, underruns, memory, battery/thermal cost. Host debug timing cannot substitute for iPhone/Android performance.
- Listening: matched loudness, concealed randomized conditions, separate front/back, elevation and externalization tasks. Do not equate more reverberation with better distance perception.

## Build maintenance during investigation

The first iOS run 37217646093 failed before executing the new regression assertions: the added workflow step did not export MACINDECODE_AC4_SPEC_DIR, so an AC-4 dependency build could not locate prepared generated tables. Commit 1fbfe41 adds that environment variable. iOS-only replacement run: 37218211175. No new experimental rendering algorithm is included in this workflow fix.
