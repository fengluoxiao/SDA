import {existsSync,mkdirSync,writeFileSync} from 'node:fs';
import {resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const args=process.argv.slice(2),option=name=>{const i=args.indexOf(`--${name}`);return i<0?null:args[i+1];};
const runtime={python:option('python'),pythonPath:option('python-path')??'',source:'ideal',hrtf:option('hrtf')??resolve(root,'apps/desktop/native-renderer/room-hrtfhq/hrtf-set.json'),roomReference:option('room-reference')??resolve(root,'apps/desktop/native-renderer',process.platform==='win32'?'SdaRoomReference.exe':'SdaRoomReference'),liveHrtf:option('live-hrtf')??resolve(root,'apps/desktop/native-renderer/hrtf-assets/hrtf-dense/hrtf-set.json'),script:resolve(root,'scripts/room-simulator.py')};
for(const key of ['python','pythonPath','source','hrtf','roomReference','liveHrtf']){
  if((key==='source'&&runtime[key]==='ideal')||(key==='pythonPath'&&runtime[key]===''))continue;
  if(!runtime[key]||!existsSync(runtime[key]))throw new Error(`Provide --${key==='pythonPath'?'python-path':key} pointing to an existing path`);
  runtime[key]=resolve(runtime[key]);
}
const destination=resolve(root,'apps/desktop/room-simulator/runtime.json');
mkdirSync(dirname(destination),{recursive:true});writeFileSync(destination,JSON.stringify(runtime,null,2)+'\n');
console.log(destination);
