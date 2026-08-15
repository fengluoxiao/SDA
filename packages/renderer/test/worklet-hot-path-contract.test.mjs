import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dirname, "..", "worklet", "sda-renderer.worklet.js"), "utf8");

assert.match(source, /routeBuses: \[\]/);
assert.match(source, /refreshRouteBuses\(src, includeRampTargets = false\)/);
assert.match(source, /for \(const bus of src\.routeBuses\)/);
assert.doesNotMatch(source, /for \(let bus = 0; bus < this\.busCount && bus < buses\.length; bus\+\+\) \{\s*const busGain/);
assert.match(source, /nextScheduledGainAt: Number\.POSITIVE_INFINITY/);
assert.match(source, /samplePosition >= src\.nextScheduledGainAt/);
assert.match(source, /nextLifecycleAt: Number\.POSITIVE_INFINITY/);
assert.match(source, /samplePosition >= src\.nextLifecycleAt/);
assert.match(source, /lifecycleCursor: 0/);
assert.doesNotMatch(source, /src\.lifecycleEvents\.shift\(\)/);
assert.match(source, /futureResumeCount === 0/);
assert.match(source, /activityHoldSamples/);
assert.match(source, /banksToClear = activeBankMask \| this\.lastActiveBankMask/);

console.log("worklet hot-path contract tests passed");
