'use strict';
// IEC61937 E-AC-3: Pd is BYTES, unlike the AC-3 burst's bit count.
// Windows carrier is little endian 16-bit; payload words require swapping.
const PERIOD = 24576;
const SYNC = Buffer.from([0x72, 0xf8, 0x1f, 0x4e]);
class Eac3Bursts {
  constructor(onPayload) { this.onPayload = onPayload; this.reset(); }
  reset() { this.pending = Buffer.alloc(0); this.skip = 0; }
  push(data) {
    // Bound copies regardless of caller's chunk size or hostile length codes.
    for (let offset = 0; offset < data.length; offset += PERIOD) {
      this.pending = Buffer.concat([this.pending, data.subarray(offset, offset + PERIOD)]);
      this.drain();
    }
  }
  drain() {
    while (this.pending.length) {
      if (this.skip) {
        const n = Math.min(this.skip, this.pending.length);
        this.pending = this.pending.subarray(n); this.skip -= n;
        if (this.skip) return;
      }
      const at = this.pending.indexOf(SYNC);
      if (at < 0) { this.pending = Buffer.from(this.pending.subarray(-3)); return; }
      this.pending = this.pending.subarray(at);
      if (this.pending.length < 8) return;
      const pc = this.pending.readUInt16LE(4), length = this.pending.readUInt16LE(6);
      const type = pc & 0x1f;
      // Null/pause bursts carry no decoder data. Search for the next real burst.
      if (type === 0 || type === 3) { this.pending = this.pending.subarray(8); continue; }
      if (type !== 0x15 || (pc & 0x80) || length < 6 || length > PERIOD - 8 || (length & 1)) {
        throw new Error(`Unsupported/corrupt IEC61937 burst: Pc=${pc}, Pd=${length}`);
      }
      if (this.pending.length < length + 8) return;
      const payload = Buffer.from(this.pending.subarray(8, 8 + length)).swap16();
      if (payload[0] !== 0x0b || payload[1] !== 0x77) throw new Error('E-AC-3 sync word missing');
      this.pending = this.pending.subarray(length + 8);
      this.skip = PERIOD - length - 8;
      this.onPayload(payload);
    }
  }
}

class CaptureRecords {
  constructor(onRecord) { this.onRecord = onRecord; this.pending = Buffer.alloc(0); }
  push(data) {
    for (let at = 0; at < data.length; at += 65536) {
      this.pending = Buffer.concat([this.pending, data.subarray(at, at + 65536)]);
      while (this.pending.length >= 120) {
        const h = this.pending;
        if (h.toString('ascii', 0, 4) !== 'SDAC' || h.readUInt32LE(4) !== 1) throw new Error('Capture protocol mismatch');
        const size = h.readUInt32LE(44), formatSize = h.readUInt32LE(48);
        if (size > 65536 - 120 || formatSize > 64 || h.readUInt32LE(40) > 3) throw new Error('Invalid capture record');
        if (h.length < 120 + size) break;
        const record = { epoch: h.readBigUInt64LE(8), offset: h.readBigUInt64LE(16), produced: h.readBigUInt64LE(24),
          overflows: h.readBigUInt64LE(32), state: h.readUInt32LE(40), format: Buffer.from(h.subarray(56, 56 + formatSize)),
          payload: Buffer.from(h.subarray(120, 120 + size)) };
        this.pending = h.subarray(120 + size);
        this.onRecord(record);
      }
    }
  }
  finish() { if (this.pending.length) throw new Error('Truncated capture record'); }
}

function classifyFormat(wave) {
  if (wave.length < 18 || wave.readUInt16LE(16) + 18 !== wave.length) throw new Error('Incomplete WAVEFORMATEX');
  const rate = wave.readUInt32LE(4);
  if (wave.readUInt16LE(2) !== 2 || wave.readUInt16LE(12) !== 4 || wave.readUInt16LE(14) !== 16 || wave.readUInt32LE(8) !== rate * 4)
    throw new Error('Unsupported carrier layout');
  if (wave.readUInt16LE(0) === 1 && rate === 48000 && wave.length === 18) return 'pcm16';
  if (wave.length < 40 || wave.readUInt16LE(0) !== 0xfffe) throw new Error('Expected WAVEFORMATEXTENSIBLE');
  const subtype = wave.readUInt32LE(24);
  const tail = wave.subarray(28, 40).toString('hex');
  if (subtype === 1 && rate === 48000 && tail === '00001000800000aa00389b71') return 'pcm16';
  if ((subtype === 0x0a || subtype === 0x10a) && rate === 192000 && tail === 'ea0c1000800000aa00389b71') {
    if (wave.length !== 40 && (wave.length !== 52 || wave.readUInt32LE(40) !== 48000 || wave.readUInt32LE(44) !== 6))
      throw new Error('Unsupported encoded content descriptor');
    return 'eac3';
  }
  throw new Error('This receiver currently supports 48k stereo PCM and DD+ IEC61937 only');
}
module.exports = { Eac3Bursts, CaptureRecords, classifyFormat, PERIOD };
