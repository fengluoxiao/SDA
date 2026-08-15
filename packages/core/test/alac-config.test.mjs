import assert from "node:assert/strict";
import * as core from "../pkg-node/sda_core.cjs";

// Complete 48 kHz stereo ALAC MP4 configuration atom. The decoder must receive
// the atom, not just its 28-byte payload, because MP4 carries its box header.
const cookie = Uint8Array.from([
  0, 0, 0, 36, 97, 108, 97, 99,
  0, 0, 0, 0, 0, 0, 16, 0, 0, 24, 40, 10, 14, 2,
  0, 255, 0, 0, 96, 4, 0, 35, 40, 0, 0, 0, 187, 128,
]);

const decoder = core.SdaDecoder.withConfig("alac", cookie);
assert.equal(decoder.codec, "alac");
decoder.free();
assert.throws(
  () => core.SdaDecoder.withConfig("alac", cookie.subarray(0, 20)),
  /invalid ALAC configuration/i,
  "rejects a truncated MP4 ALAC cookie",
);

console.log("ALAC configuration tests passed");
