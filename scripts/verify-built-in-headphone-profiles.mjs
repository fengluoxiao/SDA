#!/usr/bin/env node
/** Verify every built-in headphone profile survives the production web build. */
import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";

const dist = resolve("apps/web/dist");
const profileRoot = resolve(dist, "headphone-compensation");
const errors = [];
const headroomLimitDb = -0.1;

function maximumFirResponseDb(bytes) {
  const taps = new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / Float32Array.BYTES_PER_ELEMENT);
  let size = 1;
  while (size < taps.length * 8) size <<= 1;
  const re = new Float64Array(size);
  const im = new Float64Array(size);
  re.set(taps);
  for (let i = 1, j = 0; i < size; i++) {
    let bit = size >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) [re[i], re[j]] = [re[j], re[i]];
  }
  for (let length = 2; length <= size; length <<= 1) {
    const angle = (-2 * Math.PI) / length;
    for (let start = 0; start < size; start += length) {
      for (let offset = 0; offset < length / 2; offset++) {
        const cos = Math.cos(angle * offset);
        const sin = Math.sin(angle * offset);
        const even = start + offset;
        const odd = even + length / 2;
        const oddRe = re[odd] * cos - im[odd] * sin;
        const oddIm = re[odd] * sin + im[odd] * cos;
        re[odd] = re[even] - oddRe;
        im[odd] = im[even] - oddIm;
        re[even] += oddRe;
        im[even] += oddIm;
      }
    }
  }
  let maximum = 0;
  for (let i = 0; i <= size / 2; i++) maximum = Math.max(maximum, Math.hypot(re[i], im[i]));
  return 20 * Math.log10(maximum);
}

const ids = [
  "beyerdynamic-dt-1990-balanced-average-autoeq",
  "sony-mdr-7506-average-autoeq",
  "beyerdynamic-xelento-wired-average-autoeq",
  "beyerdynamic-xelento-2nd-gen-average-autoeq",
  "sennheiser-hd-820-average-autoeq",
];

for (const id of ids) {
  const directory = resolve(profileRoot, id);
  const manifestPath = resolve(directory, "profile.json");
  const readmePath = resolve(directory, "README.md");
  if (!existsSync(manifestPath)) {
    errors.push(`${id}: dist 缺少 profile.json`);
    continue;
  }
  if (!existsSync(readmePath)) errors.push(`${id}: dist 缺少 README.md`);
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (manifest.id !== id) errors.push(`${id}: manifest id 不匹配`);
  if (manifest.leftFir.fileName !== manifest.rightFir.fileName || manifest.leftFir.sha256 !== manifest.rightFir.sha256) {
    errors.push(`${id}: 内置 average-dual-mono FIR 必须共享同一资产`);
  }
  const firPath = resolve(directory, manifest.leftFir.fileName);
  if (!existsSync(firPath)) {
    errors.push(`${id}: dist 缺少 ${manifest.leftFir.fileName}`);
    continue;
  }
  const bytes = readFileSync(firPath);
  if (statSync(firPath).size !== manifest.leftFir.tapCount * Float32Array.BYTES_PER_ELEMENT) {
    errors.push(`${id}: FIR tapCount/字节长度不符`);
  }
  const hash = createHash("sha256").update(bytes).digest("hex");
  if (hash !== manifest.leftFir.sha256) errors.push(`${id}: FIR SHA-256 不匹配`);
  if (!Number.isFinite(manifest.preampDb) || manifest.preampDb > 0) {
    errors.push(`${id}: preampDb 必须是有限非正值`);
  } else {
    const finalPeakDb = maximumFirResponseDb(bytes) + manifest.preampDb;
    if (finalPeakDb > headroomLimitDb + 0.01) {
      errors.push(`${id}: FIR + preamp 峰值 ${finalPeakDb.toFixed(2)} dB 超过 ${headroomLimitDb} dB`);
    }
  }
  const resolvedFromFilePage = new URL(`headphone-compensation/${id}/${manifest.leftFir.fileName}`, new URL("index.html", `file:///${dist.replaceAll("\\", "/")}/`));
  if (!resolvedFromFilePage.pathname.endsWith(`/headphone-compensation/${id}/${manifest.leftFir.fileName}`)) {
    errors.push(`${id}: file:// 相对 URL 解析错误`);
  }
}

if (errors.length) {
  console.error(`生产耳机补偿资产无效: ${errors.join("；")}`);
  process.exit(1);
}
console.log(`生产耳机补偿资产有效: ${ids.length} 个内置 profile 已随 web dist 发布`);
