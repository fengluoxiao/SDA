import { open, mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
if (!process.argv[2]) throw new Error('Usage: node scripts/prepare-adm-native-benchmark.mjs input.wav [metadata.json]');
const path = resolve(process.argv[2]);
const destination = resolve(process.argv[3] ?? 'tmp/adm-performance.json');
const { outputFiles } = await build({
  entryPoints: [resolve(root, 'packages/demux/src/bwf.ts')],
  bundle: true, platform: 'node', format: 'esm', write: false,
});
const { readBwfMetadata } = await import(`data:text/javascript;base64,${Buffer.from(outputFiles[0].contents).toString('base64')}`);
const file = await open(path, 'r');
try {
  const metadata = await readBwfMetadata(async (offset, length) => {
    const bytes = Buffer.alloc(length);
    const { bytesRead } = await file.read(bytes, 0, length, offset);
    return bytes.subarray(0, bytesRead);
  }, (await file.stat()).size);
  if (!metadata.adm || metadata.format.bits !== 24 || metadata.format.sampleRate !== 48000) {
    throw new Error('The native file benchmark requires 48 kHz, 24-bit ADM PCM.');
  }
  await mkdir(dirname(destination), { recursive: true });
  await writeFile(destination, JSON.stringify({ path, ...metadata }));
  console.log(JSON.stringify({ destination, channels: metadata.format.channels,
    objects: metadata.adm.objectChannels.length, events: metadata.adm.events.length }));
} finally {
  await file.close();
}
