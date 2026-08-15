import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(
  join(import.meta.dirname, "..", "worklet", "sda-renderer.worklet.js"),
  "utf8",
);

assert.match(source, /const CALLBACK_GAP_TELEMETRY_MS = 12/);
assert.match(source, /const CALLBACK_GAP_ESCALATION_MS = 25/);
assert.match(source, /if \(gapMs > CALLBACK_GAP_TELEMETRY_MS\)/);
assert.match(source, /if \(gapMs > CALLBACK_GAP_ESCALATION_MS\) this\.callbackGapsOver25Ms\+\+/);
assert.match(source, /callbackGapsOver25Ms: this\.callbackGapsOver25Ms/);
assert.match(source, /this\.callbackGapsOver25Ms = 0/);

// Strict comparison preserves the intended boundaries: 13ms is broad telemetry
// only; 25.0ms is not escalation evidence; 25.1ms is eligible evidence.
const broad = (gapMs) => gapMs > 12;
const qualified = (gapMs) => gapMs > 25;
assert.equal(broad(13), true);
assert.equal(qualified(13), false);
assert.equal(qualified(25), false);
assert.equal(qualified(25.1), true);

console.log("callback-gap escalation contract tests passed");
