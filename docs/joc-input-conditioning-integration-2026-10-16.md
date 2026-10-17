# JOC input conditioning integration — 2026-10-16

The user requested integration and an iOS build after auditioning the complete
Egaku candidate and receiving cross-song comparisons. The shared E-AC-3 decoder
now applies the same input conditioning and cumulative sparse reconstruction to
48 kHz, five-fullband-input streams signaling downmix configuration 3. Other
configurations keep their previous behavior. No song/object IDs or file analysis
are used. No audition loudness-matching gain is applied in playback.

## Signal and timing

- Delay front QMF inputs by 10 slots; delay and rotate surround bands 1–63 by -j.
- Process surround band 0 with the auditioned 21-tap complex FIR.
- Accumulate sparse channel indices and scalar coefficients across bands, with
  unselected entries initialized to the zero quantization point on this path.
- ObjectPcmFrame reports 1217 samples of latency on this path, 577 otherwise.
  Core/LFE delay and OAMD event timestamps use that same format-dependent value.
- Reset, discontinuous sequences, configuration changes, and non-JOC boundaries
  discard conditioning state along with existing QMF/matrix history.

The existing G80/542 bank, 218 horizontal-support directions, full-height support,
mirrored triangles, common timing, actual object directions, and head tracking
are preserved. This commit descends from the existing front-near-ear-isolation
integration 3491058 and does not alter that policy or its assets.

## Verification

- 61 E-AC-3 library unit tests passed, including added conditioning polarity,
  cumulative sparse low/high quantization, latency gating, companion delay,
  reset/discontinuity, and unchanged dense coefficient arithmetic checks.
- The production StreamingDecoder was compared to the offline candidate on
  Egaku, How Do I Make You Love Me?, The Very First Night, Lose Yourself to Dance,
  Wonderful World, doll, and Hitoshizuku. Every object and LFE sample matches
  exactly (maximum PCM difference 0). A second pass after public reset with
  different compressed-byte chunk boundaries also matches exactly.
- Event timestamps, labels and object declarations match. Floating position
  fields differ only by JSON decimal readback precision (reported in local
  parity JSON), not by sample timing or placement changes.
- Shared asset staging checks passed for all four platform destination layouts;
  head-tracking source-contract and session-handoff tests passed.
- The iOS workflow runs the same 61 decoder library tests before its existing
  native host/device build checks. The standalone test runner retains the exact
  library sources and fixture files, avoiding the unrelated upstream bridge
  workspace's external Omniphony requirement.

Local parity artifacts: Downloads/SDA-clarity-validation/joc-integration-20261016/.
The standalone comparison helper lowered optimization only for its unrelated
TrueHD dependency to fit Windows compilation resources; the E-AC-3 code was
compiled at release optimization. App build profiles are unchanged.

## Limits retained from the audition report

The six additional tested song prefixes are dense-only; sparse music coverage
comes from Egaku. Wonderful World and doll changed measured excerpt loudness by
+1.028 and +1.306 LU, and Wonderful World has a local +1.455 dB R-L change at 167s.
Those observed changes were reported before the user requested integration and
were not concealed with runtime gain correction. Device listening is still needed.
The reference implementation is not an official Dolby conformance reference;
its revision and MIT notice are retained in the decoder submodule.
