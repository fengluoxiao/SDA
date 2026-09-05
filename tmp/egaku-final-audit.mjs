import { readFileSync, writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const index = JSON.parse(readFileSync('tmp/egaku-fixed-24s.idx.json'));
const load = path => {
  const b = readFileSync(path);
  return new Float32Array(b.buffer, b.byteOffset, b.length / 4);
};
const objects = load('tmp/egaku-fixed-24s.pcm');
const core = load('tmp/egaku-ffmpeg-raw-core.f32');
const n = 1536, count = 750, labels = index[0].labels;
assert.equal(index.length, count);
assert.equal(objects.length, count * n * 16);
assert.equal(core.length, count * n * 6);
assert.equal(new Set(labels).size, 16);
index.forEach((frame, i) => {
  assert.deepEqual(frame.labels, labels);
  assert.equal(frame.samplePos, i * n);
  assert.equal(frame.samplesPerChannel, n);
  assert.equal(frame.sampleRate, 48000);
});
const pairs = [];
for (let a = 1; a < 16; a++) for (let b = a + 1; b < 16; b++) {
  let aa = 0, bb = 0, ab = 0, maxDifference = 0;
  for (let f = 0; f < count; f++) for (let s = 0; s < n; s++) {
    const x = objects[(f * 16 + a) * n + s];
    const y = objects[(f * 16 + b) * n + s];
    aa += x*x; bb += y*y; ab += x*y;
    maxDifference = Math.max(maxDifference, Math.abs(x-y));
  }
  pairs.push({a: labels[a], b: labels[b], correlation: ab / Math.sqrt(aa*bb), maxDifference});
}
const db = energy => 10 * Math.log10(Math.max(energy / n, 1e-30));
const windows = [];
for (let f = 1; f < count; f++) {
  let objectEnergy = 0, objectSumEnergy = 0, coreEnergy = 0, coreSumEnergy = 0;
  const suspect = [5, 6, 13, 15].map(ch => ({label: labels[ch], energy: 0}));
  for (let s = 0; s < n; s++) {
    let os = 0, cs = 0;
    for (let ch = 1; ch < 16; ch++) {
      const x = objects[(f * 16 + ch) * n + s];
      assert.ok(Number.isFinite(x));
      objectEnergy += x*x; os += x;
    }
    for (const ch of [0, 1, 2, 4, 5]) {
      const x = core[(f*n+s-577)*6+ch];
      assert.ok(Number.isFinite(x));
      coreEnergy += x*x; cs += x;
    }
    for (const item of suspect) {
      const ch = labels.indexOf(item.label);
      const x = objects[(f*16+ch)*n+s]; item.energy += x*x;
    }
    objectSumEnergy += os*os; coreSumEnergy += cs*cs;
  }
  windows.push({frame:f, rawSeconds:f*n/48000, objectEnergyDb:db(objectEnergy), objectSumDb:db(objectSumEnergy), coreEnergyDb:db(coreEnergy), coreSumDb:db(coreSumEnergy), suspect: suspect.map(x => ({label:x.label, rmsDb:db(x.energy)}))});
}
const report = {
  frames: count, stableUniqueLabels: labels, contiguous: true,
  exactDuplicatePairs: pairs.filter(p => p.maxDifference === 0),
  strongestCorrelations: pairs.sort((a,b) => Math.abs(b.correlation)-Math.abs(a.correlation)).slice(0,5),
  selectedWindows: windows.filter(w => [410,413,415].includes(w.frame)),
  caveat: 'Raw-carrier timeline; core aligned by 577 samples. LFE excluded. Object and core energies are not required to match. Correlation does not establish decoder correctness.',
};
writeFileSync('tmp/egaku-final-audit.json', JSON.stringify({ ...report, windows }, null, 2));
console.log(JSON.stringify(report, null, 2));
