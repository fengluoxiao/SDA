import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { getBinauralIrSet, setBinauralAssetLoader } from "../src/hrtf.ts";

// The test is bundled into tmp/ before execution, so module-relative paths no
// longer identify the repository root. Test runners execute from that root.
const root = path.resolve(process.cwd());
const publicAssets = path.join(root, "apps/web/public");
const loaded = [];
const originalFetch = globalThis.fetch;

globalThis.fetch = async () => {
  throw new Error("Electron bundled loader must not call fetch");
};
setBinauralAssetLoader(async (assetPath) => {
  loaded.push(assetPath);
  const bytes = await readFile(path.join(publicAssets, ...assetPath.split("/")));
  const copy = new Uint8Array(bytes.byteLength);
  copy.set(bytes);
  return copy.buffer;
});

try {
  const [normal, pinna, dense] = await Promise.all([
    getBinauralIrSet("file:///ignored/hrtf"),
    getBinauralIrSet("file:///ignored/hrtf-ku100-d2"),
    getBinauralIrSet("file:///ignored/hrtf-dense"),
  ]);

  assert.equal(normal.positions.length, 17, "normal KU100 set stays 17 directions");
  assert.equal(pinna.positions.length, 17, "KU100 + D2 pinna set stays 17 directions");
  assert.equal(dense.positions.length, 61, "dense KU100 set loads all 61 directions");
  assert.equal(dense.calibrated, true, "dense KU100 set shares the calibrated bed reference");
  assert.ok(loaded.includes("hrtf/hrtf-set.json"), "normal set keeps hrtf directory");
  assert.ok(loaded.includes("hrtf-ku100-d2/hrtf-set.json"), "pinna set keeps its own directory");
  assert.ok(loaded.includes("hrtf-dense/hrtf-set.json"), "dense set keeps hrtf-dense directory");
  assert.ok(
    loaded.some((assetPath) => assetPath.startsWith("hrtf-dense/") && assetPath.endsWith(".f32")),
    "dense dry/wet IRs are read from hrtf-dense rather than normal hrtf",
  );
  assert.ok(
    normal.positions.every((position) => position.dry.length === position.dryLen * 2
      && position.wet.length === position.wetLen * 2),
    "normal IR ear-pairs stay intact",
  );
  assert.ok(
    dense.positions.every((position) => position.dry.length === position.dryLen * 2
      && position.wet.length === position.wetLen * 2),
    "dense IR ear-pairs stay intact",
  );
} finally {
  setBinauralAssetLoader(null);
  globalThis.fetch = originalFetch;
}

console.log("electron HRTF loader: normal, pinna, and dense asset paths passed");
