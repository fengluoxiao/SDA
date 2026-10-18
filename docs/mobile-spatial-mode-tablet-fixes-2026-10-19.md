# Mobile spatial mode and tablet fixes — 2026-10-19

Scope: retain the user-approved B renderer recipe for all twelve listening materials. The user then deferred Android/iOS packaging, Git commit and push and requested source integration plus mobile fixes. No distributable app packaging, install, commit or push is part of this change. Release-profile host checks and Android Kotlin compilation are validation only.

## Shared Off / Fixed / tracking transition

The mobile facade previously selected FrameData.compatibility_bed while binaural output was off, discarded its object PCM/events and cleared the object snapshot caches. This applies to E-AC-3 and MPEG-H/360RA alike: the MPEG-H bridge already captures a codec stereo reference beside its object scene. Turning Fixed back on therefore operated on queued channel-only frames until newly decoded object frames caught up.

The facade now keeps the object decoder and frame router independent of the final binaural bypass. A complete scene and a codec-provided Lo/Ro reference are handed to the render worker in one PCM transaction with one acknowledgement. Compatibility PCM is checked before scene mutation, included in queue memory accounting and retained in bounded absolute-clock rings. It is consumed at the same sample clock and cleared by a transport reset. Missing reference coverage uses the established speaker-projection fallback rather than stale reference samples.

The full scene's filters, source identity, convolution histories and metadata stay warm while Off selects the compatible/plain output. A fully available compatible block also keeps the same object fast path active; formats without one retain the existing general-path plain projection. E-AC-3's spatial presentation compensation is prepared in the background and applied only through the spatial output mix. There is no song-specific rule, fixed mute delay, decoder reopen or seek on a mode change. Existing partition latency, 5ms guard lookahead and short output crossfades remain.

Object targets advance at the audio presentation clock while Off; only their visibility is suppressed. Fixed immediately obtains retained targets without waiting for a new codec metadata frame. Consumed events are pruned during feed so long Off sessions cannot accumulate an unbounded metadata queue.

## Metadata typography and tablet cover blending

Tablet and optional phone tablet-style playback reuse the existing gradient player's title font (20pt / 26pt line height) and artist font (19pt), keeping existing tablet spacing. The tablet-only 28pt title and 20pt artist overrides were removed. The user explicitly requested this synchronization without Expo Go review.

A small edge alpha mask now feathers the tablet's contained cover into the actual blurred backdrop underneath. It reuses the existing artwork fade curve on all four edges; the center is opaque and the outer edges transparent. Cover size, aspect ratio and layout are unchanged. Platforms/binaries without the native mask manager return the original image. The existing phone gradient/landscape edge path is retained.

## Head icon states

Off uses the ordinary icon color. Fixed uses the selected theme accent and a stationary icon. Tracking keeps the theme accent and rotates only the icon using the real head-yaw state; Fixed and Off force its display angle to zero. iOS native menus, their fallback and Android native glass buttons receive the same state. Menu actions and 44pt touch targets are retained.

## Verification

- Shared renderer release-profile host suite: **349 passed, 0 failed, 35 ignored**. This includes Atmos/7.1.4 and Sony 360RA-13, serial and parallel object paths, queued PCM without new input, retained convolver identity, compatible-output samples, malformed transaction rejection, reset behavior and applied listener-yaw telemetry.
- Mobile facade release-profile `ios-host` suite: **105 passed, 0 failed, 2 ignored**. A real MHAS fixture exercises the MPEG-H decoder, compatible reference and object snapshot through Off/Fixed. Unknown future codec labels cover uneven packets, delayed initial/intermediate reference delivery, release of multiple pending frames and non-Obj_ channel names.
- Mobile TypeScript and `test-ios-ui`, `test-spatial-playback-modes`, `test-mobile-spatial-indicator`, `test-artwork-edge-blend`, `test-tablet-player-layout`, `test-ios-spatial-enhancement` checks passed. The edge-mask generator's `--check` passed.
- `test-mobile-production-fixtures` passed using an in-memory production bundle; development preview/mock media are excluded.
- Non-test release-profile `cargo check --lib` passed for the shared renderer and the mobile facade (`ios-host`, no default features). Existing compiler warnings remain.
- Android native chrome `:sda-core:compileReleaseKotlin` passed (61 tasks, BUILD SUCCESSFUL). This compiles the controls only; no APK was assembled or installed.

Host suites use an optimized profile with LTO disabled through task environment variables; repository Cargo profiles are unchanged. Test logs are under `C:/Users/legendshop/Downloads/SDA-hrtfhq-base-fusion-20261019/logs`. No physical-device listening or visual parity is claimed by host checks.

## Shared contract for later object codecs

The mode setter now only sends the final-output control, stores the selected mode and permits an immediate UI snapshot. It does not call a decoder, clear object metadata, or inspect a codec name. Decoder object rendering remains an independent capability and stays enabled for ordinary spatial Off/Fixed/tracking selection.

A bounded PresentationQueue joins ordered compatible PCM chunks to the complete scene frames before routing them. This handles different packet sizes and delayed compatible output without altering object PCM, declarations, gain or metadata times. Only true input EOS may exhaust an auxiliary reference early, where silence fills its tail to the primary scene's duration. Startup publishes clocks of scene frames actually committed after joining, not the newest decoder packet. The common queue and router contain no E-AC-3/MPEG-H/AC-4/etc dispatch. Unit tests deliberately use an unknown future codec label and explicit declarations with non-Obj_ channel names, proving the behavior depends on the FrameData contract rather than a format list. A reference that starts late announces itself with labelled empty channels on the first scene frame; later absent chunks remain pending. This startup convention is documented in the common CompatibilityBed contract and covered by a regression test.

The head icon also consumes renderer-applied listener yaw from the common native status; it is not driven solely by the manual yaw control or a simulated loop. No additional motion/audio commands are issued by UI polling.

## Packaging follow-up authorized on 2026-10-19

After accepting the source fixes, the user requested a Git push and iOS CI packaging, plus a locally built Android Universal APK. This supersedes the earlier packaging deferral for the follow-up delivery. Only the iOS workflow is dispatched; Android packaging runs locally for the project's supported arm64-v8a and x86_64 ABIs. The physical-device validation limitation above still applies.
