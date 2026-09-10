const assert=require("node:assert/strict"),fs=require("node:fs"),os=require("node:os"),path=require("node:path");
(async()=>{
 const m=await import("../../apps/desktop/parametric-hrtf.mjs");
 const {importInWorker}=require("../../apps/desktop/personal-hrtf.cjs");
 const candidates=Array.from({length:64},()=>m.generateParameters());assert.equal(new Set(candidates.map(m.parameterKey)).size,64);
 assert(candidates.every(m.validParameters));assert.throws(()=>m.generateParameters(()=>1));
 const peak=a=>a.reduce((best,v,i)=>Math.abs(v)>Math.abs(a[best])?i:best,0);
 for(const p of candidates){
  for(const [az,el] of [[30,0],[90,0],[135,45],[0,90]]){
   const a=m.synthesizeHrir(az,el,p),b=m.synthesizeHrir(-az,el,p);assert(a.every(Number.isFinite));
   const l=a.slice(0,512),r=a.slice(512);assert(Math.max(...a.map(Math.abs))<3);
   l.forEach((v,i)=>assert(Math.abs(v-b[512+i])<1e-6));
   if(el!==90){assert(peak(r)>peak(l));assert(l.reduce((s,v)=>s+v*v,0)>r.reduce((s,v)=>s+v*v,0));}
  }
 }
 const poleA=m.synthesizeHrir(0,90,candidates[0]),poleB=m.synthesizeHrir(130,90,candidates[0]);
 poleA.forEach((v,i)=>assert(Math.abs(v-poleB[i])<1e-6));
 const params=candidates[0],left=m.synthesizeHrir(30,0,params);
 assert.notDeepEqual(left,m.synthesizeHrir(30,0,candidates[1]));
 assert.notDeepEqual(left,m.synthesizeHrir(150,0,params));
 assert.notDeepEqual(left,m.synthesizeHrir(30,45,params));
 const field={version:2,power:2,anchors:[{name:'L',az:31.25,el:0,parameters:candidates[0]},{name:'R',az:-33.75,el:0,parameters:candidates[1]},{name:'Top',az:0,el:45,parameters:candidates[2]}]};
 assert(m.validParameters(field));
 for(const power of [1,2,3,4])for(const a of field.anchors)assert.deepEqual(m.synthesizeHrir(a.az,a.el,{...field,power}),m.synthesizeHrir(a.az,a.el,a.parameters));
 assert(m.synthesizeHrir(0,0,field).every(Number.isFinite));
 const temp=fs.mkdtempSync(path.join(os.tmpdir(),"sda-generated-hrtf-"));
 try{
  const assessment={note:"automated test, no listener assessment"};
  const result=await importInWorker(null,temp,{parameters:params,assessment});
  const root=path.join(temp,'hrtf-'+result.id),manifest=JSON.parse(fs.readFileSync(path.join(root,'hrtf-set.json')));
  assert.equal(manifest.parametricHrtfVersion,1);assert.equal(manifest.source.measured,false);assert.equal(manifest.positions.length,433);
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root,'assessment.json'))),assessment);
  for(const pos of manifest.positions){const bytes=fs.readFileSync(path.join(root,pos.dry));assert.deepEqual(bytes,Buffer.from(m.synthesizeHrir(pos.azimuth,pos.elevation,params).buffer));assert.equal(pos.wet,pos.dry);}
  assert.equal((await importInWorker(null,temp,{parameters:params,assessment})).id,result.id);
  const savedField=await importInWorker(null,temp,{parameters:field,assessment});
  const fieldRoot=path.join(temp,'hrtf-'+savedField.id),fieldManifest=JSON.parse(fs.readFileSync(path.join(fieldRoot,'hrtf-set.json')));
  for(const anchor of field.anchors){const entry=fieldManifest.positions.find(p=>p.azimuth===anchor.az&&p.elevation===anchor.el);assert(entry);assert.deepEqual(fs.readFileSync(path.join(fieldRoot,entry.dry)),Buffer.from(m.synthesizeHrir(anchor.az,anchor.el,anchor.parameters).buffer));}

  await assert.rejects(()=>importInWorker(null,temp,{parameters:{...params,radius:500},assessment}));
 }finally{fs.rmSync(temp,{recursive:true,force:true});}
 console.log('Parametric HRTF: 64 independently generated models, bilateral signs, finite filters, 433-direction worker export identical to audition, records and idempotence passed; no audio output.');
})().catch(e=>{console.error(e);process.exitCode=1});
