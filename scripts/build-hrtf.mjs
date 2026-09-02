#!/usr/bin/env node
/**
 * Convert official SADIE II KU100 HRIR/BRIR WAV data to SDA runtime assets.
 * Build into staging first; publishing is a separate validated operation.
 */

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { collectIrs, nearestImpulse } from "./lib/hrtf-source.mjs";

const TARGETS = [
  [0, 0], [30, 0], [-30, 0], [60, 0], [-60, 0],
  [100, 0], [-100, 0], [110, 0], [-110, 0], [140, 0], [-140, 0],
  [45, 45], [-45, 45], [90, 45], [-90, 45], [135, 45], [-135, 45],
];
/** Dense object-render sphere: fine azimuth ring + elevated rows + zenith,
 *  for the opt-in precise-object binaural path (VBAP snaps to a fine grid). */
const TARGETS_DENSE = [
  ...Array.from({ length: 36 }, (_, i) => [i * 10 - 180, 0]),
  ...Array.from({ length: 12 }, (_, i) => [i * 30 - 180, 45]),
  ...Array.from({ length: 12 }, (_, i) => [i * 30 - 180, -30]),
  [0, 90],
];
const DRY_TAPS = 512;
const WET_TAPS = 8192;
const SADIE_RECORD_URL = "https://zenodo.org/records/12092466";
const SADIE_DOI = "10.5281/zenodo.12092466";

const args = process.argv.slice(2);
function option(name, fallback = null) {
  const index = args.indexOf(`--${name}`);
  return index >= 0 ? args[index + 1] : fallback;
}
const HR_SOURCE = option("hr");
const BR_SOURCE = option("br");
const HR_PATH = option("hr-path", "48K_24bit");
const BR_PATH = option("br-path", "48K_24bit");
const OUTPUT_DIRECTORY = resolve(option("out", "apps/web/public/hrtf"));
const SOURCE_ARCHIVE_URL = option("source-url", null);
const SOURCE_ARCHIVE_MD5 = option("source-md5", null);
const SOURCE_ARCHIVE_SHA256 = option("source-sha256", null);
const FLIP_AZIMUTH = args.includes("--flip-az");
const DENSE = args.includes("--dense");

if (!HR_SOURCE || !BR_SOURCE) {
  console.error("用法: node scripts/build-hrtf.mjs --hr <HRIR zip/目录/URL> --br <BRIR zip/目录/URL> [--out 目录]");
  console.error("SADIE II D1: --hr D1.zip --br D1.zip --hr-path D1_HRIR_WAV/48K_24bit --br-path D1_BRIR_WAV/48K_24bit");
  process.exit(1);
}

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

function sourceHash(source) {
  if (/^https?:\/\//.test(source)) return null;
  try {
    return sha256(readFileSync(resolve(source)));
  } catch {
    return null;
  }
}

function trimImpulse(left, right, taps, preSamples) {
  let peak = 0;
  let peakIndex = 0;
  const searchLength = Math.min(left.length, taps + 4096);
  for (let index = 0; index < searchLength; index++) {
    const value = Math.max(Math.abs(left[index]), Math.abs(right[index]));
    if (value > peak) {
      peak = value;
      peakIndex = index;
    }
  }
  const start = Math.max(0, peakIndex - preSamples);
  const length = Math.min(taps, left.length - start);
  const output = new Float32Array(length * 2);
  output.set(left.subarray(start, start + length), 0);
  output.set(right.subarray(start, start + length), length);
  let maximum = 0;
  for (const value of output) maximum = Math.max(maximum, Math.abs(value));
  const normalizationGain = maximum > 0 ? 1 / maximum : 1;
  for (let index = 0; index < output.length; index++) output[index] *= normalizationGain;
  return { output, start, peakIndex, peak, normalizationGain };
}

function fileName(azimuth, elevation, kind) {
  return `az${String(azimuth).replace("-", "m")}_el${String(elevation).replace("-", "m")}_${kind}.f32`;
}

const [dryCollection, wetCollection] = await Promise.all([
  collectIrs(HR_SOURCE, HR_PATH),
  collectIrs(BR_SOURCE, BR_PATH),
]);
const sampleRate = dryCollection.impulses[0].sampleRate;
if (wetCollection.impulses[0].sampleRate !== sampleRate) {
  throw new Error(`HRIR ${sampleRate}Hz 与 BRIR ${wetCollection.impulses[0].sampleRate}Hz 不一致`);
}
mkdirSync(OUTPUT_DIRECTORY, { recursive: true });

const positions = [];
const TARGET_LIST = DENSE ? TARGETS_DENSE : TARGETS;
for (const [azimuth, elevation] of TARGET_LIST) {
  const sourceAzimuth = ((FLIP_AZIMUTH ? -azimuth : azimuth) + 360) % 360;
  const dryMatch = nearestImpulse(dryCollection.impulses, sourceAzimuth, elevation);
  const wetMatch = nearestImpulse(wetCollection.impulses, sourceAzimuth, elevation);
  if (!dryMatch || !wetMatch) throw new Error(`az${azimuth} el${elevation} 缺测量点`);

  const dryTrim = trimImpulse(dryMatch.impulse.left, dryMatch.impulse.right, DRY_TAPS, 16);
  const wetTrim = trimImpulse(wetMatch.impulse.left, wetMatch.impulse.right, WET_TAPS, 32);
  const dryName = fileName(azimuth, elevation, "dry");
  const wetName = fileName(azimuth, elevation, "wet");
  const dryBytes = Buffer.from(dryTrim.output.buffer, dryTrim.output.byteOffset, dryTrim.output.byteLength);
  const wetBytes = Buffer.from(wetTrim.output.buffer, wetTrim.output.byteOffset, wetTrim.output.byteLength);
  writeFileSync(resolve(OUTPUT_DIRECTORY, dryName), dryBytes);
  writeFileSync(resolve(OUTPUT_DIRECTORY, wetName), wetBytes);

  positions.push({
    azimuth,
    elevation,
    dry: dryName,
    wet: wetName,
    measurement: {
      dry: {
        sourcePath: dryMatch.impulse.sourcePath,
        azimuth: dryMatch.impulse.azimuth,
        elevation: dryMatch.impulse.elevation,
        angularErrorDegrees: dryMatch.distanceDegrees,
        sourceDistanceMeters: 1.2,
        monitor: "Genelec 8010",
        originalFrames: dryMatch.impulse.left.length,
        originalBitsPerSample: dryMatch.impulse.bitsPerSample,
      },
      wet: {
        sourcePath: wetMatch.impulse.sourcePath,
        azimuth: wetMatch.impulse.azimuth,
        elevation: wetMatch.impulse.elevation,
        angularErrorDegrees: wetMatch.distanceDegrees,
        sourceDistanceMeters: 1.5,
        monitor: "Genelec 8030/40",
        originalFrames: wetMatch.impulse.left.length,
        originalBitsPerSample: wetMatch.impulse.bitsPerSample,
      },
      flipAzimuth: FLIP_AZIMUTH,
    },
    processing: {
      dry: {
        trimStartSample: dryTrim.start,
        peakSample: dryTrim.peakIndex,
        originalPeak: dryTrim.peak,
        peakNormalizationGain: dryTrim.normalizationGain,
      },
      wet: {
        trimStartSample: wetTrim.start,
        peakSample: wetTrim.peakIndex,
        originalPeak: wetTrim.peak,
        peakNormalizationGain: wetTrim.normalizationGain,
      },
    },
    assets: {
      dry: { tapCountPerEar: dryTrim.output.length / 2, sha256: sha256(dryBytes) },
      wet: { tapCountPerEar: wetTrim.output.length / 2, sha256: sha256(wetBytes) },
    },
  });
  console.log(
    `az${azimuth} el${elevation} dry←(${dryMatch.impulse.azimuth},${dryMatch.impulse.elevation}, ${dryMatch.distanceDegrees.toFixed(2)}°) ` +
    `wet←(${wetMatch.impulse.azimuth},${wetMatch.impulse.elevation}, ${wetMatch.distanceDegrees.toFixed(2)}°)`,
  );
}

const localSourceHash = HR_SOURCE === BR_SOURCE ? sourceHash(HR_SOURCE) : null;
const manifest = {
  schemaVersion: 2,
  calibrationVersion: 0,
  sampleRate,
  source: {
    name: "SADIE II Database V2.2, D1 KU100",
    doi: SADIE_DOI,
    recordUrl: SADIE_RECORD_URL,
    license: "Apache-2.0",
    archiveUrl: SOURCE_ARCHIVE_URL,
    archiveMd5: SOURCE_ARCHIVE_MD5,
    archiveSha256: SOURCE_ARCHIVE_SHA256 ?? localSourceHash,
    hrPath: HR_PATH,
    brPath: BR_PATH,
    hrirMeasurement: { monitor: "Genelec 8010", radiusMeters: 1.2 },
    brirMeasurement: { monitor: "Genelec 8030/40", radiusMeters: 1.5, grid: "50-point Lebedev" },
  },
  azimuthConvention: "target and source positive azimuth = left (counter-clockwise, ADM/ITU)",
  processing: {
    dryTapLimit: DRY_TAPS,
    wetTapLimit: WET_TAPS,
    peakNormalized: true,
    calibrated: false,
    note: "Provenance staging only; absolute SPL/TOF and direct-to-reverberant ratio are not calibrated.",
  },
  positions,
};
writeFileSync(resolve(OUTPUT_DIRECTORY, "hrtf-set.json"), `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`\n完成：${positions.length} 个方向 → ${OUTPUT_DIRECTORY}`);
