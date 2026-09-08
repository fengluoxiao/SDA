# ADM/BWF Playback

SDA reads PCM WAVE masters with embedded ADM (`axml` + `chna`) using bounded
file range reads. RIFF, RF64 and BW64 chunk offsets are discovered from headers,
including `ds64` sizes, unknown chunks and odd-byte padding. Metadata may precede
or follow the PCM data. Every desktop read stays within its 1 MiB IPC limit.

The player maps PCM tracks through CHNA and ADM references, keeps fixed beds,
and schedules object positions, gain, extent and jump/interpolation events on
the PCM sample clock. Browser and native renderers accept diffuse energy and
Cartesian or polar speaker exclusion regions.
Native playback resamples PCM and event times together when needed.

ADM PCM is batched into 4096-sample frames. Desktop object metadata is sent in
acknowledged groups of at most 32, bounded to 16,000 encoded bytes so complex
object updates stay within the native protocol's 16 KiB message limit. An
individual event too large for the protocol is rejected, never truncated.
Native protocol version 7 prevents an older sidecar from silently ignoring zones.
Raw ADM object tracks bypass speaker-based loudness
measurement and cached normalization: they are not a BS.1770 speaker layout.
Silent samples still advance metadata, envelopes and filter tails, while zero
contributions skip native bus accumulation.
Native PCM reads use a small coherent cache per track to reduce scattered
memory accesses across 118 rings. Desktop file reads use 1 MiB chunks and
diagnostic logs are batched asynchronously so disk logging cannot block PCM ACKs.
Native convolution uses 256-sample partitions for long room/headphone FIRs.
The two convolution stages add 10.67 ms at 48 kHz (5.33 ms more than 128-sample
partitions), separate from output buffering. PCM and event timestamps are unchanged.

Supported PCM formats: signed 16/24/32-bit and IEEE float 32/64-bit, up to 128
tracks, including WAVE extensible with matching valid bits. Ordinary mono,
stereo and explicitly masked multichannel PCM are also accepted. Large masters
must use `playFile` or `openSeekable`; `open` alone cannot preflight tail ADM.

This is SDA's VBAP-based rendering, not a complete BS.2127/Dolby reference
renderer. External common definitions, HOA/matrix channels, multiple programmes,
interactive objects, time-varying beds and unsupported position constraints
are rejected explicitly. Non-AXML metadata chunks are limited to 64 MiB;
AXML is streamed with the resource budgets described below.

## Feature-Based Compatibility

Multiple DirectSpeakers packs can contain the same speaker labels. Each PCM
track retains its own source identity (`bed:<channel>`), gain/mute state and
sample stream; matching labels route to the same speaker without overwriting
other tracks. `rawBedLabels` describes unique speaker identities, while the
full labels array describes every PCM track.

`zoneExclusion` carries a union of Cartesian boxes or polar azimuth/elevation
regions. It is preserved through resampling, event coalescing, scheduling,
desktop IPC and both renderers. Polar ranges can cross +/-180 degrees; poles
match every azimuth range. Bounds use nominal room speaker coordinates with
1e-6 tolerance. Excluded speaker energy is redistributed by layer priority,
front/back half, Cartesian distance, then front/back distance; ties share
energy. As in EBU EAR, excluding all speakers uses the identity downmix.
This is a general constraint on SDA's existing VBAP gains, not an implementation
of EAR's separate allocentric extent panner. Neither filenames nor zone names
select rendering behavior. Layout changes recompute regions for the new layout.

Reference algorithms: [EBU EAR zone downmix](https://github.com/ebu/ebu_adm_renderer/blob/master/ear/core/objectbased/zone.py)
and [zone membership](https://github.com/ebu/ebu_adm_renderer/blob/master/ear/core/objectbased/gain_calc.py).

`node scripts/verify-adm-fixtures.mjs` preflights the six locally downloaded
Dolby/BBC/Netflix masters, or accepts explicit paths. It reports PCM tracks,
bed tracks, object counts, exclusion events and recovery warnings to
`tmp/adm-regression/masters.json`. Audio fixtures are not committed or bundled.

## Legacy Music ADM

CHNA-only track references and boolean `diffuse` values are supported. Blocks
are sorted by start time, with one sample of boundary rounding tolerance.
Gaps mute the object until the next block starts.

BWF playback uses a `latest-start` recovery policy for overlapping object
blocks: the next block permanently supersedes the previous block, including
when it ends before that previous block's declared end. Declared interpolation
speed is retained until the next event interrupts it. Duplicate start times
and invalid durations still fail. Each truncation is recorded in
`AdmMetadata.warnings` and logged by the player. The standalone ADM parser
defaults to rejecting overlaps unless recovery is explicitly requested.
This is a playback approximation for malformed metadata, not a reconstruction
of the author's intended trajectory; the input file is never rewritten.

BBC SAQAS music fixtures: https://www.bbc.co.uk/rd/publications/saqas
(`CC BY-NC-SA 4.0`). `flower_duet_bwf.wav` has five object tracks;
`machine_aer_bwf.wav` has 24 and requires four overlap recoveries.
Downloaded audio remains local and is not bundled with the application.

## Reference Fixture

### Large AXML Import

BWF import uses the `saxes` streaming XML parser with 64 KiB range reads.
The first pass retains the reference graph without audio blocks; the second
converts each block to compact playback data and releases its XML nodes.
References may precede or follow their definitions. Unordered blocks are still
sorted before event generation. The former 64 MiB AXML ceiling is removed;
other metadata chunks retain their existing limits.

Memory still scales with the playback timeline, but no complete AXML byte
buffer, string or block DOM is retained. Individual XML tokens and retained
blocks have a 1 MiB resource budget, the reference graph has a 16 MiB budget,
and nesting is limited to 256 elements. DTDs and invalid UTF-8 are rejected.
Import reads AXML twice in exchange for bounded XML working memory. The
synchronous parser remains available for small in-memory metadata consumers.

BWF playback also contracts an explicit `jumpPosition interpolationLength`
that exceeds its declared block duration to that duration. This follows the
EBU ADM Renderer `check_blockFormat_interpolationLengths(fix=True)` recovery:
https://github.com/ebu/ebu_adm_renderer/blob/master/ear/fileio/adm/timing_fixes.py
Warnings are aggregated per object. Negative and nonfinite values still fail;
valid interpolation and overlap recovery semantics are unchanged. The standalone
parser remains strict unless `interpolationPolicy: "clamp"` is selected.
This applies to any exporter, including dense Logic Pro automation, and does
not modify the source WAV or guarantee the original renderer's trajectory.

- Official Dolby training download: https://dolby.ent.box.com/s/bart3gfwblydjysegkltoz08wqlvzspg
- Archive: `Exercise_Content_2-3.zip`, 1,216,530,368 bytes.
- Official archive SHA-1: `5afb8c8a8731ec1890c2a4394b746d21ca0efdf9` (verified).
- Local master: `tmp/dolby-natures-fury/Exercise_Content_2-3/NaturesFuryADM.wav`.
- 48 kHz, 24-bit, 118 tracks: 10 bed channels and 108 object channels.
- Duration: 108.6666667 seconds; 5,216,000 PCM samples per channel.
- The extracted video and original terms PDF are kept alongside the master.
- Downloaded media is a local validation fixture, not a redistributable app asset.

## Verification

```powershell
node scripts/test-adm.mjs
node apps/desktop/test/adm-capacity.test.mjs
node scripts/inspect-adm-master.mjs
node scripts/verify-adm-master.mjs
cargo test --manifest-path apps/native-renderer/Cargo.toml --offline --no-default-features
```

After playing the complete master in the desktop app, run
`node scripts/verify-adm-native-log.mjs` to check the latest native session.
The 2026-09-08 Windows run accepted all 1,274 frames and 5,216,000 samples per
track, with zero source starvation before EOF drain and zero output FIFO
underruns. Mean native render time was 4,145 us against a 5,333 us block budget.
The report, including the tested executable SHA-256, is saved as
`tmp/dolby-natures-fury/inspection/native-playback-verification.json`.
These timings describe this machine and configuration, not a universal guarantee.

The full-file verifier checks continuous sample positions, track counts,
finite PCM, event boundaries and per-track energy. Its report is written to
`tmp/dolby-natures-fury/inspection/playback-verification.json`. Synthetic tests
cover metadata order, fragmented reads, truncation and sparse >4 GiB RF64/BW64.
