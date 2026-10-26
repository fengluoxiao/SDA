# Desktop / mobile audio alignment, 2026-10-26

The Windows and macOS shells use the same native Rust renderer as mobile.
The audible desktop output requires the sidecar; an older staged executable can
therefore hide otherwise shared source fixes. No release or tag is created by
this integration.

## Shared renderer behavior retained

- Active HRTFHQ profile `apple-hrtfhq-542-v1`, measured response v3, shared depth
  v1, clarity v1; package staging verifies the selected assets on all platforms.
- `front-direct-mirror-lowband-v2` front symmetry and bass preservation.
- +2.5 dB programme output makeup for supported formats when balance is enabled.
- Common linked true-peak protection and current directional spatial balance.
- One object-rendering preference restores both direct-object and continuous
  direction processing. The removed separate directional UI stays removed.
- Current near-field and room presentation defaults are preserved.

## Desktop gaps closed

- Programme normalization selects one startup/cached reference. Later metering
  continues for the next-play cache without retargeting active playback.
  ALAC uses the mobile seven-block startup threshold; other reference paths
  keep their existing longer startup requirement.
- ALAC upmix uses the mobile L/R, center, LFE, surround, rear and height matrix;
  removes the obsolete side high-pass; applies the fixed -2 dB calibration once;
  maintains the source graph and filter state through a 50 ms dry/wet toggle.
- Independent persisted HRTFHQ spatial-balance control is restored after resource
  replacement/restart, without reopening the track or altering its clock.
- MP3 is decoded in the existing worker, including gapless/VBR streams. Bounded
  owned stereo blocks use the same SRC, metering and native renderer. Mono stays
  centered; no new MP3 pseudo-upmix mode is introduced.
- Native health exposes the executed renderer revision, bass and spatial policies,
  program gain/makeup; player health exposes the fixed programme-balance policy.
- Builders respect custom Cargo output directories and stage binary provenance.
  Desktop development launch is portable to Windows. Desktop version is aligned
  to 0.9.25 without a new release version.

## Validation boundaries

Windows is compiled and exercised locally. macOS receives the shared source and
packaging changes; a macOS executable cannot be validated on this Windows host.
Synthetic MP3/SRC and programme-control regressions do not replace listening on
actual headphones. The local Windows compiler required a shortened diagnostic
feature list for windows-rs; its upstream source/manifest is restored afterwards.
The GNU C++ static link also explicitly includes libgcc_eh for GCC 16 TLS.
