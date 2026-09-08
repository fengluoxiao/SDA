import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import vm from "node:vm";
import { SpatialRenderer } from "../src/renderer.ts";

function event(samplePos, pos, gainDb, rampDuration = 0) {
  return {
    id: 7, samplePos, hasPos: true, pos, gainDb, rampDuration,
    size: [0, 0, 0], anchor: "room", distanceM: null,
    distanceInfinite: false, screenFactor: null, depthFactor: null,
  };
}

function pan(position) {
  return new Float32Array([(90 + position.azimuth) / 180, (90 - position.azimuth) / 180]);
}

function fixture(batched, tracking) {
  const messages = [];
  const state = {
    id: "obj:7", position: { azimuth: 0, elevation: 0, distance: 1 },
    spread: 0, gainDb: 0, hasObjectMetadata: false,
    objectRampEndSample: -Infinity, objectPoseTimeline: [],
    isLfe: false, muted: false, snapBus: -1,
  };
  const renderer = {
    mode: "binaural",
    node: { port: { postMessage: message => messages.push(message) } },
    sources: new Map([[state.id, state]]),
    headPose: { isActive: () => tracking, headRelative: position => position },
    vbap: { pan },
    topology: [{}, {}], renderLayout: [{}, {}], renderToTopology: new Int16Array([0, 1]),
    gainMessage: SpatialRenderer.prototype.gainMessage,
    panObjectBatch: batched ? states => states.map(target => pan(target.position)) : undefined,
  };
  return { renderer, state, messages };
}

let scheduled;
for (const batched of [false, true]) {
  for (const tracking of [false, true]) {
    const { renderer, state, messages } = fixture(batched, tracking);
    assert.equal(SpatialRenderer.prototype.applyEvents.call(renderer, [
      event(10, [-1, 0, 0], -6),
      event(20, [1, 0, 0], -12),
    ]), 2);
    const entries = messages[0].entries;
    const canonical = entries.filter(message => !message.poseUpdate);
    assert.deepEqual(canonical.map(message => [...message.gains]), [[1, 0], [0, 1]], "each timestamp retains its own panning target");
    assert.deepEqual(canonical.map(message => message.gain), [10 ** (-6 / 20), 10 ** (-12 / 20)], "each timestamp retains its own gain");
    assert.deepEqual(canonical.map(message => message.ramp), [0, 0], "explicit ADM jumps remain instantaneous");
    assert.deepEqual(canonical.map(message => message.at), [10, 20]);
    assert.equal(state.objectRampEndSample, 20);
    assert.equal(state.objectPoseTimeline[1].fromPosition.azimuth, 90, "a jump establishes the next event's interpolation origin");
    assert.equal(state.objectPoseTimeline[1].rampSamples, 0);
    if (tracking) {
      assert.deepEqual(entries.filter(message => message.poseUpdate).map(message => [...message.gains]), [[1, 0], [0, 1]], "paired head routes retain both targets");
    } else scheduled = messages[0];
  }
}

const legacy = fixture(false, false);
for (const tracking of [false, true]) {
  const {renderer, state, messages} = fixture(true, tracking);
  renderer.renderLayout = [{azimuth:90,elevation:0},{azimuth:-90,elevation:0}];
  const zone = {type:'cartesian',min:[-1,-1,-1],max:[-0.1,1,1]};
  assert.equal(SpatialRenderer.prototype.applyEvents.call(renderer,[
    {...event(10,[-1,0,0],0),zoneExclusion:[zone]}, {...event(20,[-1,0,0],0),zoneExclusion:[]}
  ]),2,'zone-only changes must not coalesce');
  assert.deepEqual(messages[0].entries.filter(entry=>!entry.poseUpdate).map(entry=>[...entry.gains]),[[0,1],[1,0]]);
  assert.deepEqual(state.objectPoseTimeline.map(entry=>entry.zoneExclusion),[[zone],[]]);
}
const unspecified = event(10, [-1, 0, 0], 0);
delete unspecified.rampDuration;
SpatialRenderer.prototype.applyEvents.call(legacy.renderer, [unspecified]);
assert.equal(legacy.messages[0].ramp, 128, "legacy events without a duration retain the default ramp");

const processors = new Map();
class MockAudioWorkletProcessor {
  constructor() { this.port = { postMessage() {}, onmessage: null }; }
}
vm.runInNewContext(readFileSync(resolve("packages/renderer/worklet/sda-renderer.worklet.js"), "utf8"), {
  AudioWorkletProcessor: MockAudioWorkletProcessor,
  registerProcessor: (name, processor) => processors.set(name, processor),
  sampleRate: 48000, performance: { now: () => 1 }, Date, Float32Array, Uint8Array, Map, Math, Number,
});
const processor = new (processors.get("sda-renderer"))({ processorOptions: { busCount: 2 } });
processor.onMessage({ type: "add", id: "obj:7" });
const source = processor.sources.get("obj:7");
processor.onMessage(scheduled);
processor.applyScheduledGainsThrough(source, 9);
assert.deepEqual([...source.gains], [0, 0], "future jumps cannot change earlier samples");
processor.applyScheduledGainsThrough(source, 10);
assert.deepEqual([...source.gains], [1, 0], "the new route is active at the exact jump sample");
assert.equal(source.gain, 10 ** (-6 / 20));
assert.equal(source.rampLeft, 0);
assert.equal(source.gainRampLeft, 0);
processor.applyScheduledGainsThrough(source, 20);
assert.deepEqual([...source.gains], [0, 1]);
assert.equal(source.gain, 10 ** (-12 / 20));
assert.ok([...source.rampStep, source.gainStep].every(Number.isFinite), "instantaneous jumps never divide by zero");

processor.onMessage({ type: "scheduleGains", id: "obj:7", at: 30, gains: new Float32Array([1, 0]), gain: 1, ramp: 4 });
processor.applyScheduledGainsThrough(source, 32);
assert.deepEqual([...source.gains], [0.5, 0.5], "authored nonzero ramps retain their intermediate route");

console.log("object event batch and ADM jump tests passed");
