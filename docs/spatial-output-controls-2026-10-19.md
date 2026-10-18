# Spatial output controls — 2026-10-19

Latest update — accepted B and fixed-gain export: the user confirmed B (+2/+6 base plus the 4 kHz/+1.5 dB shelf) for the other 11 materials, while explicitly withholding a Very First Night decision because the old long/short exports differed in loudness. The default source recipe is now B. All audition excerpts now share one fixed gain per track/profile/mode, verified with sample-identical overlaps. The historical A–E research and tests below remain provenance, not a claim that the pending song was accepted.


Latest presentation update (2026-10-19): the user-selected common base is now net +2 dB main / +6 dB auxiliary, using front-45-to-side-90 geometry. The linked +2 dB is spatial-only; the previous auxiliary/rear/shelf recipe below is historical. Existing HRTFHQ v3 clarity and depth processing remain. See [common-base integration and fusion audit](hrtfhq-common-base-fusion-2026-10-19.md).

The settings pages no longer expose direct-object, actual-direction, or a combined object-rendering switch. Android and iOS migrate preferences older than objectRenderingVersion 2 to direct=true and directional=true. Desktop restores the equivalent legacy storage keys to true. Existing spatial-off preferences are preserved.

The person menu selects Off, Fixed, or head tracking where available. Off uses compatible channels and bypasses HRTF and binaural coloration. Fixed and head tracking use the default object and actual-direction rendering. Low-level object-rendering and binaural controls remain separate for compatibility and tests; ordinary settings no longer offer an object-mode toggle.

E-AC-3 uses aligned decoder bed PCM; TrueHD retains the compatibility presentation while keeping the full presentation warm. MPEG-H retains the codec-rendered stereo reference. Formats without a carried compatible presentation use fixed-speaker projection before stereo folding. Existing codec delay remains; this is not a claim of bit-identical FFmpeg output for every codec. The float Lo/Ro matrix matches the measured FFmpeg 5.1 coefficients: front L/R 1, center/surround sqrt(0.5), LFE excluded.

Switches retain playback position and do not reopen playback. The renderer crossfades between paths. These features use in-memory PCM and metadata; they introduce no runtime analysis files or audio cache files.

Validation includes mobile and web TypeScript checks, executable mode/migration tests, WASM compatible-bed roundtrip, renderer PCM bypass and codec-clock routing tests, native facade checks, and Android Kotlin compile-only validation. Physical iOS/macOS listening and release UI parity have not been verified. The installed MuMu UI build still uses its earlier native library: it is not a test of these new audio controls. Packaging must rebuild native libraries; Android verification now rejects a library missing nativeSetBinauralEnabled. Release packaging and physical device listening remain separate validation steps.

## Accepted rear-balance candidate

Integrated the candidate approved in the HRTFHQ review chat, using its final isolated candidate-source and preserving concurrent output-mode changes. The shared HRTFHQ presentation adds up to 3 dB for rear sources before convolution, with a smooth spherical transition between 105 and 135 degrees from the forward axis. Front, direct sides and vertical poles receive no added rear gain; LFE is excluded. This is world-space weighting, independent of head pose, source ID, content and level. The existing auxiliary layer remains 1.5 dB; the maximum combined source lift is 4.5 dB. The existing enhancement control disables both additions. HRTFHQ impulse responses, interpolation and short reflection settings are unchanged.

Both serial and parallel object mixers apply the rule. Beds derive weights from canonical layout speakers, before listener rotation. The plain downmix takes its signal before these gains. Candidate geometry, smoothing, mirrored directions, LFE, Atmos/360RA objects, 7.1.4 beds, frontal stereo and callback partition tests are included. The accepted audition evidence remains in E:/SDA/artifacts/yudie-hrtfhq-20261018/rear-balance-audition/README.md; its older pending-acceptance wording predates the final user approval. No song or object-specific gain is included in production.

## Stereo balance before first output

Verified stereo masters now share Audio Vivid's bounded decoded preroll, in both the native mobile facade and desktop player. They use the existing BS.1770 meter, -18 LUFS attenuation target, true-peak protection and later measurement-update schedule. The initial gain is scheduled at the source origin before startup PCM becomes audible. The normal 57 overlapping measurement windows represent about six seconds of decoded material, not a mandatory six-second wall-clock delay. Already available valid measurements skip this preroll. Disabling volume balance also bypasses the gate. Short EOF uses available measurements; a silent introduction releases at seven decoded seconds, then follows the ordinary measurement policy when audible content arrives.

Eligibility is based on decoded stereo PCM, not stereo container headers. ADM/object programmes and MPEG-H reference stereo are excluded from this additional startup gate; ALAC optional upmix measures the original stereo master. There is no second analysis pass, added disk analysis record or audio cache for this startup change. Existing explicit full-track measurement facilities remain separate.

## Integration validation

The shared renderer library suite passed 336 tests with 33 diagnostic tests ignored. The native facade suite passed 54 tests with 2 ignored, serially, including real fixture decode/render checks and new stereo startup cases. A subsequent targeted plain-output test passed with the accepted rear gain explicitly enabled. Desktop startup, Audio Vivid output, stereo protective balance, preference migration and the built WASM compatibility roundtrip passed. Android x86_64 cross-check and the iOS host interface check passed; these are compile/interface checks, not physical Android/iOS/macOS listening. The earlier Android Kotlin compile-only check and mobile/web TypeScript checks also passed. The older loudness regression was updated to the existing attenuation-only policy and v6 true-peak cache format; production loudness targets were not changed.
