'use strict';
const { Eac3Bursts, classifyFormat } = require('./iec61937.cjs');

// No audio device/UI ownership here. Both replay tests and the live receiver
// use this exact decoder, including format validation and epoch resets.
class SystemDecoder {
  constructor({ createDecoder, onFrame, onReset = () => {}, onState = () => {}, onDiagnostic = () => {} }) {
    Object.assign(this, { createDecoder, onFrame, onReset, onState, onDiagnostic });
    this.epoch = null; this.decoder = null; this.state = -1;
  }
  accept(record) {
    if (record.produced < record.offset + BigInt(record.payload.length)) throw new Error('Invalid capture byte clock');
    if (this.epoch !== null && record.epoch < this.epoch) throw new Error('Stale capture epoch');
    if (record.epoch !== this.epoch) {
      this.close(); this.epoch = record.epoch; this.offset = 0n; this.sample = 0;
      this.format = Buffer.from(record.format); this.pcm = Buffer.alloc(0);
      this.kind = record.format.length ? classifyFormat(record.format) : null;
      this.onReset({ epoch: record.epoch, kind: this.kind, overflows: record.overflows });
      if (this.kind === 'eac3') {
        this.decoder = this.createDecoder('eac3');
        this.bursts = new Eac3Bursts(bytes => { this.decoder.push(bytes); this.drain(); });
      }
    }
    if (!this.format.equals(record.format)) throw new Error('Format changed without a new epoch');
    if (record.offset !== this.offset) throw new Error('Capture data gap: refusing to splice compressed audio');
    this.offset += BigInt(record.payload.length);
    if (record.state !== this.state) { this.state = record.state; this.onState(record.state); }
    if (!record.payload.length) return;
    if (record.state !== 3 || !this.kind) throw new Error('Audio received outside running state');
    if (this.kind === 'eac3') this.bursts.push(record.payload);
    else {
      this.pcm = Buffer.concat([this.pcm, record.payload]);
      const n = this.pcm.length >> 2;
      if (!n) return;
      const channels = [new Float32Array(n), new Float32Array(n)];
      for (let i = 0; i < n; i++) for (let c = 0; c < 2; c++) channels[c][i] = this.pcm.readInt16LE(i * 4 + c * 2) / 32768;
      this.onFrame({ codec: 'pcm', sampleRate: 48000, samplePos: this.sample, channels,
        labels: ['L', 'R'], objectChannels: [], events: [], programLoudness: null });
      this.sample += n; this.pcm = Buffer.from(this.pcm.subarray(n * 4));
    }
  }
  drain() {
    for (const message of this.decoder.drainErrors()) this.onDiagnostic(message);
    for (let f; (f = this.decoder.nextFrame());) {
      try {
        if (f.sampleRate !== 48000) throw new Error('Content rate is not 48 kHz; refusing to change playback pitch');
        this.onFrame({ codec: f.codec, sampleRate: f.sampleRate, samplePos: f.samplePos,
          channels: Array.from({ length: f.channelCount }, (_, i) => f.channel(i)), labels: f.labels,
          objectChannels: JSON.parse(f.objectChannelsJson), events: JSON.parse(f.eventsJson),
          programLoudness: JSON.parse(f.programLoudnessJson) });
      } finally { f.free(); }
    }
  }
  finish() { if (this.decoder) { this.decoder.flush(); this.drain(); } }
  close() { this.decoder?.free(); this.decoder = null; }
}
module.exports = { SystemDecoder };
