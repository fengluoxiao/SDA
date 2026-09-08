import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { build } from 'esbuild';

const require = createRequire(import.meta.url);
const { SdaDecoder } = require('../packages/core/pkg-node/sda_core.cjs');
function run(bytes, step, codec = 'ac4') {
  const decoder = new SdaDecoder(codec), hash = createHash('sha256');
  let frames = 0, samples = 0;
  const errors = [];
  const drain = () => {
    errors.push(...decoder.drainErrors());
    for (let frame; (frame = decoder.nextFrame());) {
      assert.equal(frame.samplePos, samples);
      hash.update(frame.eventsJson);
      for (let i = 0; i < frame.channelCount; i++) {
        const pcm = frame.channel(i);
        hash.update(Buffer.from(pcm.buffer, pcm.byteOffset, pcm.byteLength));
      }
      samples += frame.samplesPerChannel; frames++; frame.free();
    }
  };
  try {
    for (let offset = 0; offset < bytes.length; offset += step) { decoder.push(bytes.subarray(offset, offset + step)); drain(); }
    decoder.flush(); drain();
    return { frames, samples, hash: hash.digest('hex'), errors };
  } finally { decoder.free(); }
}
assert.match(run(Buffer.from([0xac, 0x40, 0, 5, 1]), 1).errors.join(), /truncated/);
assert.match(run(Buffer.from([0xac, 0x41, 0, 1, 1, 0, 0]), 2).errors.join(), /CRC/);
if (process.argv[2]) {
  await mkdir('tmp/ac4-verification', { recursive: true });
  const bundle = resolve('tmp/ac4-verification/mp4-test.mjs');
  await build({ entryPoints: ['packages/demux/src/mp4.ts'], outfile: bundle, bundle: true, platform: 'node', format: 'esm' });
  const { Mp4Demuxer } = await import(pathToFileURL(bundle));
  const packets = [];
  const demux = new Mp4Demuxer({ onTrack: track => assert.equal(track.codec, 'ac-4'), onPacket: packet => packets.push(packet.data) });
  for await (const bytes of createReadStream(process.argv[2], { highWaterMark: 32768 })) {
    demux.push(bytes); if (packets.length >= 16) break;
  }
  assert.ok(packets.length >= 16);
  const bytes = Buffer.concat(packets.slice(0, 16));
  const expected = run(bytes, bytes.length);
  assert.equal(expected.frames, 16); assert.deepEqual(expected.errors, []);
  assert.deepEqual(run(bytes, 37), expected);
  assert.deepEqual(run(bytes, 1, 'auto'), expected);
}
console.log('AC-4 WASM: CRC, truncation, fragmented input and auto-detection parity passed');
