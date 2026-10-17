# iOS MPEG-H decode stack overflow (2026-10-02)

## Evidence and scope

A device crash from the previously delivered build (`24773b0`, CI run
36991841159) matches the archived executable UUID. The faulting queue is
`sda.ios.decode`, with `Thread stack size exceeded` and a guard-page access
in `___chkstk_darwin`. Its stack mapping is 544 KiB. The app frame at offset
`0x3f3418` follows a stack probe requesting `0xf37f0` (997,360) bytes.
This is not the new CI-only PHASE prototype, nor a signing rejection.

The pinned upstream `impeghd_mhas_parse` declares an entire
`ia_drc_payload_struct` as a temporary, but passes only its `str_loud_info`
member to `impd_drc_mpegh3da_parse_loudness_info_set`. Because the compiler
reserves the function frame at entry, even streams without a loudness packet
can exhaust a dispatch worker's stack.

## Fix

The shared preparation script replaces that local with exactly the member's
type (`ia_drc_loudness_info_set_struct`). Parsing calls, ownership, bitstream
consumption, rendering and PCM processing remain unchanged. No static scratch
storage, heap lifetime around longjmp, larger feed threads, or audio-route
changes are introduced. The upstream vendor checkout and license are unchanged.
Patch anchors are checked so a changed upstream source fails preparation.

## Regression checks

- Native decoding on an explicitly 544 KiB thread for object/source and 7.1.4
  speaker layouts, including a type-22 empty loudness packet. Source PCM,
  reference PCM and OAM must equal normal-stack decoding.
- Apple device Clang compiles both the original and patched parser at the
  production `-O2`, ARM64, iOS 16 target. CI records both stack frame sizes and
  rejects a patched frame exceeding 64 KiB.
- Simulator playback now also exercises MPEG-H through the default KU100 route
  and the actual `sda.ios.decode` queue, in addition to the existing E-AC-3,
  system 7.1.4 and isolated PHASE smoke tests.
- Device archive uploads its dSYMs and executable UUID for subsequent crashes.

Local Windows GCC `-O2 -fstack-usage` measured 997,456 bytes upstream and 22,048
bytes after the patch. Apple compiler results and packaged-build verification
must be checked from CI; local compilation is not real-device validation.

The original crash report contains device/user identifiers and is intentionally
not committed. Physical-device replay of the affected track is still needed.

## Verified CI package

Source commit: `a878ca07412bbd0eb4f5949c85228f29368f44b9`.
GitHub Actions run: `37007949181`, SDK26 job `110840567976` completed successfully.
Artifact: `SDA-iOS-SDK26-13` (ID `11228027829`). SDK27 is an unavailable-SDK
skip, not iOS27 validation.

- Apple ARM64 device compiler: upstream 997,456 bytes; fixed 22,032 bytes.
- Native 544 KiB thread tests passed on the macOS host.
- Default MPEG-H/KU100 simulator playback: decoded = consumed = 144,384
  samples, FIFO drained. Real `sda.ios.decode` queue exercised.
- System 7.1.4 playback: decoded = consumed = 144,144 samples, 12 channels,
  pause/resume and non-interrupting preference toggle passed.
- E-AC-3/KU100 and isolated PHASE prototype smoke checks also passed.
- Device IPA inspected: ARM64, minimum iOS16.0, built with SDK26.5. Unsigned;
  existing valid signing/provisioning is still required for installation.
- Matching app dSYM was archived and downloaded. Device executable UUID:
  `72084B86-8663-3CF2-A076-221052C3479C`.
- IPA SHA256: `ac5bd98d358012f430bdc37eed6f300845a6778fb1553be617d799986249518f`.

These checks do not replace physical replay of the user's affected track,
AirPods listening, or iOS27 testing.
