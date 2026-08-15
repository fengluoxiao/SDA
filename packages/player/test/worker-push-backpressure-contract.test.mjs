import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const player = readFileSync(join(import.meta.dirname, "..", "src", "player.ts"), "utf8");
const worker = readFileSync(join(import.meta.dirname, "..", "src", "decoder.worker.ts"), "utf8");
const app = readFileSync(join(import.meta.dirname, "..", "..", "..", "apps", "web", "src", "App.tsx"), "utf8");

assert.match(player, /private pushWorkerChunk\(chunk: ArrayBuffer\): Promise<void>/);
assert.match(player, /pendingWorkerPushes\.set\(sequence, \{ resolve, reject \}\)/);
assert.match(player, /await this\.pushWorkerChunk\(copy\)/);
assert.match(player, /case "push-ack"/);
assert.match(worker, /self\.postMessage\(\{ type: "push-ack", sequence: msg\.sequence \}\)/);
assert.match(worker, /if \(msg\.type === "push"\) self\.postMessage\(\{ type: "push-ack", sequence: msg\.sequence, error: message \}\)/);
assert.match(app, /const FILE_CHUNK_SIZE = 1 << 18/);

console.log("worker push backpressure contract tests passed");
