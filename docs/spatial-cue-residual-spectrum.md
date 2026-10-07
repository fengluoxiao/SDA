# Bundled spatial-cue residual spectral shaping

Accepted and integrated into shared native source on 2026-10-07 after 360RA, Ether and MONTERO A/B listening approval. Not a song-specific correction or an Apple renderer emulation.

## Processing scope

After existing rear-residual balancing, apply the same centered 17-tap normalized binomial low-pass blended with the original residual. High-frequency limiting gain is 0.5011872336272722 (-6 dB); DC gain is unity. This is a gradual shelf (approximately -0.58 dB at 2 kHz, -2.07 dB at 4 kHz and -5.18 dB at 8 kHz), not a hard crossover. It acts on bundled 7.1.4 and 360RA cue profiles only. No song, object ID, source level or time-window selection exists in this policy.

Direct HRIR arrays, authored positions/gains, 61-direction interpolation, existing rear balancing, and MotionCues speed/attack/release are unchanged. The FIR is baked during cached profile preparation, not applied to live input audio. Residual support stays inside the original 5-80 ms window with zero extension and clipping at its boundaries; no added playback delay. Filtering may change residual interaural fine structure; identical ear coefficients do not mean all spatial cues remain numerically unchanged.

Existing spatial-cue gain controls still multiply the shaped residual. The new -6 dB high-frequency limit is separate from the existing overall spatial-cue gain. Custom user room profiles are untouched. Disabled/unsupported bundled-cue paths remain unaffected. Shared-native integration does not update already installed binaries.

## Verification

- Native library tests: 234 passed, 0 failed, 21 ignored (Windows GNU, default output-device features disabled). Not an iOS device test.
- Tests cover both-layout direct preservation, bypass, finite values, time support, ear exchange, nonincreasing residual energy, stage order, and existing motion/live cue regressions.
- Exported all speaker dry/wet filter pairs in both layouts from the accepted audition snapshot and merged source: byte-identical JSON, SHA256 `3793e9ba2dfc399107353ff0183cab08d772d58f4fe944ee42aa7ee00988ceff`.
- Only spatial_cues.rs changed among the native source/assets recorded before the experiment; other pre-existing working-tree edits were not reverted.
- Listening approval is limited to the auditioned material; numerical tests are not a universal perceptual guarantee.

Evidence directory: `C:/Users/legendshop/.codex/tmp/sda-cue-hf-20261007/` (merge-report.json, merged-tests.log, bank-accepted.json, bank-merged.json and cross-track/report.json). No packaging, push or deployment performed as part of this integration.
