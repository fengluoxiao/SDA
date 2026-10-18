import assert from 'node:assert/strict';
import {build,transformSync} from 'esbuild';
import {readFileSync} from 'node:fs';
const b=await build({entryPoints:['packages/player/src/av3a-startup.ts'],bundle:true,platform:'node',format:'cjs',write:false});
const mod={exports:{}};new Function('module','exports',b.outputFiles[0].text)(mod,mod.exports);
const {av3aStartupDecision,AV3A_STARTUP_MAX_SECONDS}=mod.exports;
const m=(blocks,lufs=-5)=>({blocks,integratedLufs:lufs,truePeakDbtp:3});
assert.deepEqual(av3aStartupDecision(m(56),5.9,false),{ready:false,gainDb:null});
assert.deepEqual(av3aStartupDecision(m(57),6,false),{ready:true,gainDb:-13});
assert.deepEqual(av3aStartupDecision(m(57,-35),6,false),{ready:true,gainDb:-4}); // true-peak protection still wins
assert.deepEqual(av3aStartupDecision({...m(57,-35),truePeakDbtp:-20},6,false),{ready:true,gainDb:0});
assert.deepEqual(av3aStartupDecision(m(7),1,true),{ready:true,gainDb:-13});
assert.deepEqual(av3aStartupDecision(null,7,false),{ready:true,gainDb:null});
assert.equal(AV3A_STARTUP_MAX_SECONDS,7);
const source=readFileSync('packages/player/src/player.ts','utf8');
const names=['av3aStartupPending','prepareAv3aStartup','pace','startPlaybackIfReady'];
const methods=names.map(name=>{
 const expression=new RegExp(`  private (?:async )?${name}\\([^]*?\\n  }`);
 const match=source.match(expression);assert(match,name);return match[0];
}).join('\n');
const code=transformSync(`const STARTUP_AHEAD_SECONDS=0.5; export class Harness {${methods}}`,{loader:'ts',format:'cjs'}).code;
const hm={exports:{}};new Function('module','exports','av3aStartupDecision','AV3A_STARTUP_MAX_SECONDS',code)(hm,hm.exports,av3aStartupDecision,AV3A_STARTUP_MAX_SECONDS);
const calls=[];const p=new hm.exports.Harness();
Object.assign(p,{trackCodec:'av3a',volumeBalanceEnabled:true,stereoBalanceEligible:true,playbackStarted:false,av3aStartupPrepared:false,cachedMeasuredLufs:null,
 measuredLoudness:m(56),pcmQueue:[{samplePos:96000}],startupOrigin:null,aheadSeconds:()=>5.9,
 renderer:{setProgramLoudnessGainDb:(db,at)=>calls.push(['web',db,at]),maxBufferedSeconds:()=>5,startAt:at=>calls.push(['start',at])},
 setNativeProgramGainDb:(db,at)=>calls.push(['native',db,at]),ended:false,
 disposed:false,decodeEpoch:1,targetAheadSeconds:()=>4,initialRendererReady:true,nativeStartPending:false,
 startupAcceptedEnd:120000,sampleRate:48000,pausedState:false,outputBackend:'web-audio',cb:{onPlaybackReady(){}}});
assert.equal(p.prepareAv3aStartup(),false);assert.deepEqual(calls,[]);
// Decoder pacing must reach six seconds even though the worklet ring is smaller.
p.aheadSeconds=()=>6.2;await p.pace(1);
p.measuredLoudness=m(57);assert.equal(p.prepareAv3aStartup(),true);
assert.deepEqual(calls,[['web',-13,96000],['native',-13,96000]]);
p.startupOrigin=96000;p.startPlaybackIfReady();assert.equal(calls.at(-1)[0],'start');
assert.equal(p.measuredLoudnessSettled,true);
// Silence is bounded; an EOF clip shorter than the normal 0.5s prebuffer starts.
p.playbackStarted=false;p.av3aStartupPrepared=false;p.measuredLoudness=null;p.aheadSeconds=()=>7;
assert.equal(p.prepareAv3aStartup(),true);
p.ended=true;p.startupAcceptedEnd=97000;p.startPlaybackIfReady();assert.equal(p.playbackStarted,true);
assert.match(source,/this\.ended = true;\s*this\.pumpPcm\(\)/);
const rust=readFileSync('crates/sda-native/src/balance.rs','utf8');assert.match(rust,/AV3A_STARTUP_MAX_SECONDS: u64 = 7/);
const ios=readFileSync('apps/mobile/modules/sda-core/ios/SystemSpatial360.swift','utf8');
assert.match(ios,/queued < 7\*48000/);assert.match(ios,/requiresBalancePreanalysis: Bool \{ sourceCodec != "av3a" \}/);
const android=readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt','utf8');assert.match(android,/!preparingBalance && \(decoded - consumed/);
console.log('Audio Vivid startup: unchanged target, initial gain before PCM/start, bounded silent preroll, short EOF, and decoder pacing passed.');

// The same bounded first-output gate applies to verified stereo masters.
for (const codec of ['alac','aac','mp3','flac','pcm']) {
 calls.length=0;
 Object.assign(p,{trackCodec:codec,stereoStartupEligible:true,volumeBalanceEnabled:true,stereoBalanceEligible:true,
 playbackStarted:false,av3aStartupPrepared:false,cachedMeasuredLufs:null,measuredLoudness:m(56),
 startupOrigin:96000,ended:false,aheadSeconds:()=>5.9});
 assert.equal(p.prepareAv3aStartup(),false,codec);assert.deepEqual(calls,[]);
 p.measuredLoudness=m(57);p.aheadSeconds=()=>6.2;
 assert.equal(p.prepareAv3aStartup(),true,codec);assert.deepEqual(calls,[['web',-13,96000],['native',-13,96000]]);
 p.av3aStartupPrepared=false;p.cachedMeasuredLufs=-10;assert.equal(p.av3aStartupPending(),false,'cache skips startup gate');
 p.cachedMeasuredLufs=null;p.volumeBalanceEnabled=false;assert.equal(p.av3aStartupPending(),false,'disabled balance starts immediately');
}
Object.assign(p,{trackCodec:'mpegh',volumeBalanceEnabled:true,stereoStartupEligible:false,av3aStartupPrepared:false});
assert.equal(p.av3aStartupPending(),false,'object reference is not a stereo master');
assert.match(source,/stereoStartupEligible = frame.codec !== "mpegh" && stereoMaster/);
console.log('Stereo startup: shared first-output gain, cache/disabled bypass and object exclusion passed.');
