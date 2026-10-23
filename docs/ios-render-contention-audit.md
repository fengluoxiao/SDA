# iOS Render Contention Audit

## Evidence

Physical-device snapshots showed decoder coverage several seconds ahead while
callback missing frames increased. The audio engine remained running. A later
7.1.4 E-AC-3 session had a 309 ms render block split across fast mix (104 ms),
sample processing (96 ms), and directional processing (99 ms). These are elapsed
times, not CPU-only measurements. They cannot independently prove preemption,
thermal throttling or a particular slow DSP function. Previous MPEG-H and E-AC-3
samples differ in codec/thermal conditions and are not controlled benchmarks.

## Confirmed Code Costs

- mix_continuous_objects allocated a controls Vec per render chunk. Local change
  retains the capacity in Engine and clears/reuses it, preserving control order.
- Grid::diffuse_field held its shared cache mutex while performing twelve-direction
  interpolation. Local change builds outside the mutex and rechecks on insertion.
  Concurrent misses can duplicate construction; the change trades duplicate work
  for avoiding unrelated-source lock blocking. It is not a guaranteed speedup.
- Source PCM rings reserve 480000 Slot entries per source. Slot contains u64 and
  f32, typically 16 bytes with alignment: approximately 7.68 MB per source,
  768 MB for 100 sources, excluding filters/queues/UI. Current source counts and
  actual resident memory of the device session have not been measured. No ring
  capacity changes were made: shrinking could break decoder lookahead semantics.
- One-group object mixing already runs inline in the previous candidate. iOS
  two-worker cap remains an unproven device scheduling candidate, not a fix proven
  by Windows output parity tests.

## Local Verification

25 tests passed: six diffuse-related tests including concurrent cache misses;
moving bundled cues across both layouts/mixers; eight convolution tests; hardware
parallel mixing; spatial-layer parallel parity; pure-side parity; two worker
policy tests; five absolute PCM ring tests. Windows GNU host, debug build.
No ARM scheduling/thermal performance improvement is claimed from these tests.
No sound parameters, buffer target, filter length or head-pose policy changed.

## Build Policy

The audit and two local changes were kept local during investigation. The user
subsequently requested submission together with the adjacent chat's theme-color
corrections; they are included in one consolidated candidate build.
Remaining device evidence needs build identity,
active source count, resident memory, per-thread CPU versus elapsed time, and
controlled same-codec comparisons. Consolidate any required measurement and
validated fixes into one candidate build, rather than one build per metric.
