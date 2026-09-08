import assert from 'node:assert/strict';
import { ac4SyncFrame } from '../src/mp4.ts';

for (const size of [1, 127, 65534, 65535, 100000]) {
  const payload = new Uint8Array(size); payload[0] = 0x12; payload[size - 1] = 0x34;
  const framed = ac4SyncFrame(payload);
  const extended = size >= 65535;
  assert.deepEqual([...framed.subarray(0, 2)], [0xac, 0x40]);
  assert.equal(framed.length, size + (extended ? 7 : 4));
  const declared = extended ? framed[4] * 65536 + framed[5] * 256 + framed[6] : framed[2] * 256 + framed[3];
  assert.equal(declared, size);
  assert.deepEqual(framed.subarray(extended ? 7 : 4), payload);
}
assert.throws(() => ac4SyncFrame(new Uint8Array()), /size/);
assert.throws(() => ac4SyncFrame(new Uint8Array(0x1000000)), /size/);
console.log('AC-4 MP4 transport: short/extended lengths and payload preservation passed');
