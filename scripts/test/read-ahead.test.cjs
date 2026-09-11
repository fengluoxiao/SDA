const test=require('node:test'),assert=require('node:assert/strict');
const load=()=>import('../../apps/web/src/read-ahead.ts');
test('one read overlaps decoding, preserves order and bounds memory to one prefetched chunk',async()=>{
 const {readAhead}=await load(),calls=[],gates=new Map();
 const stream=readAhead(10,4,(offset,length)=>{calls.push([offset,length]);return new Promise(resolve=>gates.set(offset,()=>resolve(new Uint8Array(length).fill(offset))));});
 const first=stream.next();await new Promise(r=>setImmediate(r));assert.deepEqual(calls,[[0,4]]);gates.get(0)();
 assert.deepEqual([...(await first).value],[0,0,0,0]);await new Promise(r=>setImmediate(r));assert.deepEqual(calls,[[0,4],[4,4]]);
 await new Promise(r=>setImmediate(r));assert.equal(calls.length,2,'slow consumer must not trigger unbounded reads');
 gates.get(4)();assert.deepEqual([...(await stream.next()).value],[4,4,4,4]);await new Promise(r=>setImmediate(r));assert.deepEqual(calls,[[0,4],[4,4],[8,2]]);
 gates.get(8)();assert.deepEqual([...(await stream.next()).value],[8,8]);assert.equal((await stream.next()).done,true);
});
test('cancellation waits for its pending read and handles a rejected prefetch',async()=>{
 const {readAhead}=await load();let rejectRead;const stream=readAhead(8,4,offset=>offset?new Promise((_,reject)=>rejectRead=reject):Promise.resolve(new Uint8Array(4)));
 await stream.next();await new Promise(r=>setImmediate(r));let closed=false;const close=stream.return().then(()=>closed=true);await new Promise(r=>setImmediate(r));assert.equal(closed,false);
 rejectRead(Error('cancelled file'));await close;assert.equal(closed,true);
});
test('truncated chunks and read failures surface at their original offsets',async()=>{
 const {readAhead}=await load();await assert.rejects(readAhead(4,4,async()=>new Uint8Array(3)).next(),/byte 0/);
 await assert.rejects(readAhead(4,4,async()=>{throw Error('disk error');}).next(),/disk error/);
});
