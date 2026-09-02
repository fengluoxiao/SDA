import assert from "node:assert/strict";
import { createRequire } from "node:module";
import path from "node:path";
import { VbapSolver as JsVbapSolver } from "../src/vbap.ts";
import { sphericalToAdm } from "../src/coords.ts";
import { DENSE_BINAURAL_FILLS, LAYOUTS } from "../src/layouts.ts";

const require = createRequire(import.meta.url);
const { VbapSolver: WasmVbapSolver } = require(path.join(process.cwd(), "packages/core/pkg-node/sda_core.cjs"));

const cases = [
  { name: "2.1", layout: LAYOUTS["2.1"] },
  { name: "7.1.4", layout: LAYOUTS["7.1.4"] },
  { name: "9.1.6", layout: LAYOUTS["9.1.6"] },
  { name: "dense", layout: [...LAYOUTS["7.1.4"], ...DENSE_BINAURAL_FILLS] },
];
const positions = [
  { azimuth: 0, elevation: 0, distance: 1 },
  { azimuth: 23, elevation: 0, distance: 1 },
  { azimuth: -73, elevation: 0, distance: 1 },
  { azimuth: 45, elevation: 35, distance: 1 },
  { azimuth: -120, elevation: 45, distance: 1 },
  { azimuth: 0, elevation: 80, distance: 1 },
];
const spreads = [0, 0.2, 0.65, 1];
const epsilon = 2e-6;

for (const { name, layout } of cases) {
  const js = new JsVbapSolver(layout);
  const directions = new Float64Array(layout.length * 3);
  const lfeMask = new Uint8Array(layout.length);
  const azimuths = new Float64Array(layout.length);
  layout.forEach((speaker, index) => {
    directions.set(sphericalToAdm(speaker), index * 3);
    lfeMask[index] = speaker.isLfe ? 1 : 0;
    azimuths[index] = speaker.azimuth;
  });
  const wasm = new WasmVbapSolver(directions, lfeMask, azimuths);
  try {
    const packedPositions = new Float64Array(positions.length * 3);
    positions.forEach((position, index) => packedPositions.set(sphericalToAdm(position), index * 3));
    for (const spread of spreads) {
      const packed = wasm.panBatch(packedPositions, new Float64Array(positions.length).fill(spread));
      positions.forEach((position, row) => {
        const expected = js.pan(position, spread);
        const actual = packed.subarray(row * layout.length, (row + 1) * layout.length);
        expected.forEach((gain, bus) => {
          assert.ok(
            Math.abs(gain - actual[bus]) <= epsilon,
            `${name} spread=${spread} row=${row} bus=${bus}: JS=${gain}, WASM=${actual[bus]}`,
          );
        });
      });
    }
  } finally {
    wasm.free();
  }
}

console.log("WASM VBAP parity: built-in and dense layouts match TypeScript solver");
