import {createScene} from "./scene.mjs";
import {createPages} from "./pages.mjs";
import {syncHostPlayback} from "./playback-sync.mjs";
import {createMediaPicker} from "./media.mjs";
import {createTools} from "./tools.mjs";
const $ = id => document.getElementById(id);
const controlResults = new Map();
const pages=createPages();
const sceneView=createScene(requestCommand);
const soundTools = createTools(command,requestCommand);
const mediaPicker = createMediaPicker(requestCommand);
function requestCommand(action,value) {
  if(action==="testAudioSuspend"||action==="testAudioResume") {
    const owner=session;if(!owner)return Promise.reject(Error("连接已断开"));
    owner.testing=action==="testAudioSuspend";
    if(owner.audio) {
      if(owner.testing){owner.testWasPlaying=!owner.audio.paused;owner.audio.pause();return Promise.resolve(stats(owner));}
      return (owner.hostRunning?resumeAudio(owner):Promise.resolve()).then(()=>stats(owner));
    }
    return (owner.testing?owner.context.suspend():owner.context.resume()).then(()=>stats(owner));
  }
  return new Promise((resolve,reject)=>{const id=command(action,value);if(!id){reject(Error("尚未连接主机"));return;}controlResults.set(id,{resolve,reject});});
}
const encoder = new TextEncoder(), decoder = new TextDecoder();
const FRAMES = 480, MAX_PACKET = 262144;
let forcePcm=false;let disconnecting=Promise.resolve();
let session = null, playback = null, playlistSignature = "", lastVolumeEdit = 0;
const mediaActions = ["play", "pause", "previoustrack", "nexttrack", "stop"];
function updateSystemPlayback() {
  if (!navigator.mediaSession) return;
  try {
    navigator.mediaSession.playbackState = !session?.ready ? "none" :
      playback?.playing && !playback.paused && (session.audio ? !session.audio.paused : session.context.state === "running") ? "playing" : "paused";
  } catch { /* Optional platform integration must not break PCM playback. */ }
}
function startSystemPlayback(owner) {
  // Safari uses the audio-session category to distinguish music from ambient
  // Web Audio. Request it in the same user gesture as AudioContext.resume().
  try {
    if (navigator.audioSession) {
      owner.previousAudioType = navigator.audioSession.type;
      navigator.audioSession.type = "playback";
    }
  } catch { /* Older browsers do not expose a configurable audio session. */ }
  if (!navigator.mediaSession) return;
  const actions = {
    play: () => { if (session === owner) { void resumeAudio(owner).catch(() => stats(owner)); command("play"); } },
    pause: () => { if (session === owner) { owner.audio?.pause(); command("pause"); } },
    previoustrack: () => { if (session === owner) command("previous"); },
    nexttrack: () => { if (session === owner) command("next"); },
    stop: () => { if (session === owner) stop(); },
  };
  for (const action of mediaActions) {
    try { navigator.mediaSession.setActionHandler(action, actions[action]); } catch { /* Action unavailable on this browser. */ }
  }
}
function endSystemPlayback(owner) {
  try {
    if (owner?.previousAudioType !== undefined && navigator.audioSession?.type === "playback") navigator.audioSession.type = owner.previousAudioType;
  } catch {}
  if (!navigator.mediaSession) return;
  for (const action of mediaActions) { try { navigator.mediaSession.setActionHandler(action, null); } catch {} }
  try { navigator.mediaSession.metadata = null; navigator.mediaSession.playbackState = "none"; navigator.mediaSession.setPositionState?.(); } catch {}
}
const loopModes = [["sequence", "顺序播放"], ["repeat-all", "列表循环"], ["repeat-one", "单曲循环"]];
const renderModes = [["original", "原始立体声"], ["dry", "双耳渲染"], ["room", "录音棚房间"]];
const time = seconds => `${Math.floor(Math.max(0, seconds || 0) / 60)}:${String(Math.floor(Math.max(0, seconds || 0) % 60)).padStart(2, "0")}`;
function message(text, error = false) { $("message").textContent = text; $("message").classList.toggle("error", error); }
function setControls(enabled) {
  sceneView.connected(enabled);
  for (const el of document.querySelectorAll(".transport button,#player-settings button:not([data-tool]):not(#theme),#local-mute,#volume,#queue button,#media-open")) el.disabled = !enabled;
}
function wire(kind, value) {
  const bytes = encoder.encode(JSON.stringify(value));
  const frame = new Uint8Array(5 + bytes.length); frame[0] = kind.charCodeAt(0);
  new DataView(frame.buffer).setUint32(1, bytes.length, true); frame.set(bytes, 5); return frame;
}
function send(kind, value, owner = session) {
  if (!owner || owner.closed || owner.socket?.readyState !== WebSocket.OPEN) return false;
  if (owner.socket.bufferedAmount > 65536) { if(owner.audio){owner.socket.close();return false;} stop("网络持续阻塞，请重新连接", true); return false; }
  owner.socket.send(wire(kind, value)); return true;
}
function command(action, value) {
  if (!session?.ready) return;
  const owner = session;
  if (owner.pending.size >= 16) { message("主机正在处理操作，请稍候"); return; }
  const id = crypto.randomUUID();
  const timer = setTimeout(() => { owner.pending.delete(id); controlResults.get(id)?.reject(Error("主机未确认操作，请检查当前配置"));controlResults.delete(id);soundTools.acknowledged(id,"主机尚未确认操作，请检查当前配置后重试"); if (session === owner) message("主机尚未确认操作，请稍后重试", true); }, action==="roomGenerate"?611000:16000);
  owner.pending.set(id, timer);
  if(owner.audio&&owner.socket?.readyState!==WebSocket.OPEN){
    void fetch(`/hls/${owner.hls}/control`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({id,command:{action,value}})}).then(async res=>{if(!res.ok)throw Error("远程控制连接已失效");const reply=await res.json();if(session===owner)receive("D",encoder.encode(JSON.stringify(reply)),owner);}).catch(error=>message(error.message,true));
    return id;
  }
  if (!send("C", {id, command:{action, value}}, owner)) { clearTimeout(timer); owner.pending.delete(id); return; }
  return id;
}
function chooseMenu(id, entries, action) {
  const trigger = $(id), menu = $(`${id}-menu`);
  for (const [value, label] of entries) {
    const option = document.createElement("button"); option.type = "button"; option.role = "option";
    option.dataset.value = value; option.textContent = label;
    option.addEventListener("click", () => { command(action, value); menu.hidden = true; trigger.setAttribute("aria-expanded", "false"); trigger.focus(); });
    menu.append(option);
  }
  trigger.addEventListener("click", () => {
    menu.hidden = !menu.hidden; trigger.setAttribute("aria-expanded", String(!menu.hidden));
    if (!menu.hidden) (menu.querySelector('[aria-selected="true"]') || menu.firstElementChild).focus();
  });
  menu.addEventListener("keydown", e => {
    const options = [...menu.children], at = options.indexOf(document.activeElement);
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
      e.preventDefault(); options[e.key === "Home" ? 0 : e.key === "End" ? options.length - 1 : (at + (e.key === "ArrowDown" ? 1 : -1) + options.length) % options.length].focus();
    } else if (e.key === "Escape") { menu.hidden = true; trigger.setAttribute("aria-expanded", "false"); trigger.focus(); }
  });
  document.addEventListener("pointerdown", e => { if (!trigger.contains(e.target) && !menu.contains(e.target)) { menu.hidden = true; trigger.setAttribute("aria-expanded", "false"); } });
}
document.addEventListener("pointerdown",e=>{if(!$("player-settings").contains(e.target))$("player-settings").open=false;});
$("player-settings").addEventListener("keydown",e=>{if(e.key==="Escape"){$("player-settings").open=false;$("player-settings").querySelector("summary").focus();}});
chooseMenu("loop", loopModes, "playbackMode"); chooseMenu("render", renderModes, "stereoMode");
function menuValue(id, entries, value) {
  $(`${id}-value`).textContent = (entries.find(item => item[0] === value) || entries[0])[1];
  if(id==="loop") {const label=(entries.find(item=>item[0]===value)||entries[0])[1];$(id).ariaLabel=label;$(id).title=label;$(id).classList.toggle("active",value!=="sequence");$("loop-one").hidden=value!=="repeat-one";}
  for (const option of $(`${id}-menu`).children) option.setAttribute("aria-selected", String(option.dataset.value === value));
}
function updateCover(state,owner){
  if(!owner)return;
  const id=/^[a-f0-9]{64}$/.test(state.artworkId||"")?state.artworkId:"";
  if(owner.coverId===id)return;owner.coverId=id;owner.coverSrc="";
  $("cover").hidden=true;$("cover").removeAttribute("src");$("cover-placeholder").style.display="";
  try{if(navigator.mediaSession?.metadata)navigator.mediaSession.metadata.artwork=[];}catch{}
  if(!id)return;
  void requestCommand("artwork",id).then(data=>{
    if(session!==owner||owner.closed||owner.coverId!==id)return;
    if(typeof data?.src!=="string"||data.src.length>=180000||!/^data:image\/jpeg;base64,[A-Za-z0-9+/=]+$/.test(data.src))return;
    owner.coverSrc=data.src;$("cover").src=data.src;$("cover").hidden=false;$("cover-placeholder").style.display="none";
    try{if(navigator.mediaSession?.metadata)navigator.mediaSession.metadata.artwork=[{src:data.src,type:"image/jpeg"}];}catch{}
    pages.update();
  }).catch(()=>{if(session===owner&&owner.coverId===id)owner.coverId=undefined;});
}
$("cover").addEventListener("error",()=>{$("cover").hidden=true;$("cover-placeholder").style.display="";});
$("local-mute").addEventListener("change",()=>command("localMute",$("local-mute").checked));
function renderState(state) {
  if (!state || !Array.isArray(state.playlist) || state.playlist.length > 500 || typeof state.title !== "string") throw Error("主机状态无效");
  playback = state;
  const source=state.source;
  const codecs={"ec-3":"E-AC-3","eac3":"E-AC-3","ac-3":"AC-3","ac3":"AC-3","ac-4":"AC-4","ac4":"AC-4","mlpa":"TrueHD","truehd":"TrueHD","alac":"ALAC","mp4a":"AAC","pcm":"PCM","adm":"ADM PCM"};
  const fields=[];
  if(source?.codec)fields.push(codecs[source.codec.toLowerCase()]||source.codec.toUpperCase());
  if(source?.sampleRate>0)fields.push(`${source.sampleRate/1000} kHz`);
  if(source?.channels>0)fields.push(`${source.channels} 声道`);
  if(source?.objects>0)fields.push(`${source.objects} 对象`);
  $("format-badge").textContent=fields.join(" · ")||"等待歌曲信息";
  $("local-mute").checked=state.localMuted!==false;
  syncHostPlayback(session,state,resumeAudio,()=>{if(session)stats(session);});
  updateCover(state,session);
  soundTools.update(state.tools, !!session?.ready, state.playing&&!state.paused);
  try {
    if (navigator.mediaSession && typeof MediaMetadata !== "undefined" && session?.mediaTitle !== JSON.stringify([state.currentId,state.title,state.artist,state.album])) {
      navigator.mediaSession.metadata = new MediaMetadata({title:state.title || "SDA 远程收听", artist:state.artist || "", album:state.album || "",artwork:session?.coverSrc?[{src:session.coverSrc,type:"image/jpeg"}]:[]});
      if (session) session.mediaTitle = JSON.stringify([state.currentId,state.title,state.artist,state.album]);
    }
    if (navigator.mediaSession?.setPositionState) {
      if (Number.isFinite(state.duration) && state.duration > 0) navigator.mediaSession.setPositionState({duration:state.duration, position:Math.min(state.duration, Math.max(0, state.position || 0)), playbackRate:1});
      else navigator.mediaSession.setPositionState();
    }
  } catch { /* Metadata support varies independently from audio support. */ }
  updateSystemPlayback();
  $("title").textContent = state.title || "等待主机选择歌曲";
  $("artist").textContent = state.artist || ""; $("artist").hidden = !state.artist;
  document.title = state.title ? `${state.title} · SDA` : "SDA · 远程收听";
  const playing = state.playing && !state.paused;
  $("transport-status").textContent = playing ? "正在播放" : state.paused ? "主机已暂停" : "等待播放";
  $("play").ariaLabel = $("play").title = playing ? "暂停" : "播放";
  $("play-icon").hidden = !!playing; $("pause-icon").hidden = !playing;
  // SVGElement.hidden is not universally reflected like HTMLElement.hidden.
  $("play-icon").toggleAttribute("hidden", !!playing); $("pause-icon").toggleAttribute("hidden", !playing);
  $("progress").max = Math.max(state.duration || 0, state.position || 0, 1); $("progress").value = state.position || 0;
  $("position").textContent = time(state.position); $("duration").textContent = time(state.duration);
  if (Date.now() - lastVolumeEdit > 400) {
    $("volume").value = Math.round(state.volume * 100); $("volume-label").textContent = `${$("volume").value}%`;
  }
  updatePlayButton();
  menuValue("loop", loopModes, state.playbackMode); menuValue("render", renderModes, state.stereoMode);
  const signature = JSON.stringify([state.playlist, state.currentId]);
  if (signature !== playlistSignature) {
    playlistSignature = signature; const fragment = document.createDocumentFragment();
    state.playlist.forEach((track, index) => {
      if (typeof track.id !== "string" || typeof track.title !== "string") throw Error("主机列表无效");
      const row = document.createElement("li"), button = document.createElement("button");
      const number = document.createElement("span"), title = document.createElement("span"), current = document.createElement("span");
      number.className = "track-number"; number.textContent = String(index + 1).padStart(2, "0");
      title.className = "track-name"; title.textContent = track.title; title.title = track.title;
      current.className = "now"; current.textContent = track.id === state.currentId ? "♫" : ""; current.ariaHidden = "true";
      button.setAttribute("aria-current", String(track.id === state.currentId)); button.ariaLabel = `播放 ${track.title}`;
      button.addEventListener("click", () => command("track", track.id)); button.append(number, title, current); row.append(button); fragment.append(row);
    });
    $("queue").replaceChildren(fragment);pages.update(); $("queue-count").textContent = `${state.playlist.length} 首`; $("empty-queue").hidden = state.playlist.length > 0;
  }
}
function browserNeedsPlay() {
  return !!session && (session.audio ? session.audio.paused : session.context?.state !== "running");
}
function updatePlayButton() {
  const playing=!!playback?.playing&&!playback.paused&&!browserNeedsPlay();
  $("play").ariaLabel=$("play").title=playing?"暂停":"播放";
  $("play-icon").toggleAttribute("hidden",playing);$("pause-icon").toggleAttribute("hidden",!playing);
}
function stats(owner) {
  if (session !== owner || owner.closed) return;
  updateSystemPlayback();
  updatePlayButton();
  if(owner.audio){
    $("connection").textContent=owner.testing?"耳廓测试中":owner.audio.paused?(owner.hostRunning===false?"主机已暂停":"点击播放收听"):owner.audio.readyState<3?"缓冲中":owner.socket?.readyState===WebSocket.OPEN?"已连接":"正在收听 · 控制重连中";
    $("audio-info").textContent="原生媒体播放 · 数秒缓冲延迟 · 浏览器和系统可能重采样";
    return;
  }
  const suspended = owner.context.state !== "running";
  $("connection").textContent = owner.testing ? "耳廓测试中" : suspended ? "浏览器已暂停音频" : owner.buffering ? "缓冲中" : "已连接";
  $("audio-info").textContent = `浏览器音频 ${owner.context.sampleRate / 1000} kHz · 缓冲 ${Math.round(owner.queued / 48)} ms · 已接收 ${(owner.bytes / 1048576).toFixed(1)} MB`;
}
function receive(kind, body, owner) {
  owner.lastHost = Date.now();
  if (kind === "A") {
    if (!owner.ready || body.length !== FRAMES * 8) throw Error("PCM 数据格式不匹配");
    const samples = body.slice().buffer;
    owner.bytes += body.length; owner.node.port.postMessage({type:"pcm", samples}, [samples]); return;
  }
  if (kind === "R") {
    if (!owner.ready || body.length) throw Error("无效音频重置");
    owner.node.port.postMessage({type:"reset"}); return;
  }
  const value = JSON.parse(decoder.decode(body));
  if (kind === "H") {
    if ((!owner.audio && owner.ready) || value.protocol !== 1 || value.sampleRate !== 48000 || value.channels !== 2 || value.sampleFormat !== (owner.audio?"hls-flac24":"f32le")) throw Error("主机音频格式不兼容");
    owner.ready = true; if(!owner.audio)send("H", {protocol:1}, owner);
    $("pairing").hidden = true; pages.show();
    setControls(true); message(""); stats(owner);
    $("transport-format").textContent=owner.audio?"传输：FLAC · 48 kHz / 24-bit":"传输：PCM · 48 kHz / 32-bit float";
    $("format-note").textContent=owner.audio?"float32 转 24-bit PCM 后无损编码；超出整数满幅会截断，无增益或响度处理。":"网络原样传输 · 浏览器和系统可能重采样";
  } else if (kind === "S") renderState(value);
  else if (kind === "D") {
    const result=controlResults.get(value.id);if(value.error)result?.reject(Error(value.error));else result?.resolve(value.data);controlResults.delete(value.id);
    soundTools.acknowledged(value.id,value.error);
    clearTimeout(owner.pending.get(value.id)); owner.pending.delete(value.id);
    if (value.error) message(value.error, true);
  } else if (kind === "E") throw Error(value.error || "主机连接失败");
  else if(kind==="T"&&owner.audio&&value.clipped>0)$("format-note").textContent=`float32 转 24-bit PCM 后无损编码；已有 ${value.clipped} 个超满幅采样截断，请检查主机输出电平。`;
  else if (kind !== "T") throw Error("无法识别主机消息");
}
async function tokenFrom(value) {
  value = value.trim();
  if (/^[a-f0-9]{64}$/.test(value)) return value;
  if (!value) throw Error("请输入主机设定的密钥，或粘贴完整网页链接");
  if (!/^https?:\/\//i.test(value)) {
    if (value.length > 256) throw Error("自定义密钥最多 256 个字符");
    const material = await crypto.subtle.importKey("raw", encoder.encode(value), "PBKDF2", false, ["deriveBits"]);
    const bits = await crypto.subtle.deriveBits({name:"PBKDF2", hash:"SHA-256", salt:encoder.encode("SDA remote pairing v1"), iterations:210000}, material, 256);
    return [...new Uint8Array(bits)].map(byte => byte.toString(16).padStart(2,"0")).join("");
  }
  let url; try { url = new URL(value); } catch { throw Error("请粘贴主机生成的完整网页链接"); }
  if (url.protocol !== "https:" || url.host !== location.host || !/^#[a-f0-9]{64}$/.test(url.hash)) throw Error("请直接打开对应主机的完整网页链接");
  return url.hash.slice(1);
}
async function resumeAudio(owner) {
  if(!owner.audio)return owner.context.resume();
  const audio=owner.audio;
  // A paused live stream resumes near the live edge, never stale music.
  if(audio.seekable.length){const end=audio.seekable.end(audio.seekable.length-1);if(end-audio.currentTime>6)audio.currentTime=Math.max(audio.seekable.start(0),end-3);}
  await audio.play();if($("message").textContent==="点击播放按钮开始收听")message("");stats(owner);
}
function openHlsControl(owner) {
  if(owner.closed||session!==owner||owner.socket?.readyState===WebSocket.OPEN||owner.socket?.readyState===WebSocket.CONNECTING)return;
  clearTimeout(owner.reconnect);
  const socket=owner.socket=new WebSocket(`wss://${location.host}/stream`);socket.binaryType="arraybuffer";
  socket.onopen=()=>socket.send(JSON.stringify({protocol:1,token:owner.key,hls:owner.hls}));
  let pending=new Uint8Array(0);
  socket.onmessage=event=>{
    if(owner.closed||session!==owner)return;
    try{
      if(!(event.data instanceof ArrayBuffer))throw Error("无效控制消息");
      const chunk=new Uint8Array(event.data),joined=new Uint8Array(pending.length+chunk.length);joined.set(pending);joined.set(chunk,pending.length);pending=joined;
      while(pending.length>=5){const length=new DataView(pending.buffer,pending.byteOffset+1,4).getUint32(0,true);if(length>MAX_PACKET)throw Error("控制消息过长");if(pending.length<length+5)break;
        const kind=String.fromCharCode(pending[0]),body=pending.subarray(5,length+5);pending=pending.subarray(length+5);receive(kind,body,owner);}
    }catch(error){stop(error.message,true);}
  };
  socket.onerror=()=>{};
  socket.onclose=event=>{
    if(owner.closed||session!==owner)return;
    setControls(false);
    if(event.code===1008){stop(event.reason||"收听会话失效",true);return;}
    stats(owner);owner.reconnect=setTimeout(()=>openHlsControl(owner),2000);
  };
}
async function connectHls(enteredKey){
  const audio=document.createElement("audio");audio.preload="auto";audio.setAttribute("playsinline","");audio.hidden=true;document.body.append(audio);
  const owner=session={audio,closed:false,ready:false,pending:new Map(),socket:null};
  startSystemPlayback(owner);$("disconnect").hidden=false;
  try{
    owner.key=await tokenFrom(enteredKey);
    await disconnecting;
    const response=await fetch("/hls/session",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({token:owner.key})});
    if(!response.ok)throw Error(response.status===409?"主机已有一个客户端连接":"无法建立无损媒体流，请检查配对密钥和主机");
    const info=await response.json();owner.hls=info.id;
    if(owner.closed||session!==owner){void fetch(`/hls/${info.id}/stop`,{method:"POST",keepalive:true});return;}
    try{sessionStorage.setItem("sda-web-pairing",owner.key);}catch{}
    audio.src=info.stream;
    for(const name of ["playing","pause","waiting","canplay"])audio.addEventListener(name,()=>stats(owner));
    audio.addEventListener("error",()=>{if(session===owner)stop("此浏览器无法播放 FLAC 无损 HLS，或媒体连接中断。未切换到有损音频。",true);});
    openHlsControl(owner);
    await resumeAudio(owner).catch(()=>{if(audio.paused&&owner.hostRunning!==false)message("点击播放按钮开始收听");stats(owner);});
    if(owner.hostRunning===false)audio.pause();
    owner.timer=setInterval(()=>{if(owner.audio.paused)send("K",{},owner);stats(owner);},1000);
  }catch(error){if(session===owner)stop(error.message,true);}
}
async function connect() {
  if (session) return;
  const enteredKey = $("invite").value;
  $("invite").blur();
  const nativeHls = !forcePcm && !!document.createElement("audio").canPlayType("application/vnd.apple.mpegurl");
  if (!window.isSecureContext || (!nativeHls && !window.AudioWorkletNode)) { message("此浏览器未提供安全音频环境。请使用 HTTPS 链接，在 Chrome、Edge、Firefox 或 Safari 新版中打开。", true); return; }
  $("connect").disabled = true; message("正在连接音频…");
  let owner;
  try {
    if(nativeHls){await connectHls(enteredKey);return;}
    const context = new AudioContext({sampleRate:48000, latencyHint:"playback"});
    owner = session = {context, node:null, socket:null, closed:false, ready:false, consumed:0, queued:0, bytes:0, buffering:true, pending:new Map(), lastHost:Date.now()};
    startSystemPlayback(owner);
    $("disconnect").hidden = false;
    // Called within the click gesture, before asynchronous module/network work.
    const resumed = context.resume();
    const key = await tokenFrom(enteredKey);
    if (context.sampleRate !== 48000) throw Error("当前浏览器无法创建 48 kHz 音频输出，请换用新版 Chrome 或 Edge");
    await context.audioWorklet.addModule("/pcm-worklet.mjs"); await resumed;
    if (session !== owner || owner.closed) return;
    const node = owner.node = new AudioWorkletNode(context, "sda-remote-pcm", {numberOfInputs:0, numberOfOutputs:1, outputChannelCount:[2], channelCount:2, channelCountMode:"explicit", channelInterpretation:"discrete"});
    node.connect(context.destination);
    node.onprocessorerror = () => { if (session === owner) stop("浏览器音频处理已中断，请重新连接", true); };
    node.port.onmessage = ({data}) => {
      if (owner.closed || session !== owner) return;
      if (data.type === "error") { stop(data.detail || "音频缓冲异常", true); return; }
      if (data.type === "progress") {
        owner.consumed = data.consumed; owner.queued = data.queued; owner.buffering = data.buffering;
        if (owner.ready) send("K", {consumed:owner.consumed}, owner);
      }
    };
    context.onstatechange = () => stats(owner);
    const socket = owner.socket = new WebSocket(`wss://${location.host}/stream`); socket.binaryType = "arraybuffer";
    socket.onopen = () => {
      if (owner.closed) { socket.close(); return; }
      socket.send(JSON.stringify({protocol:1, token:key}));
      try { sessionStorage.setItem("sda-web-pairing", key); } catch { /* Private browsing can disable storage. */ }
    };
    let pending = new Uint8Array(0);
    socket.onmessage = event => {
      if (owner.closed || session !== owner) return;
      try {
        if (!(event.data instanceof ArrayBuffer)) throw Error("主机未发送二进制 PCM 数据");
        const chunk = new Uint8Array(event.data), joined = new Uint8Array(pending.length + chunk.length);
        joined.set(pending); joined.set(chunk, pending.length); pending = joined;
        while (pending.length >= 5) {
          const length = new DataView(pending.buffer, pending.byteOffset + 1, 4).getUint32(0, true);
          if (length > MAX_PACKET) throw Error("主机数据包超过上限");
          if (pending.length < length + 5) break;
          const kind = String.fromCharCode(pending[0]), body = pending.subarray(5, 5 + length);
          pending = pending.subarray(5 + length); receive(kind, body, owner);
        }
        if (pending.length > MAX_PACKET + 5) throw Error("网络缓冲超过上限");
      } catch (error) { stop(error.message, true); }
    };
    socket.onerror = () => { if (session === owner) message("无法连接主机，请检查 HTTPS 证书、主机状态及网络", true); };
    socket.onclose = event => { if (session === owner) stop(event.reason || "连接已断开，请重新连接", event.code !== 1000); };
    owner.timer = setInterval(() => {
      if (session !== owner) return;
      if (Date.now() - owner.lastHost > 15000) { stop("主机超过 15 秒没有响应，请检查网络", true); return; }
      if (owner.ready) send("K", {consumed:owner.consumed}, owner); stats(owner);
    }, 1000);
  } catch (error) { if (!owner || session === owner) stop(error.message || "无法启动浏览器音频", true); }
}
function stop(reason = "已断开连接", error = false) {
  for(const result of controlResults.values())result.reject(Error(reason));controlResults.clear();
  soundTools.disconnected();mediaPicker.close();
  const owner = session; session = null;pages.hide();$("cover").hidden=true;$("cover").removeAttribute("src");$("cover-placeholder").hidden=false;
  if (owner) {
    endSystemPlayback(owner);
    owner.closed = true; clearInterval(owner.timer);
    for (const timer of owner.pending.values()) clearTimeout(timer);
    if (!owner.audio && owner.socket?.readyState === WebSocket.OPEN) owner.socket.send(wire("Q", {}));
    owner.socket?.close(1000); owner.node?.port.postMessage({type:"stop"}); owner.node?.disconnect();
    if(owner.audio){owner.audio.pause();owner.audio.removeAttribute("src");owner.audio.load();owner.audio.remove();
      if(owner.hls)disconnecting=fetch(`/hls/${owner.hls}/stop`,{method:"POST",keepalive:true}).then(()=>{},()=>{});
    }
    clearTimeout(owner.reconnect);
    void owner.context?.close().catch(() => {});
  }
  $("connect").disabled = false; $("pairing").hidden = false; $("disconnect").hidden = true;
  $("connection").textContent = "未连接"; setControls(false); message(reason, error);
}
$("stream-mode").addEventListener("click",()=>{
  if(session)return;forcePcm=!forcePcm;$("stream-mode").setAttribute("aria-pressed",String(forcePcm));
  $("stream-mode").textContent=forcePcm?"播放方式：低延迟 float32 PCM · 不保证后台":"播放方式：自动 · 优先原生无损 HLS";
});
$("connect").addEventListener("click", () => void connect());
$("disconnect").addEventListener("click", () => stop());
$("play").addEventListener("click", () => {
  const pause=playback?.playing&&!playback.paused&&!browserNeedsPlay();
  if(session){const owner=session;if(pause)owner.audio?.pause();else void resumeAudio(owner).then(()=>{message("");stats(owner);}).catch(()=>stats(owner));}
  command(pause?"pause":"play");
});
for (const button of document.querySelectorAll("[data-action]")) button.addEventListener("click", () => command(button.dataset.action));
$("volume").addEventListener("input", () => { lastVolumeEdit = Date.now(); $("volume-label").textContent = `${$("volume").value}%`; });
$("volume").addEventListener("change", () => command("volume", Number($("volume").value) / 100));
window.addEventListener("pagehide", () => stop());
document.addEventListener("visibilitychange", () => {
  const owner = session;
  // Hiding/locking is not a request to disconnect. On return, allow queued
  // socket events to arrive before checking a timer delayed by the OS.
  if (!owner || document.hidden) return;
  owner.lastHost = Date.now();
  if(owner.audio){openHlsControl(owner);stats(owner);return;}
  if (owner.ready) send("K", {consumed:owner.consumed}, owner);
  if (playback?.playing && !playback.paused && owner.context.state !== "running") {
    void owner.context.resume().catch(() => stats(owner));
  }
  stats(owner);
});
let theme = "dark";
try { theme = localStorage.getItem("sda-web-theme") || (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark"); } catch { /* Keep default. */ }
document.documentElement.dataset.theme = theme;$("theme-value").textContent=theme==="light"?"浅色":"深色";
$("theme").addEventListener("click", () => { theme = theme === "light" ? "dark" : "light"; document.documentElement.dataset.theme = theme;$("theme-value").textContent=theme==="light"?"浅色":"深色"; try { localStorage.setItem("sda-web-theme", theme); } catch {} });
try {
  const key = /^#[a-f0-9]{64}$/.test(location.hash) ? location.hash.slice(1) : sessionStorage.getItem("sda-web-pairing") || "";
  $("invite").value = key;
  if (location.hash) history.replaceState(null, "", location.pathname);
} catch { /* User can paste the full link into the pairing field. */ }
setControls(false);

initGlass();

// Same rounded lens displacement as desktop GlassRefraction, generated only on resize.
// WebKit uses the CSS material: SVG backdrop filters are not portable there.
function initGlass(){
 const elements=[...document.querySelectorAll('#player-settings>summary,.player-settings-menu,.transport>button,.transport-loop>button')];
 for(const element of elements)element.classList.add('liquid-control');
 if(!/Chrome|Chromium|Edg\//.test(navigator.userAgent)||/CriOS|EdgiOS/.test(navigator.userAgent)||matchMedia('(prefers-reduced-transparency: reduce)').matches)return;
 const ns='http://www.w3.org/2000/svg';const svg=document.createElementNS(ns,'svg');svg.setAttribute('width','0');svg.setAttribute('height','0');svg.classList.add('glass-definitions');svg.setAttribute('aria-hidden','true');const defs=document.createElementNS(ns,'defs');svg.append(defs);document.body.append(svg);
 elements.forEach((element,index)=>{
  const layer=document.createElement('span');layer.className='glass-lens';layer.setAttribute('aria-hidden','true');element.prepend(layer);
  const filter=document.createElementNS(ns,'filter'),map=document.createElementNS(ns,'feImage'),displace=document.createElementNS(ns,'feDisplacementMap');const id=`remote-glass-${index}`;
  for(const [key,value] of Object.entries({id,x:'0',y:'0',width:'100%',height:'100%','color-interpolation-filters':'sRGB'}))filter.setAttribute(key,value);
  map.setAttribute('result','lens');map.setAttribute('preserveAspectRatio','none');
  for(const [key,value]of Object.entries({in:'SourceGraphic',in2:'lens',scale:'12',xChannelSelector:'R',yChannelSelector:'G'}))displace.setAttribute(key,value);
  filter.append(map,displace);defs.append(filter);let frame=0,previous='';
  new ResizeObserver(()=>{cancelAnimationFrame(frame);frame=requestAnimationFrame(()=>{
   const {width,height}=element.getBoundingClientRect();if(width<1||height<1)return;const key=`${width}:${height}`;if(key===previous)return;previous=key;
   const canvas=document.createElement('canvas');canvas.width=Math.ceil(width);canvas.height=Math.ceil(height);const context=canvas.getContext('2d');if(!context)return;const pixels=context.createImageData(canvas.width,canvas.height);
   const radius=Math.min(parseFloat(getComputedStyle(element).borderRadius)||22,width/2,height/2),bevel=Math.min(40,height*.48);
   for(let y=0;y<canvas.height;y++)for(let x=0;x<canvas.width;x++){
    const px=x+.5-width/2,py=y+.5-height/2,qx=Math.abs(px)-(width/2-radius),qy=Math.abs(py)-(height/2-radius),ox=Math.max(qx,0),oy=Math.max(qy,0),length=Math.hypot(ox,oy),depth=radius-length-Math.min(Math.max(qx,qy),0),t=Math.max(0,Math.min(1,depth/bevel)),bend=16*t*t*(1-t)*(1-t),nx=length?ox/length:qx>qy?1:0,ny=length?oy/length:qx>qy?0:1,i=(y*canvas.width+x)*4;
    pixels.data[i]=Math.round(127.5-Math.sign(px)*nx*bend*110);pixels.data[i+1]=Math.round(127.5-Math.sign(py)*ny*bend*110);pixels.data[i+2]=128;pixels.data[i+3]=255;
   }
   context.putImageData(pixels,0,0);map.setAttribute('href',canvas.toDataURL());map.setAttribute('width',String(width));map.setAttribute('height',String(height));layer.style.backdropFilter=`url("#${id}")`;
  });}).observe(element);
 });
}
