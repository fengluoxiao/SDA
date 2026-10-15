import { existsSync, readFileSync, mkdirSync, copyFileSync, cpSync } from 'node:fs';
import { resolve, join, dirname, basename } from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
export function activeExperiment() {
  const path=join(root,'apps/mobile/ios-experiments/active.json');
  if(!existsSync(path)) return null;
  const experiment=JSON.parse(readFileSync(path,'utf8'));
  const supported={'horizontal-support-218-v1':[218,'horizontal-support'],
    'fullheight-triangle-commonphase-542-v1':[542,'fullheight-combined']};
  const config=supported[experiment.id];
  if(!config || experiment.directionCount!==config[0]) throw Error('Unknown shared rendering experiment');
  return {experiment,source:join(root,'apps/mobile/ios-experiments',config[1],'hrtf-dense')};
}
const hash=b=>createHash('sha256').update(b).digest('hex');
export function stageSharedExperiment(destination) {
  const active=activeExperiment();if(!active)return null;
  const {experiment,source}=active;
  const manifest=JSON.parse(readFileSync(join(source,'hrtf-set.json'),'utf8'));
  if(manifest.positions.length!==experiment.directionCount || manifest.iosExperiment!==experiment.id
    || manifest.processing?.spatialCues!==true) throw Error('Experimental manifest mismatch');
  const safe=name=>typeof name==='string' && basename(name)===name && !name.includes('\\') && !name.includes('..');
  const files=new Set(['hrtf-set.json']);
  for(const p of manifest.positions)for(const kind of ['dry','wet']) {
    if(!safe(p[kind]))throw Error('Invalid response path');
    const data=readFileSync(join(source,p[kind]));
    if(data.length!==4096 || hash(data)!==p.assets[kind].sha256)throw Error('Response hash mismatch');
    files.add(p[kind]);
  }
  if(experiment.directionCount===542) {
    if(manifest.triangleSupportFile!=='triangle-grid.json')throw Error('Combined triangle algorithm not enabled');
    const data=readFileSync(join(source,'triangle-grid.json'));
    if(hash(data)!==experiment.triangleSupportSha256)throw Error('Triangle grid hash mismatch');
    const grid=JSON.parse(data);
    if(JSON.stringify(grid.positions)!==JSON.stringify(manifest.positions.map(({azimuth,elevation})=>({azimuth,elevation}))))throw Error('Triangle order mismatch');
    files.add('triangle-grid.json');
  }
  mkdirSync(join(destination,'hrtf-dense'),{recursive:true});
  cpSync(join(root,'apps/mobile/assets/hrtf-restored/hrtf'),join(destination,'hrtf'),{recursive:true});
  for(const file of files)copyFileSync(join(source,file),join(destination,'hrtf-dense',file));
  copyFileSync(join(root,'apps/mobile/ios-experiments/active.json'),join(destination,'render-experiment.json'));
  verifySharedExperiment(destination);
  return experiment;
}
export function verifySharedExperiment(destination) {
  const active=activeExperiment();if(!active)return;
  const {source,experiment}=active;
  const manifest=JSON.parse(readFileSync(join(source,'hrtf-set.json'),'utf8'));
  const files=new Set(['hrtf-set.json',...manifest.positions.flatMap(p=>[p.dry,p.wet]),...(manifest.triangleSupportFile?[manifest.triangleSupportFile]:[])]);
  for(const file of files)if(!readFileSync(join(source,file)).equals(readFileSync(join(destination,'hrtf-dense',file))))throw Error(`Staged experiment mismatch: ${file}`);
  const fallbackRoot=join(root,'apps/mobile/assets/hrtf-restored/hrtf');
  const fallback=JSON.parse(readFileSync(join(fallbackRoot,'hrtf-set.json'),'utf8'));
  for(const file of new Set(['hrtf-set.json',...fallback.positions.flatMap(p=>[p.dry,p.wet])])) {
    if(!readFileSync(join(fallbackRoot,file)).equals(readFileSync(join(destination,'hrtf',file))))throw Error(`Fallback mismatch: ${file}`);
  }
  if(JSON.parse(readFileSync(join(destination,'render-experiment.json'),'utf8')).id!==experiment.id)throw Error('Missing experiment marker');
}
if(process.argv[1] && resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  if(!process.argv[2])throw Error('Specify destination HRTF family directory');
  const selected=stageSharedExperiment(resolve(process.argv[2]));
  console.log(selected?`Verified shared experiment ${selected.id}`:'No shared rendering experiment selected');
}
