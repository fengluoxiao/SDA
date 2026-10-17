import {prepareRtc} from './rtc.mjs';
import {ConnectionWatchdog} from "./connection-watchdog.mjs";
import {hlsAllowedByHost} from "/playback-policy.mjs";
import {createScene} from "./scene.mjs";
import {createPages} from "./pages.mjs";
import {syncHostPlayback,receiverView,beginTrackSwitch,acceptHlsMedia,cancelTrackSwitch} from "./playback-sync.mjs";
import {localClock,bufferedAhead,mediaRanges,prepareSynchronizedMedia,recoverSynchronizedMedia,synchronizeMedia} from "./synchronized-playback.mjs";
import {createMediaPicker} from "./media.mjs";
import {createTools} from "./tools.mjs";
import {PausePrefetch} from "./pause-prefetch.mjs";
import {createPcmMediaOutput} from './pcm-media-output.mjs';
import {calibrateStartup} from './startup-calibration.mjs';
import {installMoreMenu} from "./more-menu.mjs";
const $ = id => document.getElementById(id);
// Safari may ignore viewport scale limits. Cancel only native zoom gestures;
// ordinary scrolling, Swiper and scene pointer handlers remain active.
for(const type of ['gesturestart','gesturechange'])document.addEventListener(type,event=>event.preventDefault(),{passive:false});
document.addEventListener('touchmove',event=>{if(event.touches.length>1)event.preventDefault();},{passive:false});
const controlResults = new Map();
const pages=createPages();
const sceneView=createScene(requestCommand);
const soundTools = createTools(command,requestCommand,()=>({
  track:JSON.stringify([playback?.currentId,playback?.title]),hasTrack:!!playback?.duration,
  position:session?.audio?session.audio.currentTime+(session.mediaClockBase||0):Number(playback?.position)||0,
  hostPosition:Number(playback?.position)||0,
  running:!!playback?.playing&&!playback.paused&&!!session&&receiverView(session,playback).audible,
}));
const mediaPicker = createMediaPicker(requestCommand);
function requestCommand(action,value) {
  if(action==="testAudioSuspend"||action==="testAudioResume") {
    const owner=session;if(owner?.controlOnly)return Promise.reject(Error("仅控制模式不提供手机试听"));if(!owner)return Promise.reject(Error("连接已断开"));
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
let disconnecting=Promise.resolve();
let serverControlOnly=false;
let session = null, playback = null, playlistSignature = "", lastVolumeEdit = 0;
const mediaActions = ["play", "pause", "previoustrack", "nexttrack", "stop"];
function updateSystemPlayback(force=false) {
  if (!navigator.mediaSession) return;
  try {
    const state=!session||session.closed||!session.mediaActivated ? "none" : !session.ready ? "paused" : (session.controlOnly ? (session.controlIntent?.running ?? (!!playback?.playing&&!playback.paused)) : receiverView(session,playback).audible) ? "playing" : "paused";
    if(force||navigator.mediaSession.playbackState!==state)navigator.mediaSession.playbackState=state;
    if(session?.mediaActivated&&session.controlOnly)updateSystemPosition(Number(playback?.position)||0);
  } catch { /* Optional platform integration must not break PCM playback. */ }
}
function updateSystemPosition(position){
  if(!navigator.mediaSession?.setPositionState)return;
  const duration=Number(playback?.duration);
  try{
    if(Number.isFinite(duration)&&duration>0){
      navigator.mediaSession.setPositionState({duration,position:Math.max(0,Math.min(duration,Number.isFinite(position)?position:0)),playbackRate:1});
    }else navigator.mediaSession.setPositionState();
  }catch{/* Older Safari may not implement the position API. */}
}
function startSystemPlayback(owner) {
  if(owner.systemPlaybackStarted)return;owner.systemPlaybackStarted=true;
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
    play: () => systemTransport(owner,"play"),
    pause: () => systemTransport(owner,"pause"),
    previoustrack: () => { if (session === owner) command("previous"); },
    nexttrack: () => { if (session === owner) command("next"); },
    stop: () => systemTransport(owner,"pause"),
  };
  for (const action of mediaActions) {
    try { navigator.mediaSession.setActionHandler(action, actions[action]); } catch { /* Action unavailable on this browser. */ }
  }
}
// System controls remain installed while transport reconnects. Retain only the
// latest explicit play/pause intent, never replay old track or seek commands.
function systemTransport(owner,action){
  if(session!==owner||owner.closed||owner.canControl===false)return;
  if(action==="pause"){
    owner.audio?.pause();owner.pcmOutput?.pause();
    owner.controlMedia?.pause();
  }
  if(owner.ready){
    if(action==="play"&&!owner.controlOnly)void resumeAudio(owner).catch(()=>stats(owner));
    command(action);return;
  }
  owner.systemIntent=action;
  if(action==="play")void resumeAudio(owner).catch(()=>stats(owner));
  if(owner.audio)openHlsControl(owner);
  else if(!owner.reconnect)recoverPcm(owner,true);
}
function flushSystemIntent(owner){
  const action=owner.systemIntent;owner.systemIntent=null;
  if(action&&session===owner&&!owner.closed&&owner.ready)command(action);
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
  for (const el of document.querySelectorAll(".transport button,#player-settings button:not([data-tool]):not(#theme),#local-mute,#volume-balance,#volume,#queue button,#media-open")) el.disabled = !enabled || session?.canControl===false&&!el.matches("#play,#media-open");
}
function wire(kind, value) {
  const bytes = encoder.encode(JSON.stringify(value));
  const frame = new Uint8Array(5 + bytes.length); frame[0] = kind.charCodeAt(0);
  new DataView(frame.buffer).setUint32(1, bytes.length, true); frame.set(bytes, 5); return frame;
}
function send(kind, value, owner = session) {
  if (!owner || owner.closed || owner.socket?.readyState !== WebSocket.OPEN) return false;
  const transport=owner.rtc?.channel.readyState==='open'?owner.rtc.channel:owner.socket;
  if (transport.bufferedAmount > 65536) { if(owner.audio){owner.socket.close();return false;} recoverPcm(owner); return false; }
  transport.send(wire(kind, value)); return true;
}
function command(action, value) {
  if(session?.canControl===false&&!["scene","artwork","mediaList"].includes(action)){message("此设备仅允许收听");return;}
  if (!session?.ready) return;
  const owner = session;
  if (owner.pending.size >= 16) { message("主机正在处理操作，请稍候"); return; }
  if(!owner.controlOnly&&["track","next","previous"].includes(action)){beginTrackSwitch(owner);updateReceiverView();}
  const id = crypto.randomUUID();
  if(owner.controlOnly&&["play","pause","next","previous","track","replay","mediaOpen"].includes(action)){
    owner.controlLastAction=action;owner.controlIntent={id,running:action!=="pause"};
    if(action==="pause")owner.controlMedia?.pause();
    else void resumeAudio(owner).then(()=>{if(session===owner)stats(owner);}).catch(()=>{});
  }
  if(!owner.controlOnly&&["track","next","previous"].includes(action))owner.switchCommand=id;
  const timer = setTimeout(() => { if(owner.controlIntent?.id===id){owner.controlIntent=null;syncControlMedia(owner,playback);}if(owner.switchCommand===id){cancelTrackSwitch(owner);syncHostPlayback(owner,playback??{},resumeAudio,()=>stats(owner));}owner.pending.delete(id); controlResults.get(id)?.reject(Error("主机未确认操作，请检查当前配置"));controlResults.delete(id);soundTools.acknowledged(id,"主机尚未确认操作，请检查当前配置后重试"); if (session === owner) message("主机尚未确认操作，请稍后重试", true); }, action==="roomGenerate"?611000:16000);
  owner.pending.set(id, timer);
  if(owner.audio&&owner.socket?.readyState!==WebSocket.OPEN){
    void fetch(`/hls/${owner.hls}/control`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({id,command:{action,value}})}).then(async res=>{if(!res.ok)throw Error("远程控制连接已失效");const reply=await res.json();if(session===owner)receive("D",encoder.encode(JSON.stringify(reply)),owner);}).catch(error=>message(error.message,true));
    return id;
  }
  if (!send("C", {id, command:{action, value}}, owner)) { clearTimeout(timer); owner.pending.delete(id); if(owner.controlIntent?.id===id){owner.controlIntent=null;syncControlMedia(owner,playback);}if(owner.switchCommand===id){cancelTrackSwitch(owner);syncHostPlayback(owner,playback??{},resumeAudio,()=>stats(owner));} return; }
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
installMoreMenu($("player-settings"));
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
$("volume-balance").addEventListener("change",()=>command("volumeBalance",$("volume-balance").checked));
$("local-mute").addEventListener("change",()=>command("localMute",$("local-mute").checked));
function renderState(state) {
  if(Number.isSafeInteger(state?.revision)&&session){if(state.revision<(session.stateRevision??-1))return;session.stateRevision=state.revision;}
  if (!state || !Array.isArray(state.playlist) || state.playlist.length > 500 || typeof state.title !== "string") throw Error("主机状态无效");
  if(session){
    const key=JSON.stringify([state.currentId,state.title]);
    if(session.audio&&session.trackKey!==undefined&&session.trackKey!==key&&!session.switchingTrack)beginTrackSwitch(session,!(session.mediaPending&&session.mediaEpoch!==session.loadedMediaEpoch));
    session.trackKey=key;
  }
  playback = state;
  if(session&&!session.controlOnly&&state.playing)session.mediaActivated=true;
  if(session)session.songDuration=Number(state.duration)||0;
  if(session&&state.playing&&!state.loading)session.pendingStart=false;
  const source=state.source;
  const codecs={"ec-3":"E-AC-3","eac3":"E-AC-3","ac-3":"AC-3","ac3":"AC-3","ac-4":"AC-4","ac4":"AC-4","av3a":"Audio Vivid","mlpa":"TrueHD","truehd":"TrueHD","alac":"ALAC","mp4a":"AAC","pcm":"PCM","adm":"ADM PCM"};
  const fields=[];
  if(source?.codec)fields.push(codecs[source.codec.toLowerCase()]||source.codec.toUpperCase());
  if(source?.sampleRate>0)fields.push(`${source.sampleRate/1000} kHz`);
  if(source?.channels>0)fields.push(`${source.channels} 声道`);
  if(source?.objects>0)fields.push(`${source.objects} 对象`);
  $("format-badge").textContent=fields.join(" · ")||"等待歌曲信息";
  $("local-mute").checked=state.localMuted!==false;
  $("volume-balance").checked=state.volumeBalanceEnabled===true;
  if(session?.controlOnly)syncControlMedia(session,state);
  if(!session?.controlOnly)syncHostPlayback(session,state,resumeAudio,()=>{if(session)stats(session);});
  if(session?.pcmOutput){
    const owner=session,running=!!state.playing&&!state.paused;
    if(owner.pcmHostRunning!==running){owner.pcmHostRunning=running;if(running)void resumeAudio(owner).catch(()=>stats(owner));else owner.pcmOutput.pause();}
  }
  updateCover(state,session);
  sceneView.updateLayout(state.layoutSelection,!!session?.ready&&session?.canControl!==false);
  soundTools.update(state.tools&&session?.canControl===false?{...state.tools,locked:true}:state.tools, !!session?.ready, state.playing&&!state.paused);
  try {
    if (session?.mediaActivated && navigator.mediaSession && typeof MediaMetadata !== "undefined" && session?.mediaTitle !== JSON.stringify([state.currentId,state.title,state.artist,state.album])) {
      navigator.mediaSession.metadata = new MediaMetadata({title:state.title || "SDA 远程收听", artist:state.artist || "", album:state.album || "",artwork:session?.coverSrc?[{src:session.coverSrc,type:"image/jpeg"}]:[]});
      if (session) session.mediaTitle = JSON.stringify([state.currentId,state.title,state.artist,state.album]);
    }

  } catch { /* Metadata support varies independently from audio support. */ }
  updateSystemPlayback();
  $("title").textContent = state.title || "等待主机选择歌曲";
  $("artist").textContent = state.artist || ""; $("artist").hidden = !state.artist;
  document.title = state.title ? `${state.title} · SDA` : "SDA · 远程收听";
  updateReceiverView();
  if (Date.now() - lastVolumeEdit > 400) {
    $("volume").value = Math.round(state.volume * 100); $("volume-label").textContent = `${$("volume").value}%`;
  }
  updatePlayButton();
  $("render-setting").hidden=state.stereoAvailable!==true;
  if(state.stereoAvailable!==true){$("render-menu").hidden=true;$("render").setAttribute("aria-expanded","false");}
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
      button.disabled=session?.canControl===false;
      button.setAttribute("aria-current", String(track.id === state.currentId)); button.ariaLabel = `播放 ${track.title}`;
      button.addEventListener("click", () => command("track", track.id)); button.append(number, title, current); row.append(button); fragment.append(row);
    });
    $("queue").replaceChildren(fragment);pages.update(); $("queue-count").textContent = `${state.playlist.length} 首`; $("empty-queue").hidden = state.playlist.length > 0;
  }
}
function browserNeedsPlay() {
  if(session?.controlOnly)return false;
  return !!session && (session.audio ? session.audio.paused : session.context?.state !== "running"||session.pcmOutput?.audio.paused);
}
function updatePlayButton() {
  const playing=!!playback?.playing&&!playback.paused&&!browserNeedsPlay();
  $("play").ariaLabel=$("play").title=playing?"暂停":"播放";
  $("play-icon").toggleAttribute("hidden",playing);$("pause-icon").toggleAttribute("hidden",!playing);
}
function updateReceiverView() {
  if(!playback)return;
  const view=session?.controlOnly?{label:playback.loading?"主机加载中":playback.playing&&!playback.paused?"电脑播放中":playback.paused?"已暂停":"已连接 · 仅控制",buffering:!!playback.loading,position:Number(playback.position)||0}:receiverView(session,playback);
  $("transport-status").textContent=view.label;
  $("transport-status").classList.toggle("buffering",view.buffering);
  $("play").setAttribute("aria-busy",String(view.buffering));
  $("progress").max=Math.max(playback.duration||0,view.position,1);
  $("progress").value=view.position;
  $("position").textContent=time(view.position);$("duration").textContent=time(playback.duration);
  if(!session?.controlOnly)updateSystemPosition(view.position);
  updateSystemPlayback();
}
function stats(owner) {
  if (session !== owner || owner.closed) return;
  updateReceiverView();
  updatePlayButton();
  soundTools.playbackProgress();
  if(owner.controlOnly){$("connection").textContent="仅控制";$("audio-info").textContent="声音在电脑播放 · 不传输音频";return;}
  if(owner.audio){
    if(!owner.startupProbe)recoverSynchronizedMedia(owner);
    void owner.prefetch?.update(owner,playback?.paused===true);
    if(owner.ready&&Date.now()-(owner.feedbackAt??0)>=(owner.synchronized?200:1000)){
      owner.feedbackAt=Date.now();const audio=owner.audio,ahead=bufferedAhead(audio);
      const prepared=prepareSynchronizedMedia(owner);
      const startupReady=prepared&&calibrateStartup(owner,()=>stats(owner));
      send("K",{clock:localClock(),receiver:{epoch:owner.mediaEpoch,ranges:mediaRanges(audio),clockRtt:owner.clockRtt,startDelayMs:owner.syncStartDelay,timerLateMs:owner.syncTimerLateMs,syncPosition:owner.syncPosition,mediaClockBase:owner.mediaClockBase,aheadMs:Math.round(ahead*1000),time:audio.currentTime,readyState:audio.readyState,waiting:owner.mediaWaiting!==false,paused:audio.paused,pending:!!owner.mediaPending,hidden:document.hidden,revision:owner.syncRevision,syncPhase:owner.syncPhase,syncReady:startupReady,startupLeadMs:owner.startupLeadMs,hostTime:Number.isFinite(owner.clockOffset)?localClock()+owner.clockOffset:undefined}},owner);
    }
    $("connection").textContent=owner.testing?"耳廓测试中":owner.audio.paused?(owner.hostRunning===false?(owner.prefetch?.controller?"已暂停 · 正在预缓存":"已暂停"):"点击播放收听"):owner.audio.readyState<3?"缓冲中":owner.socket?.readyState===WebSocket.OPEN?"已连接":"正在收听 · 控制重连中";
    $("audio-info").textContent="原生媒体播放 · 数秒缓冲延迟 · 浏览器和系统可能重采样";
    return;
  }
  const suspended = owner.context.state !== "running";
  $("connection").textContent = owner.testing ? "耳廓测试中" : suspended ? "浏览器已暂停音频" : owner.buffering ? "缓冲中" : "已连接";
  $("audio-info").textContent = `${owner.rtc?.channel.readyState==='open'?(owner.rtc.compressed?'WebRTC 无损压缩':'WebRTC PCM'):'WebSocket'} · 浏览器音频 ${owner.context.sampleRate / 1000} kHz · 缓冲 ${Math.round(owner.queued / 48)} ms · 已接收 ${(owner.bytes / 1048576).toFixed(1)} MB`;
}
function receive(kind, body, owner) {
  owner.lastHost = Date.now();
  clearTimeout(owner.foregroundProbe);owner.foregroundProbe=null;
  if (kind === "A") {
    if (!owner.ready || body.length !== FRAMES * 8) throw Error("PCM 数据格式不匹配");
    const samples = body.slice().buffer;
    owner.bytes += body.length; owner.node.port.postMessage({type:"pcm", samples}, [samples]); return;
  }
  if (kind === "R") {
    if (!owner.ready || body.length) throw Error("无效音频重置");
    owner.buffering=true;owner.mediaPending=false;owner.switchingTrack=false;stats(owner);
    owner.node.port.postMessage({type:"reset"}); return;
  }
  const value = JSON.parse(decoder.decode(body));
  if(kind==='Y'&&owner.audio){synchronizeMedia(owner,value,()=>stats(owner));return;}
  if (kind === "H" && owner.controlOnly) {
    if(value.protocol!==1||value.controlOnly!==true||value.canControl!==true)throw Error("主机不支持仅控制或未授予控制权限");
    owner.ready=true;owner.reconnectAttempts=0;owner.controlIntent=null;owner.canControl=true;clearTimeout(owner.connectDeadline);send("H",{protocol:1},owner);
    $("pairing").hidden=true;pages.show();setControls(true);message("");stats(owner);
    $("local-mute").disabled=true;$("transport-format").textContent="连接：仅控制";$("format-note").textContent="不接收音频，不占用手机音频播放";flushSystemIntent(owner);return;
  }
  if (kind === "H") {
    if ((!owner.audio && owner.ready) || value.protocol !== 1 || value.sampleRate !== 48000 || value.channels !== 2 || value.sampleFormat !== (owner.audio?"hls-flac24":"f32le")) throw Error("主机音频格式不兼容");
    if(!owner.audio)owner.node.port.postMessage({type:"configure",bufferMs:value.bufferMs});
    if(owner.audio){
      if(typeof value.stream!=="string"||!new RegExp(`^/hls/${owner.hls}/e[0-9]+/index\\.m3u8$`).test(value.stream))throw Error("无效媒体地址");
      owner.synchronized=value.synchronized===true;
      acceptHlsMedia(owner,value);
      syncHostPlayback(owner,playback??{},resumeAudio,()=>stats(owner));
    }
    owner.canControl=value.canControl!==false;
    clearTimeout(owner.connectDeadline);
    owner.ready = true; if(!owner.audio)send("H", {protocol:1}, owner);
    $("pairing").hidden = true; pages.show();
    setControls(true); message(""); stats(owner);flushSystemIntent(owner);
    $("transport-format").textContent=owner.audio?"传输：FLAC · 48 kHz / 24-bit":owner.rtc?.compressed?"传输：无损压缩 PCM · 48 kHz / 32-bit float":"传输：PCM · 48 kHz / 32-bit float";
    $("format-note").textContent=owner.audio?"float32 转 24-bit PCM 后无损编码；超出整数满幅会截断，无增益或响度处理。":"网络原样传输 · 浏览器和系统可能重采样";
  } else if (kind === "S") {const {stateDelta,...state}=value;renderState(stateDelta?{...playback,...state}:state);}
  else if (kind === "D") {
    const result=controlResults.get(value.id);if(value.error)result?.reject(Error(value.error));else result?.resolve(value.data);controlResults.delete(value.id);
    soundTools.acknowledged(value.id,value.error,value);
    clearTimeout(owner.pending.get(value.id)); owner.pending.delete(value.id);
    if (value.error) {if(owner.controlIntent?.id===value.id){owner.controlIntent=null;syncControlMedia(owner,playback);}if(value.id===owner.switchCommand){cancelTrackSwitch(owner);syncHostPlayback(owner,playback??{},resumeAudio,()=>stats(owner));}owner.pendingStart=false;stats(owner);message(value.error, true);}
  } else if (kind === "E") {if(value.retryable===true&&!owner.audio){recoverPcm(owner);return;}throw Error(value.error || "主机连接失败");}
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
  if(owner?.controlOnly){
    if(!owner.controlMedia)owner.controlMedia=createControlMedia();
    startSystemPlayback(owner);startControlKeepAlive(owner);try{await owner.controlMedia.play();owner.controlMediaError="";}catch(error){owner.controlMediaError=String(error?.name||error).slice(0,80);message("手机系统媒体控制启动失败，电脑播放指令已发送",true);throw error;}owner.mediaActivated=true;
    owner.mediaTitle=undefined;if(session===owner&&playback?.playing&&!playback.paused)renderState(playback);updateSystemPlayback();return;
  }
  if(owner.synchronized){send('K',{resync:true,clock:localClock()},owner);return;}
  if(!owner.audio){await owner.context.resume();await owner.pcmOutput?.play();return;}
  const audio=owner.audio;
  if(owner.mediaPending||!audio.getAttribute("src"))return;
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
    if(owner.synchronized){clearTimeout(owner.syncTimer);clearTimeout(owner.connectDeadline);owner.syncPhase='holding';owner.syncWaiting=true;owner.audio.pause();}
    setControls(false);
    if(event.code===1008){stop(event.reason||"收听会话失效",true);return;}
    stats(owner);owner.reconnect=setTimeout(()=>openHlsControl(owner),2000);
  };
}
function showRememberedDevice(status){
  serverControlOnly=status.canListen===false;
  const remembered=status?.status==="authorized"&&!status.legacy;
  for(const id of ["invite","device-name"]){
    $(id).hidden=remembered;
    document.querySelector(`label[for="${id}"]`).hidden=remembered;
  }
  $("remembered-device").hidden=!remembered;
  $("remembered-device").textContent=remembered?`已记住此设备${status.name?` · ${status.name}`:""}，无需再次输入密钥。`:"";
  $("connect").textContent=remembered?"重新连接主机":"连接主机";
  if(remembered){$("invite").value="";try{sessionStorage.removeItem("sda-web-pairing");}catch{}}
}
let autoConnectAttempted=false,explicitlyDisconnected=false;
try{explicitlyDisconnected=localStorage.getItem("sda-web-disconnected")==="1";}catch{}
async function checkRememberedDevice(auto=false){
  try{
    const response=await fetch("/pair/status",{cache:"no-store"});
    if(!response.ok)return;
    const status=await response.json();if(!session&&!explicitlyDisconnected)showRememberedDevice(status);
    if(auto&&!session&&!autoConnectAttempted&&!explicitlyDisconnected&&status.status==="authorized"&&!status.legacy){autoConnectAttempted=true;void connect();}
  }catch{/* Keep manual pairing available when the host cannot be reached. */}
}
async function authorizeDevice(owner,enteredKey){
  const status=await fetch("/pair/status").then(r=>{if(!r.ok)throw Error("无法检查设备授权");return r.json();});
  showRememberedDevice(status);
  enforceControlPermission(owner,status);
  try{localStorage.setItem("sda-web-device-name",$("device-name").value);}catch{}
  if(status.status==="authorized")return status.legacy?await tokenFrom(enteredKey):"";
  const token=await tokenFrom(enteredKey);
  const response=await fetch("/pair/request",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({token,name:$("device-name").value})});
  const requested=await response.json();if(!response.ok)throw Error(requested.error||"配对请求失败");
  if(requested.status==="authorized"){enforceControlPermission(owner,await fetch("/pair/status").then(r=>r.json()));return "";}
  message("请在电脑的远程设置中批准此设备，可选择仅收听或允许控制。");
  for(let i=0;i<120;i++){
    await new Promise(resolve=>setTimeout(resolve,1000));if(owner.closed||session!==owner)throw Error("连接已取消");
    const next=await fetch("/pair/status").then(r=>r.json());
    if(next.status==="authorized"){enforceControlPermission(owner,next);message("设备已授权，正在连接…");try{sessionStorage.removeItem("sda-web-pairing");}catch{}return "";}
    if(next.status!=="pending")throw Error("配对已拒绝或过期，请重新申请");
  }
  throw Error("等待授权超时，请重新连接");
}
async function connectHls(enteredKey){
  const audio=document.createElement("audio");audio.preload="auto";audio.setAttribute("playsinline","");audio.hidden=true;document.body.append(audio);
  const owner=session={audio,closed:false,ready:false,pending:new Map(),socket:null,prefetch:new PausePrefetch()};
  startSystemPlayback(owner);$("disconnect").hidden=false;
  try{
    owner.key=await authorizeDevice(owner,enteredKey);
    if(owner.closed||session!==owner)return;
    await disconnecting;
    const response=await fetch("/hls/session",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({token:owner.key})});
    if(!response.ok)throw Error(response.status===403?"主机未允许 HLS 或配对已失效，请刷新网页重新连接":response.status===409?"已达到主机设置的连接上限，或此设备已在其他页面收听":"无法建立无损媒体流，请检查配对密钥和主机");
    const info=await response.json();owner.hls=info.id;
    if(owner.closed||session!==owner){void fetch(`/hls/${info.id}/stop`,{method:"POST",keepalive:true});return;}
    try{sessionStorage.setItem("sda-web-pairing",owner.key);}catch{}
    owner.mediaPending=true;
    for(const name of ["playing","pause","waiting","seeking","seeked","canplay","timeupdate","emptied"])
      audio.addEventListener(name,()=>{
        if(["waiting","seeking","emptied","pause"].includes(name))owner.mediaWaiting=true;
        if(!owner.mediaPending&&["playing","timeupdate"].includes(name)&&!audio.paused&&!audio.seeking&&audio.readyState>=3)owner.mediaWaiting=false;
        stats(owner);
      });
    audio.addEventListener("error",()=>{if(audio.error&&!owner.mediaPending&&session===owner)stop("此浏览器无法播放 FLAC 无损 HLS，或媒体连接中断。未切换到有损音频。",true);});
    openHlsControl(owner);
    await resumeAudio(owner).catch(()=>{if(audio.paused&&owner.hostRunning!==false)message("点击播放按钮开始收听");stats(owner);});
    if(owner.hostRunning===false)audio.pause();
    owner.timer=setInterval(()=>{if(owner.audio.paused&&!owner.synchronized)send("K",{},owner);stats(owner);},250);
  }catch(error){if(error.controlOnlyRequired){useControlOnly();return;}if(session===owner)stop(error.message,true);}
}
function recoverPcm(owner, immediate=false){
  if(session!==owner||owner.closed||owner.reconnect)return;
  // Transport loss preserves authorization and the media session. Only an
  // explicit disconnect or authorization rejection ends the session.
  owner.ready=false;owner.buffering=true;owner.pcmHostRunning=undefined;
  owner.pcmOutput?.pause();owner.node?.port.postMessage({type:"hold",enabled:true});
  clearTimeout(owner.connectDeadline);
  clearTimeout(owner.foregroundProbe);owner.foregroundProbe=null;
  owner.rtc?.close();owner.rtc=null;
  const old=owner.socket;owner.socket=null;old?.close();
  for(const [id,timer]of owner.pending){clearTimeout(timer);controlResults.get(id)?.reject(Error("连接中断，请确认操作结果后重试"));controlResults.delete(id);}
  owner.pending.clear();soundTools.disconnected();setControls(false);updateSystemPlayback();
  $("connection").textContent="正在重连";message("连接中断，正在重新连接主机…");
  const delay=immediate?0:Math.min(8000,1000*2**Math.min(3,owner.reconnectAttempts??0));owner.reconnectAttempts=(owner.reconnectAttempts??0)+1;
  owner.reconnect=setTimeout(()=>{owner.reconnect=null;if(session===owner&&!owner.closed)openPcmSocket(owner);},delay);
}
function openPcmSocket(owner){
  if(session!==owner||owner.closed)return;
  owner.ready=false;owner.consumed=0;owner.queued=0;owner.bytes=0;owner.lastHost=Date.now();
  owner.pcmEpoch=(owner.pcmEpoch??0)+1;
  owner.node?.port.postMessage({type:"new-session",epoch:owner.pcmEpoch});
    const socket = owner.socket = new WebSocket(`wss://${location.host}/stream`); socket.binaryType = "arraybuffer";
    socket.onopen = async () => {
      if (owner.closed || session!==owner || owner.socket!==socket) { socket.close(); return; }
      if(!owner.controlOnly&&!owner.rtcFailed)owner.rtc=await prepareRtc(event=>socket.onmessage(event),()=>{owner.rtcFailed=true;recoverPcm(owner);});
      if(owner.closed||session!==owner||owner.socket!==socket){owner.rtc?.close();return;}
      socket.send(JSON.stringify({protocol:1, token:owner.key,controlOnly:owner.controlOnly===true,pcmPipeline:!owner.controlOnly,stateDelta:true,...(owner.rtc?{rtcOffer:owner.rtc.offer}:{})}));
      try { sessionStorage.setItem("sda-web-pairing", owner.key); } catch { /* Private browsing can disable storage. */ }
    };
    let pending = new Uint8Array(0);
    socket.onmessage = event => {
      if (owner.closed || session !== owner || owner.socket!==socket) return;
      try {
        if(typeof event.data==='string'){
          const signal=JSON.parse(event.data);
          if(signal.type==='answer'&&owner.rtc)void owner.rtc.answer(signal.sdp).catch(()=>{owner.rtcFailed=true;recoverPcm(owner);});
          else if(signal.type==='fallback'){owner.rtc?.close();owner.rtc=null;owner.rtcFailed=true;}
          else throw Error('无效 WebRTC 协商');
          return;
        }
        if (!(event.data instanceof ArrayBuffer)) throw Error("主机未发送二进制 PCM 数据");
        const chunk = new Uint8Array(event.data), joined = new Uint8Array(pending.length + chunk.length);
        joined.set(pending); joined.set(chunk, pending.length); pending = joined;
        while (pending.length >= 5) {
          const length = new DataView(pending.buffer, pending.byteOffset + 1, 4).getUint32(0, true);
          if (length > MAX_PACKET) throw Error("主机数据包超过上限");
          if (pending.length < length + 5) break;
          const kind = String.fromCharCode(pending[0]), body = pending.subarray(5, 5 + length);
          pending = pending.subarray(5 + length); receive(kind, body, owner);
          if(session!==owner||owner.socket!==socket||owner.closed)return;
        }
        if (pending.length > MAX_PACKET + 5) throw Error("网络缓冲超过上限");
      } catch (error) { stop(error.message, true); }
    };
    socket.onerror = () => { if (session === owner) message("无法连接主机，请检查 HTTPS 证书、主机状态及网络", true); };
    socket.onclose = event => {
      if(session!==owner||owner.socket!==socket||owner.closed)return;
      clearTimeout(owner.connectDeadline);
      if(event.code===1008){stop(event.reason||"设备授权已失效，请重新连接",true);return;}
      recoverPcm(owner);
    };
    owner.connectDeadline=setTimeout(()=>{if(session===owner&&owner.socket===socket&&!owner.ready)recoverPcm(owner);},12000);
}
async function connect() {
  if (session) return;
  const enteredKey = $("invite").value;
  $("invite").blur();
  const controlOnly=serverControlOnly;
  const nativeHls = hlsAllowedByHost && !!document.createElement("audio").canPlayType("application/vnd.apple.mpegurl");
  if (!window.isSecureContext || (!controlOnly && !nativeHls && !window.AudioWorkletNode)) { message("此浏览器未提供安全音频环境。请使用 HTTPS 链接，在 Chrome、Edge、Firefox 或 Safari 新版中打开。", true); return; }
  $("connect").disabled = true; message(controlOnly?"正在连接主机控制…":"正在连接音频…");
  let owner;
  try {
    if(controlOnly){
      owner=session={controlOnly:true,closed:false,ready:false,pending:new Map(),lastHost:Date.now()};
      $("disconnect").hidden=false;owner.key=await authorizeDevice(owner,enteredKey);
      if(session!==owner||owner.closed)return;openPcmSocket(owner);
      const watchdog=new ConnectionWatchdog();
      owner.timer=setInterval(()=>{if(session!==owner||!owner.ready)return;if(watchdog.check(Date.now(),owner.lastHost,document.hidden)==="expired"){recoverPcm(owner);return;}send("K",{mediaState:{activated:owner.mediaActivated===true,paused:owner.controlMedia?.paused??true,readyState:owner.controlMedia?.readyState??0,error:owner.controlMediaError??"",hidden:document.hidden,keepAlive:owner.keepAliveContext?.state??"unavailable",keepAliveTime:owner.keepAliveContext?.currentTime??0,keepAliveError:owner.keepAliveError??""}},owner);stats(owner);},1000);return;
    }
    if(nativeHls){await connectHls(enteredKey);return;}
    const context = new AudioContext({sampleRate:48000, latencyHint:"playback"});
    owner = session = {context, node:null, socket:null, closed:false, ready:false, consumed:0, queued:0, bytes:0, buffering:true, pending:new Map(), lastHost:Date.now()};
    startSystemPlayback(owner);
    owner.pcmOutput=createPcmMediaOutput(context,()=>{if(session===owner)stats(owner);});
    $("disconnect").hidden = false;
    // Called within the click gesture, before asynchronous module/network work.
    void context.resume().catch(()=>{});
    // Connecting is not playback. The primary play gesture or an already
    // playing host starts the media element; idle connections stay out of Now Playing.
    const key = await authorizeDevice(owner,enteredKey);
    if (context.sampleRate !== 48000) throw Error("当前浏览器无法创建 48 kHz 音频输出，请换用新版 Chrome 或 Edge");
    await context.audioWorklet.addModule("/pcm-worklet.mjs");
    if (session !== owner || owner.closed) return;
    const node = owner.node = new AudioWorkletNode(context, "sda-remote-pcm", {numberOfInputs:0, numberOfOutputs:1, outputChannelCount:[2], channelCount:2, channelCountMode:"explicit", channelInterpretation:"discrete"});
    node.connect(owner.pcmOutput.destination);
    node.onprocessorerror = () => { if (session === owner) stop("浏览器音频处理已中断，请重新连接", true); };
    node.port.onmessage = ({data}) => {
      if (owner.closed || session !== owner || data.epoch!==owner.pcmEpoch) return;
      if (data.type === "error") { stop(data.detail || "音频缓冲异常", true); return; }
      if (data.type === "progress") {
        const changed=owner.buffering!==data.buffering;
        if(data.consumed>0&&!data.buffering)owner.reconnectAttempts=0;
        owner.consumed = data.consumed; owner.queued = data.queued; owner.buffering = data.buffering;
        if(changed)updateSystemPlayback();
        if (owner.ready){
          const report=changed||Date.now()-(owner.mediaReportAt??0)>5000;
          if(report)owner.mediaReportAt=Date.now();
          send("K", {consumed:owner.consumed,queuedFrames:owner.queued,...(report?{mediaState:{hidden:document.hidden,context:context.state,playback:navigator.mediaSession?.playbackState??'unavailable',buffering:owner.buffering,queuedMs:Math.round(owner.queued/48),receivedFrames:owner.bytes/8,workletReceivedFrames:data.received,feedbackAgeMs:Math.max(0,(context.currentTime-data.clock)*1000),contextRate:context.sampleRate,mediaRate:owner.pcmOutput.audio.playbackRate,mediaElement:true,mediaPaused:owner.pcmOutput.audio.paused,mediaReadyState:owner.pcmOutput.audio.readyState}}:{})}, owner);
        }
      }
    };
    context.onstatechange = () => stats(owner);
    owner.key=key;openPcmSocket(owner);
    const watchdog=new ConnectionWatchdog();
    owner.timer = setInterval(() => {
      if (session !== owner) return;
      const health=watchdog.check(Date.now(),owner.lastHost,document.hidden);
      if(owner.reconnect||!owner.ready)return;
      if(health==="expired"){recoverPcm(owner);return;}
      if(health==="probing"){owner.connectionProbing=true;message("正在确认主机连接…");}
      else if(health==="alive"&&owner.connectionProbing){owner.connectionProbing=false;message("");}
      if (owner.ready) send("K", {consumed:owner.consumed}, owner); stats(owner);
    }, 1000);
  } catch (error) { if(error.controlOnlyRequired){useControlOnly();return;}if (!owner || session === owner) stop(error.message || "无法启动浏览器音频", true); }
}
function stop(reason = "已断开连接", error = false) {
  for(const result of controlResults.values())result.reject(Error(reason));controlResults.clear();
  soundTools.disconnected();mediaPicker.close();
  const owner = session; session = null;pages.hide();$("cover").hidden=true;$("cover").removeAttribute("src");$("cover-placeholder").hidden=false;
  if (owner) {
    stopControlKeepAlive(owner);
    endSystemPlayback(owner);
    clearTimeout(owner.foregroundProbe);
    owner.closed = true; owner.startupProbe?.cancel(); clearInterval(owner.timer);clearTimeout(owner.syncTimer);clearTimeout(owner.connectDeadline);
    owner.prefetch?.cancel();
    owner.pcmOutput?.close();
    if(owner.controlMedia){owner.controlMedia.pause();const url=owner.controlMedia.src;owner.controlMedia.removeAttribute("src");owner.controlMedia.load();owner.controlMedia.remove();URL.revokeObjectURL(url);}
    for (const timer of owner.pending.values()) clearTimeout(timer);
    if (!owner.audio && owner.socket?.readyState === WebSocket.OPEN) owner.socket.send(wire("Q", {}));
    owner.rtc?.close();owner.socket?.close(1000); owner.node?.port.postMessage({type:"stop"}); owner.node?.disconnect();
    if(owner.audio){owner.audio.pause();owner.audio.removeAttribute("src");owner.audio.load();owner.audio.remove();
      if(owner.hls)disconnecting=fetch(`/hls/${owner.hls}/stop`,{method:"POST",keepalive:true}).then(()=>{},()=>{});
    }
    clearTimeout(owner.reconnect);
    void owner.context?.close().catch(() => {});
  }
  $("connect").disabled = false; $("pairing").hidden = false; $("disconnect").hidden = true;
  $("connection").textContent = "未连接"; setControls(false); message(reason, error);
  void checkRememberedDevice();
}
async function disconnectDevice(){
  explicitlyDisconnected=true;autoConnectAttempted=true;
  try{localStorage.setItem("sda-web-disconnected","1");sessionStorage.removeItem("sda-web-pairing");}catch{}
  $("invite").value="";stop();showRememberedDevice({status:"unpaired"});
  try{const response=await fetch("/pair/logout",{method:"POST",keepalive:true});if(!response.ok)throw Error();}
  catch{message("已断开；主机未确认清除授权，重新连接时会先清除旧授权。",true);}
}
$("connect").addEventListener("click", async () => {
  if(explicitlyDisconnected){
    if(!$("invite").value.trim()){message("请输入配对密钥重新连接",true);return;}
    try{const response=await fetch("/pair/logout",{method:"POST"});if(!response.ok)throw Error();}
    catch{message("无法清除旧授权，请确认主机在线后重试",true);return;}
    explicitlyDisconnected=false;try{localStorage.removeItem("sda-web-disconnected");}catch{}
  }
  void connect();
});
$("disconnect").addEventListener("click", () => void disconnectDevice());
$("play").addEventListener("click", () => {
  const pause=session?.controlOnly&&session.controlIntent?session.controlIntent.running:playback?.playing&&!playback.paused&&!browserNeedsPlay();
  if(session?.controlOnly){command(pause?"pause":"play");return;}
  if(session&&!pause&&session.canControl!==false){session.pendingStart=true;updateReceiverView();}
  if(session){const owner=session;if(pause){if(!owner.synchronized){owner.audio?.pause();if(!owner.controlOnly)owner.controlMedia?.pause();}}else void resumeAudio(owner).then(()=>{message("");stats(owner);}).catch(()=>stats(owner));}
  if(session?.canControl!==false)command(pause?"pause":"play");
});
for (const button of document.querySelectorAll("[data-action]")) button.addEventListener("click", () => command(button.dataset.action));
$("volume").addEventListener("input", () => { lastVolumeEdit = Date.now(); $("volume-label").textContent = `${$("volume").value}%`; });
$("volume").addEventListener("change", () => command("volume", Number($("volume").value) / 100));
// pagehide is a document lifecycle transition, not an explicit logout.
// Keep media metadata/handlers for Safari restoration. If the document really
// unloads, the browser releases its transports; host leases remain bounded.
function retainBackgroundSession(){
  const owner=session;
  if(!owner||owner.closed)return;
  clearTimeout(owner.foregroundProbe);owner.foregroundProbe=null;
  if(owner.ready&&!owner.audio)send("K",{consumed:owner.consumed??0,background:true},owner);
}
window.addEventListener("pagehide",retainBackgroundSession);
function restoreForegroundConnection(){
  const owner=session;
  if(!owner||owner.closed)return;
  if(document.hidden){
    retainBackgroundSession();return;
  }
  // Pairing and audio initialization must finish before opening a transport.
  if(!owner.socket&&!owner.reconnect&&!owner.ready)return;
  updateSystemPlayback(true);
  if(owner.audio){openHlsControl(owner);stats(owner);return;}
  // Probe an apparently open socket before replacing it after suspension.
  if(!owner.ready||owner.socket?.readyState!==WebSocket.OPEN){
    clearTimeout(owner.reconnect);owner.reconnect=null;
    recoverPcm(owner,true);return;
  }
  if(!owner.foregroundProbe){
    owner.foregroundProbe=setTimeout(()=>{owner.foregroundProbe=null;if(session===owner&&!owner.closed&&!document.hidden)recoverPcm(owner,true);},5000);
  }
  send("K",{consumed:owner.consumed??0,background:false},owner);
  if(!owner.controlOnly&&playback?.playing&&!playback.paused&&owner.context?.state!=="running"){
    void owner.context?.resume().catch(()=>stats(owner));
  }
  stats(owner);
}
window.addEventListener("pageshow",restoreForegroundConnection);
window.addEventListener("online",restoreForegroundConnection);
document.addEventListener("visibilitychange",restoreForegroundConnection);

let theme = "dark";
try { theme = localStorage.getItem("sda-web-theme") || (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark"); } catch { /* Keep default. */ }
document.documentElement.dataset.theme = theme;$("theme-value").textContent=theme==="light"?"浅色":"深色";
$("theme").addEventListener("click", () => { theme = theme === "light" ? "dark" : "light"; document.documentElement.dataset.theme = theme;$("theme-value").textContent=theme==="light"?"浅色":"深色"; try { localStorage.setItem("sda-web-theme", theme); } catch {} });
try {
  const key = /^#[a-f0-9]{64}$/.test(location.hash) ? location.hash.slice(1) : sessionStorage.getItem("sda-web-pairing") || "";
  $("invite").value = explicitlyDisconnected ? "" : key;
  if (location.hash) history.replaceState(null, "", location.pathname);
} catch { /* User can paste the full link into the pairing field. */ }
setControls(false);
try{$("device-name").value=localStorage.getItem("sda-web-device-name")||"";}catch{}
void checkRememberedDevice(true);
window.addEventListener("pageshow",()=>{if(!session)void checkRememberedDevice(true);});

initGlass();
for(const panel of document.querySelectorAll("#sound-tools,#media-picker")){
  let frame=0;
  const update=()=>{frame=0;panel.style.setProperty("--header-scroll",String(Math.min(1,Math.max(0,panel.scrollTop+(panel.querySelector('.media-page[aria-hidden="false"]')?.scrollTop??0))/32)));};
  panel.addEventListener("scroll",()=>{if(!frame)frame=requestAnimationFrame(update);},{passive:true,capture:true});
  if(panel.id==="media-picker"){const heading=panel.querySelector(".tools-heading");new ResizeObserver(()=>{if(heading.offsetHeight)panel.style.setProperty("--media-header-height",`${heading.offsetHeight}px`);}).observe(heading);}
  new MutationObserver(update).observe(panel,{attributes:true,attributeFilter:["open"]});
  update();
}

// Same rounded lens displacement as desktop GlassRefraction, generated only on resize.
// WebKit uses the CSS material: SVG backdrop filters are not portable there.
function initGlass(){
 const elements=[...document.querySelectorAll('#player-settings>summary,.player-settings-menu,.transport>button,.transport-loop>button,.dialog-close,.dialog-back')];
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
  const refreshGlass=()=>{cancelAnimationFrame(frame);frame=requestAnimationFrame(()=>{
   if(element.closest("[data-morphing]"))return;
   const {width,height}=element.getBoundingClientRect();if(width<1||height<1)return;const key=`${width}:${height}`;if(key===previous)return;previous=key;
   const canvas=document.createElement('canvas');canvas.width=Math.ceil(width);canvas.height=Math.ceil(height);const context=canvas.getContext('2d');if(!context)return;const pixels=context.createImageData(canvas.width,canvas.height);
   const radius=Math.min(parseFloat(getComputedStyle(element).borderRadius)||22,width/2,height/2),bevel=Math.min(40,height*.48);
   for(let y=0;y<canvas.height;y++)for(let x=0;x<canvas.width;x++){
    const px=x+.5-width/2,py=y+.5-height/2,qx=Math.abs(px)-(width/2-radius),qy=Math.abs(py)-(height/2-radius),ox=Math.max(qx,0),oy=Math.max(qy,0),length=Math.hypot(ox,oy),depth=radius-length-Math.min(Math.max(qx,qy),0),t=Math.max(0,Math.min(1,depth/bevel)),bend=16*t*t*(1-t)*(1-t),nx=length?ox/length:qx>qy?1:0,ny=length?oy/length:qx>qy?0:1,i=(y*canvas.width+x)*4;
    pixels.data[i]=Math.round(127.5-Math.sign(px)*nx*bend*110);pixels.data[i+1]=Math.round(127.5-Math.sign(py)*ny*bend*110);pixels.data[i+2]=128;pixels.data[i+3]=255;
   }
   context.putImageData(pixels,0,0);map.setAttribute('href',canvas.toDataURL());map.setAttribute('width',String(width));map.setAttribute('height',String(height));layer.style.backdropFilter=`url("#${id}")`;
  });};new ResizeObserver(refreshGlass).observe(element);element.addEventListener("glass-refresh",refreshGlass);
 });
}

$("device-name").value=/iPad/.test(navigator.userAgent)||(navigator.platform==="MacIntel"&&navigator.maxTouchPoints>1)?"iPad":/iPhone/.test(navigator.userAgent)?"iPhone":/Android/.test(navigator.userAgent)?"Android":"浏览器设备";

function enforceControlPermission(owner,status){
  serverControlOnly=status.canListen===false;
  if(serverControlOnly!==!!owner.controlOnly){const error=Error("主机设备权限已更新");error.controlOnlyRequired=true;throw error;}
}
function useControlOnly(){
  stop("正在按照主机权限连接…");void connect();
}

// An entirely local silent media session carries iOS controls; no song PCM is received.
function createControlMedia(){
  const rate=48000,length=rate*30*2,bytes=new ArrayBuffer(44+length),view=new DataView(bytes);
  const tag=(offset,value)=>{for(let i=0;i<value.length;i++)view.setUint8(offset+i,value.charCodeAt(i));};
  tag(0,"RIFF");view.setUint32(4,36+length,true);tag(8,"WAVE");tag(12,"fmt ");view.setUint32(16,16,true);
  view.setUint16(20,1,true);view.setUint16(22,1,true);view.setUint32(24,rate,true);view.setUint32(28,rate*2,true);
  view.setUint16(32,2,true);view.setUint16(34,16,true);tag(36,"data");view.setUint32(40,length,true);
  const media=document.createElement("audio");media.src=URL.createObjectURL(new Blob([bytes],{type:"audio/wav"}));
  media.loop=true;media.hidden=true;media.setAttribute("playsinline","");document.body.append(media);return media;
}


function syncControlMedia(owner,state){
  const running=!!state?.playing&&!state.paused;
  if(owner.controlIntent){
    if(owner.controlIntent.running!==running)return;
    owner.controlIntent=null;
  }
  if(!running)owner.controlMedia?.pause();
  else if(owner.mediaActivated&&owner.controlMedia?.paused)void owner.controlMedia.play().catch(error=>{owner.controlMediaError=String(error?.name||error).slice(0,80);});
}

// Experimental, independent of song/media-element clocks. A zero-filled buffer
// stays scheduled through host pause; no oscillator tone or song PCM is emitted.
function startControlKeepAlive(owner){
  if(session!==owner||owner.closed||!owner.controlOnly)return;
  try{
    const Context=globalThis.AudioContext||globalThis.webkitAudioContext;
    if(!Context)return;
    if(!owner.keepAliveContext||owner.keepAliveContext.state==="closed"){
      const context=owner.keepAliveContext=new Context({latencyHint:"playback"});
      const source=owner.keepAliveSource=context.createBufferSource();
      source.buffer=context.createBuffer(1,context.sampleRate,context.sampleRate);
      source.loop=true;source.connect(context.destination);source.start();
    }
    // Called synchronously from the user's play gesture; no retry loop fights
    // phone calls or another app taking audio focus.
    void owner.keepAliveContext.resume().then(()=>{
      if(session!==owner||owner.closed){stopControlKeepAlive(owner);return;}
      owner.keepAliveError="";
    }).catch(error=>{owner.keepAliveError=String(error?.name||error).slice(0,80);});
  }catch(error){owner.keepAliveError=String(error?.name||error).slice(0,80);stopControlKeepAlive(owner);}
}
function stopControlKeepAlive(owner){
  const source=owner.keepAliveSource,context=owner.keepAliveContext;
  owner.keepAliveSource=null;owner.keepAliveContext=null;
  try{source?.stop();source?.disconnect();}catch{}
  if(context&&context.state!=="closed")void context.close().catch(()=>{});
}
