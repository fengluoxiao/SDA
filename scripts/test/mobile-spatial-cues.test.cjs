const assert = require('node:assert/strict');
const fs = require('node:fs');
const {transformSync} = require('esbuild');
const app = fs.readFileSync('apps/mobile/App.tsx', 'utf8').replace(/\r\n/g,'\n');
const method = app.match(/  private setSpatialCueDb = async \(db: number\) => \{[\s\S]*?\n  };/)[0];
// Android restores saved cues only after nativeStart has queued the selected
// speaker layout. This supplements the device playback regression: previously
// 360RA-13 + any non-default cue setting failed before the first PCM frame.
const bridge = fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt', 'utf8');
const init = bridge.slice(bridge.indexOf('private fun ensureEngine('), bridge.indexOf('private fun stopFeedThread('));
const startAt = init.indexOf('SdaEngine.nativeStart(ptr)');
const restoreAt = init.indexOf('restoreSpatialCues(ptr, layout)');
assert.ok(startAt >= 0 && restoreAt > startAt);
assert.match(init.slice(restoreAt), /catch \(error: Throwable\) \{[\s\S]*?nativeClose\(ptr\); handle = 0L; throw error/);
const mod = {exports:{}};
const Platform = {OS:'ios'};
new Function('module','exports','Platform',transformSync(`export class Harness {${method}}`,{loader:'ts',format:'cjs'}).code)(mod,mod.exports,Platform);
function harness(wait,fail=false) {
 const p=new mod.exports.Harness(), calls=[];
 p.state={spatialCueDb:-6,spatialCueBusy:false,playing:true,paused:false,positionMs:12345,fifoFrames:8000,volume:.7,busy:false};
 p.setState=patch=>Object.assign(p.state,patch);
 p.getEngine=()=>({stop(){throw Error('stop forbidden')},play(){throw Error('play forbidden')},pause(){throw Error('pause forbidden')},async setSpatialCueDb(db){calls.push(db);if(wait)await wait;if(fail)throw Error('failed')}});
 return {p,calls};
}
(async()=>{
 for(const platform of ['ios','android'])for(const paused of [false,true])for(const db of [0,-3,-6,-9,-12]){
  Platform.OS=platform;
  const {p,calls}=harness();p.state.paused=paused;
  const before={...p.state};await p.setSpatialCueDb(db);
  for(const k of ['playing','paused','positionMs','fifoFrames','volume','busy'])assert.equal(p.state[k],before[k]);
  assert.equal(p.state.spatialCueDb,db);assert.equal(p.state.spatialCueBusy,false);assert.deepEqual(calls,[db]);
 }
 let resolve;const pending=new Promise(r=>resolve=r);const {p,calls}=harness(pending);
 const task=p.setSpatialCueDb(-3);await p.setSpatialCueDb(0);assert.deepEqual(calls,[-3]);resolve();await task;
 const failed=harness(null,true);await failed.p.setSpatialCueDb(0);assert.equal(failed.p.state.spatialCueDb,-6);assert.equal(failed.p.state.spatialCueBusy,false);assert.equal(failed.p.state.error,'failed');
 for(const db of [NaN,1,-5,-99]){const h=harness();await h.p.setSpatialCueDb(db);assert.deepEqual(h.calls,[])}
 Platform.OS='android';const h=harness();await h.p.setSpatialCueDb(0);assert.deepEqual(h.calls,[0]);
 console.log('Live spatial cue settings: transport preserved, values, failure, concurrency and both native platforms passed.');
})().catch(e=>{console.error(e);process.exitCode=1});
