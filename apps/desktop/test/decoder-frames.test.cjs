const assert=require('node:assert/strict'),{DecoderFrameReader}=require('../decoder-frames.cjs');
function packet(header,payload=Buffer.alloc(0)){const meta=Buffer.from(JSON.stringify({...header,dataBytes:payload.length})),prefix=Buffer.alloc(4);prefix.writeUInt32LE(meta.length);return Buffer.concat([prefix,meta,payload]);}
const payload=Buffer.alloc(118*4096*4);for(let i=0;i<payload.length;i++)payload[i]=(i*17)%251;
const messages=[packet({type:'ready',channels:118}),packet({type:'frame',start:0},payload),packet({type:'frame',start:4096},Buffer.from([0,1,2,255])),packet({type:'frame',eof:true})];
const wire=Buffer.concat(messages);
for(const size of [1,3,4,7,16384,65536,wire.length]){
 const received=[],reader=new DecoderFrameReader((header,bytes)=>received.push({header,bytes}));
 for(let at=0;at<wire.length;at+=size)reader.push(wire.subarray(at,at+size));
 assert.equal(received.length,4,'fragment size '+size);assert.equal(received[0].header.type,'ready');assert.deepEqual(received[1].bytes,payload);assert.deepEqual(received[2].bytes,Buffer.from([0,1,2,255]));assert.equal(received[3].bytes.length,0);
 assert.equal(reader.payload,null);assert.equal(reader.metadata,null);
}
for(const bad of [0,1,32*1024*1024+1]){const prefix=Buffer.alloc(4);prefix.writeUInt32LE(bad);assert.throws(()=>new DecoderFrameReader(()=>{}).push(prefix));}
for(const bad of [-1,0.5,2*1024*1024+1,null]){const h=Buffer.from(JSON.stringify({dataBytes:bad})),prefix=Buffer.alloc(4);prefix.writeUInt32LE(h.length);assert.throws(()=>new DecoderFrameReader(()=>{}).push(Buffer.concat([prefix,h])));}
assert.throws(()=>new DecoderFrameReader(()=>{}).push(Buffer.from([2,0,0,0,123,123])));
assert(require('../package.json').build.files.includes('decoder-frames.cjs'),'packaged desktop must include the stream reader');
console.log('PASS: 118-lane decoder packets, arbitrary fragmentation/coalescing, empty frames, bounds, malformed metadata');
