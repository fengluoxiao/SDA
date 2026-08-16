import assert from "node:assert/strict";
import { SpatialRenderer } from "../src/renderer.ts";
import { LAYOUTS, RENDER_TOPOLOGY, speakerBusKey } from "../src/layouts.ts";

function convolutionMap(layout) {
  return new Map(layout.map((speaker, index) => [index, speaker.isLfe ? null : {}]));
}

// 9.1 layouts contain one direct LFE path and one measured spatial convolution
// for every other speaker in an active measured bank.
for (const [layoutId, expectedConvolutions] of [["9.1.2", 11], ["9.1.4", 13], ["9.1.6", 15]]) {
  const renderer = Object.create(SpatialRenderer.prototype);
  renderer.convs = new Map([["near", convolutionMap(LAYOUTS[layoutId])]]);
  renderer.topology = LAYOUTS[layoutId];
  const health = renderer.binauralHealth;
  assert.equal(health.activeBankCount, 1, `${layoutId}: active bank count`);
  assert.equal(health.totalSpatialConvolutions, expectedConvolutions, `${layoutId}: measured convolution count`);
  assert.equal(health.totalDirectPaths, 0, `${layoutId}: LFE is not an off-bank direct path`);
  assert.deepEqual(health.banks, [{ bank: "near", spatialConvolutions: expectedConvolutions, directPaths: 0 }]);
}

// 2.1 retains only two spatial main paths. Its LFE is a dedicated direct
// low-frequency path, never a third spatial convolution.
{
  const renderer = Object.create(SpatialRenderer.prototype);
  renderer.convs = new Map([["near", convolutionMap(LAYOUTS["2.1"])]]);
  renderer.topology = LAYOUTS["2.1"];
  const health = renderer.binauralHealth;
  assert.equal(health.totalSpatialConvolutions, 2);
  assert.equal(health.totalDirectPaths, 0);
  assert.deepEqual(health.banks, [{ bank: "near", spatialConvolutions: 2, directPaths: 0 }]);
}

// The off bank has direct stereo paths for non-LFE buses, but none of these are
// presented as spatial convolutions.
{
  const renderer = Object.create(SpatialRenderer.prototype);
  renderer.convs = new Map([["off", new Map(LAYOUTS["9.1.2"].map((_, index) => [index, null]))]]);
  renderer.topology = LAYOUTS["9.1.2"];
  const health = renderer.binauralHealth;
  assert.equal(health.totalSpatialConvolutions, 0);
  assert.equal(health.totalDirectPaths, 11);
  assert.deepEqual(health.banks, [{ bank: "off", spatialConvolutions: 0, directPaths: 11 }]);
}

function lifecycleGraph(keys) {
  const operations = [];
  const topology = keys.map((key) => ({ name: key }));
  return {
    binauralMerger: {},
    topology,
    binauralBusKeySequence: "",
    binauralBankSplitters: new Map([["near", { id: "old-splitter" }]]),
    binauralBusNodes: [{ id: "old-node", disconnect: () => operations.push("disconnect-node") }],
    postNodes: [],
    convs: new Map([["near", new Map()]]),
    binauralMode: "near",
    sources: new Map(),
    node: { disconnect: (node) => operations.push(`disconnect-${node.id}`) },
    activeBinauralBuses: () => keys.map((_, topologyBus) => ({ topologyBus })),
    currentBinauralBusKeySequence: SpatialRenderer.prototype.currentBinauralBusKeySequence,
    buildBinauralBank: (bank) => operations.push(`build-${bank}`),
    operations,
  };
}

// A replaceable graph is rebuilt when a mode changes logical bus identities,
// but does not recreate the worklet or the final binaural merger.
{
  const graph = lifecycleGraph(["FrontLeft", "LFE", "TopMiddleLeft"]);
  graph.binauralBusKeySequence = "FrontLeft,LFE";
  const node = graph.node;
  const merger = graph.binauralMerger;
  SpatialRenderer.prototype.rebuildBinauralBusGraph.call(graph);
  assert.deepEqual(graph.operations, ["disconnect-old-splitter", "disconnect-node", "build-near"]);
  assert.equal(graph.node, node);
  assert.equal(graph.binauralMerger, merger);
  assert.equal(graph.binauralBusKeySequence, "FrontLeft,LFE,TopMiddleLeft");
}

// Changes across every approved 9.1 height layout replace only the bus graph;
// reapplying the same bus-key sequence remains a no-op.
{
  let previous = LAYOUTS["9.1.2"].map(speakerBusKey).join(",");
  for (const layoutId of ["9.1.4", "9.1.6"]) {
    const graph = lifecycleGraph(LAYOUTS[layoutId].map(speakerBusKey));
    graph.binauralBusKeySequence = previous;
    SpatialRenderer.prototype.rebuildBinauralBusGraph.call(graph);
    assert.deepEqual(graph.operations, ["disconnect-old-splitter", "disconnect-node", "build-near"], `${layoutId}: replaces bus graph`);
    previous = graph.binauralBusKeySequence;
  }
  const graph = lifecycleGraph(LAYOUTS["9.1.6"].map(speakerBusKey));
  graph.binauralBusKeySequence = previous;
  SpatialRenderer.prototype.rebuildBinauralBusGraph.call(graph);
  assert.deepEqual(graph.operations, [], "same 9.1.6 bus sequence: no rebuild");
}

// setOutputMode must run the same replacement decision after recomputing a
// logical render layout, even though its output crossfade remains independent.
{
  const graph = {
    mode: "stereo",
    modeGains: new Map(),
    updateRenderLayout: () => { graph.updated = true; },
    rebuildBinauralBusGraph: () => { graph.rebuilt = true; },
    headPose: { isActive: () => false },
    syncPoseControl: () => {},
    sources: new Map(),
    applyGains: () => { graph.gainsApplied = true; },
  };
  SpatialRenderer.prototype.setOutputMode.call(graph, "binaural");
  assert.equal(graph.mode, "binaural");
  assert.equal(graph.updated, true);
  assert.equal(graph.rebuilt, true);
}

assert.equal(speakerBusKey(RENDER_TOPOLOGY[0]), "FrontLeft");
console.log("binaural lifecycle and health tests: OK");
