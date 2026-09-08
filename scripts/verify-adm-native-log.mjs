import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';

const fixture = JSON.parse(await readFile('tmp/dolby-natures-fury/inspection/playback-verification.json', 'utf8'));
const log = await readFile(process.argv[2] ?? 'tmp/sda-startup.log', 'utf8');
const sessionStart = log.lastIndexOf('startNativeRenderer() called');
assert.ok(sessionStart >= 0, 'native session must be present');
const session = log.slice(sessionStart);
// ACK completion can be reordered during startup; the ring uses absolute clocks.
const frames = [...session.matchAll(/frame (\d+) entries=(\d+) -> accepted=(true|false) samples=(\d+) reason=([^\r\n]*)/g)].sort((a, b) => Number(a[1]) - Number(b[1]));
let end = 0;
for (const frame of frames) {
  assert.equal(frame[3], 'true', 'every PCM frame must be accepted');
  assert.equal(frame[5], '', 'no stale PCM may be discarded');
  assert.equal(Number(frame[1]), end, 'PCM clocks must be contiguous');
  assert.equal(Number(frame[2]), fixture.format.channels);
  end += Number(frame[4]);
}
assert.equal(end, fixture.samplesPerChannel, 'complete master must reach native playback');
const health = [...session.matchAll(/health sample=([^\r\n]+)/g)].map((match) => {
  const fields = Object.fromEntries([...(`sample=${match[1]}`).matchAll(/(\w+)=([^ ]+)/g)].map(([, key, value]) => [key, Number(value)]));
  return fields;
});
assert.ok(health.some((value) => value.sample >= end), 'DAC clock must reach the end');
const playback = health.filter((value) => value.sample > 0 && value.sample < end);
assert.ok(playback.length > 0);
assert.ok(playback.every((value) => value.fifoUnderrun === 0), 'no output FIFO underruns during playback');
// The engine renders ahead of the DAC and keeps draining FIR tails after EOF.
const beforeDrain = playback.filter((value) => value.sample < end - 16384);
assert.ok(beforeDrain.every((value) => value.sourceUnderrun === 0), 'no source starvation before EOF drain');
const last = beforeDrain.at(-1);
const report = {
  input: fixture.input,
  samplesPerChannel: end,
  channels: fixture.format.channels,
  acceptedFrames: frames.length,
  durationSeconds: fixture.durationSeconds,
  sourceUnderrunsBeforeDrain: last.sourceUnderrun,
  outputFifoUnderruns: playback.at(-1).fifoUnderrun,
  renderMeanMicroseconds: last.renderMeanUs,
  renderBudgetMicroseconds: 256 / fixture.format.sampleRate * 1e6,
  nativeExecutableSha256: createHash('sha256').update(await readFile('apps/desktop/native-renderer/SdaNativeRenderer.exe')).digest('hex'),
};
await writeFile(resolve('tmp/dolby-natures-fury/inspection/native-playback-verification.json'), `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify(report, null, 2));
