import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
const builder=fileURLToPath(new URL('./build-ku100-historical-direct.mjs',import.meta.url));
function fixture(){
 const root=mkdtempSync(join(tmpdir(),'sda-ku100-restore-')),src=join(root,'source'),out=join(root,'output'),license=join(root,'LICENSE');mkdirSync(src);writeFileSync(license,'fixture license');
 for(const [name,n,version] of [['hrtf-dense',61,4],['hrtf',17,5]]){
  const d=join(src,name);mkdirSync(d);const f=new Float32Array(1024);f[128]=.5;f[128+512]=.25;const b=Buffer.from(f.buffer);writeFileSync(join(d,'dry.f32'),b);
  const sha256=createHash('sha256').update(b).digest('hex');writeFileSync(join(d,'hrtf-set.json'),JSON.stringify({sampleRate:48000,calibrationVersion:version,processing:{calibrated:true},positions:Array.from({length:n},(_,i)=>({azimuth:i,elevation:0,dry:'dry.f32',wet:'old-wet.f32',assets:{dry:{sha256}}}))}));
 }
 return {root,src,out,license};
}
const run=f=>spawnSync(process.execPath,[builder,'--source-root',f.src,'--out',f.out,'--license',f.license],{encoding:'utf8'});
test('preserves dry bytes, removes room residual, does not relabel grid',()=>{
 const f=fixture(),r=run(f);assert.equal(r.status,0,r.stderr);
 for(const name of ['hrtf-dense','hrtf']){
  const m=JSON.parse(readFileSync(join(f.out,name,'hrtf-set.json')));assert.equal(m.processing.spatialCues,false);assert.equal(m.processing.historicalInterpolation,false);
  assert.deepEqual(readFileSync(join(f.src,name,'dry.f32')),readFileSync(join(f.out,name,'dry.f32')));
  assert.ok(m.positions.every(p=>p.wet===p.dry));assert.equal(m.positions.length,name==='hrtf-dense'?61:17);
 }
 assert.equal(JSON.parse(readFileSync(join(f.out,'restoration-report.json'))).directionCount,61);
 assert.notEqual(run(f).status,0,'must refuse overwrite');
});
test('rejects corrupt source before creating output',()=>{
 const f=fixture();writeFileSync(join(f.src,'hrtf','dry.f32'),Buffer.alloc(4096));assert.notEqual(run(f).status,0);assert.equal(existsSync(f.out),false);
});
test('rejects overlapping source/output',()=>{
 const f=fixture();f.out=join(f.src,'nested');assert.notEqual(run(f).status,0);assert.equal(existsSync(f.out),false);
});
test('rejects unsafe source filenames',()=>{
 const f=fixture(),p=join(f.src,'hrtf-dense/hrtf-set.json'),m=JSON.parse(readFileSync(p));m.positions[0].dry='../outside.f32';writeFileSync(p,JSON.stringify(m));assert.notEqual(run(f).status,0);assert.equal(existsSync(f.out),false);
});

test('historical interpolation must be explicitly requested',()=>{
 const f=fixture();const r=spawnSync(process.execPath,[builder,'--source-root',f.src,'--out',f.out,'--license',f.license,'--historical-interpolation'],{encoding:'utf8'});
 assert.equal(r.status,0,r.stderr);
 for(const name of ['hrtf-dense','hrtf']) assert.equal(JSON.parse(readFileSync(join(f.out,name,'hrtf-set.json'))).processing.historicalInterpolation,true);
});

test('shared spatial cues require historical interpolation and preserve dry bytes',()=>{
 const f=fixture();const args=[builder,'--source-root',f.src,'--out',f.out,'--license',f.license,'--spatial-cues'];
 assert.notEqual(spawnSync(process.execPath,args).status,0);assert.equal(existsSync(f.out),false);
 const r=spawnSync(process.execPath,[...args,'--historical-interpolation'],{encoding:'utf8'});assert.equal(r.status,0,r.stderr);
 const m=JSON.parse(readFileSync(join(f.out,'hrtf-dense/hrtf-set.json')));assert.equal(m.processing.spatialCues,true);
 assert.deepEqual(readFileSync(join(f.src,'hrtf-dense/dry.f32')),readFileSync(join(f.out,'hrtf-dense/dry.f32')));
});
