import assert from "node:assert/strict";
import { HeadPoseTracker, rotateAdmVector, yawQuaternion } from "../src/head-pose.ts";

const radians = (degrees) => degrees * Math.PI / 180;
const yaw = (degrees) => [0, 0, Math.sin(radians(degrees) / 2), Math.cos(radians(degrees) / 2)];
const close = (actual, expected, message) => assert.ok(Math.abs(actual - expected) < 1e-6, `${message}: ${actual} !== ${expected}`);

// A positive ADM yaw turns the head left, so a world-front object is to the
// listener's right (negative ADM azimuth) after inverse head rotation.
const tracker = new HeadPoseTracker({ yawMode: "yaw", smoothingMs: 0, maxDegreesPerSecond: 1e9, staleAfterMs: 100 });
assert.equal(tracker.set({ orientation: yaw(90), timestampMs: 0 }, 0), true);
const right = tracker.headRelative({ azimuth: 0, elevation: 0, distance: 2 }, 0);
close(right.azimuth, -90, "positive yaw rotates world front rightward");
assert.equal(right.distance, 2);

assert.equal(tracker.recenter(), true);
const centered = tracker.headRelative({ azimuth: 0, elevation: 0, distance: 1 }, 1);
close(centered.azimuth, 0, "recenter makes the current view neutral");
assert.equal(tracker.set({ orientation: yaw(45), timestampMs: 2 }, 2), true);
close(tracker.headRelative({ azimuth: 0, elevation: 0, distance: 1 }, 2).azimuth, 45, "post-recenter pose remains relative to the neutral view");
assert.equal(tracker.set({ orientation: [0, 0, 0, 0], timestampMs: 3 }, 3), false, "rejects zero-length quaternions");
assert.equal(tracker.set({ orientation: yaw(10), timestampMs: -200 }, 3), false, "rejects stale timestamps");
assert.equal(tracker.isActive(103), false, "stale pose disables tracking");
const stale = tracker.headRelative({ azimuth: 30, elevation: 10, distance: 1 }, 103);
close(stale.azimuth, 30, "stale pose returns unrotated world direction");

const full = new HeadPoseTracker({ yawMode: "full", smoothingMs: 0, maxDegreesPerSecond: 10000 });
// Pitch around ADM X must be ignored by yaw mode but retained by full mode.
const pitch = [Math.sin(radians(45) / 2), 0, 0, Math.cos(radians(45) / 2)];
assert.equal(full.set({ orientation: pitch }, 0), true);
const pitched = full.headRelative({ azimuth: 0, elevation: 0, distance: 1 }, 0);
close(pitched.elevation, -45, "full mode retains pitch");
const yawOnly = yawQuaternion(pitch);
assert.deepEqual(yawOnly, [0, 0, 0, 1], "yaw mode removes pitch");

const rotated = rotateAdmVector(yaw(90), [0, 1, 0]);
close(rotated[0], -1, "ADM yaw rotates front toward left");
close(rotated[1], 0, "ADM yaw front y");

const sensitive = new HeadPoseTracker({
  yawMode: "yaw",
  sensitivity: 1.25,
  smoothingMs: 0,
  maxDegreesPerSecond: 1e9,
});
assert.equal(sensitive.set({ orientation: yaw(20) }, 0), true);
assert.equal(sensitive.recenter(), true);
assert.equal(sensitive.set({ orientation: yaw(60) }, 1), true);
const sensitiveFront = sensitive.headRelative({ azimuth: 0, elevation: 0, distance: 2.5 }, 1);
close(sensitiveFront.azimuth, -50, "sensitivity scales rotation after recentering");
close(sensitiveFront.distance, 2.5, "centered head rotation preserves source radius");

const continuous = new HeadPoseTracker({
  yawMode: "yaw",
  sensitivity: 1.5,
  smoothingMs: 18,
  deadZoneDegrees: 0.6,
  maxDegreesPerSecond: 480,
  updateHz: 120,
});
assert.equal(continuous.set({ orientation: yaw(0) }, 0), true);
assert.equal(continuous.set({ orientation: yaw(100) }, 1000 / 120), true);
const firstFastTurnFrame = continuous.headRelative(
  { azimuth: 0, elevation: 0, distance: 1 },
  1000 / 120,
);
close(firstFastTurnFrame.azimuth, -6, "fast 100-degree turn advances continuously on its first frame");
assert.ok(Math.abs(firstFastTurnFrame.azimuth) < 150, "fast turn must not reach the amplified target in one frame");
const secondFastTurnFrame = continuous.headRelative(
  { azimuth: 0, elevation: 0, distance: 1 },
  2000 / 120,
);
close(secondFastTurnFrame.azimuth, -12, "render ticks keep interpolating without another provider sample");

const continuousLeft = new HeadPoseTracker({
  yawMode: "yaw",
  sensitivity: 1.5,
  smoothingMs: 18,
  deadZoneDegrees: 0.6,
  maxDegreesPerSecond: 480,
  updateHz: 120,
});
assert.equal(continuousLeft.set({ orientation: yaw(0) }, 0), true);
assert.equal(continuousLeft.set({ orientation: yaw(-100) }, 1000 / 120), true);
close(
  continuousLeft.headRelative({ azimuth: 0, elevation: 0, distance: 1 }, 1000 / 120).azimuth,
  6,
  "fast negative turn follows the same continuous first-frame limit",
);

const signContinuous = new HeadPoseTracker({
  yawMode: "full",
  smoothingMs: 0,
  deadZoneDegrees: 0,
  maxDegreesPerSecond: 1e9,
});
const tenDegrees = yaw(10);
assert.equal(signContinuous.set({ orientation: tenDegrees }, 0), true);
assert.equal(signContinuous.set({ orientation: tenDegrees.map((value) => -value) }, 1), true);
close(
  signContinuous.headRelative({ azimuth: 0, elevation: 0, distance: 1 }, 1).azimuth,
  -10,
  "equivalent q/-q samples cannot create a full-turn jump",
);

const stable = new HeadPoseTracker({
  yawMode: "yaw",
  smoothingMs: 0,
  deadZoneDegrees: 0.6,
  maxDegreesPerSecond: 1e9,
});
assert.equal(stable.set({ orientation: yaw(0) }, 0), true);
assert.equal(stable.set({ orientation: yaw(0.4) }, 1), true);
close(
  stable.headRelative({ azimuth: 0, elevation: 0, distance: 1 }, 1).azimuth,
  0,
  "stationary sub-degree sensor noise stays centered",
);
assert.equal(stable.set({ orientation: yaw(2) }, 2), true);
close(
  stable.headRelative({ azimuth: 0, elevation: 0, distance: 1 }, 2).azimuth,
  -2,
  "intentional movement outside the dead zone remains responsive",
);

console.log("head pose tests: OK");
