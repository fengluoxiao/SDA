# Historical KU100 lower-hemisphere support

## Scope

The restored 61-point profile has measurements at -30, 0, +45 and +90 degrees, but none below -30. This change supplements, rather than replaces, its original responses with twelve measured SADIE D1 directions at -60 degrees and one at -90 degrees. The effective support bank is therefore 74 directions. The shared native renderer uses it only for the historical KU100/notch-guard profile, not arbitrary subject profiles.

The 61 original assets and the accepted spatial masking compensation parameters are unchanged. Direct interpolation at and above -30 degrees is unchanged; below that boundary a smoothstep blends interpolation weights toward the extended bank, reaching it fully at -60. Arrival alignment is applied after blending weights. Diffuse response construction can sample the lower hemisphere, so this does not promise identical non-lower full-engine PCM in all scenes.

## Provenance and calibration

`scripts/build-ku100-lower-support.py` verifies the local SADIE archive against the reference manifest SHA256. The embedded bank and `provenance.json` are in `apps/native-renderer/assets/ku100-lower`. The source license accompanies them.

Reproduce from the repository root with Python and numpy:

```powershell
python scripts/build-ku100-lower-support.py --archive tmp/sadie-source/D1.zip --reference apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json --out apps/native-renderer/assets/ku100-lower
```

Processing uses common integer shifts and scalar energy normalization to the reference median, followed by mirror-pair calibration (common polarity/shift), averaging, equal ears on median planes, and normalization. No spectral EQ is applied. These are supplementary calibrated measurements, not untouched raw WAVs or a claim of identical calibration to the original bank.

## Validation (2026-10-07)

- Native library suite, no-default-features, serial: **243 passed, 0 failed, 22 ignored**.
- Exact preservation of all original 61 anchors and an above-boundary direction sweep.
- All 13 supplemental anchors reproduce their bank samples exactly; nadir is azimuth-independent.
- Finite, bounded output and continuity at -30/-60 handovers. Deep lower responses are mirror-consistent. The transition retains the original -30 ring's asymmetric calibration; its waveform mirror error is bounded relative to the old response, not asserted to be zero.
- Final spectral sweep: 2,664 directions at 5-degree spacing, 500/750/1000/1500/2000/3000/4000 Hz. Worst lower relative power is **-2.35614 dB** at azimuth 0, elevation -75, 750 Hz. The previously flagged azimuth -105, elevation -85, 4 kHz point is **-0.00119 dB** (previously -3.14524 dB).
- Spectral ratios use each version's effective measured support as reference. The reference changes with the added bank: these figures are not absolute loudness gains, proof of perceptual clarity, or a claim that every lower-direction notch is eliminated.

Offline full-engine regressions against the pre-change, spatial-balance-enabled baselines:

| Fixture | Stereo frames | New peak | L/R correlation | Largest absolute RMS change |
| --- | ---: | ---: | --- | ---: |
| How Do I Make You Love Me? approved H scene | 480768 | 0.471112 | >0.99999999999997 | <0.000001 dB |
| Ether | 480768 | 0.480934 | >0.99999999999997 | <0.000001 dB |
| 360RA fixture | 720896 | 0.891254 | >0.99999999999997 | <0.000001 dB |

All outputs were finite and frame counts matched. These fixtures preserve the previously accepted effects numerically; they do not replace listening to lower-direction material or device testing. The 360RA harness includes its existing wet mix and peak guard. No claim of an iOS playback test is made.
