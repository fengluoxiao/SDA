# ADM/BWF Playback

SDA reads PCM WAVE masters with embedded ADM (`axml` + `chna`) using bounded
file range reads. RIFF, RF64 and BW64 chunk offsets are discovered from headers,
including `ds64` sizes, unknown chunks and odd-byte padding. Metadata may precede
or follow the PCM data. Every desktop read stays within its 1 MiB IPC limit.

The player maps PCM tracks through CHNA and ADM references, keeps fixed beds,
and schedules object positions, gain, extent and jump/interpolation events on
the PCM sample clock. Browser and native renderers accept diffuse energy and
the Dolby ZB/ZT exclusion pair that restricts an object to horizontal speakers.
Native playback resamples PCM and event times together when needed.

ADM PCM is batched into 4096-sample frames. Desktop object metadata is sent in
acknowledged groups of 32 so 108-object updates stay within the native protocol's
16 KiB message limit. Raw ADM object tracks bypass speaker-based loudness
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

This is SDA rendering, not the Dolby reference renderer. General exclusion
regions, external common definitions, HOA/matrix channels, multiple programmes,
interactive objects, time-varying beds and unsupported position constraints
are rejected explicitly. Metadata chunks are limited to 64 MiB.

## Reference Fixture

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
