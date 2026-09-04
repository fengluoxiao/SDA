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
const headphoneFirFrame = (preamp, left, right) => {
  const header = Buffer.allocUnsafe(13);
  header.writeUInt8("H".charCodeAt(0), 0);
  header.writeFloatLE(preamp, 1);
  header.writeUInt32LE(left.length, 5);
  header.writeUInt32LE(right.length, 9);
  return Buffer.concat([
    header,
    Buffer.from(left.buffer, left.byteOffset, left.byteLength),
    Buffer.from(right.buffer, right.byteOffset, right.byteLength),
  ]);
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

const observedEvents = () => stdout
  .split("\n")
  .filter(Boolean)
  .flatMap((line) => {
    try { return [JSON.parse(line)]; } catch { return []; }
  });
const waitFor = async (predicate, detail) => {
  const deadline = Date.now() + 5_000;
  while (!predicate(observedEvents())) {
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${detail}: ${stdout}`);
    await new Promise((resolveWait) => setTimeout(resolveWait, 5));
  }
};
const send = (command) => child.stdin.write(jsonFrame(command));
const acceptedAck = (command) => (events) => events.some(
  (event) => event.type === "ack" && event.command === command && event.accepted,
);

const activityPcm = new Float32Array(20_000).fill(0.25);

send({ type: "hello", protocol: 6 });
await waitFor(acceptedAck("hello"), "hello ACK");
send({ type: "addSource", id: "obj:7", at: 48_000 });
await waitFor(acceptedAck("addSource"), "object source ACK");
send({ type: "addSource", id: "bed:0", at: 48_000, bedLabel: "LFE" });
await waitFor((events) => events.filter((event) => event.type === "ack" && event.command === "addSource" && event.accepted).length >= 2, "bed source ACK");
child.stdin.write(batchFrame(48_000, [
  { id: "obj:7", samples: activityPcm },
  { id: "bed:0", samples: new Float32Array(20_000).fill(0.1) },
]));
await waitFor((events) => events.some((event) => event.type === "batchAck" && event.accepted), "batch ACK");
send({ type: "objectEvents", events: [{ id: 7, samplePos: 48_000, hasPos: true, pos: [0, 1, 0], gainDb: 0, size: [0, 0, 0], rampDuration: 1 }] });
await waitFor(acceptedAck("objectEvents"), "object events ACK");
send({ type: "headPose", orientation: [0, 0, 0, 1] });
await waitFor(acceptedAck("headPose"), "head pose ACK");
send({ type: "setLfeMuted", muted: true });
await waitFor(acceptedAck("setLfeMuted"), "LFE mute ACK");
send({ type: "setVolume", volume: 0.5 });
await waitFor(acceptedAck("setVolume"), "volume ACK");
send({ type: "setProgramEnabled", enabled: true });
await waitFor(acceptedAck("setProgramEnabled"), "program enable ACK");
send({ type: "setProgramGain", gain: 0.5, at: 48_000 });
await waitFor(acceptedAck("setProgramGain"), "program gain ACK");
send({ type: "setBinauralEq", low: 1.5, mid: -2, high: 0.5, lowCut: true });
await waitFor(acceptedAck("setBinauralEq"), "binaural EQ ACK");
child.stdin.write(headphoneFirFrame(0.5, new Float32Array([1, 0]), new Float32Array([0.5, 0])));
await waitFor(acceptedAck("setHeadphoneFir"), "headphone FIR ACK");
send({ type: "setHrtf", set: "hrtf", wetWeight: 0.04 });
await waitFor(acceptedAck("setHrtf"), "HRTF ACK");
send({ type: "setLayout", layout: "5.1" });
await waitFor(acceptedAck("setLayout"), "5.1 layout ACK");
send({ type: "health" });
await waitFor((events) => events.some((event) => event.type === "health" && event.layout === "5.1" && event.spatialBusCount === 5), "5.1 graph health");
send({ type: "setLayout", layout: "9.1.6" });
await waitFor((events) => events.filter((event) => event.type === "ack" && event.command === "setLayout" && event.accepted).length >= 2, "9.1.6 layout ACK");
send({ type: "setLayout", layout: "not-a-layout" });
await waitFor((events) => events.some((event) => event.type === "ack" && event.command === "setLayout" && !event.accepted), "invalid layout rejection");
send({ type: "health" });
await waitFor((events) => events.some((event) => event.type === "health" && event.layout === "9.1.6" && event.spatialBusCount === 15), "9.1.6 graph health");
send({ type: "setOutputActive", active: true });
await waitFor(acceptedAck("setOutputActive"), "output activation ACK");
send({ type: "startAt", origin: 48_000 });
await waitFor(acceptedAck("startAt"), "start ACK");
send({ type: "health" });
await waitFor((events) => events.some((event) => event.type === "health"), "health event");
await waitFor(
  (events) => events.some((event) => event.type === "objectActivity" && Array.isArray(event.ids) && event.ids.length === 1 && event.ids[0] === 7),
  "DAC-aligned object activity",
);
send({ type: "shutdown" });
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
assert.ok(events.some((event) => event.type === "batchAck" && event.accepted && event.samples === 20_000));
assert.ok(events.some((event) => event.type === "ack" && event.command === "objectEvents" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "headPose" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "setLfeMuted" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "setVolume" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "setProgramEnabled" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "setProgramGain" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "setBinauralEq" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "setHeadphoneFir" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "setHrtf" && event.accepted), "camelCase wetWeight loads the calibrated bundled HRTF");
assert.ok(events.some((event) => event.type === "ack" && event.command === "setLayout" && event.accepted), "native layout changes are accepted");
assert.ok(events.some((event) => event.type === "health" && event.layout === "5.1" && event.spatialBusCount === 5), "5.1 creates only five physical spatial buses");
assert.ok(events.some((event) => event.type === "health" && event.layout === "9.1.6" && event.spatialBusCount === 15), "9.1.6 creates fifteen physical spatial buses");
assert.ok(events.some((event) => event.type === "ack" && event.command === "setOutputActive" && event.accepted), "native output activates only after HRTF configuration");
assert.ok(events.some((event) => event.type === "ack" && event.command === "setHrtf" && event.accepted));
assert.ok(events.some((event) => event.type === "ack" && event.command === "startAt" && event.accepted), "startAt must succeed after HRTF is configured");
const health = events.find((event) => event.type === "health");
assert.ok(health && health.activeSources === 2, "atomic PCM batch preserves independently registered sources");
assert.ok(events.some((event) => event.type === "objectActivity" && event.ids?.join(",") === "7"), "only the audible object source reports activity");
assert.ok(events.some((event) => event.type === "ack" && event.command === "shutdown"));

console.log("native renderer binary protocol tests passed");
