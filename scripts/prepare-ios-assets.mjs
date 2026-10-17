import {cpSync,mkdirSync,readFileSync,writeFileSync,existsSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {stageSharedExperiment} from './stage-shared-render-experiment.mjs';
const root=resolve(fileURLToPath(new URL('..',import.meta.url)));
const out=join(root,'apps/mobile/modules/sda-core/ios/Resources');
mkdirSync(out,{recursive:true});
cpSync(join(root,'apps/mobile/assets/hrtf-restored'),join(out,'hrtf-restored'),{recursive:true});
cpSync(join(root,'apps/mobile/rendering-presets.json'),join(out,'rendering-presets.json'));
const experimentPath=join(root,'apps/mobile/ios-experiments/active.json');
if (existsSync(experimentPath)) {
  const experiment=JSON.parse(readFileSync(experimentPath,'utf8'));
  stageSharedExperiment(join(out,'hrtf-restored'));
  cpSync(experimentPath,join(out,'ios-experiment.json'));
  console.log(`iOS TEST BUILD: ${experiment.id} (${experiment.directionCount} directions)`);
}
// Explicit iOS audition configuration; shared measured HRIRs stay identical.
const presets=JSON.parse(readFileSync(join(out,'rendering-presets.json'),'utf8'));
if (presets[0]?.iosDirectReflectionTrial === true) {
  for (const directory of ['hrtf-dense','hrtf']) {
    const path=join(out,'hrtf-restored',directory,'hrtf-set.json');
    const manifest=JSON.parse(readFileSync(path,'utf8'));
    manifest.processing.spatialCues=false;
    writeFileSync(path,JSON.stringify(manifest,null,2)+'\n');
  }
}
console.log('Staged and verified shared HRTF profile and matching speaker fallback');

cpSync(join(root,'crates/sda-native/tests/fixtures/ios-stereo-tones.m4a'),join(out,'ci-stereo-tones.m4a'));

cpSync(join(root,"packages/core/mpegh/fixtures/motion.mhas"),join(out,"ci-360ra.mhas"));

// Preserve decoder and normative-asset notices in the installed iOS bundle.
const vividNotices=join(out,"licenses/avs3a");
mkdirSync(vividNotices,{recursive:true});
cpSync(join(root,"vendor/avs3a/LICENSE"),join(vividNotices,"LICENSE"));
cpSync(join(root,"vendor/avs3a/assets/README.md"),join(vividNotices,"ASSET-NOTICES.md"));
