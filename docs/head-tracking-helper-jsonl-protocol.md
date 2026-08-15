# Experimental head-tracking helper JSONL protocol

This is a future-facing, local-only protocol between an independently run Windows helper and SDA. It does not define Bluetooth transport, reverse engineer device packets, include LibrePods code, or require a kernel driver.

## Transport

- The helper writes UTF-8 JSON Lines to SDA's stdin or a named pipe selected by a future Electron integration.
- One object per line; messages larger than 4 KiB must be rejected.
- The helper must never send device identifiers, pairing keys, raw Bluetooth packets, or credentials.
- SDA treats every message as untrusted input and ignores unknown message types.

## Handshake

The first message establishes the helper version and calibrated coordinate convention:

```json
{"type":"hello","protocol":1,"source":"windows-airpods-experimental","coordinateSystem":"sda-adm-right-forward-up","orientation":"head-to-world-quaternion"}
```

`protocol` must be `1`. The required coordinate system is SDA ADM: +x right, +y forward, +z up. `orientation` is a normalized head-to-world quaternion. The helper, not SDA's renderer, is responsible for converting its device axes, handedness, and quaternion direction into this convention.

## Pose messages

```json
{"type":"pose","timestampMs":1735689600123,"orientation":{"x":0,"y":0.130526,"z":0,"w":0.991445}}
```

- `timestampMs` is the helper wall-clock epoch timestamp in milliseconds.
- Quaternion components must be finite numbers and have norm within 1% of one; the helper should normalize before emission.
- Pose rates should be 20–100 Hz. The future Electron consumer may coalesce samples and considers samples stale after its own timeout.

## Lifecycle and diagnostics

```json
{"type":"status","state":"connected","detail":"calibrated user-mode transport"}
{"type":"status","state":"unavailable","detail":"head-tracking service not available"}
{"type":"error","code":"calibration-required","message":"user recenter required"}
```

Permitted `status.state` values are `connected`, `disconnected`, and `unavailable`. `detail` and `message` are diagnostic text only and must not contain secrets. A future SDA integration owns start, stop, and recenter lifecycle; helper commands are intentionally unspecified until transport feasibility, consent, licensing, and security review are complete.

## Security and scope

The helper must run in user mode and use only transport methods it is permitted to use. SDA will not distribute a kernel driver as part of this experiment. This protocol is intentionally transport-agnostic so a future compliant helper can be evaluated without copying GPL LibrePods sources or embedding private AirPods protocol code in SDA.
