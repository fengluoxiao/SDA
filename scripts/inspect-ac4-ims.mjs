import { createReadStream } from 'node:fs';
import { mkdir, open } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawn } from 'node:child_process';
import { build } from 'esbuild';

const input = process.argv[2], directory = process.argv[3];
if (!input || !directory) throw new Error('Usage: node scripts/inspect-ac4-ims.mjs input.m4a output-directory');
const output = resolve(directory);
await mkdir(output, { recursive: true });
const bundle = resolve(output, 'mp4.mjs');
await build({ entryPoints: ['packages/demux/src/mp4.ts'], outfile: bundle, bundle: true, platform: 'node', format: 'esm' });
const { Mp4Demuxer } = await import(pathToFileURL(bundle));
const raw = resolve(output, 'audio.ac4');
const file = await open(raw, 'wx');
let packets = [];
const demux = new Mp4Demuxer({ onTrack(track) {
  if (track.codec !== 'ac-4') throw new Error('Selected track is not AC-4');
}, onPacket(packet) { packets.push(packet.data.slice()); }, onError(error) { throw new Error(error); } });
async function drain() {
  for (const packet of packets) {
    let offset = 0;
    while (offset < packet.length) {
      const { bytesWritten } = await file.write(packet, offset, packet.length - offset);
      if (!bytesWritten) throw new Error('Zero-length write');
      offset += bytesWritten;
    }
  }
  packets = [];
}
try {
  for await (const chunk of createReadStream(input, { highWaterMark: 32768 })) {
    demux.push(chunk); await drain();
  }
  demux.flush(); await drain();
} finally { await file.close(); }
await new Promise((done, reject) => {
  const child = spawn('cargo', ['+1.98.0', 'run', '--offline', '--manifest-path',
    'apps/ac4-decoder/inspect/Cargo.toml', '--target-dir', 'tmp/ac4-ims-inspect-build', '--',
    raw, resolve(output, 'payloads.jsonl')], { stdio: 'inherit', windowsHide: true });
  child.on('error', reject);
  child.on('close', code => code === 0 ? done() : reject(new Error(`IMS inspection exited ${code}`)));
});
