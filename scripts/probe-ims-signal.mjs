import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';

const [jsonPath, wavPath, output] = process.argv.slice(2);
if (!jsonPath || !wavPath || !output) throw new Error('Usage: node scripts/probe-ims-signal.mjs payloads.jsonl core.wav new-report.json');
const frames = (await readFile(jsonPath, 'utf8')).split(/\r?\n/).filter(line => line.trim()).map(line => JSON.parse(line));
const wav = await readFile(wavPath);
assert.equal(wav.toString('ascii', 0, 4), 'RIFF');
assert.equal(wav.toString('ascii', 8, 12), 'WAVE');
let sampleRate, data;
for (let offset = 12; offset + 8 <= wav.length;) {
  const size = wav.readUInt32LE(offset + 4);
  assert.ok(offset + 8 + size <= wav.length, 'Truncated WAV chunk');
  const id = wav.toString('ascii', offset, offset + 4);
  const chunk = wav.subarray(offset + 8, offset + 8 + size);
  if (id === 'fmt ') {
    assert.ok(chunk.length >= 16);
    const format = chunk.readUInt16LE(0);
    assert.ok(format === 3 || (format === 0xfffe && chunk.length >= 40 && chunk.readUInt16LE(24) === 3));
    assert.equal(chunk.readUInt16LE(2), 2);
    assert.equal(chunk.readUInt16LE(14), 32);
    sampleRate = chunk.readUInt32LE(4);
  } else if (id === 'data') {
    assert.equal(data, undefined);
    data = chunk;
  }
  offset += size + 8 + (size & 1);
}
assert.ok(sampleRate > 0 && data?.length > 0 && data.length % 8 === 0);
const samplesPerFrame = data.length / 8 / frames.length;
assert.ok(Number.isInteger(samplesPerFrame) && samplesPerFrame > 0, 'Requires complete constant-output-block diagnostic decode');
const runs = [];
let current;
for (let index = 0; index < frames.length; index++) {
  const frame = frames[index];
  assert.equal(frame.frame, index, 'Requires complete zero-based frame sequence');
  assert.equal(frame.streams.length, 1, 'Multistream PCM attribution is unresolved');
  const payloads = frame.streams[0].payloads.filter(payload => payload.id === 18);
  assert.ok(payloads.length <= 1);
  if (payloads.length) {
    const payload = payloads[0];
    assert.match(payload.hex, /^(?:[0-9a-fA-F]{2}){2,}$/);
    const headerByte = Number.parseInt(payload.hex.slice(2, 4), 16);
    if (current?.headerByte !== headerByte) {
      current = { headerByte, hex: headerByte.toString(16).padStart(2, '0'), firstFrame: index,
        lastFrame: index, samples: 0, ll: 0, rr: 0, lr: 0, differenceEnergy: 0, previousL: 0, previousR: 0 };
      runs.push(current);
    }
  }
  if (!current) continue;
  current.lastFrame = index;
  for (let n = index * samplesPerFrame; n < (index + 1) * samplesPerFrame; n++) {
    const l = data.readFloatLE(n * 8), r = data.readFloatLE(n * 8 + 4);
    assert.ok(Number.isFinite(l) && Number.isFinite(r));
    current.ll += l * l; current.rr += r * r; current.lr += l * r;
    if (current.samples) current.differenceEnergy += (l - current.previousL) ** 2 + (r - current.previousR) ** 2;
    current.previousL = l; current.previousR = r; current.samples++;
  }
}
const report = { caveat: 'Core PCM only. Constant samples/frame alignment is checked; codec/filterbank delay is not compensated. Header byte is held until its next update as an analysis convention, not a decoded state rule. First-difference ratio is a spectral roughness proxy, not a measured frequency.',
  jsonPath, wavPath, sampleRate, samplesPerFrame, runs: runs.map(row => ({
    hex: row.hex, firstFrame: row.firstFrame, lastFrame: row.lastFrame,
    startSeconds: row.firstFrame * samplesPerFrame / sampleRate,
    endSeconds: (row.lastFrame + 1) * samplesPerFrame / sampleRate,
    rmsL: Math.sqrt(row.ll / row.samples), rmsR: Math.sqrt(row.rr / row.samples),
    normalizedCrossProduct: row.ll * row.rr > 0 ? row.lr / Math.sqrt(row.ll * row.rr) : null,
    sideToTotalEnergy: row.ll + row.rr > 0 ? (row.ll + row.rr - 2 * row.lr) / (2 * (row.ll + row.rr)) : null,
    firstDifferenceToEnergy: row.ll + row.rr > 0 ? row.differenceEnergy / (row.ll + row.rr) : null,
  })) };
await writeFile(output, JSON.stringify(report, null, 2), { flag: 'wx' });
console.log(JSON.stringify(report, null, 2));
