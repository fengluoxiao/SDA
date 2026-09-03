import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";

const root = path.resolve(process.cwd());
const appSource = await readFile(path.join(root, "apps/web/src/App.tsx"), "utf8");
const playerSource = await readFile(path.join(root, "packages/player/src/player.ts"), "utf8");

assert.match(
  appSource,
  /return assetUrl\(head === "ku100" \? "hrtf" : `hrtf-\$\{head\}`\);/,
  "complete subject selection must map directly to its own HRTF directory",
);
assert.doesNotMatch(
  appSource,
  /`hrtf-ku100-\$\{head\}`/,
  "the UI must not construct a KU100 hybrid asset path",
);
assert.match(
  appSource,
  /if \(next !== "ku100"\)[\s\S]{0,360}?setDenseBinauralObjects\(false\)/,
  "choosing a non-KU100 subject must disable KU100 dense objects",
);
assert.match(
  playerSource,
  /setDirectory\.startsWith\("hrtf-ku100-"\)/,
  "the player must reject legacy hybrid HRTF directories",
);
assert.match(
  playerSource,
  /!set\.calibrated \|\| !set\.completeSubject \|\| set\.subjectId !== requestedSubject/,
  "the player must require calibrated complete-subject metadata for D2/Hx",
);

console.log("complete subject HRTF runtime contract passed");
