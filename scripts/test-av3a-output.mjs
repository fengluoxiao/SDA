import assert from "node:assert/strict";
import {build, transformSync} from "esbuild";
import {readFileSync} from "node:fs";
const load=async file=>{const b=await build({entryPoints:[file],bundle:true,platform:"node",format:"cjs",write:false});const m={exports:{}};new Function("module","exports",b.outputFiles[0].text)(m,m.exports);return m.exports;};
const {LoudnessMeter,masterBalanceGainDb}=await load("packages/player/src/bs1770.ts");
const {AV3A_714_WEIGHTS,isAv3aBedFrame}=await load("packages/player/src/stereo-master.ts");
const labels=["L","R","C","LFE","Lb","Rb","Ls","Rs","Tfl","Tfr","Tbl","Tbr"];
const topology={codec:"av3a",labels,channels:labels.map(()=>[]),events:[],objectChannels:[]};
assert(isAv3aBedFrame(topology));
assert(!isAv3aBedFrame({...topology,codec:"eac3"}));
assert(!isAv3aBedFrame({...topology,labels:labels.map((s,i)=>i===8?"Obj_8":s)}));
assert(!isAv3aBedFrame({...topology,objectChannels:[{id:8,channel:8}]}));
const mono=Float32Array.from({length:24000},(_,i)=>0.3*Math.sin(2*Math.PI*997*i/48000));
const measure=ch=>{const m=new LoudnessMeter(48000,12,AV3A_714_WEIGHTS);m.push(labels.map((_,i)=>i===ch?mono:new Float32Array(mono.length)));return m.integrated();};
const c=measure(2),rear=measure(4),height=measure(8),lfe=measure(3);
assert(Math.abs(rear.integratedLufs-c.integratedLufs-10*Math.log10(1.41))<1e-8);
assert(Math.abs(height.integratedLufs-c.integratedLufs)<1e-8);
assert.equal(lfe.integratedLufs,null);assert(Number.isFinite(lfe.truePeakDbtp));
assert.throws(()=>new LoudnessMeter(48000,12,[1,1]),/weights/);
// Execute the actual player scheduling block and require the established
// linked master-gain policy and sample-clock scheduling.
const player=readFileSync("packages/player/src/player.ts","utf8").replace(/\r\n/g,"\n");
const constants=player.match(/const MEASURED_LOUDNESS_\w+ = [^;]+;/g).join("\n");
const block=player.slice(player.indexOf("      if (this.stereoBalanceEligible) {",player.indexOf("  private pumpPcm")),player.indexOf("\n\n      // (Re)declare bed sources"));
const method=player.match(/  private applyMeasuredLoudnessBalance\([\s\S]*?\n  }/)[0];
const code=transformSync(`${constants}\nexport class Harness {${method} process(frame:any){const renderer=this.renderer;${block}}}`,{loader:"ts",format:"cjs"}).code;
const m={exports:{}};new Function("module","exports","masterBalanceGainDb",code)(m,m.exports,masterBalanceGainDb);
function harness(){const calls=[];const p=new m.exports.Harness();Object.assign(p,{stereoBalanceEligible:true,sampleRate:48000,cachedMeasuredLufs:null,cachedMeasuredTruePeakDbtp:null,measuredLoudnessSettled:false,balancedLoudnessBlocks:0,scheduledProgramLoudnessGainDb:null,programLoudnessGainDb:0,volumeBalanceEnabled:true,trackCodec:"av3a",renderer:null,setNativeProgramGainDb:(db,at)=>calls.push({db,at})});return {p,calls};}
const frame=(at,blocks,lufs,peak)=>({...topology,samplePos:at,loudness:{blocks,integratedLufs:lufs,truePeakDbtp:peak}});
const vivid=harness(),stereo=harness();
for(const [at,blocks,lufs,peak] of [[0,0,null,3],[19200,1,-5,3],[268800,56,-5,3],[273600,57,-5,3],[513600,107,-1,3],[518400,108,-6,3],[753600,157,-28,-20],[993600,207,-5,3]]) {
 vivid.p.process(frame(at,blocks,lufs,peak));
 stereo.p.process({...frame(at,blocks,lufs,peak),codec:"alac"});
 assert.deepEqual(vivid.calls,stereo.calls,"AV3A must use the existing stereo balance schedule and gain policy");
}
assert(vivid.calls.length>0);assert(vivid.calls.every(c=>c.db<=0));
const quiet=harness();quiet.p.process(frame(273600,57,-35,-20));assert.equal(quiet.p.programLoudnessGainDb,0);
const swift=readFileSync("apps/mobile/modules/sda-core/ios/SystemSpatial360.swift","utf8"),host=readFileSync("apps/mobile/modules/sda-core/ios/SdaPlayer.swift","utf8");
assert.match(swift,/sda_ios_av3a_speakers_create/);assert.match(swift,/requiresBalancePreanalysis: Bool \{ sourceCodec != "av3a" \}/);
assert.match(host,/\["mhas", "av3a"\]\.contains\(ext\)/);assert.match(host,/system\.requiresBalancePreanalysis, system\.balanceEnabled/);
assert.match(host,/if !input\.isAv3a, !hash\.isEmpty, let measurement/);
assert.match(swift,/"sourceCodec":sourceCodec/);assert.match(swift,/allowedAudioSpatializationFormats/);
assert.match(readFileSync("apps/mobile/src/IOSNativeSettings.tsx","utf8"),/360RA／Audio Vivid/);
console.log("Audio Vivid balance/output: 12ch weights, LFE exclusion, same established attenuation/scheduling, no boost, shared Apple route and no AV3A pre-scan/cache passed.");
// Exercise the real worker path: AV3A must report progressive measurements
// through the established decoder/worker transport, with no new resampler.
const {FrameBatcher}=await load("packages/player/src/frame-batcher.ts");
const messages=[];
class Decoder {
  codec="av3a"; queue=[]; at=0;
  push(){const start=this.at;this.at+=1024;this.queue.push({...topology,sampleRate:48000,samplePos:start,rawBedLabels:labels,programLoudness:null,rampDuration:0,channels:labels.map((_,ch)=>Float32Array.from({length:1024},(_,i)=>0.4*Math.sin(2*Math.PI*997*(start+i)/48000+ch*0.03)))});}
  nextFrame(){return this.queue.shift();} drainErrors(){return [];} flush(){} free(){this.queue=[];}
}
const nativeCore={SdaDecoder:Decoder,initCore:async()=>{},configureDecoderDiagnostics(){},drainDecoderCheckpoints:()=>({events:[],dropped:0}),decoderDiagnosticBuild:{}};
const workerSource=await build({entryPoints:["packages/player/src/decoder.worker.ts"],bundle:true,platform:"node",format:"cjs",write:false,external:["*"]});
const api={postMessage:message=>messages.push(message)};
new Function("require","module","exports","self",workerSource.outputFiles[0].text)(name=>{
 if(name==="@sda/core")return nativeCore;
 if(name==="@sda/demux")return {sniffContainer:()=>"raw",createDemuxer:(_kind,cb)=>({push:bytes=>cb.onPacket({timestampMs:0,frames:[bytes]}),flush(){}})};
 if(name.includes("performance-sink"))return {PerformanceSink:class{endpoint=null;record(){}trace(){}},connectPerformanceWorker(){}};
 if(name.includes("decoder-diagnostics"))return {DecoderDiagnostics:class{record(){}reset(){}}};
 if(name.includes("stereo-master"))return {isAv3aBedFrame,AV3A_714_WEIGHTS,isStereoMasterFrame:()=>false};
 if(name.includes("bs1770"))return {LoudnessMeter};
 if(name.includes("frame-batcher"))return {FrameBatcher};
 if(name.includes("alac-resampler"))return {AlacResampler:class{constructor(){throw Error("AV3A was already resampled");}}};
 if(name.includes("mpegh.js"))return {initMpegh:async()=>{},isMhas:()=>false};
 if(name.includes("iamf.js"))return {initIamf:async()=>{},isIamf:()=>false};
 if(name.includes("control.js"))return {canCoalesceObjectEvent:()=>false};
 throw Error(name);
},{exports:{}},{},api);
const tick=()=>new Promise(resolve=>setImmediate(resolve));
async function send(data){api.onmessage({data});await tick();const errors=messages.filter(m=>m.type==="error");assert.deepEqual(errors,[]);}
await send({type:"open",epoch:1,codec:"av3a",outputSampleRate:48000});
for(let i=0;i<30;i++)await send({type:"push",epoch:1,kind:"raw",sequence:i,chunk:new Uint8Array(16).buffer});
const firstEstimate=messages.find(m=>m.type==="frame"&&m.frame.loudness?.integratedLufs!=null);
assert(firstEstimate);assert(firstEstimate.frame.loudness.blocks>=1);
await send({type:"flush",epoch:1});assert(messages.some(m=>m.type==="loudness-complete"));
console.log("Audio Vivid worker: actual open/push/flush path, existing loudness reporting and no extra SRC passed.");

await import("./test-av3a-startup.mjs");

// The reference is supplied by the shared decoder, independent of object count.
const reference={integratedLufs:-10,blocks:100,truePeakDbtp:-2};
const oldPush=Decoder.prototype.push;
Decoder.prototype.push=function(){oldPush.call(this);const f=this.queue.at(-1);f.codec="av3a";f.labels=f.labels.map((_,i)=>`Obj_${i}`);f.objectChannels=f.labels.map((_,i)=>({id:i,channel:i}));f.referenceLoudness={...reference};};
messages.length=0;
await send({type:"open",epoch:2,codec:"av3a",outputSampleRate:48000});
await send({type:"push",epoch:2,kind:"raw",sequence:0,chunk:new Uint8Array(16).buffer});
await send({type:"flush",epoch:2});
const objectFrame=messages.find(m=>m.type==="frame").frame;
assert.deepEqual(objectFrame.loudness,reference);
assert.deepEqual(messages.find(m=>m.type==="loudness-complete").loudness,reference);
const handle=player.slice(player.indexOf("    const stereoMaster = isStereoMasterFrame(frame);",player.indexOf("  private handleFrame")),player.indexOf("    if (frame.programLoudness)",player.indexOf("  private handleFrame")));
const eligibility=transformSync(`export function run(frame:any){const isStereoMasterFrame=()=>false,isAlacStereoFrame=()=>false;${handle}}`,{loader:"ts",format:"cjs"}).code;
const e={exports:{}};new Function("module","exports","isAv3aBedFrame",eligibility)(e,e.exports,isAv3aBedFrame);
for(const codec of ["av3a","eac3","future-object-format"]){
 const state={nonStereoProgrammeSeen:false,stereoBalanceEligible:false,recordDecode(){},setVolumeBalance(){this.notified=true;}};
 e.exports.run.call(state,{...objectFrame,codec});assert(state.stereoBalanceEligible && state.notified);
}
console.log("Shared object reference: worker and player carry the decoded measurement, activate programme balance, and retain it at EOF.");

// A compatible stereo listening frame still uses the decoder's full-programme
// reference. Re-estimating that listening downmix would replace the reference.
Decoder.prototype.push=function(){oldPush.call(this);const f=this.queue.at(-1);f.codec="eac3";f.channels=f.channels.slice(0,2);f.labels=["L","R"];f.objectChannels=[];f.referenceLoudness={...reference};};
const oldMeterPush=LoudnessMeter.prototype.push;
LoudnessMeter.prototype.push=function(){throw Error("decoded programme reference was measured twice");};
try {
 messages.length=0;await send({type:"open",epoch:3,codec:"auto",outputSampleRate:48000});
 await send({type:"push",epoch:3,kind:"raw",sequence:0,chunk:new Uint8Array(16).buffer});
 await send({type:"flush",epoch:3});
 assert.deepEqual(messages.find(m=>m.type==="frame").frame.loudness,reference);
} finally {LoudnessMeter.prototype.push=oldMeterPush;}
console.log("Decoded reference retains priority over a stereo compatibility listening frame.");
