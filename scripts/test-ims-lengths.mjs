import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readUnsigned, scanLengths } from './probe-ims-lengths.mjs';

test('MSB fields cross byte boundaries and reject out-of-bounds reads', () => {
  assert.equal(readUnsigned(Buffer.from([0xab, 0xcd]), 4, 8), 0xbc);
  assert.throws(() => readUnsigned(Buffer.from([0xab]), 4, 8), RangeError);
});

test('finds an unaligned planted length field on unseen lengths', () => {
  const payload = length => {
    const bytes = Buffer.alloc(length);
    const value = length - 3;
    for (let i = 0; i < 8; i++) bytes[(11 + i) >> 3] |= ((value >> (7 - i)) & 1) << (7 - ((11 + i) & 7));
    return bytes;
  };
  const result = scanLengths([31, 45, 62, 90].map(payload), [28, 73, 112].map(payload));
  assert.ok(result.exactAcrossBoth.some(row => row.offset === 11 && row.width === 8 && row.unit === 'bytes' && row.constant === 3));
});

test('rejects a training-only coincidence using independent validation', () => {
  const make = length => { const bytes = Buffer.alloc(length); bytes[1] = length; return bytes; };
  const validation = [make(50), make(70)];
  validation.forEach(bytes => { bytes[1] = 0; });
  const result = scanLengths([make(30), make(40)], validation, { start: 8, end: 16, minWidth: 8, maxWidth: 8 });
  assert.equal(result.exactAcrossBoth.length, 0);
  assert.equal(result.bestTrainingCandidates[0].trainingFraction, 1);
  assert.equal(result.bestTrainingCandidates[0].validationFraction, 0);
});
