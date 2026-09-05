# OpenJOC 0.16.0 reference attempt

Date: 2026-09-05

- Official release: https://github.com/chyinan/OpenJOC/releases/tag/v0.16.0
- CLI archive SHA-256: 0b08bfff916f57d90a4817c258111590409e2c85abf3abc6ecf50ecaf016433c
- Archive hash matched the release SHA256SUMS.
- Installed PotPlayer executable has PE machine 0x014c (x86). Published LAV package is x64; no plugin installed or player settings changed.
- No Dolby Reference Player installation found in checked common install locations/uninstall entries. This is not an exhaustive disk search.

## Results

Direct M4A input could not start because FFprobe is not on PATH.
The existing raw carrier `tmp/egaku-full.ec3`, previously verified against the source M4A in `tmp/egaku-fixed-24s.report.json`, was then tested.

Full carrier, both automatic and observed-vendor-compat validation:

```text
openjoc[malformed-input]: failed to validate OAMD profile: failed to read OAMD bitstream: truncated bitstream: requested 5 bits with 0 remaining
```

First 24 seconds extracted at E-AC-3 frame boundaries (750 frames, 1152000 samples, 2304000 bytes), observed-vendor-compat validation:

```text
openjoc[decode-failure]: OpenJOC decode error: failed to read OAMD bitstream: truncated bitstream: requested 5 bits with 0 remaining
```

No valid reference WAV was generated. These failures do not establish source corruption or explain the audible pumping. No decoder workaround was applied, to preserve the meaning of testing the official unmodified release.

## Reproduce

Run from D:/SDA:

```powershell
node tmp/openjoc-reference-0.16.0/extract-start.mjs
& 'tmp/openjoc-reference-0.16.0/cli/openjoc-0.16.0-x86_64-pc-windows-msvc/bin/openjoc.exe' render-joc 'tmp/openjoc-reference-0.16.0/egaku-start.ec3' --layout 2.0 --validation-profile observed-vendor-compat --drc disabled --output 'tmp/openjoc-reference-0.16.0/egaku-stereo.wav' --no-progress
```

Official Dolby Reference Player access: https://professionalstore.dolby.com/product/dolby-reference-player/01tQQ00000LRkYvYAL

## PotPlayer x64 live test

The user installed PotPlayer x64 and authorized unattended testing.

- Player: C:/Program Files/DAUM/PotPlayer/PotPlayerMini64.exe, preferences version 260819.
- LAV archive SHA-256: 77faf2a5775d39674f18b2b26813a280c8e8e80fa80696e43c1bf71b3e5cfb7d, matched GitHub release asset digest.
- Loaded the official runtime/LAVAudio.ax through PotPlayer's external-filter UI. No machine-wide registration or elevated installer was needed. The package installer/verify workflow was not run.
- Selected LAV Audio Decoder (OpenJOC) as preferred (Chinese UI: force use) in global filter priority.
- Played the original Documents M4A, not a re-encoded derivative.
- Live graph: Built-in MP4 Source -> LAV Audio Decoder (OpenJOC) -> Built-in Audio Codec/Transform -> DirectSound Audio Renderer.
- LAV properties identified LAV Audio Decoder (OpenJOC) 0.83.0. DRC was unchecked and compressed passthrough formats were unchecked.
- Stereo policy: admission OpenJOC, warning OpenJOC decode error, output PCM 0 channels / 0 Hz, all meters inactive.
- Binaural policy: built-in SADIE II D1, virtual layout 7.1.4, calibrated dialnorm. Applied settings, reopened the original track from the playlist, and inspected Status again: admission OpenJOC, same error and no PCM output.

Both policies reported:

```text
OpenJOC decode error: failed to read OAMD bitstream: truncated bitstream: requested 5 bits with 0 remaining
```

PotPlayer's general information displayed 2 -> 2 channels and 48 kHz, but the decoder Status reported no valid output. The general information is therefore insufficient evidence of successful decoding. The playback clock advanced despite the decoder failure.

The external filter entry was retained but unchecked after testing. No SDA renderer changes were made. This test establishes a reproducible OpenJOC compatibility failure for this file; it does not establish the cause of SDA's audible pumping or source corruption.

Recovery verified by reopening the same playlist item: graph returned to Built-in MP4 Source -> Built-in Audio Codec/Transform (internal FFmpeg eac3 + atmos) -> DirectSound Audio Renderer. Playback reported 48 kHz, 2 channels, 768 kbps PCM and showed active audio visualization. Playback was then paused. No subjective claim about audibility is made.

## Resolved: explicit eight-configuration trim profile

The first failing frame is access unit 0. Its 70-byte OAMD payload has an object element at bits 28..516 followed by a 32-bit trim element at bits 525..557. The object element parses successfully. The trim element is the failing element.

OpenJOC defaults to nine trim configurations, citing TS 103 190-2 (AC-4). TS 103 420 V1.2.1 clause 5.5.12 uses NUM_TRIM_CONFIGS without defining its cardinality. It does not justify applying nine universally to this E-AC-3 carrier.

For the first frame, trying explicit cardinalities 1..16 admits only eight with complete syntax and zero-padding validation. It decodes one custom configuration (index 1: height -4.5 dB, top/bottom Y balance +0.125), seven default configurations, and no per-object trim disabling. Reading a ninth configuration consumes the final flag/padding as configuration bits and then requests five unavailable bits.

Full-file probe with explicit eight configurations: 7515 access units, zero OAMD parsing failures. The initial 24-second probe with the default nine failed on all 750 access units. This is evidence for this carrier's explicit configuration, not an automatic universal eight-configuration default or vendor-renderer equivalence claim.

No production parser/math change was required: the official, hash-verified 0.16.0 CLI already exposes `--trim-config-count 8`. Both output files below were produced by that unmodified official executable. No object element, trim body, or malformed packet was skipped.

### Render commands

From D:/SDA in PowerShell (existing output files are intentionally protected):

```powershell
$openJocExe = 'tmp/openjoc-reference-0.16.0/cli/openjoc-0.16.0-x86_64-pc-windows-msvc/bin/openjoc.exe'
& $openJocExe render-joc 'tmp/openjoc-reference-0.16.0/egaku-start.ec3' --layout 2.0 --trim-config-count 8 --drc disabled --output 'tmp/openjoc-reference-0.16.0/egaku-stereo-trim8.wav' --no-progress
& $openJocExe render-joc 'tmp/openjoc-reference-0.16.0/egaku-start.ec3' --binaural --backend direct --trim-config-count 8 --drc disabled --output 'tmp/openjoc-reference-0.16.0/egaku-binaural-trim8.wav' --no-progress
node tmp/openjoc-reference-0.16.0/prepare-listening.mjs
```

Both renders completed 750/750 access units. Stereo took 15.12 seconds; binaural took 34.21 seconds for 24 seconds of source. Binaural uses the built-in SADIE II D1 KU100 HRTF, 7.1.4 virtual layout, and the official default LFE exclusion. Dialnorm is calibrated; encoded DRC is disabled. OpenJOC still reports experimental spatial bridging / unresolved semantic binding. This is an independent application-chain comparison, not an official Dolby reference.

The partitioned backend failed separately with `source 9 direction does not match HRIR entry 8802`. Its per-component direction tolerance is 1e-12 while HRIR lookup and the direct backend use a dot-product tolerance of 1e-12; these are different angular tolerances. The official default direct backend succeeded without modifying filters or relaxing checks. No partitioned-backend patch was applied.

### Listening artifacts and validation

- `listen-stereo.wav`: 23 seconds, 48 kHz stereo PCM16, RMS -28 dBFS, peak 0.5942.
- `listen-binaural.wav`: 23 seconds, 48 kHz stereo PCM16, RMS -28 dBFS, peak 0.5739.
- Start trim: 2048 samples to match the existing raw-carrier/MP4 comparison convention. Renderer latency retained (stereo 609 samples, binaural 577 samples).
- Every selected floating-point sample checked finite; output is non-silent and has no sample clipping. One fixed gain per clip, with no added compressor or time-varying normalization. Existing OpenJOC internal rendering/headroom behavior is retained.
- `comparison.m3u` orders existing FFmpeg core reference, OpenJOC stereo, OpenJOC binaural.
- `listening-report.json` and both `*-trim8-performance.json` files preserve numeric results.

Independent diagnostic checkout: `tmp/openjoc-oamd-fix`, based on ad6556b. Added an OAMD frame probe and a synthetic regression test verifying explicit eight-config parsing, decoded trim values, truncated-data rejection and zero-padding rejection. All 58 openjoc-oamd tests passed; `git diff --check` passed. Existing upstream source and SDA were not modified. PotPlayer's OpenJOC filter remains disabled: the published C adapter uses the default OAMD config and does not expose the CLI trim-count override.

The user subsequently reported that the audible pumping remained unchanged with these reference outputs ("still the same"). Successful parsing/rendering did not resolve the audible symptom. The bounded follow-up audit is recorded in `../../docs/egaku-object-envelope-investigation.md` and `../egaku-final-audit.json`.
