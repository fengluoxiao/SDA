const test = require("node:test");
const assert = require("node:assert/strict");
const https = require("node:https");
const net = require("node:net");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {WebSocket} = require("ws");
const {RemoteSession, packet, decodePackets} = require("../remote-session.cjs");
const {loadRemoteCertificate} = require("../remote-certificate.cjs");
const delay = ms => new Promise(r => setTimeout(r, ms));
async function until(fn) { const end=Date.now()+5000; while(!fn()){if(Date.now()>end)throw Error("timeout");await delay(10);} }
async function setup(earlyReset=false) {
  let local; const sent = [], commands = [];
  const host = new RemoteSession({status:()=>{}, disconnected:()=>{}, control:command=>{commands.push(command);host.completeControl(command.id);}, route:async value=>{
    local?.destroy(); if(!value.address)return true;
    const [address,port]=value.address.split(":");local=net.connect(+port,address);local.on("error",()=>{});
    await new Promise(r=>local.once("connect",r));local.write(value.token);if(earlyReset)local.write(packet("R"));
    local.on("data",data=>{for(const credit of data){assert.equal(credit,80);const pcm=Buffer.alloc(3840);for(let i=0;i<960;i++)pcm.writeFloatLE(Math.sin((sent.length*960+i)*0.13)*0.01,i*4);sent.push(pcm);local.write(packet("A",pcm));}});return true;
  }});
  const listener=net.createServer();await new Promise(r=>listener.listen(0,"127.0.0.1",r));const port=listener.address().port;await new Promise(r=>listener.close(r));
  await host.host({port,bufferMs:100});
  return {host,port,sent,commands,close:async()=>{await host.stop();local?.destroy();}};
}
function request(port,route="/") {
  return new Promise((resolve,reject)=>{const req=https.get({hostname:"127.0.0.1",port,path:route,rejectUnauthorized:false,ALPNProtocols:["http/1.1"]},res=>{const chunks=[];res.on("data",c=>chunks.push(c));res.on("end",()=>resolve({status:res.statusCode,headers:res.headers,body:Buffer.concat(chunks).toString()}));});req.on("error",reject);});
}
function socket(port,origin=`https://127.0.0.1:${port}`) {return new WebSocket(`wss://127.0.0.1:${port}/stream`,{rejectUnauthorized:false,ALPNProtocols:["http/1.1"],origin});}
test("browser FIFO preserves float bits through wraparound and rebuffering",async()=>{
  const {StereoPcmBuffer}=await import("../remote-web/pcm-buffer.mjs");const fifo=new StereoPcmBuffer(4096,480);
  const expected=[],actual=[];let total=0;
  for(let block=0;block<100;block++){
    const input=Float32Array.from({length:960},(_,i)=>i===0?-0:Math.sin((block*960+i)*0.08));
    expected.push(...input);fifo.push(input);total+=480;
    while(fifo.queued>=128){const left=new Float32Array(128),right=new Float32Array(128);const count=fifo.fill(left,right);if(!count)break;for(let i=0;i<count;i++)actual.push(left[i],right[i]);}
  }
  const left=new Float32Array(fifo.queued),right=new Float32Array(fifo.queued);const count=fifo.fill(left,right);for(let i=0;i<count;i++)actual.push(left[i],right[i]);
  assert.deepEqual(Buffer.from(new Float32Array(actual).buffer),Buffer.from(new Float32Array(expected).buffer));assert.equal(fifo.consumed,total);
  const silent=new Float32Array(128);assert.equal(fifo.fill(silent,new Float32Array(128)),0);
  const next=Float32Array.from({length:960},(_,i)=>i/4096);fifo.push(next);const l=new Float32Array(480),r=new Float32Array(480);assert.equal(fifo.fill(l,r),480);
  for(let i=0;i<480;i++){assert.equal(l[i],next[i*2]);assert.equal(r[i],next[i*2+1]);}
});
test("browser reset releases credits, removes old song, and malformed PCM cannot grow the FIFO",async()=>{
  const {StereoPcmBuffer}=await import("../remote-web/pcm-buffer.mjs");const fifo=new StereoPcmBuffer(1024,480);
  fifo.push(new Float32Array(960).fill(0.1));fifo.reset();assert.equal(fifo.consumed,480);assert.equal(fifo.queued,0);
  const a=new Float32Array(128);fifo.fill(a,new Float32Array(128));assert.ok(a.every(v=>v===0));
  assert.throws(()=>fifo.push(new Float32Array(2050)),/overflow/);assert.throws(()=>fifo.push(new Float32Array([NaN,0])),/Invalid/);assert.equal(fifo.queued,0);
});
test("HTTPS serves only public web assets and no host secrets or arbitrary paths",async()=>{
  const f=await setup();try{
    const page=await request(f.port);assert.equal(page.status,200);assert.match(page.body,/连接并收听/);assert.ok(!page.body.includes(f.host.key.toString("hex")));
    assert.match(page.headers["content-security-policy"],/frame-ancestors 'none'/);assert.equal(page.headers["referrer-policy"],"no-referrer");
    for(const route of ["/app.mjs","/app.css","/pcm-worklet.mjs","/pcm-buffer.mjs"])assert.equal((await request(f.port,route)).status,200);
    for(const route of ["/remote-session.cjs","/../main.cjs","/%2e%2e/main.cjs","/?key=anything"])assert.equal((await request(f.port,route)).status,404);
    assert.equal(f.host.peer,null);
  }finally{await f.close();}
});
test("WebSocket PCM is byte exact and shares the single slot with native clients",async()=>{
  const f=await setup();const ws=socket(f.port);ws.on("error",()=>{});const audio=[],responses=[];
  try{
    const decode=decodePackets((kind,body)=>{if(kind==="H")ws.send(packet("H",{protocol:1}));else if(kind==="A")audio.push(Buffer.from(body));else if(kind==="D")responses.push(JSON.parse(body));});
    ws.on("message",data=>decode(data));await new Promise(r=>ws.once("open",r));ws.send(JSON.stringify({protocol:1,token:f.host.key.toString("hex")}));
    await until(()=>audio.length===10);assert.deepEqual(audio,f.sent);await delay(80);assert.equal(audio.length,10,"no consumption credit means no more samples");
    ws.send(packet("C",{id:"next",command:{action:"next"}}));await until(()=>responses.length===1);assert.equal(f.commands[0].action,"next");assert.equal(responses[0].error,null);
    const second=socket(f.port);second.on("error",()=>{});await new Promise(r=>second.once("open",r));second.send(JSON.stringify({protocol:1,token:f.host.key.toString("hex")}));const code=await new Promise(r=>second.once("close",r));assert.equal(code,1008);
    const native=new RemoteSession({status:()=>{},prepareClient:()=>{throw Error("must not start receiver");}});await assert.rejects(native.join({invite:f.host.invite("127.0.0.1")}),/已有客户端/);await native.stop();
    ws.close();await until(()=>f.host.peer===null);
  }finally{ws.terminate();await f.close();}
});
test("WebSocket rejects wrong keys and foreign origins without acquiring the audio slot",async()=>{
  const f=await setup();try{
    const bad=socket(f.port);bad.on("error",()=>{});await new Promise(r=>bad.once("open",r));bad.send(JSON.stringify({protocol:1,token:"0".repeat(64)}));assert.equal(await new Promise(r=>bad.once("close",r)),1008);assert.equal(f.host.peer,null);
    const foreign=socket(f.port,"https://foreign.invalid");const error=await new Promise(r=>foreign.once("error",r));assert.match(error.message,/403/);assert.equal(f.host.peer,null);
  }finally{await f.close();}
});
test("browser certificate stays stable across sessions",()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),"sda-cert-test-"));try{const file=path.join(dir,"certificate.json");const first=loadRemoteCertificate(file),second=loadRemoteCertificate(file);assert.equal(first.fingerprint,second.fingerprint);assert.equal(first.key,second.key);}finally{fs.rmSync(dir,{recursive:true,force:true});}
});
function hlsHttp(port,route,method="GET",cookie="",body,origin=true,headers={}) {
  return new Promise((resolve,reject)=>{
    const req=https.request({hostname:"127.0.0.1",port,path:route,method,agent:false,rejectUnauthorized:false,ALPNProtocols:["http/1.1"],headers:{...(origin?{Origin:`https://127.0.0.1:${port}`} : {}),...(cookie?{Cookie:cookie}:{}),...headers}},res=>{const chunks=[];res.on("data",c=>chunks.push(c));res.on("end",()=>resolve({status:res.statusCode,headers:res.headers,body:Buffer.concat(chunks)}));});req.on("error",reject);req.end(body===undefined?undefined:JSON.stringify(body));
  });
}
test("HLS authenticates media, survives control loss, reconnects and releases its exclusive slot",async()=>{
  const f=await setup();let control;
  try{
    assert.equal((await hlsHttp(f.port,"/hls/session","POST","",{token:"0".repeat(64)})).status,403);
    const token=f.host.key.toString("hex");
    assert.equal((await hlsHttp(f.port,"/hls/session","POST","",{token},false)).status,403);
    const created=await hlsHttp(f.port,"/hls/session","POST","",{token});assert.equal(created.status,200);
    const info=JSON.parse(created.body),cookie=created.headers["set-cookie"][0].split(";")[0];
    const peer=f.host.peer;assert.equal(peer.header.sampleFormat,"hls-flac24");
    assert.equal((await hlsHttp(f.port,info.stream)).status,403);
    const playlist=await hlsHttp(f.port,info.stream,"GET",cookie);assert.equal(playlist.status,200);assert.match(playlist.body.toString(),/#EXT-X-MAP/);
    const init=await hlsHttp(f.port,`/hls/${info.id}/init.mp4`,"GET",cookie);assert.ok(init.body.includes(Buffer.from("fLaC")));
    const partial=await hlsHttp(f.port,`/hls/${info.id}/init.mp4`,"GET",cookie,undefined,true,{Range:"bytes=0-15"});assert.equal(partial.status,206);assert.deepEqual(partial.body,init.body.subarray(0,16));
    assert.equal((await hlsHttp(f.port,"/hls/session","POST","",{token})).status,409);
    const native=new RemoteSession({status:()=>{}});await assert.rejects(native.join({invite:f.host.invite("127.0.0.1")}),/已有客户端/);await native.stop();
    for(let attempt=0;attempt<2;attempt++){
      const messages=[];
      control=new WebSocket(`wss://127.0.0.1:${f.port}/stream`,{rejectUnauthorized:false,ALPNProtocols:["http/1.1"],origin:`https://127.0.0.1:${f.port}`,headers:{Cookie:cookie}});control.on("error",()=>{});
      control.on("message",decodePackets((kind,body)=>messages.push([kind,body])));
      await new Promise(r=>control.once("open",r));control.send(JSON.stringify({protocol:1,token,hls:info.id}));await until(()=>messages.some(v=>v[0]==="H"));
      control.send(packet("C",{id:`command${attempt}`,command:{action:"next"}}));await until(()=>messages.some(v=>v[0]==="D"));
      control.terminate();await until(()=>peer.control===null);assert.equal(f.host.peer,peer);
    }
    const before=peer.sequence;await until(()=>peer.sequence>before);assert.equal(f.host.peer,peer,"native media survives without a websocket");
    assert.equal((await hlsHttp(f.port,`/hls/${info.id}/control`,"POST",cookie,{id:"http",command:{action:"pause"}})).status,200);await until(()=>f.commands.some(c=>c.action==="pause"));
    assert.ok(peer.segments.length<=16);
    peer.write(packet("R",Buffer.alloc(0)));
    assert.equal((await hlsHttp(f.port,info.stream,"GET",cookie)).status,410);
    assert.equal((await hlsHttp(f.port,info.stream.replace('index.m3u8','0.m4s'),"GET",cookie)).status,410);
    await until(()=>peer.segments.length>=3);
    assert.equal((await hlsHttp(f.port,peer.mediaInfo().stream,"GET",cookie)).status,200);
    assert.equal((await hlsHttp(f.port,`/hls/${info.id}/stop`,"POST",cookie)).status,204);await until(()=>f.host.peer===null);
    assert.equal((await hlsHttp(f.port,info.stream,"GET",cookie)).status,403);
  }finally{control?.terminate();await f.close();}
});
test("HLS resets discard old media and use a fresh URL epoch; expired leases close",async()=>{
  const {HlsPeer}=require("../remote-hls.cjs");const peer=new HlsPeer({id:"a".repeat(32),address:"test",packet,decodePackets,onClose:()=>{}});peer.on("data",()=>{});peer.on("error",()=>{});
  try{
    peer.write(packet("H",{protocol:1,sampleRate:48000,channels:2}));
    const pcm=packet("A",Buffer.alloc(3840));for(let i=0;i<1700;i++)peer.write(pcm);
    assert.equal(peer.segments.length,16);const previous=peer.mediaInfo().stream;
    peer.write(packet("R",Buffer.alloc(0)));assert.equal(peer.segments.length,0);assert.equal(peer.frames.length,0);assert.equal(peer.playlist(),null);
    assert.notEqual(peer.mediaInfo().stream,previous);assert.equal(peer.header.mediaReady,false);
    for(let i=0;i<300;i++)peer.write(pcm);
    assert.equal(peer.segments.length,3);assert.equal(peer.header.mediaReady,true);assert.match(peer.playlist().toString(),/#EXT-X-MEDIA-SEQUENCE:0/);
    peer.activity=Date.now()-91000;await until(()=>peer.destroyed);assert.equal(peer.segments.length,0);
  }finally{peer.destroy();}
});
test('remote cover state carries only an ID and rejects URLs, SVG and oversized data',()=>{
 const host=new RemoteSession({status:()=>{}});const base={title:'Song',playlist:[]};
 host.publishState({...base,artwork:'data:image/jpeg;base64,/9j/2Q=='});const id=host.state.artworkId;assert.match(id,/^[a-f0-9]{64}$/);assert.ok(!JSON.stringify(host.state).includes('base64'));
 host.publishState({...base,artwork:'data:image/jpeg;base64,/9j/2Q=='});assert.equal(host.state.artworkId,id);
 for(const artwork of ['https://example.com/cover.jpg','file:///private.jpg','data:image/svg+xml;base64,AAAA','data:image/jpeg;base64,'+'A'.repeat(180000),'']){host.publishState({...base,artwork});assert.equal(host.state.artworkId,'');}
});
test('HLS receiver diagnostics validate epochs, bound input and keep credits untouched',()=>{
 const {HlsPeer}=require('../remote-hls.cjs');const reports=[];
 const peer=new HlsPeer({id:'b'.repeat(32),address:'test',packet,decodePackets,onClose:()=>{},diagnostic:r=>reports.push(r)});
 peer.on('data',()=>{});
 try{
  peer.latestState={playing:true};peer.lastAudioAt=Date.now()-240;peer.maxAudioGapMs=180;
  const receiver={epoch:0,aheadMs:20,time:12,readyState:2,waiting:true,paused:false};
  peer.receiverFeedback(Buffer.from(JSON.stringify({receiver:{...receiver,epoch:1}})));assert.equal(reports.length,0);
  peer.receiverFeedback(Buffer.from(JSON.stringify({receiver:{...receiver,aheadMs:-1}})));assert.equal(reports.length,0);
  peer.receiverFeedback(Buffer.from(JSON.stringify({receiver})));assert.equal(reports.length,1);
  assert.equal(reports[0].aheadMs,20);assert.ok(reports[0].audioAgeMs>=240);assert.equal(reports[0].maxAudioGapMs,180);
  assert.equal(peer.released,0);assert.equal(peer.encoded,0);
  peer.receiverFeedback(Buffer.from(JSON.stringify({receiver})));assert.equal(reports.length,1);
 }finally{peer.destroy();}
});
test('HLS reopening replaces only the same browser session and rehosting accepts the existing link',async()=>{
 const f=await setup();try{
 const token=f.host.key.toString('hex');let first=await hlsHttp(f.port,'/hls/session','POST','',{token});assert.equal(first.status,200);const old=JSON.parse(first.body),cookie=first.headers['set-cookie'][0].split(';')[0];
 const next=await hlsHttp(f.port,'/hls/session','POST',cookie,{token});assert.equal(next.status,200);const info=JSON.parse(next.body);assert.notEqual(info.id,old.id);const newCookie=next.headers['set-cookie'][0].split(';')[0];
 assert.equal((await hlsHttp(f.port,`/hls/${old.id}/stop`,'POST',cookie)).status,403);assert.equal((await hlsHttp(f.port,info.stream,'GET',newCookie)).status,200);
 await f.host.stop();await f.host.host({port:f.port,bufferMs:100});assert.equal(f.host.key.toString('hex'),token);
 const reopened=await hlsHttp(f.port,'/hls/session','POST','',{token});assert.equal(reopened.status,200);
 }finally{await f.close();}
});

test('initial native reset waits until the browser acknowledges readiness',async()=>{
 const f=await setup(true);const ws=socket(f.port);ws.on('error',()=>{});const messages=[];
 try{ws.on('message',decodePackets((kind)=>messages.push(kind)));await new Promise(r=>ws.once('open',r));ws.send(JSON.stringify({protocol:1,token:f.host.key.toString('hex')}));await until(()=>messages.includes('H'));await delay(100);assert.ok(!messages.includes('R'));ws.send(packet('H',{protocol:1}));await until(()=>messages.includes('A'));assert.ok(messages.indexOf('R')<messages.indexOf('A'));}finally{ws.terminate();await f.close();}
});

test('low-latency parts preserve full-segment bytes and reset atomically',()=>{
 const {HlsPeer}=require('../remote-hls.cjs');const p=new HlsPeer({id:'c'.repeat(32),packet,decodePackets,onClose(){}});p.on('data',()=>{});
 try{
  for(let i=0;i<320;i++)p.receive('A',Buffer.alloc(3840));
  assert.equal(p.parts.filter(v=>v.sequence===3).length,1);
  assert.deepEqual(p.segments[0].bytes,Buffer.concat(p.parts.filter(v=>v.sequence===0).map(v=>v.bytes)));
  assert.match(p.playlist().toString(),/#EXT-X-PART-INF:PART-TARGET=0.200/);
  assert.match(p.playlist().toString(),/URI="3.0.m4s"/);
  assert.match(p.playlist().toString(),/#EXT-X-PRELOAD-HINT:TYPE=PART,URI="3.1.m4s"/);
  assert.equal(p.parts[1].bytes.readBigUInt64BE(p.parts[1].bytes.indexOf('tfdt')+8),9600n);
  p.receive('R',Buffer.alloc(0));assert.equal(p.parts.length,0);assert.equal(p.playlist(),null);
 }finally{p.destroy();}
});

test('low-latency media queries remain authenticated and bounded',async()=>{
 const f=await setup();try{
  const result=await hlsHttp(f.port,'/hls/session','POST','',{token:f.host.key.toString('hex')});
  const info=JSON.parse(result.body),cookie=result.headers['set-cookie'][0].split(';')[0];
  const part=info.stream.replace('index.m3u8','0.0.m4s');
  assert.equal((await hlsHttp(f.port,part,'GET','')).status,403);
  assert.equal((await hlsHttp(f.port,part,'GET',cookie)).status,200);
  assert.equal((await hlsHttp(f.port,info.stream+'?_HLS_msn=0&_HLS_part=0','GET',cookie)).status,200);
  assert.equal((await hlsHttp(f.port,info.stream+'?_HLS_msn=99999999','GET',cookie)).status,400);
 }finally{await f.close();}
});
