> Legacy lossless-v1 reference. Current exports and independent renderer integration are specified in [SDA interchange v1](sda-interchange-v1.md).

# SDAR Bed Channel: SDA Bed Lossless v1

The original lossless-v1 SDAR and private M4A exports used an independently specified PCM24 predictive
codec, compression identifier `sda-bed-lossless-v1`.
They do not contain AAC, MPEG Audio, or renamed PCM payloads. This initial
implementation is lossless and variable bitrate. There is no target-bitrate
lossy SDAR encoder. Legacy BCH1 PCM, four-track AAC v1 and packed AAC v2 remain
readable, and are never used for new exports.

## Signal and packet

Signed PCM24 is coded in blocks of at most 1024 frames, in authored channel order
(6, 12 or 16 channels). Every block decodes independently. One packet contains
all channels, including preserved back and height bed signals. A six-channel
base presentation never discards the remaining authored signals.

Unescaped packet bytes, little endian:

| Field | Size |
| --- | --- |
| Magic SBL1 | 4 |
| First coded sample index | u64 |
| Frame count | u16 |
| Channel count | u8 |
| PCM precision, 24 | u8 |
| Per-channel descriptors | 8 × channels |
| Channel payloads, descriptor order | variable |
| CRC32 IEEE over all preceding unescaped bytes | u32 |

Each descriptor is mode u8, Rice k u8, reference u8, reserved-zero u8 and payload
length u32. Mode 0 stores PCM24 verbatim and requires k=0 and reference=255.
Modes 1/2/3 use temporal predictor orders 0/1/2. The interchannel reference is
255 (none), or an earlier channel: subtract that decoded PCM sample before
temporal prediction. Temporal predictor order 1 uses the preceding value;
order 2 uses twice the preceding value minus the previous value. Missing initial
history is zero; the second sample of order 2 uses the first value. Predictor
history is reset at each packet. No rounding occurs in integer prediction.

Residual z is zigzag mapped as (z << 1) XOR (z >> 63). Rice codes consist of
q zero bits followed by one, then k remainder bits, MSB first. Final byte is
zero padded. The encoder selects a smaller representation per channel and falls
back to raw PCM if prediction costs more. Reconstruction must stay in signed
PCM24 range; trailing data, bad padding and checksums are errors.

All packets and SDAR metadata records escape 0xFE as FE 00 and 0xFF as FE 01,
after appending CRC. Literal FF is rejected. Thus a new SDAR cannot contain valid
MPEG frame sync; a generic inspector should report an unknown proprietary
format until it implements this specification. Friendly codec names require
inspector support and cannot be guaranteed by a file title.

## SDAR BCH2 container

Magic is ASCII SDAR0002BCH2 followed by newline. Every record header is a
four-byte tag, 16 lower-case ASCII hex digits giving encoded byte length,
and newline. Ordered records are HEAD, WAVH, WAVT, one AUD2 per packet, INDX.
HEAD is CRC-protected escaped JSON with schema sda.bed-file/version 2, codec,
profile, sampleRate, channels, bits, frames, masterFrames and blockFrames.
WAVH/WAVT preserve the exact original WAV bytes before and after PCM.
INDX is CRC-protected escaped JSON entries {sample,offset,bytes}, where offset
points at encoded AUD2 payload. A footer SDARIDX2 + 16 hex index-record offset +
newline terminates the file. Boundaries, index order and declared durations
are checked before playback. Audio CRC is checked before each block is decoded.
Restoring a 48 kHz SDAR yields the original WAV including artwork, exact lyrics,
source tags, render parameters and embedded room/HRTF/FIR resources.

## Private M4A v3

Brand sda1; exactly one trak, one audio track ID and one sdar sample entry.
Its sdac JSON has schema sda.bed-codec/version 2, codec sdar-bed-channel,
compression sda-bed-lossless-v1, lossless true, and stream (the HEAD object).
Every MP4 sample is exactly one encoded SBL1 packet, identical to SDAR packets
at the same coded rate. Sample tables include the exact final partial block;
there is no AAC priming, edit list, decoder configuration or component codec.
The audio sample entry presents a 5.1 base. The codec configuration describes
all authored channels; mdhd and stream describe the coded rate. Above 65535 Hz,
the base sample-rate field is halved and an explicit full srat box carries the
actual rate. sdaf/version 3 identifies the codec, true variable bitrate,
channel map, master duration and 7.1.4 mobile default. sdrm/sdid preserve the
authoritative renderer snapshot and original tags/assets.

48 kHz is sample-exact relative to the PCM24 master. Selecting 44.1 or 96 kHz
first resamples and quantizes to PCM24, then losslessly codes those resulting
samples; it does not preserve the original sample grid. Out-of-range converted
samples are rejected rather than silently clipped. Delivery M4A is not offered
as an original WAV master conversion source. No content/render/loudness cache
is used. Temporary codec files are export-job owned and removed on completion,
failure or cancellation.
