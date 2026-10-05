const assert = require('node:assert/strict');
const fs = require('node:fs');
const {transformSync} = require('esbuild');
const app = fs.readFileSync('apps/mobile/App.tsx', 'utf8').replace(/\r\n/g,'\n');
const method = app.match(/  private setSpatialCueDb = async \(db: number\) => \{[\s\S]*?\n  };/)[0];
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
 for(const paused of [false,true])for(const db of [0,-3,-6,-9,-12]){
  const {p,calls}=harness();p.state.paused=paused;
  const before={...p.state};await p.setSpatialCueDb(db);
  for(const k of ['playing','paused','positionMs','fifoFrames','volume','busy'])assert.equal(p.state[k],before[k]);
  assert.equal(p.state.spatialCueDb,db);assert.equal(p.state.spatialCueBusy,false);assert.deepEqual(calls,[db]);
 }
 let resolve;const pending=new Promise(r=>resolve=r);const {p,calls}=harness(pending);
 const task=p.setSpatialCueDb(-3);await p.setSpatialCueDb(0);assert.deepEqual(calls,[-3]);resolve();await task;
 const failed=harness(null,true);await failed.p.setSpatialCueDb(0);assert.equal(failed.p.state.spatialCueDb,-6);assert.equal(failed.p.state.spatialCueBusy,false);assert.equal(failed.p.state.error,'failed');
 for(const db of [NaN,1,-5,-99]){const h=harness();await h.p.setSpatialCueDb(db);assert.deepEqual(h.calls,[])}
 Platform.OS='android';const h=harness();await h.p.setSpatialCueDb(0);assert.deepEqual(h.calls,[]);
 console.log('Live spatial cue settings: transport preserved, values, failure, concurrency and iOS guard passed.');
})().catch(e=>{console.error(e);process.exitCode=1});
