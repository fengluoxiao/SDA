const assert = require('node:assert/strict');
const fs = require('node:fs');
const {transformSync} = require('esbuild');
const profiles = JSON.parse(fs.readFileSync('apps/mobile/rendering-presets.json', 'utf8'));
assert.equal(profiles.length, 3);
const standard = profiles.find(p => p.id === 'desktop-standard');
const dense = profiles.find(p => p.id === 'object-dense');
const raw = profiles.find(p => p.id === 'ku100-raw-dry');
assert.deepEqual([raw.hrtfSet, raw.assetDirectory, raw.direct, raw.directional, raw.nearField, raw.roomId, raw.hrtfWetWeight], ['dense-raw', 'hrtf-dense-raw', true, true, false, '', 0]);
assert.equal(standard.hrtfWetWeight, .04); assert.equal(dense.hrtfWetWeight, .04);
assert.deepEqual([standard.hrtfSet, standard.assetDirectory, standard.direct, standard.directional, standard.nearField, standard.roomId], ['standard', 'hrtf', true, true, false, '']);
assert.deepEqual([dense.hrtfSet, dense.assetDirectory, dense.direct, dense.directional, dense.nearField, dense.roomId], ['dense', 'hrtf-dense', true, true, false, '']);
for (const p of profiles) {
  const m = JSON.parse(fs.readFileSync(`apps/desktop/native-renderer/hrtf-assets/${p.assetDirectory}/hrtf-set.json`, 'utf8'));
  assert.equal(m.sampleRate, 48000); assert.equal(m.processing.calibrated, p !== raw);
  if (p === raw) {
    assert.equal(m.calibrationVersion, 0); assert.equal(m.subjectId, 'ku100');
    assert.equal(m.processing.preserveMeasurements, true);
    assert.equal(m.processing.peakNormalized, false); assert.equal(m.processing.runtimeEnergyNormalization, false);
  }
  for (const position of m.positions) for (const part of ['dry', 'wet']) {
    assert.ok(fs.existsSync(`apps/desktop/native-renderer/hrtf-assets/${p.assetDirectory}/${position[part]}`));
  }
  assert.equal(m.positions.length, p === standard ? 17 : 61);
}
// Raw dense loading must retain its original (not calibrated) speaker fallback.
const fallback = JSON.parse(fs.readFileSync('apps/desktop/native-renderer/hrtf-assets/hrtf-raw/hrtf-set.json', 'utf8'));
assert.equal(fallback.positions.length, 17); assert.equal(fallback.processing.calibrated, false);
for (const pos of fallback.positions) for (const part of ['dry', 'wet']) {
  assert.ok(fs.existsSync(`apps/desktop/native-renderer/hrtf-assets/hrtf-raw/${pos[part]}`));
}
// APK source assets must match the web assets used by the approved offline replay.
for (const directory of ['hrtf-dense-raw', 'hrtf-raw']) {
  const source = `apps/web/public/${directory}`;
  const packaged = `apps/desktop/native-renderer/hrtf-assets/${directory}`;
  const manifest = JSON.parse(fs.readFileSync(`${source}/hrtf-set.json`, 'utf8'));
  const names = new Set(['hrtf-set.json', ...manifest.positions.flatMap(p => [p.dry, p.wet])]);
  for (const name of names) assert.deepEqual(fs.readFileSync(`${packaged}/${name}`), fs.readFileSync(`${source}/${name}`));
}
// Execute the real async handler and verify playback survives an acknowledged swap.
async function main() {
const app = fs.readFileSync('apps/mobile/App.tsx','utf8').replace(/\r\n/g,'\n');
const method = app.match(/  private setRenderingPreset = async \(id: string\) => \{[\s\S]*?\n  };/)[0];
const mod = {exports:{}};
new Function('module','exports','renderingPresets', transformSync(`export class Harness {${method}}`, {loader:'ts',format:'cjs'}).code)(mod,mod.exports,profiles);
function harness(mode='playing', pending=null, fail=false) {
  const p = new mod.exports.Harness(), calls=[];
  p.state={playing:mode!=='stopped',paused:mode==='paused',ended:false,positionMs:12345,decodedMs:18000,fifoFrames:123,objects:[{id:7}],busy:false,nearFieldBusy:false,roomBusy:false,queue:[{uri:'track'}],queueIndex:0,selectedUri:'track',volume:.72,headYaw:15,hrtfSet:'dense',hrtfWetWeight:.04,directObjects:true,directionalObjects:true,nearField:true,roomId:'saved-room'};
  p.changingTrack=false;
  p.setState = patch => Object.assign(p.state,patch);
  let selected=dense;
  p.getEngine = () => ({
    stop(){throw Error('must not stop');},play(){throw Error('must not play');},pause(){throw Error('must not pause');},
    async setRenderingPreset(id){calls.push(id);if(pending)await pending;if(fail)throw Error('preset failure');selected=profiles.find(p=>p.id===id);},
    renderingSettings(){calls.push('settings');return JSON.stringify(selected);},hrtfStatus(){return 'loaded';}
  });
  return {p,calls};
}
const retained=['playing','paused','ended','positionMs','decodedMs','fifoFrames','objects','queue','queueIndex','selectedUri','volume','headYaw'];
for(const mode of ['playing','paused','stopped']) {
  const {p,calls}=harness(mode);const before=retained.map(k=>p.state[k]);
  for(const profile of profiles) {
    await p.setRenderingPreset(profile.id);
    assert.deepEqual(retained.map(k=>p.state[k]),before);
    assert.deepEqual([p.state.hrtfSet,p.state.hrtfWetWeight,p.state.directObjects,p.state.directionalObjects,p.state.nearField,p.state.roomId],[profile.hrtfSet,profile.hrtfWetWeight,profile.direct,profile.directional,false,'']);
    assert.equal(p.state.busy,false);assert.equal(p.changingTrack,false);
  }
  assert.deepEqual(calls,profiles.flatMap(p=>[p.id,'settings']));
}
let resolve;const pending=new Promise(r=>resolve=r);const active=harness('playing',pending);
const task=active.p.setRenderingPreset(raw.id);
assert.equal(active.p.state.busy,true);assert.equal(active.p.changingTrack,true);assert.equal(active.p.state.hrtfSet,'dense');
await active.p.setRenderingPreset(standard.id);assert.deepEqual(active.calls,[raw.id]);
resolve();await task;assert.deepEqual(active.calls,[raw.id,'settings']);
const failed=harness('paused',null,true);const before={...failed.p.state};await failed.p.setRenderingPreset(raw.id);
assert.deepEqual(retained.map(k=>failed.p.state[k]),retained.map(k=>before[k]));assert.equal(failed.p.state.hrtfSet,'dense');assert.equal(failed.p.state.error,'preset failure');assert.equal(failed.p.state.busy,false);
for(const key of ['busy','nearFieldBusy','roomBusy','changingTrack','invalid']) {
  const {p,calls}=harness();if(key==='changingTrack')p.changingTrack=true;else if(key!=='invalid')p.state[key]=true;
  await p.setRenderingPreset(key==='invalid'?'unknown':raw.id);assert.deepEqual(calls,[]);
}
const legacy=harness();legacy.p.getEngine=()=>({async setRenderingPreset(){},renderingSettings(){return JSON.stringify({...dense,hrtfWetWeight:undefined});},hrtfStatus(){return '';}});
await legacy.p.setRenderingPreset(dense.id);assert.equal(legacy.p.state.hrtfWetWeight,.04);
}
main().then(()=>console.log('Live preset handler: retained playback, async ACK, guards and failures passed.')).catch(e=>{console.error(e);process.exitCode=1;});
const bridge=fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaModule.kt','utf8');
assert.match(bridge,/"dense-raw" -> "hrtf-dense-raw"/);
assert.doesNotMatch(bridge,/nativeInit\(config, java.io.File\(context.filesDir, "hrtf-dense\/hrtf-set.json"\)/);
assert.match(bridge,/AsyncFunction\("setRenderingPreset"\)/);
assert.match(bridge,/nativeSetHrtfPreset[\s\S]*?check\(error.isEmpty\(\)\)[\s\S]*?putString\("hrtfSet", set\)/);
assert.match(bridge,/putString\("hrtfSet", set\)[\s\S]*?\.commit\(\)/);
assert.match(bridge,/getString\("hrtfSet", "dense"\)/); // no implicit migration
assert.match(bridge,/\$directionCount 方向/);
assert.match(fs.readFileSync('apps/mobile/android/app/build.gradle','utf8'),/from\('\.\.\/\.\.\/rendering-presets.json'\)/);
console.log('Mobile rendering presets: asset selection, live-switch bridge, retained queue/volume/yaw, error paths and legacy settings passed.');

assert.match(bridge,/listOf\("hrtf", "hrtf-dense", "hrtf-raw", "hrtf-dense-raw"\)/);
assert.match(bridge,/putFloat\("hrtfWetWeight", wetWeight.toFloat\(\)\)/);
assert.match(bridge,/getFloat\("hrtfWetWeight", 0.04f\)/);
assert.match(bridge,/put\("hrtfWetWeight", settings.getDouble\("hrtfWetWeight"\)\)/);
assert.match(bridge,/wetWeight.isFinite\(\) && wetWeight in 0.0..1.0/);
const gradle = fs.readFileSync('apps/mobile/android/app/build.gradle','utf8');
assert.match(gradle,/include 'hrtf\/\*\*', 'hrtf-dense\/\*\*', 'hrtf-raw\/\*\*', 'hrtf-dense-raw\/\*\*'/);
assert.match(gradle,/raw.processing.preserveMeasurements != true/);
const native = fs.readFileSync('crates/sda-native/src/lib.rs','utf8');
assert.match(native,/replace_hrtf\(set, self.config.hrtf_wet_weight\)/);
assert.doesNotMatch(native,/replace_hrtf\(set, 0.04\)/);
const view = fs.readFileSync('apps/mobile/src/RemotePlayer.tsx','utf8');
assert.match(view,/Math.abs\(p.hrtfWetWeight - profile.hrtfWetWeight\) < 1e-6/);
console.log('Raw KU100 dry: zero survives roundtrip, legacy wet default retained, original fallback and all packed files verified.');

// JSON-only edits must invalidate the UI bundle too, not just native assets.
assert.match(gradle, /tasks\.withType\(com\.facebook\.react\.tasks\.BundleHermesCTask\)\.configureEach/);
assert.match(gradle, /inputs\.file\(new File\(projectRoot, 'rendering-presets\.json'\)\)/);
