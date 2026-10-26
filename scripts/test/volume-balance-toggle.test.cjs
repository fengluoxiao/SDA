const assert = require('node:assert/strict');
const fs = require('node:fs');
const {buildSync,transformSync} = require('esbuild');
const source = fs.readFileSync('packages/player/src/player.ts','utf8').replace(/\r\n/g,'\n');
function exportsOf(file){const m={exports:{}};const code=buildSync({entryPoints:[file],bundle:true,platform:'node',format:'cjs',write:false}).outputFiles[0].text;new Function('module','exports',code)(m,m.exports);return m.exports;}
const {masterBalanceGainDb}=exportsOf('packages/player/src/bs1770.ts');
const {av3aStartupDecision}=exportsOf('packages/player/src/av3a-startup.ts');
const constants=source.match(/const MEASURED_LOUDNESS_\w+ = [^;]+;/g).join('\n');
const method=name=>source.match(new RegExp('  (?:private )?'+name+'\\([^]*?\\n  }'))[0];
const start=source.indexOf('      if (this.stereoBalanceEligible) {',source.indexOf('  private pumpPcm'));
const block=source.slice(start,source.indexOf('\n\n      // (Re)declare bed sources',start));
const methods=['observeMeasuredLoudness','prepareAv3aStartup','applyMeasuredLoudnessBalance','setVolumeBalance','setMeasuredLoudness'].map(method).join('\n');
const code=transformSync(constants+'\nexport class Harness {'+methods+'\n process(frame:any){const renderer=this.renderer;'+block+'} av3aStartupPending(){return !this.av3aStartupPrepared;} aheadSeconds(){return 7;} pumpPcm(){} }',{loader:'ts',format:'cjs'}).code;
const m={exports:{}};new Function('module','exports','masterBalanceGainDb','av3aStartupDecision',code)(m,m.exports,masterBalanceGainDb,av3aStartupDecision);
function make(codec,enabled){const p=new m.exports.Harness();p.calls=[];Object.assign(p,{id:0,trackCodec:codec,mpeghReference:codec==='mpegh',sampleRate:48000,renderer:null,volumeBalanceEnabled:enabled,stereoBalanceEligible:true,startupMeasuredLoudness:null,measuredLoudness:null,cachedMeasuredLufs:null,cachedMeasuredTruePeakDbtp:null,measuredLoudnessSettled:false,av3aStartupPrepared:false,balancedLoudnessBlocks:0,scheduledProgramLoudnessGainDb:null,startupOrigin:0,pcmQueue:[],ended:false,setNativeProgramGainDb(gainDb,at){this.calls.push({gainDb,at});}});return p;}
const measurement=(blocks,integratedLufs,truePeakDbtp=-1.5)=>({blocks,integratedLufs,truePeakDbtp});
const frame=(codec,loudness)=>({codec,samplePos:loudness.blocks*4800,loudness,referenceLoudness:loudness});
for(const codec of ['alac','av3a','eac3','mpegh']){
  const startup=make(codec,true),live=make(codec,false),delayed=make(codec,false),minimum=codec==='alac'?7:57;
  for(const [blocks,lufs] of [[minimum-1,-15],[minimum,-14.75],[minimum+4,-14.5]]){
    const value=measurement(blocks,lufs);
    for(const p of [startup,live,delayed])p.observeMeasuredLoudness(value);
    live.process(frame(codec,value));
  }
  assert(startup.prepareAv3aStartup());delayed.process(frame(codec,measurement(minimum+4,-14.5)));
  for(const p of [startup,live,delayed]){assert.equal(p.programLoudnessGainDb,-3.25,codec+' must use the same first valid reference');assert.equal(p.balancedLoudnessBlocks,minimum,'later updates must share the same reference clock');}
  for(const p of [startup,live,delayed]){
    const reference=p.startupMeasuredLoudness,calls=p.calls.length;
    for(const enabled of [false,true,false,true])p.setVolumeBalance(enabled);
    assert.equal(p.programLoudnessGainDb,-3.25);assert.equal(p.startupMeasuredLoudness,reference);assert.equal(p.calls.length,calls,'toggles must not reschedule or remeasure the programme');
  }
  const peak=measurement(minimum,-14.75,6),a=make(codec,true),b=make(codec,false);
  a.observeMeasuredLoudness(peak);b.observeMeasuredLoudness(peak);a.prepareAv3aStartup();b.process(frame(codec,peak));
  assert.equal(a.programLoudnessGainDb,b.programLoudnessGainDb,'startup/live peak policy must agree');assert.equal(a.programLoudnessGainDb,codec==='mpegh'?-3.25:-7);
  const cached=make(codec,false);cached.cachedMeasuredLufs=-12;cached.cachedMeasuredTruePeakDbtp=-1.5;cached.observeMeasuredLoudness(measurement(57,-14.75));cached.process(frame(codec,measurement(57,-14.75)));assert.equal(cached.programLoudnessGainDb,-6,'a complete cached reference retains precedence');
  for(const p of [startup,live,delayed]){p.process(frame(codec,measurement(minimum+50,-10)));assert.equal(p.programLoudnessGainDb,-3.25,'later passages must not retarget the selected programme gain');const calls=p.calls.length;p.setMeasuredLoudness(-2,-1);p.process(frame(codec,measurement(minimum+100,-3)));assert.equal(p.calls.length,calls,'late cache must not replace an active reference');assert.equal(p.programLoudnessGainDb,-3.25);}
}
console.log('Volume balance toggle parity passed: first-ready reference, delayed preroll, live bypass/re-enable, cached precedence and codec-specific peak policy.');

// Execute the production output worklet, not only its scheduling harness.
let PeakGuard;
class WorkletBase {constructor(){this.port={onmessage:null,postMessage(){}};}}
new Function('AudioWorkletProcessor','registerProcessor','sampleRate',fs.readFileSync('packages/renderer/worklet/sda-renderer.worklet.js','utf8'))(WorkletBase,(name,type)=>{if(name==='sda-final-peak-guard')PeakGuard=type;},48000);
const continuous=new PeakGuard(),toggled=new PeakGuard();
for(const guard of [continuous,toggled]){guard.onMessage({type:'programGain',gain:.5});guard.onMessage({type:'programEnabled',enabled:true});guard.onMessage({type:'start',origin:0});}
for(let at=0;at<24000;at+=128){
  if(at===1024||at===4096)toggled.onMessage({type:'programEnabled',enabled:false});
  if(at===3072||at===4224)toggled.onMessage({type:'programEnabled',enabled:true});
  const left=Float32Array.from({length:128},(_,i)=>Math.sin((at+i)*.13)*.003),right=Float32Array.from(left,x=>-x*.4);
  const output=guard=>{const result=[new Float32Array(128),new Float32Array(128)];guard.process([[left,right]],[result]);return result;};
  const a=output(continuous),b=output(toggled);
  if(at>=8192){assert.deepEqual(a[0],b[0],'settled output must match continuous balance sample-for-sample');assert.deepEqual(a[1],b[1]);}
}
assert.equal(toggled.programMetadataGain,.5);
console.log('Production output worklet: off/on round trips preserve reference gain and match continuous playback after the existing click-free ramp.');
