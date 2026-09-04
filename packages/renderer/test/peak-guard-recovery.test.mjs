import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dirname, "../worklet/sda-renderer.worklet.js"), "utf8");
let PeakGuard = null;
globalThis.sampleRate = 48_000;
globalThis.AudioWorkletProcessor = class {
  constructor() {
    this.port = { onmessage: null, postMessage() {} };
  }
};
globalThis.registerProcessor = (name, processor) => {
  if (name === "sda-final-peak-guard") PeakGuard = processor;
};

// The source declares both processors; only the final guard registration is
// captured by the stub above.
eval(source);
assert.ok(PeakGuard, "final peak guard must register");

const guard = new PeakGuard();
const render = (left, right) => {
  const output = [new Float32Array(left.length), new Float32Array(left.length)];
  assert.equal(guard.process([[left, right]], [output]), true);
  return output;
};

// A sparse-object peak should stay ceiling-safe and recover within one 32 ms
// object-update interval rather than audibly attenuating later updates.
const burst = new Float32Array(240).fill(2);
render(burst, new Float32Array(240).fill(1));
const recovered = render(new Float32Array(1_536), new Float32Array(1_536));
assert.ok(recovered[0].every((sample) => Math.abs(sample) <= 1));
assert.ok(guard.gain > 0.84, `expected short peak-guard recovery, got ${guard.gain}`);

console.log("peak guard short-recovery tests passed");
