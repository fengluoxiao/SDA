import assert from "node:assert/strict";
import { SpatialRenderer } from "../src/renderer.ts";

const controlMessages = [];
const controlRenderer = {
  poseControlEnabled: false,
  node: { port: { postMessage: (message) => controlMessages.push(message) } },
};
SpatialRenderer.prototype.syncPoseControl.call(controlRenderer, true);
SpatialRenderer.prototype.syncPoseControl.call(controlRenderer, true);
SpatialRenderer.prototype.syncPoseControl.call(controlRenderer, false);
assert.deepEqual(controlMessages, [
  { type: "headTracking", enabled: true },
  { type: "headTracking", enabled: false },
]);

const objectState = {
  id: "obj:19",
  spread: 0,
  position: { azimuth: 45, elevation: 0, distance: 1 },
  gainDb: 0,
  isLfe: false,
  bedLabel: undefined,
  snapBus: -1,
};
const messageRenderer = {
  mode: "binaural",
  headPose: {
    isActive: () => true,
    headRelative: (position) => position,
  },
  vbap: { pan: () => new Float32Array([1]) },
  topology: [{}],
  renderToTopology: new Int16Array([0]),
  renderLayout: [{}],
  expansion: new Map(),
  lfeMuted: false,
  bedOccupiedBuses: () => new Set(),
};
const scheduled = SpatialRenderer.prototype.gainMessage.call(messageRenderer, objectState, 1536, 48000);
assert.equal(scheduled.type, "scheduleGains");
assert.equal(scheduled.poseControlled, true);
assert.equal(scheduled.poseUpdate, false);

const pose = SpatialRenderer.prototype.gainMessage.call(messageRenderer, objectState, 512, undefined, true);
assert.equal(pose.type, "gains");
assert.equal(pose.poseControlled, true);
assert.equal(pose.poseUpdate, true);

const futurePose = SpatialRenderer.prototype.gainMessage.call(messageRenderer, objectState, 1536, 48000, true);
assert.equal(futurePose.type, "scheduleGains");
assert.equal(futurePose.poseControlled, true);
assert.equal(futurePose.poseUpdate, true);

const scheduledMessages = [];
const eventState = {
  ...objectState,
  hasObjectMetadata: false,
  objectRampEndSample: Number.NEGATIVE_INFINITY,
  objectPoseTimeline: [],
};
const eventRenderer = {
  ...messageRenderer,
  node: { port: { postMessage: (message) => scheduledMessages.push(message) } },
  sources: new Map([["obj:19", eventState]]),
  gainMessage: SpatialRenderer.prototype.gainMessage,
};
const accepted = SpatialRenderer.prototype.applyEvents.call(eventRenderer, [{
  id: 19,
  samplePos: 48000,
  hasPos: true,
  pos: [0, 1, 0],
  gainDb: -3,
  size: [0, 0, 0],
  anchor: "room",
  distanceM: null,
  distanceInfinite: false,
  screenFactor: null,
  depthFactor: null,
  rampDuration: 1536,
}]);
assert.equal(accepted, 1, "paired pose route does not inflate accepted metadata count");
assert.equal(scheduledMessages.length, 1);
assert.equal(scheduledMessages[0].type, "scheduleGainsBatch");
assert.equal(scheduledMessages[0].entries.length, 2);
assert.equal(scheduledMessages[0].entries[0].poseUpdate, false);
assert.equal(scheduledMessages[0].entries[1].poseUpdate, true);
assert.equal(scheduledMessages[0].entries[1].at, 48000);

console.log("head-tracking renderer message tests passed");
