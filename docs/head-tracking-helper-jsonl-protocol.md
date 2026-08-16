# Experimental AirPods head-tracking helper JSONL protocol

This is a local-only protocol between SDA and a separate Windows helper process. SDA may ship the GPL helper executable as an independent program, and users may explicitly select a compatible external helper. The protocol does **not** grant Bluetooth access to the renderer or install a driver. The helper is responsible for its transport and all device-specific calibration.

## Scope and security

- SDA starts either its fixed packaged helper path or a user-selected, non-symlink `.exe` regular file; it supplies no user-controlled command-line arguments.
- The helper runs in user mode and SDA never elevates it. The Windows installer may separately offer an explicit, opt-in driver setup step; the renderer itself receives no Bluetooth/L2CAP/device access.
- The helper must never emit device addresses/identifiers, pairing keys, raw Bluetooth packets, credentials, or private protocol diagnostics.
- All helper stdout is untrusted UTF-8 JSON Lines. SDA accepts a maximum line length of 4 KiB and buffers at most 8 KiB before terminating the helper. Unknown message types are ignored; malformed known messages terminate the helper.
- Every known stdout message (`hello`, `pose`, `status`, `error`) must carry `protocol: 1` and the active session token. A duplicated `hello`, a bad protocol version, or a bad token terminates the helper.
- This is experimental AirPods motion tracking, **not** Apple Personalized Spatial Audio and not affiliated with Apple.

## Launch and session

SDA creates a fresh random 256-bit hex `session` token for every helper launch, starts the resolved executable with no user-supplied arguments, then writes this JSONL command to helper stdin:

```json
{"type":"start","protocol":1,"session":"<64-lowercase-hex-characters>"}
```

Every helper stdout message below must echo that exact `session`. SDA rejects a missing or mismatched session token.

SDA may later write these stdin commands:

```json
{"type":"recenter","protocol":1,"session":"<token>"}
{"type":"stop","protocol":1,"session":"<token>"}
```

Helpers that cannot recenter internally may ignore `recenter`; SDA also recenters its renderer at the current accepted pose.

## Required handshake

The first accepted stdout message must be:

```json
{"type":"hello","protocol":1,"session":"<token>","source":"windows-airpods-experimental","coordinateSystem":"sda-adm-right-forward-up","orientation":"head-to-world-quaternion"}
```

`protocol` must be `1`. The coordinate system is SDA ADM: `+x` right, `+y` forward, `+z` up. `orientation` is a normalized **head-to-world** quaternion. The helper, not SDA's renderer, converts device axes, handedness, and quaternion direction to this convention.

## Pose messages

```json
{"type":"pose","protocol":1,"session":"<token>","seq":1842,"timestampMs":1735689600123,"orientation":{"x":0,"y":0.130526,"z":0,"w":0.991445}}
```

- `seq` is a strictly increasing safe integer for this launch.
- `timestampMs` is helper wall-clock epoch milliseconds and must be within one minute of SDA's wall clock.
- Quaternion values must be finite and have norm within 1% of one. SDA normalizes accepted values.
- Maximum accepted rate is 120 Hz; recommended rate is 20–100 Hz.
- SDA timestamps a pose again when it crosses into the renderer's monotonic clock domain.

## Status and diagnostics

```json
{"type":"status","protocol":1,"session":"<token>","state":"connected","detail":"motion stream active"}
{"type":"status","protocol":1,"session":"<token>","state":"disconnected","detail":"headset disconnected"}
{"type":"error","protocol":1,"session":"<token>","code":"calibration-required","message":"recenter required"}
```

Permitted `status.state` values are `connected`, `disconnected`, and `unavailable`. `detail`/`message` must be plain user-safe text only: no control characters, secrets, device identifiers, raw packets, or diagnostics longer than 240 characters. Status/error messages describe provider availability but do not stop the helper; on helper exit, malformed output, invalid handshake, invalid known message, or invalid token, SDA terminates the helper, disables its saved auto-start preference, and the renderer smoothly returns to fixed-head binaural rendering after its stale-pose timeout.

## Distribution boundary

The Windows build may bundle the helper as a separate GPL-3.0-or-later executable with its license. SDA and the helper remain separate processes communicating only through this device-neutral JSONL interface. The assisted Windows installer may also distribute the test-signed profile driver, but both enabling TestSigning and installing that driver are independent, unchecked-by-default choices requiring administrator consent. See `windows-head-tracking-install.md`. AirPods model/firmware support and the one-client motion-stream limitation remain transport concerns.
