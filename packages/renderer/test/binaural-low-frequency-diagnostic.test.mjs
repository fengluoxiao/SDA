import assert from "node:assert/strict";
import { fileURLToPath, pathToFileURL } from "node:url";
import { join, dirname } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "../../..");
const { SpatialRenderer } = await import(pathToFileURL(join(root, "tmp/renderer.bundle.cjs")).href);

function audioParam(value = 0) {
  const calls = [];
  return {
    value,
    calls,
    cancelScheduledValues(time) { calls.push(["cancel", time]); },
    setValueAtTime(next, time) { this.value = next; calls.push(["set", next, time]); },
    linearRampToValueAtTime(next, time) { this.value = next; calls.push(["ramp", next, time]); },
  };
}

const left = { gain: audioParam() };
const right = { gain: audioParam() };
const renderer = Object.create(SpatialRenderer.prototype);
renderer.ctx = { currentTime: 4 };
renderer.binauralLowFrequencyDiagnosticMode = "reference";
renderer.binauralLowDiagnosticNodes = [left, right];

assert.equal(renderer.binauralLowFrequencyDiagnostic, "reference", "reference is the default literal-bypass state");
SpatialRenderer.prototype.setBinauralLowFrequencyDiagnostic.call(renderer, "reference");
assert.equal(left.gain.calls.length, 0, "reapplying reference does not automate the final graph");

SpatialRenderer.prototype.setBinauralLowFrequencyDiagnostic.call(renderer, "low-cut");
assert.equal(renderer.binauralLowFrequencyDiagnostic, "low-cut");
for (const node of [left, right]) {
  assert.deepEqual(node.gain.calls, [["cancel", 4], ["set", 0, 4], ["ramp", -3, 4.04]], "each ear gets the same -3dB diagnostic shelf ramp");
}

SpatialRenderer.prototype.setBinauralLowFrequencyDiagnostic.call(renderer, "reference");
for (const node of [left, right]) {
  assert.deepEqual(node.gain.calls.slice(-3), [["cancel", 4], ["set", -3, 4], ["ramp", 0, 4.04]], "reference restores the shelf to 0dB without rebuilding the graph");
}

console.log("binaural low-frequency diagnostic tests passed");
