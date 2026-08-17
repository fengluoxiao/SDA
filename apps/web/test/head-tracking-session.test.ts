import assert from "node:assert/strict";
import { HeadPoseTracker, type Quaternion } from "@sda/renderer";
import { HeadTrackingSession } from "../src/head-tracking-session";

const radians = (degrees: number) => degrees * Math.PI / 180;
const yaw = (degrees: number): Quaternion => [
  0,
  0,
  Math.sin(radians(degrees) / 2),
  Math.cos(radians(degrees) / 2),
];
const yawDegrees = (orientation: Quaternion) => (
  2 * Math.atan2(orientation[2], orientation[3]) * 180 / Math.PI
);

const session = new HeadTrackingSession();
assert.equal(yawDegrees(session.update({ orientation: yaw(20) }).orientation), 20);

const centered = session.recenter();
assert.ok(centered);
assert.ok(Math.abs(yawDegrees(centered.orientation)) < 1e-9);

const turned = session.update({ orientation: yaw(60) });
assert.ok(Math.abs(yawDegrees(turned.orientation) - 40) < 1e-9);
assert.equal(session.latestPose, turned, "replacement players inherit the session-relative pose");

const replacementTracker = new HeadPoseTracker({
  yawMode: "yaw",
  smoothingMs: 0,
  maxDegreesPerSecond: 1e9,
});
assert.equal(replacementTracker.set(session.latestPose!, 0), true);
const source = replacementTracker.headRelative({ azimuth: 0, elevation: 0, distance: 1 }, 0);
assert.ok(
  Math.abs(source.azimuth + 40) < 1e-9,
  "after automatic next, a left turn must keep the frontal source at the right ear",
);

session.clear();
assert.equal(session.latestPose, null);

console.log("head-tracking session handoff tests passed");
