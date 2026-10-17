# Front near-ear isolation integration — 2026-10-16

The user accepted `front-cross-ear-isolation/near-ear-only-10-29.wav` and requested renderer integration. That audition used the original stereo reference through measured G80 near-ear kernels, not the complete Atmos scene. Integration therefore preserves that transfer at the tested front directions, but does not claim the complete object mix is identical to the stereo reference.

## Implementation

`processing.frontEarIsolation` is enabled in the active 542-direction shared asset. Old manifests default to false. The shared staging validator checks the policy against active.json for iOS, Android, Windows and macOS. No user setting or prescan is added.

The geometry-only `front_ear` policy leaves the near-ear samples unchanged and attenuates the opposite-ear response. Positive SDA azimuth is left. The opposite ear reaches zero at +/-30 through +/-105 degrees at elevations within +/-15 degrees, including the audition's +/-45-degree pair. Smoothstep transitions preserve both ears at center, behind +/-135 degrees, and beyond +/-45-degree elevation. Object extent/diffuseness reduces isolation. These transition ranges are generalization choices, not additional listening-approved measurements.

Objects use listener-relative position, including head rotation. The policy runs after diffuse calibration, before output preparation, without renormalizing the retained ear. Bed/speaker filters apply it after direct/room/cue composition; reflection buses subtract already-treated dry/wet filters, so a front residual cannot restore the removed opposite-ear branch. Shared HRTF preparation snapshots retain the policy for asynchronous preparation. No object-ID, title, codec, artificial delay, ordinary stereo injection, or source-separation dependency is involved.

The change intentionally reduces binaural cross-ear cues in the affected region. G80, 542 directions including the 218 support directions, triangle interpolation, object routing and head tracking remain. Their continued operation is not a guarantee of unchanged spatial perception.

## Validation

- Renderer `cargo test --no-default-features --lib`: 317 passed, 31 ignored, zero failures.
- New tests cover exact retained-ear samples, complete opposite-ear tails, symmetry, continuity, center/rear/height identity, head rotation, bed/object preparation and snapshots.
- Shared experiment staging: four destination layouts byte-identical; fallback/triangles verified; corruption rejected; packaging entry contracts passed. These are staging checks, not four newly built application binaries.
- iOS mobile asset tests: 9 passed.
- Actual settled impulse replay, left/right front: retained-ear errors -131.19/-131.58 dB versus approved baseline; opposite-ear max abs 4.82e-11/5.18e-11.
- Full 83-second decoded object/metadata replay: 16 sources (15 objects plus LFE), direct rendering retained, spatial cues active throughout, diagnostic peak guard 1.0, finite output. Exported song 0–82 and 10–29 seconds with fixed -22 LUFS audition gain; this export gain is not a runtime setting. Four-times oversampled export peaks below -4.14 dBFS.

Local evidence: `C:/Users/legendshop/Downloads/SDA-clarity-validation/2step-delay/front-ear-integration/validation.json` plus `front-ear-tests.log`, `front-ear-ios-assets.log`. Research-only replay helper edits were already present and are not included in this change.

Physical iPhone/headphone verification and perceptual regression on other accepted songs remain unverified. This commit integrates the requested policy; it does not certify all songs' delay audibility or claim a new iOS package was built.
