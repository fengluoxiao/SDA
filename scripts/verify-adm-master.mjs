import assert from 'node:assert/strict';
import { open, writeFile, mkdir } from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { build } from 'esbuild';

const input = resolve(process.argv[2] ?? 'tmp/dolby-natures-fury/Exercise_Content_2-3/NaturesFuryADM.wav');
const output = resolve('tmp/dolby-natures-fury/inspection');
await mkdir(output, { recursive: true });
const bundle = join(output, 'bwf-verifier.mjs');
await build({ entryPoints: ['packages/demux/src/bwf.ts'], bundle: true, platform: 'node', format: 'esm', outfile: bundle });
const { readBwfMetadata, BwfDemuxer } = await import(pathToFileURL(bundle).href);
const file = await open(input, 'r');
let metadata, readBytes = 0;
try {
  metadata = await readBwfMetadata(async (offset, length) => {
    assert.ok(length <= 1024 * 1024, 'desktop IPC read limit');
    readBytes += length;
    const buffer = Buffer.alloc(length);
    const { bytesRead } = await file.read(buffer, 0, length, offset);
    return buffer.subarray(0, bytesRead);
  }, (await file.stat()).size);
} finally { await file.close(); }
const expected = metadata.dataSize / metadata.format.blockAlign;
const energy = new Float64Array(metadata.format.channels), peaks = new Float64Array(metadata.format.channels);
let samples = 0, frames = 0, events = 0;
const demux = new BwfDemuxer({
  onPcmFrame(frame) {
    assert.equal(frame.samplePos, samples);
    assert.equal(frame.channels.length, metadata.format.channels);
    const length = frame.channels[0].length;
    for (let channel = 0; channel < frame.channels.length; channel++) {
      assert.equal(frame.channels[channel].length, length);
      for (const value of frame.channels[channel]) {
        assert.ok(Number.isFinite(value));
        energy[channel] += value * value;
        peaks[channel] = Math.max(peaks[channel], Math.abs(value));
      }
    }
    for (const event of frame.events) {
      assert.ok(event.samplePos >= samples && event.samplePos < samples + length);
      assert.ok(event.pos.every(Number.isFinite));
    }
    events += frame.events.length; frames++; samples += length;
  },
}, metadata);
for await (const chunk of createReadStream(input, { highWaterMark: 1024 * 1024 + 7 })) demux.push(chunk);
demux.flush();
assert.equal(samples, expected);
assert.ok(peaks.some(peak => peak > 0.01));
const report = {
  input, format: metadata.format, durationSeconds: samples / metadata.format.sampleRate,
  samplesPerChannel: samples, frames, emittedEvents: events, metadataReadBytes: readBytes,
  beds: metadata.adm?.rawBedLabels, objects: metadata.adm?.objectChannels.length,
  diffuseEvents: metadata.adm?.events.filter(event => event.diffuse > 0).length,
  horizontalOnlyEvents: metadata.adm?.events.filter(event => event.horizontalOnly).length,
  channels: metadata.labels.map((label, i) => ({ label, peak: peaks[i], rms: Math.sqrt(energy[i] / samples) })),
};
await writeFile(join(output, 'playback-verification.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ ...report, channels: undefined }, null, 2));
