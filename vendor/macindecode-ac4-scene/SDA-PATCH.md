# SDA Scene Adapter Patch

Upstream: https://github.com/SakuzyPeng/MacinDecode-AC4-Core
Revision: a23965312a53d3cfb1a63cbae692a788efe46123
License: MIT, original LICENSE retained.

Changes: standalone Cargo manifest and an opt-in opaque presentation tail
policy. The upstream strict defaults remain unchanged. SDA enables the policy
for playback. It only applies when exactly eight trailing bits remain after
strict parsing of an independent object/A-JOC presentation with a valid DRC
configuration. The complete payload is retained, and syntax length excludes
the opaque byte. No meaning is assigned to the tail; audio substreams, framing
and CRC validation are unchanged. This extends upstream's equivalent policy
which accepts only 0x00 and 0x80. A local real-world stream contains 0x8c.
