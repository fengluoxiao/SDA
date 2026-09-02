import assert from "node:assert/strict";
import { SpatialRenderer } from "../src/renderer.ts";
import {
  DENSE_BINAURAL_FILLS,
  LAYOUTS,
  RENDER_TOPOLOGY,
  speakerBusKey,
} from "../src/layouts.ts";
import { VbapSolver } from "../src/vbap.ts";

const layout714 = LAYOUTS["7.1.4"];
const topology = [...RENDER_TOPOLOGY, ...DENSE_BINAURAL_FILLS];

// --- topology invariants -----------------------------------------------------
assert.ok(DENSE_BINAURAL_FILLS.length > 0);
assert.ok(
  DENSE_BINAURAL_FILLS.every((s) => s.binauralOnly === true),
  "every dense fill is marked binauralOnly",
);
assert.equal(
  new Set(topology.map(speakerBusKey)).size,
  topology.length,
  "dense fills add unique worklet buses",
);
assert.ok(
  RENDER_TOPOLOGY.every((s) => !s.binauralOnly),
  "the physical topology itself stays free of binaural-only buses",
);

// --- dense binaural render layout -------------------------------------------
const denseLayout = SpatialRenderer.prototype.denseBinauralLayout.call({ layout: layout714 });
assert.deepEqual(
  denseLayout.slice(0, layout714.length).map((s) => s.name),
  layout714.map((s) => s.name),
  "bed speakers keep their exact leading buses",
);
const activeFills = denseLayout.slice(layout714.length);
assert.ok(activeFills.length > 0 && activeFills.length < DENSE_BINAURAL_FILLS.length);
assert.ok(
  activeFills.every((f) => f.binauralOnly),
  "only binauralOnly fills are appended",
);
for (const fill of activeFills) {
  for (const speaker of layout714) {
    if (speaker.isLfe) continue;
    const dAz = Math.abs(((speaker.azimuth - fill.azimuth + 540) % 360) - 180);
    assert.ok(
      !(dAz < 10 && Math.abs(speaker.elevation - fill.elevation) < 10),
      `fill ${fill.name} must not duplicate ${speaker.name}`,
    );
  }
}
// LFE sits at az45/el0 but has no direction: it must not evict the h40 fill.
assert.ok(
  activeFills.some((f) => f.azimuth === 40 && f.elevation === 0),
  "LFE position does not evict a horizontal fill",
);
// Bed directions (Centre 0/0, Surround ±100/0, Top ±45/45) are not duplicated.
assert.ok(!activeFills.some((f) => f.azimuth === 0 && f.elevation === 0));
assert.ok(!activeFills.some((f) => f.azimuth === 100 && f.elevation === 0));
assert.ok(!activeFills.some((f) => f.azimuth === 45 && f.elevation === 45));

// --- VBAP lands objects on their true direction ------------------------------
const angularDistance = (a, b) => {
  const dAz = Math.abs(((a.azimuth - b.azimuth + 540) % 360) - 180);
  // crude but sufficient for these assertions: azimuth ring distance dominates
  // here because all compared speakers sit at elevation 0 like the source.
  return dAz + Math.abs(a.elevation - b.elevation);
};
const usedBuses = (layout, gains, threshold = 0.02) =>
  layout
    .map((speaker, i) => ({ speaker, gain: gains[i] }))
    .filter((entry) => entry.gain > threshold && !entry.speaker.isLfe);

const denseSolver = new VbapSolver(denseLayout);
const ringSolver = new VbapSolver(layout714);

// Object at azimuth +70 (between FrontLeft@30 and SurroundLeft@100).
const pos = { azimuth: 70, elevation: 0, distance: 1 };
const denseUsed = usedBuses(denseLayout, denseSolver.pan(pos, 0));
const ringUsed = usedBuses(layout714, ringSolver.pan(pos, 0));
assert.ok(
  denseUsed.every((entry) => angularDistance(entry.speaker, { azimuth: 70, elevation: 0 }) <= 25),
  `dense grid pans az70 only onto nearby fills, got ${denseUsed.map((e) => `${e.speaker.name}=${e.gain.toFixed(2)}`).join(" ")}`,
);
const denseTop = denseUsed.reduce((a, b) => (b.gain > a.gain ? b : a));
assert.ok(
  Math.abs(denseTop.speaker.azimuth - 70) <= 10 && denseTop.speaker.binauralOnly,
  `dense top bus sits within 10° of the object, got ${denseTop.speaker.name}`,
);
assert.ok(
  ringUsed.some((entry) => angularDistance(entry.speaker, { azimuth: 70, elevation: 0 }) >= 30),
  "speaker ring must snap az70 to its 70°-spaced bed speakers (the defect this fixes)",
);
const denseSpread = Math.max(...denseUsed.map((e) => angularDistance(e.speaker, { azimuth: 70, elevation: 0 })));
const ringSpread = Math.max(...ringUsed.map((e) => angularDistance(e.speaker, { azimuth: 70, elevation: 0 })));
assert.ok(denseSpread + 10 <= ringSpread, `dense image narrower than ring (${denseSpread} vs ${ringSpread})`);

// Object exactly on a fill direction (az 90, el 45): single-bus precision.
const elevPos = { azimuth: 90, elevation: 45, distance: 1 };
const elevUsed = usedBuses(denseLayout, denseSolver.pan(elevPos, 0), 0.001);
const elevTop = elevUsed.reduce((a, b) => (b.gain > a.gain ? b : a));
assert.equal(elevTop.speaker.name, "Dense_t90");
assert.ok(elevTop.gain > 0.85, `single fill carries the object, got ${elevTop.gain}`);

// --- fills never leak into the physical outputs ------------------------------
function mockNode(kind, created) {
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
const mockCtx = (created) => ({
  createChannelMerger: (channels) => ({ ...mockNode(`merger:${channels}`, created), channels }),
  createChannelSplitter: (channels) => ({ ...mockNode(`splitter:${channels}`, created), channels }),
  createGain: () => ({ ...mockNode("gain", created), gain: { value: 0 } }),
});

// Stereo downmix: no splitter output belonging to a fill bus may be connected.
{
  const created = [];
  const ctx = mockCtx(created);
  const graph = {
    ctx,
    node: mockNode("worklet", created),
    topology,
    postNodes: [],
  };
  SpatialRenderer.prototype.buildStereoPath.call(graph, topology.length, mockNode("output", created));
  const bedBusCount = topology.filter((s) => !s.binauralOnly).length;
  for (const splitter of created.filter((entry) => entry.kind === `splitter:${topology.length}`)) {
    assert.ok(
      splitter.connections.every((connection) => connection.output < RENDER_TOPOLOGY.length || !topology[connection.output]?.binauralOnly),
      "stereo downmix never connects a dense fill bus",
    );
    assert.equal(splitter.connections.length, bedBusCount * 2, "only real speaker buses feed L/R gains");
  }
}

// Multichannel projector: name-mapped layout speakers only, fills unmapped.
{
  const created = [];
  const ctx = mockCtx(created);
  const graph = {
    ctx,
    node: mockNode("worklet", created),
    multichannelOutput: mockNode("multichannel-output", created),
    topology,
    postNodes: [],
    layoutId: SpatialRenderer.prototype.layoutId,
    lr4: SpatialRenderer.prototype.lr4,
  };
  SpatialRenderer.prototype.createMultichannelProjector.call(graph, layout714, 1);
  for (const splitter of created.filter((entry) => entry.kind === `splitter:${topology.length}`)) {
    assert.ok(
      splitter.connections.every((connection) => connection.output < RENDER_TOPOLOGY.length),
      "multichannel projector never maps a dense fill bus",
    );
  }
}

console.log("dense-binaural-objects: all assertions passed");
