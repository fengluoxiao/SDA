/**
 * Headphone playback compensation profiles.
 *
 * Profiles belong after the final binaural stereo merge. They must never alter
 * the KU100 HRIR/BRIR assets or individual virtual-speaker buses.
 */

export type HeadphoneMeasurementMode = "independent-lr" | "average-dual-mono";

export interface HeadphoneCompensationProfile {
  /** Stable profile identifier. */
  id: string;
  /** Headphone model and revision as stated by the measurement source. */
  name: string;
  /** Public measurement source or a local, auditable measurement reference. */
  source: string;
  /** The response target used when deriving the correction FIRs. */
  target: string;
  /** Public references for independently measured left/right source responses. */
  leftMeasurement: string;
  rightMeasurement: string;
  /** Measurement rig, playback state, fit/tip/firmware, and verified channel mapping. */
  balanceEvidence: string;
  /** Optional classification for a built-in average measurement profile. */
  measurementMode?: HeadphoneMeasurementMode;
  /** Human-readable channel limitation for a non-independent profile. */
  channelClaim?: string;
  /** Original single/average response reference for average-dual-mono profiles. */
  averageMeasurement?: string;
  /** Source revision and FIR derivation summary for average-dual-mono profiles. */
  derivation?: string;
  sampleRate: number;
  /** Static attenuation derived from the published FIR's maximum frequency-response boost. */
  preampDb: number;
  /** Final stereo-output FIR asset URLs. Left/right may intentionally differ. */
  leftFirUrl: string;
  rightFirUrl: string;
}

/** Local profiles are verified by the desktop main process before registration.
 * The renderer receives only manifest evidence plus the two immutable FIR blobs;
 * it never reads arbitrary local paths. */
export interface LocalHeadphoneCompensationProfile extends HeadphoneCompensationProfile {
  schemaVersion: 1;
  /** independent-lr needs separate L/R evidence; average-dual-mono is explicitly
   * one measurement-derived EQ applied unchanged to each output channel. */
  measurementMode: HeadphoneMeasurementMode;
  /** Required for average-dual-mono. Must state it is not L/R balance calibration. */
  channelClaim: string;
  /** Required for average-dual-mono. Original single/average source data reference. */
  averageMeasurement?: string;
  /** Required for average-dual-mono. Source revision and conversion method. */
  derivation?: string;
  createdAt: string;
  deviceRevision: string;
  playbackState: string;
  earTips: string;
  firmware: string;
  measurementRig: string;
  referenceBand: string;
  leftFir: { fileName: string; tapCount: number; sha256: string };
  rightFir: { fileName: string; tapCount: number; sha256: string };
}

export interface LocalHeadphoneCompensationData {
  profile: LocalHeadphoneCompensationProfile;
  leftFir: ArrayBuffer;
  rightFir: ArrayBuffer;
}

export interface HeadphoneCompensationBuffers {
  left: AudioBuffer;
  right: AudioBuffer;
}

/**
 * Built-ins need auditable provenance. MDR-7506 is deliberately classified as
 * average-dual-mono: it applies the same published average EQ to each channel
 * after the final binaural merge and never claims an L/R balance calibration.
 */
export const HEADPHONE_COMPENSATION_PROFILES: readonly HeadphoneCompensationProfile[] = [
  {
    id: "beyerdynamic-dt-1990-balanced-average-autoeq",
    name: "拜亚动力 DT 1990 PRO（一代 · Balanced 耳垫）",
    source: "AutoEq Rtings HMS II.3 over-ear, revision 7ae0f56d53074872b028649617a22bbb4232feb7",
    target: "AutoEq over-ear target; FIR normalized at 1 kHz",
    leftMeasurement: "不适用：平均测量响应",
    rightMeasurement: "不适用：平均测量响应",
    balanceEvidence: "非独立左右声道测量或平衡校准",
    measurementMode: "average-dual-mono",
    channelClaim: "同一平均测量 EQ 应用于 L/R；非独立 L/R 校准，不修正个体声道差异。",
    averageMeasurement: "https://github.com/jaakkopasanen/AutoEq/tree/7ae0f56d53074872b028649617a22bbb4232feb7/results/Rtings/HMS%20II.3%20over-ear/Beyerdynamic%20DT%201990%20(balanced%20earpads)",
    derivation: "scripts/build-beyerdynamic-dt-1990-balanced-average-profile.mjs; published PEQ, 48 kHz, 8192 taps, 1 kHz normalization; headroom recomputed",
    sampleRate: 48000,
    preampDb: -1.8,
    leftFirUrl: "headphone-compensation/beyerdynamic-dt-1990-balanced-average-autoeq/average.f32",
    rightFirUrl: "headphone-compensation/beyerdynamic-dt-1990-balanced-average-autoeq/average.f32",
  },
  {
    id: "sennheiser-hd-820-average-autoeq",
    name: "森海塞尔 HD 820（AutoEq 平均测量 EQ，L/R 同一曲线）",
    source: "AutoEq HypetheSonics over-ear result, revision 7ae0f56d53074872b028649617a22bbb4232feb7",
    target: "AutoEq over-ear target; FIR normalized to 0 dB at 1 kHz",
    leftMeasurement: "不适用：公开来源为单一/平均测量响应",
    rightMeasurement: "不适用：公开来源为单一/平均测量响应",
    balanceEvidence: "不适用：此 profile 不声称独立左右声道测量或 balance 校准",
    measurementMode: "average-dual-mono",
    channelClaim: "同一平均测量 EQ 应用于 L/R；非独立 L/R 校准，不修正耳机个体声道差异",
    averageMeasurement: "https://github.com/jaakkopasanen/AutoEq/tree/7ae0f56d53074872b028649617a22bbb4232feb7/results/HypetheSonics/over-ear/Sennheiser%20HD%20820",
    derivation: "scripts/build-sennheiser-hd-820-average-profile.mjs; published 10-band PEQ synthesized at 48 kHz, 8192 taps, 1 kHz normalized; source -6.4 dB preamp excluded",
    sampleRate: 48000,
    preampDb: -8.3,
    leftFirUrl: "headphone-compensation/sennheiser-hd-820-average-autoeq/average.f32",
    rightFirUrl: "headphone-compensation/sennheiser-hd-820-average-autoeq/average.f32",
  },
  {
    id: "beyerdynamic-xelento-2nd-gen-average-autoeq",
    name: "Beyerdynamic Xelento 2nd Gen 有线版（AutoEq 平均测量 EQ，L/R 同一曲线）",
    source: "AutoEq HypetheSonics GRAS RA0045 in-ear result, revision 6c9a097626213b8cbb0973e5a4dd645f5f9e3fd4",
    target: "AutoEq in-ear target; FIR normalized to 0 dB at 1 kHz",
    leftMeasurement: "不适用：公开来源为单一/平均测量响应",
    rightMeasurement: "不适用：公开来源为单一/平均测量响应",
    balanceEvidence: "不适用：此 profile 不声称独立左右声道测量或 balance 校准",
    measurementMode: "average-dual-mono",
    channelClaim: "同一平均测量 EQ 应用于 L/R；非独立 L/R 校准，不修正耳机个体声道差异",
    averageMeasurement: "https://github.com/jaakkopasanen/AutoEq/tree/master/results/HypetheSonics/GRAS%20RA0045%20in-ear/Beyerdynamic%20Xelento%20%282nd%20Gen%29",
    derivation: "scripts/build-beyerdynamic-xelento-2nd-gen-average-profile.mjs; published 10-band PEQ synthesized at 48 kHz, 8192 taps, 1 kHz normalized; source -6.3 dB preamp excluded",
    sampleRate: 48000,
    preampDb: -6.3,
    leftFirUrl: "headphone-compensation/beyerdynamic-xelento-2nd-gen-average-autoeq/average.f32",
    rightFirUrl: "headphone-compensation/beyerdynamic-xelento-2nd-gen-average-autoeq/average.f32",
  },
  {
    id: "beyerdynamic-xelento-wired-average-autoeq",
    name: "Beyerdynamic Xelento 有线版（AutoEq 平均测量 EQ，L/R 同一曲线）",
    source: "AutoEq HypetheSonics B&K 5128 in-ear result, revision 6c9a097626213b8cbb0973e5a4dd645f5f9e3fd4",
    target: "AutoEq in-ear target; FIR normalized to 0 dB at 1 kHz",
    leftMeasurement: "不适用：公开来源为单一/平均测量响应",
    rightMeasurement: "不适用：公开来源为单一/平均测量响应",
    balanceEvidence: "不适用：此 profile 不声称独立左右声道测量或 balance 校准",
    measurementMode: "average-dual-mono",
    channelClaim: "同一平均测量 EQ 应用于 L/R；非独立 L/R 校准，不修正耳机个体声道差异",
    averageMeasurement: "https://github.com/jaakkopasanen/AutoEq/tree/master/results/HypetheSonics/Bruel%20%26%20Kjaer%205128%20in-ear/Beyerdynamic%20Xelento",
    derivation: "scripts/build-beyerdynamic-xelento-wired-average-profile.mjs; published 10-band PEQ synthesized at 48 kHz, 8192 taps, 1 kHz normalized; source -6.6 dB preamp excluded",
    sampleRate: 48000,
    preampDb: -5.2,
    leftFirUrl: "headphone-compensation/beyerdynamic-xelento-wired-average-autoeq/average.f32",
    rightFirUrl: "headphone-compensation/beyerdynamic-xelento-wired-average-autoeq/average.f32",
  },
  {
    id: "sony-mdr-7506-average-autoeq",
    name: "Sony MDR-7506（AutoEq 平均测量 EQ，L/R 同一曲线）",
    source: "AutoEq Super Review result, revision 36b1afcdf161c8a52b5093daefbbd335272508f3",
    target: "AutoEq Harman over-ear target; FIR normalized to 0 dB at 1 kHz",
    leftMeasurement: "不适用：公开来源为单一/平均测量响应",
    rightMeasurement: "不适用：公开来源为单一/平均测量响应",
    balanceEvidence: "不适用：此 profile 不声称独立左右声道测量或 balance 校准",
    measurementMode: "average-dual-mono",
    channelClaim: "同一平均测量 EQ 应用于 L/R；非独立 L/R 校准，不修正耳机个体声道差异",
    averageMeasurement: "https://github.com/jaakkopasanen/AutoEq/tree/master/results/Super%20Review/over-ear/Sony%20MDR-7506",
    derivation: "scripts/build-sony-mdr-7506-average-profile.mjs; published 10-band PEQ synthesized at 48 kHz, 8192 taps, 1 kHz normalized; source -4.1 dB preamp excluded",
    sampleRate: 48000,
    preampDb: -6.1,
    leftFirUrl: "headphone-compensation/sony-mdr-7506-average-autoeq/average.f32",
    rightFirUrl: "headphone-compensation/sony-mdr-7506-average-autoeq/average.f32",
  },
];

const localProfiles = new Map<string, LocalHeadphoneCompensationData>();

interface RawHeadphoneCompensation {
  profile: HeadphoneCompensationProfile;
  left: Float32Array;
  right: Float32Array;
}

const rawCache = new Map<string, Promise<RawHeadphoneCompensation>>();
let bundledAssetLoader: ((assetPath: string) => Promise<ArrayBuffer>) | null = null;

/** Desktop file:// pages cannot fetch sibling .f32 files. The shell injects a
 * path-restricted loader; normal web builds keep using HTTP fetch. */
export function setHeadphoneCompensationAssetLoader(
  loader: ((assetPath: string) => Promise<ArrayBuffer>) | null,
): void {
  bundledAssetLoader = loader;
  rawCache.clear();
}

export function headphoneProfileById(id: string | null): HeadphoneCompensationProfile | null {
  if (!id) return null;
  return localProfiles.get(id)?.profile ?? HEADPHONE_COMPENSATION_PROFILES.find((profile) => profile.id === id) ?? null;
}

/** Profiles available to the UI. Local entries have passed desktop-side validation. */
export function availableHeadphoneCompensationProfiles(): readonly HeadphoneCompensationProfile[] {
  return [...HEADPHONE_COMPENSATION_PROFILES, ...[...localProfiles.values()].map((entry) => entry.profile)];
}

export function validateHeadphoneProfile(profile: HeadphoneCompensationProfile): string[] {
  const errors = validateCommonProfile(profile);
  const mode = profile.measurementMode ?? "independent-lr";
  if (mode === "independent-lr") {
    if (!profile.leftMeasurement.trim() || !profile.rightMeasurement.trim()) errors.push("独立 L/R profile 必须提供左右测量来源");
    if (!profile.balanceEvidence.trim()) errors.push("独立 L/R profile 缺少平衡证明");
    if (profile.leftFirUrl === profile.rightFirUrl) errors.push("独立 L/R profile 的左右 FIR 必须是独立资产");
  } else if (mode === "average-dual-mono") {
    if (!profile.averageMeasurement?.trim()) errors.push("平均双单声道 profile 缺少 averageMeasurement");
    if (!profile.derivation?.trim()) errors.push("平均双单声道 profile 缺少 derivation");
    if (!profile.channelClaim?.trim() || !/not independent|非独立|同一.*(?:eq|曲线)/i.test(profile.channelClaim)) {
      errors.push("平均双单声道 profile 必须明确非独立 L/R 声明");
    }
    if (profile.leftFirUrl !== profile.rightFirUrl) errors.push("平均双单声道 profile 的左右 FIR 必须指向同一资产");
  } else {
    errors.push("measurementMode 必须为 independent-lr 或 average-dual-mono");
  }
  return errors;
}

function validateCommonProfile(profile: HeadphoneCompensationProfile): string[] {
  const errors: string[] = [];
  if (!/^[a-z0-9][a-z0-9-]*$/.test(profile.id)) errors.push("id 必须是小写 slug");
  if (!profile.name.trim()) errors.push("缺少耳机型号名称");
  if (!profile.source.trim()) errors.push("缺少测量来源");
  if (!profile.target.trim()) errors.push("缺少目标曲线说明");
  if (!Number.isFinite(profile.sampleRate) || profile.sampleRate <= 0) errors.push("采样率无效");
  if (!Number.isFinite(profile.preampDb) || profile.preampDb > 0) errors.push("preampDb 必须是有限非正值");
  if (!profile.leftFirUrl || !profile.rightFirUrl) errors.push("必须提供左右 FIR 资产");
  return errors;
}

export function validateLocalHeadphoneProfile(data: LocalHeadphoneCompensationData): string[] {
  const { profile } = data;
  const errors = validateCommonProfile(profile);
  if (profile.schemaVersion !== 1) errors.push("不支持的本地 profile schemaVersion");
  if (profile.measurementMode !== "independent-lr" && profile.measurementMode !== "average-dual-mono") {
    errors.push("measurementMode 必须为 independent-lr 或 average-dual-mono");
  }
  if (!profile.channelClaim.trim()) errors.push("缺少 channelClaim");
  if (!Number.isFinite(Date.parse(profile.createdAt))) errors.push("createdAt 无效");
  for (const key of ["deviceRevision", "playbackState", "earTips", "firmware", "measurementRig", "referenceBand"] as const) {
    if (!profile[key].trim()) errors.push(`缺少 ${key}`);
  }
  if (profile.measurementMode === "independent-lr") {
    if (!profile.leftMeasurement.trim() || !profile.rightMeasurement.trim()) errors.push("独立 L/R profile 必须提供左右测量来源");
    if (!profile.balanceEvidence.trim()) errors.push("独立 L/R profile 缺少平衡证明");
  } else if (profile.measurementMode === "average-dual-mono") {
    if (!profile.averageMeasurement?.trim()) errors.push("平均双单声道 profile 缺少 averageMeasurement");
    if (!profile.derivation?.trim()) errors.push("平均双单声道 profile 缺少 derivation");
    if (!/not independent|非独立|同一.*(?:eq|曲线)/i.test(profile.channelClaim)) {
      errors.push("平均双单声道 profile 必须明确非独立 L/R 声明");
    }
  }
  for (const [ear, asset, buffer] of [["left", profile.leftFir, data.leftFir], ["right", profile.rightFir, data.rightFir]] as const) {
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*\.f32$/.test(asset.fileName)) errors.push(`${ear} FIR 文件名无效`);
    if (!/^[a-f0-9]{64}$/i.test(asset.sha256)) errors.push(`${ear} FIR SHA-256 无效`);
    if (!Number.isInteger(asset.tapCount) || asset.tapCount < 2) errors.push(`${ear} FIR tapCount 无效`);
    if (buffer.byteLength !== asset.tapCount * Float32Array.BYTES_PER_ELEMENT) errors.push(`${ear} FIR 字节长度与 tapCount 不符`);
    try {
      decodeRawFir(buffer, asset.fileName);
    } catch (error) {
      errors.push(error instanceof Error ? error.message : `${ear} FIR 无效`);
    }
  }
  const sharedAsset = profile.leftFir.fileName === profile.rightFir.fileName || profile.leftFir.sha256 === profile.rightFir.sha256;
  if (profile.measurementMode === "independent-lr" && sharedAsset) errors.push("独立 L/R profile 的左右 FIR 必须是独立资产");
  if (profile.measurementMode === "average-dual-mono" && !sharedAsset) errors.push("平均双单声道 profile 的左右 FIR 必须指向同一资产");
  return errors;
}

/** Register an already validated in-memory local profile. Replacing an ID clears
 * raw-tap cache so later AudioContexts cannot receive stale FIR data. */
export function registerLocalHeadphoneCompensation(data: LocalHeadphoneCompensationData): void {
  const errors = validateLocalHeadphoneProfile(data);
  if (errors.length) throw new Error(`本地耳机补偿 profile 无效: ${errors.join("；")}`);
  localProfiles.set(data.profile.id, data);
  rawCache.delete(data.profile.id);
}

export function unregisterLocalHeadphoneCompensation(id: string): boolean {
  rawCache.delete(id);
  return localProfiles.delete(id);
}

function decodeRawFir(buffer: ArrayBuffer, url: string): Float32Array {
  if (!buffer.byteLength || buffer.byteLength % Float32Array.BYTES_PER_ELEMENT) {
    throw new Error(`耳机 FIR 无效字节长度: ${url}`);
  }
  const taps = new Float32Array(buffer);
  if (!taps.every(Number.isFinite)) throw new Error(`耳机 FIR 包含无效 tap: ${url}`);
  return taps;
}

function resampleLinear(taps: Float32Array, fromRate: number, toRate: number): Float32Array {
  if (Math.abs(fromRate - toRate) < 1) return taps;
  const ratio = fromRate / toRate;
  const output = new Float32Array(Math.round(taps.length / ratio));
  for (let i = 0; i < output.length; i++) {
    const pos = i * ratio;
    const floor = Math.floor(pos);
    const fraction = pos - floor;
    const a = taps[floor] ?? 0;
    const b = taps[Math.min(taps.length - 1, floor + 1)] ?? 0;
    output[i] = (a + (b - a) * fraction) * ratio;
  }
  return output;
}

async function getRawHeadphoneCompensation(profile: HeadphoneCompensationProfile): Promise<RawHeadphoneCompensation> {
  let request = rawCache.get(profile.id);
  if (!request) {
    const local = localProfiles.get(profile.id);
    request = local
      ? Promise.resolve({
          profile: local.profile,
          left: decodeRawFir(local.leftFir, local.profile.leftFir.fileName),
          right: decodeRawFir(local.rightFir, local.profile.rightFir.fileName),
        })
      : bundledAssetLoader
        ? Promise.all([
            bundledAssetLoader(profile.leftFirUrl),
            bundledAssetLoader(profile.rightFirUrl),
          ]).then(([leftBuffer, rightBuffer]) => ({
            profile,
            left: decodeRawFir(leftBuffer, profile.leftFirUrl),
            right: decodeRawFir(rightBuffer, profile.rightFirUrl),
          }))
        : Promise.all([fetch(profile.leftFirUrl), fetch(profile.rightFirUrl)])
            .then(async ([left, right]) => {
              if (!left.ok) throw new Error(`耳机左 FIR HTTP ${left.status}: ${profile.leftFirUrl}`);
              if (!right.ok) throw new Error(`耳机右 FIR HTTP ${right.status}: ${profile.rightFirUrl}`);
              const [leftBuffer, rightBuffer] = await Promise.all([left.arrayBuffer(), right.arrayBuffer()]);
              return { profile, left: decodeRawFir(leftBuffer, profile.leftFirUrl), right: decodeRawFir(rightBuffer, profile.rightFirUrl) };
            });
    request.catch(() => rawCache.delete(profile.id));
    rawCache.set(profile.id, request);
  }
  return request;
}

/** Load immutable raw taps once, then create context-local mono AudioBuffers. */
export async function getHeadphoneCompensationBuffers(
  ctx: AudioContext,
  profile: HeadphoneCompensationProfile,
): Promise<HeadphoneCompensationBuffers> {
  const raw = await getRawHeadphoneCompensation(profile);
  const makeBuffer = (taps: Float32Array) => {
    const data = resampleLinear(taps, raw.profile.sampleRate, ctx.sampleRate);
    const buffer = ctx.createBuffer(1, data.length, ctx.sampleRate);
    buffer.copyToChannel(data as Float32Array<ArrayBuffer>, 0);
    return buffer;
  };
  return { left: makeBuffer(raw.left), right: makeBuffer(raw.right) };
}
