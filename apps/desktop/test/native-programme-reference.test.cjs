'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const os=require('node:os');
const path=require('node:path');
const {EventEmitter}=require('node:events');
const {createStereoLoudness,validMeasurement}=require('../stereo-loudness.cjs');
const executable=process.env.SDA_NATIVE_PLAYER_TEST;
const root=path.resolve(__dirname,'../../..');
class Owner extends EventEmitter {isDestroyed(){return false;}send(){}}
function service(){const handlers=new Map();const api=createStereoLoudness({ipcMain:{handle:(id,fn)=>handlers.set(id,fn)},nativeExecutable:()=>executable,workerPath:path.join(os.tmpdir(),'must-not-start-wasm-worker.cjs'),corePath:'unused'});return{api,analyze:(owner,file)=>handlers.get('sda:stereo-loudness')({sender:owner},file),cancel:owner=>handlers.get('sda:stereo-loudness-cancel')({sender:owner})};}
test('native full-source reference preserves codec, sample clock and high-rate ALAC without a WASM Worker',{skip:!executable},async()=>{
 const s=service(),owner=new Owner();try{for(const [file,rate]of[['tone-48000-2ch.m4a',48000],['tone-96000-2ch-entry1.m4a',96000],['tone-44100-2ch.mp3',44100]]){
  const result=await s.analyze(owner,path.join(root,'crates/sda-native/tests/fixtures/alac',file));assert(validMeasurement(result));assert.equal(result.complete,true);assert.equal(result.sampleRate,rate);assert.equal(result.nativeReference.sampleRate,48000);assert.equal(result.nativeReference.policy,'fresh-complete-stereo-reference-v1');assert(result.frames>0);assert.equal(result.channels,2);
 }}finally{await s.api.close();}
});
test('native Audio Vivid MP4 view measures the original sample bytes without extracting an audio file',{skip:!executable},async()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sda-native-vivid-reference-'));const s=service(),owner=new Owner();
 try{const rawPath=path.join(root,'packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a');const bytes=fs.readFileSync(rawPath);const mp4=require('mp4box').createFile();const id=mp4.addTrack({type:'ec-3',hdlr:'soun',timescale:44100,samplerate:44100,channel_count:12,samplesize:16});
  for(let i=0;i<3;i++)mp4.addSample(id,Uint8Array.from(bytes.subarray(i*2415,(i+1)*2415)).buffer,{duration:1024,dts:i*1024,cts:i*1024,is_sync:true});mp4.getTrackById(id).mdia.mdhd.duration=3072;
  const container=Buffer.from(mp4.getBuffer());const at=container.indexOf(Buffer.from('ec-3'));assert(at>0);container.write('av3a',at);const file=path.join(dir,'vivid.m4a');fs.writeFileSync(file,container);
  const before=fs.readdirSync(dir);const raw=await s.analyze(owner,rawPath);const wrapped=await s.analyze(owner,file);assert(validMeasurement(raw));assert(validMeasurement(wrapped));assert.equal(raw.codec,'av3a');assert.equal(raw.channels,12);assert.equal(wrapped.frames,raw.frames);assert.equal(wrapped.integratedLufs,raw.integratedLufs);assert.equal(wrapped.truePeakDbtp,raw.truePeakDbtp);assert.deepEqual(fs.readdirSync(dir),before);
 }finally{await s.api.close();fs.rmSync(dir,{recursive:true,force:true});}
});
test('native reference cancellation is owner-scoped and closes the child before shutdown completes',{skip:!executable},async()=>{
 const s=service(),owner=new Owner(),other=new Owner();try{const first=s.analyze(owner,path.join(root,'crates/sda-native/tests/fixtures/loudness/quiet-intro-96000.m4a'));assert.equal(s.cancel(other),false);assert.equal(s.cancel(owner),true);await assert.rejects(first,/取消/);const next=await s.analyze(owner,path.join(root,'crates/sda-native/tests/fixtures/alac/tone-48000-2ch.m4a'));assert(validMeasurement(next));assert.equal(await s.analyze(owner,path.join(root,'crates/sda-native/tests/fixtures/ios-stereo-tones.eac3')),null);}finally{await s.api.close();}
});
