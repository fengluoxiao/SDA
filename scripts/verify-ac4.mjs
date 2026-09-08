import assert from 'node:assert/strict';
import { createReadStream } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { build } from 'esbuild';

const input = resolve(process.argv[2]);
const output = resolve(process.argv[3] ?? 'tmp/ac4-verification');
await mkdir(output, { recursive: true });
const bundle = resolve(output, 'mp4.mjs');
await build({ entryPoints: ['packages/demux/src/mp4.ts'], outfile: bundle, bundle: true, platform: 'node', format: 'esm' });
const { Mp4Demuxer } = await import(pathToFileURL(bundle));
const require = createRequire(import.meta.url);
const { SdaDecoder } = require('../packages/core/pkg-node/sda_core.cjs');
const decoder = new SdaDecoder('ac4');
let frames = 0, samples = 0, events = 0, packets = 0, track, labels;
let sampleRate = 0, peak = 0, energy = 0, values = 0, movingEvents = 0;
const positions = new Map(), objects = new Set();
const started = performance.now();
function drain() {
  const errors = decoder.drainErrors(); assert.deepEqual(errors, []);
  for (let frame; (frame = decoder.nextFrame());) {
    assert.equal(frame.codec, 'ac4'); assert.equal(frame.samplePos, samples);
    labels = frame.labels; sampleRate = frame.sampleRate;
    for (let i = 0; i < frame.channelCount; i++) {
      const pcm = frame.channel(i); assert.equal(pcm.length, frame.samplesPerChannel);
      for (const value of pcm) { assert.ok(Number.isFinite(value)); peak = Math.max(peak, Math.abs(value)); energy += value * value; values++; }
    }
    for (const declaration of JSON.parse(frame.objectChannelsJson)) objects.add(declaration.id);
    for (const event of JSON.parse(frame.eventsJson)) {
      assert.ok(event.samplePos >= frame.samplePos && event.samplePos < frame.samplePos + frame.samplesPerChannel);
      assert.ok(event.pos.every(Number.isFinite)); assert.ok(Number.isFinite(event.gainDb));
      const previous = positions.get(event.id);
      if (event.hasPos && previous && previous.some((v, i) => v !== event.pos[i])) movingEvents++;
      if (event.hasPos) positions.set(event.id, event.pos);
      events++;
    }
    samples += frame.samplesPerChannel; frames++; frame.free();
  }
}
const demux = new Mp4Demuxer({ onTrack(value) { track = value; assert.equal(value.codec, 'ac-4'); },
  onPacket(packet) { decoder.push(packet.data); packets++; drain(); }, onError(error) { throw Error(error); } });
try {
  for await (const bytes of createReadStream(input, { highWaterMark: 32771 })) {
    if (/\.ac4$/i.test(input)) { decoder.push(bytes); drain(); } else demux.push(bytes);
  }
  if (!/\.ac4$/i.test(input)) demux.flush();
  decoder.flush(); drain();
  assert.ok(frames > 0 && peak > 0 && objects.size > 0, 'must decode audible object PCM');
  const report = { input, frames, packets, samples, sampleRate, durationSeconds: samples / sampleRate,
    containerDuration: track?.durationSec, labels, objects: objects.size, events, movingEvents,
    peak, rms: Math.sqrt(energy / values), elapsedSeconds: (performance.now() - started) / 1000 };
  await writeFile(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
} finally { decoder.free(); }
