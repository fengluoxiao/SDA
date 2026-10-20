// Verify the actual TypeScript source adapter independently from codec fixtures.
import assert from 'node:assert/strict';
import {build} from 'esbuild';
import {mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
const folder=resolve('tmp/mpegh-source-routing');mkdirSync(folder,{recursive:true});
const file=resolve(folder,'adapter.mjs');
await build({entryPoints:['packages/core/src/mpegh.ts'],bundle:true,platform:'node',format:'esm',outfile:file,plugins:[{name:'isolated-capture-fixture',setup(b){
 b.onResolve({filter:/mpegh\.js$/},()=>({path:'capture-runtime',namespace:'source-fixture'}));
 b.onResolve({filter:/\.wasm\?url$/},()=>({path:'capture-url',namespace:'source-fixture'}));
 b.onLoad({filter:/.*/,namespace:'source-fixture'},a=>({contents:a.path==='capture-runtime'?'export default async()=>globalThis.__sdaSourceFixture;':'export default "fixture.wasm";'}));
}}]});
const buffer=new ArrayBuffer(65536);const floats=new Float32Array(buffer);const bytes=new Uint8Array(buffer);
let count=128,frames=16,captureError=0,consumed=0;
globalThis.__sdaSourceFixture={HEAPF32:floats,HEAPU8:bytes,_sda_open:()=>0,_sda_close:()=>{},_sda_capacity:()=>1024,_sda_input:()=>0,_sda_decode:n=>{consumed=n;return 0;},
 _sda_pcm:()=>4096,_sda_metadata:()=>16384,_sda_bed:()=>3,
 _sda_info:k=>({0:consumed,2:48000,3:frames,4:count,5:count-1,6:1,7:0,9:count-1,12:captureError}[k]??0)};
const {MpeghDecoder,initMpegh,mhasPacket}=await import(pathToFileURL(file));await initMpegh();
for(const total of [24,64,128]){
 count=total;frames=16;captureError=0;
 for(let lane=0;lane<count;lane++)for(let s=0;s<frames;s++)floats[1024+lane*frames+s]=(lane+1)/256+s/4096;
 for(let object=0;object<count-1;object++)floats.set([object,0,0,0,1,1,0,0,0,0,0,1],4096+object*12);
 const decoder=new MpeghDecoder();decoder.push(mhasPacket(2,new Uint8Array([0])));const frame=decoder.nextFrame();
 assert.equal(frame.channels.length,count);assert.equal(frame.objectChannels.length,count-1);assert.equal(frame.events.length,count-1);
 for(let lane=0;lane<count;lane++)for(let s=0;s<frames;s++)assert.equal(frame.channels[lane][s],(lane+1)/256+s/4096);
 assert.equal(frame.objectChannels.at(-1).channel,count-1);decoder.free();
}
for(const mode of ['overflow-with-no-frame','too-many-sources']){
 count=129;frames=mode==='overflow-with-no-frame'?0:16;captureError=mode==='overflow-with-no-frame'?1:0;
 const decoder=new MpeghDecoder();assert.throws(()=>decoder.push(mhasPacket(2,new Uint8Array([0]))),/source capture/);decoder.free();
}
delete globalThis.__sdaSourceFixture;
console.log('MPEG-H TypeScript source path: 24/64/128 PCM and metadata preserved; 129 and zero-frame capture errors rejected.');
