const test=require('node:test'),assert=require('node:assert/strict');
const {RemoteSync}=require('../remote-sync.cjs');
const tick=()=>new Promise(resolve=>setImmediate(resolve));
const peer=()=>({epoch:2,positionBase:0,messages:[],forward(k,v){this.messages.push([k,v]);}});
const feedback=(sync,p,extra={})=>sync.feedback(p,{epoch:p.epoch,revision:sync.revision,time:0,aheadMs:3000,readyState:4,paused:true,syncReady:true,...extra});
test('short render reserve requires every receiver to request low-latency HLS and grows on slow links',()=>{
 const s=new RemoteSync(),a=peer(),b=peer();s.peers.add(a);assert.equal(s.bufferMs(),6000);
 a.lowLatency=true;assert.equal(s.bufferMs(),6000);
 a.nativeHoldbackSeconds=0;assert.equal(s.bufferMs(),4000);
 a.nativeHoldbackSeconds=3.001;assert.equal(s.bufferMs(),4501);
 a.clockRtt=1000;assert.equal(s.bufferMs(),5000);
 s.peers.add(b);assert.equal(s.bufferMs(),6000);
});
test('stable Safari startup lag changes the next actual play schedule instead of repeating identical restarts',async()=>{
 const s=new RemoteSync({gate:async()=>true,position:()=>3}),a=peer();s.add(a);s.update({playing:true});await tick();s.settledAt=0;
 feedback(s,a);feedback(s,a);await tick();s.startedAt=Date.now()-3000;
 for(let i=0;i<3;i++)feedback(s,a,{time:2.5,paused:false,syncPhase:'running',hostTime:Date.now(),aheadMs:5000});
 await tick();assert.equal(s.phase,'preparing');assert.ok(Math.abs(a.playLeadMs-500)<20);
 s.settledAt=0;feedback(s,a);feedback(s,a);await tick();
 assert.ok(Math.abs(a.messages.at(-1)[1].playLeadMs-500)<20);
});
test('first audible start uses the receiver warm-start measurement before scheduling',async()=>{
 const s=new RemoteSync({gate:async()=>true,position:()=>0}),a=peer();s.add(a);s.update({playing:true});await tick();s.settledAt=0;
 feedback(s,a);feedback(s,a,{startupLeadMs:438});await tick();
 assert.equal(a.messages.at(-1)[1].action,'start');assert.equal(a.messages.at(-1)[1].playLeadMs,438);
 assert.equal(a.calibratedEpoch,a.epoch);
});
test('new media probe cannot overwrite a lead calibrated from actual playback',async()=>{
 const s=new RemoteSync({gate:async()=>true,position:()=>0}),a=peer();a.playLeadMs=380;a.measuredPlaybackLead=true;
 s.add(a);s.update({playing:true});await tick();s.settledAt=0;
 feedback(s,a);feedback(s,a,{startupLeadMs:38});await tick();
 assert.equal(a.messages.at(-1)[1].playLeadMs,380);
});
test('stale revision feedback cannot cause a new clock-drift hold',async()=>{
 const s=new RemoteSync({gate:async()=>true,position:()=>3}),a=peer();s.add(a);s.update({playing:true});await tick();s.settledAt=0;
 feedback(s,a);feedback(s,a);await tick();s.startedAt=Date.now()-3000;
 for(let i=0;i<4;i++)feedback(s,a,{revision:s.revision-1,time:0,paused:false,syncPhase:'running',hostTime:Date.now()});
 assert.equal(s.phase,'running');
});
test('clock estimate changes while armed do not falsely mark the existing start timer late',async()=>{
 const {synchronizeMedia,localClock}=await import('../remote-web/synchronized-playback.mjs');
 let callback,plays=0;const original=global.setTimeout;
 const owner={clockOffset:0,audio:{play(){plays++;return Promise.resolve();}}};
 try{
   global.setTimeout=fn=>{callback=fn;return undefined;};
   synchronizeMedia(owner,{action:'start',revision:1,at:localClock()+20});
   owner.clockOffset=1000;callback();
   assert.equal(plays,1);assert.equal(owner.syncPhase,'running');
 }finally{global.setTimeout=original;}
});
test('Safari resync seeks into available unbuffered media to trigger its download, then waits for real data',async()=>{
 const {prepareSynchronizedMedia}=await import('../remote-web/synchronized-playback.mjs');
 const audio={currentTime:0,readyState:4,seeking:false,buffered:{length:1,start:()=>0,end:()=>1},seekable:{length:1,start:()=>0,end:()=>4}};
 const owner={audio,synchronized:true,syncPhase:'preparing',syncPosition:1.45,mediaClockBase:0,clockOffset:0};
 assert.equal(prepareSynchronizedMedia(owner),false);assert.equal(audio.currentTime,1.45);
 assert.equal(prepareSynchronizedMedia(owner),false);
 audio.buffered.start=()=>1;audio.buffered.end=()=>3;
 assert.equal(prepareSynchronizedMedia(owner),true);
 owner.syncPosition=8;assert.equal(prepareSynchronizedMedia(owner),false);assert.equal(audio.currentTime,1.45);
});
test('small native HLS timestamp gaps are tolerated, missing audio is not',async()=>{
 const {bufferedAhead,prepareSynchronizedMedia}=await import('../remote-web/synchronized-playback.mjs');
 const audio={currentTime:0,readyState:4,seeking:false,buffered:{length:1,start:()=>.001,end:()=>6}};
 const owner={audio,synchronized:true,syncPhase:'preparing',syncPosition:0,clockOffset:0};
 assert.ok(bufferedAhead(audio)>5);assert.equal(prepareSynchronizedMedia(owner),true);
 audio.buffered.start=()=>1;assert.equal(bufferedAhead(audio),0);assert.equal(prepareSynchronizedMedia(owner),false);
 audio.buffered.start=()=>0;audio.buffered.end=()=>2;owner.syncMinBuffer=3;
 assert.equal(prepareSynchronizedMedia(owner),false);
});

test('Safari buffered data beyond seekable edge waits without repeatedly seeking backwards',async()=>{
 const {prepareSynchronizedMedia}=await import('../remote-web/synchronized-playback.mjs');
 let writes=0,time=7.399,end=7.399;
 const audio={get currentTime(){return time;},set currentTime(v){writes++;time=v;},readyState:4,seeking:false,
  buffered:{length:1,start:()=>0,end:()=>10.4},seekable:{length:1,start:()=>0,end:()=>end}};
 const owner={audio,synchronized:true,syncPhase:'preparing',syncPosition:13.44,mediaClockBase:4.97,clockOffset:0};
 assert.equal(prepareSynchronizedMedia(owner),false);assert.equal(writes,0);
 end=10;assert.equal(prepareSynchronizedMedia(owner),false);assert.equal(writes,1);
 assert.ok(Math.abs(time-8.47)<.001);assert.equal(prepareSynchronizedMedia(owner),true);
});
test('slow tunnel gets more startup reserve and a shared longer deadline; repeated play does not restart loading',async()=>{
 const gates=[],s=new RemoteSync({gate:async v=>{gates.push(v);return true;},position:()=>0}),a=peer();
 s.add(a);s.update({playing:true});await tick();s.settledAt=0;
 const revision=s.revision;s.hold('browser-play-request');assert.equal(s.revision,revision);
 feedback(s,a,{clockRtt:1400});assert.equal(a.messages.at(-1)[1].minBufferSeconds,3);
 feedback(s,a,{clockRtt:1400});await tick();
 const start=a.messages.at(-1)[1];assert.ok(start.at-Date.now()>3000);assert.equal(start.at,gates.at(-1).startAtMs);
});
test('both receivers and the native consumer share one future start after readiness barrier',async()=>{
 const gates=[],s=new RemoteSync({gate:async v=>{gates.push(v);return true;},position:()=>0}),a=peer(),b=peer();
 s.add(a);s.add(b);s.update({playing:true,paused:false,loading:true});await tick();s.settledAt=0;
 feedback(s,a);feedback(s,b); // prepare each receiver at actual native sample 0
 feedback(s,a);assert.equal(s.phase,'preparing');assert.ok(gates.every(g=>!g.startAtMs));
 feedback(s,b);await tick();assert.equal(s.phase,'running');
 const start=a.messages.findLast(v=>v[1].action==='start')[1];
 assert.ok(start.at>Date.now());assert.equal(start.at,gates.at(-1).startAtMs);
 assert.deepEqual(b.messages.findLast(v=>v[1].action==='start')[1],start);
});
test('stale readiness cannot release reset gate and starvation pauses all actual consumers',async()=>{
 const gates=[],s=new RemoteSync({gate:async v=>gates.push(v),position:()=>5}),a=peer();
 s.add(a);s.update({playing:true});await tick();s.settledAt=0;const previous=s.revision;
 s.reset();await tick();s.settledAt=0;feedback(s,a);feedback(s,a,{revision:previous});assert.equal(s.phase,'preparing');
 feedback(s,a);await tick();s.startedAt=Date.now()-2000;
 feedback(s,a,{paused:false,aheadMs:50,waiting:true,readyState:2});await tick();
 assert.equal(s.phase,'preparing');assert.ok(gates.at(-1).stopAtMs>Date.now());
 assert.equal(a.messages.at(-1)[1].action,'hold');
 s.remove(a);await tick();assert.equal(gates.at(-1).enabled,false);
});
test('old async start acknowledgement cannot restart consumers after reset',async()=>{
 let release;const s=new RemoteSync({gate:v=>v.startAtMs?new Promise(r=>release=r):Promise.resolve(true),position:()=>0}),a=peer();
 s.add(a);s.update({playing:true});await tick();s.settledAt=0;feedback(s,a);feedback(s,a);assert.equal(s.phase,'arming');
 s.reset();await tick();release(true);await tick();assert.equal(s.phase,'preparing');assert.ok(!a.messages.some(v=>v[1].action==='start'));
});
test('browser seeks to the held source sample and does not claim ready until buffered',async()=>{
 const {prepareSynchronizedMedia,synchronizeMedia}=await import('../remote-web/synchronized-playback.mjs');
 const audio={currentTime:0,readyState:4,seeking:false,buffered:{length:1,start:()=>0,end:()=>4},pause(){this.paused=true;}};
 const owner={audio,synchronized:true,mediaClockBase:12,clockOffset:0};
 synchronizeMedia(owner,{action:'prepare',revision:3,position:14});assert.equal(audio.paused,true);
 assert.equal(prepareSynchronizedMedia(owner),false);assert.equal(audio.currentTime,2);
 assert.equal(prepareSynchronizedMedia(owner),true);
 synchronizeMedia(owner,{action:'prepare',revision:2,position:13});assert.equal(owner.syncPosition,14);
 audio.buffered.end=()=>2.2;assert.equal(prepareSynchronizedMedia(owner),false);
});
test('HLS end marker seals the final partial segment without waiting for local EOF',()=>{
 const {HlsPeer}=require('../remote-hls.cjs'),{packet,decodePackets}=require('../remote-session.cjs');
 const p=new HlsPeer({id:'a'.repeat(32),packet,decodePackets,onClose(){}});p.on('data',()=>{});
 try{p.positionBase=0;p.endSample=480*125;p.receive('H',Buffer.from(JSON.stringify({protocol:1})));
 for(let i=0;i<125;i++)p.receive('A',Buffer.alloc(3840));
 assert.equal(p.segments.length,2);assert.equal(p.segments[1].duration,.25);assert.equal(p.frames.length,0);
 assert.match(p.playlist().toString(),/#EXT-X-ENDLIST/);assert.equal(p.mediaInfo().mediaReady,true);
 p.receive('R',Buffer.alloc(0));assert.equal(p.programEnded,false);assert.equal(p.endSample,null);
 }finally{p.destroy();}
});

test('stalled paused Safari reloads the same stream and still waits at the held sample',async()=>{
 const {recoverSynchronizedMedia,prepareSynchronizedMedia}=await import('../remote-web/synchronized-playback.mjs');
 let loads=0,plays=0,end=89;
 const audio={currentTime:88.32,paused:true,readyState:2,seeking:false,
  buffered:{length:1,start:()=>84,end:()=>end},seekable:{length:1,start:()=>84,end:()=>94},
  load(){loads++;this.currentTime=0;this.readyState=1;},play(){plays++;}};
 const owner={audio,synchronized:true,syncPhase:'preparing',syncPosition:88.32,syncRevision:12,mediaEpoch:5,clockOffset:0,syncMinBuffer:3};
 assert.equal(recoverSynchronizedMedia(owner,0),false);
 assert.equal(recoverSynchronizedMedia(owner,9999),false);
 assert.equal(recoverSynchronizedMedia(owner,10000),true);
 assert.equal(loads,1);assert.equal(plays,0);assert.equal(owner.syncPosition,88.32);
 assert.equal(prepareSynchronizedMedia(owner),false);assert.equal(audio.currentTime,88.32);
 end=94;audio.readyState=4;
 assert.equal(prepareSynchronizedMedia(owner),true);
 assert.equal(recoverSynchronizedMedia(owner,30000),false);assert.equal(loads,1);
});

test('Safari recovery is bounded and does not reload explicit pause or healthy buffering',async()=>{
 const {recoverSynchronizedMedia}=await import('../remote-web/synchronized-playback.mjs');
 let loads=0,end=.5;const audio={currentTime:0,paused:true,readyState:2,buffered:{length:1,end:()=>end},load(){loads++;}};
 const owner={audio,synchronized:true,syncPhase:'preparing',syncPosition:0,syncRevision:1};
 recoverSynchronizedMedia(owner,0);end=.8;recoverSynchronizedMedia(owner,9000);
 assert.equal(recoverSynchronizedMedia(owner,10000),false);
 recoverSynchronizedMedia(owner,19000);recoverSynchronizedMedia(owner,20000);recoverSynchronizedMedia(owner,30000);
 recoverSynchronizedMedia(owner,40000);recoverSynchronizedMedia(owner,50000);assert.equal(loads,2);
 owner.hostRunning=false;owner.syncRevision=2;recoverSynchronizedMedia(owner,60000);recoverSynchronizedMedia(owner,80000);assert.equal(loads,2);
});
