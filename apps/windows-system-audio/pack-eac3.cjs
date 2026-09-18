'use strict';
// Minimal known-source WASAPI test fixture builder, not a general media muxer.
// Supports 48k, six-block independent AUs with their dependent substreams.
// Rejects other cadence instead of generating a misleading carrier clock.
const { PERIOD } = require('./iec61937.cjs');
function packEac3(bytes) {
  const units = []; let unit = [];
  for (let at = 0; at < bytes.length;) {
    if (bytes.length - at < 6 || bytes[at] !== 0x0b || bytes[at + 1] !== 0x77) throw new Error('Invalid E-AC-3 frame');
    const size = (((bytes[at + 2] & 7) << 8) | bytes[at + 3]) * 2 + 2;
    if (size < 6 || at + size > bytes.length) throw new Error('Truncated E-AC-3 frame');
    const streamType = bytes[at + 2] >> 6, substreamId = (bytes[at + 2] >> 3) & 7;
    if (bytes[at + 4] >> 6 !== 0 || ((bytes[at + 4] >> 4) & 3) !== 3 || bytes[at + 5] >> 3 <= 10)
      throw new Error('Fixture sender requires 48k six-block E-AC-3 frames');
    if (streamType === 0 && substreamId === 0) { if (unit.length) units.push(Buffer.concat(unit)); unit = []; }
    else if (streamType !== 1 || !unit.length) throw new Error('Unsupported E-AC-3 substream arrangement');
    unit.push(bytes.subarray(at, at + size)); at += size;
  }
  if (unit.length) units.push(Buffer.concat(unit));
  if (!units.length) throw new Error('No E-AC-3 units');
  return Buffer.concat(units.map(payload => {
    if (payload.length > PERIOD - 8) throw new Error('E-AC-3 access unit exceeds carrier burst');
    const burst = Buffer.alloc(PERIOD);
    burst.writeUInt16LE(0xf872, 0); burst.writeUInt16LE(0x4e1f, 2);
    burst.writeUInt16LE(0x15, 4); burst.writeUInt16LE(payload.length, 6);
    Buffer.from(payload).swap16().copy(burst, 8); return burst;
  }));
}
module.exports = { packEac3 };
if (require.main === module) {
  const fs = require('node:fs');
  const [input, output] = process.argv.slice(2);
  if (!input || !output) throw new Error('Usage: node pack-eac3.cjs input.eac3 output.spdif');
  fs.writeFileSync(output, packEac3(fs.readFileSync(input)), { flag: 'wx' });
}
