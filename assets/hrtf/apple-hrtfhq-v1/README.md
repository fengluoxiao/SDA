# Shared HRTFHQ response bank

This bank contains synthetic transfer measurements of Apple's public AVAudioEnvironmentNode HRTFHQ renderer. SDA uses these data in its own shared Rust convolver on iOS, Android, Windows and macOS. No Apple framework is invoked during playback by this profile. It is not Apple Music's Atmos renderer, personalized Spatial Audio or AirPods processing.

The dense bank retains the existing542positions, including the218horizontal-support set and full-height coverage. The17speaker fallback is measured with the same algorithm/session. Held-out scene positions from the research capture are deliberately excluded from production geometry.

Each512tap planar stereo file has the captured nonzero response, one common stereo gain, and162common leading zero samples. Truncated trailing samples were required to be exactly zero. There is no per-ear, per-object or per-direction level normalization. See calibration.json and provenance.json for measurement hashes, the synthetic front reference and capture identity. The exact same files are staged for every platform.

The measured direct-response policy disables KU100-specific far-ear suppression, H13 adjustment and the later G80 residual. Schema revision2 separately enables the user-approved half-strength retained61 short residual through the shared reflection buses; see apps/native-renderer/assets/shared-depth. It never mixes a second direct HRTF into the signal. Those old assets and algorithms remain in the repository. The new default reproduces the accepted response presentation while retaining SDA source decoding, actual object directions, mirrored triangle support with a pairwise waveform clock (no dominant-anchor switch), continuous convolution and head tracking.

Rebuild with scripts/build-shared-hrtfhq.py --capture <downloaded-synthetic-artifact> --calibration assets/hrtf/apple-hrtfhq-v1/calibration.json. The555-direction capture also includes13diagnostic/standard extra positions; only the shared542grid and17standard positions are packaged.

Official API documentation:
- https://developer.apple.com/documentation/avfaudio/avaudio3dmixingrenderingalgorithm/hrtfhq
- https://developer.apple.com/documentation/avfaudio/avaudioenvironmentnode
- https://developer.apple.com/documentation/avfaudio/avaudioenvironmentnode/outputtype
- https://developer.apple.com/documentation/avfaudio/avaudioenvironmentdistanceattenuationparameters

Measurement settings:48kHz stereo headphone output, mono point source, identity listener, radius1.2m, inverse attenuation reference2m/rolloff1, no reverb/occlusion/obstruction. Repeated impulses, amplitude scaling, and2/15co-located-source sums were checked. These checks establish bounded linear stationary behavior under the tested conditions; they do not assert identity to every Apple OS/hardware version.

The presentation preset enables the user-selected common base on entering this response family: spatial-only linked +2 dB and world-coordinate auxiliary lift up to +4 dB, giving net +2/+6 endpoints with front-45-to-side-90 geometry. The previous auxiliary +1.5 dB and separate rear +3 dB are not stacked on this base. The subsequently accepted B adds the common 4 kHz/+1.5 dB shelf, governed by the same enhancement control. The subsequent user correction removes the independent object-bearing E-AC-3 +2dB presentation boost, leaving the common net +2/+6 base; decoder loudness metadata remains unchanged. This is source/renderer processing, not baked into head-relative HRTF samples, so turning the head does not change which authored source receives the layer gain. Same-family resource reloads preserve an explicit later user override. iOS migration restores the same default; Android and desktop obtain it directly from the common engine.

Compatibility guard: these manifests use measuredResponseVersion3 plus sharedDepthVersion1 and sharedClarityVersion1 and intentionally omit the legacy calibrationVersion. Older native libraries must reject the new profile instead of silently applying their old interpolation/defaults. Android package verification also rejects native libraries without the versioned shared-depth loader. Release builds must rebuild the Rust/JNI library; copying only response assets into an older binary is unsupported.


Profile revision3 (2026-10-19) retains these512tap measurement files and explicitly adds the user-approved common-ear midrange layer during native preparation. Prepared filters are640taps to preserve the added FIR tail. See apps/native-renderer/assets/shared-clarity. The current common-base preset replaces the separately integrated rear gain; no second rear lift is applied. This is SDA's enhanced presentation, not unchanged Apple output. Older libraries must reject version3, and packaging checks require the Shared HRTFHQ clarity profile v1 marker.

Lower-speaker preparation uses this bank's own lower hemisphere for missing below-ear speaker anchors, rather than flattening them through the 17-direction companion bank. This includes 360RA-13's -20-degree lower speakers between the measured -15/-30 rings. Exact measured speaker anchors retain priority. The historical KU100 supplementary lower bank is not added to this response family.
