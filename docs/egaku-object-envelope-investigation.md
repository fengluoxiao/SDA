# Egaku Mirai: object envelope investigation

## 2026-09-05 Follow-up: Direct HRTF And Sparse Differential Audit

The user names PotPlayer as another normal-sounding reference. Inspection of
PotPlayer's Ctrl+F1 playback information after opening the exact Documents copy
of `05. エガクミライ.m4a` shows `Built-in Audio Codec/Transform`, internal FFmpeg
decoder `(eac3 + atmos)`, 48000 -> 48000 Hz, 2 -> 2 channels, and 16 -> 16-bit
output through DirectSound Audio Renderer. This confirms the current comparison
is a stereo output path; the `atmos` format label alone does not establish
independent JOC object reconstruction. It remains useful as a listening control,
but it does not supply the missing independent decoded-object reference.

The user also confirms that `egaku-dry-sum-objects.wav` retains the symptom.
That file has no HRTF, distance attenuation, OAMD gain, EQ, or output dynamics;
those stages are not necessary to reproduce the reported sound. This localizes
the next investigation to the reconstructed signals and their summation, but
does not by itself prove a decoder error or exclude authored/codec object
allocation effects. The dry mono sum is not a normative spatial downmix.

The same-file FFmpeg compatibility stereo remains the only listening reference
reported as normal. It does not expose JOC objects. OpenJOC's matching decoded
coefficients and PCM are useful implementation checks, but share the published
sparse equations and are not an independent vendor reference. A trustworthy
same-file Atmos render or decoded-object reference is now needed to validate a
corrective decoding change. No new guessed decoder rule is deployed following
these negative listening results. Independent object playback remains intact.

The user subsequently reported that the cumulative sparse candidate (D) also
sounds unchanged. It is rejected as a listening fix; the matrix-continuity
improvement did not establish perceptual improvement or decoding correctness.
The next isolation control is `tmp/egaku-dry-sum.mjs`: unity sum of all 15
production object PCM channels plus 0.5 delayed LFE, without spatial or dynamics
processing. A matched mono FFmpeg core sum is generated as a control. These sums
are diagnostics, not spatial playback replacements or recovered authored stems.

The user reports that test C and the new direct-object HRTF setting still have
the symptom. The live sidecar acknowledged `setObjectHrtf true` at 06:42:42 UTC
and again on session initialization at 06:42:45 UTC. The subsequent logged
source and FIFO underrun counts were both zero. Direct HRTF is not a confirmed
fix and remains an optional experiment.

A new raw-codeword audit of all 750 frames found 2,310 sparse object data points,
all with 12 bands and coarse96 quantization. Of 27,720 band selections, 17,796
differ between the published adjacent-raw-symbol formula and cumulative channel
delta decoding. Example: Obj14 at zero-based frame63 has channel words
`[3,0,0,0,0,0,0,0,0,0,0,0]`. The published formula selects `[3,3,0,0,...]`, whereas
cumulative decoding selects channel3 throughout. A zero delta suggests retaining
the previous channel, but that interpretation contradicts the published formula;
it is a hypothesis, not an official erratum.

The new offline candidate combines cumulative channel deltas, cumulative scalar
coefficient deltas, and zero coefficients for unselected channels. Dense decoding,
signaled temporal interpolation, QMF history, original PCM inputs, OAMD, separate
object channels, LFE, and native speaker/HRTF output remain. This differs from C,
which only subtracted a common pedestal. It also differs from the older
`343e7be` implementation, which used raw channel words directly.

Across 156 sparse/dense boundaries per object, target-matrix RMS differences
decrease from 0.496 to 0.304 for Obj14, 0.844 to 0.705 for Obj15, 0.490 to 0.300
for Obj22, and 0.477 to 0.274 for Obj24. All 15 rows improve against the published
baseline, but several improve less than the pedestal-only variant. Continuity
is not an independent correctness oracle and can favor removing valid content.
No production decoder change is justified by these measurements alone.

Reproduction: `tmp/joc-verify/src/bin/sparse_audit.rs` and
`tmp/joc-verify/src/bin/sparse_candidates.rs`. Report:
`tmp/egaku-sparse-candidates-report.txt`. Candidate PCM:
`tmp/egaku-sparse-cumulative-24s.pcm`. Native capture variant:
`objects-sparse-cumulative` in `tmp/egaku-path-fragment.rs`.
Listening conversion: `tmp/egaku-cumulative-audio.mjs`.

Date: 2026-09-05. Scope: the first 23 seconds, especially objects 14, 15, 22,
and 24. The reported fader-like sound occurs in both full playback and Solo.
Independent object audio, metadata, and Solo controls remain required.

## Finding

The user confirmed that the FFmpeg comparison sounds normal. The audible SDA
report therefore remains open; it must not be dismissed as intentional arranging.
The user also reported that test B (removing radius attenuation) still has the
sound. That experiment is rejected as a fix. Subsequent research found a known
sparse-JOC specification problem; agreement between implementations of the same
published equations is not independent proof of correct sparse decoding.
The remaining Obj14 envelope changes are already present before binaural
processing. Current reconstruction coefficients agree with an independently
parsed OpenJOC reference. These measurements alone do not identify a corrective
audio change. They do not establish equivalence to Dolby/Apple or prove
that every fluctuation is an intentional musical effect: authored dynamics and
lossy JOC object allocation cannot be distinguished from these data alone.

## Input and implementation

- Input SHA-256: `2fcbbef3ce3c48aa1edc9bef7121416e2b7e990105baa459418a353dd7c9cec5`.
- Extracted EC-3 SHA-256: `f73e1d08829b26e5f91ffa9590c64639e7e841b15d5006e2ca2434abdfaf04ca`.
- Current WASM SHA-256: `4cbeb7b189ebfe671b836b8b253abcdf8e223451b0c0f2e6446cecc8ad1e6195`.
- Staged native SHA-256: `73946fa2d7059f845c4dc41fdfcd0d11c38a31619bd336d8b35979a3a17725cd`.
- Native configuration: `hrtf`, wet `0.04`, virtual `7.1.4` layout, 48 kHz.
- Current decoding of 24 seconds: 750 frames, no gaps, errors, or nonfinite PCM.
- Last live log observation, 03:31:30 UTC: paused at sample 762880, 16 sources,
  source/FIFO underruns both zero, monitored output peak 0.337947, large steps zero.
  This observation covers that playback session, not all possible output devices.

Earlier changes remain in place: signaled JOC interpolation without additional
frame smoothing, removal of the automatic output loudness rider, Web peak-guard
routing, and native scheduled spatial-event consumption. Those changes passed
54 E-AC-3 library tests, 50 native tests, focused renderer/bridge checks, and the
Web build. The user still hears the symptom after those changes; they are not
evidence that the remaining report is resolved.

## JOC coefficient comparison

`tmp/joc-verify` parses the same raw JOC payload with OpenJOC and compares the
complete reconstructed timeslot matrices to SDA. Reference checkout:
`tmp/openjoc-review`, commit `ad6556babf42566f1a09820b01dc333703c8b1da`.

- 719 frames (23.008 seconds), 15 objects, 5 input channels, 64 bands, 24 slots.
- 82,828,800 coefficients; maximum absolute error `6.36e-7`, zero above `2e-6`.
- One initial reset; no midstream sequence/error reset.
- All four target objects remain active and use smooth interpolation.
- Each has 154 sparse and 565 dense frames; the transitions are signaled data.

For example, Obj14 frame RMS changes from -29.93 dBFS at 13.120 seconds to
-64.61 dBFS at 13.216 seconds, while the five-channel core stays near -31 dBFS.
The payload switches from sparse to dense and its low-band SR coefficient changes
from 0.400390625 to zero. This is an observation of reconstruction parameters,
not an OAMD gain automation or a recovered authored stem.

The 15-JOC-plus-LFE / 15-dynamic-object carrier shape and ordinal mapping match
the current OpenJOC admitted profile. This coefficient check does not independently
validate all core decoding, QMF synthesis, or a proprietary renderer.

Reproduction: `cargo run --release --offline --manifest-path tmp/joc-verify/Cargo.toml --bin joc-verify -- tmp/egaku-full.ec3`.
Detailed trace: `tmp/egaku-reference-matrices.tsv`.

## Obj14 dry versus native binaural

The dry comparison is decoded Obj14 duplicated into both ears. The other path
uses the current native engine with actual scheduled metadata, HRTF, EQ, output
gain, and peak guard. Its unscaled peak is 0.03690453 with zero source underruns;
the peak guard does not engage for this Solo capture.

The 24-second listening WAVs use one fixed scalar per entire clip to match stereo
RMS at -27.96 dBFS. No per-window normalization or compression is applied.
Comparison uses 10 ms log-RMS windows from 1.0 to 22.7 seconds, excluding dry
windows below -80 dBFS after matching. A constant 14 ms alignment is selected by
maximum envelope correlation over a 0-50 ms lag search.

- Envelope correlation: `0.998894`.
- Binaural-minus-dry level difference: P05 -1.265 dB, median -0.196 dB,
  P95 +0.474 dB, after fixed matching.
- At 2.06, 13.15, 13.96, 16.69, and 19.75 seconds, differences are below 0.5 dB.

Files: [dry dual mono](../tmp/egaku-obj14-dry-matched.wav),
[native binaural](../tmp/egaku-obj14-binaural-matched.wav), and
[measurements](../tmp/egaku-obj14-listening-levels.json).
Harness: `tmp/egaku-solo-fragment.rs`; analysis: `tmp/egaku-solo-compare.mjs`.
This strongly localizes Obj14's main envelope shape before HRTF. It is not a
perceptual similarity score or proof about all objects and the entire mix.

## Fixed buses versus direct HRTF

The isolated experiment uses identical PCM, distance gain, wet weight, and HRTF
set for both paths, before EQ/limiting. The accurate static-position window is
1.0-11.8 seconds. Longer 1-23 second measurements in the raw reports extrapolate
fixed positions past actual metadata changes and are not dynamic playback A/Bs.

| Object | Default set: bus - direct RMS / envelope correlation | Dense set: bus - direct RMS / envelope correlation |
| --- | --- | --- |
| 14 | +0.916 dB / 0.9999975 | +0.899 dB / 0.9999987 |
| 15 | -1.264 dB / 0.988164 | -3.709 dB / 0.984232 |
| 22 | +0.902 dB / 0.9999954 | +1.385 dB / 0.999977 |
| 24 | +0.856 dB / 0.999974 | +1.182 dB / 0.999740 |
| Four-object sum | +0.986 dB / 0.973976 | -1.226 dB / 0.982415 |

The default 17-direction set lacks rear 180 degrees; direct Obj15 selects
-140 degrees, so that case contains a direction confound. The dense set includes
the actual rear measurement. For dense Obj15, the bus/direct response differs by
-5.83 dB at 1 kHz and -19.02 dB at 4 kHz. Fixed buses introduce spectral coloration
and can affect a combined signal's envelope as spectral content changes. These
results do not establish the reported pumping as a bus-renderer defect or justify
replacing the production architecture as a fix for this passage.

Reports: `tmp/egaku-direct-ab-hrtf.json`, `tmp/egaku-direct-ab-hrtf-dense.json`.
Harness: `tmp/egaku-direct-ab/src/main.rs`.

## Official documentation and limits

Sources read on 2026-09-05:

- [Apple monitoring formats](https://support.apple.com/zh-cn/guide/logicpro/lgcp179f27c1/mac):
  Apple Renderer provides Apple Music/Apple TV headphone monitoring and is
  distinct from Dolby Renderer.
- [Apple binaural modes](https://support.apple.com/zh-cn/guide/logicpro/lgcp789f000d/mac):
  per-object/bed Off, Near, Mid, Far settings; these do not affect Apple Renderer.
- [Dolby binaural modes](https://professionalsupport.dolby.com/s/article/What-is-Binaural-Render-Mode-and-how-do-the-settings-affect-my-mix?language=en_US):
  distance models carried as program-level metadata that cannot be automated.
- [Dolby dynamic objects versus fixed tracks](https://professionalsupport.dolby.com/s/article/Why-should-I-mix-with-dynamic-objects-when-I-can-use-7-1-4-or-9-1-6-tracks?language=en_US):
  object position, size, and distance affect HRTF processing; fixed tracks limit
  spatial resolution and binaural summation can produce loudness buildup.

These documents do not publish a complete proprietary rendering algorithm,
justify per-object dynamics flattening, or establish fixed room-tail weights and
inverse-distance parameters as Apple Music or Dolby internals. Related vendor
attributions were corrected in [the design document](binaural-rendering.md).

No same-source official Apple/Dolby rendered comparison or original ADM stems are
available in this investigation. Such a comparison is the remaining evidence
needed to separate encoding/authoring effects from differences in proprietary
rendering. Independent objects and their original dynamics are preserved.

## FFmpeg comparison

On the user's request, local FFmpeg 4.3.1 (Free Download Manager distribution)
decoded the same M4A directly for 24 seconds. Its native E-AC-3 decoder produces
`5.1(side)` compatibility audio, not individual JOC objects or Atmos binaural
output. Both default decoding and `-drc_scale 0` stereo downmixes were retained.
The numerical comparisons below use DRC disabled. Default DRC changes some
samples, so the two settings are not reported as identical.

The existing SDA five-channel core capture aligns to FFmpeg with
`ffmpeg_index = sda_index - 2625` (54.6875 ms). After alignment, normalized sample
dot products are 0.999970-0.999998 across FL/C/FR/SL/SR, with error RMS between
-81.0 and -91.3 dBFS. This supports agreement of compatibility core decoding,
without implying bit-exact decoding or equivalent object rendering.

For the same aligned 32 ms windows:

| SDA frame time | Obj14 RMS | FFmpeg stereo RMS | FFmpeg SL/SR mean RMS |
| --- | --- | --- | --- |
| 13.120 s | -29.93 dBFS | -27.00 dBFS | -41.71 dBFS |
| 13.216 s | -64.61 dBFS | -27.79 dBFS | -41.36 dBFS |
| 13.280 s | -67.20 dBFS | -26.41 dBFS | -41.24 dBFS |

The dramatic Solo object drop is not reproduced in the compatibility mix's
overall or surround-pair level at these windows. This does not establish whether
the same instrument effect is audible underneath the mixed content. No direct
subjective listening claim is made from envelope measurements.

Listening files are 48 kHz PCM16 WAVs, each using one fixed whole-clip gain for
-24 dBFS RMS, with peaks below unity and no added dynamics processing:

- [FFmpeg default stereo](../tmp/egaku-ffmpeg-default-24s.wav).
- [FFmpeg stereo, DRC disabled](../tmp/egaku-ffmpeg-stereo-24s.wav).
- [FFmpeg SL/SR only, DRC disabled](../tmp/egaku-ffmpeg-surrounds-24s.wav).

The SL/SR excerpt is explicitly compatibility surround audio, not Solo Obj14/22.
Reproducible analysis: `tmp/egaku-ffmpeg-compare.mjs`.
Full measurements: `tmp/egaku-ffmpeg-comparison.json`.

## Follow-up after user confirmed FFmpeg sounds normal

An end-to-end check now feeds identical FFmpeg-decoded raw EC-3 PCM into SDA and
OpenJOC's complete JOC decoder, including separate QMF analysis and synthesis.
Raw EC-3 avoids MP4 start trimming when pairing each 1536-sample frame with its
JOC payload. All 15 objects over 719 frames were compared (1,104,384 samples per
object). Maximum absolute error is below 1.2e-6; error RMS ranges from -145.9 to
-156.2 dBFS. Replacing SDA's compatibility core decoder with FFmpeg while keeping
SDA JOC also produces only small residual differences, not proof that either
complete rendering chain matches Dolby. Results:
`tmp/egaku-pcm-reference-summary.txt`; harness:
`tmp/joc-verify/src/bin/pcm_reference.rs`.

The next isolated native-engine comparison uses the same HRTF, EQ, output gain,
and peak guard for three paths: FFmpeg compatibility core, current independent
objects, and independent objects without the extra coordinate-radius attenuation.
The last variant is an experiment, not a deployed fix. ETSI clause 4.2.1 defines
room coordinates relative to a cuboid; SDA currently treats their Euclidean
radius as an inverse-distance gain, reducing horizontal corners by 3.01 dB.
The experiment cancels only that extra gain, retaining position, object metadata
gain, scheduled events, PCM, and independent rendering. It cannot remove a
fixed-position Solo object's encoded envelope and may not fix the reported sound.

Harness: `tmp/egaku-path-fragment.rs`, prepared with `tmp/egaku-path-prepare.mjs`.
No production audio code or deployed binary was changed in this follow-up.

All three native captures completed with zero source underruns. The experimental
gain change alters 10 ms mix envelopes by P05/median/P95 +1.510/+1.819/+2.133 dB
before listening-level matching. Much of the change is a fixed level offset;
these values do not demonstrate that it removes the pumping.

Listening files use the same 23-second window and fixed whole-clip -28 dBFS RMS:
[FFmpeg core through SDA HRTF](../tmp/egaku-path-ffmpeg-core.wav),
[current objects through SDA HRTF](../tmp/egaku-path-objects-current.wav), and
[objects without extra radius gain](../tmp/egaku-path-objects-metadata-gain.wav).
The core path is only a diagnostic control. Independent objects remain the
production path. Script/report: `tmp/egaku-path-audio.mjs` and
`tmp/egaku-path-audio.json`.

## Sparse common-offset investigation (test C)

Cavern commit `9e7f3e5b4d00e1852f4a5596e877357ff370efc0` currently disables sparse
reconstruction. Its decoder explicitly says to restore the implementation when
the standard documentation is fixed:
[source, lines 128-131](https://github.com/VoidXH/Cavern/blob/9e7f3e5b4d00e1852f4a5596e877357ff370efc0/Cavern.Format/Decoders/EnhancedAC3/JointObjectCodingDecoder.cs#L128).
The maintainer reports an unresolved specification issue in
[issue 230](https://github.com/VoidXH/Cavern/issues/230#issuecomment-3012445466)
and describes the published-code result as a mono mix of all objects in
[issue 102](https://github.com/VoidXH/Cavern/issues/102#issuecomment-3224176273).
These are third-party implementation reports, not an official corrected Dolby
algorithm. Muting sparse frames, as Cavern does, would not satisfy this project's
independent-object preservation requirement.

In the published equations, sparse unselected channels retain quantized 50/100,
but dequantization subtracts 48/96. Both resolutions therefore produce the same
nonzero coefficient: `820 / 2048 = 0.400390625`. Zero sparse vectors also produce
this common coefficient. This mixes all five input channels into every affected
object. A sparse-to-dense transition changes this contribution even when OAMD
position and gain do not change.

Test C is a bounded counterfactual, not a verified complete sparse decoder:
independently reconstruct the common coefficient with the signaled interpolation
and QMF history, then subtract only that contribution from each decoded object.
Selected-channel/vector deviations, dense coefficients, independent object PCM,
LFE, metadata, and native rendering remain. It does not zero sparse object frames
or apply an output compressor. The reconstruction uses FFmpeg raw EC-3 input in
both the reference and subtraction paths, avoiding mixed decoder residuals.

The full 750-frame timing audit found no explicit EMDF sample offsets for JOC or
OAMD, `joc_clipgain=1` throughout, and 154 sparse frames for each of the 15 objects.
There is no basis here for shifting metadata or adjusting clipgain.

| Obj14 frame | Original RMS | Test C RMS |
| --- | --- | --- |
| 2.048 s (sparse) | -29.8935 dBFS | -46.9084 dBFS |
| 13.120 s (sparse) | -29.9337 dBFS | -49.5268 dBFS |
| 13.216 s (dense) | -64.6143 dBFS | -64.6143 dBFS |
| 13.280 s (dense) | -67.2012 dBFS | -67.2012 dBFS |

This localizes most of those sparse peaks to the common term. It does not prove
that every removed component is unwanted or resolve modulo/index ambiguities in
the complete sparse coding rules. The test is intentionally offline and has not
been deployed as a production fix.

Reproduction: `cargo run --release --offline --manifest-path tmp/joc-verify/Cargo.toml --bin sparse_pedestal`.
Summary: `tmp/egaku-sparse-pedestal-summary.txt`; per-frame trace:
`tmp/egaku-sparse-pedestal-windows.tsv`. The source for test C is
`tmp/egaku-sparse-pedestal-24s.pcm`; listening output is
[test C](../tmp/egaku-path-objects-sparse-pedestal.wav).

## Final bounded follow-up (2026-09-05)

The user reported no improvement from the sparse variants, direct object
rendering, dry object sum, and subsequently the official OpenJOC 0.16.0 stereo
and binaural outputs. The latter became usable with the existing CLI option
`--trim-config-count 8`; all 7515 frames parse with that explicit count. This
is a file-specific parsing result, not a universal default or a pumping fix.
See [reference results](../tmp/openjoc-reference-0.16.0/RESULTS.md).

The final offline audit (`node tmp/egaku-final-audit.mjs`) passes assertions for
750 contiguous 1536-sample frames at 48 kHz, stable unique LFE plus Obj10..24
labels, expected PCM sizes, and finite analyzed fullband samples. No pair of
objects has an exactly duplicate waveform over the 24-second capture. Obj14
and Obj24 have correlation 0.92049, but differ by up to 0.06723 in sample value;
high correlation alone does not establish an erroneous duplicate route.

With LFE excluded and the FFmpeg core delayed by the established 577 samples,
the raw-carrier windows at 13.120 and 13.216 seconds show:

| Measurement | 13.120 s | 13.216 s |
| --- | --- | --- |
| Sum of individual object energies, dB | -17.60 | -23.75 |
| Unity object mono sum RMS, dBFS | -6.44 | -23.56 |
| Sum of individual core energies, dB | -23.93 | -24.70 |
| Unity core mono sum RMS, dBFS | -21.79 | -22.92 |

Each window is 32 ms. The mono object sum falls 17.12 dB while the core sum
falls 1.13 dB. The difference between summed individual energies and mono-sum
energy also changes, consistent with changing cross-channel contributions.
This demonstrates an envelope difference upstream of HRTF, not a requirement
that object sums reproduce the compatibility core. The two representations
have no established unity-sum or energy-conservation invariant in this audit.
Full results: [audit JSON](../tmp/egaku-final-audit.json).

The diagnostic object render submits only LFE and the 15 objects; its core
render is an alternative mode. Source registration in native-renderer uses
`sources.entry(id).or_insert_with(...)`, so re-registering the same ID does not
itself add a second source. The dry sum already reproduces the reported
symptom without production routing, HRTF, metadata gain, or output dynamics.

No defensible new production correction was identified. Agreement with
OpenJOC does not establish Dolby conformance, and these observations cannot
decide between unresolved JOC reconstruction semantics and authored object
content. Independent object rendering remains preserved. This final pass
adds only diagnostic artifacts and records the unresolved outcome; it does
not add another listening switch or claim the symptom is fixed.
