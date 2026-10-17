# Audio Vivid channel-bed integration (2026-10-18)

The user supplied `夢限大みゅーたいぷ - これはぼくたちの生存のあらすじ.m4a` and requested Windows, macOS, Android and iOS playback. This explicitly resumes the older feasibility investigation.

## Verified source

The MP4 `av3a` track is channel-based 7.1.4: 12 bed channels, zero independently coded objects, 44,100 Hz, 16-bit declaration, 832 kbps. All 10,111 coded frames have valid CRCs, no skipped bytes and no configuration changes. The compressed audio contains 24,418,065 bytes and 10,353,664 samples per channel (234.776961 seconds). The supplied music stays outside the repository. A full standalone source-decoder pass succeeded, not just header detection.

## Shared implementation

- Vendored the source decoder from `1254qwer/avs3a-rust` revision `59213e47a27ffa6ba9211bac1f5a6c6fd182e9d8`, retaining LICENSE and normative-asset notices. Native decoding retains its bounded Rayon pool. wasm32 executes the same independent neural stages serially without attempting to spawn unsupported native threads. No synthesis/PRNG/gain algorithm is changed.
- `packages/core/src/av3a_pipeline.rs` performs CRC-checked incremental framing, float decode, one PCM-unit normalisation, explicit bed-label routing and streaming sinc conversion to the common 48 kHz renderer clock. The common conversion removes initial SRC delay and flushes exactly `round(inputSamples * 48000 / sourceRate)` output samples. It does not quantise to PCM16 or add an Atmos gain boost.
- The upstream all-profile constructor overflowed a 512 KiB test worker even with a channel-bed input. The adapter selects separate, non-inlined mono/stereo/MC factories, avoiding the unused HOA construction stack. A direct optimized MC decode passed on the same 512 KiB worker; an optimized adapter regression is included.
- Mixed/object and HOA streams return an explicit unsupported-routing error rather than misrepresenting their channels as fixed speakers. This implementation validates the supplied channel-bed programme; it does not claim object reconstruction or HOA rendering.

## Host routing

MP4Box 0.5 does not classify unknown `av3a` sample entries as audio, so the common MP4 parser reads those entries directly. It preserves all original compressed access units, channel count, sample rate, duration and normal music/cover tags.

The desktop worker chooses the shared AV3A WASM decoder. Windows/macOS use the existing Dolby layout/rendering path (7.1.4 in auto mode, with existing manual layouts retained). The raw `.av3a` extension is accepted by the desktop file picker. UI format labels display Audio Vivid.

The existing bounded native import bridge extracts AV3A M4A audio to its owned compressed-track lifetime, exactly as AC-4/MPEG-H already do. This is compressed container extraction, not a pre-render, loudness analysis or PCM cache. Android/iOS accept the resulting `.av3a` source, feed the same Rust pipeline and render the labelled bed into their normal 7.1.4 binaural/head-tracking path. The 44.1 kHz source must not be fed unchanged into the fixed 48 kHz mobile clock.

## Checks and diagnostic entry points

- `node scripts/test-audiovivid.mjs`: unknown MP4 entry, exact AU preservation, bounded writes, duration/progress, cleanup and all host entry points. With `SDA_AV3A_INPUT` set to the supplied local file it checks all 10,111 AUs.
- `cargo test --manifest-path packages/core/Cargo.toml --no-default-features --locked --lib`: existing codecs plus AV3A fragmentation, corrupt CRC rejection, exact output clock and truncated tail. The release AV3A tests also exercise an Apple-sized worker stack.
- `cargo test --manifest-path crates/sda-native/Cargo.toml --no-default-features --features ios-host --locked av3a_`: actual mobile facade, decoder clock, 12 bed source declarations and frame-router handoff.
- `cargo run --release --manifest-path packages/core/Cargo.toml --example av3a_probe -- input.m4a [output.f32le] [frameLimit]`: offline full-chain PCM/clock/topology/peak/RMS/checksum probe. Diagnostic files are created only when explicitly requested through this tool, never during app playback.

The public three-frame test excerpt is not a runtime music/mock asset. Source/native notices are included in desktop public assets, Android module assets and iOS resource staging.

## Results from this integration run

- Whole supplied M4A: 10,111 / 10,111 CRC-valid frames; 12 source channels; standalone decode and shared WASM decode both completed.
- Actual shared WASM pipeline: 11,269,294 output samples per channel at 48 kHz (234.7769583 seconds), continuous sample origins, no decode errors/non-finite PCM. The measured local decode/check pass took 114.575 seconds for 234.777 seconds of music; this is a workstation measurement, not a mobile-device benchmark.
- Native versus WASM on the first 431 source frames (10.00779 seconds): all 12 channel timelines match; maximum absolute float difference 8.16584e-6, RMS difference 1.55811e-7. Neither path quantises/clips before the shared renderer.
- Exact production adapter plus 12-channel SRC: optimized 512 KiB worker smoke passed. This tests the actual adapter source, not a mocked decoder.
- Core unit regressions: 31 passed, 1 pre-existing external DTS-fixture test ignored. Actual mobile facade/FrameRouter AV3A test passed.
- Existing AC-4 import, MP4 timestamp/extraction lifecycle and iOS playback/queue bridge tests passed. Mobile TypeScript passed. Standalone demux/player TypeScript checks require overriding their existing narrow `rootDir` to the repository root for monorepo source imports; those checks passed after correcting pre-existing diagnostic references to a nonexistent `DecodedFrameData.samplesPerChannel` (use actual PCM channel length instead).

These are decoder, bridge, compiler and waveform checks on the development host. No four-platform device listening or new app-package installation is claimed.

Shared core cross-target checks passed for `aarch64-apple-ios`, `aarch64-apple-darwin` and `aarch64-linux-android`; Windows native execution and wasm32 release execution passed. These checks compile decoder code, not complete signed platform packages.
