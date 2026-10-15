import assert from 'node:assert/strict';
import { build } from 'esbuild';
import { createRequire } from 'node:module';
import { mkdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const mp4 = require('mp4box');
const dest = resolve('tmp/mobile-ac4-import');
mkdirSync(dest, { recursive: true });
await build({ entryPoints: ['apps/mobile/src/mpeghMp4.ts'], bundle: true,
  platform: 'node', format: 'cjs', outfile: dest + '/import.cjs' });
const { prepare360RaMp4 } = require(dest + '/import.cjs');

function fixture(type, payloads) {
  const file = mp4.createFile();
  const id = file.addTrack({ type, hdlr: 'soun', timescale: 48000,
    samplerate: 48000, channel_count: 2, samplesize: 16 });
  assert(id);
  payloads.forEach((data, i) => file.addSample(id, Uint8Array.from(data).buffer,
    { duration: 2048, dts: i * 2048, cts: i * 2048, is_sync: true }));
  return Buffer.from(file.getBuffer());
}

function hostFor(input, failWrite = false) {
  const output = [];
  let discarded = 0, finished = 0;
  return {
    output,
    get discarded() { return discarded; },
    get finished() { return finished; },
    async beginMp4Import() { return JSON.stringify({ token: 't', size: input.length }); },
    async readMp4Import(token, offset, count) {
      assert.equal(token, 't'); assert(count <= 256 * 1024);
      return input.subarray(offset, offset + count).toString('base64');
    },
    async appendMp4Import(token, data) {
      assert.equal(token, 't');
      if (failWrite) throw new Error('write failed');
      const bytes = Buffer.from(data, 'base64');
      assert(bytes.length <= 256 * 1024);
      output.push(bytes);
    },
    async finishMp4Import() { finished++; return 'file://normalized.mhas'; },
    async discardMp4Import() { discarded++; },
  };
}

// A container's AC-4 payloads lack sync headers. Validate the independently
// specified Annex G wire format, including crossing native write boundaries.
const payloads = [1, 127, 65534, 65535, 300000].map((size, i) => Buffer.alloc(size, i + 1));
const input = fixture('ac-4', payloads);
const host = hostFor(input);
const progress = [];
const result = await prepare360RaMp4(host, 'fixture', 'song.M4A', value => progress.push(value));
assert.equal(progress[0].fraction, 0);
assert.equal(progress.at(-1).fraction, 1);
assert(progress.some(value => value.stage === '提取 AC-4 音轨'));
assert(progress.every((value, i) => value.fraction >= 0 && value.fraction <= 1 && (!i || value.fraction >= progress[i-1].fraction)));
assert(result);
assert.equal(result.name, 'song.M4A.ac4');
assert.equal(result.uri, 'file://normalized.mhas');
assert.equal(host.finished, 1);
assert.equal(host.discarded, 0);
const bytes = Buffer.concat(host.output);
let offset = 0;
for (const payload of payloads) {
  assert.equal(bytes.readUInt16BE(offset), 0xac40);
  const extended = payload.length >= 65535;
  assert.equal(bytes.readUInt16BE(offset + 2), extended ? 65535 : payload.length);
  if (extended) assert.equal(bytes.readUIntBE(offset + 4, 3), payload.length);
  offset += extended ? 7 : 4;
  assert.deepEqual(bytes.subarray(offset, offset + payload.length), payload);
  offset += payload.length;
}
assert.equal(offset, bytes.length);
assert(host.output.length > 1);
await result.release();
assert.equal(host.discarded, 1);

const failure = hostFor(input, true);
await assert.rejects(prepare360RaMp4(failure, 'fixture', 'song.mp4'), /write failed/);
assert.equal(failure.finished, 0);
assert.equal(failure.discarded, 1);
const fallback = hostFor(fixture('ec-3', [Buffer.from([0x0b, 0x77, 0, 0])]));
assert.equal(await prepare360RaMp4(fallback, 'fixture', 'song.mp4'), null);
assert.equal(fallback.discarded, 1);
assert.equal(fallback.finished, 0);
const raw = hostFor(Buffer.alloc(0));
assert.equal(await prepare360RaMp4(raw, 'fixture', 'song.ac4'), null);
assert.equal(raw.discarded, 0);

const read = path => readFileSync(path, 'utf8');
assert.match(read('apps/mobile/App.tsx'), /"ec3", "ac4", "m4a"/);
assert.match(read('apps/mobile/modules/sda-core/ios/CompressedInput.swift'), /"ec3", "ac4", "mhas"/);
assert.match(read('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/Eac3Input.kt'), /"ec3", "ac4", "mhas"/);
assert.match(read('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt'), /equals\("ac4", ignoreCase = true\)\) "ac4"/);
console.log('Mobile AC-4 import: payload/framing parity, bounded writes, cleanup, E-AC-3 fallback and raw entry points passed.');
