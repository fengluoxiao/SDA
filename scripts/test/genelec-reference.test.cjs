const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const root=path.resolve(__dirname,'../..'),directory=path.join(root,'apps/native-renderer/assets');
const {readReferenceProfile}=require('../../apps/desktop/reference-monitor.cjs');
const manifest=JSON.parse(fs.readFileSync(path.join(directory,'genelec-reference/manifest.json'))),bytes=fs.readFileSync(path.join(directory,'genelec-reference/coefficients.f32'));
for(const layout of Object.keys(manifest.layouts)){
 const p=readReferenceProfile(directory,layout),dry=readReferenceProfile(directory,layout,0);
 assert.equal(p.speakers.length,manifest.layouts[layout].length);assert.equal(p.simulation.referenceAlgorithm,'genelec8351b-reference-room-v1');
 for(let i=0;i<p.speakers.length;i++){
  const s=p.speakers[i],r=manifest.responses.find(r=>r.key===manifest.layouts[layout][i].key);
  assert.equal(s.directLeft.length,4608);assert.equal(s.roomLeft.length,4608);assert.deepEqual(dry.speakers[i].roomLeft,s.directLeft);
  for(const index of [0,120,Math.min(500,r.directTaps-1)])assert.equal(s.directLeft[index],bytes.readFloatLE((r.offsetFloats+index)*4));
  assert(s.roomLeft.some((v,i)=>v!==s.directLeft[i]));assert(s.roomLeft.concat(s.roomRight).every(Number.isFinite));
 }
 if(layout==='360RA-13')assert.equal(p.speakers.filter(s=>s.elevation<0).length,3);
}
assert.throws(()=>readReferenceProfile(directory,'unknown'));assert.throws(()=>readReferenceProfile(directory,'7.1.4',NaN));
const {captureExportAssets}=require('../../apps/desktop/export-assets.cjs');
const snapshot={layout:{effective:'360RA-13'},cinema:{settings:{enabled:false}},equalizer:{}};
const captured=captureExportAssets({snapshot,selectedHrtf:'hrtf-dense',hrtfRoot:path.join(root,'assets/hrtf/apple-hrtfhq-v1'),referenceRoot:directory,referenceMonitor:'genelec-8351b'});
assert.equal(captured.room.layout,'360RA-13');assert.equal(snapshot.cinema.settings.enabled,true);
const room=JSON.parse(captured.blobs.find(b=>b.sha256===captured.room.profile).data);assert.equal(room.speakers.length,13);assert.equal(room.simulation.referenceMonitor,'genelec-8351b');assert.equal(captured.hrtf.wetWeight,.04);
assert(JSON.parse(fs.readFileSync(path.join(root,'apps/desktop/package.json'))).build.files.includes('reference-monitor.cjs'));
console.log('All 15 fixed layouts and self-contained reference-room export retain direction, exact direct samples, lower channels and provenance.');
