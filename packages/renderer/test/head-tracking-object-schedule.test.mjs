import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const source = readFileSync(new URL("../worklet/sda-renderer.worklet.js", import.meta.url), "utf8");
const processors = new Map();
class MockAudioWorkletProcessor {
  constructor() {
    this.port = { postMessage() {}, onmessage: null };
  }
}
vm.runInNewContext(source, {
  AudioWorkletProcessor: MockAudioWorkletProcessor,
  registerProcessor: (name, processor) => processors.set(name, processor),
  sampleRate: 48000,
  performance: { now: () => 1 },
  Date,
  Float32Array,
  Uint8Array,
  Map,
  Math,
  Number,
});

const RendererProcessor = processors.get("sda-renderer");
assert.ok(RendererProcessor, "renderer worklet registered");
const processor = new RendererProcessor({ processorOptions: { busCount: 2 } });
processor.onMessage({ type: "add", id: "obj:19" });
const object = processor.sources.get("obj:19");

processor.onMessage({
  type: "gains",
  id: "obj:19",
  gains: new Float32Array([0, 1]),
  gain: 1,
  ramp: 1,
});
processor.advanceGainRamps(object, 1);
assert.deepEqual([...object.gains], [0, 1], "initial head-relative route applied");

processor.onMessage({ type: "headTracking", enabled: true });
processor.onMessage({
  type: "scheduleGains",
  id: "obj:19",
  at: 10,
  gains: new Float32Array([1, 0]),
  gain: 0.25,
  ramp: 4,
  poseControlled: true,
});
processor.applyScheduledGainsThrough(object, 10);
processor.advanceGainRamps(object, 4);
assert.deepEqual([...object.gains], [0, 1], "scheduled world route cannot reset live head-relative route");
assert.equal(object.gain, 0.25, "scheduled metadata scalar remains sample accurate");

processor.onMessage({
  type: "gains",
  id: "obj:19",
  gains: new Float32Array([0.5, 0.5]),
  gain: 0.9,
  ramp: 512,
  poseControlled: true,
  poseUpdate: true,
});
assert.equal(object.gain, 0.25, "pose refresh does not overwrite metadata scalar");
assert.equal(object.rampLeft, 512);
assert.deepEqual([...object.target], [0.5, 0.5], "pose refresh still updates the spatial target");
processor.onMessage({
  type: "scheduleGains",
  id: "obj:19",
  at: 20,
  gains: new Float32Array([1, 0]),
  gain: 0.5,
  ramp: 4,
  poseControlled: true,
});
processor.applyScheduledGainsThrough(object, 1000);
assert.equal(object.rampLeft, 512, "overdue scalar event cannot fast-forward a live pose ramp");

// Retargeting a rapid turn must continue from the gain reached at that exact
// sample, rather than jumping to either the old or new HRTF direction.
processor.advanceGainRamps(object, 256);
const beforeRetarget = [...object.gains];
processor.onMessage({
  type: "gains",
  id: "obj:19",
  gains: new Float32Array([0.1, 0.9]),
  gain: 0.9,
  ramp: 1152,
  poseControlled: true,
  poseUpdate: true,
});
assert.deepEqual([...object.gains], beforeRetarget, "pose retarget is continuous at the message boundary");
processor.advanceGainRamps(object, 400);
assert.ok(object.gains[0] < beforeRetarget[0] && object.gains[0] > 0.1, "left gain moves continuously toward the new direction");
assert.ok(object.gains[1] > beforeRetarget[1] && object.gains[1] < 0.9, "right gain moves continuously toward the new direction");

processor.onMessage({ type: "headTracking", enabled: false });
processor.onMessage({
  type: "scheduleGains",
  id: "obj:19",
  at: 1100,
  gains: new Float32Array([1, 0]),
  gain: 1,
  ramp: 1,
  poseControlled: true,
});
processor.applyScheduledGainsThrough(object, 1100);
processor.advanceGainRamps(object, 1);
assert.deepEqual([...object.gains], [1, 0], "canonical object routes resume when tracking stops");

// A future head-relative route is queued beside metadata before PCM playback.
// It must still take effect at the exact sample if the renderer/main thread
// sends no further pose messages for the next second.
const stalledProcessor = new RendererProcessor({ processorOptions: { busCount: 2 } });
stalledProcessor.onMessage({ type: "add", id: "obj:24" });
const stalledObject = stalledProcessor.sources.get("obj:24");
stalledProcessor.onMessage({ type: "headTracking", enabled: true });
stalledProcessor.onMessage({
  type: "scheduleGains",
  id: "obj:24",
  at: 48000,
  gains: new Float32Array([1, 0]),
  gain: 0.5,
  ramp: 4,
  poseControlled: true,
});
stalledProcessor.onMessage({
  type: "scheduleGains",
  id: "obj:24",
  at: 48000,
  gains: new Float32Array([0.25, 0.75]),
  gain: 0.9,
  ramp: 4,
  poseControlled: true,
  poseUpdate: true,
});
stalledProcessor.onMessage({
  type: "scheduleGains",
  id: "obj:24",
  at: 48000,
  gains: new Float32Array([0.4, 0.6]),
  gain: 0.9,
  ramp: 4,
  poseControlled: true,
  poseUpdate: true,
});
assert.equal(stalledObject.scheduledGains.length, 2, "new pose replaces the pending route at the same sample");
stalledProcessor.applyScheduledGainsThrough(stalledObject, 48000);
stalledProcessor.advanceGainRamps(stalledObject, 4);
assert.deepEqual([...stalledObject.gains], [...new Float32Array([0.4, 0.6])], "prebuffered pose route survives a main-thread stall");
assert.equal(stalledObject.gain, 0.5, "paired pose route does not replace sample-accurate metadata gain");

stalledProcessor.onMessage({ type: "headTracking", enabled: false });
assert.equal(stalledObject.scheduledGains.length, 0, "disabling tracking discards stale future pose routes");

console.log("head-tracking object schedule tests passed");
