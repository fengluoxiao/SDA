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
