"use strict";
const {Duplex}=require("node:stream");
const crypto=require("node:crypto");
const {encodeFrame,initSegment,mediaSegment}=require("./remote-hls-codec.cjs");
class HlsPeer extends Duplex {
  constructor({id,address,packet,decodePackets,onClose}){
    super();this.id=id;this.cookie=crypto.randomBytes(32).toString("hex");this.remoteAddress=address;this.packet=packet;
    this.init=initSegment();this.segments=[];this.frames=[];this.frameIndex=0;this.sequence=0;this.baseTime=0;this.discontinuity=0;this.pendingDiscontinuity=false;
    this.encoded=0;this.released=0;this.started=Date.now();this.activity=Date.now();this.clipped=0;this.control=null;this.header=null;this.httpControls=new Map();
    this.decode=decodePackets((kind,body)=>this.receive(kind,body));
    this.timer=setInterval(()=>{
      if(Date.now()-this.activity>90000){this.sdaFailure="HLS 播放器已超过 90 秒未请求音频";this.destroy();return;}
      // Bounded real-time producer. HTTP media requests, not page JS, hold the lease.
      const allowed=Math.floor(((Date.now()-this.started)/1000+4)*48000/480)*480;
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
      this.header={...JSON.parse(body),sampleFormat:"hls-flac24",stream:`/hls/${this.id}/index.m3u8`};this.push(this.packet("H",{protocol:1}));this.forward("H",this.header);return;
    }
    if(kind==="A"){
      const frame=encodeFrame(body,this.frameIndex++);this.clipped+=frame.clipped;this.frames.push(frame.data);this.encoded+=480;if(this.frames.length>=100)this.flush();return;
    }
    if(kind==="R"){this.flush();this.pendingDiscontinuity=true;return;}
    if(kind==="D"){
      const reply=JSON.parse(body),pending=this.httpControls.get(reply.id);
      if(pending){clearTimeout(pending.timer);this.httpControls.delete(reply.id);pending.resolve(reply);}
    }
    if(kind==="T"){this.forward("T",{clipped:this.clipped});return;}
    if(kind==="S")this.latestState=JSON.parse(body);
    this.forward(kind,body);
  }
  forward(kind,value){if(this.control&&!this.control.destroyed&&this.control.writableLength<262144)this.control.write(this.packet(kind,value));}
  flush(){
    if(!this.frames.length)return;
    if(this.pendingDiscontinuity){this.discontinuity++;this.pendingDiscontinuity=false;}
    const duration=this.frames.length*.01;
    this.segments.push({sequence:this.sequence,discontinuity:this.discontinuity,duration,bytes:mediaSegment(this.frames,this.sequence,this.baseTime)});
    this.baseTime+=this.frames.length*480;this.sequence++;this.frames=[];
    while(this.segments.length>16)this.segments.shift();
  }
  playlist(){
    if(this.segments.length<3)return null;
    const segments=this.segments.slice(-10),first=segments[0];
    const lines=["#EXTM3U","#EXT-X-VERSION:7","#EXT-X-TARGETDURATION:1",`#EXT-X-MEDIA-SEQUENCE:${first.sequence}`,`#EXT-X-DISCONTINUITY-SEQUENCE:${first.discontinuity}`,'#EXT-X-MAP:URI="init.mp4"'];
    let previous=first.discontinuity;
    for(const item of segments){if(item.discontinuity!==previous)lines.push("#EXT-X-DISCONTINUITY");previous=item.discontinuity;lines.push(`#EXTINF:${item.duration.toFixed(3)},`,`${item.sequence}.m4s`);}
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
    const parse=decodePackets((kind,body)=>{if(kind==="C")this.push(this.packet(kind,body));else if(kind==="Q")this.destroy();else if(kind==="K")this.touch();else if(kind!=="H")throw Error("无效 HLS 控制消息");});
    stream.on("data",data=>{try{parse(data);}catch{stream.destroy();}});
    stream.once("close",()=>{if(this.control===stream)this.control=null;});
    if(this.header)this.forward("H",this.header);if(this.latestState)this.forward("S",this.latestState);
  }
}
module.exports={HlsPeer};
