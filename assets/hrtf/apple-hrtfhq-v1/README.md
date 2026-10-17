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

The presentation preset also enables the already-auditioned SDA recipe on entering this response family: master0dB, world-coordinate auxiliary layer up to+1.5dB, and the existing shared4kHz/+1.5dB high shelf. Existing object-bearing E-AC-3+2dB compensation remains unchanged. This is source/renderer processing, not baked into head-relative HRTF samples, so turning the head does not change which authored source receives the layer gain. Same-family resource reloads preserve an explicit later user override. iOS migration restores the same default; Android and desktop obtain it directly from the common engine.

Compatibility guard: these manifests use measuredResponseVersion2 plus sharedDepthVersion1 and intentionally omit the legacy calibrationVersion. Older native libraries must reject the new profile instead of silently applying their old interpolation/defaults. Android package verification also rejects native libraries without the versioned shared-depth loader. Release builds must rebuild the Rust/JNI library; copying only response assets into an older binary is unsupported.
