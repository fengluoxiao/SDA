import assert from "node:assert/strict";
import { mp4PacketTimestampMs } from "../src/mp4.ts";

assert.equal(mp4PacketTimestampMs(48_000, undefined, 48_000), 1000);
assert.equal(mp4PacketTimestampMs(90_000, 90_000, 48_000), 1000);
assert.equal(mp4PacketTimestampMs(1, undefined, 0), 0);

console.log("MP4 packet timestamps retain the track clock when samples omit timescale");
