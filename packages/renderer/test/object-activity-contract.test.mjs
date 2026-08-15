import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dirname, "../worklet/sda-renderer.worklet.js"), "utf8");
assert.match(source, /sample \*= gain \* muteGain/);
assert.match(source, /Math\.abs\(sample\) >= 0\.001/);
assert.match(source, /activityUntil = samplePosition \+ this\.activityHoldSamples/);
assert.match(source, /sourceId\.startsWith\("obj:"\)/);
assert.match(source, /src\.active && this\.consumed <= src\.activityUntil/);
assert.match(source, /activeObjectIds/);

console.log("object activity worklet contract tests passed");
