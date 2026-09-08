# AC-4 IMS Investigation

## Current Status

IMS spatial playback is not implemented. The existing SDA AC-4 backend remains
the MacinDecode Full A-JOC backend. No IMS spatial toggle is exposed: bypassing
SDA processing alone would not implement the requested IMS spatial processing.

The local Happy M4A sample has one AC-4 presentation, presentation version 2,
48 kHz, frame-rate index 13, and `pre_virtualized = false`. Its first TOC reports
channel mode 6. This is not evidence of eight decoded PCM channels: IMS uses a
stereo coding path. Container duration is 234.02666666666667 seconds.

The complete stereo export passed: 11,233,280 samples/channel, peak
0.6210331916809082 and RMS 0.09491858919786837. All PCM was finite and no
audio-payload underread/overread was reported. The local result is
`tmp/ac4-happy-verification/Happy-stereo.wav`, with a JSON verification sidecar.

MacinDecode's scene API rejects its channel-based coding path before decoding
PCM. The error does not indicate a damaged M4A container.

## Additional Decoder

Librempeg revision `34d5e43dc39f6e25da9497a57278032db348ba3c` has an explicit
immersive-stereo case in `ac4_substream_info_chan`: for presentation version 2,
channel modes 5 through 10 use stereo audio decoding. Its `ac4_substream` skips
the remaining substream metadata after the audio payload. This source inspection
does not establish support for an IMS-specific binaural renderer.

Source: https://github.com/librempeg/librempeg/blob/34d5e43dc39f6e25da9497a57278032db348ba3c/libavcodec/ac4dec.c

`scripts/decode-ac4-stereo.mjs` is a diagnostic export using that separately
built decoder. It is not a replacement for SDA's object decoder. It refuses
existing output files, propagates decoder errors, rejects audio payload
underread/overread warnings, and checks every output sample for finite values.
Its report always records `imsSpatialProcessing: false`.

```powershell
# Requires MSYS2 with MinGW GCC and mingw32-make. Initial configure can be slow.
& C:/msys64/usr/bin/bash.exe scripts/build-ac4-stereo.sh
# SDA_AC4_REFERENCE can select the separately built decoder executable.
node scripts/decode-ac4-stereo.mjs input.m4a output.wav
node scripts/test-ac4-stereo.mjs input.m4a
```

The standalone adapter is `apps/ac4-decoder/main.c`. It links the LGPL library
subset, not Librempeg's AGPL CLI, and exports bounded-memory float PCM without
overwriting existing files. The no-assembly patch keeps the x86 C initializer
in the library even when assembly is disabled. Sources remain in the pinned
local checkout; no third-party binary is bundled with SDA.

## Spatial Control Evidence

Dolby's AC-4 whitepaper (2021), pages 36-37, describes IMS as two coded channels
plus control data. It explicitly distinguishes headphone virtualization,
integrated-speaker virtualization, and non-virtualized LoRo output:
https://professional.dolby.com/siteassets/technologies/dolby_atmos_ac-4_whitepaper.pdf

The sample's first audio substream contains 764 audio bytes and 69 metadata
bytes. The metadata parser finds one EMDF payload with ID 18 and length 63
bytes. This is a candidate for further investigation, not a decoded spatial
parameter set. Its envelope parses completely using the physical stereo
context; dialogue enhancement is absent in that frame.

The public EMDF registry lists ID `0x12` (18) as Dolby `Reserved`, documentation
`TBD`: http://emdf-ra.org/payloads.html . The inspected OxideAV decoder likewise
lists semantic interpretation of EMDF bodies as unimplemented. These sources
do not provide the payload's parameter grammar or spatial reconstruction
equations. No unsupported bytes are discarded from the original input or
relabelled as object coordinates.

A successful stereo export proves neither IMS spatial reconstruction nor
reference-decoder parity. A trustworthy spatial implementation still needs
the applicable metadata semantics, numerical reconstruction, and independent
reference outputs. No metadata fields should be guessed from audible results.

## Cross-Stream Payload Observations

The same topology-driven extractor processed the complete Happy track and two
Dolby Online Delivery Kit 1.5 Audio ID IMS streams (112 kbps, video rates 50
and 59.94 fps). The two official streams share test content; they are distinct
encoding configurations, not independent musical sources.

| Input | Audio frames | ID 18 payloads | Payload bytes | Update gaps in audio frames |
| --- | ---: | ---: | --- | --- |
| Happy | 5485 | 2743 | 25-73 | 2 |
| Audio ID 50 | 800 | 432 | 30-100 | 1 or 2 |
| Audio ID 59.94 | 960 | 512 | 31-103 | 1 or 2 |

MSB-first bit index 5 equals the enclosing audio independence flag in every
observed ID 18 payload across these inputs. This suggests a relationship to
independence/reset, but does not establish the field's decoding semantics.
Official streams also contain prefixes `0434`, `0835`, `0c35` and others:
neither `0834`/`0c34` nor an every-two-frames cadence is a universal signature.
No object identity, count, position, or spatial reconstruction coefficients
have been established from these observations.

Reproduce extraction with `node scripts/inspect-ac4-ims.mjs input.mp4 new-dir`,
then run `node scripts/analyze-ims-payloads.mjs new-dir/payloads.jsonl report.json`.
Outputs are exclusive-created. The extractor selects a unique version-2
presentation, follows declared group/substream indices, and retains exact
EMDF bit offsets and payload bytes. Multiple IMS presentations, alternative
metadata contexts, and unsupported frame-rate/mapping forms fail explicitly.
This is a research extractor, not a complete IMS decoder.

Official sample listing:
https://ott.dolby.com/OnDelKits/AC-4/Dolby_AC-4_Online_Delivery_Kit_1.5/help_files/topics/kit_wrapper_MP4_multiplexed_streams.html

### Header Hypotheses and Counterexamples

`scripts/probe-ims-structure.mjs` tests explicit hypotheses and preserves
counterexamples. Across 3,687 ID 18 payloads, the observed first byte equals
`(sequence_counter % 2 === 0 ? 8 : 0) | (audio_independent ? 4 : 0)`.
Across all 7,245 audio frames, ID 18 is present exactly when the sequence is
even or the audio frame is independent. These are corpus observations, not
normative syntax, and are not used to reject or render other IMS streams.

File-frame parity initially fits equally well. Rebasing the official streams'
JSON records at independent frames 13 and 15 invalidates file-frame parity
for all 425 and 504 remaining payloads, respectively, while sequence parity
still matches. This is an index-rebasing experiment, not a successful decoder
seek test. Other encoders or update schedules may distinguish additional
explanations for bit 4; its semantic name remains unassigned.

The second byte changes from `34` to `35` at audio frames 642 and 770, then
back at 722 and 866, in the 50 and 59.94 fps files respectively. These are
similar relative content positions; the transitions are not restricted to
independent frames. The track-start/end values also include `09`, `0b`, and
`0c`. This supports investigating a content-dependent field but does not
identify its boundaries, coding, or meaning. The byte is not treated as a
fixed magic value or as an object count.

Both official files also completed the diagnostic stereo export with finite
PCM and no audio underread/overread. Librempeg reports 51,200 Hz and 46,034 Hz
respectively (its internal resampled-core output rates), so analysis must
use the actual output rate rather than assume 48 kHz. These exports do not
validate IMS rendering; the 50 fps export has float peak 1.1134 and must not
be silently clipped for numerical comparisons.

Run the hypothesis suite with `node --test scripts/test-ims-structure.mjs`.
Run corpus probing with `node scripts/probe-ims-structure.mjs new-report.json
payloads-a.jsonl payloads-b.jsonl`. The next unresolved step is a grammar for
the remaining control body and numerical spatial reconstruction, not an
SDA room-processing switch.

## Body Layout Experiments and Algorithm Leads

The official elementary-stream archive is publicly available at:
https://ott.dolby.com/OnDelKits/AC-4/Dolby_AC-4_Online_Delivery_Kit_1.5/Test_Signals/elementary_streams/Audio.zip

Its two IMS streams have 800 and 960 frames. Every extracted audio-substream
record, including the exact ID 18 payload, matches the corresponding MP4
sample. Different raw-file hashes therefore do not establish new control-data
coverage. These files are not counted as additional independent samples.

`scripts/probe-ims-lengths.mjs` scans MSB-first unsigned fields of 4-16 bits
within payload bits 8-127. It tests whether each field equals total payload
length in bytes or bits, minus a constant learned from the Happy track. The
official streams are validation data. None of the 2,886 field/unit candidates
fits exactly, including when independent and non-independent frames are
examined separately. This excludes only this fixed-field/total-length model,
not subblock lengths, conditional fields, variable-length coding, or entropy
coding. Synthetic tests verify discovery of an unaligned planted field and
rejection of a training-only match:

```powershell
node --test scripts/test-ims-lengths.mjs
node scripts/probe-ims-lengths.mjs new-report.json happy.jsonl official-a.jsonl official-b.jsonl
```

Two public Dolby-related patent descriptions provide algorithm leads:

- US12273702B2, paragraphs 0173-0185 and Figure 17, explicitly says its
  parametric binaural system may implement AC-4. It describes transmitting
  loudspeaker stereo LoRo and presentation transformation parameters W.
  The decoder reconstructs anechoic binaural LaRa and acoustic-environment
  input ASin; an environment simulator generates ASout, which is mixed with
  LaRa before synthesis. Paragraph 0217 describes per-frequency-band matrix
  operations. Source: https://patents.google.com/patent/US12273702B2/en
- EP3378239B1, paragraphs 0029-0044, describes time/frequency tiles,
  dominant-component direction, two prediction weights, and residual matrix
  coefficients. The effective anechoic transform is a 2x2 matrix:
  `Y_hat = (W_residual + H_direction * w_dominant) * Z`, where Z is stereo,
  H_direction is a 2x1 HRTF vector and w_dominant is a 1x2 prediction row.
  Without dominant prediction, this reduces to a parametric matrix method.
  Source: https://patents.google.com/patent/EP3378239B1/en

These are candidate architectures, not an identification of ID 18 syntax.
Neither inspected description supplies its payload bit order, quantization
tables, entropy codebooks, band mapping, or state-update grammar. Their
dominant components are not guaranteed to equal original authored objects.
No coefficients or directions have been recovered from the sample bytes.
Further work should test matrix/band/direction coding hypotheses against
the control body; implementing arbitrary patent coefficients as an IMS
backend would not constitute decoding the user's files.

### Conditional Body and Core-Signal Checks

The structure probe now partitions body statistics by audio independence
and the second byte, retaining observation counts for each constant bit.
For non-independent `35` payloads, body bits 16-19 are `0001` in all 37
records of the 50 fps file and all 45 records of the 59.94 fps file. Neither
file's non-independent `34` group has a constant bit in bits 16-63. The
ordinary `35` payload means are 67.62 and 68.29 bytes, versus 57.45 and
58.30 for `34`. This is evidence for investigating conditional layout or
different parameter distributions, not proof of a four-bit field boundary.
Both files share source content and therefore are not independent semantic
ground truth. Happy contains no `35` records to validate that branch.

`scripts/probe-ims-signal.mjs` compares these runs with the separately decoded
stereo core. It checks constant decoded samples per frame and uses each WAV's
actual sample rate. Its analysis holds the second byte until the next payload
as a convention; this is not an implemented metadata state rule, and codec
delay is not compensated. In both official files, the `35` interval is about
25.69-28.89 seconds, with left/right RMS about 0.10/0.11, normalized cross
product about 0.75, and side/total energy about 0.126. It is neither silence
nor identical-channel mono. The following `34` interval has an even higher
normalized cross product (about 0.79). A first-difference energy proxy also
decreases again in that `34` interval, so these aggregate observations do not
support interpreting `35` simply as a mono/silence or low-frequency flag.
They cannot exclude subtler signal-dependent coding decisions.

The inspected Dolby US20230343346A1 quantization/entropy-coding patent refers
to IVAS, CACPL and SPAR (paragraph 0059), not an identified IMS payload grammar.
It is not used as an IMS codebook or parser specification:
https://patents.google.com/patent/US20230343346A1/en

## Requested Routing Contract

Once a validated IMS spatial backend exists:

- Show the IMS processing switch only for a positively identified IMS track.
- On: feed the IMS spatial output directly to the output path, bypassing SDA
  HRTF, room convolution and cinema processing. Disable the corresponding
  controls, while retaining their settings for later restoration.
- Off: feed the non-IMS-processed PCM to SDA's selected rendering path.
- Keep codec reconstruction separate from optional spatial rendering. Disabling
  the spatial switch must not disable decoding tools needed to reconstruct PCM.
- Switch atomically without duplicate playback; preserve pause and seek state.
- Restore normal controls when leaving an IMS track. Do not persist an active
  bypass into other codecs or fall back silently after a backend error.
- Respect `pre_virtualized` separately: already virtualized content cannot be
  assumed to provide a recoverable unprocessed version.

ETSI TS 103 190-1 V1.4.1 section 4.3.3.3.5 defines `b_pre_virtualized` as
pre-rendering by a headphone virtualizer. ETSI TS 103 190-2 V1.3.1 section
6.3.2.3.1 includes presentation version 2. Neither a version flag nor the
presence of rendering-control metadata is an implementation of a virtualizer.
