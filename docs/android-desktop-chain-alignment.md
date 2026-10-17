# Android desktop-chain alignment

This change aligns the supported E-AC-3/Atmos playback path with the current
Windows frontend/native-renderer source. It does not claim identical operating
system output APIs or verified equivalence to an unavailable historical Windows
installation.

## Processing contract

| Stage | Windows | Android |
| --- | --- | --- |
| Container | compressed E-AC-3 access units | MediaExtractor compressed access units; previously verified byte-identical on the test songs |
| Decoder | sda-core Rust compiled to WASM | same Rust sda-core compiled natively; PCM and metadata parity measured separately |
| Metadata | decoder JSON, then native sidecar serde contract | now the same serialized decoder fields and native serde contract, instead of handwritten casts |
| Routing | objectChannels/Obj labels, codec-clock source retirement, completed-target compaction | FrameRouter mirrors these rules |
| Submission | atomic PCM plus metadata frame, then batch ACK | same validation/commit function, with a reply channel back to the producer |
| Startup | 0.5 seconds accepted PCM, then StartAt(origin) | same gate for calibrated playback; pause blocks startup and EOF releases short tracks |
| Producer reserve | TARGET_AHEAD_SECONDS = 4 | four seconds against the presentation clock |
| Neutral head | ClearHeadPose | zero yaw now uses ClearHeadPose, not an identity HeadPose command |
| Render | dense calibrated KU100, object/directional paths, room, near-field, master guard | shared renderer and assets, with the requested features enabled |
| Output | float stereo to desktop device backend | float stereo to Media3; no lossy re-encoding |

`distanceM` is preserved as a nullable physical distance; `distanceInfinite`
remains distinct from an absent distance. No distance is invented for streams
that omit it. The renderer applies the same shared distance validation and
position-based fallback. `anchor`, `screenFactor`, and `depthFactor` are serialized
by the core on both platforms; the current Windows native event contract does
not consume them, so Android does not add a different interpretation.

Android now waits for renderer acceptance before advancing the accepted startup
watermark. Invalid/unknown-source transactions are rejected before metadata or
PCM is changed, and rejection reaches nativeFeed instead of being silently
reported as queued. The unconfigured no-HRTF reference-mix test utility retains
its existing path; the app always loads calibrated HRTFs.

Seek also queues the desktop Reset command before new source declarations,
rather than only flushing the decoder/FIFO while leaving renderer sources alive.

## Validation and limits

Regression checks cover finite-distance changes, infinite and absent distance,
gain-only metadata, paused startup, short-file EOF, single startup submission,
and atomic acceptance/rejection. Existing render-dump validation now paces by
the presentation clock; FIFO occupancy alone did not bound accepted source-ring
data and previously hid rejected batches.

The 12-second second-song host replay with all spatial features enabled differs
from the previous mobile-path replay by max 3.5762787e-7 / RMS 3.0711007e-8.
Against the older staged Windows executable, max 0.016890049 / RMS 0.0014875846
remains. These alignment changes therefore do not establish that the reported
front/back listening issue is fixed. The older binary residual is retained as
an unresolved comparison, not hidden by gain changes or disabled processing.

Validation completed: 21 mobile library tests passed; the shared renderer's
new atomic ACK regression passed. Native Android release and Gradle release APK
builds succeeded. Installed `E:/SDA/apk/SDA-android-desktop-chain-x86_64.apk` on
MuMu (`127.0.0.1:16416`), selected the second song and verified playback, a stable
paused clock at 0:52, then resume. The settings retained calibrated KU100 61,
both object switches, near-field 1.00 m and room enabled. No frame-rejection,
ACK-timeout or Media3 output error was observed during this smoke check.

APK SHA-256:
`3E92CC4230B31630A62B9C497BAB409B6AB1217FF5F0B7C2B8E2ABCECBE90FA1`.
The installed artifact's native library matches the newly built release library.
