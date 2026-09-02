import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { resolve } from "node:path";

const executable = resolve("apps/native-renderer/target/release/sda-native-renderer.exe");
const child = spawn(executable, [], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
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

child.stdin.write(jsonFrame({ type: "hello", protocol: 1 }));
child.stdin.write(jsonFrame({ type: "addSource", id: "obj:7" }));
child.stdin.write(pcmFrame("obj:7", 48_000, new Float32Array([0.25, -0.5, 0.75])));
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
assert.ok(events.some((event) => event.type === "ack" && event.command === "feed" && event.accepted));
const health = events.find((event) => event.type === "health");
assert.ok(health && health.activeSources === 1, "binary PCM source remains independently registered");
assert.ok(events.some((event) => event.type === "ack" && event.command === "shutdown"));

console.log("native renderer binary protocol tests passed");
