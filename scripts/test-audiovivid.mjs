import assert from 'node:assert/strict';
import {build} from 'esbuild';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
const require=createRequire(import.meta.url),mp4=require('mp4box');
const bundle=await build({entryPoints:['apps/mobile/src/mpeghMp4.ts'],bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};
new Function('require','module','exports',bundle.outputFiles[0].text)(require,module,module.exports);
const {prepare360RaMp4}=module.exports;
const frames=readFileSync('packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a');
const file=mp4.createFile();
const id=file.addTrack({type:'ec-3',hdlr:'soun',timescale:44100,samplerate:44100,channel_count:12,samplesize:16});
for(let i=0;i<3;i++)file.addSample(id,Uint8Array.from(frames.subarray(i*2415,(i+1)*2415)).buffer,{duration:1024,dts:i*1024,cts:i*1024,is_sync:true});
file.getTrackById(id).mdia.mdhd.duration=3*1024;
const synthetic=Buffer.from(file.getBuffer());
const codecOffset=synthetic.indexOf(Buffer.from('ec-3'));assert(codecOffset>0);synthetic.write('av3a',codecOffset);
async function verify(input,expectedSamples,expectedBytes){
 const output=[],stages=[];let finished=0,discarded=0;
 const host={
  async beginMp4Import(){return JSON.stringify({token:'vivid',size:input.length})},
  async readMp4Import(token,offset,count){assert.equal(token,'vivid');assert(count<=262144);return input.subarray(offset,offset+count).toString('base64')},
  async appendMp4Import(token,bytes){const block=Buffer.from(bytes,'base64');assert(block.length<=262144);output.push(block)},
  async finishMp4Import(){finished++;return 'file://vivid.mhas'},
  async discardMp4Import(){discarded++},
 };
 const result=await prepare360RaMp4(host,'input','song.m4a',p=>stages.push(p));
 assert(result);assert.equal(result.name,'song.m4a.av3a');assert.equal(finished,1);assert.equal(discarded,0);
 assert(Math.abs(result.durationMs-expectedSamples/44100*1000)<1);
 const raw=Buffer.concat(output);if(expectedBytes)assert.deepEqual(raw,expectedBytes);
 assert.equal(raw.length,expectedSamples/1024*2415,'every original access unit is retained exactly once');
 assert(stages.some(p=>p.stage==='提取 Audio Vivid 音轨'));assert.equal(stages.at(-1).fraction,1);
 await result.release();assert.equal(discarded,1);
 return raw;
}
await verify(synthetic,3*1024,frames);
let realRaw;
if(process.env.SDA_AV3A_INPUT){const input=readFileSync(process.env.SDA_AV3A_INPUT);realRaw=await verify(input,10111*1024);console.log('Real Audio Vivid M4A: all 10,111 access units extracted with 12-channel metadata and exact duration.');}
if(process.env.SDA_AV3A_WASM){
 const wasm=require(process.env.SDA_AV3A_WASM);
 const input=realRaw??frames;
 const limit=Number(process.env.SDA_AV3A_FRAME_LIMIT??input.length/2415);
 const raw=input.subarray(0,limit*2415);
 const decoder=new wasm.SdaDecoder('av3a');
 const reference=process.env.SDA_AV3A_EXPECTED_PCM?readFileSync(process.env.SDA_AV3A_EXPECTED_PCM):null;
 let samples=0,peak=0,maxError=0,sumError=0,refOffset=0;
 const energy=Array(12).fill(0),started=performance.now();
 const drain=()=>{
  assert.deepEqual(decoder.drainErrors(),[]);
  let frame;
  while(frame=decoder.nextFrame()){
   assert.equal(frame.codec,'av3a');assert.equal(frame.sampleRate,48000);assert.equal(frame.samplePos,samples);
   assert.deepEqual(frame.labels,['L','R','C','LFE','Lb','Rb','Ls','Rs','Tfl','Tfr','Tbl','Tbr']);
   assert.deepEqual(JSON.parse(frame.eventsJson),[]);assert.deepEqual(JSON.parse(frame.objectChannelsJson),[]);
   const channels=Array.from({length:12},(_,i)=>frame.channel(i));
   for(let i=0;i<frame.samplesPerChannel;i++)for(let ch=0;ch<12;ch++){
    const sample=channels[ch][i];assert(Number.isFinite(sample));peak=Math.max(peak,Math.abs(sample));energy[ch]+=sample*sample;
    if(reference){const diff=sample-reference.readFloatLE(refOffset);maxError=Math.max(maxError,Math.abs(diff));sumError+=diff*diff;refOffset+=4;}
   }
   samples+=frame.samplesPerChannel;frame.free();
  }
 };
 for(let offset=0;offset<raw.length;offset+=2415*8){decoder.push(raw.subarray(offset,offset+2415*8));drain();}
 decoder.flush();drain();
 assert.equal(samples,Math.round(limit*1024*48000/44100));
 if(reference){assert.equal(refOffset,reference.length);assert(maxError<0.0001,`native/WASM PCM difference ${maxError}`);}
 decoder.free();
 console.log(JSON.stringify({scope:'actual Audio Vivid WASM pipeline',frames:limit,samples,sampleRate:48000,seconds:samples/48000,peak,rms:energy.map(e=>Math.sqrt(e/samples)),maxNativeDifference:reference?maxError:null,rmsNativeDifference:reference?Math.sqrt(sumError/(samples*12)):null,elapsedSeconds:(performance.now()-started)/1000}));
}

const read=p=>readFileSync(p,'utf8');
assert.match(read('apps/mobile/modules/sda-core/ios/CompressedInput.swift'),/"av3a"\]\.contains\(ext\)/);
assert.match(read('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/Eac3Input.kt'),/"av3a" ->/);
assert.match(read('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt'),/equals\("av3a", ignoreCase = true\)\) "av3a"/);
assert.match(read('apps/web/src/auto-layout.ts'),/"av3a"/);
assert.match(read('packages/player/src/decoder.worker.ts'),/t.codec === "av3a" \? "av3a" : "ac4"/);
assert.match(read('apps/mobile/src/IOSPlayer.tsx'),/sourceCodec === "av3a" \? "Audio Vivid"/);
console.log('Audio Vivid import: unknown MP4 sample entry, untouched AU bytes, bounded I/O, cleanup, native entry points, WASM routing and Dolby layout selection passed.');
