# Authored object routing: backing-vocal audit

## Scope
The user reports missing upper-left/right harmonies in *The Very First Night*,
particularly 0–38 seconds, compared with Apple Render. The isolated upper-front
object group (16/22 in this capture) contains the reported harmony; other groups
may contribute. No Apple reference capture is available. Object IDs are diagnostic
labels for this segment, not permanent speaker assignments.

## Confirmed defect and fix
`FrontCommon` previously defaulted to enabled. It extracts the common component
of a stationary front pair, routes it through an invented center, and applies a
spectrum-dependent gain. Signal correlation is not authored metadata and cannot
identify which shared content is lead vocal, harmony, or ambience. This changes
the authored multi-object mix even with per-object and actual-direction rendering.

Normal playback now defaults to bypassing this experiment. Each object retains
its own route; explicit diagnostic opt-in remains available. This is shared native
renderer code for Android, iOS, and Windows, effective after rebuilding each app.
No song-specific gain, object-ID routing, height boost, decoder modification,
room simulation, near-field processing, or HRIR replacement is introduced.

## Evidence and regression coverage
- The new authored-route test failed before the fix (maximum PCM difference
  0.008369803 against explicit bypass), and passes after it.
- All 11 front-common tests pass. New tests cover desktop dense and mobile
  128-direction assets, serial/parallel mixers, callback chunk independence,
  no invented center, and additive quiet upper-front object contributions below
  the UI activity threshold. Explicit opt-in tests remain intact.
- Full renderer library suite: 203 passed, 2 failed, 21 ignored. Both failures
  (`shared_reflections_match_independent_objects_with_near_field_and_focus` and
  `object_depth_loudness_follows_adm_distance`) reproduce with the pre-fix
  `FrontCommon` source, so this patch does not claim a clean full suite.
- Independent FFmpeg core PCM supplied to the current JOC reconstructor leaves
  upper-front objects substantially unchanged (correlations 0.999552/0.999140).
- Verified OpenJOC v0.16.0 release reconstruction agrees closely with current
  object PCM (upper-front correlations 0.999947/0.999897). Agreement with a
  published-equation implementation is not an Apple/vendor ground-truth check.
- Fixed shared MobileEngine capture renders all 1,923,072 frames (40.064 seconds)
  without decode errors or non-finite PCM. Peak 0.891251; left/right RMS
  0.103602/0.112001. Before/after PCM differs; this is not an audibility metric.
  Capture uses a Windows host and mobile HRIR, not a device/package recording.

## Limits and delivery
The unintended recentering is fixed, but perceptual recovery of the exact harmony
and parity with Apple Render are not established. Nonzero object contribution
alone does not prove correct audible balance. Audio and song-specific tools stay
outside the repository. iOS CI now runs the object-route regressions; only iOS
build dispatch is intended for this change.
