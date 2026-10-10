// Exercise mobile import with genuine ISO BMFF mha1/mhm1 fixtures and shared Windows demux.
import {build} from 'esbuild';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import assert from 'node:assert/strict';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
const require=createRequire(import.meta.url), mp4=require('mp4box');
const dest=resolve('tmp/mpegh-import');mkdirSync(dest,{recursive:true});
await build({entryPoints:['apps/mobile/src/mpeghMp4.ts'],bundle:true,platform:'node',format:'cjs',outfile:dest+'/import.cjs'});
// Bundled parser is isolated from the fixture writer's registered sample entries.
const {prepare360RaMp4}=require(dest+'/import.cjs');
await build({entryPoints:['packages/core/src/mpegh.ts'],bundle:true,platform:'node',format:'esm',outfile:dest+'/decoder.mjs',plugins:[{name:'wasm-path',setup(b){b.onResolve({filter:/\.wasm\?url$/},a=>({path:resolve(a.resolveDir,a.path.replace('?url','')),namespace:'wasm-path'}));b.onLoad({filter:/.*/,namespace:'wasm-path'},a=>({contents:'export default '+JSON.stringify(a.path)}));}}]});
const {MpeghDecoder,initMpegh}=await import(pathToFileURL(dest+'/decoder.mjs'));await initMpegh();
function decode(input){const decoder=new MpeghDecoder(), result=[];try{for(let p=0;p<input.length;p+=317){decoder.push(input.subarray(p,p+317));for(let f;f=decoder.nextFrame();)result.push(f);}decoder.flush();return result;}finally{decoder.free();}}
const bytes=readFileSync('packages/core/mpegh/fixtures/motion.mhas');
let bit=0,start=0;const units=[],frames=[];let config;
function get(n){let v=0;for(let i=0;i<n;i++){v=v*2+((bytes[bit>>3]>>(7-(bit&7)))&1);bit++;}return v;}
function escaped(a,b,c){let v=get(a);if(v===2**a-1){const w=get(b);v+=w;if(w===2**b-1)v+=get(c);}return v;}
while(bit<bytes.length*8){const type=escaped(3,8,8);escaped(2,8,32);const size=escaped(11,24,24);assert.equal(bit%8,0);const payload=bytes.subarray(bit/8,bit/8+size);bit+=size*8;if(type===1)config=payload;if(type===2){frames.push(payload);units.push(bytes.subarray(start,bit/8));start=bit/8;}}
assert(config);assert.equal(start,bytes.length);const expected=decode(bytes);
for(const type of ['mha1','mhm1']){
 mp4.BoxParser.createSampleEntryCtor(mp4.BoxParser.SAMPLE_ENTRY_TYPE_AUDIO,type);
 const file=mp4.createFile(), box=new mp4.BoxParser.Box('mhaC');
 box.data=Uint8Array.from(Buffer.concat([Buffer.from([1,0x0b,0,config.length>>8,config.length&255]),config]));
 const id=file.addTrack({type,hdlr:'soun',timescale:48000,samplerate:48000,channel_count:2,samplesize:16,description:box});assert(id);
 const samples=type==='mha1'?frames:units;
 samples.forEach((data,i)=>file.addSample(id,Uint8Array.from(data).buffer,{duration:1024,dts:i*1024,cts:i*1024,is_sync:true}));
 const input=Buffer.from(file.getBuffer());writeFileSync(dest+'/motion-'+type+'.mp4',input);
 const output=[];let discarded=false;
 const host={async beginMp4Import(){return JSON.stringify({token:'t',size:input.length});},async readMp4Import(t,offset,count){return input.subarray(offset,offset+count).toString('base64');},async appendMp4Import(t,data){output.push(Buffer.from(data,'base64'));},async finishMp4Import(){return 'file://normalized.mhas';},async discardMp4Import(){discarded=true;}};
 const result=await prepare360RaMp4(host,'fixture',type+'.mp4');assert(result);assert(!discarded);const normalized=Buffer.concat(output);writeFileSync(dest+'/motion-'+type+'.mhas',normalized);
 if(type==='mhm1')assert.deepEqual(normalized,bytes);
 assert.deepEqual(decode(normalized),expected,'mobile MP4 extraction must preserve Windows source PCM and objects');
 assert(output.length < samples.length / 4, "native writes must be batched rather than one per audio AU");
 await result.release();assert(discarded);
 console.log(JSON.stringify({type,packets:samples.length,mp4Bytes:input.length,mhasBytes:normalized.length,nativeWrites:output.length}));
}
