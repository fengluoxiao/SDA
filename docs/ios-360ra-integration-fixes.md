# iOS 360RA routing and integration fixes (2026-10-02)

## Route contract

- Source layout stays `360RA-13`: 5 horizontal + 5 upper + 3 lower positions; no LFE speaker. It is not 11.3. Apple output layout is independently 7.1.4 (12 channels).
- The iOS-only switch selects the next playback route. It does not stop/reopen the current song. UI distinguishes the saved preference from the active route.
- Off: captured MPEG-H objects and non-HOA bed channels enter the SDA renderer, using the saved KU100 rendering preferences and optional room. Final stereo is binaural output, not a plain downmix. Existing HOA-only fallback still uses upstream stereo; this change does not implement native HOA source rendering.
- On: upstream MPEG-H CICP19 speaker PCM enters AVSampleBufferAudioRenderer. KU100, near-field and room are bypassed. Actual Apple spatialization remains a system/device decision.

## Changes

1. Room selection is scoped to the source layout. Legacy single-room preferences migrate only if compatible. When the saved system preference is off but current playback is still system-rendered, the user may select a room for the next SDA playback. UI labels that room as pending rather than active.
2. The system decoder retains original OAM events alongside speaker PCM. Serialized polling exposes positions at the consumed sample clock. Speaker channels are never labeled as source objects. Status polling also drains consumed OAM so background playback does not accumulate a whole-song timeline. Tracks without transmitted object positions cannot show real object dots.
3. Non-HOA bed-only MPEG-H content now preserves captured bed PCM rather than defaulting to the upstream stereo reference. Normal object rendering and upstream reference metering remain unchanged.
4. The scene owns gestures starting inside its viewport and disables both ancestor scroll views until release, termination or unmount. Scene and volume gesture locks compose; gestures outside the scene still page normally.
5. Original playlist metadata is passed after playback opens, keyed to the content hash to reject stale updates. Now Playing publishes title, artist, album, album artist, artwork, duration, elapsed position and rate for both routes. Playback updates preserve metadata. No custom Live Activity or imitation Dolby signaling is added.
6. The system route accepts the existing balance preference and live toggle. A dedicated multichannel K-weighted/gated loudness meter measures the 7.1.4 speaker PCM (LFE excluded from loudness, horizontal surrounds weighted 1.41; all channels checked for true peak). Attenuation targets -18 LUFS / -1 dBTP without boosting. A common smoothed gain is applied to all 12 channels before submission. It does not alter routing or impose binaural processing. It needs initial measurement accumulation; already submitted Apple buffers are not rewritten. This meter is distinct from the upstream stereo reference used by the normal SDA route; stereo loudness caches are not reused.

## Verification

- Mobile TypeScript check passed.
- Local native regression: 39 passed, 1 ignored, 4 diagnostic dump tests excluded. Includes 544 KiB decoder-stack regression, speaker PCM chunk/interleave invariance, object timeline polling, smooth uniform gain/toggle preservation and multichannel/stereo loudness calibration.
- Simulator assertions added for both production routes showing two real fixture objects; system Now Playing title/artist/album/artwork and stale-track rejection; balance control and uninterrupted next-play switch behavior.
- macOS compilation and simulator assertions must pass in GitHub Actions before an IPA is called verified. A simulator cannot verify Dynamic Island presentation, iPhone gesture feel, AirPods spatialization or subjective loudness.
