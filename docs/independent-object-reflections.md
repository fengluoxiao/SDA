> STATUS: Withdrawn from the default implementation after the user rejected preview G.
> Source snapshot: `E:/SDA/artifacts/ether-orbit/G-independent-reflections-source-snapshot`.
> The production path has returned to F; the following records the withdrawn experiment, not current behavior.

# Independent object reflection histories

## Scope

With continuous actual-direction object rendering enabled, each decoded object
now owns both its existing direct HRTF convolver and a private reflection bank.
The bank contains the existing layout-specific residual responses, but **PCM,
convolution history, hardware state, background-filter state and decay tails are
not shared between objects**. Only completed binaural outputs are summed.

This is not a new acoustic model, source separation, or a claim of Apple renderer
parity. Reflection directions still use the existing calibrated layout response
basis and object routing gains. The direct HRTF remains interpolated at the
object's actual direction. Beds and LFE retain their existing paths.

## Implementation

- `ContinuousSource` owns its reflection renderer. It is initialized together
  with the source, not in the per-sample loop. HRTF/layout rebuilds already
  discard and recreate source renderers.
- The general and source-major mixers send the object's distance-scaled
  reflection excitation to that source, not the shared reflection bus.
- Reflection state runs through silence to preserve tails; near-field/occlusion
  processing of direct sound does not inadvertently filter the reflection tail.
- Object reflection banks allocate no unused bed convolvers. They run serially
  within the existing per-object parallel worker to avoid nested worker fan-out.
- No new room simulation is enabled. Existing residual strength, rear balancing,
  elevation cues and direct interpolation are retained.

## Regression coverage

- Independent full reflection output versus the legacy per-object full response,
  including near-field correction, focus filtering and audible tail decay.
- Two independently moving reflection inputs versus the former linear shared
  reference: maximum-error threshold 2e-6. A third unexcited object's reflection
  output must remain exactly zero.
- Existing general-versus-parallel, object mode-switch, hardware, bed, metadata
  trajectory and HRTF tests remain part of the full suite.

## Release boundary

This change increases per-object memory and CPU costs. Host synthetic benchmarks
are not mobile-device real-time certification. No iOS/Android/Windows release
package has been produced for this change. The previously accepted Ether F WAV
has not been overwritten and does not contain this new architecture.

## Host results

Final optimized suite: 221 passed, 0 failed, 22 ignored (offline diagnostics).
Synthetic mobile-asset 7.1.4 benchmarks, 48 kHz / 1024 frames, ten additional
beds, room/near-field off:

- 15 moving objects: mean 4.40 ms, p95 5.70 ms, max 7.20 ms.
- 24 moving objects: mean 7.14 ms, p95 8.83 ms, max 9.73 ms.

Dense 108-object + ten-bed 9.1.6 reference benchmark (different assets and
near-field enabled): mean 28.25 ms, p95 31.62 ms, max 35.85 ms. This **exceeds**
the 21.33 ms block budget; a passing benchmark test only checks finite output,
not deadline compliance. Dense-scene CPU/memory optimization remains necessary
before claiming this architecture release-ready across all workloads.

Logs: `E:/SDA/artifacts/ether-orbit/independent-reflection-final-tests.log`,
`independent-reflection-mobile15-benchmark.log`,
`independent-reflection-mobile24-benchmark.log`, and
`independent-reflection-final108-benchmark.log` in that directory.
