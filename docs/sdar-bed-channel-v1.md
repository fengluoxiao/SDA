> Legacy format reference. New exports use [SDA Bed Lossless v1](sdar-bed-lossless-v1.md). BCH1 PCM and the AAC transports below are retained for reading old files only.

# SDAR Bed Channel — authored metadata v1 / M4A transport v2

SDAR playback is file-authored on desktop, iOS and Android. A player preset must
not replace an authored mix. Normal system/listening volume, the output device
and live head motion remain listener controls.

## Exported files

One export action produces one final file, selected in the desktop export panel:

- WAV master: 48 kHz, PCM24, twelve discrete bed channels. Desktop only.
- `.sdar`: the same lossless master PCM, metadata and resources in the SDA bed
  envelope (`SDAR` / `BCH1`). Plays through the shared desktop/mobile engine.
- SDA `.m4a`: **one physical audio track and one audio ID**, using the
  `SDAR Bed Channel` codec profile. A 5.1 base and the complete twelve-channel
  upmix bed live inside this single synchronized coded stream. It does not ask
  the player to infer missing signals from an ordinary six-channel downmix.
  Default mobile rendering is 7.1.4; other SDA layouts, including 9.1.6,
  11.1.8 and 22.2, use the existing renderer routing and the file's parameters.
  The export dialog explicitly selects the SDAR Bed Channel profile, 44.1/48/96
  kHz and a total target bitrate of 256–1536 kbps.

Canonical master order is `FL FR FC LFE BL BR SL SR TFL TFR TBL TBR`. Internally,
AAC-LC compresses synchronized components mapped to `[0,1,2,3,6,7]`, `[4,5]`,
`[8,9]` and `[10,11]`, apportioned 5:1:1:1 of the total target bitrate. These are
components of one SDAR codec access unit, not independent MP4 tracks. Each unit
contains `SBP1`, component count/reserved bytes, four big-endian u32 lengths and
the corresponding component data. This retains the original bed signals and
channel identities for the same renderer at larger output layouts. AAC is lossy;
this is not a sample-exact claim relative to the PCM master. Final native output
protection remains active.

New M4A files use the `sda1` brand, a single `sdar` sample entry, the full handler
name `SDAR Bed Channel`, and a versioned `sdac` codec configuration. `sdaf` v2
identifies `sdar-bed-channel`; `sdrm`/`sdid` retain rendering and original metadata
and resources. Third-party inspectors may display the `sdar` FourCC until they
implement recognition; writing a friendly name cannot register a new format
with those tools. This is a private SDA format, not encryption/DRM.

The shared reader still accepts legacy `sda.delivery` v1 files with four `sdaA`
audio tracks. New exports always use v2. Tests verify that repacking identical
AAC components from legacy tracks into one track changes no decoded sample.
Current SDA decoder builds are required for new v2 files; the export capability
check prevents pairing the new writer with an old bundled native reader.

Existing SDA WAV/SDAR masters can be converted without decoding/upmixing or
applying the current player settings again. Conversion preserves the original
source; temporary internal files are removed on completion/cancellation. A
lossy SDA M4A is not presented as a source for lossless master reconstruction.

## Authoritative processing and resources

The versioned `sda.render-metadata` record contains:

- Stable channel/PCM identities, roles, polar and Cartesian geometry, source
  mappings and explicit per-channel runtime gain. The original source mapping
  is provenance; it is not applied a second time to the bed PCM.
- Source metadata, artwork and exact embedded/same-stem LRC/TTML attachments.
- A complete playback snapshot, baked processing decisions, renderer revision,
  peak protection and a runtime recipe.
- SHA-256-addressed HRTF manifests, kernels, triangle support and companion
  speaker sets; the complete selected room response; the actual active
  headphone-simulation FIR and its preamp.

`baked.volumeBalance.applied` and `baked.spatialBalance.applied` say whether those
operations already happened in the master. If true they are bypassed during
playback, including the extra programme makeup and directional source processing.
Disabled authored operations also remain disabled. An enabled, unbaked operation
uses the file's values. No programme loudness analysis/cache is used for SDAR.

The HRTF front presentation is retained independently from already-baked source
spatial processing. Disabling all spatial presentation merely because source
processing was baked would change the approved mix.

Unbaked HRTF, room, EQ, monitoring, near-field and routing parameters run in the
same shared Rust renderer on all platforms. Files carry their own resources;
mobile does not substitute its built-in room/HRTF preset. Export also accepts
an active room/HRTF comparison: it records the acknowledged room settings,
reflection stage, speaker calibration and comparison gain. Merely having an
audition selection is not an export restriction. External simulated/measured
rooms embed the complete selected response; intrinsic HRTF room effects use the
embedded dry/wet kernels and do not require an external room profile. Missing resources,
invalid values, unsupported schema versions/operations and future object or
parameter automation are rejected explicitly. Real object authoring remains a
future `SDAR Bed Channel With Objects` feature.

A different output speaker layout keeps common room responses unchanged and
interpolates additional directions using the nearest responses in the same
height tier. This is an explicitly versioned adaptation, not a new room
measurement. The full provenance stays in the file/UI, while the executable
recipe and extracted resource paths stay under the native 16 KiB control limit.

## Source lifetime and playback

`crates/sda-format` supplies the shared bounded file reader. PCM is sample-exact;
AAC-LC uses vendored FAAD2 2.11.4, with explicit channel reordering, edit-list
priming, rational-phase SRC and exact master duration. Seeking keeps the
original descriptor/sample table, resets the decoder and uses bounded preroll.
It does not reopen a possibly removed picker copy or decode the entire song.

Extracted resources live in a source-owned private temporary directory and are
hash-checked. Android passes the application's writable `cacheDir`, not the
system Rust temporary default. These temporary asset files are not a loudness
or rendered-audio cache. Stop/source replacement releases them. Native polled
status exposes the authored policy flag, not track titles, artwork or paths.

Desktop uses the native executable's `--sda-file serve` path before audio-device
startup. The helper sends framed JSON and bounded float PCM, with owned IPC
sessions and cancellation. Both mobile hosts detect SDA signatures before their
ordinary MP4/codec heuristics. iOS bypasses Apple system-spatial offload for these
files. User enhancement controls are bypassed/disabled until an ordinary source
restores its normal settings.

## Distribution and checks

`ffmpeg-static` 5.3.0 / binary release b6.1.1 supplies the desktop encoder. The
staging script selects the target OS/architecture and includes the exact binary
license and source instructions. FAAD2 source and notices are in `vendor/faad2`
and the desktop/mobile distribution resources. The required Nero attribution
is included in the shared CLI and notices.

Meaningful regression coverage includes:

- Sample-exact SDAR PCM and unchanged original masters after conversion.
- All twelve AAC channels and duration at 44.1/48/96 kHz, including 5.1 identity.
- Stream/pull, backwards/forwards/end seeks, AAC/SRC phase and resource teardown.
- One physical audio track/ID, explicit codec selection, and ordinary-player
  rejection of the private M4A codec/config identities.
- Live-versus-authored upmix rendering at 5.1, 7.1.2, 7.1.4, 9.1.6, 11.1.8
  and 22.2, with programme and spatial balance both on/off (max error < 2e-5
  in the controlled float-PCM fixture; separate AAC channel/clock checks).
- WAV original metadata, cover, exact lyrics, ADM/chna and render-schema roundtrip.
- Shared-engine scenes with embedded room, EQ/FIR, per-channel parameters,
  file gain authority, layout adaptation and missing-resource rejection.
- Web/mobile TypeScript, mobile layout/import and production-fixture checks.

Host checks are not a claim of physical iPhone/Android listening or visual
acceptance. Xcode/device and Android host builds must be recorded separately.
