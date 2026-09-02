import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { VbapSolver } = require("../pkg-node/sda_core.cjs");

const directions = new Float64Array([
  0, 1, 0,  // front
  -1, 0, 0, // left
  1, 0, 0,  // right
  0, 0, 1,  // top
  0, -1, 0, // rear LFE (must stay silent)
]);
const lfeMask = new Uint8Array([0, 0, 0, 0, 1]);
const azimuths = new Float64Array([0, 90, -90, 0, 180]);
const solver = new VbapSolver(directions, lfeMask, azimuths);

try {
  assert.equal(solver.speaker_count, 5);
  const gains = solver.panBatch(
    new Float64Array([
      0, 1, 0,  // front
      -1, 0, 0, // left
      0, 0, 1,  // top
    ]),
    new Float64Array([0, 0.5, 0]),
  );
  assert.equal(gains.length, 15, "one logical gain row per input position");
  const row = (index) => gains.subarray(index * 5, (index + 1) * 5);
  assert.ok(row(0)[0] > 0.99, "front target resolves to front speaker");
  assert.ok(row(1)[1] > 0.5, "left target remains localized after spread");
  assert.ok(row(2)[3] > 0.99, "top target resolves to top speaker");
  for (let i = 0; i < 3; i++) {
    const gainsRow = row(i);
    assert.equal(gainsRow[4], 0, "LFE stays excluded from VBAP");
    const power = gainsRow.reduce((sum, value) => sum + value * value, 0);
    assert.ok(Math.abs(power - 1) < 1e-5, "each output row is power normalized");
  }
} finally {
  solver.free();
}

console.log("WASM VBAP batch solver: typed-array API passed");
