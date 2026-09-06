import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";

const source = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const start = source.indexOf("onDecodedFormat: (");
const callback = source.slice(start + "onDecodedFormat: ".length, source.indexOf("\n        onBinauralMetadata:", start)).trim().replace(/,$/, "");

test("stereo restores automatic or manually selected immersive layout before PCM", () => {
  for (const immersive of ["auto", "7.1.4", "9.1.6"]) {
    const calls = [];
    let current = true;
    const context = {
      isCurrent: () => current,
      setTrack() {}, setLayoutId() {}, setDetectedLayout() {},
      layoutIdRef: { current: immersive },
      immersiveLayoutRef: { current: "auto" },
      stereoLayoutRef: { current: "2.0" },
      LAYOUTS: { "2.0": "2.0", "2.1": "2.1", "7.1.4": "7.1.4", "9.1.6": "9.1.6" },
      detectLayoutId: () => "7.1.4",
      createdPlayer: { setLayout: (layout) => calls.push(layout), setAutoLayout: () => calls.push("auto") },
    };
    const decoded = runInNewContext(`(${callback})`, context);
    const stereo = { rawBedLabels: ["L", "R"], bedLabels: ["L", "R"], objectChannels: 0 };
    const atmos = { rawBedLabels: ["LFE"], bedLabels: ["LFE"], objectChannels: 16 };
    decoded(stereo);
    assert.equal(context.layoutIdRef.current, "2.0");
    assert.equal(calls.at(-1), "2.0");
    decoded(atmos);
    assert.equal(context.layoutIdRef.current, immersive);
    assert.equal(calls.at(-1), immersive === "auto" ? "7.1.4" : immersive);
    context.stereoLayoutRef.current = "2.1";
    decoded(stereo);
    assert.equal(calls.at(-1), "2.1");
    const count = calls.length;
    current = false;
    decoded(atmos);
    assert.equal(calls.length, count, "retired player changed the layout");
  }
});
