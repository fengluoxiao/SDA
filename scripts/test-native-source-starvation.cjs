const {spawn}=require('node:child_process'),path=require('node:path'),assert=require('node:assert/strict');
const executable=process.env.SDA_TEST_EXE||path.resolve('apps/native-renderer/target/release/sda-native-renderer.exe');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
(async()=>{
 const child=spawn(executable,[],{windowsHide:true,stdio:['pipe','pipe','pipe'],env:{...process.env,SDA_HRTF_ROOT:path.resolve('apps/web/public'),SDA_OUTPUT_SETTINGS:JSON.stringify({deviceId:null,exclusive:false})}});
 let text='',ready=false,health=null,acks=new Map(),batch=null;
 child.stderr.on('data',()=>{});child.stdin.on('error',()=>{});
 child.stdout.on('data',d=>{text+=d;let n;while((n=text.indexOf('\n'))>=0){const line=text.slice(0,n);text=text.slice(n+1);try{const m=JSON.parse(line);if(m.type==='ready')ready=true;if(m.type==='health')health=m;if(m.type==='ack')acks.set(m.command,m);if(m.type==='batchAck')batch=m;}catch{}}});
 async function until(fn){const end=Date.now()+15000;while(!fn()){if(Date.now()>end)throw Error('timeout');await sleep(10);}}
 function send(c){const b=Buffer.from(JSON.stringify(c)),h=Buffer.alloc(5);h[0]=74;h.writeUInt32LE(b.length,1);child.stdin.write(Buffer.concat([h,b]));}
 async function command(c){acks.delete(c.type);send(c);await until(()=>acks.has(c.type));assert.equal(acks.get(c.type).accepted,true);}
 async function snapshot(){health=null;send({type:'health'});await until(()=>health);return health;}
 const ids=Array.from({length:118},(_,i)=>i<10?`bed:${i}`:`obj:${i-10}`);
 async function feed(start,count){batch=null;const h=Buffer.alloc(11);h[0]=66;h.writeBigUInt64LE(BigInt(start),1);h.writeUInt16LE(ids.length,9);const metadata=Buffer.from(JSON.stringify([])),prefix=Buffer.alloc(5);prefix[0]=70;prefix.writeUInt32LE(metadata.length,1);const parts=process.env.SDA_TEST_ATOMIC?[prefix,metadata,h.subarray(1)]:[h];const pcm=Buffer.alloc(count*4);for(let i=0;i<count;i++)pcm.writeFloatLE(Math.sin((start+i)*.05)*.00001,i*4);
  for(const id of ids){const name=Buffer.from(id),entry=Buffer.alloc(6);entry.writeUInt16LE(name.length,0);entry.writeUInt32LE(count,2);parts.push(entry,name,pcm);}child.stdin.write(Buffer.concat(parts));await until(()=>batch);assert.equal(batch.accepted,true);assert.ok(!batch.detail?.includes('stale'),JSON.stringify(batch));}
 try{
  await until(()=>ready);await command({type:'setHrtf',set:'hrtf',wetWeight:.04});
  for(let i=0;i<ids.length;i++)await command({type:'addSource',id:ids[i],...(i<10?{bedLabel:['FrontLeft','FrontRight','FrontCenter','LFE','SideLeft','SideRight','BackLeft','BackRight','TopFrontLeft','TopFrontRight'][i]}:{})});
  await feed(0,24576);await command({type:'startAt',origin:0});
  await sleep(1600);const before=await snapshot();
  console.log({phase:'decoderGap',position:before.samplePos,underruns:before.underrunSamples});
  assert.equal(before.samplePos,24576,'clock must stop at last real sample while next batch is late');
  await feed(24576,24576);await sleep(1200);const after=await snapshot();
  assert.equal(after.samplePos,49152);assert.equal(after.underrunSamples,0);
  console.log({sources:118,lateBatchPreserved:true,position:after.samplePos,underruns:after.underrunSamples});
 }finally{child.kill();}
})().catch(e=>{console.error(e);process.exitCode=1});
