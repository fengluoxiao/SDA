# MPEG-H / Sony 360 Reality Audio

SDA decodes local MHAS streams and MP4 `mha1` / `mhm1` tracks with an
Ittiam libmpegh WebAssembly module. Source revision:
`f7ff0ac78d4d83f0b853bf2dff2ef075c92724f8`.

Object PCM is captured before the upstream speaker renderer. OAM positions,
gain, spread and subframe timing feed the existing SDA object events and PCM
declarations. Channel-bed signals retain channel labels and are not counted as
objects. Multiple channel groups sharing a speaker are summed after the
upstream default group selection. Object IDs are stream-local indices, not
instrument names or authoring-project stem IDs. Rendering uses SDA's own HRTF.

## Build

Install and activate Emscripten 4.0.15. By default the build script looks in
`tmp/emsdk`; `EMCC` (path to emcc.py) and `EMSDK_PYTHON` can override this.
Run `pnpm mpegh:build`, then the usual `pnpm web:build`.
The decoder repository is fetched to ignored `vendor/libmpegh`; its revision
is checked before building. Build-only patches are applied in `tmp/mpegh-build`:
libc typedef compatibility, object/PCM capture hooks, and the ASI input-signal
count correction. Upstream source and notices are retained.

The generated module is separate from the Rust core. It executes in the
decoder worker, uses bounded WASM memory, and receives complete MHAS access
units only: retrying a partially decoded frame corrupts the arithmetic state.

## Verification

`pnpm mpegh:test` checks the included synthetic two-object motion fixture and
the upstream object test stream with whole-file and
317-byte input chunks. `node scripts/fetch-mpegh-test-content.mjs` fetches the
SHA-256-verified official music test (5.1.4 bed, alternate commentaries and one
OAM object (stationary in this programme)). Run `node scripts/test-mpegh.mjs
tmp/mpegh-test/fraunhofer-objects.mp4` to test that MP4. Test content remains in
ignored scratch storage and is not distributed with SDA.

## Scope

- Local unencrypted files only; no streaming-service login or DRM integration.
- Channel-only and HOA-only streams currently use the upstream stereo output;
  mixed HOA/object streams are rejected explicitly.
- Standard CICP channel-bed geometry is supported. Unmapped custom geometry
  fails explicitly instead of being shown as invented objects.
- Default scene selection is retained. An interactive programme/preset picker,
  exact MPEG-H enhanced-object exclusion/divergence rendering, and sample-exact
  gapless/truncation handling are not implemented.
- This is not Sony's proprietary ear-personalization renderer or a certified
  Sony 360 Reality Audio product. Decoder source licensing and patent licensing
  are separate; see the bundled Ittiam LICENSE and LICENSE2.

## Scene visualization

MPEG-H uses a listener-centred full sphere; other codecs retain the rectangular
room view. The display radius is 2 scene units, matching the existing normalized
object directions. Its height is 4 units, from -2 to +2 (the room was -0.6 to +2).
This is display scale, not a claimed Sony room dimension in metres. Object
coordinates and audio rendering are unchanged. No floor cuts the lower hemisphere.
The 2D compatibility view uses a circular outline and marks negative elevation.

Source: https://github.com/ittiam-systems/libmpeghe/blob/main/encoder/impeghe_oam_enc_utils.h
specifies azimuth -180 to +180 degrees and elevation -90 to +90 degrees.

## Android native reuse

Android uses the same Ittiam libmpegh revision as Windows
(`7ff0ac78d4d83f0b853bf2dff2ef075c92724f8`), the same
`packages/core/mpegh/bridge.c`, and the same source preparation and
PCM/OAM capture patches in `scripts/prepare-mpegh.mjs`. Windows compiles
these C sources into WASM; `crates/sda-native/build.rs` compiles them into
the native Android library. The Rust module only adapts MHAS framing,
FFI ownership, channel labels and events to the shared render pipeline;
it does not implement a second MPEG-H audio decoder.

Supported Android inputs are raw `.mhas` and MPEG-H tracks in
`.m4a/.mp4` (`mha1` and `mhm1`). Mobile imports the existing
Windows `Mp4Demuxer` and shared MHAS packet writer directly. Kotlin performs
bounded file I/O and temporary-file cleanup, without MP4 audio decoding.
MPEG-H selects the existing `360RA-13` layout, including lower speakers.
Desktop and Android import the same SphericalRoom component and speaker definitions, including the full lower hemisphere. If room processing is enabled,
the selected/remembered room follows the playback layout, using byte-identical
Windows 7.1.4 and 360RA room assets; disabled room processing stays disabled.

The source metadata preserves relative OAM radius and does not invent
`distanceM`. Channel-only and HOA-only programmes use the same upstream
stereo fallback as Windows. Mixed HOA/object rejection and unmapped-bed errors
remain explicit. This Android output path currently requires 48 kHz MPEG-H;
other sample rates fail explicitly rather than playing at the wrong clock.
Windows's separate reference-stereo loudness measurement/normalization UI is
not connected on Android. Decoder parity does not establish end-to-end
subjective listening parity or certification.

### Native build prerequisites

Use Node.js (set `SDA_NODE` if it is not on PATH), Rust with the desired
Android target, and Android NDK Clang/llvm-ar. The build prepares the pinned
upstream source automatically; the first build requires Git/network if
`vendor/libmpegh` is absent. A host GNU build needs a complete GCC toolchain
(including cc1), not only Rust's bundled linker.

Example PowerShell for the current x86_64/API 26 emulator build:

```powershell
$ndkBin = "$env:ANDROID_NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin"
$env:CC_x86_64_linux_android = "$ndkBin/clang.exe"
$env:AR_x86_64_linux_android = "$ndkBin/llvm-ar.exe"
$env:CFLAGS_x86_64_linux_android = "--target=x86_64-linux-android26"
$env:CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER = "$ndkBin/clang.exe"
$env:CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS = "-C link-arg=--target=x86_64-linux-android26"
cargo build --manifest-path crates/sda-native/Cargo.toml --target x86_64-linux-android --release --no-default-features --locked
```

Copy the resulting `libsda_native.so` into the matching app `jniLibs`
ABI directory before assembling the APK. MPEG-H LICENSE and LICENSE2 are
packaged in the APK under `assets/licenses/libmpegh`.

### Regression evidence (2026-09-30)

`scripts/test-mpegh-native.mjs` compares real Windows WASM output with
native C output, before HRTF. Build the `mpegh_capture` example; set
`SDA_ADB` and `SDA_ADB_SERIAL` to run the Android executable on a device.
Both 1-byte and 1024-byte input chunks were checked on MuMu x86_64:

| Fixture | Frames | Float PCM samples | Max PCM difference | Max metadata difference |
| --- | ---: | ---: | ---: | ---: |
| Two moving objects | 141 | 288,768 | 0 | 1.11e-16 |
| Channel-only CICP 6 | 469 | 480,256 | 0 | 0 |

`scripts/test-mpegh-import.mjs` builds genuine mha1/mhm1 MP4 fixtures,
executes the mobile importer, and checks all decoded frames, PCM and objects
against the original Windows MHAS output. Extracted inputs also passed the
Windows/Android comparison. Rust regression tests cover chunk invariance,
exclusive C-bridge ownership, truncated EOF, and decoder reopening.
