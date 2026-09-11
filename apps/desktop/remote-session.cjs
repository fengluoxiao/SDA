"use strict";
const tls = require("node:tls");
const {RemoteDevices}=require("./remote-devices.cjs");
const {RemoteFanout}=require("./remote-fanout.cjs");
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
    if(arg.profileId!==undefined&&arg.profileId!==null&&(typeof arg.profileId!=="string"||!/^[a-f0-9]{64}$/.test(arg.profileId)))return null;
    return {action,value:{expected:arg.expected,settings,...(arg.profileId!==undefined?{profileId:arg.profileId}:{})}};
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
    this.hooks = hooks;
    this.sync=hooks.gate?new (require('./remote-sync.cjs').RemoteSync)(hooks):null;
    if(hooks.readDevices&&hooks.writeDevices)this.devices=new RemoteDevices({read:hooks.readDevices,write:hooks.writeDevices,changed:()=>this.publish(),kick:id=>this.disconnectDevice(id)}); this.role = "off"; this.phase = "off"; this.detail = "未连接";
    this.peer = null; this.server = null; this.localServer = null; this.local = null; this.receiver = null;
    this.state = null; this.key = null; this.port = null; this.generation = 0; this.pendingControls = new Map();
    this.hostPeers = new Set(); this.fanout=null; this.stateRevision=0;this.controlJobs=[];this.activeControl=null;
    this.sockets = new Set(); this.bufferMs = 300; this.queued = 0; this.bytes = 0;
  }
  status() {
    return { capacity:this.hooks.maxPeers??1, connectedDevices:[...this.hostPeers].map(p=>({id:p.deviceId??p.remoteAddress,name:p.deviceName??p.remoteAddress,canControl:p.canControl!==false})), devices:this.devices?.list()??[], pendingDevices:this.devices?.pendingList()??[], role: this.role, phase: this.phase, detail: this.detail, peer: this.peer?.remoteAddress ?? null,
      port: this.port, addresses: this.role === "host" ? addresses() : [],
      invites: this.role === "host" && this.key && !this.devices ? addresses().map(a => this.invite(a)) : [],
      webInvites: this.role === "host" && this.key ? addresses().map(a => this.webInvite(a)) : [],
      format: this.peer?.header?.sampleFormat === "hls-flac24" ? "48 kHz · 24-bit FLAC · 双声道" : "48 kHz · 32-bit float PCM · 双声道", localMuted:this.hooks.localMuted?.()??true, hlsAllowed:this.hooks.hlsAllowed?.()===true, bufferMs: this.bufferMs, queuedMs: this.queued / 48,
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
        const device=this.devices?.authenticateToken(auth.subarray(0,32).toString("hex"));
        if(this.devices?!device:!crypto.timingSafeEqual(auth.subarray(0,32),key)){socket.destroy();return;}
        if(device){socket.deviceId=device.id;socket.deviceName=device.name;socket.canControl=device.canControl!==false;}
        socket.pause();socket.off("data",authenticate);
        if (!this.canAccept() || this.role !== "host" || generation !== this.generation) {
          socket.end(packet("E", { error: "主机已有一个客户端连接" })); return;
        }
        if(auth.length>32)socket.unshift(auth.subarray(32));auth=Buffer.alloc(0);
        this.reservePeer(socket);this.phase="connecting";this.detail="客户端已配对，正在连接音频";
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
  disconnectDevice(id){for(const socket of this.hostPeers)if(socket.deviceId===id)this.failPeer(socket,"设备连接已被电脑断开或撤销");}
  manageDevice(action,value){if(!this.devices)throw Error("设备授权不可用");if(action==="deviceApprove")this.devices.approve(value?.id,value?.canControl);else if(action==="deviceReject")this.devices.reject(value?.id);else if(action==="deviceRevoke")this.devices.revoke(value?.id);else if(action==="devicePermission")this.devices.permission(value?.id,value?.canControl);else if(action==="deviceDisconnect")this.disconnectDevice(value?.id);else throw Error("无效设备操作");return this.status();}
  canAccept(){return this.hostPeers.size<(this.hooks.maxPeers??1);}
  reservePeer(socket){if(this.hostPeers.has(socket))return true;if(!this.canAccept()||socket.deviceId&&[...this.hostPeers].some(p=>p.deviceId===socket.deviceId&&!p.destroyed))return false;this.hostPeers.add(socket);this.peer??=socket;socket.once("close",()=>{this.hostPeers.delete(socket);if(this.peer===socket)this.peer=this.hostPeers.values().next().value??null;});return true;}
  async attachHost(socket,generation){
    if(!this.reservePeer(socket))throw Error("已达到设备连接上限");
    await this.detaching;
    if(!this.hostPeers.has(socket)||socket.destroyed||this.generation!==generation)return;
    const hub=this.fanout??=new RemoteFanout(this,{packet,decodePackets});const p=hub.add(socket);
    const incoming=decodePackets((kind,body)=>{
      const message=readJson(body);
      if(kind==="H"){
        if(p.ready||message.protocol!==1)throw Error("远程协议不兼容");p.ready=true;p.lastFeedback=Date.now();if(p.reset){p.reset=false;socket.write(packet("R"));}hub.pump();
      }else if(kind==="K"){
        if(!p.ready||!Number.isSafeInteger(message.consumed)||message.consumed<p.consumed||message.consumed>p.sent)throw Error("无效音频确认");
        p.consumed=message.consumed;p.lastFeedback=Date.now();this.queued=Math.max(0,...[...hub.peers.values()].map(v=>v.sent-v.consumed));hub.pump();
        if(message.mediaState&&Date.now()-(p.mediaReportAt??0)>1000){
          const m=message.mediaState;p.mediaReportAt=Date.now();
          this.hooks.diagnostic?.({transport:'pcm',hidden:m.hidden===true,context:['running','suspended','interrupted','closed'].includes(m.context)?m.context:'unknown',playback:['none','paused','playing','unavailable'].includes(m.playback)?m.playback:'unknown',buffering:m.buffering===true,mediaElement:m.mediaElement===true,mediaPaused:m.mediaPaused===true,mediaReadyState:Number.isInteger(m.mediaReadyState)&&m.mediaReadyState>=0&&m.mediaReadyState<=4?m.mediaReadyState:undefined});
        }
      }else if(kind==="C"){
        if(!p.ready||typeof message.id!=="string"||message.id.length>64)throw Error("无效远程控制");
        const own=[...this.pendingControls.values()].filter(v=>v.socket===socket).length;
        if(own>=16){socket.write(packet("D",{id:message.id,error:"操作过于频繁，请稍候"}));return;}
        const command=validateControl(message.command);
        if(socket.canControl===false&&!["scene","artwork","mediaList"].includes(command.action)){socket.write(packet("D",{id:message.id,error:"此设备仅允许收听"}));return;}
        const id=crypto.randomUUID(),timer=setTimeout(()=>this.completeControl(id,"主机没有响应此操作"),command.action==="roomGenerate"?610000:15000).unref();
        this.pendingControls.set(id,{socket,remoteId:message.id,timer});
        if(command.action==="artwork")this.completeControl(id,null,{src:command.value===this.artworkId?this.artwork:""});
        else if(command.action==="scene")this.completeControl(id,null,this.scene??null);
        else this.dispatchControl(id,command);
      }else if(kind==="Q")socket.end();else throw Error("不支持的远程消息");
    });
    socket.on("data",chunk=>{try{incoming(chunk);}catch(e){this.failPeer(socket,e.message);}});
    const timer=setInterval(()=>{if(Date.now()-p.lastFeedback>15000)this.failPeer(socket,"设备超过 15 秒没有音频响应");else{socket.write(packet("T",{}));this.publish();}},1000).unref();
    socket.once("close",()=>{
      clearInterval(timer);hub.peers.delete(socket);this.hostPeers.delete(socket);
      for(const [id,v]of this.pendingControls)if(v.socket===socket)this.completeControl(id,"设备已断开");
      if(this.peer===socket)this.peer=this.hostPeers.values().next().value??null;
      if(this.role!=="host")return;
      if(!this.hostPeers.size){hub.close();if(this.fanout===hub)this.fanout=null;this.local=null;this.localServer=null;this.queued=0;this.phase="waiting";this.detail="设备已断开，等待连接";this.detaching=Promise.resolve(this.hooks.disconnected?.()).catch(()=>{});}
      else{this.phase="connected";this.detail=`${this.hostPeers.size} 台设备无损收听中`;hub.pump();}
      this.publish();
    });
    try{await hub.start();}catch(e){hub.fail(e.message);throw e;}
    if(socket.destroyed||generation!==this.generation)return;
    this.phase="connected";this.detail=`${this.hostPeers.size} 台设备无损收听中`;this.publish();
    socket.write(packet("H",{protocol:1,sampleRate:48000,channels:2,sampleFormat:"f32le",bufferMs:this.bufferMs,canControl:socket.canControl!==false}));
    if(this.state)socket.write(packet("S",this.state));
  }
  failPeer(socket, detail) {
    if (this.peer !== socket && !this.hostPeers.has(socket)) return;
    socket.sdaFailure = detail;
    this.detail = detail; this.publish();
    if (socket.writable) socket.write(packet("E", { error: detail })); socket.destroy();
  }
  dispatchControl(id,command){
    if(["mediaList","roomCancel"].includes(command.action)){this.hooks.control?.({id,...command});return;}
    this.controlJobs.push({id,...command});this.drainControls();
  }
  drainControls(){
    if(this.activeControl)return;
    while(this.controlJobs.length){const job=this.controlJobs.shift();if(!this.pendingControls.has(job.id))continue;this.activeControl=job.id;try{this.hooks.control?.(job);}catch(e){this.completeControl(job.id,e.message);}return;}
  }
  completeControl(id, error = null, data = undefined) {
    const pending = this.pendingControls.get(id); if (!pending) return;
    clearTimeout(pending.timer); this.pendingControls.delete(id);
    if(this.activeControl===id){this.activeControl=null;setImmediate(()=>this.drainControls());}
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
    this.state = { revision:++this.stateRevision, source:source&&typeof source==="object"?{codec:text(source.codec),sampleRate:count(source.sampleRate),channels:count(source.channels),objects:count(source.objects)}:null, localMuted:this.hooks.localMuted?.()??true, artworkId:this.artworkId||"", title: text(state.title), artist:text(state.artist), album:text(state.album), playing: !!state.playing, paused: !!state.paused, loading:state.loading===true&&!state.paused,
      position: Number.isFinite(state.position) ? Math.max(0, state.position) : 0,
      duration: Number.isFinite(state.duration) ? Math.max(0, state.duration) : 0,
      volume: Number.isFinite(state.volume) ? Math.min(1, Math.max(0, state.volume)) : 1,
      currentId: text(state.currentId), playbackMode: text(state.playbackMode), stereoMode: text(state.stereoMode),
      tools: this.curateTools(state.tools),
      playlist: Array.isArray(state.playlist) ? state.playlist.slice(0, 500).map(v => ({ id: text(v.id), title: text(v.title) })) : [] };
    this.sync?.update(this.state);
    if(this.role==="host")for(const socket of this.hostPeers)if(!socket.destroyed&&socket.writableLength<MAX_PACKET)socket.write(packet("S",this.state));
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
    this.devices?.clearPending();
    const wasHost = this.role === "host"; this.generation++; this.role = "off"; this.phase = "off";
    this.fanout?.close();this.fanout=null;this.hostPeers.clear();
    this.peer = null; this.local = null;
    for (const socket of this.sockets) socket.destroy(); this.sockets.clear();
    const server=this.server;this.server=null;
    if(server)await new Promise(resolve=>server.close(()=>resolve())); this.localServer?.close(); this.localServer = null;
    this.web?.close(); this.web = null;
    const receiver = this.receiver; this.receiver = null; receiver?.kill();
    this.controlJobs=[];this.activeControl=null;
    for (const item of this.pendingControls.values()) clearTimeout(item.timer); this.pendingControls.clear();
    await this.detaching;
    if (wasHost) { await this.hooks.route({ address: null, token: null }); }
    this.key = null; this.certificate=null; this.port = null; this.queued = 0; this.bytes = 0; this.output = null; this.state = null;
    this.detail = "未连接"; this.publish(); return this.status();
  }
}
module.exports = { RemoteSession, packet, decodePackets, parseInvite, validateControl };
