import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const bit = (bytes, index) => (bytes[index >> 3] >> (7 - (index & 7))) & 1;

export function probeFrames(frames) {
  const groups = new Map();
  for (const frame of frames) {
    for (const stream of frame.streams) {
      const key = `${frame.presentationIndex ?? 0}:${stream.index}`;
      if (!groups.has(key)) groups.set(key, []);
      const payloads = stream.payloads.filter(payload => payload.id === 18);
      if (payloads.length > 1) throw new Error(`Multiple ID 18 payloads at frame ${frame.frame}; ordering is unresolved`);
      const payload = payloads[0];
      let bytes = null;
      if (payload) {
        if (typeof payload.hex !== 'string' || !/^(?:[a-fA-F0-9]{2}){2,}$/.test(payload.hex)) {
          throw new Error(`Invalid or short payload at frame ${frame.frame}`);
        }
        bytes = Buffer.from(payload.hex, 'hex');
        if (bytes.length !== payload.bytes) throw new Error(`Length mismatch at frame ${frame.frame}`);
      }
      if (!Number.isSafeInteger(frame.frame) || !Number.isSafeInteger(frame.sequence)
          || typeof stream.independent !== 'boolean') throw new Error('Missing frame context');
      groups.get(key).push({ frame: frame.frame, sequence: frame.sequence, independent: stream.independent, bytes });
    }
  }
  return [...groups].map(([stream, rows]) => {
    const records = rows.filter(row => row.bytes);
    const check = (name, eligible, prediction, observed) => {
      const failures = eligible.filter(row => prediction(row) !== observed(row));
      return { name, observations: eligible.length, mismatches: failures.length,
        counterexamples: failures.slice(0, 12).map(row => ({ frame: row.frame,
          expected: prediction(row), observed: observed(row) })) };
    };
    const runs = [];
    for (const row of records) {
      const value = row.bytes[1];
      const previous = runs.at(-1);
      if (previous?.value === value) {
        previous.lastFrame = row.frame; previous.records++;
        previous.independent += Number(row.independent);
      } else runs.push({ value, hex: value.toString(16).padStart(2, '0'),
        firstFrame: row.frame, lastFrame: row.frame, records: 1, independent: Number(row.independent) });
    }
    // These are competing sample-derived hypotheses, never a decoding grammar.
    const hypotheses = [
      check('bit5 equals audio independence', records, row => Number(row.independent), row => bit(row.bytes, 5)),
      check('bit4 equals even file-frame parity', records, row => Number(row.frame % 2 === 0), row => bit(row.bytes, 4)),
      check('bit4 equals even sequence parity', records, row => Number(row.sequence % 2 === 0), row => bit(row.bytes, 4)),
      check('first byte contains only parity and independence', records,
        row => (row.frame % 2 === 0 ? 8 : 0) + (row.independent ? 4 : 0), row => row.bytes[0]),
      check('payload presence equals even file-frame or independent', rows,
        row => row.frame % 2 === 0 || row.independent, row => Boolean(row.bytes)),
      check('first byte contains only sequence parity and independence', records,
        row => (row.sequence % 2 === 0 ? 8 : 0) + (row.independent ? 4 : 0), row => row.bytes[0]),
      check('payload presence equals even sequence or independent', rows,
        row => row.sequence % 2 === 0 || row.independent, row => Boolean(row.bytes)),
    ];
    const constantPrefixBits = [];
    for (let index = 0; index < 16 && records.length; index++) {
      const value = bit(records[0].bytes, index);
      if (records.every(row => bit(row.bytes, index) === value)) constantPrefixBits.push({ index, value });
    }
    const bodyGroups = new Map();
    for (const row of records) {
      const key = `${row.independent}:${row.bytes[1].toString(16).padStart(2, '0')}`;
      if (!bodyGroups.has(key)) bodyGroups.set(key, []);
      bodyGroups.get(key).push(row);
    }
    const conditionalBodyStatistics = [...bodyGroups].map(([group, rows]) => {
      const constantBits = [];
      for (let index = 16; index < 64; index++) {
        const eligible = rows.filter(row => row.bytes.length * 8 > index);
        if (eligible.length < 2) continue;
        const value = bit(eligible[0].bytes, index);
        if (eligible.every(row => bit(row.bytes, index) === value)) {
          constantBits.push({ index, value, observations: eligible.length });
        }
      }
      return { group, count: rows.length,
        minLength: Math.min(...rows.map(row => row.bytes.length)),
        maxLength: Math.max(...rows.map(row => row.bytes.length)),
        meanLength: rows.reduce((sum, row) => sum + row.bytes.length, 0) / rows.length,
        constantBits };
    });
    return { stream, frames: rows.length, payloads: records.length, hypotheses, constantPrefixBits,
      secondByteRuns: runs, conditionalBodyStatistics };
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [output, ...inputs] = process.argv.slice(2);
  if (!output || !inputs.length) throw new Error('Usage: node scripts/probe-ims-structure.mjs new-report.json payloads.jsonl [...]');
  const sources = [];
  for (const input of inputs) {
    const frames = (await readFile(input, 'utf8')).split(/\r?\n/).filter(line => line.trim()).map(line => JSON.parse(line));
    sources.push({ source: resolve(input), streams: probeFrames(frames) });
  }
  const report = { interpretation: 'Hypothesis tests only. Matches do not establish syntax or spatial semantics.', sources };
  await writeFile(output, JSON.stringify(report, null, 2), { flag: 'wx' });
  console.log(JSON.stringify(report, null, 2));
}
