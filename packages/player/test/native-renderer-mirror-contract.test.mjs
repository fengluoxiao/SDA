import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dirname, "../src/player.ts"), "utf8");
assert.match(source, /export interface NativeRendererSink/);
assert.match(source, /nativeRendererSink\?: NativeRendererSink/);
assert.match(source, /this\.nativeRendererSink = options\.nativeRendererSink/);
assert.match(source, /this\.nativeRendererSink\?\.events\(events\)/);
assert.match(source, /this\.nativeRendererSink\?\.addSource\(entry\.id, frame\.samplePos\)/);
assert.match(source, /this\.nativeRendererSink\?\.frame\(frame\.samplePos, entries\)/);
assert.match(source, /native renderer frame mirror failed/);
assert.match(source, /this\.renderer\.feedBatch\(sequence, frame\.samplePos, entries\)/);

console.log("native renderer mirror contract tests passed");
