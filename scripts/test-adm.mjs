import { build } from 'esbuild';
import { mkdir } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { basename, resolve } from 'node:path';

const directory = resolve('tmp/adm-regression');
await mkdir(directory, { recursive: true });
const tests = [
  'packages/demux/test/adm-bwf.test.mjs',
  'packages/demux/test/dbmd.test.mjs',
  'packages/demux/test/alac-sample-entry.test.mjs',
  'packages/demux/test/mkv-streaming.test.mjs',
  'packages/demux/test/mp4-extraction-lifecycle.test.mjs',
  'packages/player/test/adm-resampler.test.mjs',
  'packages/player/test/control.test.mjs',
  'packages/player/test/frame-batcher.test.mjs',
  'packages/renderer/test/object-event-batch.test.mjs',
  'packages/renderer/test/head-pose.test.mjs',
  'packages/renderer/test/renderer-start-at.test.mjs',
  'apps/desktop/test/adm-capacity.test.mjs',
  'apps/desktop/test/native-renderer-contract.test.mjs',
  'apps/desktop/test/startup-log.test.mjs',
];
let failures = 0;
for (const entry of tests) {
  const outfile = resolve(directory, basename(entry));
  const direct = entry.endsWith('mp4-extraction-lifecycle.test.mjs') || entry.startsWith('apps/desktop/');
  if (!direct) await build({ entryPoints: [entry], outfile, bundle: true, format: 'esm', platform: 'node', loader: { '.wasm': 'file' } });
  const result = spawnSync(process.execPath, [direct ? resolve(entry) : outfile], { stdio: 'inherit', windowsHide: true });
  if (result.status !== 0) failures++;
}
if (failures) throw new Error(`${failures} ADM regression suites failed`);
console.log(`${tests.length} ADM regression suites passed`);
