# AC-4 Playback

SDA integrates the MIT-licensed MacinDecode-AC4-Core scene decoder by
SakuzyPeng, pinned to `a23965312a53d3cfb1a63cbae692a788efe46123`:
https://github.com/SakuzyPeng/MacinDecode-AC4-Core

The reference integration is MacinDecode-AC4-Player:
https://github.com/SakuzyPeng/MacinDecode-AC4-Player

## Scope

- Raw `.ac4` sync frames, including extended sizes and CRC-protected frames.
- Unencrypted MP4/M4A `ac-4` samples, framed before entering the decoder.
- Full A-JOC reconstructed object PCM, independent LFE, and OAMD events.
- Object identity is mapped from the upstream 64-bit scene ID into SDA IDs;
  these are reconstructed spatial groups, not original authoring objects.
- Coordinates already use SDA's x-right, y-front, z-up convention.
- Position, fractional gain, extent, screen/depth factors and sample ramps
  are carried into SDA's object renderer. LFE gain ramps apply to its PCM.
- Inactive or semantically incomplete upstream states are muted, matching
  the reference player's conservative handling. No position is invented.

This is not support for every AC-4 presentation. The upstream Full A-JOC
subset and AutoUnique presentation selection determine decode availability;
ambiguous presentations and unsupported forms report an error. Zone snapping,
AC-4-specific headphone modes, DRC/dialogue enhancement, and MP4 edit-list
priming compensation are not implemented. Playback uses SDA's own renderer
and HRTF configuration. It does not require Dolby Access.

IMS investigation and the requested mutually exclusive rendering paths are
tracked in [ac4-ims.md](ac4-ims.md). IMS spatial playback is not yet implemented.

SDA vendors the small scene adapter with an opt-in opaque one-byte presentation
tail policy; the bitstream and DSP crates remain pinned Git dependencies.
See `vendor/macindecode-ac4-scene/SDA-PATCH.md`. The complete original metadata
payload is retained, strict syntax must succeed before the tail, and audio
substream/CRC validation stays strict. This is a playback compatibility policy,
not a claim that unknown metadata bytes are standard padding.

## Build

Rust 1.98 and its WASM target are selected by `packages/core/rust-toolchain.toml`.
The upstream decoder requires locally generated tables from official ETSI
documents. These downloaded documents and generated tables stay local.

```powershell
# PYTHON may specify an explicit Python 3 executable.
node scripts/prepare-ac4.mjs
node scripts/build-core.mjs
node scripts/test-adm.mjs
node scripts/verify-ac4.mjs path/to/input.mp4
```

`MACINDECODE_AC4_SPEC_DIR` can point to an existing prepared upstream `spec`
directory. Direct Cargo commands require this variable; the SDA build script
defaults it to `tmp/MacinDecode-AC4-Core/spec`. Upstream verifies SHA-256 values
for the downloaded inputs and generated tables during the build.

The verifier checks finite object PCM, continuous sample positions, event
boundaries, object declarations, movement and energy. It writes a report to
`tmp/ac4-verification/report.json`; it does not verify perceived audio quality.
