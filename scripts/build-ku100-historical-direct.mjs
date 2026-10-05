/** Recover a historical calibrated KU100 direct path without changing its samples.
 * Explicit output only: never replaces shipped assets or changes platform defaults.
 * The recovered grid is 61 directions, NOT a relabelled 128-direction set.
 */
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, join, basename, relative, isAbsolute } from 'node:path';
const args = process.argv.slice(2);
const historicalInterpolation = args.includes('--historical-interpolation');
const spatialCues = args.includes('--spatial-cues');
if(spatialCues && !historicalInterpolation)throw Error('Spatial cues require explicit historical interpolation');
const option = name => { const i=args.indexOf(`--${name}`); if(i<0 || !args[i+1]) throw Error(`Required: --${name}`); return resolve(args[i+1]); };
const source=option('source-root'), out=option('out'), license=option('license');
const within=(parent,child)=>{const r=relative(parent,child);return r==='' || (!r.startsWith('..') && !isAbsolute(r));};
if(within(source,out)||within(out,source))throw Error('Source and output must not overlap');
if(existsSync(out))throw Error('Refusing to overwrite existing output');
const sha=b=>createHash('sha256').update(b).digest('hex');
const plans=[];
for(const [name,count] of [['hrtf-dense',61],['hrtf',17]]) {
 const root=join(source,name), manifestBytes=readFileSync(join(root,'hrtf-set.json'));
 const m=JSON.parse(manifestBytes);
 if(m.sampleRate!==48000 || ![4,5].includes(m.calibrationVersion) || !m.processing?.calibrated || m.positions?.length!==count)throw Error(`Unexpected historical manifest: ${name}`);
 const entries=m.positions.map(p=>{
  if(basename(p.dry)!==p.dry || /[\\/]/.test(p.dry))throw Error('Asset filename must be local');
  const bytes=readFileSync(join(root,p.dry));
  if(bytes.length!==512*2*4)throw Error('Expected calibrated 512-tap stereo HRIR');
  const f=new Float32Array(bytes.buffer,bytes.byteOffset,bytes.length/4);
  if(!f.every(Number.isFinite)||!f.some(x=>x!==0))throw Error('Invalid historical HRIR');
  const hash=sha(bytes);
  if(p.assets?.dry?.sha256!==hash)throw Error(`Historical asset hash mismatch: ${p.dry}`);
  return {position:p,bytes,hash};
 });
 plans.push({name,m,manifestHash:sha(manifestBytes),entries});
}
// Validate everything before writing. Wet equals dry because the generic loader
// computes dry + weight*(wet-dry); this makes the room residual exactly zero.
const licenseBytes=readFileSync(license);
mkdirSync(out,{recursive:true});
const report={mode:'historical-calibrated-direct-restore',directionCount:61,speakerFallbackCount:17,sampleRate:48000,roomResidual:false,spatialCues,preservesDrySamplesExactly:true,sources:[]};
for(const {name,m,manifestHash,entries} of plans){
 const root=join(out,name);mkdirSync(root);
 m.positions=entries.map(({position:p,bytes,hash})=>{
  writeFileSync(join(root,p.dry),bytes);
  // Reuse the same file as the dry and wet direct prefix: no BRIR is shipped.
  return {...p,wet:p.dry,assets:{dry:{...p.assets.dry,sha256:hash,tapCountPerEar:512},wet:{sha256:hash,tapCountPerEar:512}}};
 });
 m.processing={...m.processing,historicalInterpolation,mobileDirectOnly:false,spatialCues,dryTapLimit:512,wetTapLimit:512,
  note:'Historical calibrated direct samples preserved byte-for-byte; no measured room residual; added spatial cues are controlled by the explicit spatialCues flag. 61-direction dense grid plus matching 17-speaker fallback. Not a 128-direction migration.'};
 m.restoration={sourceManifestSha256:manifestHash,originalWetRemoved:true};
 writeFileSync(join(root,'hrtf-set.json'),JSON.stringify(m,null,2)+'\n');writeFileSync(join(root,'LICENSE-SADIE.txt'),licenseBytes);
 report.sources.push({name,manifestSha256:manifestHash,files:entries.map(e=>({file:e.position.dry,sha256:e.hash}))});
}
writeFileSync(join(out,'restoration-report.json'),JSON.stringify(report,null,2)+'\n');
console.log(`Recovered exact calibrated direct samples: 61 directions + 17 speaker fallback -> ${out}`);
