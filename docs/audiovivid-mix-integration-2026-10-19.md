# Audio Vivid Mixed scenes — 2026-10-19

The initial AV3A integration supported channel-based speaker beds. It rejected
Mixed and HOA profiles before synthesis. Mixed transport channels describe coded
signals, not a fixed speaker layout: the programme's static references and dynamic
metadata must reach the existing object renderer. This change adds that shared
scene mapping; it does not change the approved HRTFHQ gain or clarity settings.

## Supported mapping

`packages/core/src/av3a_objects.rs` resolves the programme/content/object/pack/channel
graph. Explicit transport reuse, static object/channel gain and mute are applied
before rendering. Direct-speaker packs use ADM common-definition IDs (mono 1,
stereo 2, 5.1 3, 7.1 back 15); these are not AV3A header ChannelConfig indices.
Point-object packs retain per-instance source IDs and declarations. Dynamic
Cartesian/polar position, extent, diffusion, gain, mute and jumps become the
existing generic ObjectEvents. Dynamic object gain is applied by the renderer
once; bed gain is applied to PCM. Normalised position is preserved without adding
an inverse-square gain or inventing physical distance.

The pipeline retains the original 48 kHz output clock, trims only SRC latency,
and emits events at that clock even when the source rate differs. Logical layout
is known before SRC allocation. Seek resets decoder/SRC/dynamic history and may
retain the programme's static transport mapping. A topology change during playback
is reported rather than silently reassigning sources.

Common speaker-pack ordering was checked against EBU ADM common definitions,
`ear/fileio/adm/data/2094_common_definitions.xml` in `ebu/ebu_adm_renderer`.
Unsupported HOA, matrix/custom speaker packs, head locking, nondefault object
lock/divergence/screen controls and nonzero polar extents fail explicitly.
Absolute pack-distance/VR-room effects do not add separate loudness processing.
This is a supported subset, not a reference-renderer conformance claim.

## iOS route capability

The existing Apple 12-speaker system-output option remains available for the
verified channel-based 7.1.4 profile. A non-consuming raw-header capability probe
selects that route only for this profile. Mixed scenes keep the saved preference
but use SDA's native scene renderer so objects are not fed into a fixed speaker
buffer. Full header/CRC validation still happens in Rust. The C ABI also refuses
non-bed AV3A frames on its system speaker entry point.

## Private-file verification

The supplied file is not committed. Its 9,722 original 4,096-byte access units
contain a 48 kHz/16-transport Mixed scene. Static metadata expands it into six
5.1 logical bed roles plus 15 objects: the five full-range bed gains are zero,
and the reused transport contributes only to LFE. This is why interpreting all
16 signals as speakers or all 16 as objects would be incorrect.

The shared decoder processed all 9,955,328 samples (207.4026667 coded seconds)
on a 512 KiB worker stack, with finite output and 19,732 spatial events. The
full scene also passed the existing HRTFHQ stereo replay. Container edit-list
priming/trimming remains outside the elementary-stream pipeline; no new MP4
edit-list support is claimed.

Automated coverage includes scene reuse/LFE mapping, mute/motion/gain/reset,
invalid references and polar direction; existing bed clock/CRC/chunk/stack tests;
mobile scene routing and seek with an ignored environment-supplied private-file
test; Swift capability selection and preservation of the first header; and the
existing JS import/startup/output checks. No user audio, decoded PCM or per-track
runtime analysis cache is added to the repository or app.
