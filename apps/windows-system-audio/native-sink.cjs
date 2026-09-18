'use strict';
const { spawn } = require('node:child_process');
const path = require('node:path');

class NativeSink {
  constructor({ root, outputDevice, volume = 0.5 }) {
    if (!outputDevice || /SdaSystemAudio|SDA Spatial Bitstream/i.test(outputDevice)) throw new Error('Select an explicit physical output device');
    this.pending = new Map(); this.sources = new Map(); this.started = false; this.volume = volume; this.outputDevice = outputDevice;
    this.child = spawn(path.join(root, 'apps/desktop/native-renderer/SdaNativeRenderer.exe'), [], {
      windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, SDA_HRTF_ROOT: path.join(root, 'apps/web/public'),
        SDA_OUTPUT_SETTINGS: JSON.stringify({ deviceId: outputDevice, exclusive: false, remoteCompatible: false }) },
    });
    let lines = '';
    this.child.stdout.on('data', data => {
      lines += data;
      if (lines.length > 4 * 1024 * 1024) return this.fail(Error('Oversized renderer response'));
      let i;
      while ((i = lines.indexOf('\n')) >= 0) {
        const line = lines.slice(0, i); lines = lines.slice(i + 1);
        let event; try { event = JSON.parse(line); } catch { continue; }
        const key = event.type === 'ack' ? event.command : event.type === 'batchAck' ? `batch:${event.start}` : event.type;
        if (this.pending.has(key)) {
          const { resolve, reject, timer } = this.pending.get(key); this.pending.delete(key); clearTimeout(timer);
          if (event.accepted === false) reject(Error(JSON.stringify(event))); else resolve(event);
        }
      }
    });
    this.child.stderr.on('data', data => process.stderr.write(data));
    this.child.stdin.on('error', error => this.fail(error));
    this.child.on('error', error => this.fail(error));
    this.child.on('exit', code => this.fail(Error(`Native renderer exited: ${code}`)));
    this.ready = this.wait('ready');
  }
  fail(error) { this.failure = error; for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(error); } this.pending.clear(); }
  wait(key) {
    if (this.failure) return Promise.reject(this.failure);
    return new Promise((resolve, reject) => {
      if (this.pending.has(key)) return reject(Error(`Duplicate renderer request: ${key}`));
      const timer = setTimeout(() => { this.pending.delete(key); reject(Error(`Renderer timeout: ${key}`)); }, 10000);
      this.pending.set(key, { resolve, reject, timer });
    });
  }
  async command(value) {
    const response = this.wait(value.type), data = Buffer.from(JSON.stringify(value)), h = Buffer.alloc(5);
    h[0] = 74; h.writeUInt32LE(data.length, 1); this.child.stdin.write(Buffer.concat([h, data])); return response;
  }
  async initialize() {
    const ready = await this.ready;
    if (ready.protocol !== 7 || (ready.sample_rate ?? ready.sampleRate) !== 48000) throw new Error('Need native renderer protocol 7 at 48 kHz');
    // Reject a driver endpoint by its resolved name, including opaque WASAPI IDs.
    const response = this.wait('outputDevices');
    await this.command({ type: 'listOutputDevices' });
    const devices = await response;
    if (devices.status?.actualId !== this.outputDevice) throw new Error('Requested output was not opened; refusing silent fallback');
    if (/SDA Spatial Bitstream/i.test(devices.status?.actualName ?? '')) throw new Error('Output would feed back into the system input');
    await this.command({ type: 'setHrtf', set: 'hrtf', wetWeight: 0.04 });
    await this.command({ type: 'setVolume', volume: this.volume });
    await this.command({ type: 'setObjectHrtf', enabled: true });
    await this.command({ type: 'setDirectionalHrtf', enabled: true });
  }
  async reset() { await this.command({ type: 'reset', origin: 0 }); this.sources.clear(); this.started = false; this.lastEnd = 0; }
  async health() {
    const response = this.wait('health');
    const data = Buffer.from('{"type":"health"}'), h = Buffer.alloc(5); h[0] = 74; h.writeUInt32LE(data.length, 1);
    this.child.stdin.write(Buffer.concat([h, data])); return response;
  }
  async frame(f) {
    // Bound render lookahead; a replay file may arrive much faster than real time.
    const deadline = Date.now() + 10000;
    while (this.started && f.samplePos - (await this.health()).samplePos > 48000) {
      if (Date.now() > deadline) throw new Error('Renderer stopped consuming system audio');
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    const declared = new Map(f.objectChannels.map(o => [o.channel, o.id]));
    const ids = f.labels.map((label, channel) => {
      const match = /^Obj_(\d+)$/.exec(label);
      const object = declared.get(channel) ?? (match ? Number(match[1]) : null);
      return object === null ? `bed:${channel}` : `obj:${object}`;
    });
    if (ids.length !== f.channels.length || new Set(ids).size !== ids.length) throw new Error('Invalid decoded source mapping');
    for (const id of this.sources.keys()) if (!ids.includes(id)) {
      await this.command({ type: 'removeSource', id, at: f.samplePos }); this.sources.delete(id);
    }
    for (let i = 0; i < ids.length; i++) if (this.sources.get(ids[i]) !== f.labels[i]) {
      await this.command({ type: 'addSource', id: ids[i], at: f.samplePos, ...(ids[i].startsWith('bed:') ? { bedLabel: f.labels[i] } : {}) });
      this.sources.set(ids[i], f.labels[i]);
    }
    if (f.programLoudness) await this.command({ type: 'setProgramGain', gain: 10 ** (f.programLoudness.gainDb / 20), at: f.samplePos });
    const events = Buffer.from(JSON.stringify(f.events)), h = Buffer.alloc(15);
    h[0] = 70; h.writeUInt32LE(events.length, 1); h.writeBigUInt64LE(BigInt(f.samplePos), 5); h.writeUInt16LE(ids.length, 13);
    // F = events length, events JSON, start, source count, planar source blocks.
    const parts = [h.subarray(0, 5), events, h.subarray(5)];
    for (let i = 0; i < ids.length; i++) {
      const id = Buffer.from(ids[i]), pcm = f.channels[i], header = Buffer.alloc(6);
      header.writeUInt16LE(id.length); header.writeUInt32LE(pcm.length, 2);
      parts.push(header, id, Buffer.from(pcm.buffer, pcm.byteOffset, pcm.byteLength));
    }
    const response = this.wait(`batch:${f.samplePos}`); this.child.stdin.write(Buffer.concat(parts)); await response;
    this.lastEnd = f.samplePos + f.channels[0].length;
    if (!this.started && f.samplePos + f.channels[0].length >= 12000) {
      await this.command({ type: 'startAt', origin: 0 }); this.started = true;
    }
  }
  async drain() {
    if (!this.lastEnd) return;
    if (!this.started) { await this.command({ type: 'startAt', origin: 0 }); this.started = true; }
    const deadline = Date.now() + 5000;
    // The native callback reports the consumed codec position, not IPC receipt.
    while ((await this.health()).samplePos < this.lastEnd) {
      if (Date.now() > deadline) throw new Error('Renderer did not drain the final audio');
      await new Promise(resolve => setTimeout(resolve, 10));
    }
  }
  async close() {
    try { if (!this.failure) await this.command({ type: 'shutdown' }); }
    finally { this.child.stdin.end(); }
  }
}
module.exports = { NativeSink };
