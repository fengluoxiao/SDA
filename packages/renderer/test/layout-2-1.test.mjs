import assert from "node:assert/strict";
import { LAYOUTS, detectLayoutId, physicalChannelOrder } from "../src/layouts.ts";
import { VbapSolver } from "../src/vbap.ts";

const layout = LAYOUTS["2.1"];
assert.deepEqual(layout.map((speaker) => speaker.name), ["FrontLeft", "FrontRight", "LFE"]);
assert.deepEqual(physicalChannelOrder(layout), [0, 1, 2], "2.1 physical order is FL, FR, LFE");

// Auto remains conservative: it must not select a physical subwoofer route for
// a device merely because the source is stereo or contains an LFE bed.
assert.equal(detectLayoutId(["L", "R"], false), "5.1");
assert.equal(detectLayoutId(["L", "R", "LFE"], false), "5.1");
assert.equal(detectLayoutId(["L", "R"], true), "7.1.4");

// The two non-LFE speakers form a valid VBAP pair; LFE is excluded from panning.
const vbap = new VbapSolver(layout);
const front = vbap.pan({ azimuth: 0, elevation: 0, distance: 1 }, 0);
assert.ok(front[0] > 0 && front[1] > 0, "front program signal reaches both mains");
assert.equal(front[2], 0, "program panning never creates a synthetic LFE signal");

console.log("2.1 layout tests passed");
