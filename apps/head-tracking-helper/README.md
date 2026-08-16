# SDA AirPods Head Tracking for Windows

This standalone helper is a Windows port of the LibrePods AirPods head-tracking
transport. It contains the AACP handshake, motion stream request, calibration,
orientation mapping, and SDA's versioned JSON Lines protocol.

## Windows transport status

The port compiles and discovers paired AirPods, but the stock Microsoft Bluetooth
stack does not expose arbitrary Classic Bluetooth L2CAP channels to normal desktop
applications. Microsoft's Winsock documentation lists `BTHPROTO_RFCOMM` as the
supported Bluetooth socket protocol. Although Windows registers an internal
`MSAFD L2CAP [Bluetooth]` provider, attempts to connect to AirPods PSM `0x1001`
or its AACP service UUID return `WSAENETDOWN` (`10050`) on tested Windows 11
hardware, including while A2DP/HFP audio is connected.

Consequently this user-mode Winsock transport is a research prototype, not a
working stock-Windows AirPods motion provider. A deployable native implementation
requires a signed Windows Bluetooth profile driver (or another transport that
legitimately exposes L2CAP). The JSONL integration and device-independent pose
pipeline can remain unchanged when such a transport is available.

References:

- https://learn.microsoft.com/windows/win32/bluetooth/bluetooth-and-socket
- https://learn.microsoft.com/windows-hardware/drivers/bluetooth/bluetooth-profile-drivers-overview

The helper has no UI and is intended to be launched by SDA for transport testing. Pair and connect
the AirPods in Windows Settings first. When multiple pairs are remembered, a
connected pair is preferred. Set `SDA_AIRPODS_ADDRESS=AA:BB:CC:DD:EE:FF` before
launching SDA to select a specific pair whose display name no longer contains
"AirPods".

Build and test from the repository root:

```powershell
pnpm head-tracking:build
cargo test --manifest-path apps/head-tracking-helper/Cargo.toml --offline
```

The implementation is an independent GPL-3.0-or-later executable. SDA talks to
it only through the documented stdin/stdout protocol in
`docs/head-tracking-helper-jsonl-protocol.md`.

## Attribution

AirPods AACP packet formats and the measured orientation mapping are based on
LibrePods, Copyright (C) 2025 LibrePods contributors:
https://github.com/librepods-org/librepods

LibrePods and this helper are distributed under GPL-3.0-or-later. SDA itself is
not a derivative library of this helper; the two executables communicate over
a narrow, device-neutral JSON Lines interface.
