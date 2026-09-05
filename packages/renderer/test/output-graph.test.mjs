import assert from "node:assert/strict";
import { SpatialRenderer } from "../src/renderer.ts";
import { LAYOUTS } from "../src/layouts.ts";

const param = () => ({
  value: 0,
  setValueAtTime(value) { this.value = value; },
  linearRampToValueAtTime(value) { this.value = value; },
  cancelScheduledValues() {},
});
function node(tag) {
  return {
    tag,
    connections: new Set(),
    connect(target) { this.connections.add(target); return target; },
    disconnect(target) { if (target) this.connections.delete(target); else this.connections.clear(); },
    gain: param(), frequency: param(), Q: param(),
    threshold: param(), knee: param(), ratio: param(), attack: param(), release: param(),
    positionX: param(), positionY: param(), positionZ: param(),
    maxChannelCount: 16,
  };
}

const worklets = [];
globalThis.AudioWorkletNode = class {
  constructor(_context, name) {
    Object.assign(this, node(name));
    this.messages = [];
    this.port = { postMessage: (message) => this.messages.push(message), onmessage: null };
    worklets.push(this);
  }
};
const context = {
  sampleRate: 48000, currentTime: 1, state: "running",
  destination: node("destination"),
  audioWorklet: { addModule: async () => {} },
  createGain: () => node("gain"),
  createDelay: () => Object.assign(node("delay"), { delayTime: param() }),
  createBiquadFilter: () => node("biquad"),
  createConvolver: () => node("convolver"),
  createPanner: () => node("panner"),
  createDynamicsCompressor: () => node("compressor"),
  createChannelSplitter: () => node("splitter"),
  createChannelMerger: () => node("merger"),
  createBuffer: (_channels, length) => ({ length, copyToChannel() {} }),
  async close() { this.state = "closed"; },
};

const refreshWasmVbap = SpatialRenderer.prototype.refreshWasmVbap;
SpatialRenderer.prototype.refreshWasmVbap = () => {};
const renderer = new SpatialRenderer(context, { mode: "binaural", layout: LAYOUTS["7.1.4"] });
SpatialRenderer.prototype.refreshWasmVbap = refreshWasmVbap;
await renderer.init("mock://worklet");
assert.deepEqual(worklets.map((worklet) => worklet.tag), ["sda-renderer", "sda-final-peak-guard"]);
const guard = worklets[1];
function checkOutputGraph() {
  for (const mode of ["stereo", "binaural"]) {
    assert.deepEqual([...renderer.modeGains.get(mode).connections], [guard], `${mode} must reach final peak guard`);
  }
  assert.deepEqual([...guard.connections], [renderer.master], "final guard must feed master once");
  const [delay] = renderer.modeGains.get("multichannel").connections;
  assert.equal(delay.delayTime.value, 0.005);
  assert.deepEqual([...delay.connections], [renderer.master]);
}
checkOutputGraph();
const retiredOutputs = [...renderer.modeGains.values()];
renderer.setBinauralData({ sampleRate: 48000, positions: [] });
checkOutputGraph();
assert.ok(retiredOutputs.every((output) => output.connections.size === 0), "rebuild must disconnect former mode outputs");
assert.equal(worklets.length, 2, "HRTF rebuild must preserve the final guard");

renderer.setVolumeBalance(true);
renderer.setProgramLoudnessGainDb(-6, 48000);
assert.deepEqual(guard.messages.at(-2), { type: "programEnabled", enabled: true });
assert.deepEqual(guard.messages.at(-1), { type: "scheduleProgramGain", gain: 10 ** (-6 / 20), at: 48000 });
await renderer.close();
assert.equal(guard.connections.size, 0);
console.log("output graph tests passed");
