# Audio Vivid / AV3A feasibility research

Date: 2026-09-14. This is a feasibility investigation, not implemented SDA support.

## Verified findings

- `raddyfiy/av3a_ffmpeg_decoder` v1.0 Windows archive downloaded and extracted under `tmp/audiovivid-research/windows`. Its ffmpeg runs and lists `libarcdav3a` as `AV3a decoder by ArcVideo (codec av3a)`. Decoder help reports no threading capability. No AV3A sample was decoded; performance remains unmeasured.
- Archive SHA256: `55b77de82ed8220945a331d325ab8394048c37a23189bd440bf5ca0af2833a0c`.
- `WoKee/ffmpeg-av3a` contains a wrapper and metadata headers. Its configure option describes `--enable-libarcdav3a` as decoding via ArcVideo AV3A. The wrapper relies on external `avs3_create_decoder`, `parse_header`, `avs3_decode`, `avs3_destroy_decoder`. This is not evidence of an independently buildable complete codec implementation.
- In `avs3_stat_dec.h`, decoder state includes `numObjsOutput`, `isMixedContent`, `numChansOutput`, and `hMetadataDec`.
- `avs3_stat_meta.h` describes static object/pack/channel references and dynamic channel metadata: `transChRef`, mute, polar/cartesian coordinates, distance, gain, extent, diffuse and jumpPosition. Static object records are not necessarily one-to-one mono PCM objects; mappings must be resolved.
- `wuxianlin/ijkplayer-av3a` includes ARMv7/ARM64 decoder and renderer libraries and a Java `AudioVividMetaDataHandler` exposing object count/name and dynamic channel azimuth/elevation/distance. Interface presence does not prove all streams populate the fields.

## Obstacles in the examined FFmpeg wrapper

`libavcodec/libarcdav3a.c`:

- Around lines 240–246: resets when `numObjsOutput > 6`. This is a wrapper restriction, not a claimed Audio Vivid standard maximum. It cannot be blindly removed without auditing buffers and library limits.
- Around lines 310–326: collects PCM count, object count and a pointer to decoded metadata, then resets `numObjsOutput`. The metadata is library-owned and must be copied before the next decode if consumed asynchronously.
- Around lines 529–570: caches metadata internally and exports PCM to AVFrame, but this path does not attach those decoded dynamic records as output frame side data. The `AV_PKT_DATA_AUDIO_VIVID` reference earlier in the function reads input packet side data; it is not an exported per-frame object interface.
- Binaural rendering is disabled in the shown packet path. PCM channels before rendering are a possible extraction point, but independent-object ordering, timing and channel mapping are not yet validated.
- Three-order HOA is labeled as a generic 16-channel layout in the wrapper. HOA must not be treated as 16 discrete speaker/object positions in SDA.
- The raddyfiy project issue #1 reports a 12-channel source producing five output channels. This is an unverified user report, but makes known-layout sample verification essential.

## Recommended SDA approach

1. Validate a known 5.1.4/7.1.4 AV3A sample with ffprobe and decoded PCM, checking channel count/order, duration, errors and throughput.
2. Validate a mixed bed/object sample. Extract PCM before the vendor binaural renderer and copy static/dynamic metadata for each decoded frame. Resolve object-to-pack-to-channel references and `transChRef`; derive timestamps from decoded sample counts. Verify polar/cartesian conventions and discontinuities against an independent renderer.
3. Use a separate native decoder adapter initially, rather than attempting to turn an opaque Windows/Android library into browser WASM. Feed SDA's existing bed/object PCM and position events; reuse the existing native-to-phone PCM transport.
4. Declare channel-only support separately from full object support. Do not synthesize moving objs from a mixed stereo or multichannel output.
5. Verify the decoder/renderer library redistribution terms separately from the FFmpeg wrapper's license before shipping binaries. A publicly downloadable binary or FFmpeg license is not proof of permission to redistribute every bundled component.

## Sources and limits

- https://github.com/raddyfiy/av3a_ffmpeg_decoder
- https://github.com/raddyfiy/av3a_ffmpeg_decoder/releases/tag/v1.0
- https://github.com/raddyfiy/av3a_ffmpeg_decoder/issues/1
- https://github.com/WoKee/ffmpeg-av3a/blob/master/libavcodec/libarcdav3a.c (use the repository default branch if this branch changes)
- https://github.com/WoKee/ffmpeg-av3a
- https://github.com/wuxianlin/ijkplayer-av3a/blob/master/ijkplayer-java/src/main/java/tv/danmaku/ijk/media/player/av3a/AudioVividMetaDataHandler.java (use the repository default branch if needed)
- https://github.com/nilaoda/av3a_decoder — archived September 2024; README says only 5.1.4 tested and very slow. Its historical linked source repository now returns 404; it is not an available integration dependency.

No known-reference Audio Vivid sample was found in the examined repositories/releases. Decoder startup is verified; real-file decode, object PCM separation, moving-object accuracy and real-time performance are not yet verified. No SDA runtime behavior was changed.

## Object extraction path (follow-up)

The headers expose the graph required to associate sound with a position:

- AudioObject.objectIdx / refPackFormatIdx → AudioPackFormat.packFormatIdx.
- Pack typeLabel differentiates representations; handle object packs separately from direct-speaker, matrix and HOA packs. Do not count every AudioObject container as an independently moving source.
- AudioPackFormat.refChannelIdx → AudioChannelFormat.channelFormatIdx. The pack also has transChRef and packFormatStartIdx, with a hasChannelReuse condition. Exact implicit-index rules require the matching metadata decoder/renderer or standard; do not guess zero/one-based indexing.
- Avs3MetaDataDynamic.transChRef associates each dynamic record with a transmission channel. Its parallel arrays include muteFlag and avs3DmL1MetaData (polar/cartesian position, gain and extent).

At each successful avs3_decode call, before another decode/reset:

1. Snapshot format/channel/object counts and deep-copy current metadata.
2. Copy decoded interleaved PCM. In this examined wrapper the output is signed 16-bit; samples per channel should be derived from returned bytes / (2 * channel count), rather than blindly using the wrapper's hardcoded 1024.
3. Resolve object/source identity and transmitted-channel association. Retain previous valid static metadata when an update is absent, according to the codec's update rules. Keep static object identifiers separate from dynamic-array indices.
4. Emit a PCM block and metadata events at the same sample origin; advance by the actual samples per channel. Account for container timestamps and decoder delay when aligning to video.
5. Feed those sources into SDA, without calling GetBinauralInterleavedAudioBuffer first. After binaural mixing the individual object signals are no longer available.

Why the current wrapper is insufficient: its dav3a_decode_frame can loop over avs3_decode and accumulate PCM, then exposes metadata from the final decoder state. Attaching that one metadata record to the whole accumulated PCM would mis-time moving objects. Export inside the individual successful decode step, not only at the final AVFrame assignment.

The native state header is conditional on build macros (MIX_DEVELOPE, MIX_EXT, METADATA_EXT, METADATA_UPDATE, etc.) and contains C long. A foreign-function binding must use the library's exact ABI and build options; copying a struct from another platform can return plausible-looking but incorrect coordinates. The static object-name header comment and bit-width constants also differ, so names must not be decoded by assuming UTF-16.

These associations are evidenced by headers and wrapper data flow. Exact transmission-index semantics, PCM order, metadata timing granularity, polar orientation and decoded gain units are still unverified without matching renderer/decoder implementation or a reference object sample. No working object extraction has yet been demonstrated.

## Sample discovery and successful decode

The archived nilaoda project's releases DO include samples (not in its repository tree):
- v0.2 sample2_ts.zip (15,021,163 bytes): https://github.com/nilaoda/av3a_decoder/releases/download/v0.2/sample2_ts.zip
- v0.1 sample_ts.zip (76,873,910 bytes): https://github.com/nilaoda/av3a_decoder/releases/download/v0.1/sample_ts.zip

Downloaded v0.2 under tmp/audiovivid-research/sample2. Its TS contains HEVC and AV3A, with the AV3A stream reported as 48 kHz, 10 channels, 5.1.4 and approximately 3.883 seconds. The Windows decoder successfully produced sample2-decoded.wav; elementary audio was extracted losslessly as sample2.av3a. This supersedes the earlier no-sample/no-real-decode status. Dynamic-object presence remains unverified: 5.1.4 channel reporting alone is not evidence either for or against objects.

Issue https://github.com/nilaoda/av3a_decoder/issues/1 reports that the older sample only has signal in the 5.1 channels; that package was not downloaded in this test. The UWA resources page was also checked and provides standards/white papers, not a verified downloadable moving-object sample: https://uhd-world-association.com/uwa-resources/

## Verified frame-level result (2026-09-14)

sample2 is channel-based, not a moving-object test. Parsed the entire 187,392-byte elementary stream as 183 consecutive frames, with no skipped or trailing bytes. All 183 payload CRCs match the header CRC using the decoder source CRC table. Every frame has codingProfile=0, channelNumIdx=8 (5.1.4), 48 kHz, 384 kbps and 1024 samples. Duration is 3.904 seconds. Independently coded objects: zero. This establishes the coding mode from frame headers, rather than inferring it from ffprobe channel reporting. It does not establish the absence of all static metadata.

Important extraction correction: the Windows FFmpeg stream-copy data command produced an EMPTY file despite opening the stream. Recovered the original compressed bytes from ffprobe -show_packets -show_data, then validated the full frame chain and CRCs. The elementary file now contains 187,392 bytes. Diagnostic script and report: tmp/audiovivid-research/inspect_sample2.py and sample2-header-report.json.

Examined source candidate: https://github.com/nicaicaii/avs3a (research checkout under tmp only; source licensing/provenance has not been approved for integration). src/decoder.c explicitly distinguishes profile 0 channels, profile 1 objects/mixed beds, and profile 2 HOA. src/avs3_metadata_dec.c assigns dynamic metadata numDmChans from numObjsOutput. A profile-1 sample with known moving positions is still needed to validate actual object PCM extraction, metadata mapping and motion. Successful multichannel decoding must not be reported as successful object decoding. No SDA runtime integration was made in this step.

## Additional online sample search (2026-09-14)

Found a new independently hosted elementary AV3A fixture:
- https://github.com/1254qwer/avs3a-rust/blob/main/tests/fixtures/test.av3a
- Direct download: https://raw.githubusercontent.com/1254qwer/avs3a-rust/main/tests/fixtures/test.av3a
- Description: https://github.com/1254qwer/avs3a-rust/blob/main/tests/fixtures/README.md
- Downloaded to tmp/audiovivid-research/rust-test.av3a, 21,012,915 bytes.
- SHA-256 e8648fe7a67fdafe94bf6d9653d510a6f6a574aa293a24e29e8fc257d6e01574 matches the repository README.
- Independently parsed all 8,701 frames with all CRCs passing and zero trailing bytes: profile 0, channel configuration 10 (7.1.4), 44.1 kHz, 832 kbps, 202.036825 seconds. No independently coded objects. Useful for channel decoding and duration/performance testing, not moving-object validation. Full PCM decode has not yet been run for this fixture.

Other leads screened: xatabhk/avs2-avs3-video-samples predominantly supplies AVS video samples and is not evidence of AV3A object audio. MediaInfoLib issue 1843 says samples were sent privately by email, not posted publicly. androidx/media issue 2737 is an unrelated FLV audio declaration problem and was excluded. The older nilaoda v0.1 sample download was also initiated; it should not be called an object sample without header verification.
The v0.1 archive download subsequently completed (76,873,910 bytes) and extracted successfully to tmp/audiovivid-research/sample1/sample.ts (77,372,340 bytes). The attempted packet extraction returned no audio packets; no object/channel-mode conclusion is drawn for this older file. Ignore its empty sample1.av3a and empty report as failed extraction artifacts.

## Final check and user-directed stop (2026-09-14)

Rechecked public GitHub object/sample leads; no independently verifiable object-mode fixture was found in this search. This is not a claim that no such sample exists anywhere.

Resolved the old sample extraction without relying on the FFmpeg demuxer: reconstructed TS PID 257 payloads and stripped PES headers. sample1-pid257.av3a contains 482,304 bytes, 471 consecutive frames, all CRCs valid, no trailing bytes. Every frame is profile 0, channel config 8 (5.1.4), 48 kHz, 384 kbps; duration 10.048 seconds; zero independently coded objects. This supersedes the previous unknown status for the older sample.

Three examined samples are now confirmed channel-only (5.1.4 / 5.1.4 / 7.1.4). Per the user's instruction to stop Audio Vivid if no object sample is found, Audio Vivid integration is stopped. Preserve research and fixtures for reference; do not add runtime support or imply object decoding has been validated.


## Resumed integration with supplied file (2026-10-18)

The user explicitly resumed Audio Vivid work and supplied a 7.1.4 channel-bed M4A. All 10,111 frames/CRCs and a full standalone PCM decode are now verified. The newer pure-Rust source decoder is integrated into the shared core and native/WASM host entry points; see `audiovivid-integration-2026-10-18.md` for scope and validation. This supersedes the earlier user-directed stop for channel-bed playback. Independent object/HOA routing is still not claimed.
