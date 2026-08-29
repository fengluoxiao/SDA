import assert from "node:assert/strict";
import { stereoDownmixGains } from "../src/renderer.ts";
import { LAYOUTS } from "../src/layouts.ts";

const close = (actual, expected, message) =>
  assert.ok(Math.abs(actual - expected) < 1e-6, `${message}: ${actual} !== ${expected}`);
const byName = (layout, name) => layout.find((speaker) => speaker.name === name);

// Dolby Lo/Ro 2.0 downmix: L + (−3 dB × C) + (−3 dB × Ls), mirrored for Ro.
const side = 10 ** (-3 / 20);
const lfe = 10 ** (-10 / 20);

const [l, r] = stereoDownmixGains({ azimuth: 30, isLfe: false });
close(l, 1, "front left stays at unity");
close(r, 0, "front left does not feed right");
const [rl, rr] = stereoDownmixGains({ azimuth: -30, isLfe: false });
close(rl, 0, "front right does not feed left");
close(rr, 1, "front right stays at unity");

const [cl, cr] = stereoDownmixGains({ azimuth: 0, isLfe: false });
close(cl, side, "center folds at −3 dB into left");
close(cr, side, "center folds at −3 dB into right");

for (const azimuth of [60, 100, 110, 140, 45, 135, 90]) {
  const [gl, gr] = stereoDownmixGains({ azimuth, isLfe: false });
  close(gl, side, `left-side speaker ${azimuth}° folds at −3 dB`);
  close(gr, 0, `left-side speaker ${azimuth}° silent on right`);
  const [grl, grr] = stereoDownmixGains({ azimuth: -azimuth, isLfe: false });
  close(grl, 0, `right-side speaker −${azimuth}° silent on left`);
  close(grr, side, `right-side speaker −${azimuth}° folds at −3 dB`);
}

const [fl, fr] = stereoDownmixGains({ azimuth: 45, isLfe: true });
close(fl, lfe, "LFE folds at −10 dB into left");
close(fr, lfe, "LFE folds at −10 dB into right");

const [bcl, bcr] = stereoDownmixGains({ azimuth: 180, isLfe: false });
close(bcl, side, "rear center feeds both channels");
close(bcr, side, "rear center feeds both channels");

// The declared layouts must land in the intended buckets end to end.
const layout = LAYOUTS["7.1.4"];
close(stereoDownmixGains(byName(layout, "FrontLeft"))[0], 1, "layout front left unity");
close(stereoDownmixGains(byName(layout, "FrontRight"))[1], 1, "layout front right unity");
close(stereoDownmixGains(byName(layout, "Center"))[0], side, "layout center −3 dB");
close(stereoDownmixGains(byName(layout, "SurroundLeft"))[0], side, "layout surround −3 dB");
close(stereoDownmixGains(byName(layout, "RearRight"))[1], side, "layout rear −3 dB");
close(stereoDownmixGains(byName(layout, "LFE"))[0], lfe, "layout LFE −10 dB");
const wide = LAYOUTS["9.1.4"];
close(stereoDownmixGains(byName(wide, "WideLeft"))[0], side, "wide −3 dB");
close(stereoDownmixGains(byName(wide, "TopFrontLeft"))[0], side, "top front −3 dB");

console.log("stereo downmix tests: OK");
