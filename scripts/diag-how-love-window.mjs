// Local object/metadata audit for the user-supplied 55-63 s passage. No production DSP changes.
import {build} from 'esbuild';
import {readFileSync,mkdirSync,existsSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL,fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
mkdirSync('tmp/how-love-diag',{recursive:true});
await build({entryPoints:['packages/player/src/decoder.worker.ts'],bundle:true,platform:'node',format:'esm',outfile:'tmp/how-love-diag/worker.mjs',plugins:[{name:'wasm-path',setup(b){b.onResolve({filter:/\.wasm\?url$/},a=>({path:resolve(a.resolveDir,a.path.replace('?url','')),namespace:'wasm-path'}));b.onLoad({filter:/.*/,namespace:'wasm-path'},a=>({contents:`export default ${JSON.stringify(a.path)}`}));}}]});
const nativeFetch=globalThis.fetch;
globalThis.fetch=async (url,...args)=>{const path=String(url).startsWith('file:')?fileURLToPath(url):String(url);return existsSync(path)?new Response(readFileSync(path),{headers:{'Content-Type':'application/wasm'}}):nativeFetch(url,...args);};
let output=[],pending;
globalThis.self={postMessage(message){output.push(message);if(pending&&(message.type===pending.type||message.type==='error')){const p=pending;pending=null;clearTimeout(p.timer);message.type==='error'?p.reject(Error(message.message)):p.resolve(message);}}};
await import(pathToFileURL(resolve('tmp/how-love-diag/worker.mjs')));
const send=(data,type)=>new Promise((resolve,reject)=>{pending={type,resolve,reject,timer:setTimeout(()=>reject(Error(`worker timed out: ${type}`)),60000)};self.onmessage({data});});
await send({type:'init'},'ready');
console.log('worker ready');
if(!process.argv[2]) throw new Error('Expected input file path');
const bytes=readFileSync(process.argv[2]);
output.length=0;self.onmessage({data:{type:'open',codec:'auto'}});
console.log('opened, pushing',bytes.length,'bytes');
const limit=Math.min(bytes.length, 10<<20);
for(let i=0;i<limit;i+=65537)await send({type:'push',sequence:i,chunk:Uint8Array.from(bytes.subarray(i,Math.min(limit,i+65537))).buffer},'push-ack');
console.log('pushed',limit,'of',bytes.length,'bytes');
await send({type:'flush'},'flushed');
const frames=output.filter(m=>m.type==='frame').map(m=>m.frame);
console.log(`decoded frames: ${frames.length}, rate: ${frames[0]?.sampleRate}, channels: ${frames[0]?.channels.length}`);

const selected=frames.filter(f=>f.samplePos/f.sampleRate<64&&(f.samplePos+f.channels[0].length)/f.sampleRate>54);
// Preserve pre-window metadata: unchanged objects must not lose their position.
const latest=new Map();
for(const f of frames) { if(f.samplePos>=54*48000)break;for(const e of f.events)latest.set(e.id,e); }
writeFileSync('tmp/how-love-diag/initial-events.json',JSON.stringify([...latest.values()],null,2));
writeFileSync('tmp/how-love-diag/frames.jsonl',selected.map(f=>JSON.stringify({start:f.samplePos,samples:f.channels[0].length,labels:f.labels,objectChannels:f.objectChannels,events:f.events})).join('\n')+'\n');
writeFileSync('tmp/how-love-diag/sources.f32',Buffer.concat(selected.flatMap(f=>f.channels.map(c=>Buffer.from(c.buffer,c.byteOffset,c.byteLength)))));
const stats=new Map();
for(const f of selected){for(let c=0;c<f.channels.length;c++){
 const key=f.labels[c];const a=stats.get(key)||{label:key,n:0,energy:0,peak:0,positions:[],gains:[]};
 for(let i=0;i<f.channels[c].length;i++){const t=(f.samplePos+i)/f.sampleRate;if(t<55||t>=63)continue;const x=f.channels[c][i];a.n++;a.energy+=x*x;a.peak=Math.max(a.peak,Math.abs(x));}
 const id=f.objectChannels.find(o=>o.channel===c)?.id ?? (key.startsWith('Obj_')?Number(key.slice(4)):null);
 for(const e of f.events.filter(e=>e.id===id)){if(e.hasPos!==false&&e.pos)a.positions.push(e.pos);if(e.gainDb!==undefined)a.gains.push(e.gainDb);}
 stats.set(key,a);
}}
const result={file:process.argv[2],decodedEnd:frames.at(-1)?.samplePos/48000,firstFrame:frames[0]&&{codec:frames[0].codec,labels:frames[0].labels,rawBedLabels:frames[0].rawBedLabels,objectChannels:frames[0].objectChannels},window:[55,63],channels:[...stats.values()].map(a=>({...a,rmsDb:20*Math.log10(Math.sqrt(a.energy/a.n)),positions:[...new Map(a.positions.map(p=>[JSON.stringify(p),p])).values()],gains:[...new Set(a.gains)]}))};
writeFileSync('tmp/how-love-diag/analysis.json',JSON.stringify(result,null,2));
console.log('Saved analysis and unrendered source PCM: tmp/how-love-diag');
process.exit(0);
