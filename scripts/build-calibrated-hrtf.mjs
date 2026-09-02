#!/usr/bin/env node
/** Build level-, arrival-, and room-response-calibrated KU100 assets into staging. */
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { analyzeStereoImpulse, median } from "./lib/impulse-metrics.mjs";
import { collectIrs, nearestImpulse } from "./lib/hrtf-source.mjs";

const DRY_TAPS = 512;
const WET_TAPS = 8192;
const COMMON_ARRIVAL_SAMPLE = 128;
const DIRECT_WINDOW_MS = 4;
const REFERENCE_MINIMUM_HZ = 500;
const REFERENCE_MAXIMUM_HZ = 2000;
const ROOM_FRACTION = 3;
const ROOM_MINIMUM_HZ = 125;
const ROOM_MAXIMUM_HZ = 16000;
const ROOM_MAX_GAIN_DB = 3;
const ROOM_FIR_TAPS = 257;
const ROOM_GATE_START_MS = 2;
const ROOM_GATE_END_MS = 4;
/** Only the derived BRIR room residual is low-cut; dry HRIR and runtime LFE stay untouched. */
const ROOM_RESIDUAL_HIGHPASS_HZ = 150;
const ROOM_RESIDUAL_HIGHPASS_ORDER = 4;
const ROOM_RESIDUAL_HIGHPASS_Q = Math.SQRT1_2;
const ROOM_RESIDUAL_BASS_BAND_HZ = [20, 120];
const ROOM_DECORRELATION_VERSION = "sda-ku100-tail-ap-v2";
const ROOM_DECORRELATION_MINIMUM_HZ = 80;
const ROOM_DECORRELATION_MAXIMUM_HZ = 16000;
/** Four sections preserve C80 of the high-passed residual while separating reused BRIR variants. */
const ROOM_DECORRELATION_SECTIONS = 4;
const ROOM_DECORRELATION_MAX_ENERGY_TRIM_DB = 0.25;
const DEFAULT_MAX_SPEAKER_LEVEL_GAIN_DB = 3;
/** v3 双侧对称化：KU100 头模左右耳/测量摆放的反对称偏差会让同一对象在 +θ 与 -θ
 *  经 VBAP 相干叠加后同侧耳能量差最高达 4.4dB（±80°），听感就是 7.1.x/9.1.x
 *  左右不平衡。物理上正前方的头对 ±θ 应有镜像响应，所以每个镜像对按
 *  相关最优的公共符号 + 公共整数位移对齐后取平均，再镜像给两侧使用。
 *  只用一个左右共用符号/位移/标量，不单独动任何一只耳朵，ITD/ILD 的
 *  方向性结构保留（每方向仍有自己的 ITD/ILD——镜像对均值）。 */
const SYMMETRY_VERSION = "sda-ku100-bilateral-v1";
const SYMMETRY_WINDOW = [COMMON_ARRIVAL_SAMPLE, COMMON_ARRIVAL_SAMPLE + 192];
const SYMMETRY_MAX_SHIFT_SAMPLES = 8;
const SYMMETRY_CENTER_MAX_SHIFT_SAMPLES = 4;

const args = process.argv.slice(2);
function option(name, fallback = null) {
  const index = args.indexOf(`--${name}`);
  return index >= 0 ? args[index + 1] : fallback;
}

const manifestPath = resolve(option("manifest", "apps/web/public/hrtf/hrtf-set.json"));
const archivePath = resolve(option("archive", "tmp/sadie-source/D1.zip"));
const maxSpeakerLevelGainDb = Number(option("max-speaker-level-gain-db", String(DEFAULT_MAX_SPEAKER_LEVEL_GAIN_DB)));
if (!Number.isFinite(maxSpeakerLevelGainDb) || maxSpeakerLevelGainDb <= 0 || maxSpeakerLevelGainDb > 12) {
  throw new Error("max-speaker-level-gain-db 必须在 (0, 12] dB");
}
const outputDirectory = resolve(option("out", "tmp/hrtf-calibrated"));
const sourceManifest = JSON.parse(readFileSync(manifestPath, "utf8"));
if (sourceManifest.schemaVersion !== 2) throw new Error("校准构建要求schema v2 provenance manifest");
const sourceAssetDirectory = dirname(manifestPath);
if (![0, 3].includes(sourceManifest.calibrationVersion ?? 0)) {
  throw new Error("v4校准要求schema v2且具有原始测量 provenance 的资产基线");
}
if (!Array.isArray(sourceManifest.positions) || sourceManifest.positions.length < 2) {
  throw new Error("校准构建要求至少两个具有 provenance 的方向");
}
const calibrationBaseline = sourceManifest.calibrationVersion === 3 ? "v3" : "raw-provenance";

const archiveBytes = readFileSync(archivePath);
const archiveSha256 = createHash("sha256").update(archiveBytes).digest("hex");
if (archiveSha256 !== sourceManifest.source.archiveSha256) throw new Error("原始档案SHA-256与manifest不匹配");

const [dryCollection, wetCollection] = await Promise.all([
  collectIrs(archivePath, sourceManifest.source.hrPath),
  collectIrs(archivePath, sourceManifest.source.brPath),
]);
const dryByPath = new Map(dryCollection.impulses.map((impulse) => [impulse.sourcePath, impulse]));
const wetByPath = new Map(wetCollection.impulses.map((impulse) => [impulse.sourcePath, impulse]));
const sampleRate = sourceManifest.sampleRate;
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const gainFromDb = (db) => 10 ** (db / 20);
const clamp = (value, minimum, maximum) => Math.max(minimum, Math.min(maximum, value));

function analyze(impulse, kind) {
  return analyzeStereoImpulse(impulse.left, impulse.right, impulse.sampleRate ?? sampleRate, {
    onsetThresholdDb: kind === "dry" ? -30 : -24,
    onsetHoldSamples: 1,
    onsetSearchSamples: Math.round(sampleRate * 0.03),
    directWindowMs: DIRECT_WINDOW_MS,
    earlyWindowMs: 50,
    lateStartMs: 50,
    directFftSize: 4096,
    fullFftSize: kind === "dry" ? 4096 : 16384,
    fraction: ROOM_FRACTION,
    referenceMinimumHz: REFERENCE_MINIMUM_HZ,
    referenceMaximumHz: REFERENCE_MAXIMUM_HZ,
  });
}

function alignStereo(left, right, commonOnset, length) {
  const shift = Math.round(COMMON_ARRIVAL_SAMPLE - commonOnset);
  const outputLeft = new Float64Array(length);
  const outputRight = new Float64Array(length);
  for (let index = 0; index < length; index++) {
    const sourceIndex = index - shift;
    outputLeft[index] = left[sourceIndex] ?? 0;
    outputRight[index] = right[sourceIndex] ?? 0;
  }
  return { left: outputLeft, right: outputRight, shift };
}

function scaleStereo(stereo, gain) {
  for (let index = 0; index < stereo.left.length; index++) {
    stereo.left[index] *= gain;
    stereo.right[index] *= gain;
  }
  return stereo;
}

function convolve(signal, filter) {
  const output = new Float64Array(signal.length + filter.length - 1);
  for (let sample = 0; sample < signal.length; sample++) {
    const value = signal[sample];
    if (value === 0) continue;
    for (let tap = 0; tap < filter.length; tap++) output[sample + tap] += value * filter[tap];
  }
  return output;
}

function interpolateCorrectionDb(bands, frequency) {
  if (frequency <= ROOM_MINIMUM_HZ || frequency >= ROOM_MAXIMUM_HZ) return 0;
  const active = bands.filter((band) => band.centerHz >= ROOM_MINIMUM_HZ && band.centerHz <= ROOM_MAXIMUM_HZ);
  if (frequency <= active[0].centerHz) {
    const blend = Math.log(frequency / ROOM_MINIMUM_HZ) / Math.log(active[0].centerHz / ROOM_MINIMUM_HZ);
    return active[0].correctionDb * clamp(blend, 0, 1);
  }
  if (frequency >= active.at(-1).centerHz) {
    const blend = Math.log(ROOM_MAXIMUM_HZ / frequency) / Math.log(ROOM_MAXIMUM_HZ / active.at(-1).centerHz);
    return active.at(-1).correctionDb * clamp(blend, 0, 1);
  }
  for (let index = 1; index < active.length; index++) {
    if (frequency > active[index].centerHz) continue;
    const lower = active[index - 1];
    const upper = active[index];
    const blend = Math.log(frequency / lower.centerHz) / Math.log(upper.centerHz / lower.centerHz);
    return lower.correctionDb + (upper.correctionDb - lower.correctionDb) * blend;
  }
  return 0;
}

function designLinearPhaseCorrection(bands) {
  const fftSize = 2048;
  const magnitudes = new Float64Array(fftSize / 2 + 1);
  for (let bin = 0; bin < magnitudes.length; bin++) {
    const frequency = bin * sampleRate / fftSize;
    magnitudes[bin] = gainFromDb(interpolateCorrectionDb(bands, frequency));
  }

  const center = (ROOM_FIR_TAPS - 1) / 2;
  const filter = new Float64Array(ROOM_FIR_TAPS);
  for (let tap = 0; tap < filter.length; tap++) {
    const time = tap - center;
    let value = magnitudes[0] + magnitudes.at(-1) * Math.cos(Math.PI * time);
    for (let bin = 1; bin < magnitudes.length - 1; bin++) {
      value += 2 * magnitudes[bin] * Math.cos(2 * Math.PI * bin * time / fftSize);
    }
    const window = 0.5 - 0.5 * Math.cos(2 * Math.PI * tap / (filter.length - 1));
    filter[tap] = value * window / fftSize;
  }
  const dc = filter.reduce((sum, value) => sum + value, 0);
  if (dc !== 0) for (let index = 0; index < filter.length; index++) filter[index] /= dc;
  return filter;
}

function roomTail(stereo) {
  const start = COMMON_ARRIVAL_SAMPLE + Math.round(ROOM_GATE_START_MS * sampleRate / 1000);
  const end = COMMON_ARRIVAL_SAMPLE + Math.round(ROOM_GATE_END_MS * sampleRate / 1000);
  for (let index = 0; index < stereo.left.length; index++) {
    let gain = 1;
    if (index <= start) gain = 0;
    else if (index < end) gain = 0.5 - 0.5 * Math.cos(Math.PI * (index - start) / (end - start));
    stereo.left[index] *= gain;
    stereo.right[index] *= gain;
  }
  return stereo;
}

/** RBJ Butterworth-Q high-pass biquad. Two identical linked sections form LR4. */
function highPassBiquad(signal, cutoffHz) {
  const w0 = 2 * Math.PI * cutoffHz / sampleRate;
  const cosine = Math.cos(w0);
  const alpha = Math.sin(w0) / (2 * ROOM_RESIDUAL_HIGHPASS_Q);
  const b0 = (1 + cosine) / 2;
  const b1 = -(1 + cosine);
  const b2 = b0;
  const a0 = 1 + alpha;
  const a1 = -2 * cosine;
  const a2 = 1 - alpha;
  const output = new Float64Array(signal.length);
  let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
  for (let index = 0; index < signal.length; index++) {
    const x0 = signal[index];
    const y0 = (b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) / a0;
    output[index] = y0;
    x2 = x1;
    x1 = x0;
    y2 = y1;
    y1 = y0;
  }
  return output;
}

function highPassRoomResidual(stereo) {
  return {
    left: highPassBiquad(highPassBiquad(stereo.left, ROOM_RESIDUAL_HIGHPASS_HZ), ROOM_RESIDUAL_HIGHPASS_HZ),
    right: highPassBiquad(highPassBiquad(stereo.right, ROOM_RESIDUAL_HIGHPASS_HZ), ROOM_RESIDUAL_HIGHPASS_HZ),
  };
}

function stereoEnergy(stereo) {
  let total = 0;
  for (let index = 0; index < stereo.left.length; index++) {
    total += stereo.left[index] ** 2 + stereo.right[index] ** 2;
  }
  return total;
}

function stereoRangeEnergy(stereo, start, end) {
  let total = 0;
  const first = Math.max(0, Math.trunc(start));
  const last = Math.min(stereo.left.length, Math.trunc(end));
  for (let index = first; index < last; index++) {
    total += stereo.left[index] ** 2 + stereo.right[index] ** 2;
  }
  return total;
}

function stereoBandEnergy(stereo, minimumHz, maximumHz) {
  let total = 0;
  for (let frequency = minimumHz; frequency <= maximumHz; frequency += 5) {
    const phase = 2 * Math.PI * frequency / sampleRate;
    for (const channel of [stereo.left, stereo.right]) {
      let real = 0;
      let imaginary = 0;
      for (let index = 0; index < channel.length; index++) {
        real += channel[index] * Math.cos(phase * index);
        imaginary -= channel[index] * Math.sin(phase * index);
      }
      total += real ** 2 + imaginary ** 2;
    }
  }
  return total;
}

function energyDb(energy) {
  return energy > 0 ? 10 * Math.log10(energy) : Number.NEGATIVE_INFINITY;
}

function directEnergyCentroid(stereo) {
  const start = COMMON_ARRIVAL_SAMPLE;
  const end = Math.min(stereo.left.length, start + Math.round(DIRECT_WINDOW_MS * sampleRate / 1000));
  let weighted = 0;
  let total = 0;
  for (const channel of [stereo.left, stereo.right]) {
    for (let index = start; index < end; index++) {
      const power = channel[index] ** 2;
      weighted += index * power;
      total += power;
    }
  }
  return weighted / total;
}

function shiftStereo(stereo, amount) {
  const output = {
    left: new Float64Array(stereo.left.length),
    right: new Float64Array(stereo.right.length),
  };
  for (const [source, target] of [[stereo.left, output.left], [stereo.right, output.right]]) {
    for (let index = 0; index < source.length; index++) target[index] = source[index - amount] ?? 0;
  }
  return output;
}

function xorshift32(seed) {
  let state = seed >>> 0;
  return () => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) / 0x100000000;
  };
}

function allPassBiquad(signal, centerHz, radius) {
  const cosine = Math.cos(2 * Math.PI * centerHz / sampleRate);
  const a1 = -2 * radius * cosine;
  const a2 = radius * radius;
  const b0 = a2;
  const b1 = a1;
  const b2 = 1;
  const output = new Float64Array(signal.length);
  let x1 = 0;
  let x2 = 0;
  let y1 = 0;
  let y2 = 0;
  for (let index = 0; index < signal.length; index++) {
    const x0 = signal[index];
    const y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
    output[index] = y0;
    x2 = x1;
    x1 = x0;
    y2 = y1;
    y1 = y0;
  }
  return output;
}

function decorrelationParameters(position) {
  const key = `${position.azimuth}/${position.elevation}`;
  const digest = createHash("sha256")
    .update(`${ROOM_DECORRELATION_VERSION}\0${archiveSha256}\0${key}`)
    .digest();
  const random = xorshift32(digest.readUInt32LE(0));
  const logarithmicRange = Math.log(ROOM_DECORRELATION_MAXIMUM_HZ / ROOM_DECORRELATION_MINIMUM_HZ);
  const sections = Array.from({ length: ROOM_DECORRELATION_SECTIONS }, (_, index) => {
    const frequencyPosition = (index + 0.15 + 0.7 * random()) / ROOM_DECORRELATION_SECTIONS;
    return {
      centerHz: ROOM_DECORRELATION_MINIMUM_HZ * Math.exp(logarithmicRange * frequencyPosition),
      radius: 0.4 + 0.3 * random(),
    };
  });
  return { key, digestSha256: digest.toString("hex"), sections };
}

function decorrelateRoomTail(stereo, position) {
  const parameters = decorrelationParameters(position);
  let filteredLeft = stereo.left;
  let filteredRight = stereo.right;
  for (const section of parameters.sections) {
    filteredLeft = allPassBiquad(filteredLeft, section.centerHz, section.radius);
    filteredRight = allPassBiquad(filteredRight, section.centerHz, section.radius);
  }

  const output = { left: filteredLeft, right: filteredRight };
  const targetEnergy = stereoEnergy(stereo);
  const outputEnergy = stereoEnergy(output);
  const energyTrimDb = 10 * Math.log10(targetEnergy / outputEnergy);
  if (Math.abs(energyTrimDb) > ROOM_DECORRELATION_MAX_ENERGY_TRIM_DB) {
    throw new Error(
      `room tail去相关能量恢复越界 ${position.azimuth}/${position.elevation}: ${energyTrimDb.toFixed(3)}dB`,
    );
  }
  scaleStereo(output, gainFromDb(energyTrimDb));
  return {
    stereo: output,
    provenance: {
      algorithm: ROOM_DECORRELATION_VERSION,
      ...parameters,
      commonLeftRightFilter: true,
      energyTrimDb,
    },
  };
}

function combineWet(dry, tail) {
  const output = { left: Float64Array.from(tail.left), right: Float64Array.from(tail.right) };
  for (let index = 0; index < dry.left.length; index++) {
    output.left[index] += dry.left[index];
    output.right[index] += dry.right[index];
  }
  return output;
}

function stereoBytes(stereo) {
  const output = new Float32Array(stereo.left.length * 2);
  output.set(stereo.left, 0);
  output.set(stereo.right, stereo.left.length);
  return Buffer.from(output.buffer, output.byteOffset, output.byteLength);
}

function readAssetStereo(fileName) {
  const bytes = readFileSync(resolve(sourceAssetDirectory, fileName));
  const samples = new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / Float32Array.BYTES_PER_ELEMENT);
  const perEar = samples.length >> 1;
  return {
    left: Float64Array.from(samples.subarray(0, perEar)),
    right: Float64Array.from(samples.subarray(perEar)),
  };
}

const rows = sourceManifest.positions.map((position) => {
  const baselineDry = readAssetStereo(position.dry);
  const baselineWet = readAssetStereo(position.wet);
  const baselineResidual = {
    left: Float64Array.from(baselineWet.left),
    right: Float64Array.from(baselineWet.right),
  };
  for (let index = 0; index < baselineDry.left.length; index++) {
    baselineResidual.left[index] -= baselineDry.left[index];
    baselineResidual.right[index] -= baselineDry.right[index];
  }
  const targetDry = dryByPath.get(position.measurement.dry.sourcePath);
  const wet = wetByPath.get(position.measurement.wet.sourcePath);
  if (!targetDry || !wet) throw new Error(`manifest源路径未命中: ${position.azimuth}/${position.elevation}`);
  const pairedDryMatch = nearestImpulse(dryCollection.impulses, wet.azimuth, wet.elevation);
  if (!pairedDryMatch || pairedDryMatch.distanceDegrees > 1e-3) {
    throw new Error(`BRIR方向缺同坐标HRIR: ${wet.azimuth}/${wet.elevation}`);
  }
  return {
    position,
    baselineResidualBassEnergyDb: energyDb(stereoBandEnergy(baselineResidual, ...ROOM_RESIDUAL_BASS_BAND_HZ)),
    baselineResidualMidEnergyDb: energyDb(stereoBandEnergy(baselineResidual, 250, 4000)),
    targetDry,
    wet,
    pairedDry: pairedDryMatch.impulse,
    targetDryAnalysis: analyze(targetDry, "dry"),
    wetAnalysis: analyze(wet, "wet"),
    pairedDryAnalysis: analyze(pairedDryMatch.impulse, "dry"),
  };
});

const directEnergyValuesDb = rows.map((row) => energyDb(stereoEnergy({
  left: row.targetDry.left,
  right: row.targetDry.right,
})));
const targetDirectEnergyDbUnbounded = median(directEnergyValuesDb);
const directEnergyMinimumDb = Math.min(...directEnergyValuesDb);
const directEnergyMaximumDb = Math.max(...directEnergyValuesDb);
const targetDirectEnergyDb = clamp(
  targetDirectEnergyDbUnbounded,
  directEnergyMaximumDb - maxSpeakerLevelGainDb,
  directEnergyMinimumDb + maxSpeakerLevelGainDb,
);
const roomBandTargets = rows[0].wetAnalysis.fullBands.map((band, bandIndex) => {
  const ratioDb = median(rows.map((row) => (
    row.wetAnalysis.fullBands[bandIndex].powerDb - row.pairedDryAnalysis.fullBands[bandIndex].powerDb
  )));
  return { centerHz: band.centerHz, ratioDb: Number.isFinite(ratioDb) ? ratioDb : null };
});

for (const row of rows) {
  row.roomCorrectionBands = row.wetAnalysis.fullBands.map((band, bandIndex) => {
    const measuredRatioDb = band.powerDb - row.pairedDryAnalysis.fullBands[bandIndex].powerDb;
    const targetRatioDb = roomBandTargets[bandIndex].ratioDb;
    const inCorrectionRange = band.centerHz >= ROOM_MINIMUM_HZ && band.centerHz <= ROOM_MAXIMUM_HZ;
    const correctionDb = inCorrectionRange && Number.isFinite(measuredRatioDb) && Number.isFinite(targetRatioDb)
      ? clamp(targetRatioDb - measuredRatioDb, -ROOM_MAX_GAIN_DB, ROOM_MAX_GAIN_DB)
      : 0;
    return {
      centerHz: band.centerHz,
      measuredRatioDb: Number.isFinite(measuredRatioDb) ? measuredRatioDb : null,
      targetRatioDb: Number.isFinite(targetRatioDb) ? targetRatioDb : null,
      correctionDb,
    };
  });
}

rmSync(outputDirectory, { recursive: true, force: true });
mkdirSync(outputDirectory, { recursive: true });
const positions = [];
const wetTotalGains = [];
const dryTotalGains = [];
const roomSourceCanonical = new Map();
for (const row of rows) {
  const sourcePath = row.position.measurement.wet.sourcePath;
  if (!roomSourceCanonical.has(sourcePath)) {
    roomSourceCanonical.set(sourcePath, {
      azimuth: row.position.azimuth,
      elevation: row.position.elevation,
    });
  }
}

function stereoSymmetryScore(a, b, sign, shift, perEarLen) {
  const [from, to] = SYMMETRY_WINDOW;
  let dot = 0, energyA = 0, energyB = 0;
  for (const offset of [0, perEarLen]) {
    for (let index = from; index < to; index++) {
      const va = a[offset + index] ?? 0;
      const vb = sign * (b[offset + index - shift] ?? 0);
      dot += va * vb;
      energyA += va * va;
      energyB += vb * vb;
    }
  }
  return dot / Math.sqrt(energyA * energyB + 1e-18);
}

function mirrorStereo(data, perEarLen) {
  const output = new Float64Array(data.length);
  output.set(data.subarray(perEarLen), 0);
  output.set(data.subarray(0, perEarLen), perEarLen);
  return output;
}

/** 镜像对对齐平均：B=mirror(minus)，搜公共 sign/shift 使直达窗相关最大，
 *  avg=0.5·(plus + s·shift(B))，-θ 侧用 mirror(avg)。 */
function symmetrizePair(plus, minus, perEarLen, maxShift) {
  const mirroredMinus = mirrorStereo(minus, perEarLen);
  let best = { score: -2, sign: 1, shift: 0 };
  for (const sign of [1, -1]) {
    for (let shift = -maxShift; shift <= maxShift; shift++) {
      const score = stereoSymmetryScore(plus, mirroredMinus, sign, shift, perEarLen);
      if (score > best.score) best = { score, sign, shift };
    }
  }
  const avg = new Float64Array(plus.length);
  for (let index = 0; index < plus.length; index++) {
    avg[index] = 0.5 * ((plus[index] ?? 0) + best.sign * (mirroredMinus[index - best.shift] ?? 0));
  }
  return { avg, mirrored: mirrorStereo(avg, perEarLen), ...best };
}

/** 正中方向：双耳都给左右平均 IR。头朝正前时两耳物理上应收到相同响应，
 *  实测偏差是头模/摆放不对称——是人声结像倾斜的直接来源。 */
function symmetrizeCenter(data, perEarLen, maxShift) {
  const mirrored = mirrorStereo(data, perEarLen);
  let best = { score: -2, sign: 1, shift: 0 };
  for (const sign of [1, -1]) {
    for (let shift = -maxShift; shift <= maxShift; shift++) {
      const score = stereoSymmetryScore(data, mirrored, sign, shift, perEarLen);
      if (score > best.score) best = { score, sign, shift };
    }
  }
  const output = new Float64Array(data.length);
  for (let index = 0; index < perEarLen; index++) {
    output[index] = 0.5 * ((data[index] ?? 0) + best.sign * (mirrored[index - best.shift] ?? 0));
  }
  output.set(output.subarray(0, perEarLen), perEarLen);
  return { avg: output, ...best };
}

/** 对称化后把每方向 dry/wet 分别公共缩放回对称化前的立体声能量，
 *  保持 v2 逐音箱电平校准不变（公共标量，不动 ILD/ITD）。 */
function rescaleToEnergy(stereo, targetEnergy) {
  const current = stereoEnergy(stereo);
  const trimDb = energyDb(targetEnergy) - energyDb(current);
  scaleStereo(stereo, gainFromDb(trimDb));
  return trimDb;
}

const earlyRoomStartSample = COMMON_ARRIVAL_SAMPLE + Math.round(ROOM_GATE_END_MS * sampleRate / 1000);
const earlyRoomEndSample = COMMON_ARRIVAL_SAMPLE + Math.round(50 * sampleRate / 1000);
const prepared = rows.map((row) => {
  let dryAligned = alignStereo(
    row.targetDry.left,
    row.targetDry.right,
    row.targetDryAnalysis.onset.commonSample,
    DRY_TAPS,
  );
  const dryBeforeGain = analyze({ ...dryAligned, sampleRate }, "dry");
  const dryEnergyDbBeforeGain = energyDb(stereoEnergy(dryAligned));
  const dryGainDb = targetDirectEnergyDb - dryEnergyDbBeforeGain;
  if (Math.abs(dryGainDb) > maxSpeakerLevelGainDb + 1e-9) {
    throw new Error(`宽带直达校准超过±${maxSpeakerLevelGainDb}dB ${row.position.azimuth}/${row.position.elevation}: ${dryGainDb.toFixed(2)}dB`);
  }
  scaleStereo(dryAligned, gainFromDb(dryGainDb));

  const correctionFir = designLinearPhaseCorrection(row.roomCorrectionBands);
  const filteredWet = {
    left: convolve(row.wet.left, correctionFir),
    right: convolve(row.wet.right, correctionFir),
    sampleRate,
  };
  const filteredWetAnalysis = analyze(filteredWet, "wet");
  const wetAligned = alignStereo(
    filteredWet.left,
    filteredWet.right,
    filteredWetAnalysis.onset.commonSample,
    WET_TAPS,
  );
  // Isolate wet - dry before the gate. The LR4 is never applied to the target dry HRIR.
  const residual = {
    left: Float64Array.from(wetAligned.left),
    right: Float64Array.from(wetAligned.right),
  };
  for (let index = 0; index < dryAligned.left.length; index++) {
    residual.left[index] -= dryAligned.left[index];
    residual.right[index] -= dryAligned.right[index];
  }
  let tail = highPassRoomResidual(roomTail(residual));
  return {
    row,
    dryAligned,
    dryCoarseShiftSamples: dryAligned.shift,
    dryBeforeGain,
    dryEnergyDbBeforeGain,
    dryGainDb,
    filteredWetAnalysis,
    wetAligned,
    wetCoarseShiftSamples: wetAligned.shift,
    tail,
    directEnergyCentroidSample: directEnergyCentroid(dryAligned),
  };
});
const targetDirectEnergyCentroidSample = median(prepared.map((entry) => entry.directEnergyCentroidSample));
for (const entry of prepared) {
  entry.energyCentroidShiftSamples = Math.round(
    targetDirectEnergyCentroidSample - entry.directEnergyCentroidSample,
  );
  entry.dryAligned = shiftStereo(entry.dryAligned, entry.energyCentroidShiftSamples);
  entry.tail = shiftStereo(entry.tail, entry.energyCentroidShiftSamples);
  entry.directEnergyCentroidOutputSample = directEnergyCentroid(entry.dryAligned);
  entry.roomEarlyEnergyDb = energyDb(stereoRangeEnergy(
    entry.tail,
    earlyRoomStartSample,
    earlyRoomEndSample,
  ));
}
const targetRoomEarlyEnergyDb = median(prepared.map((entry) => entry.roomEarlyEnergyDb));

for (const entry of prepared) {
  const { dryAligned, tail } = entry;
  const { row, dryGainDb } = entry;
  const roomGainDb = targetRoomEarlyEnergyDb - entry.roomEarlyEnergyDb;
  if (Math.abs(roomGainDb) > maxSpeakerLevelGainDb + 1e-9) {
    throw new Error(`房间residual校准超过±${maxSpeakerLevelGainDb}dB ${row.position.azimuth}/${row.position.elevation}: ${roomGainDb.toFixed(2)}dB`);
  }
  scaleStereo(tail, gainFromDb(roomGainDb));
  const sourcePath = row.position.measurement.wet.sourcePath;
  const canonical = roomSourceCanonical.get(sourcePath);
  let roomTailOutput = tail;
  let roomTailDecorrelation = {
    role: "canonical",
    canonicalTarget: canonical,
    algorithm: null,
  };
  if (canonical.azimuth !== row.position.azimuth || canonical.elevation !== row.position.elevation) {
    const decorrelated = decorrelateRoomTail(tail, row.position);
    roomTailOutput = decorrelated.stereo;
    roomTailDecorrelation = {
      role: "variant",
      canonicalTarget: canonical,
      ...decorrelated.provenance,
    };
  }
  entry.baselineWetAnalysis = analyze({ ...combineWet(dryAligned, tail), sampleRate }, "wet");
  entry.preSymmetryWet = combineWet(dryAligned, roomTailOutput);
  entry.finalDry = dryAligned;
  entry.finalTail = roomTailOutput;
  entry.tailEnergyBefore = stereoEnergy(roomTailOutput);
  entry.roomGainDb = roomGainDb;
  entry.roomTailDecorrelation = roomTailDecorrelation;
  dryTotalGains.push(dryGainDb);
  wetTotalGains.push(roomGainDb);
}

// ---- v3 双侧对称化（在 v2 电平/TOF/房间校准全部完成后进行） ----
// 设计约束：
// 1) dry 与房间尾声共用同一组 sign/shift（取自 dry 直达窗的相关搜索）。
// 2) dry 与尾声分别电平恢复：尾声是扩散场，镜像对尾声互不相关，直接平均会
//    损失约 3dB 房间能量——尾声单独缩放回对称化前能量，dry 缩放回 dry 能量，
//    各自保持 v2 校准目标；尾声在房间门前为零，wet 直达前缀仍与 dry 逐样本一致。
// 3) 对称化后按既有 energyCentroidTof 机制再做一次双耳公共整数 shift（dry/尾声
//    同一 shift），把直达能量质心重新对齐到共同目标，TOF 离散回到 ≤1 sample。
function interleave(stereo) {
  const output = new Float64Array(stereo.left.length * 2);
  output.set(stereo.left, 0);
  output.set(stereo.right, stereo.left.length);
  return output;
}
function deinterleave(data) {
  const perEarLen = data.length >> 1;
  return {
    left: Float64Array.from(data.subarray(0, perEarLen)),
    right: Float64Array.from(data.subarray(perEarLen)),
  };
}
function symmetrizedPairOutputs(plus, minus, sign, shift, perEarLen) {
  const mirroredMinus = mirrorStereo(interleave(minus), perEarLen);
  const base = interleave(plus);
  const avg = new Float64Array(base.length);
  for (let index = 0; index < base.length; index++) {
    avg[index] = 0.5 * ((base[index] ?? 0) + sign * (mirroredMinus[index - shift] ?? 0));
  }
  return { plus: deinterleave(avg), minus: deinterleave(mirrorStereo(avg, perEarLen)) };
}
const finalByKey = new Map(prepared.map((entry) => [`${entry.row.position.azimuth}/${entry.row.position.elevation}`, entry]));
const symmetryByKey = new Map();
// Calibrate every actual ±azimuth pair in the target manifest. Dense targets can
// include one-sided/reused BRIR directions; those must retain their calibrated
// original direction rather than being forced into a non-existent mirror pair.
const symmetryPairs = [...finalByKey.values()]
  .map((entry) => [entry.row.position.azimuth, entry.row.position.elevation])
  .filter(([azimuth]) => azimuth > 0)
  .filter(([azimuth, elevation]) => finalByKey.has(`-${azimuth}/${elevation}`));
for (const [azimuth, elevation] of symmetryPairs) {
  const plus = finalByKey.get(`${azimuth}/${elevation}`);
  const minus = finalByKey.get(`-${azimuth}/${elevation}`);
  if (!plus || !minus) continue;
  const perEarLen = plus.finalDry.left.length;
  const alignment = symmetrizePair(interleave(plus.finalDry), interleave(minus.finalDry), perEarLen, SYMMETRY_MAX_SHIFT_SAMPLES);
  const dryOutputs = symmetrizedPairOutputs(plus.finalDry, minus.finalDry, alignment.sign, alignment.shift, perEarLen);
  const tailOutputs = symmetrizedPairOutputs(plus.finalTail, minus.finalTail, alignment.sign, alignment.shift, plus.finalTail.left.length);
  // 镜像对共用电平恢复：两侧同一增益（几何均值目标），保证输出资产逐耳精确镜像。
  // dry 用全长能量；尾声用 4-50ms 房间窗能量（v2 房间 residual 校准的同一窗口）。
  const dryPairTarget = Math.sqrt(stereoEnergy(plus.finalDry) * stereoEnergy(minus.finalDry));
  const tailWindow = [earlyRoomStartSample, earlyRoomEndSample];
  const tailPairTarget = Math.sqrt(
    stereoRangeEnergy(plus.finalTail, ...tailWindow) * stereoRangeEnergy(minus.finalTail, ...tailWindow),
  );
  for (const [entry, dryOutput, tailOutput, role, partner] of [
    [plus, dryOutputs.plus, tailOutputs.plus, "pair-average", minus],
    [minus, dryOutputs.minus, tailOutputs.minus, "pair-average-mirrored", plus],
  ]) {
    entry.finalDry = dryOutput;
    entry.finalTail = tailOutput;
    const trimDb = rescaleToEnergy(entry.finalDry, dryPairTarget);
    const tailTrimDb = (() => {
      const current = stereoRangeEnergy(entry.finalTail, ...tailWindow);
      const trim = energyDb(tailPairTarget) - energyDb(current);
      scaleStereo(entry.finalTail, gainFromDb(trim));
      return trim;
    })();
    symmetryByKey.set(`${entry.row.position.azimuth}/${entry.row.position.elevation}`, {
      role,
      pairWith: `${partner.row.position.azimuth}/${elevation}`,
      sign: alignment.sign,
      shiftSamples: alignment.shift,
      directWindowCorrelation: alignment.score,
      energyTrimDb: trimDb,
      roomTailTrimDb: tailTrimDb,
    });
  }
}
{
  const center = finalByKey.get("0/0");
  if (!center) throw new Error("缺正中方向 0/0");
  const perEarLen = center.finalDry.left.length;
  const alignment = symmetrizeCenter(interleave(center.finalDry), perEarLen, SYMMETRY_CENTER_MAX_SHIFT_SAMPLES);
  const centerWith = (stereo, earLen) => {
    const data = interleave(stereo);
    const mirrored = mirrorStereo(data, earLen);
    const output = new Float64Array(data.length);
    for (let index = 0; index < earLen; index++) {
      output[index] = 0.5 * ((data[index] ?? 0) + alignment.sign * (mirrored[index - alignment.shift] ?? 0));
    }
    output.set(output.subarray(0, earLen), earLen);
    return deinterleave(output);
  };
  const dryEnergyBefore = stereoEnergy(center.finalDry);
  const tailWindowEnergyBefore = stereoRangeEnergy(center.finalTail, earlyRoomStartSample, earlyRoomEndSample);
  center.finalDry = centerWith(center.finalDry, perEarLen);
  center.finalTail = centerWith(center.finalTail, center.finalTail.left.length);
  const trimDb = rescaleToEnergy(center.finalDry, dryEnergyBefore);
  const tailTrimDb = (() => {
    const current = stereoRangeEnergy(center.finalTail, earlyRoomStartSample, earlyRoomEndSample);
    const trim = energyDb(tailWindowEnergyBefore) - energyDb(current);
    scaleStereo(center.finalTail, gainFromDb(trim));
    return trim;
  })();
  symmetryByKey.set("0/0", {
    role: "center-average",
    sign: alignment.sign,
    shiftSamples: alignment.shift,
    directWindowCorrelation: alignment.score,
    energyTrimDb: trimDb,
    roomTailTrimDb: tailTrimDb,
  });
}
// 对称化后重新对齐直达能量质心（双耳公共、dry/尾声同一整数 shift）。
// 镜像方向的 v2 shift 可不同；若各自按边界硬门控会破坏数值镜像。每个镜像对使用
// 两者中较晚的边界：仍满足各自 wet 直达前缀契约，同时保持两侧尾声逐样本镜像。
for (const entry of prepared) {
  const centroid = directEnergyCentroid(entry.finalDry);
  entry.postSymmetryShiftSamples = Math.round(targetDirectEnergyCentroidSample - centroid);
  entry.finalDry = shiftStereo(entry.finalDry, entry.postSymmetryShiftSamples);
  entry.finalTail = shiftStereo(entry.finalTail, entry.postSymmetryShiftSamples);
  entry.gateBoundary = COMMON_ARRIVAL_SAMPLE
    + Math.round(ROOM_GATE_START_MS * sampleRate / 1000)
    + entry.energyCentroidShiftSamples
    + entry.postSymmetryShiftSamples;
}
function hardGateTail(stereo, boundary) {
  for (let index = 0; index <= boundary && index < stereo.left.length; index++) {
    stereo.left[index] = 0;
    stereo.right[index] = 0;
  }
}
for (const [azimuth, elevation] of symmetryPairs) {
  const plus = finalByKey.get(`${azimuth}/${elevation}`);
  const minus = finalByKey.get(`-${azimuth}/${elevation}`);
  if (!plus || !minus) continue;
  const sharedBoundary = Math.max(plus.gateBoundary, minus.gateBoundary);
  hardGateTail(plus.finalTail, sharedBoundary);
  hardGateTail(minus.finalTail, sharedBoundary);
}
hardGateTail(finalByKey.get("0/0").finalTail, finalByKey.get("0/0").gateBoundary);

rmSync(outputDirectory, { recursive: true, force: true });
mkdirSync(outputDirectory, { recursive: true });
if (process.env.SDA_DEBUG_SYMMETRY) {
  for (const entry of prepared) {
    const tail = entry.finalTail;
    let first = -1;
    for (let index = 0; index < tail.left.length; index++) {
      if (tail.left[index] !== 0 || tail.right[index] !== 0) { first = index; break; }
    }
    console.log(`tail-onset ${entry.row.position.azimuth}/${entry.row.position.elevation}: ${first} postShift=${entry.postSymmetryShiftSamples}`);
  }
}
for (const entry of prepared) {
  const { row, dryBeforeGain, dryEnergyDbBeforeGain, dryGainDb, filteredWetAnalysis } = entry;
  const roomGainDb = entry.roomGainDb;
  const roomTailDecorrelation = entry.roomTailDecorrelation;
  const baselineWetAnalysis = entry.baselineWetAnalysis;
  const dryAligned = entry.finalDry;
  const wetOutput = combineWet(entry.finalDry, entry.finalTail);
  const residualBands = {
    baselineV3BassEnergyDb: row.baselineResidualBassEnergyDb,
    outputBassEnergyDb: energyDb(stereoBandEnergy(entry.finalTail, ...ROOM_RESIDUAL_BASS_BAND_HZ)),
    baselineV3MidEnergyDb: row.baselineResidualMidEnergyDb,
    outputMidEnergyDb: energyDb(stereoBandEnergy(entry.finalTail, 250, 4000)),
  };
  residualBands.bassReductionDb = residualBands.outputBassEnergyDb - residualBands.baselineV3BassEnergyDb;
  residualBands.midDifferenceDb = residualBands.outputMidEnergyDb - residualBands.baselineV3MidEnergyDb;
  const preSymmetryWetAnalysis = analyze({ ...entry.preSymmetryWet, sampleRate }, "wet");
  const dryBytes = stereoBytes(dryAligned);
  const wetBytes = stereoBytes(wetOutput);
  writeFileSync(resolve(outputDirectory, row.position.dry), dryBytes);
  writeFileSync(resolve(outputDirectory, row.position.wet), wetBytes);

  const dryOutputAnalysis = analyze({ ...dryAligned, sampleRate }, "dry");
  const wetOutputAnalysis = analyze({ ...wetOutput, sampleRate }, "wet");
  positions.push({
    ...row.position,
    measurement: {
      ...row.position.measurement,
      roomReferenceDry: {
        sourcePath: row.pairedDry.sourcePath,
        azimuth: row.pairedDry.azimuth,
        elevation: row.pairedDry.elevation,
        sourceDistanceMeters: sourceManifest.source.hrirMeasurement.radiusMeters,
        monitor: sourceManifest.source.hrirMeasurement.monitor,
      },
    },
    processing: {
      dry: {
        sourceOnset: row.targetDryAnalysis.onset,
        coarseAlignmentShiftSamples: entry.dryCoarseShiftSamples,
        energyCentroidTof: {
          beforeSample: entry.directEnergyCentroidSample,
          targetSample: targetDirectEnergyCentroidSample,
          commonShiftSamples: entry.energyCentroidShiftSamples + entry.postSymmetryShiftSamples,
          v2ShiftSamples: entry.energyCentroidShiftSamples,
          postSymmetryShiftSamples: entry.postSymmetryShiftSamples,
          afterSample: directEnergyCentroid(dryAligned),
        },
        commonDelaySamples: entry.dryCoarseShiftSamples + entry.energyCentroidShiftSamples + entry.postSymmetryShiftSamples,
        calibrationGainDb: dryGainDb,
        fullHrirEnergyDbBeforeGain: dryEnergyDbBeforeGain,
        fullHrirEnergyTargetDb: targetDirectEnergyDb,
        fullHrirEnergyDbOutput: energyDb(stereoEnergy(dryAligned)),
        outputOnset: dryOutputAnalysis.onset,
      },
      wet: {
        sourceOnset: row.wetAnalysis.onset,
        filteredOnset: filteredWetAnalysis.onset,
        coarseAlignmentShiftSamples: entry.wetCoarseShiftSamples,
        energyCentroidTof: {
          targetSample: targetDirectEnergyCentroidSample,
          commonShiftSamples: entry.energyCentroidShiftSamples + entry.postSymmetryShiftSamples,
        },
        commonDelaySamples: entry.wetCoarseShiftSamples + entry.energyCentroidShiftSamples + entry.postSymmetryShiftSamples,
        calibrationGainDb: roomGainDb,
        roomEarlyEnergyDbBeforeGain: entry.roomEarlyEnergyDb,
        roomEarlyEnergyTargetDb: targetRoomEarlyEnergyDb,
        residualBands,
        roomCorrectionBands: row.roomCorrectionBands,
        directPathSource: row.position.measurement.dry.sourcePath,
        roomTailSource: row.position.measurement.wet.sourcePath,
        roomTailDecorrelation: {
          ...roomTailDecorrelation,
          baselineMetrics: {
            c50Db: baselineWetAnalysis.windows.c50Db,
            c80Db: baselineWetAnalysis.windows.c80Db,
            directToLateDb: baselineWetAnalysis.windows.directToLateDb,
            totalEnergyDb: baselineWetAnalysis.windows.totalEnergyDb,
          },
          outputMetrics: {
            c50Db: preSymmetryWetAnalysis.windows.c50Db,
            c80Db: preSymmetryWetAnalysis.windows.c80Db,
            directToLateDb: preSymmetryWetAnalysis.windows.directToLateDb,
            totalEnergyDb: preSymmetryWetAnalysis.windows.totalEnergyDb,
          },
        },
        outputOnset: wetOutputAnalysis.onset,
      },
    },
    assets: {
      dry: { tapCountPerEar: DRY_TAPS, sha256: sha256(dryBytes) },
      wet: { tapCountPerEar: WET_TAPS, sha256: sha256(wetBytes) },
    },
    symmetry: symmetryByKey.get(`${row.position.azimuth}/${row.position.elevation}`),
  });
}

const dryGlobalGainDb = median(dryTotalGains);
const wetGlobalGainDb = median(wetTotalGains);
for (let index = 0; index < positions.length; index++) {
  positions[index].processing.dry.globalGainDb = dryGlobalGainDb;
  positions[index].processing.dry.speakerLevelTrimDb = dryTotalGains[index] - dryGlobalGainDb;
  positions[index].processing.wet.globalGainDb = wetGlobalGainDb;
  positions[index].processing.wet.speakerLevelTrimDb = wetTotalGains[index] - wetGlobalGainDb;
}

const manifest = {
  ...sourceManifest,
  calibrationVersion: 4,
  processing: {
    dryTapLimit: DRY_TAPS,
    wetTapLimit: WET_TAPS,
    peakNormalized: false,
    calibrated: true,
    runtimeEnergyNormalization: false,
    directPathModel: "target HRIR plus calibrated BRIR room tail",
    note: "One KU100 room/listening position. Per-speaker common arrival, direct reference level, low-resolution room-response correction, an offline 150Hz LR4 high-pass only on the derived BRIR room residual, deterministic decorrelation only for reused BRIR room tails, and bilateral mirror-pair symmetrization (common sign/shift/scalar only); no layout- or programme-specific EQ.",
  },
  calibration: {
    algorithm: "sda-ku100-room-v4",
    baseline: calibrationBaseline,
    sampleRate,
    commonArrivalSample: COMMON_ARRIVAL_SAMPLE,
    bilateralSymmetry: {
      version: SYMMETRY_VERSION,
      scope: "every mirror pair ±az/el plus the 0/0 centre direction",
      windowSamples: SYMMETRY_WINDOW,
      maxShiftSamples: SYMMETRY_MAX_SHIFT_SAMPLES,
      centerMaxShiftSamples: SYMMETRY_CENTER_MAX_SHIFT_SAMPLES,
      commonLeftRightSignShift: true,
      energyPreservedPerDirection: true,
      rationale: "KU100 head/placement asymmetry made correlated VBAP speaker pairs sum unevenly between sides (up to 4.4dB at ±80°). Mirror-pair averaging with correlation-optimal common sign/shift restores the physical bilateral symmetry of a frontal head; each direction keeps its own ITD/ILD (the pair mean).",
    },
    energyCentroidTof: {
      metric: "stereo direct-energy time centroid",
      targetSample: targetDirectEnergyCentroidSample,
      windowStartSample: COMMON_ARRIVAL_SAMPLE,
      windowMs: DIRECT_WINDOW_MS,
      maximumOutputSpreadSamples: 1,
      commonLeftRightShift: true,
      commonDryRoomResidualShift: true,
    },
    directReference: {
      metric: "stereo full-anechoic-HRIR energy",
      targetEnergyDb: targetDirectEnergyDb,
      minimumHz: 20,
      maximumHz: sampleRate / 2,
      windowMs: null,
    },
    roomResidualReference: {
      metric: "stereo 4-50ms gated room-residual energy",
      targetEnergyDb: targetRoomEarlyEnergyDb,
      startMs: ROOM_GATE_END_MS,
      endMs: 50,
      maximumGainDb: 3,
    },
    roomCorrection: {
      fraction: ROOM_FRACTION,
      minimumHz: ROOM_MINIMUM_HZ,
      maximumHz: ROOM_MAXIMUM_HZ,
      maximumGainDb: ROOM_MAX_GAIN_DB,
      firTaps: ROOM_FIR_TAPS,
      phase: "linear",
      commonLeftRightFilter: true,
      target: "robust median BRIR/paired-HRIR room transfer across all virtual speakers",
      bands: roomBandTargets,
    },
    roomTailGate: { startMs: ROOM_GATE_START_MS, endMs: ROOM_GATE_END_MS },
    roomResidualHighPass: {
      algorithm: "sda-ku100-room-residual-lr4-v1",
      cutoffHz: ROOM_RESIDUAL_HIGHPASS_HZ,
      order: ROOM_RESIDUAL_HIGHPASS_ORDER,
      sectionQ: ROOM_RESIDUAL_HIGHPASS_Q,
      commonLeftRightFilter: true,
      applicationOrder: "wet minus aligned dry, room-tail gate, LR4 high-pass, 4-50ms residual calibration",
      scope: "offline derived BRIR room residual only",
      excludes: ["dry HRIR", "runtime LFE", "final headphone EQ", "physical multichannel output"],
      acceptanceBandHz: ROOM_RESIDUAL_BASS_BAND_HZ,
    },
    roomTailDecorrelation: {
      algorithm: ROOM_DECORRELATION_VERSION,
      scope: "only non-canonical virtual speakers that reuse the same measured BRIR source",
      sections: ROOM_DECORRELATION_SECTIONS,
      minimumHz: ROOM_DECORRELATION_MINIMUM_HZ,
      maximumHz: ROOM_DECORRELATION_MAXIMUM_HZ,
      maximumEnergyTrimDb: ROOM_DECORRELATION_MAX_ENERGY_TRIM_DB,
      commonLeftRightFilter: true,
    },
    level: { dryGlobalGainDb, wetGlobalGainDb },
  },
  positions,
};
writeFileSync(resolve(outputDirectory, "hrtf-set.json"), `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`校准staging完成: ${positions.length}方向 -> ${outputDirectory}`);
console.log(`共同到达=${COMMON_ARRIVAL_SAMPLE} samples, 宽带直达=${targetDirectEnergyDb.toFixed(2)} dB`);
console.log(`直达能量质心TOF=${targetDirectEnergyCentroidSample.toFixed(3)} samples`);
console.log(`房间residual 4-50ms=${targetRoomEarlyEnergyDb.toFixed(2)} dB`);
console.log(`dry全局=${dryGlobalGainDb.toFixed(2)} dB, wet全局=${wetGlobalGainDb.toFixed(2)} dB`);
