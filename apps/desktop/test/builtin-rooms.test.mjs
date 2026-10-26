import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createBuiltinRooms} from '../builtin-rooms.cjs';
import {createRoomLab} from '../room-lab.cjs';
import profiles from '../cinema-profiles.cjs';
const root=path.resolve(import.meta.dirname,'../builtin-rooms');
const temp=fs.mkdtempSync(path.resolve(import.meta.dirname,'../../../tmp/builtin-verify-'));
const cache=path.join(temp,'cache');
const lib=createBuiltinRooms(root,cache);
const list=lib.list();
assert.deepEqual(list.map(x=>x.layout),['7.1.4','2.0','2.1','5.1','5.1.2','5.1.4','7.1.2','9.1.2','9.1.4','9.1.6','360RA-13','22.2','11.1.8']);
assert(list.every(x=>x.builtin&&x.measurement==='simulated'));
assert.equal(createRoomLab({runtimeFile:path.join(temp,'missing.json'),storeRoot:temp,assetsRoot:temp}).status().available,false);
for(const item of list){
 const {filePath,profile}=lib.read(item.id);
 assert(fs.existsSync(filePath));assert.equal(profile.layout,item.layout);
 const c=profile.simulation.config;
 assert.deepEqual([c.length,c.width,c.height,c.earHeight,c.placement,c.order],[6,5,3.2,1.2,.7,10]);
 assert.equal(profile.simulation.sourceModel,'ideal-omnidirectional');
 assert.equal(profile.simulation.revision,6);
 assert.equal(profile.simulation.hrtfProfile,"apple-hrtfhq-542-v1");
 assert.equal(profile.simulation.receiverPreparation,"native-shared-clarity-v1");
 assert.equal(profile.simulation.comparison.basis,"native-mixed-speaker-v1");
 assert.equal(profile.simulation.comparison.energyDb.hrtfhq,profile.simulation.comparison.energyDb.direct);
 assert.equal(c.material,'studio');
 assert.equal(c.listeningDistance,1.2);
 assert.equal(profile.simulation.reference.makeupGainDb,0);
 assert.equal(profile.simulation.reference.absoluteSplCalibrated,false);
 assert.equal(profile.simulation.reference.rirHighpassEnabled,false);
 assert.equal(Object.keys(profile.simulation.surfaces).length,6);
 if(c.shape!=="box")assert(profile.simulation.studioDesign.firstOrderEarlyReflections.every(p=>p.worstDb<=-10));
 if(item.layout==="22.2")assert.equal(c.shape,"sphere");
 if(item.layout==="11.1.8"){
  assert.equal(c.shape,"box");
  const distances=Object.values(profile.simulation.positions).map(p=>Math.hypot(...p.map((v,i)=>v-profile.simulation.listener[i])));
  assert(Math.max(...distances)-Math.min(...distances)>.5,'rectangular boundary positions must not be equal-radius');
  const moved=structuredClone(profile);moved.simulation.positions.FrontLeft[0]+=.1;
  assert.throws(()=>profiles.validateRoom(moved),/方形布置距离/);
 }
 const badSurface=structuredClone(profile);badSurface.simulation.surfaces.floor.coverage=2;
 assert.throws(()=>profiles.validateRoom(badSurface),/表面/);
 const badDistance=structuredClone(profile);badDistance.simulation.config.listeningDistance=2;
 assert.throws(()=>profiles.validateRoom(badDistance),/设计/);
 const broken=structuredClone(profile);broken.simulation.reference.makeupGainDb=6;
 assert.throws(()=>profiles.validateRoom(broken),/参考条件/);
 const badMaterial=structuredClone(profile);badMaterial.simulation.material.coeffs[0]=1.1;
 assert.throws(()=>profiles.validateRoom(badMaterial),/材料来源/);
 assert(profile.simulation.hrtfSha256);assert.equal(profile.simulation.sourceSha256,null);
 for(const s of profile.speakers){
  assert(s.roomLeft.length<=32768);
  assert(s.roomLeft.some((v,i)=>Math.abs(v-s.directLeft[i])>1e-7),'must contain computed reflections');
  assert(profile.simulation.paths[s.name][0].distance>0);
 }
 fs.writeFileSync(filePath,'corrupt cache');
 assert.equal(lib.read(item.id).profile.layout,item.layout);
 console.log(`PASS ${item.layout}: ${profile.speakers.length} speakers, no generator runtime`);
}
assert.throws(()=>lib.read('../settings.json'));
assert.equal(lib.has('../settings.json'),false);
// Old release selections still resolve by their own verified bytes, not by
// silently applying a new room with different physical assumptions.
const old='3a13f276cb17cd6577362603384bc0a5c8a94d06980e85938b83b019ee1057b4';
if(fs.existsSync(path.join(root,`${old}.json.gz`))){
 assert(lib.has(old));assert.equal(lib.read(old).profile.simulation.revision,3);
}
const badRoot=path.join(temp,'bad');fs.mkdirSync(badRoot);
fs.copyFileSync(path.join(root,'catalog.json'),path.join(badRoot,'catalog.json'));
fs.writeFileSync(path.join(badRoot,`${list[0].id}.json.gz`),'corrupt archive');
assert.throws(()=>createBuiltinRooms(badRoot,path.join(temp,'bad-cache')).read(list[0].id),/checksum/);
const pkg=JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname,'../package.json')));
for(const file of ['builtin-rooms/**','builtin-rooms.cjs','cinema-profiles.cjs','monitor-settings.cjs','room-layouts.cjs'])assert(pkg.build.files.includes(file));
console.log('PASS cache recovery, archive corruption, path rejection and packaging inclusion');

const catalog=JSON.parse(fs.readFileSync(path.join(root,'catalog.json'),'utf8'));
for(const entry of catalog.profiles){
 for(const oldId of entry.supersedes??[]){
  const saved={settings:{enabled:false,directDb:-2},profileId:oldId,layoutProfiles:{[entry.summary.layout]:oldId}};
  const upgraded=lib.upgrade(saved);
  assert.equal(upgraded.profileId,entry.id);assert.equal(upgraded.layoutProfiles[entry.summary.layout],entry.id);
  assert.strictEqual(upgraded.settings,saved.settings,'upgrading a preset must preserve disabled state and controls');
  assert.strictEqual(lib.upgrade(upgraded),upgraded,'repeated upgrade is stable');
  assert(lib.read(oldId).profile,'legacy IDs remain verifiable by their original data');
 }
}
const personal={profileId:'f'.repeat(64),settings:{enabled:true}};assert.strictEqual(lib.upgrade(personal),personal);
console.log('PASS HRTFHQ receiver identity, native comparison references, saved preset upgrade and personal-profile isolation');
