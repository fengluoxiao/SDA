import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import path from 'node:path';
import {migrateSharedHrtf,playbackHrtfSet,SHARED_HRTF_PROFILE} from '../apps/web/src/shared-hrtf-profile.ts';
const require=createRequire(import.meta.url);
const {BUNDLED_HRTF_PATTERN,NATIVE_HRTF_SET_PATTERN,bundledHrtfRoot}=require('../apps/desktop/hrtf-policy.cjs');
const store=(values={})=>{const m=new Map(Object.entries(values));return {getItem:k=>m.get(k)??null,setItem:(k,v)=>m.set(k,v),m};};
for(const head of [null,'ku100','hrtfhq','obsolete-hybrid']) {
 const s=store({'sda-volume':'0.72',...(head?{'sda-binaural-head':head}:{})});
 assert.equal(migrateSharedHrtf(s,SHARED_HRTF_PROFILE),true);
 for(const key of ['sda-direct-object-hrtf','sda-directional-hrtf-v1'])assert.equal(s.getItem(key),'true');
 assert.equal(s.getItem('sda-dense-binaural-objects'),'1');assert.equal(s.getItem('sda-binaural-head'),'hrtfhq');
 assert.equal(s.getItem('sda-volume'),'0.72');
 s.setItem('sda-direct-object-hrtf','false');assert.equal(migrateSharedHrtf(s,SHARED_HRTF_PROFILE),false);
 assert.equal(s.getItem('sda-direct-object-hrtf'),'false','repeat initialization must preserve explicit later choices');
}
for(const head of ['h13','d2','personal-'+'a'.repeat(64)]) {
 const s=store({'sda-binaural-head':head});migrateSharedHrtf(s,SHARED_HRTF_PROFILE);assert.equal(s.getItem('sda-binaural-head'),head);
}
assert.equal(migrateSharedHrtf(store(),'fullheight-triangle-commonphase-542-v1'),false);
assert.equal(playbackHrtfSet('hrtfhq',true,true,true),'hrtf-dense');
assert.equal(playbackHrtfSet('hrtfhq',false,false,true),'hrtf');
assert.equal(playbackHrtfSet('ku100',true,true,true),'hrtf-ku100-reference-dense');
assert.equal(playbackHrtfSet('ku100',false,true,true),'hrtf-ku100-reference');
assert.equal(playbackHrtfSet('ku100',true,false,true),'hrtf-dense-raw');
assert.equal(playbackHrtfSet('ku100',true,true,false),'hrtf-dense');
for(const name of ['hrtf-dense/direction-000.f32','hrtf/hrtf-set.json','hrtf-dense/triangle-grid.json','hrtf-ku100-reference-dense/direction-541.f32'])assert(BUNDLED_HRTF_PATTERN.test(name),name);
for(const name of ['../hrtf/hrtf-set.json','hrtf-dense/../secret','hrtf-dense/dir/response.f32','hrtf-dense/direction-000.f32.exe'])assert(!BUNDLED_HRTF_PATTERN.test(name),name);
for(const name of ['hrtf','hrtf-dense','hrtf-ku100-reference','hrtf-ku100-reference-dense'])assert(NATIVE_HRTF_SET_PATTERN.test(name));
assert(!NATIVE_HRTF_SET_PATTERN.test('../hrtf'));
assert.equal(bundledHrtfRoot('hrtf-dense',path.resolve('native/renderer'),'stale-web'),path.resolve('native/hrtf-assets'));
assert.equal(bundledHrtfRoot('hrtf-dense',null,'web-only'),'web-only');
const app=readFileSync('apps/web/src/App.tsx','utf8');
assert(app.indexOf('migrateSharedHrtf(localStorage, sharedRenderProfile.id)')<app.indexOf('export function App()'));
assert(app.includes('return playbackHrtfSet(head, dense, calibrated, sharedHrtfActive)'));
const stage=readFileSync('scripts/build-native-renderer.mjs','utf8');assert(stage.includes('hrtf-ku100-reference-dense'));
assert(JSON.parse(readFileSync('apps/desktop/package.json','utf8')).build.files.includes('hrtf-policy.cjs'));
console.log('Desktop shared profile: defaults, legacy/personal preservation, sidecar-backed asset reads and traversal rejection passed.');
