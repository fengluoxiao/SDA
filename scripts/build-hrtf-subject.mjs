#!/usr/bin/env node
/**
 * Convert one SADIE II subject (D2 dummy head or H* human) to a packaged SDA
 * HRTF set: base conversion (build-hrtf.mjs logic is invoked via child script)
 * + broadband level match to the calibrated KU100 front reference, so A/B
 * compares tone, not loudness. Real human pinnae are left unsymmetrized.
 *
 * Usage: node scripts/build-hrtf-subject.mjs --subject H3
 */
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const args = process.argv.slice(2);
function option(name) {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : null;
}
const subject = option("subject");
if (!subject) throw new Error("usage: build-hrtf-subject.mjs --subject H3");

const archive = resolve("tmp/sadie-source", `${subject}.zip`);
const staging = resolve("tmp", `hrtf-${subject.toLowerCase()}`);
const outDir = resolve("apps/web/public", `hrtf-${subject.toLowerCase()}`);
const hrPath = `${subject}/${subject}_HRIR_WAV/48K_24bit`;
const brPath = `${subject}/${subject}_BRIR_WAV/48K_24bit`;

// 1. base conversion
execFileSync("node", [
  "scripts/build-hrtf.mjs",
  "--hr", archive,
  "--br", archive,
  "--hr-path", hrPath,
  "--br-path", brPath,
  "--out", staging,
], { stdio: "inherit" });

// 2. broadband level match to the calibrated KU100 front reference
function bandEnergy(path) {
  const buf = readFileSync(path);
  const a = new Float32Array(buf.buffer, buf.byteOffset, Math.floor(buf.byteLength / 4));
  let e = 0;
  for (const v of a) e += v * v;
  return Math.sqrt(e / a.length);
}
const reference = bandEnergy(resolve("apps/web/public/hrtf/az0_el0_dry.f32"));
const subjectEnergy = bandEnergy(resolve(staging, "az0_el0_dry.f32"));
const gainDb = 20 * Math.log10(reference / subjectEnergy);
const gain = Math.pow(10, gainDb / 20);

// 3. package with level match
mkdirSync(outDir, { recursive: true });
for (const f of readdirSync(staging)) {
  if (!f.endsWith(".f32")) continue;
  const a = new Float32Array(readFileSync(resolve(staging, f)).buffer.slice(0));
  const out = new Float32Array(a.length);
  for (let i = 0; i < a.length; i++) out[i] = a[i] * gain;
  writeFileSync(resolve(outDir, f), Buffer.from(out.buffer));
}
const manifest = JSON.parse(readFileSync(resolve(staging, "hrtf-set.json"), "utf8"));
manifest.source = { ...manifest.source, name: `SADIE II Database V2.2, ${subject}` };
manifest.levelMatchDb = Math.round(gainDb * 100) / 100;
writeFileSync(resolve(outDir, "hrtf-set.json"), JSON.stringify(manifest, null, 2));
console.log(`[${subject}] packaged -> ${outDir} (level match ${gainDb.toFixed(2)} dB)`);
