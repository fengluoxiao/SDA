import { existsSync, readFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { resolve, join, dirname, basename } from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const hash=b=>createHash('sha256').update(b).digest('hex');
export function activeExperiment() {
 const path=join(root,'apps/mobile/ios-experiments/active.json');
 if(!existsSync(path)) return null;
 const experiment=JSON.parse(readFileSync(path,'utf8'));
 const legacy=join(root,'apps/mobile/ios-experiments');
 const supported={
  'horizontal-support-218-v1':[218,join(legacy,'horizontal-support')],
  'fullheight-triangle-commonphase-542-v1':[542,join(legacy,'fullheight-combined')],
  'apple-hrtfhq-542-v1':[542,join(root,'assets/hrtf/apple-hrtfhq-v1')],
 };
 const config=supported[experiment.id];
 if(!config || experiment.directionCount!==config[0]) throw Error('Unknown shared rendering profile');
 const measured=experiment.id==='apple-hrtfhq-542-v1';
 return {experiment,measured,source:join(config[1],'hrtf-dense'),
  fallbackSource:measured?join(config[1],'hrtf'):join(root,'apps/mobile/assets/hrtf-restored/hrtf')};
}
function validateFamily(source,active,dense) {
 const {experiment,measured}=active;
 const manifest=JSON.parse(readFileSync(join(source,'hrtf-set.json'),'utf8'));
 const count=dense?experiment.directionCount:17;
 if(manifest.positions.length!==count || manifest.sampleRate!==48000) throw Error('HRTF topology mismatch');
 if(measured) {
  if(experiment.measuredResponseVersion!==2 || experiment.sharedDepthVersion!==1) throw Error("Measured depth selection mismatch");
  if(manifest.sharedRenderProfile!==experiment.id || manifest.subjectId!=='apple-public-hrtfhq'
    || manifest.measuredResponseVersion!==2 || manifest.sharedDepthVersion!==1 || manifest.calibrationVersion!==undefined || manifest.completeSubject!==true
    || manifest.processing?.preserveMeasurements!==true || manifest.processing?.calibrated!==true
    || manifest.processing?.historicalInterpolation!==false || manifest.processing?.spatialCues!==false
    || manifest.processing?.frontEarIsolation!==false || manifest.processing?.mobileDirectOnly!==false)
    throw Error('Measured response policy mismatch');
 } else if(dense && (manifest.iosExperiment!==experiment.id || manifest.processing?.spatialCues!==true)) {
  throw Error('Experimental manifest mismatch');
 }
 if(dense) for(const key of ['historicalInterpolationMode','historicalInterpolation','spatialCueWindow','spatialCues','frontEarIsolation']) {
  if(experiment[key]!==undefined && manifest.processing?.[key]!==experiment[key]) throw Error(`Approved render policy mismatch: ${key}`);
 }
 const safe=name=>typeof name==='string' && basename(name)===name && !name.includes('\\') && !name.includes('..');
 const files=new Set(['hrtf-set.json']);
 const coordinates=new Set(manifest.positions.map(p=>`${p.azimuth}/${p.elevation}`));
 if(coordinates.size!==count) throw Error('Duplicate HRTF directions');
 for(const p of manifest.positions) for(const kind of ['dry','wet']) {
  if(!safe(p[kind]) || p.wet!==p.dry) throw Error('Invalid response path or wet response');
  const data=readFileSync(join(source,p[kind]));
  if(data.length!==4096 || hash(data)!==p.assets?.[kind]?.sha256) throw Error('Response hash mismatch');
  for(let i=0;i<data.length;i+=4) if(!Number.isFinite(data.readFloatLE(i))) throw Error('Non-finite HRTF response');
  files.add(p[kind]);
 }
 if(dense && count===542) {
  const horizontal=JSON.parse(readFileSync(join(root,'apps/mobile/ios-experiments/horizontal-support/hrtf-dense/hrtf-set.json'),'utf8'));
  if(horizontal.positions.length!==218 || horizontal.positions.some(p=>!coordinates.has(`${p.azimuth}/${p.elevation}`)))
   throw Error('Combined renderer must include all218 directions');
  if(manifest.triangleSupportFile!=='triangle-grid.json') throw Error('Combined triangle algorithm not enabled');
  const data=readFileSync(join(source,'triangle-grid.json'));
  if(hash(data)!==experiment.triangleSupportSha256) throw Error('Triangle grid hash mismatch');
  const grid=JSON.parse(data);
  if(JSON.stringify(grid.positions)!==JSON.stringify(manifest.positions.map(({azimuth,elevation})=>({azimuth,elevation})))) throw Error('Triangle order mismatch');
  files.add('triangle-grid.json');
 }
 return files;
}
export function stageSharedExperiment(destination) {
 const active=activeExperiment();if(!active)return null;
 // Validate both families before copying, including all response hashes.
 const families=[['hrtf-dense',active.source,true],['hrtf',active.fallbackSource,false]];
 const validated=families.map(([name,source,dense])=>[name,source,validateFamily(source,active,dense)]);
 for(const [name,source,files] of validated) {
  mkdirSync(join(destination,name),{recursive:true});
  for(const file of files) copyFileSync(join(source,file),join(destination,name,file));
 }
 copyFileSync(join(root,'apps/mobile/ios-experiments/active.json'),join(destination,'render-experiment.json'));
 verifySharedExperiment(destination);return active.experiment;
}
export function verifySharedExperiment(destination) {
 const active=activeExperiment();if(!active)return;
 for(const [name,source,dense] of [['hrtf-dense',active.source,true],['hrtf',active.fallbackSource,false]]) {
  for(const file of validateFamily(source,active,dense))
   if(!readFileSync(join(source,file)).equals(readFileSync(join(destination,name,file)))) throw Error(`Staged profile mismatch: ${name}/${file}`);
 }
 const expected=readFileSync(join(root,'apps/mobile/ios-experiments/active.json'));
 if(!expected.equals(readFileSync(join(destination,'render-experiment.json')))) throw Error('Missing or mismatched rendering profile marker');
}
if(process.argv[1] && resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
 if(!process.argv[2])throw Error('Specify destination HRTF family directory');
 const selected=stageSharedExperiment(resolve(process.argv[2]));
 console.log(selected?`Verified shared rendering profile: ${selected.id}`:'No shared rendering profile selected');
}
