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
assert.ok(Math.abs(output.channels[0][0] - 0.8) < 1e-6, "front left retains left programme");
assert.ok(Math.abs(output.channels[1][1] + 0.8) < 1e-6, "front right retains right programme");
assert.ok(Math.abs(output.channels[2][0] - 0.5) < 1e-6, "centre is derived quietly from mono");
assert.equal(output.channels[4][0], 0, "mono material does not fill surrounds");
assert.ok(output.channels[4][1] > 0 && output.channels[5][1] < 0, "stereo difference reaches surrounds");
assert.ok(Math.abs(output.channels[4][2]) < Math.abs(output.channels[4][1]), "surrounds return to zero with the authored stereo difference");
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

const constantSide={...input,channels:[new Float32Array(48000).fill(.2),new Float32Array(48000).fill(-.2)],samplePos:0};
const bass=new AlacStereoUpmixer().upmix(constantSide);
assert(Math.abs(bass.channels[4][47999]-.2*.34)<1e-6,"authored side bass must not be high-pass removed");
const live=new AlacStereoUpmixer();
const block=(at)=>({...input,samplePos:at,channels:[new Float32Array(128).fill(.1),new Float32Array(128).fill(-.1)]});
const dry=live.liveSda(block(0),false);assert.deepEqual(dry.channels[0],block(0).channels[0]);assert(dry.channels.slice(2).every(c=>c.every(v=>v===0)));
const first=live.liveSda(block(128),true);assert(Math.abs(first.channels[0][0]-.1)<.001,"toggle must retain a short crossfade");
let last;for(let i=2;i<24;i++)last=live.liveSda(block(i*128),true);
assert(Math.abs(last.channels[0][127]-.1*.8*.7943282)<1e-6,"native upmix uses the mobile fixed -2 dB calibration once");
assert(Math.abs(last.channels[4][127]-.1*.34*.7943282)<1e-6,"rear direction and relative gain match mobile");
console.log("Mobile stereo matrix, preserved side bass, fixed native trim and warm dry/wet transition passed");
