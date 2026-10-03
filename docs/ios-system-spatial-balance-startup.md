# iOS 360RA system-spatial normalization startup

## Failure mechanism

The old gate waited for six seconds of queued CICP19 PCM. The loudness meter,
however, required 57 **non-silent, absolute-gated** blocks before choosing a gain.
Six seconds of PCM is not six seconds of eligible loudness blocks. A quiet/silent
intro could therefore release the first buffer at unity gain, followed by a later
attenuation once the meter acquired enough eligible blocks. The system route also
ignored the `measured` command and returned `null` for `loudness`, so it could not
restore a complete measurement on replay.

## Policy

When normalization is enabled before first output:

1. Restore a complete CICP19 measurement from `sda.loudness.system714.v1.<hash>`.
   It is deliberately separate from the KU100/reference cache.
2. If no valid cache exists, scan the entire MHAS source with an independent
   7.1.4 decoder on the existing serial decode queue. Do not retain PCM or object
   events; do not start the Apple presentation clock or enqueue audible samples.
3. Freeze that full-track gain before reading playback PCM. The first nonzero
   sample, including after a silent intro, has the same gain as subsequent PCM.
   A complete short track does not need 57 gated blocks; an entirely silent track
   has unity gain. Gains only attenuate and are uniform across all 12 channels.
4. Cache by content hash. Preparation can take longer on the first uncached play;
   subsequent plays restore the result before the first submission. The UI uses
   the existing preparing-audio state; the media clock remains at zero.

Analysis does not hold the player lock while decoding and checks the playback
session generation per compressed chunk and before installing/caching results.
Switching track/stop cannot publish an old result into a new decoder. Import-copy
behavior, CICP19 channel order, object time alignment and KU100 rendering are not
changed. A user enabling normalization after playback already began retains the
existing smooth live gain transition (not a new full-track startup scan).

## Regression coverage

- Rust: six-second silent intro plus a four-second loud segment; first audible
  sample is attenuated using the completed measurement even with fewer than 57
  eligible blocks. EOF cannot overwrite that fixed gain.
- Rust: scan repeated real MPEG-H fixtures beyond the playback backlog cap with
  zero retained PCM/events.
- Existing Rust speaker ABI test: exact unbalanced 12-channel interleaving and
  object-clock preservation.
- iOS simulator smoke: uncached preparation and cached replay, requiring the
  full-track measurement and balanced first Apple buffer submission. This is not
  physical AirPods listening validation.

### Exclusive bridge ownership

The MPEG-H bridge supports only one active decoder. The full scan uses the
current speaker decoder in discard-only mode, under the player lock per bounded
chunk, then closes/recreates it for playback and installs the measured gain.
It does not construct a concurrent probe decoder. The separately opened analysis
reader survives JS unlinking, and per-chunk generation checks protect cancellation.
