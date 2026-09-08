import assert from 'node:assert/strict';
import { test } from 'node:test';
import { probeFrames } from './probe-ims-structure.mjs';

const frame = (index, independent, hex, sequence = index) => ({ frame: index, sequence,
  streams: [{ index: 3, independent, payloads: hex === null ? [] : [{ id: 18, hex, bytes: hex.length / 2 }] }] });

test('cadence hypotheses include missing payload frames and odd independent frames', () => {
  const [report] = probeFrames([frame(0, true, '0c34'), frame(1, false, null),
    frame(2, false, '0834'), frame(3, true, '0435')]);
  assert.equal(report.hypotheses.every(row => row.mismatches === 0), true);
  assert.deepEqual(report.secondByteRuns.map(row => [row.hex, row.records]), [['34', 2], ['35', 1]]);
});

test('counterexamples remain visible rather than forcing a guessed header', () => {
  const [report] = probeFrames([frame(0, false, '0834', 1), frame(1, false, '0934')]);
  assert.equal(report.hypotheses.find(row => row.name === 'bit4 equals even sequence parity').mismatches, 2);
  assert.equal(report.hypotheses.find(row => row.name === 'first byte contains only parity and independence').mismatches, 1);
  assert.equal(report.hypotheses.find(row => row.name.startsWith('payload presence')).mismatches, 1);
});

test('reject malformed input rather than silently truncating hex', () => {
  assert.throws(() => probeFrames([frame(0, true, '0c34xx')]), /Invalid/);
  assert.throws(() => probeFrames([frame(0, true, '0c')]), /short/);
  const row = frame(0, true, '0c34');
  row.streams[0].payloads.push(row.streams[0].payloads[0]);
  assert.throws(() => probeFrames([row]), /Multiple/);
});

test('rebasing local frame indices does not change sequence-based hypotheses', () => {
  const [report] = probeFrames([frame(0, true, '0434', 13), frame(1, false, '0834', 14),
    frame(2, false, null, 15)]);
  assert.equal(report.hypotheses.find(row => row.name === 'bit4 equals even file-frame parity').mismatches, 2);
  assert.equal(report.hypotheses.find(row => row.name === 'first byte contains only sequence parity and independence').mismatches, 0);
  assert.equal(report.hypotheses.find(row => row.name === 'payload presence equals even sequence or independent').mismatches, 0);
});

test('conditional body statistics retain context and do not invent absent bits', () => {
  const [report] = probeFrames([frame(0, false, '083512'), frame(2, false, '08351a'),
    frame(4, true, '0c3504'), frame(6, false, '0834')]);
  const group = report.conditionalBodyStatistics.find(row => row.group === 'false:35');
  assert.equal(group.count, 2);
  assert.deepEqual(group.constantBits.filter(row => row.index < 20).map(row => row.value), [0, 0, 0, 1]);
  assert.ok(group.constantBits.every(row => row.index < 24 && row.observations === 2));
  assert.deepEqual(report.conditionalBodyStatistics.find(row => row.group === 'false:34').constantBits, []);
  assert.deepEqual(report.conditionalBodyStatistics.find(row => row.group === 'true:35').constantBits, []);
});
