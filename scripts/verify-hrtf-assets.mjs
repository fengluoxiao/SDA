#!/usr/bin/env node
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { analyzeStereoImpulse } from "./lib/impulse-metrics.mjs";

const manifestPath = resolve(process.argv[2] ?? "apps/web/public/hrtf/hrtf-set.json");
if (!existsSync(manifestPath)) {
  console.error(`HRTF manifest 不存在: ${manifestPath}`);
  process.exit(1);
}
const root = dirname(manifestPath);
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const errors = [];
const expectedDirections = new Set([
  "0/0", "30/0", "-30/0", "60/0", "-60/0", "100/0", "-100/0", "110/0", "-110/0", "140/0", "-140/0",
  "45/45", "-45/45", "90/45", "-90/45", "135/45", "-135/45",
]);
const hex64 = /^[a-f0-9]{64}$/;
const calibrated = manifest.calibrationVersion >= 1 && manifest.processing?.calibrated === true;
const calibrationV2 = calibrated && manifest.calibrationVersion >= 2;
const calibrationV3 = calibrated && manifest.calibrationVersion >= 3;
const calibrationV4 = calibrated && manifest.calibrationVersion >= 4;

function stereoEnergyDb(samples, length, start = 0, end = length) {
  let energy = 0;
  for (let index = Math.max(0, start); index < Math.min(length, end); index++) {
    energy += samples[index] ** 2 + samples[length + index] ** 2;
  }
  return energy > 0 ? 10 * Math.log10(energy) : Number.NEGATIVE_INFINITY;
}

function directEnergyCentroid(samples, length, calibration) {
  const start = calibration.windowStartSample;
  const end = Math.min(length, start + Math.round(calibration.windowMs * manifest.sampleRate / 1000));
  let energy = 0;
  let weighted = 0;
  for (const offset of [0, length]) {
    for (let index = start; index < end; index++) {
      const power = samples[offset + index] ** 2;
      energy += power;
      weighted += index * power;
    }
  }
  return weighted / energy;
}

function roomResidualEnergyDb(dry, dryLength, wet, wetLength, start, end) {
  let energy = 0;
  for (let index = start; index < Math.min(wetLength, end); index++) {
    const dryLeft = index < dryLength ? dry[index] : 0;
    const dryRight = index < dryLength ? dry[dryLength + index] : 0;
    energy += (wet[index] - dryLeft) ** 2 + (wet[wetLength + index] - dryRight) ** 2;
  }
  return energy > 0 ? 10 * Math.log10(energy) : Number.NEGATIVE_INFINITY;
}

function roomResidualBandEnergyDb(dry, dryLength, wet, wetLength, minimumHz, maximumHz) {
  let energy = 0;
  for (let frequency = minimumHz; frequency <= maximumHz; frequency += 5) {
    const phase = 2 * Math.PI * frequency / manifest.sampleRate;
    for (const offset of [0, wetLength]) {
      let real = 0;
      let imaginary = 0;
      for (let index = 0; index < wetLength; index++) {
        const drySample = index < dryLength ? dry[(offset === 0 ? 0 : dryLength) + index] : 0;
        const sample = wet[offset + index] - drySample;
        real += sample * Math.cos(phase * index);
        imaginary -= sample * Math.sin(phase * index);
      }
      energy += real ** 2 + imaginary ** 2;
    }
  }
  return energy > 0 ? 10 * Math.log10(energy) : Number.NEGATIVE_INFINITY;
}

if (manifest.schemaVersion !== 2) errors.push("schemaVersion 必须为 2");
if (!Number.isInteger(manifest.calibrationVersion) || manifest.calibrationVersion < 0) errors.push("calibrationVersion 无效");
if (manifest.sampleRate !== 48000) errors.push("sampleRate 必须为 48000");
if (manifest.source?.doi !== "10.5281/zenodo.12092466") errors.push("SADIE II DOI 不匹配");
if (manifest.source?.license !== "Apache-2.0") errors.push("license 必须为 Apache-2.0");
if (!hex64.test(manifest.source?.archiveSha256 ?? "")) errors.push("archiveSha256 无效");
if (!/^[a-f0-9]{32}$/.test(manifest.source?.archiveMd5 ?? "")) errors.push("archiveMd5 无效");
if (manifest.source?.hrirMeasurement?.radiusMeters !== 1.2) errors.push("HRIR 测量半径必须记录为 1.2m");
if (manifest.source?.brirMeasurement?.radiusMeters !== 1.5) errors.push("BRIR 测量半径必须记录为 1.5m");
if (!Array.isArray(manifest.positions) || manifest.positions.length !== expectedDirections.size) {
  errors.push(`positions 应有 ${expectedDirections.size} 个方向`);
}
if (calibrated) {
  if (manifest.processing?.peakNormalized !== false) errors.push("校准资产不得峰值归一");
  if (manifest.processing?.runtimeEnergyNormalization !== false) errors.push("校准资产必须禁用运行时IR能量归一");
  if (!["sda-ku100-room-v1", "sda-ku100-room-v2", "sda-ku100-room-v3", "sda-ku100-room-v4"].includes(manifest.calibration?.algorithm)) {
    errors.push("校准算法版本无效");
  }
  if (!Number.isFinite(manifest.calibration?.commonArrivalSample)) errors.push("缺共同到达目标");
  const room = manifest.calibration?.roomCorrection;
  if (room?.commonLeftRightFilter !== true || room?.phase !== "linear") errors.push("房间校正必须使用左右共用线性相位FIR");
  if (!(room?.maximumGainDb > 0 && room.maximumGainDb <= 3)) errors.push("房间校正上限必须在0..3dB");
  if (calibrationV2) {
    const tof = manifest.calibration?.energyCentroidTof;
    if (tof?.metric !== "stereo direct-energy time centroid" || tof?.commonLeftRightShift !== true) {
      errors.push("v2必须记录左右共用直达能量质心TOF");
    }
    if (tof?.commonDryRoomResidualShift !== true || !Number.isFinite(tof?.targetSample)) {
      errors.push("v2必须对dry与room residual应用同一质心shift");
    }
    const direct = manifest.calibration?.directReference;
    if (direct?.metric !== "stereo full-anechoic-HRIR energy" || !Number.isFinite(direct?.targetEnergyDb)) {
      errors.push("v2缺full-HRIR直达能量目标");
    }
    const residual = manifest.calibration?.roomResidualReference;
    if (residual?.metric !== "stereo 4-50ms gated room-residual energy" || !Number.isFinite(residual?.targetEnergyDb)) {
      errors.push("v2缺4-50ms房间residual能量目标");
    }
    const decorrelation = manifest.calibration?.roomTailDecorrelation;
    if (decorrelation?.commonLeftRightFilter !== true || decorrelation?.scope?.includes("non-canonical") !== true) {
      errors.push("v2重复BRIR去相关范围/双耳链接无效");
    }
  }
  if (calibrationV3) {
    const symmetry = manifest.calibration?.bilateralSymmetry;
    if (symmetry?.commonLeftRightSignShift !== true || symmetry?.energyPreservedPerDirection !== true) {
      errors.push("v3双侧对称化必须只用一个左右共用sign/shift/标量并保持每方向能量");
    }
    if (!Array.isArray(symmetry?.windowSamples) || symmetry.windowSamples.length !== 2) {
      errors.push("v3缺对称化直达窗记录");
    }
    if (!(symmetry?.maxShiftSamples >= 0)) errors.push("v3缺对称化位移上限");
  }
  if (calibrationV4) {
    const highPass = manifest.calibration?.roomResidualHighPass;
    if (highPass?.algorithm !== "sda-ku100-room-residual-lr4-v1"
      || highPass?.cutoffHz !== 150
      || highPass?.order !== 4
      || highPass?.commonLeftRightFilter !== true) {
      errors.push("v4缺150Hz左右链接LR4 room residual高通记录");
    }
    if (highPass?.scope !== "offline derived BRIR room residual only"
      || highPass?.applicationOrder !== "wet minus aligned dry, room-tail gate, LR4 high-pass, 4-50ms residual calibration") {
      errors.push("v4 room residual高通范围或顺序无效");
    }
    const exclusions = highPass?.excludes;
    if (!Array.isArray(exclusions) || !["dry HRIR", "runtime LFE", "final headphone EQ", "physical multichannel output"].every((item) => exclusions.includes(item))) {
      errors.push("v4 room residual高通排除范围无效");
    }
  }
}

const seen = new Set();
const assetHashes = new Map();
const dryAnalyses = [];
const v2Outputs = [];
const symmetryEnergies = new Map();
for (const position of manifest.positions ?? []) {
  const key = `${position.azimuth}/${position.elevation}`;
  if (!expectedDirections.has(key)) errors.push(`非目标方向 ${key}`);
  if (seen.has(key)) errors.push(`重复方向 ${key}`);
  seen.add(key);
  const samplesByKind = {};

  for (const kind of ["dry", "wet"]) {
    const fileName = position[kind];
    const asset = position.assets?.[kind];
    const measurement = position.measurement?.[kind];
    const processing = position.processing?.[kind];
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*\.f32$/.test(fileName ?? "")) {
      errors.push(`${key}/${kind}: 文件名无效`);
      continue;
    }
    const filePath = resolve(root, fileName);
    if (!existsSync(filePath)) {
      errors.push(`${key}/${kind}: 缺少 ${fileName}`);
      continue;
    }
    const bytes = readFileSync(filePath);
    if (bytes.byteLength % (2 * Float32Array.BYTES_PER_ELEMENT) !== 0) errors.push(`${key}/${kind}: 不是等长双耳 f32`);
    const tapCountPerEar = bytes.byteLength / (2 * Float32Array.BYTES_PER_ELEMENT);
    if (asset?.tapCountPerEar !== tapCountPerEar) errors.push(`${key}/${kind}: tapCountPerEar 不匹配`);
    if (tapCountPerEar > manifest.processing?.[`${kind}TapLimit`]) errors.push(`${key}/${kind}: 超出tap上限`);
    const hash = createHash("sha256").update(bytes).digest("hex");
    if (asset?.sha256 !== hash) errors.push(`${key}/${kind}: SHA-256 不匹配`);
    const samples = new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / Float32Array.BYTES_PER_ELEMENT);
    if (![...samples].every(Number.isFinite)) errors.push(`${key}/${kind}: 含非有限样本`);
    samplesByKind[kind] = { samples, tapCountPerEar };
    if (!measurement?.sourcePath || !Number.isFinite(measurement.azimuth) || !Number.isFinite(measurement.elevation)) {
      errors.push(`${key}/${kind}: 缺少源测量路径/坐标`);
    }
    if (!Number.isFinite(measurement?.angularErrorDegrees) || measurement.angularErrorDegrees < 0) {
      errors.push(`${key}/${kind}: angularErrorDegrees 无效`);
    }
    if (kind === "dry" && measurement?.angularErrorDegrees > 1e-6) errors.push(`${key}/dry: HRIR 应精确命中目标方向`);

    if (calibrated) {
      if (!Number.isFinite(processing?.commonDelaySamples) || !Number.isFinite(processing?.calibrationGainDb)) {
        errors.push(`${key}/${kind}: 缺校准时间/电平provenance`);
      }
      const gainLimit = calibrationV2 ? processing?.calibrationGainDb : processing?.speakerLevelTrimDb;
      if (!Number.isFinite(gainLimit) || Math.abs(gainLimit) > 3) {
        errors.push(`${key}/${kind}: 逐音箱电平修正超过±3dB`);
      }
      const sourceItd = processing?.sourceOnset?.itdSamples;
      const outputItd = processing?.outputOnset?.itdSamples;
      if (!Number.isFinite(sourceItd) || !Number.isFinite(outputItd)) errors.push(`${key}/${kind}: 缺ITD provenance`);
      if (kind === "dry" && !calibrationV3 && Math.abs(sourceItd - outputItd) > 1) errors.push(`${key}/dry: ITD误差超过1 sample`);
      if (kind === "wet") {
        if (processing?.directPathSource !== position.measurement?.dry?.sourcePath) errors.push(`${key}/wet: 直达路径不是目标HRIR`);
        if (processing?.roomTailSource !== position.measurement?.wet?.sourcePath) errors.push(`${key}/wet: 房间尾声来源不匹配`);
        for (const band of processing?.roomCorrectionBands ?? []) {
          if (!Number.isFinite(band.correctionDb) || Math.abs(band.correctionDb) > manifest.calibration.roomCorrection.maximumGainDb + 1e-9) {
            errors.push(`${key}/wet: 房间频响修正越界`);
            break;
          }
        }
        if (calibrationV2) {
          const dryTof = position.processing?.dry?.energyCentroidTof;
          const wetTof = processing?.energyCentroidTof;
          if (!Number.isInteger(dryTof?.commonShiftSamples)
            || dryTof.commonShiftSamples !== wetTof?.commonShiftSamples) {
            errors.push(`${key}/wet: dry与room residual质心shift不一致`);
          }
          const decorrelation = processing?.roomTailDecorrelation;
          const variant = ["60/0", "-60/0"].includes(key);
          if (variant) {
            if (decorrelation?.role !== "variant"
              || decorrelation?.algorithm !== manifest.calibration.roomTailDecorrelation.algorithm
              || decorrelation?.commonLeftRightFilter !== true) {
              errors.push(`${key}/wet: 重复BRIR variant未使用左右共用确定性去相关`);
            }
            if (!hex64.test(decorrelation?.digestSha256 ?? "")
              || !Array.isArray(decorrelation?.sections)
              || decorrelation.sections.length !== manifest.calibration.roomTailDecorrelation.sections) {
              errors.push(`${key}/wet: 去相关参数provenance无效`);
            }
            if (!Number.isFinite(decorrelation?.energyTrimDb)
              || Math.abs(decorrelation.energyTrimDb) > manifest.calibration.roomTailDecorrelation.maximumEnergyTrimDb) {
              errors.push(`${key}/wet: 去相关能量恢复越界`);
            }
            for (const metric of ["c50Db", "c80Db", "totalEnergyDb"]) {
              const baseline = decorrelation?.baselineMetrics?.[metric];
              const output = decorrelation?.outputMetrics?.[metric];
              const tolerance = metric === "totalEnergyDb" ? 0.05 : 0.5;
              if (!Number.isFinite(baseline) || !Number.isFinite(output) || Math.abs(output - baseline) > tolerance) {
                errors.push(`${key}/wet: 去相关${metric}偏移越界`);
              }
            }
          } else if (decorrelation?.role !== "canonical" || decorrelation?.algorithm !== null) {
            errors.push(`${key}/wet: 非重复BRIR不得去相关`);
          }
        }
      }
    } else if (!Number.isFinite(processing?.trimStartSample) || !Number.isFinite(processing?.peakNormalizationGain)) {
      errors.push(`${key}/${kind}: 缺少处理provenance`);
    }
    assetHashes.set(`${kind}:${key}`, hash);
  }

  if (calibrated && samplesByKind.dry && samplesByKind.wet) {
    const dry = samplesByKind.dry.samples;
    const wet = samplesByKind.wet.samples;
    const dryLength = samplesByKind.dry.tapCountPerEar;
    const wetLength = samplesByKind.wet.tapCountPerEar;
    if (calibrationV3) {
      const record = position.symmetry;
      const maxShift = manifest.calibration.bilateralSymmetry.maxShiftSamples;
      const centerMaxShift = manifest.calibration.bilateralSymmetry.centerMaxShiftSamples;
      if (!record?.role || (record?.sign !== 1 && record?.sign !== -1)) {
        errors.push(`${key}: v3缺对称化provenance`);
      }
      const shiftLimit = key === "0/0" ? centerMaxShift : maxShift;
      if (!Number.isInteger(record?.shiftSamples) || Math.abs(record.shiftSamples) > shiftLimit) {
        errors.push(`${key}: v3对称化位移越界`);
      }
      if (!Number.isFinite(record?.directWindowCorrelation) || record.directWindowCorrelation < 0.5) {
        errors.push(`${key}: v3对称化直达窗相关过低（对齐质量不足）`);
      }
      if (!Number.isFinite(record?.energyTrimDb) || Math.abs(record.energyTrimDb) > 1.5) {
        errors.push(`${key}: v3对称化能量恢复越界`);
      }
      const energy = (samples, perEar, ear) => {
        let total = 0;
        for (let index = ear * perEar; index < (ear + 1) * perEar; index++) total += samples[index] ** 2;
        return total;
      };
      symmetryEnergies.set(key, {
        dryL: energy(dry, dryLength, 0),
        dryR: energy(dry, dryLength, 1),
        wetL: energy(wet, wetLength, 0),
        wetR: energy(wet, wetLength, 1),
      });
    }
    const fineShift = calibrationV2 ? position.processing?.dry?.energyCentroidTof?.commonShiftSamples : 0;
    const identicalSamples = Math.round(
      manifest.calibration.commonArrivalSample
      + manifest.calibration.roomTailGate.startMs * manifest.sampleRate / 1000
      + (Number.isFinite(fineShift) ? fineShift : 0),
    );
    for (let index = 0; index <= identicalSamples; index++) {
      if (dry[index] !== wet[index] || dry[dryLength + index] !== wet[wetLength + index]) {
        errors.push(`${key}/wet: 房间尾声淡入前未保留目标HRIR直达路径`);
        break;
      }
    }
    try {
      const analysis = analyzeStereoImpulse(
        dry.subarray(0, dryLength),
        dry.subarray(dryLength),
        manifest.sampleRate,
        { onsetThresholdDb: -30, directWindowMs: 4, directFftSize: 4096, referenceMinimumHz: 500, referenceMaximumHz: 2000 },
      );
      dryAnalyses.push({
        key,
        sourceItd: position.processing.dry.sourceOnset.itdSamples,
        analysis,
      });
      if (calibrationV2) {
        const tof = manifest.calibration.energyCentroidTof;
        const room = manifest.calibration.roomResidualReference;
        const roomStart = manifest.calibration.commonArrivalSample + Math.round(room.startMs * manifest.sampleRate / 1000);
        const roomEnd = manifest.calibration.commonArrivalSample + Math.round(room.endMs * manifest.sampleRate / 1000);
        const centroid = directEnergyCentroid(dry, dryLength, tof);
        const fullHrirEnergyDb = stereoEnergyDb(dry, dryLength);
        const roomEnergyDb = roomResidualEnergyDb(dry, dryLength, wet, wetLength, roomStart, roomEnd);
        v2Outputs.push({ key, centroid, fullHrirEnergyDb, roomEnergyDb });
        const recorded = position.processing.dry.energyCentroidTof;
        if (Math.abs(recorded.afterSample - centroid) > 1e-4) errors.push(`${key}/dry: 质心provenance与资产不匹配`);
        if (Math.abs(position.processing.dry.fullHrirEnergyDbOutput - fullHrirEnergyDb) > 1e-4) {
          errors.push(`${key}/dry: full-HRIR能量provenance与资产不匹配`);
        }
        if (calibrationV4) {
          const residualBands = position.processing.wet?.residualBands;
          const bassEnergyDb = roomResidualBandEnergyDb(dry, dryLength, wet, wetLength, 20, 120);
          const midEnergyDb = roomResidualBandEnergyDb(dry, dryLength, wet, wetLength, 250, 4000);
          if (!Number.isFinite(residualBands?.baselineV3BassEnergyDb)
            || !Number.isFinite(residualBands?.bassReductionDb)
            || !Number.isFinite(residualBands?.baselineV3MidEnergyDb)
            || !Number.isFinite(residualBands?.midDifferenceDb)) {
            errors.push(`${key}/wet: v4缺residual频带基线provenance`);
          } else {
            if (Math.abs(residualBands.outputBassEnergyDb - bassEnergyDb) > 1e-4
              || Math.abs(residualBands.outputMidEnergyDb - midEnergyDb) > 1e-4) {
              errors.push(`${key}/wet: v4 residual频带provenance与资产不匹配`);
            }
            if (bassEnergyDb - residualBands.baselineV3BassEnergyDb > -6) {
              errors.push(`${key}/wet: 20-120Hz room residual相对v3未降低至少6dB`);
            }
            if (Math.abs(midEnergyDb - residualBands.baselineV3MidEnergyDb) > 0.5) {
              errors.push(`${key}/wet: 250Hz-4kHz room residual相对v3偏移超过0.5dB`);
            }
          }
        }
      }
    } catch (error) {
      errors.push(`${key}/dry: ${error.message}`);
    }
  }
}
for (const expected of expectedDirections) {
  if (!seen.has(expected)) errors.push(`缺目标方向 ${expected}`);
}

// v3 数值镜像契约：镜像对双耳能量必须精确交叉相等（构造保证），正中方向双耳相等。
if (calibrationV3 && symmetryEnergies.size === expectedDirections.size) {
  const SYMMETRY_PAIRS = [[30, 0], [60, 0], [100, 0], [110, 0], [140, 0], [45, 45], [90, 45], [135, 45]];
  for (const [azimuth, elevation] of SYMMETRY_PAIRS) {
    const plus = symmetryEnergies.get(`${azimuth}/${elevation}`);
    const minus = symmetryEnergies.get(`-${azimuth}/${elevation}`);
    for (const kind of ["dry", "wet"]) {
      const crossLeft = plus[`${kind}L`];
      const crossRight = minus[`${kind}R`];
      const directLeft = plus[`${kind}R`];
      const directRight = minus[`${kind}L`];
      const tolerance = 1e-6 * Math.max(crossLeft, crossRight, 1e-12);
      if (Math.abs(crossLeft - crossRight) > tolerance || Math.abs(directLeft - directRight) > tolerance) {
        errors.push(`±${azimuth}/${elevation}/${kind}: 镜像能量不守恒（对称化资产必须逐耳镜像）`);
      }
    }
  }
  const center = symmetryEnergies.get("0/0");
  for (const kind of ["dry", "wet"]) {
    const tolerance = 1e-6 * Math.max(center[`${kind}L`], 1e-12);
    if (Math.abs(center[`${kind}L`] - center[`${kind}R`]) > tolerance) {
      errors.push(`0/0/${kind}: 正中方向双耳能量不等`);
    }
  }
}

if (calibrated && dryAnalyses.length === expectedDirections.size) {
  if (!calibrationV3) {
    for (const entry of dryAnalyses) {
      if (Math.abs(entry.analysis.onset.itdSamples - entry.sourceItd) > 1) errors.push(`${entry.key}/dry: 实际ITD误差超过1 sample`);
    }
  } else {
    // v3 对称化把每方向 ITD 校准到镜像对均值：不再逐方向钉死源 ITD，
    // 改为验证镜像反对称（+θ 与 -θ 的 ITD 必须互为相反数）与有界偏移。
    const itdByKey = new Map(dryAnalyses.map((entry) => [entry.key, entry]));
    for (const [azimuth, elevation] of [[30, 0], [60, 0], [100, 0], [110, 0], [140, 0], [45, 45], [90, 45], [135, 45]]) {
      const plus = itdByKey.get(`${azimuth}/${elevation}`);
      const minus = itdByKey.get(`-${azimuth}/${elevation}`);
      if (Math.abs(plus.analysis.onset.itdSamples + minus.analysis.onset.itdSamples) > 1) {
        errors.push(`±${azimuth}/${elevation}/dry: 镜像对ITD不反对称（${plus.analysis.onset.itdSamples}/${minus.analysis.onset.itdSamples}）`);
      }
    }
    for (const entry of dryAnalyses) {
      // 对称化的目的就是修正 KU100 头模/摆放的反对称 ITD 偏差（源 ITD 可错数
      // 十 samples，如正中方向源 ITD 27），不再锚定源值；只留生理界限
      // （人头最大 ITD ≈ 0.7ms ≈ 34 samples @48k）。
      if (Math.abs(entry.analysis.onset.itdSamples) > 35) {
        errors.push(`${entry.key}/dry: 对称化ITD超出生理界限（${entry.analysis.onset.itdSamples}）`);
      }
    }
    const center = itdByKey.get("0/0");
    if (Math.abs(center.analysis.onset.itdSamples) > 1) errors.push(`0/0/dry: 正中方向ITD应≈0（实际 ${center.analysis.onset.itdSamples}）`);
  }
  if (calibrationV2 && v2Outputs.length === expectedDirections.size) {
    const centroidValues = v2Outputs.map((entry) => entry.centroid);
    const fullHrirValues = v2Outputs.map((entry) => entry.fullHrirEnergyDb);
    const roomValues = v2Outputs.map((entry) => entry.roomEnergyDb);
    const centroidSpread = Math.max(...centroidValues) - Math.min(...centroidValues);
    const fullHrirSpread = Math.max(...fullHrirValues) - Math.min(...fullHrirValues);
    const roomSpread = Math.max(...roomValues) - Math.min(...roomValues);
    if (centroidSpread > manifest.calibration.energyCentroidTof.maximumOutputSpreadSamples + 1e-6) {
      errors.push(`直达能量质心TOF离散${centroidSpread.toFixed(3)} samples超过1 sample`);
    }
    if (fullHrirSpread > 0.01) errors.push(`full-HRIR能量离散${fullHrirSpread.toFixed(3)}dB超过0.01dB`);
    if (roomSpread > 0.1) errors.push(`4-50ms room residual能量离散${roomSpread.toFixed(3)}dB超过0.1dB`);
    for (const entry of v2Outputs) {
      if (Math.abs(entry.fullHrirEnergyDb - manifest.calibration.directReference.targetEnergyDb) > 0.01) {
        errors.push(`${entry.key}/dry: full-HRIR能量偏离目标`);
      }
      if (Math.abs(entry.roomEnergyDb - manifest.calibration.roomResidualReference.targetEnergyDb) > 0.1) {
        errors.push(`${entry.key}/wet: room residual能量偏离目标`);
      }
    }
  } else if (!calibrationV2) {
    const arrivals = dryAnalyses.map((entry) => entry.analysis.onset.commonSample);
    const references = dryAnalyses.map((entry) => entry.analysis.windows.referencePowerDb);
    const arrivalSpread = Math.max(...arrivals) - Math.min(...arrivals);
    const referenceSpread = Math.max(...references) - Math.min(...references);
    if (arrivalSpread > manifest.sampleRate * 0.0001) errors.push(`共同到达离散${arrivalSpread.toFixed(2)} samples超过0.1ms`);
    if (referenceSpread > 1) errors.push(`直达参考电平离散${referenceSpread.toFixed(2)}dB超过1dB`);
  }
} else if (!calibrated) {
  const wet30 = assetHashes.get("wet:30/0");
  const wet60 = assetHashes.get("wet:60/0");
  const wetMinus30 = assetHashes.get("wet:-30/0");
  const wetMinus60 = assetHashes.get("wet:-60/0");
  if (wet30 !== wet60 || wetMinus30 !== wetMinus60) {
    errors.push("未校准D1最近邻映射应保留±30/±60共享稀疏45度BRIR");
  }
}

for (const [a, b] of [["30/0", "60/0"], ["-30/0", "-60/0"]]) {
  const first = manifest.positions.find((position) => `${position.azimuth}/${position.elevation}` === a);
  const second = manifest.positions.find((position) => `${position.azimuth}/${position.elevation}` === b);
  if (first?.measurement?.wet?.sourcePath !== second?.measurement?.wet?.sourcePath) {
    errors.push(`${a}/${b}: 共享BRIR房间源路径未记录一致`);
  }
  if (!(first?.measurement?.wet?.angularErrorDegrees > 0) || !(second?.measurement?.wet?.angularErrorDegrees > 0)) {
    errors.push(`${a}/${b}: 稀疏BRIR不得标为精确测量`);
  }
}

if (errors.length) {
  console.error(`HRTF资产契约失败:\n- ${errors.join("\n- ")}`);
  process.exit(1);
}
console.log(`HRTF资产契约通过: ${manifest.positions.length}方向，schema v${manifest.schemaVersion}，calibration v${manifest.calibrationVersion}${calibrated ? "（逐音箱房间校准）" : ""}`);
