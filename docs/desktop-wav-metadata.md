# Desktop WAV source metadata, version 1

A stereo-to-7.1.4 export produces one 48 kHz PCM24 WAVE file. Descriptive source
metadata is embedded after the PCM `data` chunk. The channel mask, data size,
sample count and PCM bytes describe only the rendered 12-channel audio.

## Interoperable tags

- A UTF-8 `LIST/INFO` chunk carries title, artist, album, date/year, genre,
  track number and copyright. `CSET` declares code page 65001.
- An `id3 ` chunk carries ID3v2.4 text fields, album artist, composer, original
  release date, track/disc totals, credits and sorting fields. `APIC` stores the
  original artwork bytes. `USLT` carries the original lyric text.
- RIFF/RF64 file sizes include every metadata chunk and its even-byte padding.
  The RF64 `ds64` audio size and sample count exclude metadata.

## Lossless lyric attachments

Native embedded lyric strings are read before common-tag LRC normalization.
Exact same-stem `.lrc` and `.ttml` files in the source directory are also read
(case-insensitive extension matching). Both formats can coexist. Sidecar BOM,
encoding, line endings and TTML word timing/XML are retained byte-for-byte in
ID3 `GEOB` attachments. No remote lyric, remote image or XML entity is loaded. The attachments do not create separate output files.

## Source metadata record for future containers

A JSON `GEOB`, with MIME type `application/json`, filename
`source-metadata.json`, and description `SDA source metadata v1`, contains:

```json
{
  "schema": "sda.source-metadata",
  "version": 1,
  "filename": "source.m4a",
  "common": {},
  "native": {},
  "format": {},
  "lyrics": [],
  "assets": []
}
```

`common` preserves normalized source values; `native` preserves original tag IDs
and values, including tags without a direct WAVE/ID3 equivalent. `format`
describes the source, not the new WAV's codec. Binary values are replaced with
`{"asset":"<sha256>"}` references. `assets` records each hash, MIME type,
filename, kind and original byte length. The corresponding bytes are retained
in `APIC` or `GEOB`; multiple tag references do not duplicate the attachment.
`lyrics` records format, source (`embedded`/`sidecar`), language and asset hash.

Future proprietary writers should consume this source record/asset model,
rather than reverse-mapping flattened display strings. Source loudness or
ReplayGain tags may be retained in this provenance record but are not promoted
to active playback gain tags on the processed output.

Export snapshots metadata once per action, verifies source identity before
commit, and uses the existing cancellation, peak verification and atomic
replacement path. No loudness analysis cache is used. Individual attachments
are bounded at 16 MiB; total metadata is bounded at 64 MiB. Exceeding either
bound fails before replacing the destination, rather than silently truncating
original metadata.

Generic WAV players vary in their lyric/attachment support. The JSON record and
original assets ensure SDA/future formats can recover the source even when a
particular player does not display a TTML attachment.
