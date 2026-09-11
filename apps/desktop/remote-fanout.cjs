"use strict";
const net=require('node:net'),crypto=require('node:crypto');
const FRAMES=480,MAX_BACKLOG=600;
// One native producer, independent flow control and bounded queues per receiver.
class RemoteFanout {
 constructor(owner,{packet,decodePackets}){this.owner=owner;this.packet=packet;this.decodePackets=decodePackets;this.peers=new Map();this.inflight=0;this.starting=null;this.local=null;this.server=null;this.closed=false;this.history=[];this.sourceSample=null;this.sourceEnd=null;}
 add(socket){if(this.sourceEnd!==null)socket.endSample=this.sourceEnd;const position=this.owner.hooks.position?.();const queue=socket.sync&&Number.isFinite(position)?this.history.filter(f=>f.sample>=Math.max(0,position-1)*48000):[];const state={socket,ready:false,sent:0,consumed:0,queue:[...queue],lastFeedback:Date.now(),reset:false};this.peers.set(socket,state);return state;}
 flush(p){const window=this.owner.bufferMs*48;while(p.ready&&p.queue.length&&p.sent-p.consumed<window&&!p.socket.destroyed){const frame=p.queue.shift();if(p.socket.frameIndex===0&&Number.isSafeInteger(frame.sample))p.socket.positionBase=frame.sample/48000;p.sent+=FRAMES;p.socket.write(this.packet('A',frame.body??frame));}if(p.queue.length>(p.socket.sync?1000:MAX_BACKLOG)||p.socket.writableLength>1024*1024)this.owner.failPeer(p.socket,'此设备网络未跟上播放，请重新连接');}
 pump(){if(this.closed||!this.local)return;let demand=0;for(const p of this.peers.values()){this.flush(p);if(p.ready&&!p.socket.destroyed)demand=Math.max(demand,Math.floor((this.owner.bufferMs*48-(p.sent-p.consumed)-p.queue.length*FRAMES)/FRAMES));}const count=Math.max(0,demand-this.inflight);if(count){this.inflight+=count;this.local.write(Buffer.alloc(count,'P'));}}
 async start(){if(this.starting)return this.starting;this.starting=this.open();return this.starting;}
 async open(){
  const token=crypto.randomBytes(32).toString('hex'),owner=this.owner;
  this.server=net.createServer(local=>{
   owner.track(local);local.setTimeout(5000,()=>local.destroy());let handshake=Buffer.alloc(0),authenticated=false;
   const audio=this.decodePackets((kind,body)=>{
    if(this.closed)return;
    if(kind==='R'&&body.length===0){this.history=[];this.sourceSample=null;this.sourceEnd=null;for(const p of this.peers.values()){p.queue=[];if(p.ready)p.socket.write(this.packet('R'));else p.reset=true;}return;}
    if(kind==='M'&&body.length===8){const sample=Number(body.readBigUInt64LE());if(!Number.isSafeInteger(sample))throw Error('无效音频时钟');this.sourceSample=sample;return;}
    if(kind==='N'&&body.length===8){const sample=Number(body.readBigUInt64LE());if(!Number.isSafeInteger(sample))throw Error('Invalid end sample');this.sourceEnd=sample;for(const p of this.peers.values()){p.socket.endSample=sample;p.socket.sealIfComplete?.();}return;}
    if(kind!=='A'||body.length!==3840)throw Error('无效原生音频包');
    this.inflight=Math.max(0,this.inflight-1);owner.bytes+=body.length;
    const frame={body,sample:this.sourceSample};if(this.sourceSample!==null){this.sourceSample+=FRAMES;this.history.push(frame);while(this.history.length>1000)this.history.shift();}
    for(const p of this.peers.values()){if(p.ready&&!p.socket.destroyed){p.queue.push(frame);this.flush(p);}}
    this.pump();
   });
   local.on('data',chunk=>{try{
    if(!authenticated){handshake=Buffer.concat([handshake,chunk]);if(handshake.length<64)return;if(this.local||!crypto.timingSafeEqual(handshake.subarray(0,64),Buffer.from(token)))throw Error('原生音频认证失败');authenticated=true;this.local=local;owner.local=local;local.setTimeout(0);chunk=handshake.subarray(64);handshake=Buffer.alloc(0);this.pump();}audio(chunk);
   }catch(e){local.destroy();if(authenticated)this.fail(e.message);}});
   local.once('close',()=>{if(this.local===local&&!this.closed)this.fail('主机原生音频连接中断');});
  });
  const address=await new Promise((resolve,reject)=>{this.server.once('error',reject);this.server.listen(0,'127.0.0.1',()=>resolve(this.server.address()));});
  if(this.closed)return;
  owner.localServer=this.server;
  if(!await owner.hooks.route({address:`127.0.0.1:${address.port}`,token}))throw Error('主机未能开启远程音频');
 }
 fail(message){for(const p of this.peers.values())this.owner.failPeer(p.socket,message);}
 close(){this.closed=true;this.local?.destroy();this.server?.close();this.peers.clear();}
}
module.exports={RemoteFanout};
