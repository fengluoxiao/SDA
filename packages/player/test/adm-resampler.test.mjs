import assert from "node:assert/strict";
import { AlacResampler } from "../src/alac-resampler.ts";

const rate = 96000;
const length = 4001;
const changes = [
  { samplePos: 0, rampDuration: 0, pos: [-1, 1, 0] },
  { samplePos: 333, rampDuration: 2001, pos: [1, 0, 1] },
  { samplePos: 3960, rampDuration: 0, pos: [0, -1, 0] },
].map(change => ({ id: 7, hasPos: true, gainDb: -6, size: [0, 0, 0], anchor: "room", distanceM: null, distanceInfinite: false, screenFactor: null, depthFactor: null, ...change }));

async function convert(chunkSize) {
  const converter = new AlacResampler(48000);
  const output = [];
  for (let at = 0; at < length; at += chunkSize) {
    const samples = Math.min(chunkSize, length - at);
    const frame = await converter.push({
      codec: "adm", sampleRate: rate, samplePos: at,
      channels: [Float32Array.from({ length: samples }, (_, i) => .25 * Math.sin(2 * Math.PI * 997 * (at + i) / rate))],
      labels: ["Obj_7"], rawBedLabels: [], objectChannels: [{ id: 7, channel: 0 }],
      events: changes.filter(event => event.samplePos >= at && event.samplePos < at + samples),
      rampDuration: 2001, programLoudness: null,
    });
    if (frame) output.push(frame);
  }
  const tail = converter.finish();
  if (tail) output.push(tail);
  let next = 0;
  for (const frame of output) {
    assert.equal(frame.samplePos, next);
    assert.equal(frame.sampleRate, 48000);
    assert.deepEqual(frame.objectChannels, [{ id: 7, channel: 0 }]);
    assert.equal(frame.rampDuration, 1001);
    next += frame.channels[0].length;
    for (const event of frame.events) assert.ok(event.samplePos >= frame.samplePos && event.samplePos < next, "event must accompany its PCM interval");
  }
  assert.equal(next, Math.round(length / 2));
  assert.deepEqual(output.flatMap(frame => frame.events), changes.map(event => ({
    ...event, samplePos: Math.round(event.samplePos / 2),
    rampDuration: Math.round((event.samplePos + event.rampDuration) / 2) - Math.round(event.samplePos / 2),
  })));
  return output.flatMap(frame => [...frame.channels[0]]);
}

const small = await convert(127);
const large = await convert(2048);
assert.ok(Math.max(...small.map((value, i) => Math.abs(value - large[i]))) < 1e-6);
console.log("ADM resampling: PCM continuity, exact duration, object clock, jumps, ramps and delayed tail passed");
