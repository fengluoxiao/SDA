'use strict';
const { Eac3Bursts, classifyFormat, pcmFormat } = require('./iec61937.cjs');

// No audio device/UI ownership here. Both replay tests and the live receiver
// use this exact decoder, including format validation and epoch resets.
class SystemDecoder {
  constructor({ createDecoder, onFrame, onReset = () => {}, onState = () => {}, onDiagnostic = () => {}, discreteLayout, inputMode = 'auto' }) {
    if (!['auto', 'bitstream'].includes(inputMode)) throw Error('Invalid system input mode');
    Object.assign(this, { createDecoder, onFrame, onReset, onState, onDiagnostic, discreteLayout, inputMode });
    this.epoch = null; this.decoder = null; this.state = -1;
  }
  accept(record) {
    if (record.produced < record.offset + BigInt(record.payload.length)) throw new Error('Invalid capture byte clock');
    if (this.epoch !== null && record.epoch < this.epoch) throw new Error('Stale capture epoch');
    if (record.epoch !== this.epoch) {
      this.close(); this.epoch = record.epoch; this.offset = 0n; this.sample = 0;
      this.format = Buffer.from(record.format); this.pcm = Buffer.alloc(0);
      this.kind = record.format.length ? classifyFormat(record.format, this.discreteLayout) : null;
      this.pcmInfo = this.kind && this.kind !== 'eac3' ? pcmFormat(record.format, this.discreteLayout) : null;
      this.blocked = this.inputMode === 'bitstream' && this.kind !== null && this.kind !== 'eac3';
      this.onReset({ epoch: record.epoch, kind: this.kind, blocked: this.blocked, overflows: record.overflows });
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
    // Keep listening for a new encoded stream, but never present the compatible
    // PCM mix as a successful Atmos decode in explicit bitstream mode.
    if (this.blocked) return;
    if (this.kind === 'eac3') this.bursts.push(record.payload);
    else {
      this.pcm = Buffer.concat([this.pcm, record.payload]);
      const info = this.pcmInfo, n = Math.floor(this.pcm.length / info.stride);
      if (!n) return;
      const channels = Array.from({length:info.count},()=>new Float32Array(n));
      for (let i = 0; i < n; i++) for (let c = 0; c < info.count; c++) {
        const at = i * info.stride + c * info.bits / 8;
        const value = info.float ? this.pcm.readFloatLE(at) : this.pcm.readIntLE(at, info.bits / 8) / 2 ** (info.bits - 1);
        if (!Number.isFinite(value)) throw Error('Non-finite PCM sample');
        channels[c][i] = value;
      }
      this.onFrame({ codec: 'pcm', sampleRate: info.rate, samplePos: this.sample, channels,
        labels: info.labels, objectChannels: [], events: [], programLoudness: null });
      this.sample += n; this.pcm = Buffer.from(this.pcm.subarray(n * info.stride));
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
