'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { Readable } = require('node:stream');
const { SystemAudio } = require('./system-audio.cjs');
const { CaptureRecords } = require('../windows-system-audio/iec61937.cjs');
const { packEac3 } = require('../windows-system-audio/pack-eac3.cjs');
const { SdaDecoder } = require('../../packages/core/pkg-node/sda_core.cjs');
const { createHash } = require('node:crypto');
function digest(batch) {
  const hash = createHash('sha256');
  hash.update(JSON.stringify({sample:batch.sample,events:batch.events,ids:batch.channels.map(c=>c.id)}));
  for (const c of batch.channels) hash.update(Buffer.from(c.samples.buffer,c.samples.byteOffset,c.samples.byteLength));
  return hash.digest('hex');
}

function wave(encoded) {
  const b = Buffer.alloc(40);
  b.writeUInt16LE(0xfffe); b.writeUInt16LE(2, 2);
  b.writeUInt32LE(encoded ? 192000 : 48000, 4);
  b.writeUInt32LE(encoded ? 768000 : 192000, 8);
  b.writeUInt16LE(4, 12); b.writeUInt16LE(16, 14);
  b.writeUInt16LE(22, 16); b.writeUInt16LE(16, 18);
  b.writeUInt32LE(encoded ? 0x3f : 3, 20);
  Buffer.from(encoded ? '0a000000ea0c1000800000aa00389b71' : '0100000000001000800000aa00389b71', 'hex').copy(b, 24);
  return b;
}
function record(epoch, payload, encoded) {
  const parts = [];
  for (let offset = 0; offset < payload.length; offset += 16384) {
    const chunk = payload.subarray(offset, offset + 16384), h = Buffer.alloc(120);
    h.write('SDAC'); h.writeUInt32LE(1, 4); h.writeBigUInt64LE(BigInt(epoch), 8);
    h.writeBigUInt64LE(BigInt(offset), 16); h.writeBigUInt64LE(BigInt(payload.length), 24); h.writeUInt32LE(3, 40);
    h.writeUInt32LE(chunk.length, 44); h.writeUInt32LE(40, 48);
    wave(encoded).copy(h, 56); parts.push(h, chunk);
  }
  return Buffer.concat(parts);
}
(async () => {
  const raw = fs.readFileSync(require('node:path').join(__dirname, '../../harletty-bridge/harletty/tests/fixtures/joc_atmos_1s.eac3'));
  const carrier = packEac3(raw);
  // Compare every live batch to the independent direct-file decoder, including
  // object IDs/positions and samples, not merely a nonzero output meter.
  const direct = new SdaDecoder('eac3'); direct.push(raw);
  const expected = [];
  for (let f; (f = direct.nextFrame());) {
    const objects = new Map(JSON.parse(f.objectChannelsJson).map(o => [o.channel, o.id]));
    expected.push({ sample: f.samplePos, events: JSON.parse(f.eventsJson), channels: f.labels.map((label, i) => ({
      id: objects.has(i) ? `obj:${objects.get(i)}` : /^Obj_\d+$/.test(label) ? `obj:${label.slice(4)}` : `bed:${i}`, samples: new Float32Array(f.channel(i)),
    })) });
    f.free();
  }
  direct.free();
  assert.ok(expected.length > 30);
  const batches = [], commands = [], statuses = [];
  const service = new SystemAudio({ command: async c => { commands.push(c); return true; },
    batch: async (sample, channels, events) => { batches.push({ sample, channels, events }); return { accepted: true }; },
    publish: status => statuses.push(status) });
  const session = { inputMode: 'bitstream' }; service.session = session;
  const stream = Buffer.concat([
    record(1, Buffer.alloc(48000, 0x40), false), // Audible PCM must be blocked.
    record(2, carrier.subarray(0, 501), true), // Discard incomplete pre-seek data.
    record(3, carrier, true),
    record(4, Buffer.alloc(48000, 0x40), false), // Stop rendering on PCM fallback.
    record(5, carrier, true), // Same connection recovers automatically.
  ]);
  const chunks = []; for (let i = 0; i < stream.length; i += 997) chunks.push(stream.subarray(i, i + 997));
  await service.consume(session, Readable.from(chunks), CaptureRecords,
    require('../windows-system-audio/decoder.cjs').SystemDecoder, SdaDecoder);
  assert.equal(batches.length, expected.length * 2);
  for (let i = 0; i < batches.length; i++) {
    const a=batches[i], b=expected[i%expected.length];
    assert.equal(a.sample,b.sample,`clock ${i}`);
    assert.deepEqual(a.events,b.events,`metadata ${i}`);
    assert.deepEqual(a.channels.map(c=>c.id),b.channels.map(c=>c.id),`IDs ${i}`);
    assert.equal(digest(a),digest(b),`PCM ${i}`);
  }
  assert.equal(commands.filter(c => c.type === 'startAt').length, 2);
  assert.equal(statuses.filter(s => s.detail.includes('收到的是 PCM')).length, 2);
  assert.equal(service.status.objects, 15);
  assert.ok(commands.some(c => c.type === 'addSource' && c.id.startsWith('obj:') && !c.bedLabel));
  console.log(`PASS bitstream mode: PCM blocked, seek reset, automatic recovery; ${batches.length} native batches match direct object PCM and metadata`);
  if (process.argv[2]) {
    batches.length = 0;
    await service.consume(session, fs.createReadStream(process.argv[2]), CaptureRecords,
      require('../windows-system-audio/decoder.cjs').SystemDecoder, SdaDecoder);
    assert.deepEqual(batches.map(digest), expected.map(digest), 'captured driver stream must match the full reference');
    console.log(`PASS actual driver capture: ${batches.length} object batches match direct file decode`);
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
