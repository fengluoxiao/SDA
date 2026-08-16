import assert from "node:assert/strict";
import { quaternionAngularVelocity, quaternionEulerAngles } from "../src/head-tracking-telemetry";

const yaw90 = [0, 0, Math.sin(Math.PI / 4), Math.cos(Math.PI / 4)] as const;
const velocity = quaternionAngularVelocity([0, 0, 0, 1], yaw90, 500);
assert.ok(Math.abs(velocity.x) < 1e-9);
assert.ok(Math.abs(velocity.y) < 1e-9);
assert.ok(Math.abs(velocity.z - 180) < 1e-9);

const angles = quaternionEulerAngles(yaw90);
assert.ok(Math.abs(angles.yaw - 90) < 1e-9);
assert.ok(Math.abs(angles.pitch) < 1e-9);
assert.ok(Math.abs(angles.roll) < 1e-9);

// q and -q describe the same pose and must not generate a 360-degree spike.
const equivalent = quaternionAngularVelocity(yaw90, [0, 0, -yaw90[2], -yaw90[3]], 10);
assert.deepEqual(equivalent, { x: 0, y: 0, z: 0 });

console.log("head-tracking telemetry tests passed");
