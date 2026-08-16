# SDA AirPods L2CAP profile driver

This experimental KMDF Bluetooth profile driver binds only to known motion-capable
AirPods AACP service nodes. Its INF requires Apple's Bluetooth vendor ID and one
of the explicit LibrePods-derived product IDs for AirPods Pro, Pro 2, AirPods 3,
AirPods 4, or AirPods Max variants; matching the AACP UUID alone is not enough.
Windows BthEnum supplies the paired remote address to the driver. The driver
opens Classic Bluetooth L2CAP PSM `0x1001` and exposes the channel as ordinary
file reads and writes through device interface
`8d09ce09-58c6-4f67-95a7-9824b4d54cb3`.

The current hardware allowlist is:

- AirPods Pro (`PID&200E`)
- AirPods Pro 2, Lightning and USB-C (`PID&2014`, `PID&2024`)
- AirPods 3 (`PID&2013`)
- AirPods 4, standard and ANC (`PID&2019`, `PID&201B`)
- AirPods Max, Lightning and USB-C (`PID&200A`, `PID&201F`)

AirPods 1 and 2 do not expose supported motion data and are intentionally
excluded. Models without a confirmed LibrePods proximity model/PID mapping are
also excluded rather than guessed.

The implementation is derived from Microsoft's Bluetooth Echo L2CAP Profile
Driver sample at commit `717778a20ba4dd2440fe609f69153a1f8a64f597` and retains
the original Microsoft copyright notices. See `LICENSE.microsoft.txt`.

This is test-only software. Installing it requires an administrator, a trusted
test certificate, and Windows test-signing mode when Secure Boot policy does not
accept the certificate. It does not replace the Bluetooth radio, A2DP, HFP, or
AVRCP drivers; its INF matches only the AACP service node.
