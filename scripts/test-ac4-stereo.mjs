import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const directory = await mkdtemp(resolve('tmp/ac4-stereo-test-'));
const executable = resolve('tmp/librempeg-ims/sda-ac4-stereo.exe');
const run = (args) => {
  const result = spawnSync(executable, args, { encoding: 'utf8', windowsHide: true });
  if (result.error) throw result.error;
  assert.equal(result.signal, null);
  return result;
};
assert.equal(run([]).status, 2);
const invalid = resolve(directory, 'invalid.ac4');
await writeFile(invalid, Buffer.from([0xac, 0x40, 0xff, 0xff, 0x01]));
assert.notEqual(run([invalid, resolve(directory, 'invalid.wav')]).status, 0);
const sample = process.argv[2];
if (sample) {
  const existing = resolve(directory, '\u9a8c\u8bc1.wav');
  const sentinel = Buffer.from('Existing output must survive AC-4 decoding unchanged');
  await writeFile(existing, sentinel);
  assert.notEqual(run([resolve(sample), existing]).status, 0);
  assert.deepEqual(await readFile(existing), sentinel);
}
console.log('AC-4 stereo CLI: argument, malformed input, and optional Unicode overwrite checks passed.');
