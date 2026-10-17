# iOS accepted balance package — 2026-10-08

- Workflow run 66: https://github.com/fengluoxiao/SDA/actions/runs/37769175360 (success)
- Source: `577bda573438c71b977dd6cda57cd96c2a5d68cc`; tree-identical API publish, selected five files only. Existing unrelated working changes untouched.
- IPA: `E:\SDA\artifacts\ios-accepted-balance-37769175360\SDA-accepted-balance-SDK26-run66-unsigned.ipa`
- SHA256: `71c25857f5265ee24b073fc169d3a34b81740fb8d0373be5402a1dbb013375e7`
- Size: 16369449 bytes; arm64 iPhone, SDK26.5, minimum iOS16.4, unsigned (requires signing).
- SDK27 produced only unavailable logs, not an IPA.

## Preset
Existing persisted/default-off switch now labeled 空间平衡 · −0.75 dB. Enabled: master +2dB, auxiliary layer 0dB, main restore2dB, final constant -0.75dB AFTER linked guard (20ms ramp on live changes). Disabled restores defaults. System spatial route bypass unchanged. This replaces previous auxiliary +6 preset globally when switch enabled; no song/time/ID special cases. Earlier How Love target acceptance for this new preset remains unverified; do not claim universal vocal correction.

## Verification
258 native tests passed, 0 failed, 27 ignored. iOS preference/UI/bridge portable checks passed. Actual production-preset render vs accepted Very First Night 0–65 audition max absolute sample error 3.6214314913518564e-7. CI completed. Artifact GitHub digest verified; IPA device plist/arm64 Mach-O/current UI strings/native bridge/61-point HRTF verified. No physical-device listening or signing certification.
