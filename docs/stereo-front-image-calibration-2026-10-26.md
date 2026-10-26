# Stereo front-image calibration candidate — 2026-10-26

Status: the user selected this candidate on 2026-10-26 and authorized the 0.9.24 mobile release. Installed-package device acceptance is still pending.

The reported symptom is a recessed/inward left image in both native stereo and upmix. Earlier broadband left/right RMS checks do not establish perceptual image position. Isolated inputs did not reproduce a consistently narrower left image, although the generic shared HRTF front responses have non-mirrored spectral and phase differences. The change below is an audition candidate, not a proven diagnosis of that subjective symptom.

`front-direct-mirror-lowband-v2` averages corresponding near-ear and corresponding far-ear **direct FIRs** across the mirrored front directions. It also averages the two direct center responses for stereo upmix. Source PCM remains independent and near/far arrival delays remain distinct. The existing stereo low-band preservation then runs on these calibrated direct responses.

This intentionally replaces the old per-ear front common-response invariant with an all-ear average reference. It can change front timbre; it preserves the summed direct common response across both ears, the complete original per-ear room residuals, and all rear/height routes. Explicit monitor calibration keeps its earlier low-band behavior without mirrored calibration. Explicit room/hardware profiles remain excluded. Non-stereo programme codecs keep original prepared filters, and original stereo bypass remains unchanged.

The preparation stage reuses existing convolvers. The optional center filter adds preparation/cache data, not a new audio-processing stage, inference path, or gain envelope. Native iOS and Android share this implementation. Upmix trim, programme makeup, and peak guard settings are unchanged.

Validation used local BADDY BADDY excerpts, isolated left/right inputs, and a dual-mono control through the actual mobile adapter/renderer/output queue. With programme balance on, dual-mono 1–3 kHz R−L changes from +0.3557 to −0.0019 dB in ordinary stereo and +0.3847 to −0.0048 dB in upmix. Ordinary isolated left/right 3–6 kHz outward level differences become 10.2524/10.2694 dB (previously 11.2605/8.9875). These are signal measurements, not a listening-position guarantee. Source mix differences remain in the actual song output.

Checks passed: stereo and original-bypass regressions, codec/layout/head-direction tests, low-band preservation, shared-room residual preservation, bit-identical rear/height/reflection output, common peak protection, unchanged speaker PCM adapter, and same-programme Atmos loudness matching. Matched output is −23.129 / −22.684 / −22.879 LUFS for Atmos/stereo/upmix. No phone listening verification has occurred.

Local audition files and exact measurements are under `E:/SDA/outputs/baddy-image-depth-20261026`; no private source or derived audio is added to the repository or uploaded. The iOS provenance recorder derives the algorithm identifier from the renderer constant rather than recording a stale literal.
