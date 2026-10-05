const assert=require('node:assert/strict');
const fs=require('node:fs');
const {createHash}=require('node:crypto');
const {transformSync}=require('esbuild');
const profiles=JSON.parse(fs.readFileSync('apps/mobile/rendering-presets.json','utf8'));
assert.equal(profiles.length,1);
const dense=profiles[0];
assert.deepEqual([dense.id,dense.hrtfSet,dense.assetDirectory,dense.direct,dense.directional,dense.nearField,dense.roomId,dense.hrtfWetWeight],['object-dense','dense','hrtf-restored/hrtf-dense',true,true,false,'',0]);
const sha=b=>createHash('sha256').update(b).digest('hex');
for(const [directory,count] of [['hrtf-dense',61],['hrtf',17]]) {
 const source=`apps/mobile/assets/hrtf-restored/${directory}`;
 const manifest=JSON.parse(fs.readFileSync(`${source}/hrtf-set.json`,'utf8'));
 assert.equal(manifest.positions.length,count);
 assert.equal(manifest.sampleRate,48000);
 assert.equal(manifest.processing.calibrated,true);
 assert.equal(manifest.processing.historicalInterpolation,true);
 assert.equal(manifest.processing.spatialCues,true);
 assert.ok(Math.abs(manifest.processing.spatialCueGain-Math.pow(10,-6/20))<1e-12);
 assert.deepEqual(manifest,JSON.parse(fs.readFileSync(`apps/mobile/modules/sda-core/ios/Resources/hrtf-restored/${directory}/hrtf-set.json`,'utf8')));
 assert.equal(manifest.processing.mobileDirectOnly,false);
 assert.equal(new Set(manifest.positions.map(p=>`${p.azimuth}/${p.elevation}`)).size,count);
 for(const position of manifest.positions) {
  assert.equal(position.wet,position.dry,'no measured room residual');
  const data=fs.readFileSync(`${source}/${position.dry}`);
  assert.equal(sha(data),position.assets.dry.sha256);
  assert.equal(data.length,4096);
  assert.deepEqual(data,fs.readFileSync(`apps/mobile/modules/sda-core/ios/Resources/hrtf-restored/${directory}/${position.dry}`));
 }
}
for(const view of ['RemotePlayer.tsx','IOSPlayer.tsx','IOSNativeSettings.tsx']){
 const text=fs.readFileSync(`apps/mobile/src/${view}`,'utf8');
 assert.doesNotMatch(text,/p\.setNearField|p\.setRoom|近场渲染|房间仿真|距离与房间/);
 assert.match(text,/逐对象渲染/);assert.match(text,/实际方向/);
}
const gradle=fs.readFileSync('apps/mobile/android/app/build.gradle','utf8');
assert.match(gradle,/exclude 'hrtf\*\/\*\*', 'rooms\/\*\*'/);
assert.match(gradle,/include 'hrtf-restored\/\*\*'/);
assert.doesNotMatch(gradle,/from\(windowsHrtfAssets\)/);
assert.match(gradle,/inputs\.file\(new File\(projectRoot, 'rendering-presets\.json'\)\)/);
const bridge=fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt','utf8');
assert.match(bridge,/listOf\("hrtf-restored\/hrtf-dense", "hrtf-restored\/hrtf"\)/);
assert.match(bridge,/putInt\("mobileDirectVersion", 1\)/);
assert.match(bridge,/\.put\("hrtfWetWeight", 0.0\)/);
assert.match(bridge,/\.put\("roomId", ""\)/);
assert.match(bridge,/\.put\("nearField", false\)/);
const swift=fs.readFileSync('apps/mobile/modules/sda-core/ios/SdaPlayer.swift','utf8');
assert.match(swift,/sda.mobileDirectVersion/);assert.match(swift,/let directory = "hrtf-restored\/hrtf-dense"/);
const pod=fs.readFileSync('apps/mobile/modules/sda-core/ios/SdaCore.podspec','utf8');
assert.doesNotMatch(pod,/Resources\/rooms|Resources\/hrtf-dense|Resources\/hrtf-raw/);
// Execute the production async handler, checking all playback state survives.
async function main(){
 const app=fs.readFileSync('apps/mobile/App.tsx','utf8').replace(/\r\n/g,'\n');
 const method=app.match(/  private setRenderingPreset = async \(id: string\) => \{[\s\S]*?\n  };/)[0];
 const mod={exports:{}};
 new Function('module','exports','renderingPresets',transformSync(`export class Harness {${method}}`,{loader:'ts',format:'cjs'}).code)(mod,mod.exports,profiles);
 function harness(pending,fail=false){
  const p=new mod.exports.Harness(),calls=[];
  p.state={playing:true,paused:false,ended:false,positionMs:12345,decodedMs:18000,fifoFrames:123,objects:[{id:7}],busy:false,nearFieldBusy:false,roomBusy:false,queue:[{uri:'track'}],queueIndex:0,selectedUri:'track',volume:.72,headYaw:15,hrtfSet:'dense',hrtfWetWeight:.04,directObjects:false,directionalObjects:false,nearField:true,roomId:'legacy'};
  p.changingTrack=false;p.setState=patch=>Object.assign(p.state,patch);
  p.getEngine=()=>({stop(){throw Error('must not stop')},play(){throw Error('must not play')},pause(){throw Error('must not pause')},async setRenderingPreset(id){calls.push(id);if(pending)await pending;if(fail)throw Error('failure')},renderingSettings(){return JSON.stringify(dense)},hrtfStatus(){return '61 direct'}});
  return {p,calls};
 }
 const retained=['playing','paused','ended','positionMs','decodedMs','fifoFrames','objects','queue','queueIndex','selectedUri','volume','headYaw'];
 for(const mode of ['playing','paused','stopped']){
  const {p,calls}=harness();p.state.playing=mode!=='stopped';p.state.paused=mode==='paused';const before=retained.map(k=>p.state[k]);
  await p.setRenderingPreset(dense.id);
  assert.deepEqual(retained.map(k=>p.state[k]),before);assert.deepEqual(calls,[dense.id]);
  assert.deepEqual([p.state.hrtfWetWeight,p.state.directObjects,p.state.directionalObjects,p.state.nearField,p.state.roomId],[0,true,true,false,'']);
  assert.equal(p.state.busy,false);assert.equal(p.changingTrack,false);
 }
 let resolve;const wait=new Promise(r=>resolve=r);const active=harness(wait);const task=active.p.setRenderingPreset(dense.id);
 assert.equal(active.p.state.busy,true);await active.p.setRenderingPreset(dense.id);assert.deepEqual(active.calls,[dense.id]);resolve();await task;
 const failed=harness(null,true);await failed.p.setRenderingPreset(dense.id);assert.equal(failed.p.state.error,'failure');assert.equal(failed.p.state.busy,false);assert.equal(failed.p.state.positionMs,12345);
 for(const key of ['busy','nearFieldBusy','roomBusy','changingTrack','invalid']){
  const {p,calls}=harness();if(key==='changingTrack')p.changingTrack=true;else if(key!=='invalid')p.state[key]=true;
  await p.setRenderingPreset(key==='invalid'?'desktop-standard':dense.id);assert.deepEqual(calls,[]);
 }
 console.log('Mobile direct KU100: 61 unique measurements, zero measured room residual, mobile packaging/migration, removed controls, live-preset playback preservation passed.');
}
main().catch(error=>{console.error(error);process.exitCode=1});
