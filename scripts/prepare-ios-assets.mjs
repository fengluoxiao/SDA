import {cpSync,mkdirSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('..',import.meta.url)));
const out=join(root,'apps/mobile/modules/sda-core/ios/Resources');
mkdirSync(out,{recursive:true});
cpSync(join(root,'apps/mobile/assets/hrtf-mobile-direct'),join(out,'hrtf-mobile-direct'),{recursive:true});
cpSync(join(root,'apps/mobile/rendering-presets.json'),join(out,'rendering-presets.json'));
console.log('Staged mobile direct-only 128-direction KU100 HRIR (no room assets)');

cpSync(join(root,'crates/sda-native/tests/fixtures/ios-stereo-tones.m4a'),join(out,'ci-stereo-tones.m4a'));

cpSync(join(root,"packages/core/mpegh/fixtures/motion.mhas"),join(out,"ci-360ra.mhas"));
