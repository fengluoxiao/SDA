import {cpSync,mkdirSync,readFileSync,writeFileSync,existsSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {gunzipSync} from 'node:zlib';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('..',import.meta.url)));
const out=join(root,'apps/mobile/modules/sda-core/ios/Resources');
mkdirSync(out,{recursive:true});
for(const dir of ['hrtf','hrtf-dense','hrtf-raw','hrtf-dense-raw']) {
 const source=join(root,'apps/web/public',dir);
 if(!existsSync(join(source,'hrtf-set.json'))) throw Error('Missing HRTF: '+dir);
 cpSync(source,join(out,dir),{recursive:true});
}
const rooms=join(root,'apps/desktop/builtin-rooms');
const catalog=JSON.parse(readFileSync(join(rooms,'catalog.json'),'utf8'));
mkdirSync(join(out,'rooms'),{recursive:true});
const sha=b=>createHash('sha256').update(b).digest('hex');
for(const p of catalog.profiles) {
 if(!/^[0-9a-f]{64}$/.test(p.id) || p.file!==p.id+'.json.gz') throw Error('Invalid room path');
 const compressed=readFileSync(join(rooms,p.file));
 if(sha(compressed)!==p.compressedSha256) throw Error('Room compressed hash mismatch');
 const data=gunzipSync(compressed);
 if(data.length!==p.bytes || sha(data)!==p.id) throw Error('Room integrity mismatch');
 writeFileSync(join(out,'rooms',p.id+'.json'),data);
}
cpSync(join(rooms,'catalog.json'),join(out,'rooms/catalog.json'));
for(const file of ['NOTICE.txt','LICENSE-SADIE.txt','MATERIAL-SOURCES.md']) if(existsSync(join(rooms,file))) cpSync(join(rooms,file),join(out,'rooms',file));
cpSync(join(root,'apps/mobile/rendering-presets.json'),join(out,'rendering-presets.json'));
console.log('Staged KU100 assets and '+catalog.profiles.length+' verified room profiles');

cpSync(join(root,'crates/sda-native/tests/fixtures/ios-stereo-tones.m4a'),join(out,'ci-stereo-tones.m4a'));

cpSync(join(root,"packages/core/mpegh/fixtures/motion.mhas"),join(out,"ci-360ra.mhas"));
