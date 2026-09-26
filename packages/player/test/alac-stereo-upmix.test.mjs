import assert from "node:assert/strict";
import { AlacStereoUpmixer, ALAC_STEREO_UPMIX_LABELS, isAlacStereoFrame } from "../src/alac-stereo-upmix.ts";

const input = {
  codec: "alac", sampleRate: 48_000, samplePos: 0,
  channels: [Float32Array.from([1, 1, -1]), Float32Array.from([1, -1, -1])],
  labels: ["L", "R"], rawBedLabels: ["L", "R"], events: [], objectChannels: [],
  programLoudness: null, rampDuration: 0,
};
assert.equal(isAlacStereoFrame(input), true);
const output = new AlacStereoUpmixer().upmix(input);
assert.deepEqual(output.labels, [...ALAC_STEREO_UPMIX_LABELS]);
assert.deepEqual(output.rawBedLabels, ["L", "R"]);
assert.equal(output.channels.length, 12);
assert.ok(Math.abs(output.channels[0][0] - 0.78) < 1e-6, "front left retains left programme");
assert.ok(Math.abs(output.channels[1][1] + 0.78) < 1e-6, "front right retains right programme");
assert.ok(Math.abs(output.channels[2][0] - 0.3) < 1e-6, "centre is derived quietly from mono");
assert.equal(output.channels[4][0], 0, "mono material does not fill surrounds");
assert.ok(output.channels[4][1] > 0 && output.channels[5][1] < 0, "stereo difference reaches surrounds");
assert.ok(Math.abs(output.channels[4][2]) < Math.abs(output.channels[4][1]), "steady side bass is filtered out of surrounds");
assert.ok(Math.abs(output.channels[3][2]) < 0.25, "LFE is low-pass and gain limited");
const signalEnergy = (channels) => channels.reduce(
  (total, channel) => total + channel.reduce((sum, sample) => sum + sample ** 2, 0),
  0,
);
const sourceEnergy = signalEnergy(input.channels);
const upmixedEnergy = signalEnergy(output.channels);
assert.ok(upmixedEnergy < sourceEnergy, "pseudo-upmix retains headroom instead of accumulating copied-channel energy");
assert.equal(isAlacStereoFrame({ ...input, codec: "eac3" }), false, "E-AC-3/Atmos is never transformed");
console.log("ALAC stereo pseudo-upmix: 7.1.4 labels, L/R preservation, centre, side routing, LFE filter and headroom passed");
