"use strict";
const tls = require("node:tls");
const net = require("node:net");
const crypto = require("node:crypto");
const os = require("node:os");
const { spawn } = require("node:child_process");
const { createRemoteCertificate } = require("./remote-certificate.cjs");
const { createRemoteWeb } = require("./remote-web-server.cjs");
const cinemaProfiles = require("./cinema-profiles.cjs");
const monitorSettings = require("./monitor-settings.cjs");
const {validateConfig:validateRoomConfig} = require("./room-lab.cjs");

const FRAMES = 480, AUDIO_BYTES = FRAMES * 8, MAX_PACKET = 256 * 1024;
const TLS_OPTIONS = { minVersion: "TLSv1.2" };
function packet(kind, data = Buffer.alloc(0)) {
  const body = Buffer.isBuffer(data) ? data : Buffer.from(JSON.stringify(data));
  const head = Buffer.alloc(5); head[0] = kind.charCodeAt(0); head.writeUInt32LE(body.length, 1);
  return Buffer.concat([head, body]);
}
function decodePackets(onPacket) {
  let pending = Buffer.alloc(0);
  return chunk => {
    pending = pending.length ? Buffer.concat([pending, chunk]) : chunk;
    while (pending.length >= 5) {
      const length = pending.readUInt32LE(1);
      if (length > MAX_PACKET) throw Error("远程数据包超过上限");
      if (pending.length < 5 + length) break;
      const kind = String.fromCharCode(pending[0]), body = pending.subarray(5, 5 + length);
      pending = pending.subarray(5 + length); onPacket(kind, body);
    }
    if (pending.length > MAX_PACKET + 5) throw Error("远程缓冲超过上限");
  };
}
function readJson(body) {
  const value = JSON.parse(body.toString("utf8"));
  if (!value || typeof value !== "object" || Array.isArray(value)) throw Error("无效远程消息");
  return value;
}
function parseInvite(invite) {
  let url;
  try { url = new URL(invite); } catch { throw Error("请粘贴主机生成的完整配对地址"); }
  if (url.protocol !== "sda:" || !url.hostname || !/^\d+$/.test(url.port) || +url.port < 1 || +url.port > 65535 ||
      !/^#[a-f0-9]{64}\.[a-f0-9]{64}$/.test(url.hash) || url.username || url.password || url.search || (url.pathname && url.pathname !== "/")) {
    throw Error("配对地址格式无效");
  }
  const [key, fingerprint] = url.hash.slice(1).split(".");
  return { host: url.hostname.replace(/^\[|\]$/g, ""), port: +url.port, key: Buffer.from(key, "hex"), fingerprint: Buffer.from(fingerprint, "hex") };
}
function validateControl(value) {
  if (!value || typeof value !== "object") throw Error("无效控制指令");
  const { action, value: arg } = value;
  if(action==="scene")return {action};
  if(action==="localMute"&&typeof arg==="boolean")return {action,value:arg};
  if(action==="artwork"&&typeof arg==="string"&&/^[a-f0-9]{64}$/.test(arg))return {action,value:arg};
  if(action==="mediaList"&&(arg==null||typeof arg==="string"&&/^[a-f0-9]{64}$/.test(arg)))return {action,value:arg??null};
  if(action==="mediaOpen"&&typeof arg==="string"&&/^[a-f0-9]{64}$/.test(arg))return {action,value:arg};
  if (["hrtfRename","hrtfCopy"].includes(action) && /^personal-[a-f0-9]{64}$/.test(arg?.id) && typeof arg?.name === "string" && arg.name.trim().length>0 && arg.name.length<=80) return {action,value:{id:arg.id,name:arg.name.trim()}};
  if (action === "roomGenerate") return {action,value:validateRoomConfig(arg)};
  if (action === "roomCancel") return {action};
  if (action === "hrtfTune" && typeof arg?.dense === "boolean" && typeof arg?.calibrated === "boolean") return {action,value:{dense:arg.dense,calibrated:arg.calibrated}};
  if (action === "hrtfGenerate" && arg?.parameters && [1,2].includes(arg.parameters.version) && JSON.stringify(arg).length < 48000) return {action,value:{parameters:arg.parameters,assessment:arg.assessment}};
  if (action === "roomApply" && typeof arg === "string" && /^[a-f0-9]{64}$/.test(arg)) return {action,value:arg};
  if (["roomDisable", "monitorAlign"].includes(action)) return {action};
  if (action === "roomSettings" && arg && typeof arg.expected === "string" && arg.expected.length <= 16000) {
    const {monitor, ...settings} = cinemaProfiles.validateSettings(arg.settings);
    return {action,value:{expected:arg.expected,settings}};
  }
  if (action === "monitorSettings" && arg && typeof arg.expected === "string" && arg.expected.length <= 16000) return {action,value:{expected:arg.expected,settings:monitorSettings.validate(arg.settings)}};
  if (["monitorPreset", "hardwarePreset"].includes(action) && typeof arg === "string" &&
      (action === "monitorPreset" ? ["transparent","bass-80"] : ["ahb2-high","ahb2-mid","ahb2-low"]).includes(arg)) return {action,value:arg};
  if (action === "hrtf" && typeof arg === "string" && (/^(ku100|d2|h([3-9]|1[0-9]|20))$/.test(arg) || /^personal-[a-f0-9]{64}$/.test(arg))) return {action,value:arg};
  if (["play", "pause", "next", "previous", "replay"].includes(action)) return { action };
  if (action === "track" && typeof arg === "string" && arg.length <= 160) return { action, value: arg };
  if (action === "volume" && Number.isFinite(arg) && arg >= 0 && arg <= 1) return { action, value: arg };
  if (action === "playbackMode" && ["sequence", "repeat-all", "repeat-one"].includes(arg)) return { action, value: arg };
  if (action === "stereoMode" && ["original", "dry", "room"].includes(arg)) return { action, value: arg };
  throw Error("不支持的远程控制指令");
}
function listen(server, port, host) {
  return new Promise((resolve, reject) => { server.once("error", reject); server.listen(port, host, () => { server.off("error", reject); resolve(server.address()); }); });
}
function authenticated(socket) {
  socket.pause();
  return new Promise((resolve,reject)=>{
    const cleanup=()=>{clearTimeout(timer);socket.off("readable",read);socket.off("close",closed);socket.off("error",failed);};
    const failed=()=>{cleanup();reject(Error("配对失败，请检查地址及密钥"));};
    const closed=()=>failed();
    const read=()=>{const byte=socket.read(1);if(!byte)return;cleanup();byte[0]===89?resolve():reject(Error("主机拒绝连接：已有客户端或配对信息无效"));};
    const timer=setTimeout(failed,5000);
    socket.on("readable",read);socket.once("close",closed);socket.once("error",failed);read();
  });
}
const addresses = () => [...new Set(Object.values(os.networkInterfaces()).flat().filter(v => v && !v.internal && v.family === "IPv4").map(v => v.address))];

class RemoteSession {
  constructor(hooks) {
    this.hooks = hooks; this.role = "off"; this.phase = "off"; this.detail = "未连接";
    this.peer = null; this.server = null; this.localServer = null; this.local = null; this.receiver = null;
    this.state = null; this.key = null; this.port = null; this.generation = 0; this.pendingControls = new Map();
    this.sockets = new Set(); this.bufferMs = 300; this.queued = 0; this.bytes = 0;
  }
  status() {
    return { role: this.role, phase: this.phase, detail: this.detail, peer: this.peer?.remoteAddress ?? null,
      port: this.port, addresses: this.role === "host" ? addresses() : [],
      invites: this.role === "host" && this.key ? addresses().map(a => this.invite(a)) : [],
      webInvites: this.role === "host" && this.key ? addresses().map(a => this.webInvite(a)) : [],
      format: this.peer?.header?.sampleFormat === "hls-flac24" ? "48 kHz · 24-bit FLAC · 双声道" : "48 kHz · 32-bit float PCM · 双声道", localMuted:this.hooks.localMuted?.()??true, bufferMs: this.bufferMs, queuedMs: this.queued / 48,
      bytes: this.bytes, output: this.output ?? null, state: this.role === "client" ? this.state : null };
  }
  invite(address) { return `sda://${address}:${this.port}#${this.key.toString("hex")}.${this.certificate.fingerprint}`; }
  webInvite(address) { return `https://${address}:${this.port}/#${this.key.toString("hex")}`; }
  publish() { this.hooks.status?.(this.status()); }
  track(socket) {
    this.sockets.add(socket); socket.on("close", () => this.sockets.delete(socket));
    socket.setNoDelay(true); socket.on("error", () => {});
    return socket;
  }
  async host({ port = 49632, bufferMs = 300, pairingKey = this.hooks.savedPairingKey?.() ?? "" } = {}) {
    if (this.role !== "off") throw Error("请先结束当前远程会话");
    if (!Number.isInteger(port) || port < 1024 || port > 65535) throw Error("端口须为 1024–65535");
    if (typeof pairingKey !== "string" || pairingKey.length > 256) throw Error("自定义密钥最多 256 个字符");
    const secret = pairingKey.trim();
    const sessionKey = !secret ? (this.defaultKey ??= this.hooks.defaultKey?.() ?? crypto.randomBytes(32)) : /^[a-f0-9]{64}$/.test(secret)
      ? Buffer.from(secret, "hex")
      : crypto.pbkdf2Sync(secret, "SDA remote pairing v1", 210000, 32, "sha256");
    this.bufferMs = [100, 300, 600, 1000].includes(bufferMs) ? bufferMs : 300;
    this.certificate = this.hooks.certificate?.() ?? createRemoteCertificate();
    this.role = "host"; this.phase = "starting"; this.key = sessionKey;
    this.generation++; const generation = this.generation;
    const key = this.key;
    const web = this.web = createRemoteWeb(this, {packet,decodePackets});
    const server = this.server = tls.createServer({ ...TLS_OPTIONS, ALPNProtocols:["http/1.1", "sda-pcm"], cert:this.certificate.cert, key:this.certificate.key, handshakeTimeout: 5000 }, socket => {
      this.track(socket);
      if (socket.alpnProtocol === "http/1.1") { web.accept(socket); return; }
      let auth=Buffer.alloc(0);
      const timer=setTimeout(()=>socket.destroy(),5000).unref();socket.once("close",()=>clearTimeout(timer));
      const authenticate=chunk=>{
        auth=Buffer.concat([auth,chunk]);if(auth.length<32)return;
        clearTimeout(timer);
        if(!crypto.timingSafeEqual(auth.subarray(0,32),key)){socket.destroy();return;}
        socket.pause();socket.off("data",authenticate);
        if (this.peer || this.role !== "host" || generation !== this.generation) {
          socket.end(packet("E", { error: "主机已有一个客户端连接" })); return;
        }
        if(auth.length>32)socket.unshift(auth.subarray(32));auth=Buffer.alloc(0);
        this.peer=socket;this.phase="connecting";this.detail="客户端已配对，正在连接音频";
        socket.write(Buffer.from("Y"));
        this.publish();void this.attachHost(socket,generation).catch(e=>this.failPeer(socket,e.message));socket.resume();
      };
      socket.on("data",authenticate);
    });
    server.on("connection", socket => {
      // Bound even unauthenticated sockets; possession of the invite is required
      // before control state, playlist or audio is sent.
      if (this.sockets.size >= 32) socket.destroy(); else this.track(socket);
    });
    server.on("tlsClientError", () => {});
    try { await listen(server, port, "0.0.0.0"); this.hooks.rememberPairingKey?.(secret); this.port = port; this.phase = "waiting"; this.detail = "等待浏览器或 SDA 客户端配对"; this.publish(); }
    catch (e) { await this.stop(); throw Error(`无法监听端口 ${port}：${e.code ?? e.message}`); }
    server.on("error", e => { this.detail = `远程监听失败：${e.code ?? e.message}`; this.publish(); });
    return this.status();
  }
  async attachHost(socket, generation) {
    await this.detaching;
    if(this.peer!==socket||this.generation!==generation)return;
    let granted = 0, sent = 0, consumed = 0, ready = false, initialReset = false, lastFeedback = Date.now();
    const windowFrames = this.bufferMs * 48;
    const grant = () => {
      if (!ready || !this.local || this.peer !== socket) return;
      const count = Math.floor((windowFrames - (granted - consumed)) / FRAMES);
      if (count > 0) { granted += count * FRAMES; this.local.write(Buffer.alloc(count, "P")); }
    };
    const incoming = decodePackets((kind, body) => {
      const message = readJson(body);
      if (kind === "H") {
        if (ready || message.protocol !== 1) throw Error("远程协议不兼容");
        ready = true; lastFeedback = Date.now(); if(initialReset){initialReset=false;socket.write(packet("R"));} grant();
      } else if (kind === "K") {
        if (!ready || !Number.isSafeInteger(message.consumed) || message.consumed < consumed || message.consumed > sent) throw Error("无效音频确认");
        consumed = message.consumed; lastFeedback = Date.now(); this.queued = sent - consumed; grant();
      } else if (kind === "C") {
        if (!ready || typeof message.id !== "string" || message.id.length > 64) throw Error("无效远程控制");
        if (this.pendingControls.size >= 16) { socket.write(packet("D", { id: message.id, error: "操作过于频繁，请稍候" })); return; }
        const command = validateControl(message.command);
        const id = crypto.randomUUID();
        const timer = setTimeout(() => this.completeControl(id, "主机没有响应此操作"), command.action === "roomGenerate" ? 610000 : 15000).unref();
        this.pendingControls.set(id, { socket, remoteId: message.id, timer });
        if(command.action==="artwork")this.completeControl(id,null,{src:command.value===this.artworkId?this.artwork:""});
        else if(command.action==="scene")this.completeControl(id,null,this.scene??null);
        else this.hooks.control?.({ id, ...command });
      } else if (kind === "Q") { socket.end(); }
      else throw Error("不支持的远程消息");
    });
    socket.on("data", chunk => { try { incoming(chunk); } catch (e) { this.failPeer(socket, e.message); } });
    socket.once("close", () => {
      if (this.peer !== socket) return;
      this.peer = null; this.local?.destroy(); this.local = null; this.localServer?.close(); this.localServer = null;
      this.phase = "waiting"; this.detail = socket.sdaFailure ? `${socket.sdaFailure}；本机继续播放` : "客户端已断开；本机继续播放"; this.queued = 0;
      this.detaching=Promise.resolve(this.hooks.disconnected?.()).catch(()=>{}); this.publish();
      // The receiver has its own output; local playback remains active.
    });
    const token = crypto.randomBytes(32).toString("hex");
    this.localServer?.close();
    const localServer = this.localServer = net.createServer(local => {
      this.track(local); local.setTimeout(5000, () => local.destroy());
      let handshake = Buffer.alloc(0), authenticated = false;
      const audio = decodePackets((kind, body) => {
        if (this.peer !== socket) return;
        if ((kind !== "A" || body.length !== AUDIO_BYTES) && (kind !== "R" || body.length !== 0)) throw Error("无效原生音频包");
        if(kind==="R"&&!ready){initialReset=true;return;}
        if (kind === "A") { sent += FRAMES; this.bytes += body.length; }
        if (socket.writableLength > 1024 * 1024) throw Error("远端网络持续阻塞，请重新连接");
        socket.write(packet(kind, body));
      });
      local.on("data", chunk => {
        try {
          if (!authenticated) {
            handshake = Buffer.concat([handshake, chunk]);
            if (handshake.length < 64) return;
            if (this.local || !crypto.timingSafeEqual(handshake.subarray(0, 64), Buffer.from(token))) throw Error("原生音频认证失败");
            authenticated = true; this.local = local; local.setTimeout(0); chunk = handshake.subarray(64); handshake = Buffer.alloc(0);
            grant();
          }
          audio(chunk);
        } catch (e) { local.destroy(); if (authenticated) this.failPeer(socket, e.message); }
      });
      local.once("close", () => { if (this.local === local && this.peer === socket) this.failPeer(socket, "主机原生音频连接中断"); });
    });
    const address = await listen(localServer, 0, "127.0.0.1");
    if (this.peer !== socket || generation !== this.generation) { localServer.close(); return; }
    if (!await this.hooks.route({ address: `127.0.0.1:${address.port}`, token })) throw Error("主机原生渲染器未能切换远程输出");
    if (this.peer !== socket) return;
    this.phase = "connected"; this.detail = "一对一无损发送中"; this.publish();
    socket.write(packet("H", { protocol: 1, sampleRate: 48000, channels: 2, sampleFormat: "f32le", bufferMs: this.bufferMs }));
    if (this.state) socket.write(packet("S", this.state));
    const timer = setInterval(() => {
      if (this.peer !== socket) return;
      if (Date.now() - lastFeedback > 15000) this.failPeer(socket, "远端超过 15 秒没有音频响应");
      else { socket.write(packet("T", {})); this.publish(); }
    }, 1000).unref();
    socket.once("close", () => clearInterval(timer));
  }
  failPeer(socket, detail) {
    if (this.peer !== socket) return;
    socket.sdaFailure = detail;
    this.detail = detail; this.publish();
    if (socket.writable) socket.write(packet("E", { error: detail })); socket.destroy();
  }
  completeControl(id, error = null, data = undefined) {
    const pending = this.pendingControls.get(id); if (!pending) return;
    clearTimeout(pending.timer); this.pendingControls.delete(id);
    if (!pending.socket.destroyed) pending.socket.write(packet("D", { id: pending.remoteId, error: error ? String(error).slice(0, 300) : null, ...(data === undefined ? {} : {data}) }));
  }
  publishScene(scene) {
    if(!scene||!Array.isArray(scene.objects)||!Array.isArray(scene.layout)){this.scene=null;return;}
    const text=v=>typeof v==="string"?v.slice(0,160):"";
    const finite=(v,f=0)=>Number.isFinite(v)?Math.max(-100000,Math.min(100000,v)):f;
    const vector=v=>Array.isArray(v)&&v.length===3?v.map(x=>finite(x)):[0,0,0];
    const ids=v=>Array.isArray(v)?v.filter(Number.isInteger).slice(0,256):[];
    this.scene={trackId:text(scene.trackId),position:finite(scene.position),
      objects:scene.objects.slice(0,256).filter(o=>o&&Number.isInteger(o.id)).map(o=>({id:o.id,pos:vector(o.pos),size:vector(o.size),hasPos:!!o.hasPos,gainDb:finite(o.gainDb),anchor:["room","screen","speaker"].includes(o.anchor)?o.anchor:"room",distanceM:Number.isFinite(o.distanceM)?finite(o.distanceM):null,distanceInfinite:!!o.distanceInfinite})),
      layout:scene.layout.slice(0,64).filter(Boolean).map(s=>({name:text(s.name),azimuth:finite(s.azimuth),elevation:finite(s.elevation),distance:finite(s.distance,1),isLfe:!!s.isLfe})),
      muted:ids(scene.muted),sounding:ids(scene.sounding),hiddenSpeakers:Array.isArray(scene.hiddenSpeakers)?scene.hiddenSpeakers.slice(0,64).map(text):[]};
  }
  publishState(state) {
    // Only curated UI data, never file paths, file handles, artwork data URLs or
    // arbitrary renderer objects, cross this interface.
    if (!state || typeof state !== "object") return;
    const artwork=typeof state.artwork==="string"&&state.artwork.length<180000&&/^data:image\/jpeg;base64,[A-Za-z0-9+/=]+$/.test(state.artwork)?state.artwork:"";
    if(artwork!==this.artwork){this.artwork=artwork;this.artworkId=artwork?crypto.createHash("sha256").update(artwork).digest("hex"):"";}
    const text = v => typeof v === "string" ? v.slice(0, 200) : "";
    const source=state.source;const count=v=>Number.isInteger(v)&&v>0&&v<=1000000?v:0;
    this.state = { source:source&&typeof source==="object"?{codec:text(source.codec),sampleRate:count(source.sampleRate),channels:count(source.channels),objects:count(source.objects)}:null, localMuted:this.hooks.localMuted?.()??true, artworkId:this.artworkId||"", title: text(state.title), artist:text(state.artist), album:text(state.album), playing: !!state.playing, paused: !!state.paused,
      position: Number.isFinite(state.position) ? Math.max(0, state.position) : 0,
      duration: Number.isFinite(state.duration) ? Math.max(0, state.duration) : 0,
      volume: Number.isFinite(state.volume) ? Math.min(1, Math.max(0, state.volume)) : 1,
      currentId: text(state.currentId), playbackMode: text(state.playbackMode), stereoMode: text(state.stereoMode),
      tools: this.curateTools(state.tools),
      playlist: Array.isArray(state.playlist) ? state.playlist.slice(0, 500).map(v => ({ id: text(v.id), title: text(v.title) })) : [] };
    if (this.role === "host" && this.phase === "connected" && this.peer?.writableLength < MAX_PACKET) this.peer.write(packet("S", this.state));
  }
  curateTools(value) {
    if (!value) return null;
    try {
      const text = v => typeof v === "string" ? v.slice(0,200) : "";
      return {layout:text(value.layout),head:text(value.head),locked:!!value.locked,dense:!!value.dense,calibrated:!!value.calibrated,
        generator:{available:!!value.generator?.available,running:!!value.generator?.running,current:Number(value.generator?.current)||0,total:Number(value.generator?.total)||0},
        error:text(value.error),cinema:{profileId:text(value.cinema.profileId)||null,settings:cinemaProfiles.validateSettings(value.cinema.settings)},
        speakers:value.speakers.slice(0,16).map(v=>({name:text(v.name),label:text(v.label),az:Number(v.az)||0,el:Number(v.el)||0})),
        rooms:value.rooms.slice(0,100).map(v=>({id:text(v.id),name:text(v.name),layout:text(v.layout),builtin:!!v.builtin})),
        heads:value.heads.slice(0,100).map(v=>({id:text(v.id),name:text(v.name)}))};
    } catch { return null; }
  }
  async join({ invite, deviceId = null, exclusive = true }) {
    if (this.role !== "off") throw Error("请先结束当前远程会话");
    const target = parseInvite(invite);
    if (deviceId !== null && (typeof deviceId !== "string" || deviceId.length > 1024)) throw Error("输出设备无效");
    this.role = "client"; this.phase = "connecting"; this.detail = "正在连接主机"; this.state = null;
    this.generation++; const generation = this.generation; this.publish();
    try {
      const socket = this.peer = this.track(tls.connect({ ...TLS_OPTIONS, host: target.host, port: target.port,
        rejectUnauthorized: false })); // The exact certificate is pinned below, before sending the secret.
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => { socket.destroy(); reject(Error("连接超时，请检查主机地址、组网连接及防火墙 TCP 端口")); }, 10000);
        socket.once("secureConnect", () => {
          clearTimeout(timer);
          const raw=socket.getPeerCertificate()?.raw;
          if(!raw||!crypto.timingSafeEqual(crypto.createHash("sha256").update(raw).digest(),target.fingerprint)){
            socket.destroy();reject(Error("主机证书指纹不匹配，请重新复制配对地址"));return;
          }
          socket.write(target.key);resolve();
        });
        socket.once("error", e => { clearTimeout(timer); reject(Error(`无法连接或配对密钥不匹配：${e.code ?? "TLS error"}`)); });
        socket.once("close", () => { clearTimeout(timer); reject(Error("连接已关闭")); });
      });
      await authenticated(socket);
      if (generation !== this.generation) { socket.destroy(); return this.status(); }
      await this.hooks.prepareClient?.();
      if (generation !== this.generation || socket.destroyed) throw Error("连接已取消");
      const process = this.receiver = (this.hooks.spawnReceiver ?? spawn)(this.hooks.executable(), [], { windowsHide: true, stdio: ["pipe", "pipe", "pipe"],
        env: { ...global.process.env, SDA_REMOTE_RECEIVER: "1", SDA_OUTPUT_SETTINGS: JSON.stringify({ deviceId, exclusive: !!exclusive, remoteCompatible: false }) } });
      let line = "", hostReady = false, outputReady = false, announced = false, lastHost = Date.now();
      const announce = () => {
        if (hostReady && outputReady && !announced && this.peer === socket) {
          announced = true; socket.write(packet("H", { protocol: 1 })); this.phase = "connected"; this.detail = "无损收听中"; this.publish();
        }
      };
      process.stdout.setEncoding("utf8"); process.stdout.on("data", data => {
        line += data; if (line.length > MAX_PACKET) { this.failPeer(socket, "接收器状态异常"); return; }
        let end; while ((end = line.indexOf("\n")) >= 0) {
          const row = line.slice(0, end); line = line.slice(end + 1);
          try {
            const value = JSON.parse(row);
            if (value.type === "outputDevices") {
              this.output = value.status; outputReady = value.status.state === "ready";
              if (!outputReady) { this.failPeer(socket, value.status.detail || "远端输出设备不可用"); return; } announce(); this.publish();
            } else if (value.type === "remoteProgress" && announced) {
              this.queued = value.queued; this.detail = value.buffering ? "网络缓冲中 · PCM 不降质" : "无损收听中";
              if (!socket.destroyed) socket.write(packet("K", { consumed: value.consumed }));
            } else if (value.type === "error") this.failPeer(socket, value.detail);
          } catch { /* Non-protocol diagnostic line. */ }
        }
      });
      process.stderr.on("data", () => {}); process.stdin.on("error", () => this.failPeer(socket, "原生接收器管道关闭"));
      process.once("error", () => this.failPeer(socket, "无法启动内置音频接收器"));
      process.once("exit", () => { if (this.receiver === process && this.peer === socket) this.failPeer(socket, "原生接收器已退出"); });
      const incoming = decodePackets((kind, body) => {
        lastHost = Date.now();
        if (kind === "A" || kind === "R") {
          if (!announced || (kind === "A" ? body.length !== AUDIO_BYTES : body.length !== 0)) throw Error("无效无损音频格式");
          this.bytes += body.length;
          if (!process.stdin.write(packet(kind, body))) socket.pause();
        } else {
          const value = readJson(body);
          if (kind === "H") {
            if (hostReady || value.protocol !== 1 || value.sampleRate !== 48000 || value.channels !== 2 || value.sampleFormat !== "f32le") throw Error("主机音频格式不兼容");
            hostReady = true; this.bufferMs = value.bufferMs; announce();
          } else if (kind === "S") { this.state = value; this.publish(); }
          else if (kind === "D") { this.hooks.result?.(value); }
          else if (kind === "E") throw Error(String(value.error).slice(0, 300));
          else if (kind !== "T") throw Error("无效主机消息");
        }
      });
      process.stdin.on("drain", () => { if (this.peer === socket) socket.resume(); });
      socket.on("data", chunk => { try { incoming(chunk); } catch (e) { this.failPeer(socket, e.message); } });
      socket.resume();
      socket.once("close", () => {
        if (this.peer !== socket) return;
        this.peer = null; this.receiver = null; process.kill(); this.phase = "disconnected";
        if (this.detail === "无损收听中" || this.detail.startsWith("网络缓冲")) this.detail = "主机连接已断开";
        this.publish();
      });
      const timer = setInterval(() => { if (Date.now() - lastHost > 15000) this.failPeer(socket, "主机超过 15 秒没有响应"); this.publish(); }, 1000).unref();
      socket.once("close", () => clearInterval(timer));
      return this.status();
    } catch (e) { await this.stop(); this.phase="error"; this.detail=e.message; this.publish(); throw e; }
  }
  command(command) {
    if (this.role !== "client" || this.phase !== "connected" || !this.peer) throw Error("尚未连接主机");
    const id = crypto.randomUUID(); this.peer.write(packet("C", { id, command: validateControl(command) })); return id;
  }
  async stop() {
    const wasHost = this.role === "host"; this.generation++; this.role = "off"; this.phase = "off";
    this.peer = null; this.local = null;
    for (const socket of this.sockets) socket.destroy(); this.sockets.clear();
    const server=this.server;this.server=null;
    if(server)await new Promise(resolve=>server.close(()=>resolve())); this.localServer?.close(); this.localServer = null;
    this.web?.close(); this.web = null;
    const receiver = this.receiver; this.receiver = null; receiver?.kill();
    for (const item of this.pendingControls.values()) clearTimeout(item.timer); this.pendingControls.clear();
    await this.detaching;
    if (wasHost) { await this.hooks.route({ address: null, token: null }); }
    this.key = null; this.certificate=null; this.port = null; this.queued = 0; this.bytes = 0; this.output = null; this.state = null;
    this.detail = "未连接"; this.publish(); return this.status();
  }
}
module.exports = { RemoteSession, packet, decodePackets, parseInvite, validateControl };
