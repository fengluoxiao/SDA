# iOS-only 360RA system spatial audio

The default remains SDA KU100. The iOS settings switch persists a preference for the **next playback**; changing it never stops or changes the current route. It applies only to imported MPEG-H/MHAS (360RA), not E-AC-3/Atmos or MP3, and is not exposed on Android or desktop.

When enabled, the existing upstream MPEG-H decoder renders objects/bed/HOA to CICP 19 (7.1.4) PCM at 48 kHz, rather than relabeling the 360RA-13 source. Signed interleaved PCM24 is converted to Float32 without changing channel positions. Order: L, R, C, LFE, rear L, rear R, side L, side R, top front L/R, top rear L/R. The Apple format carries explicit channel descriptions matching this order.

A separate C ABI and AVSampleBufferAudioRenderer/AVSampleBufferRenderSynchronizer path bypass MobileEngine, stereo mixing, KU100, near-field, room simulation and SDA loudness balancing. The normal volume slider still works. SDA DSP controls are disabled during this route; saved preferences are retained for normal playback. Pause, resume, stop, interruption, unplug and lock-screen controls use the selected route. Native/system queue sizes are bounded; format changes and decoder/output failures are surfaced rather than silently falling back to mislabeled stereo.

The renderer requests allowedAudioSpatializationFormats = .multichannel (available before iOS 16). Actual spatialization and head tracking depend on compatible output hardware and system settings. The API supports multichannel layouts generally: 7.1.4 is the selected output layout, not a claim that Apple rejects all other layouts. This is not Dolby Atmos encoding and does not add Dolby metadata to 360RA.

Tests use the existing generated motion.mhas fixture, not a copyrighted song. Rust verifies 12-channel PCM, source/reference route preservation, chunk invariance, exclusive decoder ownership and strict EOF. CI also checks simulator enqueue/playback-clock/drain, pause/resume and non-disruptive preference changes. A simulator does **not** prove AirPods spatial listening quality. Physical iPhone/AirPods testing is still required. Minimum deployment target remains iOS 16; iOS 27 is not validated when no SDK/runtime is installed.

## Verified build — 2026-10-02

Actions run **36991841159**, build source **24773b02165b242bc842b1b4a99d159f73907faa**, artifact **SDA-iOS-SDK26-10** (ID 11221207940), completed the SDK-26 job successfully. Actual environment: macOS 26.6.2, Xcode 26.6, SDK/runtime iOS 26.5, iPhone 17e simulator. The SDK-27 matrix result is only a skip because no SDK exists, not compatibility validation.

Simulator report: system360RA.ok=true; outputChannels=12; outputLayout=7.1.4; sampleRate=48000; decodedSamplePos=consumedSamplePos=144144; fifoFrames=0; allowedMultichannel=true; pauseClockStable=true; togglePreservesCurrentRoute=true; hrtfBypassed=roomBypassed=true; physicalSpatialListeningVerified=false. The normal KU100 compressed-MP4 route also passed and consumed all 47616 samples. The simulator screenshot verifies normal app launch and safe-area layout, not the settings toggle or physical spatial sound.

Independent IPA inspection: arm64, Info.plist minimumOSVersion=16.0, Mach-O minimum=16.0.0, SDK=26.5.0, iPhone/iPad support and background audio, no provisioning profile or signature directory. IPA bytes=82790233, SHA256=e9dc48d79fe185a9f7b97d936a62cc6f5c25be2625a2010b836b4ffdc031d076. It still requires valid user-controlled signing before installation. Local native suite: 39 passed, 0 failed, 1 ignored; mobile TypeScript and existing rendering/balance/room/playback-order regressions passed.
