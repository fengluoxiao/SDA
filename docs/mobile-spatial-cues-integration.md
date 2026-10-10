# Production integration: retained spatial cues

## Included

- Authored independent object routes; central relocation remains off (base commit 52ef1d0).
- Bandlimited fractional-delay HRIR interpolation replaces HF-attenuating two-tap interpolation.
- Audio emitters no longer occlude other audio emitters without physical scene geometry.
- The exact retained 7.1.4 depth/upper-response profile and 360RA-13 lower-response profile are embedded in the shared Rust library. SHA-256 and measurement attribution are in apps/native-renderer/assets/spatial-cues.
- Mobile manifest explicitly opts in with processing.spatialCues. The 128 raw KU100 measurements and zero wet files remain unchanged. Residual support is 5-80 ms, with no late tail. This is an audible short-reflection path, not completely dry playback.
- Per-object and actual-direction rendering stay enabled; generic room and near-field controls remain off. Status distinguishes spatialCuesEnabled from roomEnabled.
- Preset reload, room bypass and supported layout switches restore the cues. Unsupported layouts do not receive an incorrectly mapped profile.

## Validation

- Shared renderer suite: 212 passed, 0 failed, 21 ignored before adding one additional preset-switch regression; the final four spatial-cue tests separately pass.
- Mobile front/rear, upper/lower and normalized-radius tests: 3 passed.
- iOS host ABI tests: 8 passed; MPEG-H tests: 3 passed, run serially because the decoder has exclusive ownership. An initial parallel whole-crate invocation caused seven ownership conflicts; these were rerun serially rather than hidden.
- Mobile runtime test confirms 128 directions, spatial cues on, object and actual-direction on, legacy room/near-field off.
- iOS bundle tests: 5 passed; preset, UI and playback-polling scripts passed.
- 360RA supplied-track production replay: 1,920,000 stereo frames, 24 continuous objects, no decoder errors, finite/nonzero both ears, peak 0.0182846. Compared at identical frame coordinates against the retained short-residual audition: max sample error 7.45058e-9, relative RMS error 2.18241e-7. No rescaling or time realignment was used for this comparison.
- The Very First Night production replay: 1,923,072 stereo frames, 15 continuous objects, cues enabled, object and actual-direction enabled, legacy room/near-field off; no decode errors, finite/nonzero both ears, peak 0.0095734.
- iOS simulator smoke now checks runtime cue activation and rendering flags, not only bundled file presence. A Mac CI build is still required to validate the Swift/device artifact.

## Boundaries

The early The Very First Night B/D and depth auditions used a diagnostic EAR 7.1.4 speaker-bus path with direct flags off. That diagnostic routing has NOT replaced production actual-direction rendering. The retained response profiles are included, but production PCM is not claimed to be bit-identical to those earlier bus auditions. No Apple renderer data is used, and Apple-level harmony clarity has not been proven. Filter separation and signal checks do not establish subjective externalization for every listener.

This change is shared-core, not an iOS-specific song workaround. Android uses the mobile asset; Windows integrators must deliberately use the compatible mobile KU100 manifest with spatialCues enabled to obtain these bundled profiles. Legacy desktop manifests do not silently acquire them. This task builds iOS only; no Windows or Android build is dispatched.

No user song, decoded PCM or listening WAV is committed or uploaded.
