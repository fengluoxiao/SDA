# Apple renderer boundary audit — 2026-10-04

## Scope and outcome

User clarified that the required fix is cross-platform (Windows, Android, iOS), in the shared renderer. Apple APIs are reference information only; do not implement an iOS replacement backend as the solution. The earlier AVPlayer comparison proposal below is not the current work plan.

The 7.1.4 -> KU100 audition did not recover the user's target upper-left/right harmony. It is not an Apple renderer comparison. No production audio defaults are changed by this investigation.

## Source provenance

The web lookup tool returned empty results. The following Apple resources were instead retrieved directly with PowerShell Invoke-WebRequest, and their body/documentation JSON was inspected. Local evidence directory: `E:/SDA/artifacts/very-first-night-audit`.

1. Apple Logic Pro monitoring formats: https://support.apple.com/en-il/guide/logicpro/lgcp179f27c1/mac (`apple-logic.html`).
2. Apple Logic Pro binaural render modes: https://support.apple.com/en-bh/guide/logicpro/lgcp789f000d/mac (`apple-binaural.html`).
3. Apple AudioToolbox multichannel sample: https://developer.apple.com/documentation/audiotoolbox/generating-spatial-audio-from-a-multichannel-audio-stream ; machine-readable body https://developer.apple.com/tutorials/data/documentation/audiotoolbox/generating-spatial-audio-from-a-multichannel-audio-stream.json (`apple-spatial-sample.json`).
4. Apple WWDC21, Immerse your app in spatial audio: https://developer.apple.com/videos/play/wwdc2021/10265/ (`apple-wwdc.html`, transcript included). Historical session, not a complete statement of all 2026 platform support.

## What these sources actually establish

- Logic's Apple Renderer Music mode uses Apple Music headphone virtualization; Movie mode uses Apple TV headphone virtualization. Head tracking and personalized spatial audio are separately described controls. This establishes the identity of Logic's monitoring option, NOT equivalence of every Apple spatial audio API to that option. [1]
- Dolby Off/Near/Mid/Far binaural render modes do not affect the Apple Renderer monitoring options. Therefore missing Dolby distance-mode handling is not by itself an explanation of the Apple-only harmony. This does NOT mean Apple performs no distance processing. [2]
- Apple's public multichannel sample instantiates the SpatialMixer audio unit, stereo output, UseOutputType algorithm, and AmbienceBed input source mode. AmbienceBed spatializes input channels as far-field sources according to their channel layout. PointSource instead treats the input signal as a single source, subject to the documented in-head exception. Treating a whole multichannel bed as one PointSource would be a different operation. [3]
- The sample article contains an internal mismatch: prose mentions MPEG_7_1_A / eight-channel input, but its displayed code uses Atmos_7_1_4. Do not infer channel count/order from the prose alone; inspect the downloaded implementation and SDK layout definitions before wiring PCM. [3]
- AVPlayerItem and AVSampleBufferAudioRenderer expose allowedAudioSpatializationFormats. This is permission to spatialize, not proof that spatialization actually ran. The session distinguishes route capability/user permission via isSpatialAudioEnabled and system controls. [4]
- None of these inspected sources provide the internal Apple Music HRTF coefficients, reflection impulse responses, object-to-ear DSP, or complete Music-mode processing parameters. They cannot justify reconstructing an Apple renderer by inventing room reflections or EQ.

## Actual repository route for this song

Inspected `apps/mobile/modules/sda-core/ios/SdaPlayer.swift`, `CompressedInput.swift`, and `SystemSpatial360.swift`:

1. `CompressedInput` uses AVAssetReaderTrackOutput with outputSettings:nil for M4A/MP4. Apple demuxes compressed E-AC-3 access units; this is NOT Apple-decoded Atmos playback.
2. `play` selects SystemSpatial360 only for ext == mhas AND the systemSpatial360RA preference. An M4A never enters that branch.
3. `startNative` creates the SDA renderer with outputChannels:2. AVAudioSourceNode provides two PCM channels to AVAudioEngine.mainMixerNode. Thus the original objects/height layout are no longer available at this handoff. This observation does not establish whether any downstream system stereo treatment occurs.
4. SystemSpatial360 really does submit 12 explicitly labeled PCM channels through AVSampleBufferAudioRenderer and requests .multichannel, but its decoder is the MPEG-H bridge. It cannot simply be enabled for E-AC-3.
5. Its status currently records systemSpatial360RAActive=true when the path exists. It does not read isSpatialAudioEnabled. That flag proves route selection, not an audible Apple spatialization result.

These are architecture findings, not proof that the missing harmony was lost at the two-channel handoff: SDA intentionally performs binaural rendering before that handoff.

## Optional Apple-only reference ideas — not the implementation plan

A. Original M4A -> AVPlayerItem/AVPlayer, bypassing CompressedInput, Rust JOC, KU100 and SDA room processing. Keep this an explicit diagnostic mode, not a replacement default. Record player/item errors and route state; do not silently fall back to SDA. A successful AVPlayer status alone does not prove this local file was rendered as Atmos or by Logic Music mode.

B. SDA E-AC-3 objects -> explicitly labeled multichannel speaker PCM -> Apple public renderer. The current offline bus714 tool already binauralizes its speaker buses through KU100; its stereo files cannot serve as this input. A new pre-HRTF PCM tap is required. Do not pass E-AC-3 data to the MPEG-H bridge. Do not claim this speaker projection preserves the complete object scene.

C. Current SDA direct-object/KU100 baseline, unchanged.

Capture route port type/name, route isSpatialAudioEnabled, sample rate, requested spatialization policy, selected backend, errors, and user-controlled head-tracking/personalization conditions. isSpatialAudioEnabled is not a fingerprint of the underlying renderer nor a guarantee of Music-mode identity. Avoid double head rotation or treating loopback/screen recordings as guaranteed captures of the headphone endpoint.

Interpretation: if A recovers the harmony and B does not, investigate the SDA decoding/metadata/speaker-projection boundary before tuning HRTF again. If A and B recover it but C does not, investigate binaural rendering. If A does not recover it, first establish whether A actually reproduces the user's Apple playback conditions; do not conclude the harmony does not exist. This is not an authored-stem oracle.

No integration, iOS compilation, physical-device audition, CI dispatch, commit or push is claimed by this research note. Direct/directional defaults, center relocation off, mobile dry128 assets and disabled room/near-field remain unchanged.