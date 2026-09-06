import assert from "node:assert/strict";
import { FrameBatcher } from "../src/frame-batcher.ts";

const frame = (samplePos, overrides = {}) => ({
  codec: "truehd", sampleRate: 48000, samplePos,
  channels: [Float32Array.from({ length: 40 }, (_, i) => samplePos + i)],
  labels: ["Obj_10"], rawBedLabels: [], objectChannels: [],
  events: [{ id: 10, samplePos: samplePos + 7, gainDb: -3 }],
  programLoudness: null, rampDuration: 0, ...overrides,
});
const output = [];
const batcher = new FrameBatcher(f => output.push(f));
for (let i = 0; i < 24; i++) batcher.push(frame(i * 40, i ? {} : { objectChannels: [{ id: 10, channel: 0 }] }));
assert.equal(output.length, 1);
assert.deepEqual([...output[0].channels[0]], Array.from({ length: 960 }, (_, i) => i));
assert.deepEqual(output[0].events.map(e => e.samplePos), Array.from({ length: 24 }, (_, i) => i * 40 + 7));
assert.deepEqual(output[0].objectChannels, [{ id: 10, channel: 0 }]);
batcher.push(frame(960));
batcher.push(frame(1040)); // A missing AU must remain a clock gap.
assert.equal(output[1].samplePos, 960);
batcher.push(frame(1080, { objectChannels: [{ id: 11, channel: 0 }], labels: ["Obj_11"] }));
assert.equal(output[2].samplePos, 1040);
batcher.flush();
assert.equal(output[3].samplePos, 1080);
assert.equal(output[3].channels[0].length, 40);
batcher.flush();
assert.equal(output.length, 4);
batcher.push(frame(1120));
batcher.push(frame(1160, { programLoudness: { gainDb: -6 } }));
assert.equal(output[4].samplePos, 1120);
batcher.flush();
assert.equal(output[5].programLoudness.gainDb, -6);
console.log("frame batching preserves PCM, timestamps, declarations, gaps and gain boundaries");
