import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

test('WAV conversion preserves ITD and room tail, rejects unsupported sample rate', () => {
  const dir = mkdtempSync(join(tmpdir(), 'sda-cinema-converter-'));
  try {
    const wav = Buffer.alloc(44 + 1024 * 8);
    wav.write('RIFF'); wav.writeUInt32LE(wav.length - 8, 4); wav.write('WAVEfmt ', 8);
    wav.writeUInt32LE(16, 16); wav.writeUInt16LE(3, 20); wav.writeUInt16LE(2, 22);
    wav.writeUInt32LE(48000, 24); wav.writeUInt32LE(384000, 28);
    wav.writeUInt16LE(8, 32); wav.writeUInt16LE(32, 34); wav.write('data', 36);
    wav.writeUInt32LE(wav.length - 44, 40);
    wav.writeFloatLE(1, 44 + 20 * 8); wav.writeFloatLE(1, 44 + 24 * 8 + 4);
    wav.writeFloatLE(0.25, 44 + 600 * 8); wav.writeFloatLE(0.25, 44 + 600 * 8 + 4);
    const input = join(dir, 'input.json'), output = join(dir, 'room.json');
    writeFileSync(join(dir, 'impulse.wav'), wav);
    writeFileSync(input, JSON.stringify({name:'Synthetic test only',source:'Generated test impulse',license:'Test',measurement:'dummy-head',layout:'2.0',speakers:[
      {name:'FrontLeft',azimuth:30,elevation:0,file:'impulse.wav'},
      {name:'FrontRight',azimuth:-30,elevation:0,file:'impulse.wav'}
    ]}));
    const script = fileURLToPath(new URL('../../../scripts/prepare-room-profile.mjs', import.meta.url));
    const run = () => spawnSync(process.execPath, [script,input,output], {encoding:'utf8'});
    const result = run(); assert.equal(result.status, 0, result.stderr);
    const room = JSON.parse(readFileSync(output, 'utf8'));
    assert.equal(room.speakers[0].directLeft[20], 1);
    assert.equal(room.speakers[0].directRight[24], 1);
    assert.equal(room.speakers[0].directLeft[600], 0);
    assert.equal(room.speakers[0].roomLeft[600], 0.25);
    const report = JSON.parse(readFileSync(`${output}.report.json`, 'utf8'));
    assert.equal(report.rows[0].itdMs, 4 / 48);
    wav.writeUInt32LE(44100, 24); writeFileSync(join(dir, 'impulse.wav'), wav);
    assert.notEqual(run().status, 0);
  } finally { rmSync(dir, {recursive:true,force:true}); }
});
