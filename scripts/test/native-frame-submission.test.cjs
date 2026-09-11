const test=require('node:test'),assert=require('node:assert/strict');
const {eventBatches}=require('../../apps/desktop/native-event-batches.cjs');
test('118 object events fit byte limit without arbitrary 32-event ACK rounds',()=>{
 const events=Array.from({length:118},(_,id)=>({id,samplePos:4096,pos:[.1,.2,.3],gain:1,ramp:4096}));
 const batches=eventBatches(events);assert.equal(batches.length,1);assert.deepEqual(batches.flat(),events);
});
test('large Unicode metadata splits within native byte bound and preserves every event',()=>{
 const events=Array.from({length:118},(_,id)=>({id,tag:'音响'.repeat(300),samplePos:id*4096}));
 const batches=eventBatches(events);assert.ok(batches.length>1);assert.deepEqual(batches.flat(),events);
 for(const batch of batches)assert.ok(Buffer.byteLength(JSON.stringify({type:'objectEvents',events:batch}))<=16000);
 assert.equal(eventBatches([{tag:'a'.repeat(16000)}]),null);
 assert.equal(eventBatches(Array(4097).fill({})),null);
});
test('future frame metadata cannot overtake current frame PCM',async()=>{
 const {NativeFrameQueue}=await import('../../packages/player/src/native-frame-queue.ts');
 const q=new NativeFrameQueue(),calls=[];
 await Promise.all(Array.from({length:12},(_,i)=>q.submit(async()=>{
  calls.push(`metadata:${i}`);await new Promise(r=>setImmediate(r));
  calls.push(`declare:${i}`);await new Promise(r=>setImmediate(r));
  calls.push(`pcm:${i}`);await new Promise(r=>setImmediate(r));
 })));
 assert.deepEqual(calls,Array.from({length:12},(_,i)=>[`metadata:${i}`,`declare:${i}`,`pcm:${i}`]).flat());
});
test('rejected frame does not block following queued work',async()=>{
 const {NativeFrameQueue}=await import('../../packages/player/src/native-frame-queue.ts');
 const q=new NativeFrameQueue();let next=false;
 const results=await Promise.allSettled([q.submit(async()=>{throw Error('rejected');}),q.submit(async()=>{next=true;})]);
 assert.equal(results[0].status,'rejected');assert.equal(results[1].status,'fulfilled');assert.equal(next,true);
});
const fs=require('node:fs'),vm=require('node:vm');
function transport(){
 const source=fs.readFileSync(require.resolve('../../apps/desktop/main.cjs'),'utf8');
 const writes=[],pending=new Map();
 const context={Buffer,ArrayBuffer,Float32Array,Number,Promise,Map,Date,setTimeout,clearTimeout,nativeRendererWritable:true,nativeRendererPendingBatches:pending,nativeRendererBatchQueue:[],NATIVE_RENDERER_BATCH_ACK_TIMEOUT_MS:1000,writeStartupLog(){},nativeRenderer:{stdin:{write(b){writes.push(b);return false;}}}};
 vm.createContext(context);vm.runInContext(source.slice(source.indexOf('function nativeRendererBatch('),source.indexOf('let outputDevicesState =')),context);
 return {context,writes,pending,submit:context.nativeRendererBatch,ack(start,result){const p=pending.get(start);pending.delete(start);clearTimeout(p.timeout);p.resolve(result);}};
}
test('combined native transaction retains metadata, PCM bytes and waits for real ACK',async()=>{
 const t=transport(),events=[{id:1,samplePos:128,pos:[1,0,0]}],samples=new Float32Array([.25,-.5]);let resolved=false;
 const promise=t.submit(128,[{id:'obj:1',samples}],events).then(v=>{resolved=true;return v;});
 await new Promise(r=>setImmediate(r));assert.equal(resolved,false);
 const b=t.writes[0],len=b.readUInt32LE(1);assert.equal(b[0],70);assert.deepEqual(JSON.parse(b.subarray(5,5+len)),events);
 let p=5+len;assert.equal(Number(b.readBigUInt64LE(p)),128);p+=8;assert.equal(b.readUInt16LE(p),1);p+=2;
 const idLength=b.readUInt16LE(p);assert.equal(b.readUInt32LE(p+2),2);p+=6;assert.equal(b.subarray(p,p+idLength).toString(),'obj:1');p+=idLength;
 assert.deepEqual(b.subarray(p),Buffer.from(samples.buffer));
 t.ack(128,{accepted:true,samples:2});assert.equal((await promise).accepted,true);
});
test('duplicate in-flight native batch shares its real ACK instead of throwing',async()=>{
 const t=transport(),entry=[{id:'bed:0',samples:new Float32Array([.5])}];
 const one=t.submit(0,entry,[]),two=t.submit(0,entry,[]);assert.equal(t.writes.length,1);
 t.ack(0,{accepted:true,samples:1});assert.equal((await one).samples,1);assert.equal((await two).samples,1);
});
test('oversized transaction metadata rejects without submitting partial PCM',async()=>{
 const t=transport();const result=await t.submit(0,[{id:'bed:0',samples:new Float32Array([1])}],[{text:'x'.repeat(1024*1024)}]);
 assert.equal(result.accepted,false);assert.equal(t.writes.length,0);
});
test('pipe drain preserves each queued frame metadata and individual backend ACK',async()=>{
 const t=transport();t.context.nativeRendererWritable=false;
 let first=false,second=false;
 const a=t.submit(0,[{id:'bed:0',samples:new Float32Array([1])}],[{id:1}]).then(()=>first=true);
 const b=t.submit(1,[{id:'bed:0',samples:new Float32Array([2])}],[{id:2}]).then(()=>second=true);
 const source=fs.readFileSync(require.resolve('../../apps/desktop/main.cjs'),'utf8');
 const start=source.indexOf('    // Preserve each frame\'s own backend ACK;');
 t.context.nativeRendererWritable=true;
 vm.runInContext(source.slice(start,source.indexOf('\n  });',start)),t.context);
 await new Promise(r=>setImmediate(r));assert.equal(first,false);assert.equal(second,false);assert.equal(t.writes.length,2);
 t.ack(1,{accepted:true,samples:1});await b;assert.equal(first,false);assert.equal(second,true);
 t.ack(0,{accepted:true,samples:1});await a;assert.equal(first,true);
 for(let i=0;i<2;i++){const bytes=t.writes[i];assert.equal(bytes[0],70);assert.deepEqual(JSON.parse(bytes.subarray(5,5+bytes.readUInt32LE(1))),[{id:i+1}]);}
});
