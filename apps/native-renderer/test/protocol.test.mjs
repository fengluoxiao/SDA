import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { resolve } from "node:path";

const executable = resolve("apps/native-renderer/target/release/sda-native-renderer.exe");
const child = spawn(executable, [], {
  stdio: ["pipe", "pipe", "pipe"],
  windowsHide: true,
  env: { ...process.env, SDA_HRTF_ROOT: resolve("apps/web/public") },
});
let stdout = "";
let stderr = "";
child.stdout.setEncoding("utf8");
child.stderr.setEncoding("utf8");
child.stdout.on("data", (chunk) => { stdout += chunk; });
child.stderr.on("data", (chunk) => { stderr += chunk; });

const jsonFrame = (command) => {
  const body = Buffer.from(JSON.stringify(command));
  const header = Buffer.allocUnsafe(5);
  header.writeUInt8("J".charCodeAt(0), 0);
  header.writeUInt32LE(body.length, 1);
  return Buffer.concat([header, body]);
};
const pcmFrame = (id, start, samples) => {
  const idBytes = Buffer.from(id);
  const header = Buffer.allocUnsafe(15);
  header.writeUInt8("P".charCodeAt(0), 0);
  header.writeUInt16LE(idBytes.length, 1);
  header.writeBigUInt64LE(BigInt(start), 3);
  header.writeUInt32LE(samples.length, 11);
  return Buffer.concat([header, idBytes, Buffer.from(samples.buffer, samples.byteOffset, samples.byteLength)]);
};
const batchFrame = (start, entries) => {
  const header = Buffer.allocUnsafe(11);
  header.writeUInt8("B".charCodeAt(0), 0);
  header.writeBigUInt64LE(BigInt(start), 1);
  header.writeUInt16LE(entries.length, 9);
  const parts = [header];
  for (const { id, samples } of entries) {
    const idBytes = Buffer.from(id);
    const entryHeader = Buffer.allocUnsafe(6);
    entryHeader.writeUInt16LE(idBytes.length, 0);
    entryHeader.writeUInt32LE(samples.length, 2);
    parts.push(entryHeader, idBytes, Buffer.from(samples.buffer, samples.byteOffset, samples.byteLength));
  }
  return Buffer.concat(parts);
};

child.stdin.write(jsonFrame({ type: "hello", protocol: 1 }));
child.stdin.write(jsonFrame({ type: "addSource", id: "obj:7" }));
child.stdin.write(jsonFrame({ type: "addSource", id: "bed:0" }));
child.stdin.write(batchFrame(48_000, [
  { id: "obj:7", samples: new Float32Array([0.25, -0.5, 0.75]) },
  { id: "bed:0", samples: new Float32Array([0.1, 0.2, 0.3]) },
]));
child.stdin.write(jsonFrame({ type: "objectEvents", events: [{ id: 7, samplePos: 48_000, hasPos: true, pos: [0, 1, 0], gainDb: 0, size: [0, 0, 0], rampDuration: 1 }] }));
child.stdin.write(jsonFrame({ type: "headPose", orientation: [0, 0, 0, 1] }));
child.stdin.write(jsonFrame({ type: "setHrtf", set: "hrtf", wetWeight: 0.2 }));
child.stdin.write(jsonFrame({ type: "setOutputActive", active: true }));
child.stdin.write(jsonFrame({ type: "startAt", origin: 48_000 }));
child.stdin.write(jsonFrame({ type: "health" }));
child.stdin.write(jsonFrame({ type: "shutdown" }));
child.stdin.end();

const exitCode = await new Promise((resolveExit, reject) => {
  child.once("error", reject);
  child.once("exit", (code) => resolveExit(code));
});
assert.equal(exitCode, 0, stderr);
const events = stdout.trim().split("\n").filter(Boolean).map((line) => JSON.parse(line));
assert.equal(events[0].type, "ready");
assert.ok(events.some((event) => event.type === "ack" && event.command === "hello" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "addSource" && event.accepted));
assert.ok(events.some((event) => event.type === "batchAck" && event.accepted && event.samples === 3));
assert.ok(events.some((event) => event.type === "ack" && event.command === "objectEvents" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "headPose" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "setHrtf" && event.accepted), "camelCase wetWeight loads the calibrated bundled HRTF");
assert.ok(events.some((event) => event.type === "ack" && event.command === "setOutputActive" && event.accepted), "native output activates only after HRTF configuration");
assert.ok(events.some((event) => event.type === "ack" && event.command === "setHrtf" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "startAt" && event.accepted), "startAt must succeed after HRTF is configured");
const health = events.find((event) => event.type === "health");
assert.ok(health && health.activeSources === 2, "atomic PCM batch preserves independently registered sources");
assert.ok(events.some((event) => event.type === "ack" && event.command === "shutdown"));

console.log("native renderer binary protocol tests passed");
