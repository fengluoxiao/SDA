import { createReadStream } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import { resolve } from 'node:path';

const input = process.argv[2], output = process.argv[3];
if (!input || !output) throw new Error('Usage: node scripts/analyze-ims-payloads.mjs payloads.jsonl report.json');
const groups = new Map();
let frames = 0, framesWithoutPayload = 0;
for await (const line of createInterface({ input: createReadStream(input), crlfDelay: Infinity })) {
  if (!line.trim()) continue;
  const frame = JSON.parse(line); frames++;
  let found = false;
  for (const stream of frame.streams) for (const payload of stream.payloads) {
    found = true;
    const key = `${stream.index}:${payload.id}`;
    if (!groups.has(key)) groups.set(key, { stream: stream.index, id: payload.id, records: [] });
    if (typeof payload.hex !== 'string' || !/^(?:[0-9a-fA-F]{2})*$/.test(payload.hex)) {
      throw new Error(`Invalid payload hex at frame ${frame.frame}`);
    }
    const bytes = Buffer.from(payload.hex, 'hex');
    if (bytes.length !== payload.bytes) throw new Error(`Bad payload length at frame ${frame.frame}`);
    groups.get(key).records.push({ frame: frame.frame, independent: stream.independent, bytes });
  }
  if (!found) framesWithoutPayload++;
}
const histogram = values => {
  const result = {};
  for (const value of values) result[value] = (result[value] ?? 0) + 1;
  return result;
};
const reports = [...groups.values()].map(({ stream, id, records }) => {
  const bitStats = [];
  for (let bit = 0; bit < 128; bit++) {
    const eligible = records.filter(row => row.bytes.length * 8 > bit);
    let ones = 0, independentAgreement = 0;
    for (const row of eligible) {
      const value = (row.bytes[bit >> 3] >> (7 - (bit & 7))) & 1;
      ones += value; independentAgreement += Number(Boolean(value) === row.independent);
    }
    bitStats.push({ bit, observations: eligible.length, ones, independentAgreement });
  }
  return { stream, id, count: records.length,
    independent: records.filter(row => row.independent).length,
    lengths: histogram(records.map(row => row.bytes.length)),
    gapsInFrames: histogram(records.slice(1).map((row, index) => row.frame - records[index].frame)),
    firstTwoBytes: histogram(records.map(row => row.bytes.subarray(0, 2).toString('hex'))),
    lastTwoBytes: histogram(records.map(row => row.bytes.subarray(-2).toString('hex'))),
    uniquePayloads: new Set(records.map(row => row.bytes.toString('hex'))).size,
    meanLengthByIndependence: Object.fromEntries([false, true].map(independent => {
      const matching = records.filter(row => row.independent === independent);
      return [String(independent), matching.length ? matching.reduce((sum, row) => sum + row.bytes.length, 0) / matching.length : null];
    })),
    prefixBitStatistics: bitStats,
    firstIndependentFrames: records.filter(row => row.independent).slice(0, 16).map(row => row.frame),
  };
});
const report = { source: resolve(input), frames, framesWithoutPayload,
  interpretation: 'Observational statistics only; no payload field semantics established', payloads: reports };
await writeFile(output, JSON.stringify(report, null, 2), { flag: 'wx' });
console.log(JSON.stringify({ frames, framesWithoutPayload, payloads: reports.map(({prefixBitStatistics, lastTwoBytes, lengths, ...rest}) => ({
  ...rest, minLength: Math.min(...Object.keys(lengths).map(Number)), maxLength: Math.max(...Object.keys(lengths).map(Number)),
  bitsMatchingIndependence: rest.independent > 0 && rest.independent < rest.count
    ? prefixBitStatistics.filter(bit => bit.observations === rest.count && bit.independentAgreement === rest.count).map(bit => bit.bit) : [],
})) }, null, 2));
