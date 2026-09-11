"use strict";
const {Duplex}=require("node:stream");
const crypto=require("node:crypto");
const {encodeFrame,initSegment,mediaSegment}=require("./remote-hls-codec.cjs");
class HlsPeer extends Duplex {
  constructor({id,address,packet,decodePackets,onClose,diagnostic,sync}){
    super();this.id=id;this.cookie=crypto.randomBytes(32).toString("hex");this.remoteAddress=address;this.packet=packet;
    this.init=initSegment();this.segments=[];this.frames=[];this.frameIndex=0;this.sequence=0;this.baseTime=0;this.discontinuity=0;this.pendingDiscontinuity=false;this.epoch=0;this.positionBase=null;
    this.parts=[];this.fragmentSequence=0;
    this.encoded=0;this.released=0;this.started=Date.now();this.activity=Date.now();this.clipped=0;this.control=null;this.header=null;this.httpControls=new Map();
    this.sync=sync;this.diagnostic=diagnostic;this.lastAudioAt=0;this.maxAudioGapMs=0;this.lastDiagnosticAt=0;
    this.decode=decodePackets((kind,body)=>this.receive(kind,body));
    this.timer=setInterval(()=>{
      if(Date.now()-this.activity>90000){this.sdaFailure="HLS 播放器已超过 90 秒未请求音频";this.destroy();return;}
      // Bounded real-time producer. HTTP media requests, not page JS, hold the lease.
      // A joining listener must not over-pull PCM and overflow the first
      // listener's queue. Accumulate its media reserve at clock speed.
      const allowed=Math.floor(((Date.now()-this.started)/1000+1)*48000/480)*480;
      const next=Math.min(this.encoded,allowed);if(this.header){this.released=next;this.push(packet("K",{consumed:next}));}
    },50).unref();this.once("close",onClose);
  }
  setNoDelay(){return this;}
  _read(){}
  _write(bytes,_encoding,done){try{this.decode(bytes);done();}catch(e){done(e);}}
  _destroy(error,done){for(const result of this.httpControls.values()){clearTimeout(result.timer);result.resolve({error:"收听会话已断开"});}this.httpControls.clear();clearInterval(this.timer);this.control?.destroy();this.control=null;this.segments=[];this.frames=[];done(error);}
  touch(){this.activity=Date.now();}
  receive(kind,body){
    if(kind==="H"){
      this.header={...JSON.parse(body),sampleFormat:"hls-flac24",...this.mediaInfo()};this.push(this.packet("H",{protocol:1}));this.forward("H",this.header);return;
    }
    if(kind==="A"){
      if(this.programEnded){this.encoded+=480;return;}
      const now=Date.now();if(this.lastAudioAt&&this.latestState?.playing&&!this.latestState?.paused&&!this.latestState?.loading)this.maxAudioGapMs=Math.max(this.maxAudioGapMs,now-this.lastAudioAt);this.lastAudioAt=now;
      if(this.positionBase===null)this.positionBase=this.latestState?.loading?0:Math.max(0,Number(this.latestState?.position)||0);
      const frame=encodeFrame(body,this.frameIndex++);this.clipped+=frame.clipped;this.frames.push(frame.data);this.encoded+=480;
      if(this.frames.length%20===0)this.flushPart();
      if(this.frames.length>=100)this.flush();this.sealIfComplete();return;
    }
    if(kind==="R"){
      // Old songs must not remain addressable in either the server window or
      // Safari's media buffer. Each reset gets an isolated media URL namespace.
      this.epoch++;this.segments=[];this.frames=[];this.parts=[];this.fragmentSequence=0;this.frameIndex=0;this.sequence=0;this.baseTime=0;
      this.lastAudioAt=0;this.maxAudioGapMs=0;
      this.endSample=null;this.programEnded=false;
      this.sync?.reset();
      this.discontinuity=0;this.pendingDiscontinuity=false;this.positionBase=null;
      if(this.header){Object.assign(this.header,this.mediaInfo());this.forward("H",this.header);}
      return;
    }
    if(kind==="D"){
      const reply=JSON.parse(body),pending=this.httpControls.get(reply.id);
      if(!reply.error&&this.receiverHealth){
        reply.effectPendingSeconds=Math.max(0,this.baseTime/48000-this.receiverHealth.time)+1;
        body=Buffer.from(JSON.stringify(reply));
      }
      if(pending){clearTimeout(pending.timer);this.httpControls.delete(reply.id);pending.resolve(reply);}
    }
    if(kind==="T"){this.forward("T",{clipped:this.clipped});return;}
    if(kind==="S"){
      const state=JSON.parse(body);
      if(!state.playing||state.paused||state.loading)this.lastAudioAt=0;
      if(!this.sync&&this.latestState&&state.currentId!==this.latestState.currentId&&this.segments.length<3)this.positionBase=0;
      this.latestState=state;
    }
    this.forward(kind,body);
  }
  mediaInfo(){return {stream:`/hls/${this.id}/e${this.epoch}/index.m3u8`,mediaEpoch:this.epoch,mediaReady:this.segments.length>=3||!!this.programEnded&&this.segments.length>0,positionBase:this.positionBase??0,synchronized:!!this.sync};}
  sealIfComplete(){
    if(this.programEnded||!Number.isSafeInteger(this.endSample)||this.positionBase===null||this.frameIndex*480<(this.endSample-Math.round(this.positionBase*48000)))return;
    this.flush();this.programEnded=true;
    if(this.header){Object.assign(this.header,this.mediaInfo(),{mediaReady:true});this.forward('H',this.header);}
  }
  receiverFeedback(body){
    this.touch();
    const now=Date.now();
    const value=JSON.parse(body),r=value.receiver;
    if(value.resync===true)this.sync?.hold('browser-play-request');
    if(Number.isFinite(value.clock))this.forward('Y',{action:'clock',echo:value.clock,hostTime:now});
    if(!r||r.epoch!==this.epoch||![r.aheadMs,r.time,r.readyState].every(Number.isFinite))return;
    if(r.aheadMs<0||r.aheadMs>120000||r.time<0||r.readyState<0||r.readyState>4)return;
    this.sync?.feedback(this,r);
    if(now-this.lastDiagnosticAt<1000)return;
    const stalled=r.waiting===true&&!r.paused&&!r.pending;
    if(!stalled&&now-this.lastDiagnosticAt<5000)return;
    this.lastDiagnosticAt=now;
    this.receiverHealth={at:now,epoch:this.epoch,aheadMs:Math.round(r.aheadMs),time:r.time,readyState:r.readyState,
      syncPhase:typeof r.syncPhase==='string'?r.syncPhase.slice(0,16):undefined,
      ranges:r.ranges?{buffered:Array.isArray(r.ranges.buffered)?r.ranges.buffered.slice(0,8).filter(v=>Array.isArray(v)&&v.length===2&&v.every(Number.isFinite)):[],seekable:Array.isArray(r.ranges.seekable)?r.ranges.seekable.slice(0,8).filter(v=>Array.isArray(v)&&v.length===2&&v.every(Number.isFinite)):[],seeking:r.ranges.seeking===true}:undefined,
      startDelayMs:Number.isFinite(r.startDelayMs)?Math.round(r.startDelayMs):undefined,
      timerLateMs:Number.isFinite(r.timerLateMs)?Math.round(r.timerLateMs):undefined,
      syncPosition:Number.isFinite(r.syncPosition)?r.syncPosition:undefined,
      mediaClockBase:Number.isFinite(r.mediaClockBase)?r.mediaClockBase:undefined,
      clockSkewMs:Number.isFinite(r.hostTime)&&this.sync&&Math.abs(now-r.hostTime)<2000?Math.round(1000*((this.positionBase??0)+r.time+(now-r.hostTime)/1000-this.sync.hooks.position())):undefined,
      waiting:r.waiting===true,paused:r.paused===true,pending:r.pending===true,hidden:r.hidden===true,
      hostPlaying:!!this.latestState?.playing&&!this.latestState?.paused,hostLoading:!!this.latestState?.loading,
      audioAgeMs:this.lastAudioAt?now-this.lastAudioAt:null,maxAudioGapMs:this.maxAudioGapMs,
      producedSeconds:this.baseTime/48000,segments:this.segments.length,lowLatency:!!this.lowLatency,renderBufferMs:this.sync?.bufferMs?.()};
    this.diagnostic?.(this.receiverHealth);this.maxAudioGapMs=0;
  }
  forward(kind,value){if(this.control&&!this.control.destroyed&&this.control.writableLength<262144)this.control.write(this.packet(kind,value));}
  flushPart(){
    const start=this.parts.filter(p=>p.sequence===this.sequence).reduce((n,p)=>n+p.frames,0);
    const frames=this.frames.slice(start);if(!frames.length)return;
    const index=this.parts.filter(p=>p.sequence===this.sequence).length;
    this.parts.push({sequence:this.sequence,index,frames:frames.length,duration:frames.length*.01,bytes:mediaSegment(frames,this.fragmentSequence++,this.baseTime+start*480)});
  }
  flush(){
    if(!this.frames.length)return;
    this.flushPart();
    if(this.pendingDiscontinuity){this.discontinuity++;this.pendingDiscontinuity=false;}
    const duration=this.frames.length*.01;
    this.segments.push({sequence:this.sequence,discontinuity:this.discontinuity,duration,bytes:Buffer.concat(this.parts.filter(p=>p.sequence===this.sequence).map(p=>p.bytes))});
    this.baseTime+=this.frames.length*480;this.sequence++;this.frames=[];
    while(this.segments.length>16)this.segments.shift();
    this.parts=this.parts.filter(p=>p.sequence>=(this.segments[0]?.sequence??this.sequence));
    if(this.header&&this.segments.length===3){Object.assign(this.header,this.mediaInfo());this.forward("H",this.header);}
  }
  playlist(){
    if((this.segments.length<3&&!this.programEnded)||!this.segments.length)return null;
    const segments=this.segments.slice(-10),first=segments[0];
    const lines=["#EXTM3U","#EXT-X-VERSION:9","#EXT-X-TARGETDURATION:1",'#EXT-X-PART-INF:PART-TARGET=0.200','#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=0.600',`#EXT-X-MEDIA-SEQUENCE:${first.sequence}`,`#EXT-X-DISCONTINUITY-SEQUENCE:${first.discontinuity}`,'#EXT-X-MAP:URI="init.mp4"'];
    // Safari otherwise chooses a live-edge starting segment, which can leave
    // sample zero outside its buffered ranges while the native DAC waits at zero.
    if(this.sync)lines.push('#EXT-X-START:TIME-OFFSET=0,PRECISE=YES');
    let previous=first.discontinuity;
    const partLines=sequence=>this.parts.filter(p=>p.sequence===sequence).map(p=>`#EXT-X-PART:DURATION=${p.duration.toFixed(3)},URI="${p.sequence}.${p.index}.m4s",INDEPENDENT=YES`);
    for(const item of segments){if(item.discontinuity!==previous)lines.push("#EXT-X-DISCONTINUITY");previous=item.discontinuity;if(item.sequence>=this.sequence-3)lines.push(...partLines(item.sequence));lines.push(`#EXTINF:${item.duration.toFixed(3)},`,`${item.sequence}.m4s`);}
    if(!this.programEnded){lines.push(...partLines(this.sequence));lines.push(`#EXT-X-PRELOAD-HINT:TYPE=PART,URI="${this.sequence}.${this.parts.filter(p=>p.sequence===this.sequence).length}.m4s"`);}
    if(this.programEnded)lines.push('#EXT-X-ENDLIST');
    return Buffer.from(lines.join("\n")+"\n");
  }
  requestControl(body){
    const request=JSON.parse(body);
    if(typeof request.id!=="string"||request.id.length>64||this.httpControls.has(request.id)||this.httpControls.size>=16)throw Error("控制请求无效或过多");
    return new Promise(resolve=>{
      const timer=setTimeout(()=>{this.httpControls.delete(request.id);resolve({id:request.id,error:"主机未确认操作"});},request.command?.action==="roomGenerate"?611000:16000).unref();
      this.httpControls.set(request.id,{resolve,timer});this.push(this.packet("C",body));
    });
  }
  attachControl(stream,decodePackets){
    if(this.control&&!this.control.destroyed)throw Error("此收听会话已有控制页面");this.control=stream;
    const parse=decodePackets((kind,body)=>{if(kind==="C")this.push(this.packet(kind,body));else if(kind==="Q")this.destroy();else if(kind==="K")this.receiverFeedback(body);else if(kind!=="H")throw Error("无效 HLS 控制消息");});
    stream.on("data",data=>{try{parse(data);}catch{stream.destroy();}});
    stream.once("close",()=>{if(this.control===stream){this.control=null;if(!this.destroyed)this.sync?.hold('control-disconnected');}});
    if(this.header){Object.assign(this.header,this.mediaInfo());this.forward("H",this.header);}if(this.latestState)this.forward("S",this.latestState);
  }
}
module.exports={HlsPeer};
