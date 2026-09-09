#!/usr/bin/env node
/** Build the DT 1990 PRO Balanced average-measurement FIR from AutoEq's published PEQ. */
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

const sampleRate = 48000;
const taps = 8192;
const outputPath = resolve("apps/web/public/headphone-compensation/beyerdynamic-dt-1990-balanced-average-autoeq/average.f32");
// AutoEq Rtings HMS II.3, revision 7ae0f56d53074872b028649617a22bbb4232feb7.
const filters = [
 ["lowshelf",105,.7,4.2],["peaking",8280,.97,-8.7],["peaking",4605,2.41,5.3],
 ["peaking",1524,.45,3.4],["peaking",172,.59,-2.4],["highshelf",10000,.7,-4.5],
 ["peaking",9279,2.61,2.1],["peaking",2170,4.69,1.1],["peaking",3046,2.46,-.7],["peaking",4133,6,.7],
];

function biquad(type, frequency, q, gainDb) {
  const a = 10 ** (gainDb / 40);
  const w = (2 * Math.PI * frequency) / sampleRate;
  const cos = Math.cos(w);
  const sin = Math.sin(w);
  const alpha = sin / (2 * q);
  const beta = 2 * Math.sqrt(a) * alpha;
  let b0; let b1; let b2; let a0; let a1; let a2;
  if (type === "peaking") {
    b0 = 1 + alpha * a; b1 = -2 * cos; b2 = 1 - alpha * a;
    a0 = 1 + alpha / a; a1 = -2 * cos; a2 = 1 - alpha / a;
  } else if (type === "lowshelf") {
    b0 = a * ((a + 1) - (a - 1) * cos + beta);
    b1 = 2 * a * ((a - 1) - (a + 1) * cos);
    b2 = a * ((a + 1) - (a - 1) * cos - beta);
    a0 = (a + 1) + (a - 1) * cos + beta;
    a1 = -2 * ((a - 1) + (a + 1) * cos);
    a2 = (a + 1) + (a - 1) * cos - beta;
  } else {
    b0 = a * ((a + 1) + (a - 1) * cos + beta);
    b1 = -2 * a * ((a - 1) + (a + 1) * cos);
    b2 = a * ((a + 1) + (a - 1) * cos - beta);
    a0 = (a + 1) - (a - 1) * cos + beta;
    a1 = 2 * ((a - 1) - (a + 1) * cos);
    a2 = (a + 1) - (a - 1) * cos - beta;
  }
  return { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, x1: 0, x2: 0, y1: 0, y2: 0 };
}

const stages = filters.map(([type, frequency, q, gainDb]) => biquad(type, frequency, q, gainDb));
const responseAt = (frequency) => stages.reduce((product, s) => {
  const w = (2 * Math.PI * frequency) / sampleRate;
  const z1 = { re: Math.cos(w), im: -Math.sin(w) };
  const z2 = { re: Math.cos(2 * w), im: -Math.sin(2 * w) };
  const numerator = { re: s.b0 + s.b1 * z1.re + s.b2 * z2.re, im: s.b1 * z1.im + s.b2 * z2.im };
  const denominator = { re: 1 + s.a1 * z1.re + s.a2 * z2.re, im: s.a1 * z1.im + s.a2 * z2.im };
  const magnitude = Math.hypot(numerator.re, numerator.im) / Math.hypot(denominator.re, denominator.im);
  return product * magnitude;
}, 1);
const referenceScale = 1 / responseAt(1000);
const fir = new Float32Array(taps);
for (let i = 0; i < taps; i++) {
  let value = i === 0 ? 1 : 0;
  for (const stage of stages) {
    const input = value;
    value = stage.b0 * input + stage.b1 * stage.x1 + stage.b2 * stage.x2 - stage.a1 * stage.y1 - stage.a2 * stage.y2;
    stage.x2 = stage.x1; stage.x1 = input; stage.y2 = stage.y1; stage.y1 = value;
  }
  fir[i] = value * referenceScale;
}
if (![...fir].every(Number.isFinite)) throw new Error("FIR synthesis produced non-finite taps");
mkdirSync(dirname(outputPath), { recursive: true });
const bytes = Buffer.from(fir.buffer);
writeFileSync(outputPath, bytes);
console.log(JSON.stringify({ outputPath, sampleRate, taps, sha256: createHash("sha256").update(bytes).digest("hex"), referenceGainDb: 20 * Math.log10(responseAt(1000) * referenceScale) }, null, 2));


const sha256=createHash("sha256").update(bytes).digest("hex");
let peak=0;
for(let i=0;i<=65536;i++)peak=Math.max(peak,responseAt(i*24000/65536)*referenceScale);
const preampDb=-Math.ceil((20*Math.log10(peak)+0.2)*10)/10;
const manifest={
 schemaVersion:1,id:"beyerdynamic-dt-1990-balanced-average-autoeq",
 name:"拜亚动力 DT 1990 PRO（一代 · Balanced 耳垫）",
 source:"AutoEq Rtings HMS II.3 over-ear, revision 7ae0f56d53074872b028649617a22bbb4232feb7",
 target:"AutoEq over-ear target; 1 kHz normalized",
 measurementMode:"average-dual-mono",
 channelClaim:"同一平均测量 EQ 应用于 L/R；非独立 L/R 校准，不修正个体声道差异。",
 averageMeasurement:"https://github.com/jaakkopasanen/AutoEq/tree/7ae0f56d53074872b028649617a22bbb4232feb7/results/Rtings/HMS%20II.3%20over-ear/Beyerdynamic%20DT%201990%20(balanced%20earpads)",
 derivation:"scripts/build-beyerdynamic-dt-1990-balanced-average-profile.mjs; published PEQ synthesized at 48 kHz, 8192 taps, 1 kHz normalized; source -4.1 dB preamp excluded; headroom recomputed.",
 createdAt:"2026-09-09T00:00:00.000Z",deviceRevision:"DT 1990 PRO first generation",
 playbackState:"Passive wired over-ear",earTips:"Balanced earpads, not Analytical",firmware:"Not applicable",
 measurementRig:"Rtings HMS II.3",referenceBand:"1 kHz",sampleRate,preampDb,
 leftFir:{fileName:"average.f32",tapCount:taps,sha256},rightFir:{fileName:"average.f32",tapCount:taps,sha256}
};
writeFileSync(resolve(dirname(outputPath),"profile.json"),JSON.stringify(manifest,null,2)+"\n");
console.log(JSON.stringify({preampDb,sha256}));
