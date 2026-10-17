import assert from 'node:assert/strict';
import {mkdtempSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {stageSharedExperiment,verifySharedExperiment,activeExperiment} from './stage-shared-render-experiment.mjs';
const temporary=mkdtempSync(join(tmpdir(),'sda-shared-experiment-'));
try {
 const roots=['ios','android','windows','macos'].map(name=>join(temporary,name));
 for(const root of roots) {
  const selected=stageSharedExperiment(root);assert.equal(selected.id,activeExperiment().experiment.id);
  verifySharedExperiment(root);
 }
 for(const name of ['hrtf-dense/hrtf-set.json','hrtf-dense/triangle-grid.json','hrtf/hrtf-set.json']) {
  for(const root of roots.slice(1))assert.deepEqual(readFileSync(join(root,name)),readFileSync(join(roots[0],name)));
 }
 const manifest=JSON.parse(readFileSync(join(roots[0],'hrtf-dense/hrtf-set.json'),'utf8'));
 const horizontal=JSON.parse(readFileSync('apps/mobile/ios-experiments/horizontal-support/hrtf-dense/hrtf-set.json','utf8'));
 const coordinates=new Set(manifest.positions.map(p=>`${p.azimuth}/${p.elevation}`));
 assert.equal(coordinates.size,542);
 for(const p of horizontal.positions)assert(coordinates.has(`${p.azimuth}/${p.elevation}`),'218direction missing from combined renderer');
 assert.equal(manifest.processing.spatialCues,activeExperiment().experiment.spatialCues ?? true);
 if(activeExperiment().measured) {
  assert.equal(manifest.subjectId,"apple-public-hrtfhq");
  const fallback=JSON.parse(readFileSync(join(roots[0],"hrtf/hrtf-set.json"),"utf8"));
  assert.equal(fallback.subjectId,manifest.subjectId);
  assert.equal(fallback.measuredResponseVersion,2);
  assert.equal(fallback.sharedDepthVersion,1);
  assert.equal(manifest.processing.frontEarIsolation,false);
 }
 assert.equal(manifest.triangleSupportFile,'triangle-grid.json');
 writeFileSync(join(roots[0],'hrtf-dense',manifest.positions[0].dry),'broken');
 assert.throws(()=>verifySharedExperiment(roots[0]),/mismatch/);
 const read=p=>readFileSync(p,'utf8');
 assert.match(read('scripts/prepare-ios-assets.mjs'),/stageSharedExperiment/);
 assert.match(read('apps/mobile/android/app/build.gradle'),/stage-shared-render-experiment.mjs/);
 assert.match(read('scripts/build-native-renderer.mjs'),/stageSharedExperiment\(hrtfDestination\)/);
 assert.match(read('apps/desktop/package.json'),/"from": "native-renderer"/);
 assert.match(read('.github/workflows/release.yml'),/Build web \+ package Electron \(unsigned DMG\)[\s\S]*?node scripts\/build-native-renderer.mjs/);
 console.log('Shared experiment:4staged layouts byte-identical, fallback+triangle verified, corruption rejected; packaging entry contracts passed. Not4built artifacts.');
} finally {rmSync(temporary,{recursive:true,force:true});}
