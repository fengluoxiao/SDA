'use strict';
const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const ts=require('typescript');
const layouts=require('./layouts.json');
const {SystemDecoder}=require('./decoder.cjs');
const {pcmFormat}=require('./iec61937.cjs');
const context={exports:{}};
vm.runInNewContext(ts.transpileModule(fs.readFileSync(require('node:path').join(__dirname,'../../packages/renderer/src/layouts.ts'),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText,context);
assert.equal(JSON.stringify(layouts),JSON.stringify(Object.fromEntries(Object.entries(context.exports.LAYOUTS).map(([id,channels])=>[id,channels.map(c=>c.name)]))), 'input order must match every SDA layout');
for(const [id,labels] of Object.entries(layouts)) for(const [bits,float] of [[16,false],[24,false],[32,false],[32,true]]) {
  const count=labels.length,stride=count*bits/8,w=Buffer.alloc(40);
  w.writeUInt16LE(0xfffe);w.writeUInt16LE(count,2);w.writeUInt32LE(48000,4);w.writeUInt32LE(48000*stride,8);
  w.writeUInt16LE(stride,12);w.writeUInt16LE(bits,14);w.writeUInt16LE(22,16);w.writeUInt16LE(bits,18);w.writeUInt32LE(float?3:1,24);
  Buffer.from('00001000800000aa00389b71','hex').copy(w,28);
  assert.deepEqual(pcmFormat(w,id).labels,labels);
  assert.throws(()=>pcmFormat(w),/explicit input layout/);
  const payload=Buffer.alloc(count*stride),frames=[];
  for(let c=0;c<count;c++){let at=c*stride+c*bits/8;if(float)payload.writeFloatLE(.5,at);else payload.writeIntLE(2**(bits-2),at,bits/8);}
  const decoder=new SystemDecoder({discreteLayout:id,onFrame:f=>frames.push(f)});
  for(let at=0;at<payload.length;at+=13){const part=payload.subarray(at,at+13);decoder.accept({epoch:1n,offset:BigInt(at),produced:BigInt(at+part.length),state:3,format:w,payload:part,overflows:0n});}
  for(let c=0;c<count;c++) assert.deepEqual(frames.flatMap(f=>Array.from(f.channels[c])),Array.from({length:count},(_,n)=>n===c?.5:0),`${id} channel ${c}`);
  assert(frames.every(f=>JSON.stringify(f.labels)===JSON.stringify(labels)&&f.objectChannels.length===0));
  decoder.close();
}
console.log(`PASS ${Object.keys(layouts).length} layouts, all isolated channels, four sample formats, fragmented transport, explicit mapping required`);
