# Shared midrange presentation

User-approved Seiza candidate, integrated on2026-10-19. Recipe points are the archived H13-guided KU100 band coefficients from apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json. They form an explicitly selected supplemental presentation, not an HRTF calibration claim.

Seven points span500–2000Hz, bounded at±3dB. All61directions participate in continuous interpolation; frontal±45degrees bypass and45–75degrees enter smoothly. A257tap symmetric unity-DC FIR is identical for both ears at each prepared direction. The centered implementation keeps the arrival reference, pads all prepared responses to640taps, and rejects leading-sample loss. Source HRIR files and their hashes remain unchanged. KU100-only far-ear corrections and per-direction energy normalization are not copied.

The layer is required only by measuredResponseVersion3/sharedClarityVersion1 and is compiled into the shared native crate. It executes while preparing responses, not as input analysis during playback. It has no song, object-ID or time-window selector. Reloading a profile rebuilds once from raw measurements; snapshots share the prepared policy. Older response families stay unchanged.

The existing enhancement control handles source-layer gains and the final shelf. This response-family EQ remains part of its profile; spatial Off uses the compatible/plain path and bypasses HRTF processing. No UI redesign is involved.
