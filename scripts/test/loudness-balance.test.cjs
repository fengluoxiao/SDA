const assert=require('node:assert/strict'),fs=require('fs'),{transformSync}=require('esbuild');
const player=fs.readFileSync('packages/player/src/player.ts','utf8').replace(/\r\n/g,'\n'),app=fs.readFileSync('apps/web/src/App.tsx','utf8');
const js=ts=>transformSync(ts,{loader:'ts',format:'cjs'}).code;
const {buildSync}=require('esbuild');
buildSync({entryPoints:['packages/player/src/bs1770.ts'],bundle:true,platform:'node',format:'cjs',outfile:'tmp/master-balance.cjs'});
const {masterBalanceGainDb,LoudnessMeter}=require('../../tmp/master-balance.cjs');
assert.equal(masterBalanceGainDb(-10,-1.5),-8);
assert.equal(masterBalanceGainDb(-23,-12),5);
assert.equal(masterBalanceGainDb(-23,-3),2);
assert.equal(masterBalanceGainDb(NaN,-12),0);
const constants=player.match(/const MEASURED_LOUDNESS_\w+ = [^;]+;/g).join('\n');
const method=player.match(/  private applyMeasuredLoudnessBalance\([\s\S]*?\n  }/)[0];
const block=player.slice(player.indexOf('      if (this.stereoBalanceEligible) {',player.indexOf('  private pumpPcm')),player.indexOf('\n\n      // (Re)declare bed sources'));
const Harness=new Function("masterBalanceGainDb",js(`${constants}\nclass Harness {${method} process(frame:any){ const renderer=this.renderer; ${block.replace(/^else /,'')} }}\nexports.Harness=Harness;`).replace('exports.Harness = Harness;','return Harness;'))(masterBalanceGainDb);
const calls=[],p=new Harness();Object.assign(p,{stereoBalanceEligible:true,sampleRate:48000,cachedMeasuredLufs:null,measuredLoudnessSettled:false,balancedLoudnessBlocks:0,scheduledProgramLoudnessGainDb:null,renderer:null,setNativeProgramGainDb:(gain,at)=>calls.push({gain,at})});
const frame=(blocks,lufs)=>({codec:'alac',channels:[[],[]],labels:['L','R'],samplePos:blocks*4800,loudness:{blocks,integratedLufs:lufs,peakDbfs:-12}});
p.process(frame(56,-17));assert.equal(calls.length,0);p.process(frame(57,-17.28));assert(Math.abs(calls.at(-1).gain+.72)<1e-6);p.process(frame(107,-10.66));assert(Math.abs(calls.at(-1).gain+7.34)<1e-6);const n=calls.length;p.process(frame(108,-8));assert.equal(calls.length,n);p.process(frame(157,-22));assert.equal(calls.at(-1).gain,4);
p.cachedMeasuredLufs=-10.66;p.measuredLoudnessSettled=false;p.process(frame(0,-30));assert(Math.abs(calls.at(-1).gain+7.34)<1e-6);assert.equal(calls.at(-1).at,0);const cachedCalls=calls.length;p.process(frame(200,-5));assert.equal(calls.length,cachedCalls);
const keyCode=app.match(/function measuredLoudnessStorageKey\([\s\S]*?\n}/)[0];const key=new Function(js(keyCode)+';return measuredLoudnessStorageKey')();assert.notEqual(key({channels:2,sampleRate:48000},{kind:'path',path:'a.m4a'}),key({channels:2,sampleRate:48000},{kind:'path',path:'b.m4a'}));
const read=app.match(/const stored = localStorage.getItem\(key\);[\s\S]*?createdPlayer\?\.setMeasuredLoudness\([^\n]+/)[0];for(const v of [null,'',' ','invalid','-10.66',JSON.stringify({integratedLufs:-23,peakDbfs:-12})]){let actual;try{new Function('localStorage','key','createdPlayer',read)({getItem:()=>v},'test',{setMeasuredLoudness:(...x)=>actual=x})}catch{actual=[null,null]}assert.deepEqual(actual,v?.startsWith('{')?[-23,-12]:[null,null])}
console.log('Loudness regression: missing cache, file isolation, quiet intro, continued convergence, master attenuation/boost with peak headroom, cached first sample passed.');

// Partial/aborted playback cannot overwrite a full-track cache.
const handle=player.slice(player.indexOf('  private handleFrame('),player.indexOf('  private submittedEndSample('));
assert(!handle.includes('onMeasuredLoudness'));
const finish=player.slice(player.indexOf('      case "flushed":')+'      case "flushed":'.length,player.indexOf('      case "error":',player.indexOf('      case "flushed":'))).replace(/break;\s*$/,'');
const done=new Function(finish),persisted=[];
const state={stereoBalanceEligible:true,programLoudness:null,measuredLoudnessBlocks:2515,measuredLoudness:{integratedLufs:-10.66},cb:{onMeasuredLoudness:x=>persisted.push(x)},startPlaybackIfReady(){},checkEnded(){}};
// Use production gate constant when executing the actual completion handler.
const complete=new Function('MEASURED_LOUDNESS_MIN_BLOCKS','return function(){'+finish+'}')(57);
complete.call(state);assert.deepEqual(persisted,[-10.66]);
state.measuredLoudness={integratedLufs:null};complete.call(state);assert.equal(persisted.length,1);
assert(app.includes('sda-measured-lufs-v4:'));
console.log('Complete-track cache regression: only decoder completion persists; invalid estimates excluded; partial v2 ignored.');

// Combined stereo energy: dual mono is 3.01 LU above a one-channel master.
const sr=48000,tone=Float32Array.from({length:sr*7},(_,i)=>.03*Math.sin(2*Math.PI*997*i/sr));
const stereo=new LoudnessMeter(sr,2),single=new LoudnessMeter(sr,2);
stereo.push([tone,tone]);single.push([tone,new Float32Array(tone.length)]);
assert(Math.abs(stereo.integrated().integratedLufs-single.integrated().integratedLufs-3.0103)<.01);
const m=stereo.integrated(),gain=masterBalanceGainDb(m.integratedLufs,m.peakDbfs);
const balanced=new LoudnessMeter(sr,2),output=Float32Array.from(tone,x=>x*10**(gain/20));balanced.push([output,output]);
assert(Math.abs(balanced.integrated().integratedLufs+18)<.02);
console.log('Stereo master PCM measures -18 LUFS after one linked positive gain.');

buildSync({entryPoints:['packages/player/src/stereo-master.ts'],bundle:true,platform:'node',format:'cjs',outfile:'tmp/stereo-master.cjs'});
const {isStereoMasterFrame}=require('../../tmp/stereo-master.cjs');
const stereoFrame={codec:'alac',channels:[[],[]],labels:['L','R'],objectChannels:[],events:[]};
assert(isStereoMasterFrame(stereoFrame));
for(const changed of [{codec:'adm'},{channels:[[],[],[]]},{labels:['L','Obj_1']},{objectChannels:[{id:1}]},{events:[{id:1}]},{labels:['C','LFE']}])assert(!isStereoMasterFrame({...stereoFrame,...changed}));
const toggle=player.match(/  setVolumeBalance\(enabled: boolean\): void \{[\s\S]*?\n  }/)[0];
const Toggle=new Function(js('class Toggle {'+toggle+'};exports.Toggle=Toggle').replace('exports.Toggle = Toggle;','return Toggle;'))();
const t=new Toggle(),web=[],native=[];t.renderer={setVolumeBalance:x=>web.push(x)};t.nativeRendererSink={setProgramEnabled:x=>native.push(x)};
for(const eligible of [false,true,false,true]){t.stereoBalanceEligible=eligible;t.setVolumeBalance(true);assert.equal(web.at(-1),eligible);assert.equal(native.at(-1),eligible);assert.equal(t.volumeBalanceEnabled,true)}
t.setVolumeBalance(false);assert.equal(native.at(-1),false);
state.stereoBalanceEligible=false;state.measuredLoudness={integratedLufs:-10};complete.call(state);assert.equal(persisted.length,1);
console.log('Stereo-only eligibility: ADM, multichannel and objects excluded; preference retained across tracks; both outputs bypass.');
