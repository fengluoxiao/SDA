import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, open, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

// A diagnostic PCM export, not an IMS spatial renderer or an SDA playback backend.
const input = process.argv[2];
const output = process.argv[3];
if (!input || !output) throw new Error('Usage: node scripts/decode-ac4-stereo.mjs input.m4a output.wav');
const executable = process.env.SDA_AC4_REFERENCE ?? resolve('tmp/librempeg-ims/sda-ac4-stereo.exe');
await mkdir(dirname(resolve(output)), { recursive: true });
await new Promise((resolveRun, reject) => {
  let diagnostics = '';
  let incompleteAudio = false;
  const child = spawn(executable, [resolve(input), resolve(output)],
  { stdio: ['ignore', 'inherit', 'pipe'], windowsHide: true });
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', (text) => {
    process.stderr.write(text);
    diagnostics = (diagnostics + text).slice(-16384);
    incompleteAudio ||= /substream audio data (?:overread|underread)/i.test(diagnostics);
  });
  child.on('error', reject);
  child.on('close', (code) => {
    if (code !== 0 || incompleteAudio) reject(new Error(`AC-4 PCM export not validated (exit=${code}, incompleteAudio=${incompleteAudio})`));
    else resolveRun();
  });
});

const file = await open(output, 'r');
let channels, sampleRate, bits, dataOffset, dataBytes;
let peak = 0, energy = 0, values = 0;
try {
  const size = (await file.stat()).size;
  const header = Buffer.alloc(12);
  await file.read(header, 0, 12, 0);
  assert.equal(header.toString('ascii', 0, 4), 'RIFF', 'Verifier requires a RIFF WAVE below 4 GiB');
  assert.equal(header.toString('ascii', 8, 12), 'WAVE');
  for (let offset = 12; offset + 8 <= size;) {
    const chunk = Buffer.alloc(8);
    await file.read(chunk, 0, 8, offset);
    const id = chunk.toString('ascii', 0, 4), length = chunk.readUInt32LE(4);
    assert.ok(offset + 8 + length <= size, `Truncated ${id} chunk`);
    if (id === 'fmt ') {
      assert.ok(length >= 16);
      const fmt = Buffer.alloc(Math.min(length, 40));
      await file.read(fmt, 0, fmt.length, offset + 8);
      const format = fmt.readUInt16LE(0);
      assert.ok(format === 3 || (format === 0xfffe && fmt.length >= 40 && fmt.readUInt16LE(24) === 3));
      channels = fmt.readUInt16LE(2); sampleRate = fmt.readUInt32LE(4); bits = fmt.readUInt16LE(14);
    } else if (id === 'data') {
      assert.equal(dataOffset, undefined, 'Multiple data chunks');
      dataOffset = offset + 8; dataBytes = length;
    }
    offset += 8 + length + (length & 1);
  }
  assert.equal(bits, 32); assert.equal(channels, 2, 'Expected decoded stereo PCM');
  assert.ok(sampleRate > 0 && dataBytes > 0); assert.equal(dataBytes % (channels * 4), 0);
  const buffer = Buffer.alloc(65536);
  for (let offset = 0; offset < dataBytes;) {
    const count = Math.min(buffer.length, dataBytes - offset);
    const { bytesRead } = await file.read(buffer, 0, count, dataOffset + offset);
    assert.equal(bytesRead, count);
    for (let i = 0; i < count; i += 4) {
      const value = buffer.readFloatLE(i);
      assert.ok(Number.isFinite(value), `Non-finite PCM at byte ${offset + i}`);
      peak = Math.max(peak, Math.abs(value)); energy += value * value; values++;
    }
    offset += count;
  }
  assert.ok(peak > 0, 'Silent output');
} finally { await file.close(); }
const report = { input: resolve(input), output: resolve(output), backend: 'Librempeg AC-4 stereo',
  imsSpatialProcessing: false, channels, sampleRate, samples: values / channels,
  durationSeconds: values / channels / sampleRate, peak, rms: Math.sqrt(energy / values) };
await writeFile(`${output}.verification.json`, JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
