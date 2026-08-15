import assert from "node:assert/strict";
import { SpatialRenderer } from "../src/renderer.ts";
import { LAYOUTS } from "../src/layouts.ts";

function node(kind, created) {
  const value = {
    kind,
    connections: [],
    connect(target, output, input) {
      this.connections.push({ target, output, input });
      return target;
    },
    disconnect() {},
  };
  created.push(value);
  return value;
}

const created = [];
const ctx = {
  createChannelMerger: (channels) => ({ ...node(`merger:${channels}`, created), channels }),
  createChannelSplitter: (channels) => ({ ...node(`splitter:${channels}`, created), channels }),
  createGain: () => ({ ...node("gain", created), gain: { value: 0 } }),
  createBiquadFilter: () => ({ ...node("biquad", created), type: "", frequency: { value: 0 }, Q: { value: 0 } }),
};
const graph = {
  ctx,
  node: node("worklet", created),
  multichannelOutput: node("multichannel-output", created),
  topology: LAYOUTS["2.1"],
  postNodes: [],
  layoutId: SpatialRenderer.prototype.layoutId,
  lr4: SpatialRenderer.prototype.lr4,
};

const projector = SpatialRenderer.prototype.createBassManaged21Projector.call(graph, LAYOUTS["2.1"], 1);
assert.ok(projector);
assert.equal(projector.id, "FrontLeft,FrontRight,LFE");
assert.equal(created.filter((entry) => entry.kind === "merger:3").length, 1, "creates a physical FL/FR/sub merger");
assert.equal(created.filter((entry) => entry.kind === "splitter:3").length, 4, "keeps one branch per persistent worklet output bank");

for (const splitter of created.filter((entry) => entry.kind === "splitter:3")) {
  const buses = splitter.connections.map((connection) => connection.output).sort();
  assert.deepEqual(buses, [0, 0, 1, 1, 2], "each bank routes FL/FR through both crossover branches and LFE only to its dedicated branch");
}
assert.deepEqual(
  created.flatMap((entry) => entry.connections
    .filter((connection) => connection.target.kind === "merger:3")
    .map((connection) => connection.input)).sort(),
  [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2],
  "each persistent bank keeps mains discrete and sends its summed bass/LFE feed only to sub channel 2",
);

console.log("bass-managed 2.1 projector tests passed");
