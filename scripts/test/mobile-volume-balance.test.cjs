const assert = require('node:assert/strict');
const fs = require('node:fs');
const { transformSync } = require('esbuild');
const ts = fs.readFileSync('packages/player/src/bs1770.ts','utf8');
const mod = {exports:{}};
new Function('module','exports',transformSync(ts,{loader:'ts',format:'cjs'}).code)(mod,mod.exports);
const {LoudnessMeter,masterBalanceGainDb}=mod.exports;
const player=fs.readFileSync('packages/player/src/player.ts','utf8').replace(/\r\n/g,'\n');
const constants=player.match(/const MEASURED_LOUDNESS_\w+ = [^;]+;/g).join('\n');
const method=player.match(/  private applyMeasuredLoudnessBalance\([\s\S]*?\n  }/)[0];
const block=player.slice(player.indexOf('      if (this.stereoBalanceEligible) {',player.indexOf('  private pumpPcm')),player.indexOf('\n\n      // (Re)declare bed sources'));
const hmod={exports:{}};
new Function('module','exports','masterBalanceGainDb',transformSync(`${constants}\nexport class Harness {${method} process(frame:any){const renderer=this.renderer;${block}}}`,{loader:'ts',format:'cjs'}).code)(hmod,hmod.exports,masterBalanceGainDb);
const waves=[44100,48000,96000,192000].map(rate=>{
  const m=new LoudnessMeter(rate,2), length=Math.round(rate*6.4), chunks=997;
  for(let start=0;start<length;start+=chunks){
    const channels=[0,1].map(ch=>Float32Array.from({length:Math.min(chunks,length-start)},(_,j)=>{
      const i=start+j,amp=i<rate*1.1?0.012:0.42;
      return amp*(Math.sin(2*Math.PI*997*i/rate+ch*0.3)+0.13*Math.sin(2*Math.PI*17003*i/rate));
    }));m.push(channels);
  }
  return {rate,seconds:6.4,...m.integrated()};
});
const silent=new LoudnessMeter(48000,2);silent.push([new Float32Array(48000),new Float32Array(48000)]);assert.equal(silent.integrated().integratedLufs,null);
const gains=[[-10,-1.5],[-23,-12],[-23,-3],[-10,null],[-18,2],[-80,-90],[60,-90]].map(([lufs,peak])=>({lufs,peak,gainDb:masterBalanceGainDb(lufs,peak)}));
assert.equal(masterBalanceGainDb(-23,-12),0);assert.equal(masterBalanceGainDb(-10,-1.5),-8);
const calls=[],p=new hmod.exports.Harness();Object.assign(p,{stereoBalanceEligible:true,sampleRate:48000,cachedMeasuredLufs:null,cachedMeasuredTruePeakDbtp:null,measuredLoudnessSettled:false,balancedLoudnessBlocks:0,scheduledProgramLoudnessGainDb:null,volumeBalanceEnabled:true,trackCodec:'alac',renderer:null,setNativeProgramGainDb:(gainDb,at)=>calls.push({gainDb,at})});
const schedule=[];
for(const [blocks,lufs,peak] of [[56,-17,-2],[57,-17.28,-2],[107,-10.66,-2],[108,-8,-2],[157,-22,-2],[207,-10,2]]) {
  calls.length=0;
  p.process({codec:'alac',samplePos:blocks*4800,loudness:{blocks,integratedLufs:lufs,truePeakDbtp:peak}});
  schedule.push({blocks,lufs,peak,at:blocks*4800,calls:[...calls]});
}
const fixture={waves,gains,schedule};
const path='crates/sda-native/src/balance-windows-fixtures.json';
if(process.argv.includes('--update'))fs.writeFileSync(path,JSON.stringify(fixture,null,2)+'\n');
else assert.deepEqual(JSON.parse(fs.readFileSync(path,'utf8')),fixture,'Windows policy changed: regenerate and review Android parity fixtures');
// Bridge wiring and full-decode-only persistence are part of parity, not just UI.
const kotlin=fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt','utf8');
assert.match(kotlin,/nativeSetVolumeBalance/);assert.match(kotlin,/nativeSetMeasuredLoudness/);
assert.match(kotlin,/nativeFinish\(ptr\)[\s\S]*?check\(result >= 0\)[\s\S]*?nativeCompleteLoudness\(ptr\)/);
assert.match(kotlin,/getBoolean\("volumeBalanceEnabled", false\)/);
assert.match(kotlin,/sda-measured-lufs-v6:\$contentHash:48000/);
assert.match(fs.readFileSync('apps/mobile/App.tsx','utf8'),/this\.state\.headYaw, track\.contentHash/);
console.log('Windows/Android balance golden vectors, protective gain, cache and JNI wiring passed.');
