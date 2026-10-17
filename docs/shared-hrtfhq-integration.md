# Shared HRTFHQ presentation

The user accepted the public-HRTFHQ response audition and explicitly requested the same presentation on iOS, Android, Windows and macOS. This supersedes keeping KU100/G80 as the default for this change. The old measured assets remain available. On2026-10-17 the user additionally approved the isolated half-strength retained61 short residual for front/rear motion and360RA overhead motion; this is now the shared default. It is separate from the old direct-HRTF corrections and later G80 preset.

## Implementation

- One bank under `assets/hrtf/apple-hrtfhq-v1`:542generic directions (including the existing218horizontal support), full-height coverage and17matching speaker fallback directions. The9song-scene diagnostic directions are excluded from production geometry.
- Captured on a GitHub macOS runner through public AVAudioEnvironmentNode HRTFHQ. No user music was uploaded. Run38924265715 / measurement commitd808534c105f51a89c9db68e85a646c2b0424d6e, macOS26.6.2 arm64. All555captured directions passed repeat/amplitude/source-count checks. The original551responses are byte-identical to run38914469033 used in the accepted audition.
- Captured interleaved responses become512tap planar stereo files. Only exact-zero tails are removed. One shared frontal calibration gain0.8035689911221481 and162sample common delay are applied to every response. No direction/ear/object/song-specific normalization.
- Runtime playback uses SDA's shared Rust convolution, not an Apple framework. This is not Apple Music's Atmos decoder, personalized Spatial Audio or a promise to reproduce every Apple OS version.
- The existing presentation recipe is applied by the common engine when entering this profile through initial loads or live preset changes: master0dB, world-coordinate auxiliary lift up to1.5dB, existing4kHz/+1.5dB high shelf, with the existing object-bearing E-AC-3+2dB compensation unchanged. The layer rule remains in world coordinates, outside head-relative HRTF coefficients. Same-family resource reloads preserve an explicit later override; track reset retains the preset.
- No decoder changes or rejected sparse-offset restoration. No user-side prescan, per-song analysis file, source substitution, object deletion or ID-based gain was added.

## Cross-platform interpolation repair

The first real Android/Windows comparison failed at a moving-source + head-pitch transition: maximum difference4.3483e-5, residual-63dB. Stationary and first-yaw segments already agreed near-131dB. A2e-6degree sweep across equal weights at45/23.84264degrees reproduced a0.00291relative waveform jump. Dominant-neighbour selection was not stable under platform floating-point rounding.

The measured profile now uses a pairwise weighted waveform clock, without a dominant-anchor fallback. Geometry is not quantized. Every measured anchor remains preserved, while the failing boundary and callback-partition tests pass. Existing KU100/historical policies remain separate.

## Actual host integration

- Both mobile packaging paths and desktop native packaging consume the same bank/fallback via the shared staging script.
- The desktop UI now selects HRTFHQ explicitly, migrates the old built-in default to dense/direct/continuous rendering, and reads HRTF assets from the sidecar's directory instead of an older web copy. Explicit human/personal selections are preserved. Retained KU100 aliases keep the old reference data available.
- iOS/Android migrations enable the accepted shared presentation and actual-direction defaults once. iOS's previously selected system-rendered output defaults are cleared for the new shared preset. Subsequent explicit user overrides remain possible.
- Merged upstream Android commitsc4a9b93 and3d0066e, preserving their ALAC controls, Liquid Glass UI and360RA startup ordering. Android enhancement now reads its persisted setting after the shared-profile migration; it is not hardcoded in the settings result.
- Inactive G80-only controls are hidden for this profile. Format identification (Atmos/360RA/stereo) is unchanged.

## Stale-build protection

The measured manifest uses `measuredResponseVersion:2` plus `sharedDepthVersion:1` and deliberately omits the legacy `calibrationVersion`. A pre-HRTFHQ native renderer was tested and correctly rejects it. It cannot quietly apply its old generic interpolation to the new bank.

Android Gradle and APK verification reject JNI libraries lacking the measured-response loader. The normal Android workflow rebuilds both ARM64 and x86_64 from source before assembling the APK. The checked-in prebuilt libraries inherited from the Android merge are old and must be rebuilt for a local native APK build. Desktop packaging similarly rejects an old sidecar or web bundle lacking the shared-profile migration. Do not distribute only new response files with older native binaries.

## Verification and current limits

- Four staged resource families byte-identical; hashes, complete direction coverage, triangle order and17speaker fallback checked. Nine iOS bundle-validator tests passed.
- Actual Windows/macOS native test binaries, iOS simulator binary and Android emulator binary rendered the same deterministic PCM with yaw/pitch, source movement, Atmos objects,360RA objects,7.1.4beds and stereo. CIrun38931313811 (cda3b3a) passed12cross-platform comparisons, maximum sample error1.862645149230957e-8 and worst residual-126.815dB. The final merged/default-path revision requires the fresh follow-up CI recorded below.
- Current local native tests cover all542anchors, matching fallback, incompatible-policy rejection, head pose, equal-weight continuity, automatic-vs-explicit presentation, track reset/live preset handling, same-family override preservation and callback partition invariance.
- Actual shared playback versus the user-approved exact-position public reference over original2-10s: RMS difference+0.05678dB, spectral-band level differences below0.088dB, normalized dot product0.999798. This is close but not sample-identical, owing to the shared generic direction interpolation; no per-song fitting was used.
- Six additional songs were continuously replayed through old and new profiles. Outputs were finite and the diagnostic-12dB runs did not activate the guard. This is a new HRTF presentation: relative tone and local L/R values change. In particular, the700-1500Hz band differs from the old KU100 mix by roughly1.8-5.1dB after equal-energy comparison, and some local balance windows differ more. These are not certified subjective improvements or evidence of inaudible regression. Existing reconstruction PCM remains the accepted baseline.
- Four-runtime comparisons concern engine output with the same input/layout/pose/settings, before device/OS/headphone effects. They are not four physical-device listening certifications.
- Web TypeScript checking passed after setting up ignored local workspace links and existing generated WASM declarations. No new runtime WASM binary was copied from the research checkout; normal release workflows rebuild runtime artifacts.

## Reproduction and evidence

- `scripts/build-shared-hrtfhq.py`: response-bank construction from the synthetic measurement artifact.
- `.github/workflows/shared-hrtfhq-parity.yml`: actual runtime fixture builds and the four-platform comparison gate.
- `scripts/compare-shared-hrtfhq-pcm.py`: maximum error<=2e-6 and residual<=-100dB, no signal alignment/gain fitting.
- `scripts/test-desktop-shared-profile.mjs`: default migration, selection, matching sidecar asset root and path validation.
- `scripts/verify-android-render-experiment.py`: actual APK checks, plus optional IPA/APK byte comparison.
- `assets/hrtf/apple-hrtfhq-v1/README.md`: official Apple documentation links and capture conditions.
- Local evidence: `C:/Users/legendshop/Downloads/SDA-clarity-validation/egaku-apple-reference-20261016/`, especially `platform-parity/`, `shared-profile-final-render/`, `shared-profile-default-render/`, and `shared-profile-regression/`.

No new formal installer has yet been declared complete in this checkpoint. Source/runtime/package validation and listening acceptance must be reported separately.


## Approved depth integration — 2026-10-17

The shared engine now adds the exact auditioned residual at half strength, with the same fixed common calibration. Existing layout-specific reflection buses are reused; no second object engine, source scan or cache is added. Measured HRTFHQ direct responses, full-height direction support, interpolation, JOC input processing and head tracking remain in place. The35ms retained61 residual must not be confused with G80. No audition-only loudness matching is in runtime playback.

Actual integrated output was compared against the stored approved offline candidates, with no time alignment or gain fit. Four trajectories passed: Atmos front/rear,360RA front/top/rear, bottom/top and upper-left/top/upper-right. Maximum sample error3.968e-9; worst residual-127.49dB. This establishes equivalence to those synthetic auditions, not acceptance of every song or device.

Both response manifests now require schema2/depth1, so older native loaders reject them. Android JNI and desktop sidecar package checks require the new depth loader marker. iOS smoke checks expect the separate depth layer while the legacy processing flags remain off. The residual banks are compiled into the same native crate on iOS, Android, Windows and macOS.

Local integration validation:326 native engine tests passed,0failed,33ignored offline/fixture tests;9iOS resource tests passed. Shared four-platform staging bytes, desktop integration, mobile preset, head-tracking and Android settings/ALAC checks passed. Fresh four-runtime parity CI is required for this revision; earlier v1 parity numbers above do not validate this new revision.
