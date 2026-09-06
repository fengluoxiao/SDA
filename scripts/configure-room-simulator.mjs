import {existsSync,mkdirSync,writeFileSync} from 'node:fs';
import {resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const args=process.argv.slice(2),option=name=>{const i=args.indexOf(`--${name}`);return i<0?null:args[i+1];};
const runtime={python:option('python'),pythonPath:option('python-path'),source:option('source'),hrtf:option('hrtf'),script:resolve(root,'scripts/room-simulator.py')};
for(const key of ['python','pythonPath','source','hrtf']){
  if(!runtime[key]||!existsSync(runtime[key]))throw new Error(`Provide --${key==='pythonPath'?'python-path':key} pointing to an existing path`);
  runtime[key]=resolve(runtime[key]);
}
const destination=resolve(root,'apps/desktop/room-simulator/runtime.json');
mkdirSync(dirname(destination),{recursive:true});writeFileSync(destination,JSON.stringify(runtime,null,2)+'\n');
console.log(destination);
