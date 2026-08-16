import assert from "node:assert/strict";
import { SpatialRenderer, interpolateObjectPosition } from "../src/renderer.ts";

const halfwayRight = interpolateObjectPosition(
  { azimuth: 0, elevation: 0, distance: 1 },
  { azimuth: -90, elevation: 0, distance: 1 },
  0.5,
);
assert.ok(Math.abs(halfwayRight.azimuth + 45) < 1e-6, "object metadata follows an intermediate spatial direction");
assert.ok(Math.abs(halfwayRight.distance - Math.SQRT1_2) < 1e-6, "ADM cartesian interpolation preserves the authored path");

// Symbol I object 17 moves from [-1, 1, 1] to [1, -1, 1] at 4:02.636.
// The authored Cartesian ramp passes over the listener instead of crossfading
// directly between the two opposite HRTF endpoints.
const symbolMidpoint = interpolateObjectPosition(
  { azimuth: 45, elevation: 35.264389682754654, distance: Math.sqrt(3) },
  { azimuth: -135, elevation: 35.264389682754654, distance: Math.sqrt(3) },
  0.5,
);
assert.ok(symbolMidpoint.elevation > 89.999, "the 180-degree Symbol I move traverses its intermediate position");
assert.ok(Math.abs(symbolMidpoint.distance - 1) < 1e-6);

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
assert.deepEqual(eventState.objectPoseTimeline[0].fromPosition, objectState.position);
assert.ok(Math.abs(eventState.objectPoseTimeline[0].position.azimuth) < 1e-12);
assert.equal(eventState.objectPoseTimeline[0].position.elevation, 0);
assert.equal(eventState.objectPoseTimeline[0].position.distance, 1);

const overlappingStart = interpolateObjectPosition(objectState.position, eventState.objectPoseTimeline[0].position, 0.5);
SpatialRenderer.prototype.applyEvents.call(eventRenderer, [{
  id: 19,
  samplePos: 48000 + 768,
  hasPos: true,
  pos: [-1, 0, 0],
  gainDb: -3,
  size: [0, 0, 0],
  anchor: "room",
  distanceM: null,
  distanceInfinite: false,
  screenFactor: null,
  depthFactor: null,
  rampDuration: 1536,
}]);
assert.ok(Math.abs(eventState.objectPoseTimeline[1].fromPosition.azimuth - overlappingStart.azimuth) < 1e-6);
assert.ok(Math.abs(eventState.objectPoseTimeline[1].fromPosition.distance - overlappingStart.distance) < 1e-6);

console.log("head-tracking renderer message tests passed");
