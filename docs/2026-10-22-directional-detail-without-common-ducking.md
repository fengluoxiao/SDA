# Directional detail without live common loudness ducking

The user reports long-play level drops and occasional leftward vocal movement in the installed build 5f39d9e. They explicitly requested removing the live common volume reduction and shipping the native change instead of relying on browser auditions.

## Runtime policy

- Both spatial states now use `FinalMixAttenuator::process_programme_reference`: measurement continues, but its moving loudness estimate does not attenuate the audible mix. Decoded programme volume balance, listener volume, the requested format makeup (+2.5 dB with volume balance), and the independent linked true-peak guard remain.
- Spatial balance uses source-local spectral redistribution for weak rear/overhead material. A geometry-weighted foreground/auxiliary deficit controls contrast; it does not apply a fixed layer gain. The filtered component is normalized with its complementary low component and cross terms so the existing source energy is redistributed rather than raised and then paid for by reducing the whole mix.
- Protected foreground sources pass through the new detail processor unchanged. LFE stays excluded. The 250 Hz complementary split protects bass; detail emphasis is above 1.8 kHz. The scalar control slews, is homogeneous under source splitting, and is independent of head pose, codec IDs, song identity and song position. No analysis pass, new startup wait, PCM cache or additional HRTF convolver is introduced.
- The approved source/HRTF clarity and common bass treatment remain. Common-front references have a bounded one-second spectral history instead of accumulating the entire programme. A retained front pair must remain on opposite, approximately mirrored front directions; same-side/asymmetric movement returns to authored routes instead of remaining forcibly associated.

## Client / audition diagnosis

The prior browser audition is a Windows offline native render. It does not prove physical iOS playback parity. iOS may use the independent Apple system-spatial route, saved balance/tracking/cue preferences, a different installed revision, or different output volume. Resource byte identity alone cannot settle a listening discrepancy.

The shared facade and iOS read-only diagnostics now expose the actual renderer revision, algorithm, live-ducking policy and effective configuration alongside programme gain and balance states. Apple system rendering remains identified separately. No user preferences are silently reset to make a comparison match.

## Validation contract

Tests cover source-energy/bass/detail response at 44.1/48/96 kHz, amplitude homogeneity and source splitting, 128 independent source states, foreground spectral isolation in both mixers and volume states with neutral/turned heads, bounded common references, opposite-side geometry, and sample-for-sample no-ducking after quiet/loud histories. Existing broadband-controller regressions remain explicitly legacy tests.

Physical iOS listening is not claimed. Package source hashes and algorithm identity must match the new commit. Decoded programme normalization and the true-peak guard are not a certification of final programme LUFS: cancelling live loudness ducking deliberately removes the prior moving LUFS ceiling. Actual final LUFS/TP values belong in the release evidence, not an unconditional standards claim.
