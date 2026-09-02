#!/usr/bin/env node
/**
 * Build a "KU100 head + swappable pinnae" hybrid HRTF set: below the LR4
 * crossover the KU100's calibrated head/torso response is kept; above it the
 * subject's pinna/ear-canal response is used. Both IRs are aligned on their
 * direct-path peak before the split, so the 4th-order Linkwitz-Riley sum is
 * phase-coherent and flat through the crossover.
 *
 * Usage: node scripts/build-hrtf-hybrid.mjs --pinna H3 [--out apps/web/public/hrtf-ku100-h3]
 */
import { readFileSync, readdirSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const args = process.argv.slice(2);
const option = (name, fallback = null) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const pinna = option("pinna");
if (!pinna) throw new Error("usage: build-hrtf-hybrid.mjs --pinna H3");
const PINNA_ID = pinna.toLowerCase();
const KU100_DIR = resolve("apps/web/public/hrtf");
const SUBJECT_DIR = resolve("apps/web/public", `hrtf-${PINNA_ID}`);
const OUT_DIR = resolve(option("out", `apps/web/public/hrtf-ku100-${PINNA_ID}`));
const SAMPLE_RATE = 48000;
/** Crossover where head/torso diffraction hands off to pinna resonance. */
const CROSSOVER_HZ = 2500;

function readIr(path) {
  const buf = readFileSync(path);
  return new Float32Array(buf.buffer, buf.byteOffset, Math.floor(buf.byteLength / 4));
}

/** RBJ 2nd-order Butterworth section (LR4 = two in cascade). */
function biquadCoefficients(type, fc, fs) {
  const w0 = (2 * Math.PI * fc) / fs;
  const cos0 = Math.cos(w0);
  const alpha = Math.sin(w0) / Math.SQRT2;
  const a0 = 1 + alpha;
  if (type === "low") {
    const b0 = (1 - cos0) / 2;
    return [b0 / a0, (1 - cos0) / a0, b0 / a0, (-2 * cos0) / a0, (1 - alpha) / a0];
  }
  const b0 = (1 + cos0) / 2;
  return [b0 / a0, -(1 + cos0) / a0, b0 / a0, (-2 * cos0) / a0, (1 - alpha) / a0];
}

function biquadProcess(x, c) {
  const [b0, b1, b2, a1, a2] = c;
  const out = new Float32Array(x.length);
  let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
  for (let i = 0; i < x.length; i++) {
    const x0 = x[i];
    const y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
    x2 = x1; x1 = x0; y2 = y1; y1 = y0;
    out[i] = y0;
  }
  return out;
}

/** 4th-order Linkwitz-Riley: two cascaded Butterworth sections, flat sum. */
function lr4(x, type, fc, fs) {
  const c = biquadCoefficients(type, fc, fs);
  return biquadProcess(biquadProcess(x, c), c);
}

/** Sample offset of the direct-path peak (largest absolute value). */
function directPeak(x) {
  let index = 0, peak = 0;
  for (let i = 0; i < x.length; i++) {
    const v = Math.abs(x[i]);
    if (v > peak) { peak = v; index = i; }
  }
  return index;
}

/** Shift x by `offset` samples (positive = later), keeping length. */
function shiftSamples(x, offset) {
  const out = new Float32Array(x.length);
  const clamped = Math.max(-x.length, Math.min(x.length, offset));
  if (clamped >= 0) out.set(x.subarray(0, x.length - clamped), clamped);
  else out.set(x.subarray(-clamped), 0);
  return out;
}

const manifest = JSON.parse(readFileSync(resolve(KU100_DIR, "hrtf-set.json"), "utf8"));
mkdirSync(OUT_DIR, { recursive: true });

for (const entry of manifest.positions) {
  for (const kind of ["dry", "wet"]) {
    const ku = readIr(resolve(KU100_DIR, entry[kind]));
    const subject = readIr(resolve(SUBJECT_DIR, entry[kind]));
    const half = ku.length >> 1;
    const out = new Float32Array(ku.length);
    for (const [kOff, oOff] of [[0, 0], [half, half]]) {
      const kuEar = ku.subarray(kOff, kOff + half);
      // Pad the subject to the KU100's length before aligning: sets differ in
      // dry IR length (calibrated KU100 is 512 taps, subjects may be shorter).
      const subjectRaw = subject.subarray(kOff, kOff + half);
      const subjectEar = new Float32Array(half);
      subjectEar.set(subjectRaw.subarray(0, Math.min(half, subjectRaw.length)));
      const shift = directPeak(kuEar) - directPeak(subjectEar);
      const subjectAligned = shiftSamples(subjectEar, shift);
      const low = lr4(kuEar, "low", CROSSOVER_HZ, SAMPLE_RATE);
      const high = lr4(subjectAligned, "high", CROSSOVER_HZ, SAMPLE_RATE);
      for (let i = 0; i < half; i++) out[oOff + i] = low[i] + high[i];
    }
    writeFileSync(resolve(OUT_DIR, entry[kind]), Buffer.from(out.buffer, out.byteOffset, out.byteLength));
  }
}

const hybridManifest = {
  ...manifest,
  source: { ...manifest.source, name: `SADIE II KU100 head + ${pinna} pinnae (hybrid, LR4 ${CROSSOVER_HZ} Hz)` },
  hybrid: { headBase: "ku100", pinna, crossoverHz: CROSSOVER_HZ },
};
writeFileSync(resolve(OUT_DIR, "hrtf-set.json"), JSON.stringify(hybridManifest, null, 2));
console.log(`[ku100+${pinna}] hybrid -> ${OUT_DIR}`);
