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

## Follow-up: startup / viewport / background (2026-10-02)

- System balance now pre-decodes six seconds (or the entire shorter input) before the first Apple PCM submission. The feeder bypasses its ordinary four-second backpressure only during this bounded pre-roll; the native eight-second cap remains. Initial gain is applied at sample zero; subsequent live changes retain the common smooth ramp. This is initial-window measurement, not whole-track advance analysis, so later integrated gain can still change.
- The native Canvas waits for nonzero viewport dimensions, receives explicit width/height, and stays mounted after the spatial page is first visited. A render error now reports a message instead of leaving an unlabelled blank. This mitigates GL layout/recreation failures; the reported Dolby-only blank still needs device reproduction and visual verification.
- Now Playing refreshes on entering background and returning foreground, adds local content identity and non-live status. This does not implement custom Live Activities or establish that the system Dynamic Island animation is fixed.
- Preparation status moved to the top Now Playing heading, including native system-balance pre-roll; the lower duplicate was removed.

### Additional device reports

- Balance preference is re-applied after opening the selected source/route. Simulator regression explicitly plays system -> native KU100 with balance retained, without touching its switch.
- Both horizontal pager and the spatial page's own vertical ScrollView now share the interaction lock (the latter was missing). OS edge gestures remain system-owned.
- EOF derives the real decoded duration; queue drain marks Now Playing stopped with zero rate and final elapsed time. Native remote resume cannot revive an exhausted decoder. Added end-state smoke assertion.
- Foreground re-entry opens the playback page and refreshes state; registered `sda://now-playing` routing provides an explicit playback entry. This is not a custom Dynamic Island activity or proof that the system's own tap launches the sideloaded app.
- AVFoundation operationInterrupted (-11847) is recoverable, not generic EOF. Compressed MP4 reader can reopen at the last delivered packet end (bounded retries), without restarting Rust; CI compares compressed bytes before/after reopen. Invalid timestamps still play normally but disallow unsafe resume. System renderer retains only its bounded submitted queue for replay following recoverable interruption; other errors remain visible. Real device interruption recovery remains unverified.


## Remembered iOS media directory

The iOS import path uses a retained UIDocumentPicker delegate with `directoryURL`
set to the previous original file-provider parent, persisted in UserDefaults.
Android still uses Expo DocumentPicker. Imported cache paths are never used as
the starting directory. Provider files are security-scoped only during a
coordinated copy; saving a parent URL does not grant directory access. Missing
providers or inaccessible directories may fall back to the OS default. Cancel
does not overwrite the saved location. Real-device provider navigation remains
to be verified (local Files and iCloud, including app relaunch).

CI confirmed the iOS SDK does not expose the interrupted-operation enum.
Recovery therefore matches AVFoundationErrorDomain plus the reported code
-11847 explicitly (rather than referencing a non-existent Swift enum case).

Simulator rerun exposed a pause race: the Rust pause command is asynchronous,
so AVAudioEngine could still consume FIFO data after the host reported pause.
The native route now also pauses the Apple consumer synchronously, and resumes
the consumer without resetting the Rust decoder/FIFO. System-spatial clock
pause remains handled by its synchronizer. Watchdog errors include route/clock
diagnostics rather than suppressing a stalled output.


## October 3 follow-up: import regression and cold-start scene

The custom open-in-place picker introduced in 87494e0 has been withdrawn.
Imports again use Expo DocumentPicker with copyToCacheDirectory:true on both
platforms. Remembered-directory behavior is deferred; no signing configuration
or additional entitlement was changed. This restores the previously used
import-copy contract rather than blaming the user's signing certificate.

The first scene now delays GL creation until visible with a nonzero measured
viewport, reports actual first-frame draw calls/triangles, and retries a stalled
context at most twice before displaying a retry action. No track, DSP or
decoder reset is used to refresh the visualizer. A separate cold-launch CI
process tests the initial 7.1.4 speaker scene without first visiting 360RA.
Draw calls and a simulator screenshot are not a real-device visual or acoustic
validation; the user's first-track Atmos case still requires device confirmation.
