# KU100 elevated boundary selection

The calibrated KU100 interpolation guard can replace an interpolated response
with the dominant measured HRIR. Selecting the largest manifest index when
weights tie made mirrored elevated directions choose non-mirrored measurements:
the front upper left chose +60 degrees while the right chose -30 degrees.

The shared Rust renderer now resolves elevated weight ties (within 1e-6) by
absolute measured azimuth, then elevation. This preserves the established left
hemisphere convention on its mirrored counterpart. It is a deterministic
boundary convention, not a claim that a more lateral measurement sounds better.
Unequal weights retain their strongest measurement. Horizontal ties retain the
existing rule: changing those would alter the dominant music channels that
already match the staged Windows renderer.

No HRTF asset, gain, room setting, near-field setting, or output backend changed.
This does not eliminate discontinuities at all measurement-cell boundaries.

Validation:

- Four elevated mirror pairs match after swapping ears (relative error below
  0.001); f32 perturbations, weight/input ordering, horizontal preservation,
  unequal weights, and same-X front/back distinction pass.
- Final directional suite: 23 passed, seven offline diagnostics ignored.
  One existing shared-reflection equivalence test was excluded only after both
  the original and changed selection rules failed with exactly 0.0040866397
  maximum PCM difference. This remains an unresolved pre-existing failure.
- Second song, 2–12 seconds, host probe with calibrated dense KU100, direct
  objects, actual direction, near-field and studio room enabled: RMS against
  the staged Windows capture fell from 0.0016286674 to 3.3712e-8; maximum
  difference after the correction was 2.6822e-7.
- Android release probe built from the same source, same assets and all-enabled
  settings: RMS versus staged Windows was 8.4547e-8, maximum 8.1956e-7;
  RMS versus the updated host probe was 8.4336e-8. These are pre-output probe
  captures, not a recording of the APK's Media3 output.

Delivered `E:/SDA/apk/SDA-android-ku100-elevated-fix-x86_64.apk`, SHA-256
`76528c5c7c3fbcd5f5609aaa2a9483c2b436e8fa069881437f4bc810842983f5`.
All allocated native-library ELF sections match the built release library.
Installed with data preserved on MuMu and verified a successful cold launch.

The staged Windows executable is a numerical reference, not the user's accepted
listening reference. These measurements do not establish that the whole-song
front/back listening complaint is resolved.
