'use strict';
const net = require('node:net');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn } = require('node:child_process');

class SystemAudio {
  constructor({ root, command, batch, publish }) {
    Object.assign(this, { root, command, batch, publish });
    this.speakerMonitor = {names:[],focus:[]};
    this.status = { active: false, phase: 'stopped', detail: '未启动', frames: 0, objects: 0 };
  }
  setSpeakerMonitor(names, focus) { this.speakerMonitor = {names:[...names],focus:[...focus]}; }
  update(value) { Object.assign(this.status, value); this.publish({ ...this.status }); return { ...this.status }; }
  async stop() {
    const session = this.session; this.session = null;
    if (session) {
      await this.command({ type: 'setSystemLoopback', address: null, token: null });
      session.returnSocket?.destroy(); session.returnServer?.close();
      clearTimeout(session.timer); session.socket?.destroy(); session.server.close();
      await session.work?.catch(() => {});
      await this.command({ type: 'reset', origin: 0 });
    }
    return this.update({ active: false, phase: 'stopped', detail: '已停止' });
  }
  async start(discreteLayout, inputMode = 'auto') {
    if (this.session) return { ...this.status };
    if (!['auto', 'bitstream'].includes(inputMode)) throw Error('无效的系统音频输入模式');
    if (discreteLayout !== undefined && !Object.hasOwn(require('../windows-system-audio/layouts.json'), discreteLayout)) throw Error('无效的离散输入布局');
    const { CaptureRecords } = require('../windows-system-audio/iec61937.cjs');
    const { SystemDecoder } = require('../windows-system-audio/decoder.cjs');
    const { SdaDecoder } = require('../../packages/core/pkg-node/sda_core.cjs');
    const token = crypto.randomBytes(32).toString('hex');
    const pipeName = `sda-system-${crypto.randomBytes(16).toString('hex')}`;
    const session = { server: net.createServer(), socket: null, discreteLayout, inputMode };
    this.session = session;
    const fail = async error => {
      if (this.session !== session) return;
      await this.stop(); this.update({ phase: 'error', detail: String(error.message || error) });
    };
    session.server.on('error', fail);
    session.server.on('connection', socket => {
      if (session.socket || this.session !== session) { socket.destroy(); return; }
      socket.on('error', () => {});
      let header = Buffer.alloc(0);
      const authenticate = data => {
        if (this.session !== session || session.socket) { socket.destroy(); return; }
        header = Buffer.concat([header, data]);
        const end = header.indexOf(10);
        if (end < 0) { if (header.length > 64) socket.destroy(); return; }
        if (header.subarray(0, end).toString() !== token) { socket.destroy(); return; }
        socket.removeListener('data', authenticate); socket.pause();
        session.socket = socket; clearTimeout(session.timer);
        if (header.length > end + 1) socket.unshift(header.subarray(end + 1));
        void this.command({ type: 'setSystemLoopback', address: session.returnAddress, token }).then(ok => { if (!ok) void fail(Error('原生渲染器不支持输出回送，请更新')); });
        this.update({ phase: 'waiting', detail: '已连接，等待播放器输入' });
        session.work = this.consume(session, socket, CaptureRecords, SystemDecoder, SdaDecoder);
        session.work.then(() => fail(Error('系统音频读取器已断开')), fail);
      };
      socket.on('data', authenticate);
    });
    await new Promise((resolve, reject) => { session.server.once('error', reject); session.server.listen(`\\\\.\\pipe\\${pipeName}`, resolve); });
    await this.prepareReturn(session, token, fail);
    this.update({ active: true, phase: 'authorizing', detail: '请在 UAC 弹窗中允许读取系统音频', frames: 0, objects: 0, inputMode, inputChannels: [], codec: null });
    session.timer = setTimeout(() => void fail(Error('管理员授权或驱动连接超时，请重试')), 120000);
    const quote = s => `'${s.replace(/'/g, "''")}'`;
    const script = path.join(__dirname, 'system-audio-reader.ps1');
    const capture = path.join(this.root, 'tools/windows-audio-probe/target/debug/capture.exe');
    const args = `-NoProfile -File "${script}" -PipeName ${pipeName} -Token ${token} -CapturePath "${capture}"`;
    const code = `$ErrorActionPreference='Stop'; Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -ArgumentList ${quote(args)} -Wait`;
    const launcher = spawn('powershell.exe', ['-NoProfile', '-EncodedCommand', Buffer.from(code, 'utf16le').toString('base64')], { windowsHide: true, stdio: 'ignore' });
    launcher.on('error', fail);
    launcher.on('exit', code => { if (!session.socket) void fail(Error(code ? 'UAC 已取消或读取器启动失败' : '读取器未连接')); });
    return { ...this.status };
  }
  async prepareReturn(session, token, fail) {
    session.returnServer = net.createServer(socket => {
      socket.on('error', () => {});
      let header=Buffer.alloc(0), authenticated=false;
      socket.setTimeout(5000, () => socket.destroy());
      socket.on('data', data => {
        if (this.session!==session) {socket.destroy();return;}
        if (!authenticated) {
          header=Buffer.concat([header,data]); const end=header.indexOf(10);
          if(end<0){if(header.length>64)socket.destroy();return;}
          if(end!==64 || header.subarray(0,end).toString()!==token || session.returnSocket){socket.destroy();return;}
          authenticated=true; socket.setTimeout(0); session.returnSocket=socket;
          data=header.subarray(end+1);header=null;
        }
        if (!data.length) return;
        // Independent bounded return; never back up the render/audio callback.
        if (!session.socket || session.socket.destroyed || session.socket.writableLength>65536) {
          void fail(Error('输出回送连接阻塞'));return;
        }
        session.socket.write(data);
      });
      socket.on('close', () => { if(authenticated && this.session===session) void fail(Error('原生输出回送已断开')); });
    });
    session.returnServer.on('error',fail);
    await new Promise((resolve,reject)=>{session.returnServer.once('error',reject);session.returnServer.listen(0,'127.0.0.1',resolve);});
    session.returnAddress=`127.0.0.1:${session.returnServer.address().port}`;
  }
  async consume(session, socket, CaptureRecords, SystemDecoder, SdaDecoder) {
    const steps = [], sources = new Map(); let started = false, lastPublished = 0, peaks = [];
    const checked = async command => { if (!await this.command(command)) throw Error(`渲染器拒绝 ${command.type}`); };
    const decoder = new SystemDecoder({ createDecoder: codec => new SdaDecoder(codec), discreteLayout: session.discreteLayout, inputMode: session.inputMode,
      onReset: info => steps.push({ reset: info }),
      onFrame: frame => steps.push({ frame: { ...frame, channels: frame.channels.map(c => new Float32Array(c)) } }),
      onDiagnostic: message => { throw Error(message); },
    });
    const records = new CaptureRecords(record => { this.status.returnStreams = record.returnStreams ?? 0; decoder.accept(record); });
    try {
      for await (const chunk of socket) {
        if (this.session !== session) break;
        records.push(chunk);
        for (const step of steps.splice(0)) {
          if (this.session !== session) return;
          if (step.reset) {
            await checked({ type: 'reset', origin: 0 });
            await checked({ type: 'setSpeakerMutes', ...this.speakerMonitor });
            sources.clear(); started = false; peaks = [];
            this.update({ phase: 'waiting', detail: step.reset.blocked
              ? '收到的是 PCM，尚未收到 DD+ 码流。请在播放器开启 E-AC-3 / DD+ 直通，并选择 SDA 的 WASAPI 输出。'
              : step.reset.kind === 'eac3' ? '已识别 DD+ 码流，正在解码'
              : step.reset.kind ? '正在缓冲输入' : '等待播放器输入', objects: 0, inputChannels: [], codec: null });
            continue;
          }
          const f = step.frame;
          for (let c=0;c<f.channels.length;c++) {
            let peak=peaks[c]??0;
            for(const value of f.channels[c]) peak=Math.max(peak,Math.abs(value));
            peaks[c]=peak;
          }
          const declared = new Map(f.objectChannels.map(o => [o.channel, o.id]));
          const ids = f.labels.map((label, i) => {
            const object = declared.get(i) ?? (/^Obj_(\d+)$/.exec(label)?.[1]);
            return object === undefined ? `bed:${i}` : `obj:${object}`;
          });
          if (new Set(ids).size !== ids.length) throw Error('对象通道映射重复');
          for (const id of sources.keys()) if (!ids.includes(id)) { await checked({ type: 'removeSource', id, at: f.samplePos }); sources.delete(id); }
          for (let i = 0; i < ids.length; i++) if (sources.get(ids[i]) !== f.labels[i]) {
            await checked({ type: 'addSource', id: ids[i], at: f.samplePos, ...(ids[i].startsWith('bed:') ? { bedLabel: f.labels[i] } : {}) });
            sources.set(ids[i], f.labels[i]);
          }
          if (f.programLoudness) await checked({ type: 'setProgramGain', gain: 10 ** (f.programLoudness.gainDb / 20), at: f.samplePos });
          const result = await this.batch(f.samplePos, ids.map((id, i) => ({ id, samples: f.channels[i] })), f.events);
          if (!result.accepted) throw Error(result.reason || '渲染队列拒绝输入');
          if (!started && f.samplePos + f.channels[0].length >= 12000) { await checked({ type: 'startAt', origin: 0 }); started = true; }
          // Decoder channel declarations may be emitted only when they change.
          // Count active mapped sources, not declarations in the current frame.
          this.status.frames++; this.status.objects = ids.filter(id => id.startsWith('obj:')).length;
          if (Date.now() - lastPublished > 250) {
            lastPublished = Date.now();
            this.update({ phase: 'receiving', codec: f.codec, detail: f.codec === 'pcm' ? `正在接收 ${f.channels.length} 声道 PCM`
              : this.status.objects ? `正在渲染 DD+ Atmos · ${this.status.objects} 个对象`
              : '正在渲染 DD+ · 当前帧无对象', inputChannels:f.labels.map((label,c)=>({label,peak:peaks[c]??0})) });
            peaks=[];
          }
        }
      }
    } finally { decoder.close(); }
  }
}
module.exports = { SystemAudio };
