# Continuous historical KU100 interpolation, without remixing

## Defect and scope

The historical KU100 interpolator chose a dominant measured neighbour, aligned
all neighbouring waveforms to it, and blended a protective anchor when coherent
energy fell. The geometric weights were continuous but the reference waveform
was not. Crossing a neighbour tie could change the impulse response abruptly,
even for a 0.0002-degree direction change. An azimuth sweep at elevations
-30..85 found 408/1,728 sampled boundaries with relative waveform change >0.01;
the maximum was 1.636924 at azimuth 105, elevation 25. At 105/40 it was about
1.57995. The decoded music metadata includes upper directions near this region.
This proves a generic interpolation defect, not identification of a particular
vocal stem and not proof that all perceived vocal masking came from this defect.

The correction applies to the shared historical KU100 direction interpolator,
not a platform, song, object ID or time range. Other subject packs keep their
existing waveform interpolation. Ordinary loudness normalization (the user
volume-balance setting) is not disabled. The separate default-on *spatial masking
compensation*, which attenuated front sources by up to 6 dB, is disabled: it was
a remix, not a rendering correction.

## Implementation

- Cache each measured ear response's FFT power and frequency-unwrapped phase
  at pack initialization. Route updates reuse the bank and inverse FFT plan.
- Using the existing continuous spherical weights, interpolate power and phase
  on the common measurement clock, then reconstruct a real impulse response.
  There is no dominant-neighbour pivot or cancellation fallback in this path.
- Use weighted real DC/Nyquist bins and conjugate symmetry. Reconstruct with a
  2,048-point inverse FFT for the bundled 512-tap measurements; retain the
  existing 608-tap output length. Do not renormalize programme or source levels.
- At exact measurements, return the original samples directly, with the same
  zero tail. The original 61 assets and separately measured 13 lower anchors
  remain unchanged. The lower support handover remains -30..-60 degrees.
- Existing spatial-cue residuals, user cue gain, occlusion, near-field controls,
  source PCM, metadata gain, layout and transport are not changed.
- Interpolated responses are intentionally different. Preserving measured
  anchors does not mean bit-identical output at arbitrary directions.

## Checks performed

GNU Windows host, native library, no audio-device feature:

- Full suite after final timing/energy assertions and formatting: **246 passed,
  0 failed, 25 ignored**. Three mobile front/back/upper/lower integration tests
  also pass.
- Boundary regression sweeps azimuth -180..175 and elevation -90..90 in 5-degree
  increments, testing both azimuth and elevation perturbations. Includes poles,
  the azimuth seam, lower support handover and previous dominant ties.
- Matched 1,728-boundary before/after azimuth audit: maximum relative change
  falls from **1.636924 to 0.000266588**; sampled changes >0.01 fall from 408 to 0.
- Exact original 61 anchors and additional lower anchors, left/right symmetry,
  finite energy, nadir azimuth independence and handover checks pass.
- 648 off-grid directions spanning upper, horizontal and lower regions:
  per-ear low-band arrival and interaural phase stay within one sample of the
  weighted measured phase at 492 Hz; per-ear energy within 0.2%; sampled
  492..4,008 Hz power within 0.3 dB of weighted measurement power.
- Full 2,664-direction spectral audit, 500/750/1,000/1,500/2,000/3,000/4,000 Hz:

| Region | Minimum / maximum deviation from weighted measured stereo power |
| --- | --- |
| Front | -0.00414 / +0.01142 dB |
| Rear | -0.01115 / +0.00732 dB |
| Left/right sides | -0.01544 / +0.02446 dB |
| Upper | -0.00392 / +0.01114 dB |
| Lower | -0.01507 / +0.03144 dB |

A denser 23.4375-Hz FFT-bin audit at the same 2,664 directions finds maximum
summed-ear 515.625..3,984.375 Hz deviations of -0.04690/+0.04079 dB (sides);
upper -0.01334/+0.01114 dB and lower -0.02885/+0.03536 dB. Individual ears at
narrow deep nulls are less accurate after cropping: -1.228/+0.450 dB in this
voice band, and -12.19/+1.11 dB in 4..12 kHz. Do not claim exact power matching
at every frequency or perfect reproduction of every pinna notch from the
seven-frequency stereo-summed table. No minimum-phase replacement was shipped;
a separate feasibility experiment worsened the far-ear voice-band error.

These are transfer-function checks, not perceptual or dense-measurement ground
truth. Weighted measured power defines the interpolation target; it is not a
claim about what an unmeasured HRTF must be.

A separate full-sphere crop audit estimates maximum discarded energy at 0.1014%
of one ear's full inverse-FFT response. Maximum energy before sample 100 is
0.1295%; the corresponding original-anchor maximum is 0.01566%. Energy centroids
span 167.45..207.89 samples, versus 167.45..207.87 for original measured ears.
The short filter is an approximation: there is a small residual tail/pre-onset
tradeoff, not an assertion of perfect continuous-time causality.

## Actual decoded music regressions (local fixtures, not committed)

No spatial balance override: test the actual disabled default. No audition
normalization and no foreground/support source attenuation. Comparisons are
against **explicit-bypass / authored-level** baselines, not earlier default-on
remix files.

| Excerpt | Frames | L/R RMS change | L/R waveform correlation |
| --- | ---: | --- | --- |
| How Do I Make You Love Me, about 54..64 s | 480,768 | -0.18139 / -0.18123 dB | .99421 / .99446 |
| Ether | 480,768 | -0.18210 / -0.18011 dB | .98778 / .98607 |
| Existing 360RA fixture | 720,896 | -0.00724 / -0.00639 dB | .99985 / .99967 |

All replays produce finite PCM. How Love/Ether report peak-guard gain 1.0.
The 360RA harness retains its existing wet/peak-guard policy; its waveform is
not a physical iOS capture. The 360RA figure preserves the previous accepted
excerpt closely; Ether changes more, so numerical success is not new listening
approval of that song.

For How Love, independently rendering the foreground (LFE and Obj10/11) and its
exact complement (Obj12..24) with unchanged metadata reconstructs the full
render: maximum residual 2.68221e-7, relative residual -133.36 dB. This validates
additivity between these diagnostic groups, not exact extraction/identification
of the vocal the user heard in the SL/SR channel audition.

Cached filter-update benchmark, optimized GNU Windows host: **133.44 us/update**
over 4,000 moving directions. This is not an iPhone realtime guarantee. FFT
planning and measurement phase unwrapping are outside the route-update loop.

## Reproduction and delivery boundary

Run normal native tests with `--no-default-features --lib -- --test-threads=1`.
The iOS workflow explicitly runs `spectral_interpolation_audit` (ignoring only
its offline exports/benchmark), `restored_historical_interpolation_regressions`
and `spatial_balance::tests` in addition to existing directional/cue tests.

Ignored audit environment variables:

- `SDA_BOUNDARY_AUDIT_OUT`: before/after boundary JSON.
- `SDA_SPECTRAL_AUDIT_OUT`: full-sphere sampled voice-band transfer JSON.
- `SDA_FILTER_AUDIT_REQUESTS`, `SDA_FILTER_AUDIT_OUT`: effective impulse responses.
- `benchmark_historical_spectral_updates`: use release mode and `--ignored`.

Local reports are under `tmp/how-love-diag/masking-audit/`; audio remains private.
Do not commit the music PCM or user recordings. iOS packaging is the requested
platform; no Windows/Android package is part of this correction.

The measurable boundary/cancellation interpolation defect is corrected. Actual
clarity of the 55..63 s vocal, height perception and physical-device playback
still require listening confirmation; do not relabel numerical tests as that
confirmation.
