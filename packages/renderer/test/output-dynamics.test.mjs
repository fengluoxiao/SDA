import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const source = readFileSync(join(import.meta.dirname, "../worklet/sda-renderer.worklet.js"), "utf8");
const processors = new Map();
globalThis.sampleRate = 48_000;
globalThis.AudioWorkletProcessor = class {
  constructor() {
    this.port = { onmessage: null, postMessage() {} };
  }
};
globalThis.registerProcessor = (name, processor) => processors.set(name, processor);
eval(source);
const PeakGuard = processors.get("sda-final-peak-guard");
assert.ok(PeakGuard, "final peak guard must register");

function render(guard, left, right) {
  const length = left.length + guard.lookahead;
  const result = [new Float32Array(length), new Float32Array(length)];
  for (let offset = 0; offset < length; offset += 128) {
    const count = Math.min(128, length - offset);
    const input = [left, right].map((channel) => {
      const block = new Float32Array(count);
      block.set(channel.subarray(offset, offset + count));
      return block;
    });
    const output = input.map(() => new Float32Array(count));
    assert.equal(guard.process([input], [output]), true);
    output.forEach((channel, index) => result[index].set(channel, offset));
  }
  return result.map((channel) => channel.subarray(guard.lookahead));
}

test("output preserves quiet passages after loud passages below the ceiling", () => {
  for (const rate of [44_100, 48_000, 96_000]) {
    globalThis.sampleRate = rate;
    const guard = new PeakGuard();
    const tone = (phase) => Float32Array.from({ length: rate * 3 }, (_, index) => {
      const amplitude = index < rate ? 0.6 : index < rate * 2 ? 0.02 : 0.35;
      return amplitude * Math.sin(2 * Math.PI * 997 * index / rate + phase);
    });
    const left = tone(0);
    const right = tone(0.7);
    const output = render(guard, left, right);
    assert.deepEqual(output[0], left, `left dynamics at ${rate} Hz`);
    assert.deepEqual(output[1], right, `right dynamics at ${rate} Hz`);
  }
});

test("program attenuation remains opt-in and both ears keep their dynamics", () => {
  globalThis.sampleRate = 48_000;
  const left = Float32Array.from({ length: 4800 }, (_, index) => 0.3 * Math.sin(index * 0.13));
  const right = Float32Array.from(left, (sample) => sample * 0.5);
  for (const enabled of [false, true]) {
    const guard = new PeakGuard();
    guard.port.onmessage({ data: { type: "programGain", gain: 0.5 } });
    guard.port.onmessage({ data: { type: "programEnabled", enabled } });
    const output = render(guard, left, right);
    const gain = enabled ? 0.5 : 1;
    assert.deepEqual(output[0], Float32Array.from(left, (sample) => sample * gain));
    assert.deepEqual(output[1], Float32Array.from(right, (sample) => sample * gain));
  }
});
