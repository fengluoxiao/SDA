# User-authorized C combination iOS experiment

Explicit authorization: package full-height + mirrored triangle support + common timing/phase calibration and trigger iOS. AC-4 branch merged locally as d3e0317 (parents dba1b4a,d07fadf); includes Full A-JOC import. No claim of subjective acceptance or global optimality.

Selected resource id fullheight-triangle-commonphase-542-v1: exact response bytes from C-combined-140-180 audition (fullheight-combined-vs-current218). Original61 responses byte-preserved. Supplementary responses have identical common fractional-delay FIR in both ears plus common energy restoration; not interaural phase removal or source replacement. Original17speaker fallback/cues retained.

Triangle support is now a real opt-in manifest triangleSupportFile native path, not a cfg(test) environment variable. Loader validates position order, face indices, inverse matrices, mirror mapping and sampled fullsphere coverage. Positive L1 barycentric weights averaged with reflected operator, sparse <=6 supports; no per-source voice/song/id rules or playback scans/cache. Other packs without field retain previous interpolation. All-face search remains an experimental realtime cost requiring device tests; no zero-underrun or AirPods guarantee.

Actual staged resource replay vs C audition: target140-180 maxabs1.639e-7, residual-130.281dB, existing1e-6gate passed. Complete native299passed/0failed/30ignored; portable asset8tests passed including corrupt triangle sidecar rejection. Original61 response preservation checked in packaging. No triangle diagnostic switches or unrelated Android/desktop edits staged.

This replaces218 only in this explicitly requested iOS test checkpoint. Known motion and other-song risks remain; simulator and deviceCI, actual IPA marker/response/sidecar hashes and user listening required. User did not ask to eliminate all interference or strip physical binaural phase. No later research is silently included.
