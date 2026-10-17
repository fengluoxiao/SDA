# HRTFHQ / retained 61-direction depth attribution

User requests: retain the old 61-direction front/rear distance impression for moving objects, plus overhead treatment for 360RA. The user subsequently approved candidateB for production on2026-10-17; integration is recorded below.

The retained 61-direction manifest contains both a calibrated KU100 response and an enabled independent short cue layer. It uses the default 35ms window; this reference must not be confused with the later 542-direction G80 preset. Direction count alone does not attribute the listening difference.

Added `hrtfhq_depth_probe` under `cfg(test)` only. Native shared-engine synthetic constant-radius objects follow identical trajectories and authored gains. Atmos 7.1.4 and Sony 360RA-13 use their own layouts. The overhead case crosses front/top/rear, holds at zenith, then crosses side/top/other side. No physical metre metadata is invented, and no head movement or decoder is simulated.

Three engine renders per case: retained61 with cues, identical retained61 with zero residual send, and current HRTFHQ. Full-minus-zero-send isolates the residual. Impulse checks locate its onset about 4.94ms after the common direct peak, with early direct relative errors below -139dB for the horizontal cases. Both ears share calibration and timing. The offline candidate adds half-strength residual to the HRTFHQ render; it never averages two direct HRTFs. This is a cross-response residual experiment, not an established physically coherent room model ; subjective approval subsequently came from the user.

No sample clipping or guard action occurred in these synthetic engine renders. The candidate changes unnormalized one-second levels by at most 0.20dB in the delivered front/rear example and 0.23dB in the overhead example. Each listening segment is separately matched to -23LUFS for audition only. PCM24 exports were decoded back and checked. These measurements do not certify perceived depth, vocal clarity, other songs, head-tracking behaviour or four-device parity.

Evidence and reproducible export scripts:
`C:/Users/legendshop/Downloads/SDA-clarity-validation/hrtfhq-legacy-depth-20261017/`

Delivered files are 47s each: HRTFHQ 0-15s, silence, candidate16-31s, silence, retained61 32-47s. `delivery-validation.json` records hashes, loudness and limitations. A direct-only61 control is available separately to test whether the preferred impression survives removal of the residual. This describes the pre-approval audition. The user has now approved this candidate; production integration follows below.

## Additional height trajectories

Added two user-requested 360RA-13 paths, still synthetic constant-radius objects with the same source signal and presentation: bottom (-90 degrees) via frontal horizon to zenith (+90), and upper-left (+45) across zenith to upper-right (+45). Analytic geometry tests verify the endpoints, constant radius and millisecond-scale Cartesian continuity, including the azimuth convention change at zenith. The source stays at constant authored level.

The retained61 data have measured elevation rings only at -30, 0, +45 and +90 degrees. Its below--30-degree output is the legacy implementation fallback, not a measured lower-hemisphere reference. The shared HRTFHQ bank covers -90 to +90.

Exports and verification are in `height-extensions-delivery-validation.json`; both combined WAVs are 47 seconds with the same A/B/C order. All source engine renders retained guard gain1. The two candidates changed unnormalized one-second levels by at most0.273dB and0.214dB respectively. Each exported segment is -23LUFS. PCM24 readback error is bounded by1.3e-7. Perceptual trajectory accuracy remains for user audition; no production integration has occurred.

## Production integration after user approval

CandidateB is integrated through fixed native speaker residual banks, retaining the exact0.5 multiplier and common calibration. Four comparisons against the stored accepted output pass without time/gain fitting: max error3.968e-9, worst residual-127.49dB. Full engine, resource and four-platform checks are recorded by the current build/test results. The source contains reproducible offline probes; no runtime analysis files or user controls were added.
