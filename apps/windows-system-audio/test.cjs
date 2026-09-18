'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { Eac3Bursts, CaptureRecords, classifyFormat } = require('./iec61937.cjs');
const { packEac3 } = require('./pack-eac3.cjs');
const { SystemDecoder } = require('./decoder.cjs');
const { SdaDecoder } = require('../../packages/core/pkg-node/sda_core.cjs');
const root = path.resolve(__dirname, '../..');
const raw = fs.readFileSync(path.join(root, 'harletty-bridge/harletty/tests/fixtures/joc_atmos_1s.eac3'));
const carrier = packEac3(raw);
function wave() {
  const w = Buffer.alloc(52); w.writeUInt16LE(0xfffe); w.writeUInt16LE(2, 2);
  w.writeUInt32LE(192000, 4); w.writeUInt32LE(768000, 8); w.writeUInt16LE(4, 12);
  w.writeUInt16LE(16, 14); w.writeUInt16LE(34, 16); w.writeUInt16LE(16, 18); w.writeUInt32LE(0x3f, 20);
  Buffer.from('0a000000ea0c1000800000aa00389b71', 'hex').copy(w, 24);
  w.writeUInt32LE(48000, 40); w.writeUInt32LE(6, 44); return w;
}
assert.equal(classifyFormat(wave()), 'eac3');
for (const channels of [2,6,8]) for (const mask of [0,3,0x3f,0x60f,0x63f]) {
  const descriptor = wave(); descriptor.writeUInt32LE(channels,44); descriptor.writeUInt32LE(mask,20);
  assert.equal(classifyFormat(descriptor),'eac3');
}
function record(payload, offset = 0n, epoch = 1n, state = 3) {
  return { epoch, state, offset, produced: offset + BigInt(payload.length), overflows: 0n, format: wave(), payload };
}
function encode(r) {
  const h = Buffer.alloc(120); h.write('SDAC'); h.writeUInt32LE(1, 4);
  h.writeBigUInt64LE(r.epoch, 8); h.writeBigUInt64LE(r.offset, 16); h.writeBigUInt64LE(r.produced, 24);
  h.writeBigUInt64LE(r.overflows, 32); h.writeUInt32LE(r.state, 40); h.writeUInt32LE(r.payload.length, 44);
  h.writeUInt32LE(r.format.length, 48); r.format.copy(h, 56); return Buffer.concat([h, r.payload]);
}
function digest() {
  const hash = crypto.createHash('sha256'); let frames = 0, events = 0, objects = 0;
  return { add(f) {
    hash.update(JSON.stringify({ codec: f.codec, sampleRate: f.sampleRate, samplePos: f.samplePos,
      labels: f.labels, events: f.events, objectChannels: f.objectChannels, programLoudness: f.programLoudness }));
    for (const c of f.channels) hash.update(Buffer.from(c.buffer, c.byteOffset, c.byteLength));
    frames++; events += f.events.length; objects = Math.max(objects, f.objectChannels.length);
  }, done() { return { sha256: hash.digest('hex'), frames, events, objects }; } };
}
const expected = digest();
const direct = new SystemDecoder({ createDecoder: c => new SdaDecoder(c), onFrame: f => expected.add(f) });
direct.accept(record(Buffer.alloc(0)));
direct.decoder.push(raw); direct.drain(); direct.finish(); direct.close();
const reference = expected.done();
assert.ok(reference.objects > 0 && reference.events > 0, 'must actually recover Atmos objects');
for (const chunk of [1, 7, 997, 65536]) {
  const got = []; const depacketizer = new Eac3Bursts(b => got.push(b));
  for (let at = 0; at < carrier.length; at += chunk) depacketizer.push(carrier.subarray(at, at + chunk));
  assert.deepEqual(Buffer.concat(got), raw, `payload parity at chunk=${chunk}`);
}
const received = digest();
const decoder = new SystemDecoder({ createDecoder: c => new SdaDecoder(c), onFrame: f => received.add(f) });
const records = new CaptureRecords(r => decoder.accept(r));
const stream = [];
for (let at = 0; at < carrier.length; at += 1777) stream.push(encode(record(carrier.subarray(at, at + 1777), BigInt(at))));
const wire = Buffer.concat(stream);
for (let at = 0; at < wire.length; at += 619) records.push(wire.subarray(at, at + 619));
records.finish(); decoder.finish(); decoder.close();
assert.deepEqual(received.done(), reference, 'object timeline, labels and PCM must match direct decode');
// A pause/seek must erase a partial burst and old decoder state.
const fresh = digest();
const afterSeek = new SystemDecoder({ createDecoder: c => new SdaDecoder(c), onFrame: f => fresh.add(f) });
afterSeek.accept(record(carrier.subarray(0, 501)));
afterSeek.accept(record(Buffer.alloc(0), 0n, 2n, 2));
afterSeek.accept(record(carrier, 0n, 2n)); afterSeek.finish(); afterSeek.close();
assert.deepEqual(fresh.done(), reference);
const bad = Buffer.from(carrier.subarray(0, 8)); bad.writeUInt16LE(65535, 6);
assert.throws(() => new Eac3Bursts(() => {}).push(bad), /corrupt/);
const unsupported = Buffer.from(carrier.subarray(0, 8)); unsupported.writeUInt16LE(0x16, 4);
assert.throws(() => new Eac3Bursts(() => {}).push(unsupported), /Unsupported/);
const corrupt = encode(record(Buffer.alloc(0))); corrupt.writeUInt32LE(100000, 44);
assert.throws(() => new CaptureRecords(() => {}).push(corrupt), /Invalid/);
const truncated = new CaptureRecords(() => {}); truncated.push(wire.subarray(0, 119));
assert.throws(() => truncated.finish(), /Truncated/);
const gap = new SystemDecoder({ createDecoder: c => new SdaDecoder(c), onFrame: () => {} });
gap.accept(record(Buffer.alloc(0)));
assert.throws(() => gap.accept(record(Buffer.alloc(0), 1n)), /gap/); gap.close();
const altered = wave(); altered[28] = 0;
assert.throws(() => classifyFormat(altered), /supports/);
const wrongRate = wave(); wrongRate.writeUInt32LE(96000, 40);
assert.throws(() => classifyFormat(wrongRate), /content descriptor/);
// Standard WAVEFORMATEX PCM and byte fragments preserve channel order and gain.
const pcmWave = wave().subarray(0, 18); pcmWave.writeUInt16LE(1, 0); pcmWave.writeUInt16LE(0, 16);
pcmWave.writeUInt32LE(48000, 4); pcmWave.writeUInt32LE(192000, 8);
const pcmFrames = []; const pcmDecoder = new SystemDecoder({ onFrame: f => pcmFrames.push(f) });
const pcmBytes = Buffer.from([0, 64, 0, 192, 255, 127, 0, 128]);
for (let i = 0; i < pcmBytes.length; i++) pcmDecoder.accept({ ...record(pcmBytes.subarray(i, i + 1), BigInt(i)), format: pcmWave });
assert.deepEqual(pcmFrames.map(f => f.samplePos), [0, 1]);
assert.equal(pcmFrames[0].channels[0][0], 0.5); assert.equal(pcmFrames[0].channels[1][0], -0.5);
assert.equal(pcmFrames[1].channels[1][0], -1);
assert.throws(() => pcmDecoder.accept({ ...record(Buffer.alloc(0), 0n, 0n), format: pcmWave }), /Stale/);
pcmDecoder.close();
console.log(JSON.stringify({ passed: true, reference, rawBytes: raw.length, carrierBytes: carrier.length,
  tests: 'payload/object/PCM parity; arbitrary fragmentation; pause/seek reset; invalid types, lengths, gaps, GUIDs and truncation' }, null, 2));
if (process.argv[2]) fs.writeFileSync(process.argv[2], wire, { flag: 'wx' });
