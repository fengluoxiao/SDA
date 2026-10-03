import {createHash} from 'node:crypto';
import {mkdirSync,readFileSync,writeFileSync,cpSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {collectIrs,directionVector} from './lib/hrtf-source.mjs';
const archive=resolve(process.argv[2] ?? 'tmp/sadie-source/D1.zip');
const out=resolve('apps/mobile/assets/hrtf-mobile-direct');
const reference=JSON.parse(readFileSync('apps/web/public/hrtf-raw/hrtf-set.json','utf8'));
const sha=b=>createHash('sha256').update(b).digest('hex');
if(sha(readFileSync(archive))!==reference.source.archiveSha256) throw Error('KU100 source archive hash mismatch');
const {impulses}=await collectIrs(archive,reference.source.hrPath);
const wrap=az=>((az+180)%360+360)%360-180;
const samples=impulses.map(ir=>({...ir,azimuth:wrap(ir.azimuth),v:directionVector(ir.azimuth,ir.elevation)}))
 .sort((a,b)=>a.elevation-b.elevation || a.azimuth-b.azimuth);
const dot=(a,b)=>a.reduce((n,x,i)=>n+x*b[i],0);
const selected=[];
function addClosest(az,el){const v=directionVector(az,el);const ir=samples.reduce((a,b)=>dot(a.v,v)>=dot(b.v,v)?a:b);if(!selected.includes(ir))selected.push(ir);}
// Preserve the existing object ring and measured bed anchors, then fill sphere gaps
// with distinct original HRIR measurements (no duplicated or synthetic directions).
for(const directory of ['hrtf-raw','hrtf-dense-raw']){
 const m=JSON.parse(readFileSync(`apps/web/public/${directory}/hrtf-set.json`,'utf8'));
 for(const p of m.positions)addClosest(p.azimuth,p.elevation);
}
while(selected.length<128){
 const remaining=samples.filter(ir=>!selected.includes(ir));
 if(!remaining.length)throw Error('Insufficient unique measured directions');
 const distance=ir=>1-Math.max(...selected.map(s=>dot(s.v,ir.v)));
 selected.push(remaining.reduce((a,b)=>distance(a)>=distance(b)?a:b));
}
mkdirSync(out,{recursive:true});
writeFileSync(join(out,'zero-wet.f32'),Buffer.alloc(8));
const positions=selected.sort((a,b)=>a.elevation-b.elevation||a.azimuth-b.azimuth).map((ir,i)=>{
 if(ir.sampleRate!==48000 || ir.left.length!==256 || ir.right.length!==256)throw Error('Expected original 48kHz 256-tap HRIR');
 const packed=new Float32Array(512);packed.set(ir.left);packed.set(ir.right,256);
 const bytes=Buffer.from(packed.buffer);const file=`direction-${String(i).padStart(3,'0')}-dry.f32`;
 writeFileSync(join(out,file),bytes);
 return {azimuth:ir.azimuth,elevation:ir.elevation,dry:file,wet:'zero-wet.f32',measurement:{dry:{sourcePath:ir.sourcePath,azimuth:ir.azimuth,elevation:ir.elevation,originalFrames:256}},assets:{dry:{tapCountPerEar:256,sha256:sha(bytes)},wet:{tapCountPerEar:1,sha256:sha(Buffer.alloc(8))}}};
});
writeFileSync(join(out,'hrtf-set.json'),JSON.stringify({schemaVersion:2,calibrationVersion:0,completeSubject:true,subjectId:'ku100',sampleRate:48000,
 source:{...reference.source,name:'SADIE II KU100 mobile direct-only (128 measured directions)',brPath:null},azimuthConvention:reference.azimuthConvention,
 processing:{calibrated:false,preserveMeasurements:true,mobileDirectOnly:true,peakNormalized:false,runtimeEnergyNormalization:false,dryTapLimit:256,wetTapLimit:1,
 note:'Original HRIR samples only. No BRIR, room residual, room EQ, early reflections, decorrelation or synthetic tail. Same direct HRIR grid serves objects and bed channels; inter-ear timing and level retained.'},positions},null,2)+'\n');
cpSync(resolve('apps/desktop/builtin-rooms/LICENSE-SADIE.txt'),join(out,'LICENSE-SADIE.txt'));
writeFileSync(join(out,'NOTICE.txt'),`SDA Mobile Direct KU100 HRIR

Derived from SADIE II Database V2.2, D1 KU100.
Copyright 2018, University of York.
Licensed under Apache License 2.0; see LICENSE-SADIE.txt.
Record: https://zenodo.org/records/12092466
Source SHA-256: ${reference.source.archiveSha256}

SDA selects 128 distinct original 48 kHz, 256-tap stereo HRIR measurements
and packs the unmodified samples as float32. No BRIR, simulated room,
room EQ, reflection or generated reverberation data is included.
Original measurement-speaker response and inter-ear timing are retained.
`);
console.log(`Mobile KU100: ${positions.length} distinct measured directions, 256 taps/ear, no room tail`);
