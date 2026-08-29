import assert from "node:assert/strict";
import { LoudnessMeter } from "../src/bs1770.ts";

const SAMPLE_RATE = 48000;
/** ITU anchor: a 997 Hz sine at 0 dBFS in ONE channel integrates to −3.01 LKFS
 *  (a dual-mono copy measures +3.01 dB above that, i.e. ~0 LKFS). */
const anchorLufs = -3.01;

function sine(amplitude, seconds, frequency = 997) {
  const n = Math.round(seconds * SAMPLE_RATE);
  const data = new Float32Array(n);
  for (let i = 0; i < n; i++) data[i] = amplitude * Math.sin((2 * Math.PI * frequency * i) / SAMPLE_RATE);
  return data;
}

function silence(seconds) {
  return new Float32Array(Math.round(seconds * SAMPLE_RATE));
}

function feed(meter, channels, chunkSamples = 1024) {
  for (let offset = 0; offset < channels[0].length; offset += chunkSamples) {
    meter.push(channels.map((c) => c.subarray(offset, Math.min(c.length, offset + chunkSamples))));
  }
}

const fullScale = new LoudnessMeter(SAMPLE_RATE, 2);
feed(fullScale, [sine(1, 5), silence(5)]);
const anchor = fullScale.integrated();
assert.ok(anchor.blocks >= 45, `expected ~47 blocks for 5 s of audio, got ${anchor.blocks}`);
assert.ok(
  Math.abs(anchor.integratedLufs - anchorLufs) < 0.5,
  `0 dBFS single-channel sine should anchor at ${anchorLufs} LKFS, got ${anchor.integratedLufs}`,
);

// Linearity: −20 dBFS amplitude must read 20 LU lower than the anchor.
const quiet = new LoudnessMeter(SAMPLE_RATE, 2);
feed(quiet, [sine(0.1, 5), silence(5)]);
assert.ok(
  Math.abs(quiet.integrated().integratedLufs - (anchorLufs - 20)) < 0.5,
  `−20 dBFS sine should read ${(anchorLufs - 20).toFixed(2)} LKFS, got ${quiet.integrated().integratedLufs}`,
);

// Relative gate: a −60 dBFS tail must not drag the integrated value down.
const gated = new LoudnessMeter(SAMPLE_RATE, 2);
feed(gated, [sine(0.1, 8), silence(8)]);
feed(gated, [sine(0.001, 8), silence(8)]);
const gatedResult = gated.integrated();
assert.ok(
  Math.abs(gatedResult.integratedLufs - (anchorLufs - 20)) < 0.5,
  `quiet tail should gate out, got ${gatedResult.integratedLufs}`,
);

// Silence never passes the absolute gate.
const silent = new LoudnessMeter(SAMPLE_RATE, 2);
feed(silent, [new Float32Array(SAMPLE_RATE * 2), new Float32Array(SAMPLE_RATE * 2)]);
assert.equal(silent.integrated().integratedLufs, null, "silence must not produce an integrated value");

// Odd-length frames and mono both flow through without index errors.
const mono = new LoudnessMeter(SAMPLE_RATE, 1);
feed(mono, [sine(0.1, 1.234)], 997);
assert.ok(mono.integrated().blocks > 8, "mono meter accumulates blocks");

console.log("bs1770 loudness tests: OK");
