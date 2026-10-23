import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const source=readFileSync('apps/native-renderer/src/spatial_cues.rs','utf8');
assert.ok(source.includes('apply_temporal_reflection_trial(&mut p)'));
const helper=source.slice(source.indexOf('fn apply_temporal_reflection_trial'));
assert.ok(helper.includes('ms<=12.0'));
assert.ok(helper.includes('(ms-12.0)/23.0'));
assert.ok(!/azimuth|elevation|source.id/.test(helper));
for(const dir of ['hrtf-dense','hrtf']) {
 const manifest=JSON.parse(readFileSync('apps/mobile/assets/hrtf-restored/'+dir+'/hrtf-set.json','utf8'));
 assert.equal(manifest.processing.spatialCues,true);
}
console.log('Temporal trial: reflection enabled, direction-independent12-35ms, no front policy.');
