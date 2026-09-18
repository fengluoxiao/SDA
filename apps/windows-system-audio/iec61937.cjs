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

          returnStreams: h.readUInt32LE(52), overflows: h.readBigUInt64LE(32), state: h.readUInt32LE(40), format: Buffer.from(h.subarray(56, 56 + formatSize)),

          payload: Buffer.from(h.subarray(120, 120 + size)) };

        this.pending = h.subarray(120 + size);

        this.onRecord(record);

      }

    }

  }

  finish() { if (this.pending.length) throw new Error('Truncated capture record'); }

}



function pcmFormat(wave, discreteLayout) {

  if (wave.length < 18 || wave.readUInt16LE(16) + 18 !== wave.length) throw Error('Incomplete WAVEFORMATEX');

  const tag = wave.readUInt16LE(0), count = wave.readUInt16LE(2), rate = wave.readUInt32LE(4), bits = wave.readUInt16LE(14);

  const ext = tag === 0xfffe;

  if (ext && wave.length !== 40) return null;

  const sub = ext ? wave.readUInt32LE(24) : tag;

  if (![1,3].includes(sub) || (ext && wave.subarray(28,40).toString('hex') !== '00001000800000aa00389b71')) return null;

  if (rate !== 48000 || count < 1 || count > 24 || ![16,24,32].includes(bits) || (sub === 3 && bits !== 32)) throw Error('Unsupported PCM format: expected 48 kHz, 1-24 channels, 16/24/32-bit');

  const stride = count * bits / 8;

  if (wave.readUInt16LE(12) !== stride || wave.readUInt32LE(8) !== rate * stride) throw Error('Invalid PCM block alignment');

  const mask = ext ? wave.readUInt32LE(20) : count === 1 ? 4 : count === 2 ? 3 : 0;

  const names = ['L','R','C','LFE','Lb','Rb','WideLeft','WideRight','RearCenter','Ls','Rs','TopCenter','TopFrontLeft','TopFrontCenter','TopFrontRight','TopRearLeft','TopRearCenter','TopRearRight'];

  const labels = mask === 0 && ext
    ? require('./layouts.json')[discreteLayout]?.slice()
    : names.filter((_,i) => mask & (1 << i));
  if (!labels) throw Error('Discrete PCM requires an explicit input layout');

  if ((mask >>> names.length) || labels.length !== count) throw Error('PCM channel mask does not match channel count');

  const valid = ext ? wave.readUInt16LE(18) : bits;

  if (valid < 1 || valid > bits || (sub === 3 && valid !== 32)) throw Error('Invalid PCM valid bits');

  return { count, rate, bits, stride, float: sub === 3, labels };

}

function classifyFormat(wave, discreteLayout) {

  if (pcmFormat(wave, discreteLayout)) return 'pcm16'; // Legacy kind name retained; actual sample type is parsed separately.

  if (wave.length < 40 || wave.readUInt16LE(0) !== 0xfffe) throw Error('Expected WAVEFORMATEXTENSIBLE');

  const rate = wave.readUInt32LE(4);

  if (wave.readUInt16LE(2) !== 2 || wave.readUInt16LE(12) !== 4 || wave.readUInt16LE(14) !== 16 || wave.readUInt32LE(8) !== rate * 4) throw Error('Unsupported carrier layout');

  const subtype = wave.readUInt32LE(24), tail = wave.subarray(28,40).toString('hex');

  if ((subtype === 0x0a || subtype === 0x10a) && rate === 192000 && tail === 'ea0c1000800000aa00389b71') {

    if (wave.length !== 40 && (wave.length !== 52 || wave.readUInt32LE(40) !== 48000 || ![2,6,8].includes(wave.readUInt32LE(44)))) throw Error('Unsupported encoded content descriptor');

    return 'eac3';

  }

  throw Error('This receiver supports 48k multichannel PCM and DD+ IEC61937');

}

module.exports = { Eac3Bursts, CaptureRecords, classifyFormat, pcmFormat, PERIOD };
