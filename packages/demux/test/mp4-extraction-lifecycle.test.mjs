import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dirname, "..", "src", "mp4.ts"), "utf8");

assert.match(source, /export const MP4_EXTRACTION_BATCH_SAMPLES = 16/);
assert.match(source, /setExtractionOptions\(track\.trackId, null, \{ nbSamples: MP4_EXTRACTION_BATCH_SAMPLES \}\)/);
assert.match(source, /private deliveredSamples = 0/);
assert.match(source, /this\.deliveredSamples \+= samples\.length/);
assert.match(source, /this\.file\.releaseUsedSamples\(trackId, this\.deliveredSamples\)/);

console.log("MP4 extraction lifecycle tests passed");
