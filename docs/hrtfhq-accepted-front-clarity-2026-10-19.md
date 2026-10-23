# Accepted HRTFHQ front clarity — 2026-10-19

The user selected the **strong clarity** complete-mix audition after reporting that the front-common version was much better but the lead remained unclear. The shared HRTFHQ spatial-enhancement default now includes that selected processing. Fixed and head-tracked object playback use the same policy; the ordinary Off presentation remains separate.

## Processing

The previously accepted B base remains: linked spatial +2 dB, Front45 auxiliary extra at most +4 dB (net auxiliary at most +6 dB), and the common 4 kHz/+1.5 dB shelf. No independent rear gain or object E-AC-3 presentation gain is restored.

Eligible stationary front-corner objects are grouped by geometry and identical complete rendering routes. Multiple co-located objects therefore do not defeat discovery or multiply the correction. The groups' common component is rendered through the selected central HRTF; their difference retains the original left/right paths. A slowly updated input-spectrum reference calibrates common-component broadband energy. The selected **3 kHz, Q=0.9, +3 dB presence filter** affects only this new common direct excitation. Rear sources, Side excitation and reflection sends do not receive that filter. Original common reflection excitation is replaced, rather than duplicated, by the central send.

This is an explicit common-component presentation change, not vocal separation or an assertion that the measured HRTF is acoustically wrong. Common instruments can move and change timbre too. No title, time range, decoder object number, semantic classifier, runtime analysis file or audio cache is used. Measured HRTF files are unchanged. Beds, diffuse/extended/moving objects and unmatched front geometry retain their existing paths.

## Controls and lifetime

The policy follows the existing spatial-enhancement control only for the shared HRTFHQ family. Enhancement off disables front regrouping along with the existing B gains; the control remains an ordinary reversible runtime command. Same-family resource reloads preserve a disabled override. Transport reset keeps the selected recipe while clearing convolution and IIR history. Repeating the same tone configuration preserves IIR state.

The object and common-center pipelines remain warm during binaural Off when enhancement is enabled. Off-to-Fixed retains PCM, source identities, transport epoch and center identity. The geometry policy is format-independent, including object formats not identified as Dolby. Head pose changes the regular HRTF direction; normalization uses the neutral listener reference.

Archived replay configurations explicitly override regrouping and presence, so prior A/B definitions remain reproducible. Production replay without an audition config now verifies the actual accepted default.

## Validation and provenance

- Full native release library suite, serial: **360 passed, 0 failed, 35 ignored**. Non-test library check passed.
- Runtime tests cover enhancement on/off, initial/live HRTF load, disabled reload preservation, family changes, seek/reset, idempotent tone configuration, co-located object splitting, pure difference preservation, partial callbacks, and warm Off/Fixed/head-pose behavior in serial/parallel mixers for E-AC-3, MPEG-H and an unknown future object format.
- The iOS enhancement bridge/control checks and mobile Off/Fixed/tracking command checks passed.
- All **13** real diagnostic inputs (Happier plus the previous 12) completed the actual default path with finite output and unity diagnostic peak guard. This does not certify subjective acceptance of every new rendering or actual phone full-volume limiting.
- Actual default Happier matches the chosen strong native audition without fitted gain or time alignment: maximum absolute difference **8.1956387e-8**, relative residual **-130.459 dB**. The listening export's per-song gain is not a runtime setting.
- Separate strong-tone rear-only replay was sample-identical to the original rear-only output; pure difference and unchanged low-frequency gain are independently tested.

Local evidence is under `C:/Users/legendshop/Downloads/SDA-Happier-accepted-strong-20261019` and the preceding `SDA-Happier-vocal-clarity-20261019` delivery. The accepted raw strong audition SHA256 is `c1144f8e9e0bff5b6789750c035d9e14ee3f3edf32134bc1cff9eb5caaeffa69`. User media and diagnostics are excluded from Git.

The iOS workflow executes the runtime lifecycle tests and emits `front-presentation.json` containing the measured engine policy fields and build revision. Package verification must bind that receipt and the Mach-O revision to the requested commit. An earlier 2329393 IPA does not contain this change. Physical-device listening and installed-release visual verification remain separate from host tests and simulator smoke checks.
