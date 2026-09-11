const fs=require('node:fs'),{spawn}=require('node:child_process'),path=require('node:path'),assert=require('node:assert/strict');
const {eventBatches}=require('../apps/desktop/native-event-batches.cjs');
const m=JSON.parse(fs.readFileSync(process.env.SDA_ADM_BENCHMARK||'tmp/adm-performance.json','utf8'));
const combined=!process.argv.includes('--legacy'),seconds=Math.min(m.dataSize/m.format.blockAlign/m.format.sampleRate,Number(process.env.SDA_BENCH_SECONDS||Infinity));
const exe=process.env.SDA_TEST_EXE||'apps/native-renderer/target/release/sda-native-renderer.exe';
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
(async()=>{
const child=spawn(path.resolve(exe),[],{windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,SDA_HRTF_ROOT:path.resolve('apps/web/public'),SDA_OUTPUT_SETTINGS:JSON.stringify({deviceId:null,exclusive:false})}});
let lines='',ready=false,health={samplePos:0},waiters=new Map(),timer,started=0,accepted=0,maxAck=0,minAhead=4,runningFifoUnderruns=0;
child.stderr.on('data',()=>{});child.stdin.on('error',()=>{});
child.stdout.on('data',d=>{lines+=d;let i;while((i=lines.indexOf('\n'))>=0){const line=lines.slice(0,i);lines=lines.slice(i+1);try{const e=JSON.parse(line);if(e.type==='ready')ready=true;if(e.type==='health'){health=e;if(e.samplePos>0&&e.samplePos<(seconds-1)*48000)runningFifoUnderruns=Math.max(runningFifoUnderruns,e.callbackFifoUnderrunFrames);}const key=e.type==='ack'?e.command:e.type==='batchAck'?`batch:${e.start}`:e.type;const w=waiters.get(key);if(w){waiters.delete(key);w(e);}}catch{}}});
function wait(key){return new Promise((resolve,reject)=>{const t=setTimeout(()=>{waiters.delete(key);reject(Error('timeout '+key));},30000);waiters.set(key,v=>{clearTimeout(t);resolve(v);});});}
function send(c){const json=Buffer.from(JSON.stringify(c)),h=Buffer.alloc(5);h[0]=74;h.writeUInt32LE(json.length,1);child.stdin.write(Buffer.concat([h,json]));}
async function cmd(c){const w=wait(c.type);send(c);const e=await w;assert.equal(e.accepted,true,JSON.stringify(e));}
const ids=m.labels.map((l,i)=>{const o=m.adm.objectChannels.find(o=>o.channel===i);return o?`obj:${o.id}`:`bed:${i}`;});
let fd;
try{
while(!ready)await sleep(10);
await cmd({type:'setHrtf',set:process.env.SDA_HRTF_SET||'hrtf',wetWeight:.04});await cmd({type:'setVolume',volume:0});await cmd({type:'setDirectionalHrtf',enabled:true});
for(let i=0;i<ids.length;i++)await cmd({type:'addSource',id:ids[i],...(ids[i].startsWith('bed:')?{bedLabel:m.labels[i]}:{})});
await cmd({type:'setObjectHrtf',enabled:true});
fd=fs.openSync(m.path,'r');let ei=0;
const raw=Buffer.alloc(4096*ids.length*3);
for(let at=0;at<seconds*48000;at+=4096){
 while(started&&at-health.samplePos>4*48000)await sleep(5);
 const n=Math.min(4096,seconds*48000-at);fs.readSync(fd,raw,0,n*ids.length*3,m.dataOffset+at*ids.length*3);
 const events=[];while(ei<m.adm.events.length&&m.adm.events[ei].samplePos<at+n)events.push(m.adm.events[ei++]);
 const chunks=[],header=Buffer.alloc(11);header[0]=66;header.writeBigUInt64LE(BigInt(at),1);header.writeUInt16LE(ids.length,9);
 if(combined){const json=Buffer.from(JSON.stringify(events)),meta=Buffer.alloc(5);meta[0]=70;meta.writeUInt32LE(json.length,1);chunks.push(meta,json,header.subarray(1));}else{for(const batch of eventBatches(events))await cmd({type:'objectEvents',events:batch});chunks.push(header);}
 for(let ch=0;ch<ids.length;ch++){const id=Buffer.from(ids[ch]),h=Buffer.alloc(6),pcm=Buffer.alloc(n*4);h.writeUInt16LE(id.length);h.writeUInt32LE(n,2);for(let i=0;i<n;i++)pcm.writeFloatLE(raw.readIntLE((i*ids.length+ch)*3,3)/8388608,i*4);chunks.push(h,id,pcm);}
 const t=performance.now(),w=wait(`batch:${at}`);child.stdin.write(Buffer.concat(chunks));const e=await w;maxAck=Math.max(maxAck,performance.now()-t);assert.equal(e.accepted,true,JSON.stringify(e));accepted=at+n;
 if(!started&&accepted>=1.5*48000){await cmd({type:'startAt',origin:0});started=Date.now();timer=setInterval(()=>send({type:'health'}),100);}
 if(started&&Date.now()-started>2000)minAhead=Math.min(minAhead,(accepted-health.samplePos)/48000);
}
while(health.samplePos<seconds*48000)await sleep(25);
clearInterval(timer);await cmd({type:'pause',paused:true});
assert.equal(health.underrunSamples,0,'real source samples must not be lost');
if(process.env.SDA_ASSERT_REALTIME)assert.equal(runningFifoUnderruns,0,'output starved during playback');
console.log(JSON.stringify({combined,seconds,wallSeconds:(Date.now()-started)/1000,minAhead,maxAck,runningFifoUnderruns,health},null,2));
}finally{clearInterval(timer);if(fd)fs.closeSync(fd);child.kill();}
})().catch(e=>{console.error(e);process.exitCode=1;});

