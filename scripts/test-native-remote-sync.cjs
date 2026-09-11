// Real WASAPI consumer + real Chromium HLS consumer. Run after native release build.
const fs=require('node:fs'),{spawn}=require('node:child_process'),net=require('node:net'),path=require('node:path'),assert=require('node:assert/strict');
const {RemoteSession}=require('../apps/desktop/remote-session.cjs');
const {chromium}=require(process.env.SDA_PLAYWRIGHT_MODULE||'playwright');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,ms=20000){const end=Date.now()+ms;while(!await fn()){if(Date.now()>end)throw Error('condition timed out');await sleep(25);}}
(async()=>{
 const child=spawn(path.resolve('apps/native-renderer/target/release/sda-native-renderer.exe'),[],{windowsHide:true,env:{...process.env,SDA_HRTF_ROOT:path.resolve('apps/web/public'),SDA_OUTPUT_SETTINGS:JSON.stringify({deviceId:null,exclusive:false})},stdio:['pipe','pipe','pipe']});
 let ready=false,health={},text='',queue=Promise.resolve(),host,browser,timer,page;
 const pending=new Map(),events=[],skews=[];
 child.stdout.setEncoding('utf8');child.stdout.on('data',s=>{text+=s;let i;while((i=text.indexOf('\n'))>=0){const line=text.slice(0,i);text=text.slice(i+1);try{const m=JSON.parse(line);if(m.type==='ready')ready=true;if(m.type==='health')health=m;if(m.type==='ack'||m.type==='batchAck'){const key=m.type==='batchAck'?'batch:'+m.start:m.command;pending.get(key)?.(m);pending.delete(key);}if(m.type==='error')events.push({error:m.detail});}catch{}}});child.stderr.on('data',()=>{});child.stdin.on('error',()=>{});
 function send(value){const b=Buffer.from(JSON.stringify(value)),h=Buffer.alloc(5);h[0]=74;h.writeUInt32LE(b.length,1);child.stdin.write(Buffer.concat([h,b]));}
 function command(value){const next=queue.then(()=>new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(Error('command timeout: '+value.type)),10000);pending.set(value.type,m=>{clearTimeout(timeout);m.accepted?resolve(true):reject(Error(m.detail??value.type));});send(value);}));queue=next.catch(()=>{});return next;}
 const adm=process.env.SDA_SYNC_ADM_FILE?JSON.parse(fs.readFileSync(process.env.SDA_SYNC_ADM_FILE,'utf8')):null;const lateJoin=process.env.SDA_SYNC_JOIN_DURING_PLAY==='1';const count=456336;let blockMedia=true;const blockedRequests=[];
 let playing=false,paused=false;
 const state=()=>({playing,paused,loading:playing&&!(health.samplePos>0),position:(health.samplePos??0)/48000,duration:count/48000,currentId:'sync-proof',title:'Synchronization proof',playlist:[{id:'sync-proof',title:'Synchronization proof'}]});
 try{
  await until(()=>ready);timer=setInterval(()=>{send({type:'health'});if(host)host.publishState(state());},100);
  const listener=net.createServer();await new Promise(r=>listener.listen(0,'127.0.0.1',r));const port=listener.address().port;await new Promise(r=>listener.close(r));
  host=new RemoteSession({maxPeers:2,status(){},gate:v=>command({type:'setRemoteSync',...v}),position:()=>Number(health.samplePos??0)/48000,diagnostic:v=>{if(v.sync)events.push(v);},route:v=>command({type:'setRemoteOutput',...v}),disconnected:()=>command({type:'setRemoteOutput',address:null,token:null}),control:c=>host.completeControl(c.id)});
  await host.host({port,bufferMs:600});host.publishState(state());
  browser=await chromium.launch({channel:'msedge',headless:true,args:['--autoplay-policy=no-user-gesture-required']});
  const context=await browser.newContext({ignoreHTTPSErrors:true,bypassCSP:true});await context.addInitScript(()=>{window.__audioErrors=[];document.addEventListener('error',e=>{if(e.target instanceof HTMLMediaElement)window.__audioErrors.push({code:e.target.error?.code,message:e.target.error?.message,time:e.target.currentTime});},true);});await context.addInitScript(require('./test/remote-hls-mse-shim.cjs'));await context.route('**/*.m4s',async route=>{const sequence=Number(new URL(route.request().url()).pathname.split('/').pop().split('.')[0]);if(blockMedia&&sequence>=3)await new Promise(r=>blockedRequests.push(r));await route.continue().catch(()=>{});});page=await context.newPage();
  page.on('pageerror',e=>events.push({browserError:e.message}));
  await page.goto(host.webInvite('127.0.0.1'));if(!lateJoin){await page.locator('#connect').click();await until(()=>host.phase==='connected');}
  await command({type:'setHrtf',set:'hrtf',wetWeight:.04});await command({type:'setStereoMode',mode:'original'});await command({type:'reset',origin:0});
  if(adm){
   assert.equal(adm.format.sampleRate,48000);const ids=adm.labels.map((label,i)=>{const object=adm.adm.objectChannels.find(o=>o.channel===i);return object?'obj:'+object.id:'bed:'+i;});
   await command({type:'setVolume',volume:0});await command({type:'setObjectHrtf',enabled:true});
   for(let i=0;i<ids.length;i++)await command({type:'addSource',id:ids[i],...(ids[i].startsWith('bed:')?{bedLabel:adm.labels[i]}:{})});
   const file=fs.openSync(adm.path,'r');let event=0;try{for(let at=0;at<count;at+=4096){
    const n=Math.min(4096,count-at),raw=Buffer.alloc(n*ids.length*3);fs.readSync(file,raw,0,raw.length,adm.dataOffset+at*ids.length*3);
    const events=[];while(event<adm.adm.events.length&&adm.adm.events[event].samplePos<at+n)events.push(adm.adm.events[event++]);
    const json=Buffer.from(JSON.stringify(events)),head=Buffer.alloc(15);head[0]=70;head.writeUInt32LE(json.length,1);head.writeBigUInt64LE(BigInt(at),5);head.writeUInt16LE(ids.length,13);
    const chunks=[head.subarray(0,5),json,head.subarray(5)];
    for(let ch=0;ch<ids.length;ch++){const id=Buffer.from(ids[ch]),h=Buffer.alloc(6),pcm=Buffer.alloc(n*4);h.writeUInt16LE(id.length);h.writeUInt32LE(n,2);for(let i=0;i<n;i++)pcm.writeFloatLE(raw.readIntLE((i*ids.length+ch)*3,3)/8388608,i*4);chunks.push(h,id,pcm);}
    await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(Error('ADM batch timeout')),10000);pending.set('batch:'+at,m=>{clearTimeout(timeout);m.accepted?resolve():reject(Error('ADM batch rejected'));});child.stdin.write(Buffer.concat(chunks));});
   }}finally{fs.closeSync(file);}
  }else{
  for(const id of ['bed:0','bed:1']){
   await command({type:'addSource',id,bedLabel:id==='bed:0'?'FrontLeft':'FrontRight'});
   const samples=Float32Array.from({length:count},(_,i)=>Math.sin(i*2*Math.PI*440/48000)*.000001),name=Buffer.from(id),h=Buffer.alloc(15);h[0]=80;h.writeUInt16LE(name.length,1);h.writeBigUInt64LE(0n,3);h.writeUInt32LE(samples.length,11);child.stdin.write(Buffer.concat([h,name,Buffer.from(samples.buffer)]));
  }
  }
  playing=true;host.publishState(state());await command({type:'startAt',origin:0});await command({type:'setRemoteEnd',sample:count});
  if(lateJoin){await until(()=>health.samplePos>24000);await page.locator('#connect').click();await until(()=>host.phase==='connected');await sleep(400);const heldJoin=health.samplePos;await sleep(300);assert.equal(health.samplePos,heldJoin,'joining a live song must hold the real native consumer');}else{await sleep(800);assert.equal(health.samplePos,0,'native output must remain at sample zero while phone buffers');}
  await until(()=>host.sync.phase==='running',30000);const start=events.findLast(v=>v.reason==='scheduled-start');
  assert.ok(start.at>Date.now(),'native and browser arm before the common deadline');
  const startCursor=health.samplePos;await until(()=>health.samplePos>startCursor+48000,15000);
  const audio=()=>page.evaluate(()=>{const a=document.querySelector('audio');return {time:a.currentTime,paused:a.paused,ready:a.readyState,ended:a.ended,error:a.error?.message};});
  let received=await audio();assert.equal(received.paused,false,JSON.stringify({received,events}));skews.push(Math.round(1000*(received.time+host.peer.positionBase-health.samplePos/48000)));assert.ok(Math.abs(skews.at(-1))<300,'actual media and native sample clocks differ by more than 300 ms');
  paused=true;await command({type:'pause',paused:true});host.publishState(state());await sleep(650);
  const held=health.samplePos;received=await audio();assert.equal(received.paused,true);await sleep(250);assert.equal(health.samplePos,held,'native consumer must also stop');
  paused=false;await command({type:'pause',paused:false});host.publishState(state());
  await until(()=>health.samplePos>held+12000,15000);received=await audio();skews.push(Math.round(1000*(received.time+host.peer.positionBase-health.samplePos/48000)));assert.ok(Math.abs(skews.at(-1))<300,'resume must align the real output clocks');
  await until(()=>events.some(e=>e.reason==='receiver-buffer-low'),15000);await sleep(700);const stalled=health.samplePos;await sleep(300);assert.equal(health.samplePos,stalled,'native must hold when network buffer is exhausted');
  blockMedia=false;for(const release of blockedRequests.splice(0))release();
  await until(()=>health.samplePos>stalled+4800,15000);
  const secondContext=await browser.newContext({ignoreHTTPSErrors:true,bypassCSP:true});await secondContext.addInitScript(()=>{window.__audioErrors=[];});await secondContext.addInitScript(require('./test/remote-hls-mse-shim.cjs'));const second=await secondContext.newPage();await second.goto(host.webInvite('127.0.0.1'));await second.locator('#connect').click();
  await until(()=>host.hostPeers.size===2&&host.sync.phase==='running',20000);await sleep(1800);
  const secondTime=await second.evaluate(()=>document.querySelector('audio').currentTime);const secondPeer=[...host.hostPeers][1];skews.push(Math.round(1000*(secondTime+secondPeer.positionBase-health.samplePos/48000)));assert.ok(Math.abs(skews.at(-1))<300,'second receiver must share the native clock');
  await until(()=>health.samplePos>=count,20000);await until(async()=>(await audio()).ended,4000);
  received=await audio();assert.ok(Math.abs(received.time+host.peer.positionBase-count/48000)<.02,'partial final segment must play');
  console.log(JSON.stringify({nativeHeldUntilReady:true,commonStart:true,commonPause:true,commonResume:true,networkRebufferHoldsBoth:true,twoReceiversAligned:true,nativeEnd:health.samplePos/48000,browserEnd:received.time+host.peer.positionBase,joinedDuringPlayback:lateJoin,sourceCount:health.activeSources,sourceUnderruns:health.underrunSamples,clockSkewMs:skews,events:events.filter(v=>v.sync)},null,2));
 }catch(e){console.error(e.message.replace(/#[a-f0-9]{64}/g,'#[redacted]'));if(page){console.error(await page.locator('#message').innerText().catch(()=>''));console.error(await page.evaluate(()=>window.__audioErrors).catch(()=>[]));}console.error(JSON.stringify({health,events},null,2));process.exitCode=1;}
 finally{blockMedia=false;for(const release of blockedRequests.splice(0))release();clearInterval(timer);await browser?.close();await host?.stop().catch(()=>{});child.kill();}
})().catch(e=>{console.error(e.message.replace(/#[a-f0-9]{64}/g,'#[redacted]'));process.exitCode=1;});
