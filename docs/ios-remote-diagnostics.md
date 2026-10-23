# iPhone Diagnostic HTTP Server

Enable Remote Diagnostic Service in Settings. No phone-side address, key,
pairing or discovery is required. The phone listens on TCP port 8765.

From the computer, open either:

```text
http://PHONE_LAN_IP:8765/diagnostics
http://PHONE_TAILSCALE_IP:8765/diagnostics
```

The root path also returns the snapshot. GET is read-only. Unknown paths return
404; other methods return 405. Responses contain playback counters, head pose,
thermal state, render timing and recent event names, not audio/song names/paths.
The old computer receiver and outgoing phone uploader have been removed.

The service is off at every app launch. Disabling cancels the listener and
existing connections. Up to four clients and 8 KiB headers are allowed; connections
close after one response or a five-second deadline. Snapshots are read on a
utility queue, never generated in the audio callback. No CORS access is granted.

There is no authentication or TLS: any reachable device can read the diagnostic
snapshot while enabled. Use trusted Wi-Fi or Tailscale ACLs and do not forward
the port to the public internet. Tailscale IP reachability depends on the iOS
VPN extension/routing and must be checked on the actual phone, not assumed.

Background audio may keep the app executing; iOS suspension stops the server
from responding. No silent audio, PiP, location or background assertion is added
to keep this server alive. Real foreground, background-playback and locked-screen
HTTP tests remain necessary. Source-contract CI is not a socket integration test.

The snapshot also includes allowlisted `errorDetails` categories/domain codes and
`input` reader counters. Raw exception descriptions, media titles, URLs and paths
remain local. Successful EC-3 cursor recovery records `compressedReaderRecovered`,
the verified-prefix byte count and indexed-reader position; it never starts a test
track, adds keepalive audio, or changes the decoder/render session.

CI smoke methods are compiled only for the simulator. The device IPA packaging
step removes `ci-stereo-tones.m4a` and `ci-360ra.mhas`; installed physical-device
apps cannot enter the simulator smoke path through environment variables.


空间平衡诊断：`spatialEnhancementEnabled` 是渲染器实际空间开关，`auxiliaryBalanceEnabled` 是当前允许辅助层控制的状态（硬件/solo 等旁路可禁用）。`auxiliaryBalanceLevels` 依次为辅助组实际包络 dB、历史最大辅助组衰减 dB、前方加权参考 dB、辅助组加权参考 dB。它不是 LUFS，不代表前方被施加同一衰减；每个源还按场景方向连续分配组权重。数值属于渲染器时钟，可能领先 FIFO 消费时钟；不应当作耳机当前瞬间的精确电平。所有格式复用同一诊断字段，仍不返回音频、对象内容、曲名或路径。
