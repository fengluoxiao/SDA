import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function readUnsigned(bytes, offset, width) {
  if (!Number.isInteger(offset) || offset < 0 || !Number.isInteger(width)
      || width < 1 || width > 24 || offset + width > bytes.length * 8) {
    throw new RangeError('Invalid bit field');
  }
  let value = 0;
  for (let i = offset; i < offset + width; i++) value = value * 2 + ((bytes[i >> 3] >> (7 - (i & 7))) & 1);
  return value;
}

export function scanLengths(training, validation, { start = 8, end = 128, minWidth = 4, maxWidth = 16 } = {}) {
  if (!training.length || !validation.length) throw new Error('Training and validation payloads are required');
  const ranked = [];
  let fieldsTested = 0;
  for (let offset = start; offset < end; offset++) {
    for (let width = minWidth; width <= maxWidth && offset + width <= end; width++) {
      if ([...training, ...validation].some(bytes => bytes.length * 8 < offset + width)) continue;
      for (const unit of [1, 8]) {
        fieldsTested++;
        const differences = new Map();
        for (const bytes of training) {
          const difference = bytes.length * unit - readUnsigned(bytes, offset, width);
          differences.set(difference, (differences.get(difference) ?? 0) + 1);
        }
        const [constant, matches] = [...differences].sort((a, b) => b[1] - a[1] || a[0] - b[0])[0];
        const validationMatches = validation.filter(bytes => bytes.length * unit - readUnsigned(bytes, offset, width) === constant).length;
        ranked.push({ offset, width, unit: unit === 1 ? 'bytes' : 'bits', constant,
          trainingMatches: matches, trainingCount: training.length,
          validationMatches, validationCount: validation.length,
          trainingFraction: matches / training.length, validationFraction: validationMatches / validation.length });
      }
    }
  }
  ranked.sort((a, b) => b.trainingFraction - a.trainingFraction || b.validationFraction - a.validationFraction
    || a.offset - b.offset || a.width - b.width);
  return { fieldsTested, exactAcrossBoth: ranked.filter(row => row.trainingFraction === 1 && row.validationFraction === 1),
    bestTrainingCandidates: ranked.slice(0, 10) };
}

async function records(path) {
  const frames = (await readFile(path, 'utf8')).split(/\r?\n/).filter(line => line.trim()).map(line => JSON.parse(line));
  return frames.flatMap(frame => frame.streams.flatMap(stream => stream.payloads.filter(payload => payload.id === 18).map(payload => {
    if (typeof payload.hex !== 'string' || !/^(?:[a-fA-F0-9]{2})+$/.test(payload.hex)) throw new Error('Invalid payload hex');
    const bytes = Buffer.from(payload.hex, 'hex');
    if (bytes.length !== payload.bytes || typeof stream.independent !== 'boolean') throw new Error('Invalid payload context');
    return { bytes, independent: stream.independent };
  })));
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [output, trainPath, ...validationPaths] = process.argv.slice(2);
  if (!output || !trainPath || !validationPaths.length) throw new Error('Usage: node scripts/probe-ims-lengths.mjs new-report.json training.jsonl validation.jsonl [...]');
  const training = await records(trainPath);
  const validation = [];
  for (const path of validationPaths) validation.push(...await records(path));
  const partitions = [];
  for (const independence of [null, false, true]) {
    const select = rows => rows.filter(row => independence === null || row.independent === independence).map(row => row.bytes);
    const train = select(training), valid = select(validation);
    if (!train.length || !valid.length) continue;
    partitions.push({ independence, ...scanLengths(train, valid) });
  }
  const report = { scope: 'MSB-first fixed unsigned fields within bits 8..127, widths 4..16. Tests total byte/bit length minus a learned constant. Does not test subblock lengths, variable-length codes, or spatial semantics.',
    training: resolve(trainPath), validation: validationPaths.map(path => resolve(path)), partitions };
  await writeFile(output, JSON.stringify(report, null, 2), { flag: 'wx' });
  console.log(JSON.stringify(partitions.map(part => ({ independence: part.independence, fieldsTested: part.fieldsTested,
    exactAcrossBoth: part.exactAcrossBoth.length, best: part.bestTrainingCandidates[0] })), null, 2));
}
