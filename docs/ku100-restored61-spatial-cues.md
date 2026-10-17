# Restored KU100 61-direction default with shared spatial cues

## Configuration
- Shared mobile default: `apps/mobile/assets/hrtf-restored/hrtf-dense/hrtf-set.json`.
- Keep sibling `hrtf/` (17 calibrated speaker anchors) when copying the pack. These are fallback anchors, not 78 independently rendered objects.
- Calibrated 512-tap dry samples copied byte-for-byte from historical snapshot 87b0423b6e60fe8de704a51314a99ee7af97c7c8. Both manifest dry/wet point to the dry file; original measured BRIR is not shipped.
- `historicalInterpolation=true`, `spatialCues=true`, `mobileDirectOnly=false`. The last flag is specifically the raw128 validation contract, not permission to enable user room simulation.
- Single high-resolution per-object preset, direct=true, directional=true, wet=0, room empty, near-field=false.
- Shared native renderer adds the existing 7.1.4 / 360RA13 depth/elevation residual profiles. It retains historical speaker direct anchors including the standard fallback, rather than replacing them with interpolated anchors.
- Historical interpolation deliberately remains historical: the newer raw128 interpolation cannot also be active in the same filter. This is not a claim that every line of the new interpolator was merged.

## Platform integration
- iOS staging, CocoaPods resource bundle, Swift asset path/status and portable bundle validation updated.
- Shared mobile preset plus Android asset copying and Gradle asset validation updated. No Android or Windows package built.
- Windows maintainer should copy the entire hrtf-restored directory and select its hrtf-dense manifest using the updated shared native renderer; desktop defaults have not been changed here.
- Old128 assets remain in the repository for existing regressions, not selected by mobile packaging.

## Verification and caveat
- Native library: 227 passed, 21 ignored. Includes both layouts with spatial cues active and exact preservation of direct speaker filters.
- Historical asset builder: 6 tests. iOS bundle validator: 5 tests. Mobile preset, iOS UI/playback tests and exact depth/elevation profile hash checks passed.
- Full Seiza172s replay and Ether ~62s replay completed without errors; exported both harmony windows and Ether210–270s. Ether telemetry confirms spatialCuesEnabled=true.
- Adding spatial residuals changes the complete mix, even though direct filters stay exact. Same four reference windows now correlate 0.94017–0.95010 versus 0.98878–0.99408 with cues disabled. Do not present this combined mix as identical to the user-approved dry audition, or correlation as a perceptual score.
- User-approved dry auditions remain intact. Combined previews: E:/SDA/artifacts/seiza-ios-regression/oct03-history/merged61/.
- No iOS build dispatched; no subjective approval of the combined previews claimed.

## Approved release update — 2026-10-05
- User approved the fixed -6 dB spatial-residual audition. Both production manifests now explicitly set spatialCueGain=0.5011872336272722, copied from the audition assets.
- Direct historical61 filters, per-object rendering and actual directions remain unchanged. Spatial cues stay enabled at reduced strength; do not describe their strength as unchanged.
- Same integer-aligned recording comparison: 0.974325–0.981574, not 0.9998. Native tests: 228 passed, 21 ignored before promotion.
- Approved audition: E:/SDA/artifacts/seiza-ios-regression/oct03-history/correlation-audit/cues-minus6db/.
- Release scope: iOS only. Windows/Android maintainers must use the same shared renderer and entire restored asset pack (including both manifests) to match the approved tuning.
